//! `blumi web` — the embedded web UI + HTTP/SSE server with live session
//! switch/resume.

use crate::engine::build_session;
use async_trait::async_trait;
use blumi_config::BlumiConfig;
use blumi_core::{SessionHandle, SessionState};
use blumi_persist::Store;
use blumi_protocol::SessionId;
use std::net::SocketAddr;
use std::sync::Arc;

/// Creates / resumes / lists / saves sessions for the web server.
struct WebSessionProvider {
    config: BlumiConfig,
    store: Option<Arc<Store>>,
}

#[async_trait]
impl blumi_web::SessionProvider for WebSessionProvider {
    async fn create(&self) -> anyhow::Result<SessionHandle> {
        // Approvals are handled by the UI's cards, so no yolo.
        build_session(&self.config, false, None).await
    }

    async fn resume(&self, id: &str) -> anyhow::Result<SessionHandle> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("session history is unavailable"))?;
        let stored = store
            .load_session(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("session {id} not found"))?;
        let model = if stored.meta.model.is_empty() {
            self.config.llm.model.clone()
        } else {
            stored.meta.model.clone()
        };
        let mut state = SessionState::new(SessionId::from(stored.meta.id.clone()), model);
        state.messages = stored.messages;
        state.total_input_tokens = stored.meta.input_tokens.max(0) as u32;
        state.total_output_tokens = stored.meta.output_tokens.max(0) as u32;
        build_session(&self.config, false, Some(state)).await
    }

    async fn reload(&self, snapshot: blumi_core::SessionSnapshot) -> anyhow::Result<SessionHandle> {
        // Re-read config from disk so the agent's own `self_config` edits take
        // effect; fall back to the startup config if it can't be loaded.
        let config = BlumiConfig::load(
            &self.config.paths.working_dir,
            Some(self.config.paths.home.clone()),
        )
        .unwrap_or_else(|_| self.config.clone());

        // Seed from the live snapshot so the conversation is preserved; skills
        // are re-scanned inside build_session.
        let mut state = SessionState::new(snapshot.id, snapshot.model);
        state.messages = snapshot.messages;
        state.todos = snapshot.todos;
        state.total_input_tokens = snapshot.total_input_tokens;
        state.total_output_tokens = snapshot.total_output_tokens;
        state.turn_count = snapshot.turn_count;

        build_session(&config, false, Some(state)).await
    }

    async fn list(&self) -> Vec<blumi_web::SessionInfo> {
        match &self.store {
            Some(store) => store
                .list_sessions(50)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| blumi_web::SessionInfo {
                    id: m.id,
                    title: m.title,
                    model: m.model,
                    message_count: m.message_count,
                })
                .collect(),
            None => Vec::new(),
        }
    }

    async fn save(&self, handle: &SessionHandle) {
        if let Some(store) = &self.store {
            if let Err(e) = store.save_snapshot(&handle.snapshot().await).await {
                tracing::warn!("could not save session: {e}");
            }
        }
    }
}

pub async fn run(
    config: BlumiConfig,
    host: Option<String>,
    password: Option<String>,
) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();

    let port: u16 = std::env::var("BLUMI_WEB_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7777);

    // Resolve the bind address (default loopback).
    let host = host.unwrap_or_else(|| "127.0.0.1".to_string());
    let ip: std::net::IpAddr = host
        .parse()
        .map_err(|_| anyhow::anyhow!("--host must be an IP address, got '{host}'"))?;
    let addr = SocketAddr::new(ip, port);

    // Resolve auth: --password (hashed + persisted) overrides the stored hash.
    let mut password_hash = config.web.password_hash.clone();
    if let Some(pw) = password {
        let hash = blumi_web::Auth::hash_password(&pw)?;
        persist_password_hash(&config.paths.settings_json(), &hash)?;
        eprintln!(
            "  password saved to {}",
            config.paths.settings_json().display()
        );
        password_hash = hash;
    }
    let auth = if password_hash.trim().is_empty() {
        None
    } else {
        let key = load_or_create_key(&config.paths.home.join("web_key"))?;
        Some(blumi_web::Auth::new(password_hash, key))
    };

    // Refuse to expose blumi on the network without a password.
    if !ip.is_loopback() && auth.is_none() {
        anyhow::bail!(
            "binding to {host} would expose blumi on the network — set a password first:\n  \
             blumi web --host {host} --password <password>"
        );
    }

    let url = format!("http://{addr}");
    let store = Store::open(&config.paths.db).await.ok().map(Arc::new);

    let personas = crate::engine::resolve_personas(&config)
        .into_iter()
        .map(|p| (p.name, p.description))
        .collect();

    let web = blumi_web::WebConfig {
        model: config.llm.model.clone(),
        models: vec![config.llm.model.clone()],
        working_dir: config.paths.working_dir.display().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        personas,
        persona: crate::engine::active_persona_name(&config),
        context_size: config.llm.context_size,
    };

    let provider = Arc::new(WebSessionProvider { config, store });

    // Discovery lock file (analog of OpenMono's ACP lock writer) so other tools
    // can find the running server.
    let lock = provider.config.paths.home.join("web.lock");
    let _ = std::fs::write(
        &lock,
        format!("{{\"url\":\"{url}\",\"pid\":{}}}", std::process::id()),
    );

    crate::branding::banner();
    eprintln!(
        "  blumi web → {url}  ({})  (Ctrl+C to stop)",
        if auth.is_some() {
            "login required"
        } else {
            "no auth — loopback only"
        }
    );
    // Only auto-open the browser for a local, no-auth server.
    if ip.is_loopback() && auth.is_none() && std::env::var_os("BLUMI_NO_BROWSER").is_none() {
        open_browser(&url);
    }

    let result = blumi_web::serve(provider, web, addr, auth).await;
    let _ = std::fs::remove_file(&lock);
    result
}

/// Load the 32-byte cookie-signing key, creating it (0600) on first use.
fn load_or_create_key(path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
    if let Ok(bytes) = std::fs::read(path) {
        if bytes.len() >= 32 {
            return Ok(bytes);
        }
    }
    use rand::RngCore;
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, &key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(key)
}

/// Merge `web.password_hash` into settings.json (atomic, 0600).
fn persist_password_hash(path: &std::path::Path, hash: &str) -> anyhow::Result<()> {
    let mut root: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    root["web"]["password_hash"] = serde_json::Value::String(hash.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let body = serde_json::to_string_pretty(&root)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Best-effort: open the default browser at `url`.
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = ("open", vec![url]);
    #[cfg(target_os = "linux")]
    let cmd = ("xdg-open", vec![url]);
    #[cfg(target_os = "windows")]
    let cmd = ("cmd", vec!["/C", "start", url]);

    let _ = std::process::Command::new(cmd.0)
        .args(cmd.1)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

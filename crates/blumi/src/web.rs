//! `blumi web` — the embedded web UI + HTTP/SSE server with live session
//! switch/resume.

use crate::engine::build_session;
use async_trait::async_trait;
use blumi_config::BlumiConfig;
use blumi_core::{SessionHandle, SessionState};
use blumi_cron::CronStore;
use blumi_persist::Store;
use blumi_protocol::SessionId;
use blumi_skills::SkillCatalog;
use blumi_web::{
    BrainView, CronJobInfo, GatewayView, Management, ModelOptions, ModelUsage, ProviderOption,
    SettingsPatch, SettingsView, SkillInfo, UsageStats, VoiceView,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use time::OffsetDateTime;

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

/// Control-center data + actions (cron / skills / memory / usage) over the cron
/// store, skill catalog, memory files, and the persistence store.
struct WebManagement {
    config: BlumiConfig,
    store: Option<Arc<Store>>,
    /// Grid membership + live peer registry, when the grid is enabled.
    grid: Option<crate::grid::GridShared>,
}

impl WebManagement {
    fn cron_path(&self) -> std::path::PathBuf {
        self.config.paths.home.join("cron.json")
    }
    fn skill_dirs(&self) -> [std::path::PathBuf; 2] {
        [
            self.config.paths.skills.clone(),
            self.config.paths.working_dir.join(".blumi").join("skills"),
        ]
    }
}

#[async_trait]
impl Management for WebManagement {
    async fn cron_list(&self) -> Vec<CronJobInfo> {
        CronStore::load(self.cron_path())
            .jobs()
            .iter()
            .map(|j| CronJobInfo {
                id: j.id.clone(),
                name: j.name.clone(),
                schedule: j.schedule.clone(),
                prompt: j.prompt.clone(),
            })
            .collect()
    }

    async fn cron_add(&self, name: &str, schedule: &str, prompt: &str) -> anyhow::Result<()> {
        let mut store = CronStore::load(self.cron_path());
        store
            .add(name, schedule, prompt, "log", OffsetDateTime::now_utc())
            .map_err(|e| anyhow::anyhow!("invalid schedule: {e}"))?;
        store.save()?;
        Ok(())
    }

    async fn cron_remove(&self, id: &str) -> anyhow::Result<()> {
        let mut store = CronStore::load(self.cron_path());
        if store.remove(id) {
            store.save()?;
            Ok(())
        } else {
            anyhow::bail!("no cron job '{id}'")
        }
    }

    fn skills(&self) -> Vec<SkillInfo> {
        let cat = SkillCatalog::load(&self.skill_dirs());
        cat.list()
            .into_iter()
            .filter_map(|m| {
                cat.get(&m.name).map(|s| SkillInfo {
                    name: s.name.clone(),
                    description: s.description.clone(),
                    body: s.body.clone(),
                })
            })
            .collect()
    }

    fn tasks(&self) -> serde_json::Value {
        let board = blumi_task::TaskBoard::load(crate::task::board_path(&self.config));
        serde_json::json!({ "tasks": board.tasks(), "counts": board.counts() })
    }

    fn task_next(&self) -> Option<serde_json::Value> {
        use blumi_task::{TaskBoard, TaskState};
        let path = crate::task::board_path(&self.config);
        let mut board = TaskBoard::load(&path);
        let task = board.next_todo().cloned()?;
        board.set_state_now(&task.id, TaskState::Doing);
        board.save().ok();
        let prompt = if task.detail.trim().is_empty() {
            task.title.clone()
        } else {
            format!("{}\n\n{}", task.title, task.detail)
        };
        Some(serde_json::json!({ "id": task.id, "prompt": prompt, "title": task.title }))
    }

    fn task_advance(&self, id: &str, review: bool) {
        use blumi_task::{TaskBoard, TaskState};
        let path = crate::task::board_path(&self.config);
        let mut board = TaskBoard::load(&path);
        let to = if review {
            TaskState::Review
        } else {
            TaskState::Done
        };
        board.set_state_now(id, to);
        board.save().ok();
    }

    fn grid_peers(&self) -> serde_json::Value {
        match &self.grid {
            Some(g) => g.peers_json(env!("CARGO_PKG_VERSION")),
            None => serde_json::json!({ "enabled": false, "peers": [] }),
        }
    }

    fn grid_peer_ids(&self) -> Vec<String> {
        match &self.grid {
            Some(g) => g.registry.live().into_iter().map(|p| p.id).collect(),
            None => Vec::new(),
        }
    }

    fn task_peek_next(&self) -> Option<serde_json::Value> {
        let board = blumi_task::TaskBoard::load(crate::task::board_path(&self.config));
        let task = board.next_todo()?;
        let prompt = if task.detail.trim().is_empty() {
            task.title.clone()
        } else {
            format!("{}\n\n{}", task.title, task.detail)
        };
        Some(serde_json::json!({ "id": task.id, "prompt": prompt, "title": task.title }))
    }

    async fn grid_dispatch(&self, task_id: &str, peer_id: &str, review: bool) -> serde_json::Value {
        use blumi_task::{TaskBoard, TaskState};
        let Some(grid) = &self.grid else {
            return serde_json::json!({ "ok": false, "error": "grid disabled" });
        };
        let secret = self.config.grid.secret.clone();
        if secret.trim().is_empty() {
            return serde_json::json!({ "ok": false, "error": "no grid secret" });
        }
        let Some(peer) = grid.registry.get(peer_id) else {
            return serde_json::json!({ "ok": false, "error": "peer offline" });
        };

        // Claim: mark doing + owner, and build the prompt from the task.
        let path = crate::task::board_path(&self.config);
        let mut board = TaskBoard::load(&path);
        let Some(task) = board.tasks().iter().find(|t| t.id == task_id).cloned() else {
            return serde_json::json!({ "ok": false, "error": "task not found" });
        };
        board.set_state_now(task_id, TaskState::Doing);
        board.set_owner(task_id, Some(peer.name.clone()));
        board.save().ok();
        let prompt = if task.detail.trim().is_empty() {
            task.title.clone()
        } else {
            format!("{}\n\n{}", task.title, task.detail)
        };

        // Run on the peer's own runtime, then advance (done/review) on success or
        // release (→ todo, clear owner) on failure.
        let client = crate::grid::client::Client::for_peer(&peer, &secret);
        let res = client
            .run_task(prompt, std::time::Duration::from_secs(900))
            .await;
        let mut board = TaskBoard::load(&path);
        match res {
            Ok(summary) => {
                let to = if review {
                    TaskState::Review
                } else {
                    TaskState::Done
                };
                board.set_state_now(task_id, to);
                // Keep `owner` so the UI shows which peer ran it.
                board.save().ok();
                serde_json::json!({ "ok": true, "peer": peer.name, "summary": summary })
            }
            Err(e) => {
                board.set_state_now(task_id, TaskState::Todo);
                board.set_owner(task_id, None);
                board.save().ok();
                serde_json::json!({
                    "ok": false, "peer": peer.name, "error": e.to_string(), "released": true
                })
            }
        }
    }

    fn memory(&self) -> (String, String) {
        let mem = std::fs::read_to_string(self.config.paths.memory_md()).unwrap_or_default();
        let usr = std::fs::read_to_string(self.config.paths.user_md()).unwrap_or_default();
        (mem, usr)
    }

    fn memory_set(&self, which: &str, content: &str) -> anyhow::Result<()> {
        let path = if which == "user" {
            self.config.paths.user_md()
        } else {
            self.config.paths.memory_md()
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    async fn usage(&self) -> UsageStats {
        let Some(store) = &self.store else {
            return UsageStats::default();
        };
        let metas = store.list_sessions(1000).await.unwrap_or_default();
        let mut stats = UsageStats::default();
        let mut by: BTreeMap<String, ModelUsage> = BTreeMap::new();
        for m in &metas {
            stats.sessions += 1;
            stats.messages += m.message_count.max(0) as u64;
            stats.input_tokens += m.input_tokens.max(0) as u64;
            stats.output_tokens += m.output_tokens.max(0) as u64;
            let model = if m.model.is_empty() {
                "default".to_string()
            } else {
                m.model.clone()
            };
            let e = by.entry(model.clone()).or_insert_with(|| ModelUsage {
                model,
                sessions: 0,
                input_tokens: 0,
                output_tokens: 0,
            });
            e.sessions += 1;
            e.input_tokens += m.input_tokens.max(0) as u64;
            e.output_tokens += m.output_tokens.max(0) as u64;
        }
        stats.by_model = by.into_values().collect();
        stats
    }

    fn voice_config(&self) -> Option<blumi_voice::VoiceConfig> {
        let c = self.fresh_config();
        c.voice.enabled.then(|| to_voice_config(&c))
    }

    fn settings_view(&self) -> SettingsView {
        let c = self.fresh_config();
        let v = &c.voice;
        let g = &c.gateway;
        SettingsView {
            brain: BrainView {
                mode: blumi_core::BrainMode::parse(&c.brain.mode)
                    .unwrap_or_default()
                    .label()
                    .to_string(),
                provider: c.brain.provider.clone(),
                model: c.brain.model.clone(),
            },
            voice: VoiceView {
                enabled: v.enabled,
                stt_base_url: v.stt_base_url.clone(),
                stt_model: v.stt_model.clone(),
                tts_provider: v.tts_provider.clone(),
                tts_base_url: v.tts_base_url.clone(),
                tts_model: v.tts_model.clone(),
                tts_voice: v.tts_voice.clone(),
                api_key_set: !v.api_key.trim().is_empty(),
                tts_api_key_set: !v.tts_api_key.trim().is_empty(),
            },
            gateway: GatewayView {
                yolo: g.yolo,
                telegram_token_set: !g.telegram.token.trim().is_empty(),
                discord_token_set: !g.discord.token.trim().is_empty(),
                slack_bot_token_set: !g.slack.bot_token.trim().is_empty(),
                slack_app_token_set: !g.slack.app_token.trim().is_empty(),
                whatsapp_token_set: !g.whatsapp.token.trim().is_empty(),
                whatsapp_phone_number_id: g.whatsapp.phone_number_id.clone(),
                whatsapp_verify_token: g.whatsapp.verify_token.clone(),
            },
        }
    }

    fn settings_apply(&self, p: SettingsPatch) -> anyhow::Result<()> {
        merge_settings_json(&self.config.paths.settings_json(), |root| {
            // Brain (local-LLM approvals). Only accept a valid mode.
            if let Some(m) = p
                .brain_mode
                .as_deref()
                .and_then(blumi_core::BrainMode::parse)
            {
                set_path(root, &["brain", "mode"], json!(m.label()));
            }
            set_str(root, &["brain", "provider"], p.brain_provider);
            set_str(root, &["brain", "model"], p.brain_model);
            if let Some(b) = p.voice_enabled {
                set_path(root, &["voice", "enabled"], json!(b));
            }
            set_str(root, &["voice", "stt_base_url"], p.stt_base_url);
            set_str(root, &["voice", "stt_model"], p.stt_model);
            set_str(root, &["voice", "tts_provider"], p.tts_provider);
            set_str(root, &["voice", "tts_base_url"], p.tts_base_url);
            set_str(root, &["voice", "tts_model"], p.tts_model);
            set_str(root, &["voice", "tts_voice"], p.tts_voice);
            set_secret(root, &["voice", "api_key"], p.voice_api_key);
            set_secret(root, &["voice", "tts_api_key"], p.tts_api_key);
            if let Some(b) = p.gateway_yolo {
                set_path(root, &["gateway", "yolo"], json!(b));
            }
            set_secret(root, &["gateway", "telegram", "token"], p.telegram_token);
            set_secret(root, &["gateway", "discord", "token"], p.discord_token);
            set_secret(root, &["gateway", "slack", "bot_token"], p.slack_bot_token);
            set_secret(root, &["gateway", "slack", "app_token"], p.slack_app_token);
            set_secret(root, &["gateway", "whatsapp", "token"], p.whatsapp_token);
            set_str(
                root,
                &["gateway", "whatsapp", "phone_number_id"],
                p.whatsapp_phone_number_id,
            );
            set_str(
                root,
                &["gateway", "whatsapp", "verify_token"],
                p.whatsapp_verify_token,
            );
        })
    }

    fn model_options(&self) -> ModelOptions {
        let (provider, model, models, providers) = crate::providers::options(&self.fresh_config());
        ModelOptions {
            provider,
            model,
            models,
            providers: providers
                .into_iter()
                .map(|(name, label, ready)| ProviderOption { name, label, ready })
                .collect(),
        }
    }

    fn set_provider(&self, provider: &str, api_key: Option<&str>) -> anyhow::Result<()> {
        if !self.fresh_config().providers.contains_key(provider) {
            anyhow::bail!("unknown provider '{provider}'");
        }
        crate::providers::persist_provider(&self.config.paths.settings_json(), provider, api_key)
    }
}

impl WebManagement {
    /// Re-read config from disk so edits made via the control center take effect
    /// without a restart.
    fn fresh_config(&self) -> BlumiConfig {
        BlumiConfig::load(
            &self.config.paths.working_dir,
            Some(self.config.paths.home.clone()),
        )
        .unwrap_or_else(|_| self.config.clone())
    }
}

/// Set a nested JSON path, creating intermediate objects.
fn set_path(root: &mut Value, path: &[&str], val: Value) {
    let mut cur = root;
    for key in &path[..path.len() - 1] {
        if !cur[*key].is_object() {
            cur[*key] = json!({});
        }
        cur = &mut cur[*key];
    }
    cur[path[path.len() - 1]] = val;
}

/// Set a string field when provided (non-secret — empty is allowed).
fn set_str(root: &mut Value, path: &[&str], v: Option<String>) {
    if let Some(s) = v {
        set_path(root, path, json!(s));
    }
}

/// Set a secret field only when a non-empty value is provided (blank = keep).
fn set_secret(root: &mut Value, path: &[&str], v: Option<String>) {
    if let Some(s) = v {
        if !s.trim().is_empty() {
            set_path(root, path, json!(s));
        }
    }
}

/// Read settings.json (or `{}`), apply `edit`, write back atomically (0600).
fn merge_settings_json(
    path: &std::path::Path,
    edit: impl FnOnce(&mut Value),
) -> anyhow::Result<()> {
    let mut root: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    edit(&mut root);
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

pub async fn run(
    config: BlumiConfig,
    host: Option<String>,
    password: Option<String>,
    port: Option<u16>,
) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();

    let port: u16 = port
        .or_else(|| {
            std::env::var("BLUMI_WEB_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
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

    // Grid: derive our public grid_id (None when disabled) and, if enabled, a
    // peer registry that the mDNS browser (spawned after we advertise) feeds.
    let grid_id = crate::grid::grid_id(&config.grid);
    let grid_registry = grid_id.as_ref().map(|_| crate::grid::PeerRegistry::new());
    // The shared grid secret authenticates peer→peer execution (None = disabled).
    let grid_secret = grid_id.as_ref().map(|_| config.grid.secret.clone());
    // Recover orphaned grid work: tasks left "doing" with an owner (a peer was
    // mid-execution when this orchestrator last stopped) go back to "todo".
    {
        use blumi_task::{TaskBoard, TaskState};
        let mut board = TaskBoard::load(crate::task::board_path(&config));
        let orphans: Vec<String> = board
            .tasks()
            .iter()
            .filter(|t| t.state == TaskState::Doing && t.owner.is_some())
            .map(|t| t.id.clone())
            .collect();
        if !orphans.is_empty() {
            for id in &orphans {
                board.set_state_now(id, TaskState::Todo);
                board.set_owner(id, None);
            }
            board.save().ok();
        }
    }
    let grid_shared = match (&grid_id, &grid_registry) {
        (Some(gid), Some(reg)) => Some(crate::grid::GridShared {
            grid_id: gid.clone(),
            node_name: if config.grid.node_name.trim().is_empty() {
                whoami::fallible::hostname().unwrap_or_else(|_| "blumi".to_string())
            } else {
                config.grid.node_name.clone()
            },
            registry: reg.clone(),
        }),
        _ => None,
    };

    let mgmt = Arc::new(WebManagement {
        config: config.clone(),
        store: store.clone(),
        grid: grid_shared,
    });

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

    // Advertise on the LAN over mDNS so blugo auto-discovers this gateway. Held
    // for the server's lifetime; unregisters on drop. Best-effort (no-op on
    // loopback / failure).
    let _beacon = crate::discovery::advertise(ip, addr.port(), auth.is_some(), grid_id.as_deref());

    // Grid: browse for same-grid peers on a dedicated thread (mdns-sd's browse
    // Receiver is blocking), feeding the shared registry. Excludes our own
    // advertisement by mDNS fullname. Runs for the process lifetime.
    if let (Some(gid), Some(reg)) = (grid_id, grid_registry) {
        let self_id = _beacon.as_ref().map(|b| b.fullname().to_string());
        std::thread::spawn(move || {
            crate::grid::browse_into(
                gid,
                self_id,
                reg,
                tokio_util::sync::CancellationToken::new(),
            );
        });
    }

    let result = blumi_web::serve(provider, mgmt, web, addr, auth, grid_secret).await;
    let _ = std::fs::remove_file(&lock);
    result
}

/// Map the config's voice section to a `blumi_voice::VoiceConfig`. Shared by the
/// web server and the messaging gateways.
pub(crate) fn to_voice_config(config: &BlumiConfig) -> blumi_voice::VoiceConfig {
    blumi_voice::VoiceConfig {
        api_key: config.voice.api_key.clone(),
        stt_base_url: config.voice.stt_base_url.clone(),
        stt_model: config.voice.stt_model.clone(),
        tts_provider: config.voice.tts_provider.clone(),
        tts_base_url: config.voice.tts_base_url.clone(),
        tts_model: config.voice.tts_model.clone(),
        tts_voice: config.voice.tts_voice.clone(),
        tts_api_key: config.voice.tts_api_key.clone(),
    }
}

/// Load the 32-byte cookie-signing key, creating it (0600) on first use.
pub(crate) fn load_or_create_key(path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
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
pub(crate) fn persist_password_hash(path: &std::path::Path, hash: &str) -> anyhow::Result<()> {
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

//! `blumi web` — the embedded web UI + HTTP/SSE server over a live session.

use crate::engine::build_session;
use blumi_config::BlumiConfig;
use std::net::SocketAddr;
use std::sync::Arc;

pub async fn run(config: BlumiConfig) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();

    let port: u16 = std::env::var("BLUMI_WEB_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7777);
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let url = format!("http://{addr}");

    // Approvals are handled by the UI's cards, so the server runs without yolo.
    let session = build_session(&config, false).await?;
    let store = blumi_persist::Store::open(&config.paths.db)
        .await
        .ok()
        .map(Arc::new);

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
    };

    // Discovery lock file (analog of OpenMono's ACP lock writer) so other tools
    // can find the running server.
    let lock = config.paths.home.join("web.lock");
    let _ = std::fs::write(
        &lock,
        format!("{{\"url\":\"{url}\",\"pid\":{}}}", std::process::id()),
    );

    crate::branding::banner();
    eprintln!("  blumi web → {url}  (Ctrl+C to stop)");
    if std::env::var_os("BLUMI_NO_BROWSER").is_none() {
        open_browser(&url);
    }

    let result = blumi_web::serve(session, store, web, addr).await;
    let _ = std::fs::remove_file(&lock);
    result
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

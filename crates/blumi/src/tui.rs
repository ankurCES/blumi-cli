//! `blumi tui` — interactive terminal UI over a live session.

use crate::engine::build_session;
use blumi_config::BlumiConfig;

pub async fn run(config: BlumiConfig) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();

    // Interactive: approvals are handled by the TUI dialog, so no yolo.
    let session = build_session(&config, false).await?;
    let persist = session.clone();

    let model_name = config.llm.model.clone();
    let working_dir = config.paths.working_dir.display().to_string();

    blumi_tui::run(session, model_name, working_dir).await?;

    // Persist the session on exit (best-effort).
    if let Ok(store) = blumi_persist::Store::open(&config.paths.db).await {
        let snapshot = persist.snapshot().await;
        if let Err(e) = store.save_snapshot(&snapshot).await {
            tracing::warn!("could not save session: {e}");
        }
    }
    Ok(())
}

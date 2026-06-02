//! `blumi tui` — interactive terminal UI over a live session.

use crate::engine::build_session;
use blumi_config::BlumiConfig;

pub async fn run(config: BlumiConfig) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();

    // Interactive: approvals are handled by the TUI dialog, so no yolo.
    let session = build_session(&config, false).await?;
    let persist = session.clone();

    // Skills listing for the `/skills` command + dashboard.
    let skills = blumi_skills::SkillCatalog::load(&[
        config.paths.skills.clone(),
        config.paths.working_dir.join(".blumi").join("skills"),
    ])
    .list()
    .into_iter()
    .map(|m| (m.name, m.description))
    .collect();

    let cfg = blumi_tui::TuiConfig {
        model_name: config.llm.model.clone(),
        working_dir: config.paths.working_dir.display().to_string(),
        memory_md: config.paths.memory_md(),
        user_md: config.paths.user_md(),
        skills,
    };

    blumi_tui::run(session, cfg).await?;

    // Persist the session on exit (best-effort).
    if let Ok(store) = blumi_persist::Store::open(&config.paths.db).await {
        let snapshot = persist.snapshot().await;
        if let Err(e) = store.save_snapshot(&snapshot).await {
            tracing::warn!("could not save session: {e}");
        }
    }
    Ok(())
}

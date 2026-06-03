//! `blumi tui` — interactive terminal UI with live session switch/resume.

use crate::engine::build_session;
use async_trait::async_trait;
use blumi_config::BlumiConfig;
use blumi_core::{SessionHandle, SessionState};
use blumi_persist::Store;
use blumi_protocol::SessionId;
use std::sync::Arc;

/// Creates / resumes / lists / saves sessions for the TUI, over the engine +
/// the persistence store.
struct TuiSessionFactory {
    config: BlumiConfig,
    store: Option<Arc<Store>>,
}

#[async_trait]
impl blumi_tui::SessionFactory for TuiSessionFactory {
    async fn create(&self) -> anyhow::Result<SessionHandle> {
        // Interactive: approvals handled by the TUI dialog, so no yolo.
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
        // Re-read config from disk so the agent's own `self_config` edits (and
        // any other changes to settings.json) take effect on reload. Fall back
        // to the startup config if the file can't be loaded.
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

    async fn list(&self) -> Vec<(String, String)> {
        match &self.store {
            Some(store) => store
                .list_sessions(12)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| (m.id, m.title))
                .collect(),
            None => Vec::new(),
        }
    }

    async fn save(&self, handle: &SessionHandle) {
        if let Some(store) = &self.store {
            let snapshot = handle.snapshot().await;
            if let Err(e) = store.save_snapshot(&snapshot).await {
                tracing::warn!("could not save session: {e}");
            }
        }
    }

    fn model_options(&self) -> blumi_tui::ModelOptions {
        let (provider, model, models, providers) = crate::providers::options(&self.fresh_config());
        blumi_tui::ModelOptions {
            provider,
            model,
            models,
            providers: providers
                .into_iter()
                .map(|(name, label, ready)| blumi_tui::ProviderOpt { name, label, ready })
                .collect(),
        }
    }

    async fn set_provider(&self, provider: &str, api_key: Option<String>) -> anyhow::Result<()> {
        if !self.fresh_config().providers.contains_key(provider) {
            anyhow::bail!("unknown provider '{provider}'");
        }
        crate::providers::persist_provider(
            &self.config.paths.settings_json(),
            provider,
            api_key.as_deref(),
        )
    }
}

impl TuiSessionFactory {
    /// Re-read config from disk so picker edits reflect the latest settings.
    fn fresh_config(&self) -> BlumiConfig {
        BlumiConfig::load(
            &self.config.paths.working_dir,
            Some(self.config.paths.home.clone()),
        )
        .unwrap_or_else(|_| self.config.clone())
    }
}

pub async fn run(config: BlumiConfig) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();

    let store = Store::open(&config.paths.db).await.ok().map(Arc::new);

    // Skills listing for the `/skills` command + dashboard.
    let skills = blumi_skills::SkillCatalog::load(&[
        config.paths.skills.clone(),
        config.paths.working_dir.join(".blumi").join("skills"),
    ])
    .list()
    .into_iter()
    .map(|m| (m.name, m.description))
    .collect();

    // Recent sessions for the dashboard + `/sessions` (best-effort).
    let recent_sessions = match &store {
        Some(s) => s
            .list_sessions(12)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|m| (m.id, m.title))
            .collect(),
        None => Vec::new(),
    };

    // Personas (built-ins + configured) for the `/persona` command.
    let personas = crate::engine::resolve_personas(&config)
        .into_iter()
        .map(|p| (p.name, p.description))
        .collect();

    // Scheduled cron jobs for `/cron`.
    let cron_jobs = blumi_cron::CronStore::load(config.paths.home.join("cron.json"))
        .jobs()
        .iter()
        .map(|j| (j.name.clone(), j.schedule.clone()))
        .collect();

    let cfg = blumi_tui::TuiConfig {
        model_name: config.llm.model.clone(),
        working_dir: config.paths.working_dir.display().to_string(),
        memory_md: config.paths.memory_md(),
        user_md: config.paths.user_md(),
        skills,
        recent_sessions,
        personas,
        persona: crate::engine::active_persona_name(&config),
        export_dir: config.paths.sessions.clone(),
        context_size: config.llm.context_size,
        cron_jobs,
        tasks_path: crate::task::board_path(&config),
        brain_mode: blumi_core::BrainMode::parse(&config.brain.mode)
            .unwrap_or_default()
            .label()
            .to_string(),
    };

    let factory = Arc::new(TuiSessionFactory { config, store });
    blumi_tui::run(factory, cfg).await
}

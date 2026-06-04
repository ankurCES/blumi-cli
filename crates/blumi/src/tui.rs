//! `blumi tui` — interactive terminal UI with live session switch/resume.

use crate::engine::build_session;
use async_trait::async_trait;
use blumi_config::BlumiConfig;
use blumi_core::{SessionHandle, SessionState};
use blumi_persist::Store;
use blumi_protocol::{Message, Role, SessionId};
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

    async fn create_background(&self) -> anyhow::Result<SessionHandle> {
        // Background jobs run unattended — auto-approve so they never block on a
        // prompt no one can answer.
        build_session(&self.config, true, None).await
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

    async fn rollover(
        &self,
        snapshot: blumi_core::SessionSnapshot,
    ) -> anyhow::Result<SessionHandle> {
        let config = self.fresh_config();
        let model = if snapshot.model.is_empty() {
            config.llm.model.clone()
        } else {
            snapshot.model.clone()
        };

        // Summarize the old session for the handoff (best-effort; skipped for the
        // mock provider or if a client can't be built).
        let summary = match (config.llm.provider.as_str(), config.active_provider()) {
            ("mock", _) | (_, None) => None,
            (_, Some(provider)) => match blumi_llm::build_client(provider) {
                Ok(llm) => {
                    let opts = blumi_core::LlmOptions {
                        model: model.clone(),
                        max_output_tokens: 1024,
                        temperature: 0.0,
                        top_p: 1.0,
                        top_k: 0,
                        thinking: false,
                        prompt_cache: false,
                    };
                    blumi_core::summarize_history(
                        &llm,
                        &snapshot.messages,
                        &opts,
                        &tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                }
                Err(_) => None,
            },
        };

        // Carry the last few user/assistant turns verbatim (text only; dropping
        // tool messages keeps the seeded history self-consistent / orphan-free).
        let mut recent: Vec<Message> = snapshot
            .messages
            .iter()
            .filter(|m| {
                matches!(m.role, Role::User | Role::Assistant) && !m.text().trim().is_empty()
            })
            .rev()
            .take(6)
            .map(|m| {
                if m.role == Role::Assistant {
                    Message::assistant(m.text())
                } else {
                    Message::user(m.text())
                }
            })
            .collect();
        recent.reverse();

        let mut seed: Vec<Message> = Vec::new();
        if let Some(s) = summary {
            seed.push(Message::user(format!(
                "[Carryover from the previous session — the context window filled up. \
                 Continue seamlessly from this summary.]\n\n{s}"
            )));
        }
        seed.extend(recent);

        if seed.is_empty() {
            return build_session(&config, false, None).await;
        }
        let mut state = SessionState::new(SessionId::new(), model);
        state.messages = seed;
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
            // Don't persist empty sessions (e.g. a fresh one the user skipped at
            // the launch picker) — they'd just clutter the history list.
            if snapshot.messages.is_empty() {
                return;
            }
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

    fn remotes(&self) -> Vec<String> {
        self.fresh_config()
            .remote
            .instances
            .iter()
            .map(|r| r.name.clone())
            .collect()
    }

    async fn connect_remote(&self, name: &str) -> anyhow::Result<blumi_core::SessionHandle> {
        let cfg = self.fresh_config();
        let inst = cfg
            .remote
            .instances
            .iter()
            .find(|r| r.name == name)
            .ok_or_else(|| anyhow::anyhow!("unknown remote '{name}'"))?;
        if inst.url.trim().is_empty() {
            anyhow::bail!("remote '{name}' has no url");
        }
        Ok(crate::remote::connect(inst))
    }

    fn workspaces(&self) -> Vec<blumi_tui::Workspace> {
        crate::workspace::discover(&self.fresh_config())
    }

    async fn open_workspace(&self, path: &str) -> anyhow::Result<SessionHandle> {
        let dir = std::path::PathBuf::from(path);
        if !dir.is_dir() {
            anyhow::bail!("not a directory: {path}");
        }
        // Load that project's layered config (global home + the project's own
        // .blumi/settings.json), then spawn a session rooted there.
        let config = BlumiConfig::load(&dir, Some(self.config.paths.home.clone()))?;
        config.paths.ensure_dirs().ok();
        crate::workspace::record_recent(
            &self.config,
            dir.display().to_string().trim_end_matches('/'),
        );
        build_session(&config, false, None).await
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
        auto_continue: config.llm.max_auto_continue,
        themes: blumi_tui::theme::load_user_themes(&config.paths.home.join("themes")),
    };

    let factory = Arc::new(TuiSessionFactory { config, store });
    blumi_tui::run(factory, cfg).await
}

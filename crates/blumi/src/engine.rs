//! Shared agent wiring used by both `run` and `tui`: build the provider client,
//! tools, permissions, executor, system prompt, and spawn the session actor.

use crate::prompt::build_system_prompt;
use blumi_config::BlumiConfig;
use blumi_core::{
    builtin_agents, AgentSpawner, AgentTurnRunner, LlmClient, LlmOptions, PermissionEngine,
    SessionHandle, ToolRegistry,
};
use blumi_exec::LocalExecutor;
use blumi_llm::{build_client, MockLlmClient};
use blumi_protocol::{FinishReason, Message, SessionId, StreamChunk};
use blumi_skills::MemorySnapshot;
use std::sync::Arc;

/// A markdown + code sample streamed by the offline `mock` provider, so the TUI
/// (markdown, syntax highlighting, lists, streaming) can be exercised with no
/// network or API key.
fn mock_demo() -> Vec<StreamChunk> {
    let parts = [
        "# blumi mock provider\n\n",
        "Hello! This is the **mock** provider — no network or API key needed.\n\n",
        "It shows the v1 TUI: *markdown*, `inline code`, lists, and highlighted code.\n\n",
        "```rust\nfn main() {\n    let name = \"blumi\";\n    println!(\"hello, {name}\");\n}\n```\n\n",
        "Things to try:\n\n- `Ctrl+P` command palette\n- `/theme` to cycle rose / spatial / aurora / …\n- `/help` to list commands\n\n",
        "> Configure a real provider (anthropic, openai, ollama) to do real work.\n",
    ];
    let mut chunks: Vec<StreamChunk> = parts
        .iter()
        .map(|p| StreamChunk::Text {
            text: (*p).to_string(),
        })
        .collect();
    chunks.push(StreamChunk::Done {
        reason: FinishReason::Stop,
    });
    chunks
}

/// Build and spawn a session actor from config. `yolo` forces auto-approval
/// (used by headless `run`); the TUI passes `false` so approvals are interactive.
/// `seed` resumes an existing conversation (its messages become the new actor's
/// state). Async because connecting MCP servers spawns child processes.
pub async fn build_session(
    config: &BlumiConfig,
    yolo: bool,
    seed: Option<blumi_core::SessionState>,
) -> anyhow::Result<SessionHandle> {
    let llm: Arc<dyn LlmClient> = if config.llm.provider == "mock" {
        Arc::new(MockLlmClient::new(mock_demo()))
    } else {
        let provider = config.active_provider().ok_or_else(|| {
            anyhow::anyhow!(
                "unknown provider '{}' (check ~/.blumi/settings.json)",
                config.llm.provider
            )
        })?;
        build_client(provider)?
    };

    let mut registry = ToolRegistry::new();
    blumi_tools::register_builtin_tools(&mut registry);

    // Skills: discover SKILL.md under the user + project skills dirs; register
    // the `skill` tool and advertise them in the system prompt.
    let skill_dirs = [
        config.paths.skills.clone(),
        config.paths.working_dir.join(".blumi").join("skills"),
    ];
    let skills = blumi_skills::SkillCatalog::load(&skill_dirs);
    let skills_section = skills.prompt_section();
    if !skills.is_empty() {
        registry.register(Arc::new(blumi_core::Typed(blumi_skills::SkillTool::new(
            Arc::new(skills),
        ))));
    }

    // Long-term memory: the agent can persist to MEMORY.md / USER.md.
    registry.register(Arc::new(blumi_core::Typed(blumi_skills::MemoryTool::new(
        config.paths.memory_md(),
        config.paths.user_md(),
    ))));

    // Self-evolution: the agent can author its own skills, edit its own config
    // (validated before it lands), and reload itself in place to apply both.
    registry.register(Arc::new(blumi_core::Typed(
        blumi_skills::SkillManager::new(config.paths.skills.clone()),
    )));
    registry.register(Arc::new(blumi_core::Typed(blumi_skills::SelfConfig::new(
        config.paths.settings_json(),
    ))));
    registry.register(Arc::new(blumi_core::Typed(blumi_skills::ReloadTool::new())));
    registry.register(Arc::new(blumi_core::Typed(
        blumi_skills::RestartGatewayTool::new(),
    )));
    // Grid introspection: answer questions about peers/metrics in chat.
    registry.register(Arc::new(blumi_core::Typed(
        blumi_skills::GridStatusTool::new(),
    )));
    // Grid dispatch: run self-contained jobs on grid peers (round-robin) so a
    // single prompt can fan work across the fleet and collate the results.
    registry.register(Arc::new(blumi_core::Typed(
        blumi_skills::GridDispatchTool::new(),
    )));

    // Cross-session recall (FTS5) + durable-execution checkpoints share one
    // history DB. Skipped if it can't be opened — it must never block startup.
    let history_store: Option<Arc<blumi_persist::Store>> =
        match blumi_persist::Store::open(&config.paths.db).await {
            Ok(store) => {
                let store = Arc::new(store);
                registry.register(Arc::new(blumi_core::Typed(
                    blumi_tools::SessionSearch::new(store.clone()),
                )));
                Some(store)
            }
            Err(e) => {
                tracing::warn!("session history unavailable; SessionSearch disabled: {e}");
                None
            }
        };

    // Code intelligence: register the `Lsp` tool if any language servers are
    // configured (language-agnostic, keyed by file extension).
    if !config.lsp_servers.is_empty() {
        let servers: Vec<blumi_lsp::LspServer> = config
            .lsp_servers
            .values()
            .map(|s| blumi_lsp::LspServer {
                command: s.command.clone(),
                args: s.args.clone(),
                extensions: s.extensions.clone(),
                language_id: if s.language_id.is_empty() {
                    s.extensions.first().cloned().unwrap_or_default()
                } else {
                    s.language_id.clone()
                },
            })
            .collect();
        registry.register(Arc::new(blumi_core::Typed(blumi_lsp::LspTool::new(
            servers,
            config.paths.working_dir.clone(),
        ))));
    }

    let mut perm_cfg = config.permissions.clone();
    if yolo {
        perm_cfg.yolo = true;
    }
    // The brain is attached below, once the active model is resolved.
    let perm_engine = PermissionEngine::new(perm_cfg);

    // The agent's working directory. For ssh it's the remote workspace; for
    // local/docker it's the project dir (docker bind-mounts it).
    let work_dir: std::path::PathBuf =
        if config.executor.backend == "ssh" && !config.executor.ssh_workdir.is_empty() {
            std::path::PathBuf::from(&config.executor.ssh_workdir)
        } else {
            config.paths.working_dir.clone()
        };

    // Select the execution backend. Sandbox/remote failures fall back to local
    // so a missing daemon/host never blocks startup.
    let executor: Arc<dyn blumi_core::Executor> = match config.executor.backend.as_str() {
        "docker" => match blumi_exec::DockerExecutor::start(
            &config.executor.docker_image,
            &config.paths.working_dir,
        )
        .await
        {
            Ok(d) => {
                tracing::info!("docker sandbox: {}", config.executor.docker_image);
                Arc::new(d)
            }
            Err(e) => {
                tracing::warn!("docker sandbox unavailable ({e}); using local executor");
                Arc::new(LocalExecutor::new(&config.paths.working_dir))
            }
        },
        "ssh" if !config.executor.ssh_host.is_empty() => {
            tracing::info!(
                "ssh sandbox: {} ({})",
                config.executor.ssh_host,
                work_dir.display()
            );
            Arc::new(blumi_exec::SshExecutor::new(
                &config.executor.ssh_host,
                &config.executor.ssh_workdir,
            ))
        }
        other => {
            if other == "ssh" {
                tracing::warn!("ssh backend needs executor.ssh_host; using local executor");
            }
            Arc::new(LocalExecutor::new(&work_dir))
        }
    };

    // Stamp a default git identity on every command, so commits the agent makes
    // via `git`/`gh` are authored consistently regardless of repo/host config.
    let executor: Arc<dyn blumi_core::Executor> = if config.git.author_name.trim().is_empty() {
        executor
    } else {
        Arc::new(blumi_exec::GitIdentityExecutor::new(
            executor,
            &config.git.author_name,
            &config.git.author_email,
        ))
    };

    // Personas: built-ins merged with config; the active one seeds the model and
    // is layered onto the system prompt by the runner.
    let personas = resolve_personas(config);
    let active = active_persona_name(config);
    let active_persona = personas
        .iter()
        .find(|p| p.name == active)
        .cloned()
        .unwrap_or_default();
    let model = active_persona
        .model
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| config.llm.model.clone());

    let options = LlmOptions {
        model: model.clone(),
        max_output_tokens: config.llm.max_output_tokens,
        temperature: config.llm.temperature,
        top_p: config.llm.top_p,
        top_k: config.llm.top_k,
        thinking: false,
        prompt_cache: true,
    };

    // MCP: connect each enabled server and register its tools (mcp__server__tool).
    // A failed connection is logged and skipped — it never blocks startup. If the
    // user hasn't configured any, fall back to the default no-config set so a
    // fresh install has filesystem/fetch/git/etc out of the box.
    let mcp_servers = if config.mcp_servers.is_empty() {
        blumi_config::default_mcp_servers()
    } else {
        config.mcp_servers.clone()
    };
    // `{workspace}` / `{cwd}` in args/env resolve to the session's working dir
    // (settings.json is static; the path is per-session).
    let ws = work_dir.display().to_string();
    let subst = |s: &str| s.replace("{workspace}", &ws).replace("{cwd}", &ws);
    for (srv_name, srv) in &mcp_servers {
        if !srv.enabled {
            continue;
        }
        let args: Vec<String> = srv.args.iter().map(|a| subst(a)).collect();
        let env: Vec<(String, String)> =
            srv.env.iter().map(|(k, v)| (k.clone(), subst(v))).collect();
        match blumi_mcp::connect_server(srv_name, &srv.command, &args, &env).await {
            Ok(tools) => {
                for tool in tools {
                    registry.register(tool);
                }
            }
            Err(e) => tracing::warn!("MCP server '{srv_name}' failed to connect: {e}"),
        }
    }

    let memory = MemorySnapshot::load(&config.paths.memory_md(), &config.paths.user_md());
    let system_prompt = build_system_prompt(config, &memory, &skills_section);

    let registry = Arc::new(registry);

    // Brain: a local-LLM reviewer over the approval path (claudectl-style). It
    // can reuse the main client or judge with a cheaper dedicated provider
    // (e.g. a local `ollama` model) so auto-approval stays fast and free. We
    // always attach it (starting in the configured mode, default `off`) so the
    // `/brain` command can switch advisory/auto on live without a restart.
    let brain_mode = blumi_core::BrainMode::parse(&config.brain.mode).unwrap_or_default();
    let brain_llm: Arc<dyn LlmClient> = if config.brain.provider.is_empty() {
        llm.clone()
    } else if let Some(p) = config.providers.get(&config.brain.provider) {
        build_client(p).unwrap_or_else(|e| {
            tracing::warn!(
                "brain provider '{}' failed ({e}); reusing main client",
                config.brain.provider
            );
            llm.clone()
        })
    } else {
        tracing::warn!(
            "brain provider '{}' not configured; reusing main client",
            config.brain.provider
        );
        llm.clone()
    };
    let brain_model = if config.brain.model.is_empty() {
        model.clone()
    } else {
        config.brain.model.clone()
    };
    if brain_mode != blumi_core::BrainMode::Off {
        tracing::info!(
            "brain enabled: mode={} model={brain_model}",
            brain_mode.label()
        );
    }
    let perms = Arc::new(perm_engine.with_brain(
        Arc::new(blumi_core::LocalBrain::new(brain_llm, brain_model)),
        brain_mode,
    ));

    // Sub-agent delegation: the spawner shares the same provider/registry/executor.
    let spawner = Arc::new(
        AgentSpawner::new(
            llm.clone(),
            registry.clone(),
            perms.clone(),
            executor.clone(),
            options.clone(),
            config.llm.context_size,
            work_dir.clone(),
            builtin_agents(),
        )
        .with_max_local_agents(config.llm.max_local_agents),
    );

    let mut runner = AgentTurnRunner::new(
        llm,
        registry,
        perms,
        executor,
        options,
        config.llm.max_iterations,
        config.llm.context_size,
        system_prompt,
        work_dir.clone(),
    )
    .with_spawner(spawner)
    .with_personas(personas, &active)
    .with_auto_continue(config.llm.max_auto_continue)
    .with_auto_continue_tokens(config.llm.max_auto_continue_tokens);
    // Durable execution: checkpoint the turn after each tool step (shares the
    // history DB) so a crash/restart resumes from the last step.
    if let Some(store) = &history_store {
        runner = runner.with_checkpoint(Arc::new(blumi_persist::CheckpointSinkImpl(store.clone())));
    }
    let runner = Arc::new(runner);

    let mut state =
        seed.unwrap_or_else(|| blumi_core::SessionState::new(SessionId::new(), model.clone()));
    // Durable execution: if this session has an in-progress turn checkpointed
    // (interrupted mid-turn), resume from the last completed tool step.
    if let Some(store) = &history_store {
        if let Ok(Some(cp)) = store.take_incomplete(state.id.as_str()).await {
            if cp.messages.len() > state.messages.len() {
                state.messages = cp.messages;
                state.todos = cp.todos;
                state.messages.push(Message::user(
                    "[Resuming after an interruption — continue exactly where you left off; \
                     do not repeat already-completed steps.]"
                        .to_string(),
                ));
            }
        }
    }
    Ok(blumi_core::spawn_session_seeded(state, runner))
}

/// Built-in personas merged with any configured in settings (config entries
/// override or add by name).
pub fn resolve_personas(config: &BlumiConfig) -> Vec<blumi_core::Persona> {
    let mut personas = blumi_core::builtin_personas();
    for (name, pc) in &config.personas {
        let p = blumi_core::Persona {
            name: name.clone(),
            description: pc.description.clone(),
            instructions: pc.instructions.clone(),
            model: pc.model.clone(),
            temperature: pc.temperature,
        };
        match personas.iter_mut().find(|x| x.name == *name) {
            Some(slot) => *slot = p,
            None => personas.push(p),
        }
    }
    personas
}

/// The active persona name (falling back to `default`).
pub fn active_persona_name(config: &BlumiConfig) -> String {
    if config.persona.is_empty() {
        "default".into()
    } else {
        config.persona.clone()
    }
}

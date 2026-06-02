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
use blumi_protocol::{FinishReason, SessionId, StreamChunk};
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

    // Cross-session recall: full-text (FTS5) search over past sessions. Skipped
    // if the history DB can't be opened — it must never block startup.
    match blumi_persist::Store::open(&config.paths.db).await {
        Ok(store) => {
            registry.register(Arc::new(blumi_core::Typed(
                blumi_tools::SessionSearch::new(Arc::new(store)),
            )));
        }
        Err(e) => tracing::warn!("session history unavailable; SessionSearch disabled: {e}"),
    }

    let mut perm_cfg = config.permissions.clone();
    if yolo {
        perm_cfg.yolo = true;
    }
    let perms = Arc::new(PermissionEngine::new(perm_cfg));

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
    // A failed connection is logged and skipped — it never blocks startup.
    for (srv_name, srv) in &config.mcp_servers {
        if !srv.enabled {
            continue;
        }
        let env: Vec<(String, String)> = srv
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        match blumi_mcp::connect_server(srv_name, &srv.command, &srv.args, &env).await {
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

    // Sub-agent delegation: the spawner shares the same provider/registry/executor.
    let spawner = Arc::new(AgentSpawner::new(
        llm.clone(),
        registry.clone(),
        perms.clone(),
        executor.clone(),
        options.clone(),
        config.llm.context_size,
        work_dir.clone(),
        builtin_agents(),
    ));

    let runner = Arc::new(
        AgentTurnRunner::new(
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
        .with_personas(personas, &active),
    );

    let state =
        seed.unwrap_or_else(|| blumi_core::SessionState::new(SessionId::new(), model.clone()));
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

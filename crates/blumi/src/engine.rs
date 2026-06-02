//! Shared agent wiring used by both `run` and `tui`: build the provider client,
//! tools, permissions, executor, system prompt, and spawn the session actor.

use crate::prompt::build_system_prompt;
use blumi_config::BlumiConfig;
use blumi_core::{
    builtin_agents, spawn_session, AgentSpawner, AgentTurnRunner, LlmClient, LlmOptions,
    PermissionEngine, SessionHandle, ToolRegistry,
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
        "Things to try:\n\n- `Ctrl+P` command palette\n- `/theme` to cycle bloom / dark / mono\n- `/help` to list commands\n\n",
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
pub fn build_session(config: &BlumiConfig, yolo: bool) -> anyhow::Result<SessionHandle> {
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

    let mut perm_cfg = config.permissions.clone();
    if yolo {
        perm_cfg.yolo = true;
    }
    let perms = Arc::new(PermissionEngine::new(perm_cfg));
    let executor = Arc::new(LocalExecutor::new(&config.paths.working_dir));

    let options = LlmOptions {
        model: config.llm.model.clone(),
        max_output_tokens: config.llm.max_output_tokens,
        temperature: config.llm.temperature,
        top_p: config.llm.top_p,
        top_k: config.llm.top_k,
        thinking: false,
        prompt_cache: true,
    };

    let memory = MemorySnapshot::load(&config.paths.memory_md(), &config.paths.user_md());
    let system_prompt = build_system_prompt(config, &memory);

    let registry = Arc::new(registry);

    // Sub-agent delegation: the spawner shares the same provider/registry/executor.
    let spawner = Arc::new(AgentSpawner::new(
        llm.clone(),
        registry.clone(),
        perms.clone(),
        executor.clone(),
        options.clone(),
        config.llm.context_size,
        config.paths.working_dir.clone(),
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
            config.paths.working_dir.clone(),
        )
        .with_spawner(spawner),
    );

    Ok(spawn_session(
        SessionId::new(),
        config.llm.model.clone(),
        runner,
    ))
}

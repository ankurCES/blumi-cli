//! `lumi run` — headless one-shot: stream a single prompt's result to stdout.

use crate::prompt::build_system_prompt;
use lumi_config::LumiConfig;
use lumi_core::{
    spawn_session, AgentTurnRunner, LlmClient, LlmOptions, PermissionEngine, ToolRegistry,
};
use lumi_exec::LocalExecutor;
use lumi_llm::{build_client, MockLlmClient};
use lumi_protocol::{
    ApprovalScope, Command, Decision, Event, FinishReason, SessionId, StreamChunk,
};
use lumi_skills::MemorySnapshot;
use std::io::Write;
use std::sync::Arc;

pub async fn run(config: LumiConfig, prompt: String, yolo: bool) -> anyhow::Result<()> {
    let prompt = resolve_prompt(prompt)?;
    config.paths.ensure_dirs().ok();

    // Build the LLM client (special-case the offline "mock" provider).
    let llm: Arc<dyn LlmClient> =
        if config.llm.provider == "mock" {
            Arc::new(MockLlmClient::new(vec![
            StreamChunk::Text {
                text: "Hello from lumi (mock provider). Configure a real provider to do real work."
                    .into(),
            },
            StreamChunk::Done { reason: FinishReason::Stop },
        ]))
        } else {
            let provider = config.active_provider().ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown provider '{}' (check ~/.lumi/settings.json)",
                    config.llm.provider
                )
            })?;
            build_client(provider)?
        };

    // Tools, permissions, executor.
    let mut registry = ToolRegistry::new();
    lumi_tools::register_builtin_tools(&mut registry);

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
    let system_prompt = build_system_prompt(&config, &memory);

    let runner = Arc::new(AgentTurnRunner::new(
        llm,
        Arc::new(registry),
        perms,
        executor,
        options,
        config.llm.max_iterations,
        config.llm.context_size,
        system_prompt,
        config.paths.working_dir.clone(),
    ));

    let session = spawn_session(SessionId::new(), config.llm.model.clone(), runner);
    let mut events = session.subscribe();
    session
        .send(Command::UserMessage {
            text: prompt,
            attachments: vec![],
            stream_id: None,
        })
        .await?;

    let mut stdout = std::io::stdout();
    loop {
        let env = events.recv().await?;
        match env.event {
            Event::Token { text } => {
                write!(stdout, "{text}")?;
                stdout.flush()?;
            }
            Event::ToolStart { name, summary, .. } => {
                eprintln!("\x1b[2m  ⚙ {name}: {}\x1b[0m", first_line(&summary));
            }
            Event::ToolResult {
                name, ok, preview, ..
            } => {
                let mark = if ok { "✓" } else { "✗" };
                eprintln!("\x1b[2m  {mark} {name}: {}\x1b[0m", first_line(&preview));
            }
            Event::ApprovalRequest {
                request_id,
                tool,
                summary,
                ..
            } => {
                // Headless: auto-allow with --yolo, otherwise deny (never hang).
                let decision = if yolo {
                    Decision::Allow
                } else {
                    Decision::Deny
                };
                eprintln!(
                    "\x1b[33m  permission: {tool} {} → {decision:?}\x1b[0m",
                    first_line(&summary)
                );
                session
                    .send(Command::ApproveTool {
                        request_id,
                        decision,
                        scope: ApprovalScope::Once,
                    })
                    .await?;
            }
            Event::ClarifyRequest { request_id, .. } => {
                // No interactive prompt in headless mode; answer empty.
                session
                    .send(Command::AnswerClarify {
                        request_id,
                        value: String::new(),
                    })
                    .await?;
            }
            Event::Error { message, .. } => {
                eprintln!("\x1b[31m  error: {message}\x1b[0m");
            }
            Event::TurnDone { reason } => {
                writeln!(stdout)?;
                tracing::debug!(?reason, "turn finished");
                break;
            }
            _ => {}
        }
    }

    // Persist the session (best-effort; never fail the run on a save error).
    match lumi_persist::Store::open(&config.paths.db).await {
        Ok(store) => {
            let snapshot = session.snapshot().await;
            if let Err(e) = store.save_snapshot(&snapshot).await {
                tracing::warn!("could not save session: {e}");
            }
        }
        Err(e) => tracing::warn!("could not open session store: {e}"),
    }

    Ok(())
}

fn resolve_prompt(prompt: String) -> anyhow::Result<String> {
    if !prompt.trim().is_empty() {
        return Ok(prompt);
    }
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        anyhow::bail!("no prompt provided (pass it as an argument or pipe it on stdin)");
    }
    Ok(buf)
}

fn first_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or("");
    if line.len() > 120 {
        format!(
            "{}…",
            &line[..line
                .char_indices()
                .take(120)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0)]
        )
    } else {
        line.to_string()
    }
}

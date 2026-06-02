//! `blumi run` — headless one-shot: stream a single prompt's result to stdout.

use crate::engine::build_session;
use blumi_config::BlumiConfig;
use blumi_protocol::{ApprovalScope, Command, Decision, Event};
use std::io::Write;

pub async fn run(config: BlumiConfig, prompt: String, yolo: bool) -> anyhow::Result<()> {
    let prompt = resolve_prompt(prompt)?;
    config.paths.ensure_dirs().ok();

    let session = build_session(&config, yolo)?;
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
    match blumi_persist::Store::open(&config.paths.db).await {
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

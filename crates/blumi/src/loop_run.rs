//! `blumi loop` — the autonomous task loop (ralph-style): pull the highest
//! priority todo from the board, run it, advance it, repeat — with iteration,
//! budget, and notification guardrails (claudectl-style).

use crate::engine::build_session;
use crate::task::board_path;
use blumi_config::BlumiConfig;
use blumi_core::SessionHandle;
use blumi_protocol::{ApprovalScope, Command, Decision, Event};
use blumi_task::{TaskBoard, TaskState};
use std::io::Write;
use time::OffsetDateTime;

/// Options for `blumi loop`.
pub struct LoopOptions {
    /// Stop after at most N iterations.
    pub max: Option<u32>,
    /// Stop once cumulative reported cost (USD) reaches this (provider-dependent).
    pub budget: Option<f64>,
    /// Auto-approve tool calls (otherwise approval-requiring tools are denied).
    pub yolo: bool,
    /// Send finished tasks to "review" instead of "done" (human gate).
    pub review: bool,
    /// Desktop notification when the loop finishes.
    pub notify: bool,
}

pub async fn run(config: BlumiConfig, opts: LoopOptions) -> anyhow::Result<()> {
    let path = board_path(&config);
    if TaskBoard::load(&path).next_todo().is_none() {
        println!("no todo tasks — add some first:  blumi task add \"build X\" -p 1");
        return Ok(());
    }
    config.paths.ensure_dirs().ok();

    crate::branding::banner();
    eprintln!(
        "  blumi loop — {} mode{}{}  (Ctrl+C to stop)\n",
        if opts.yolo { "auto-approve" } else { "safe" },
        opts.max.map(|m| format!(" · max {m}")).unwrap_or_default(),
        opts.budget
            .map(|b| format!(" · budget ${b:.2}"))
            .unwrap_or_default(),
    );

    // One session, reused across tasks, so context carries between iterations.
    let session = build_session(&config, opts.yolo, None).await?;

    let mut iter = 0u32;
    let mut cost = 0.0f64;
    loop {
        // Re-read the board each iteration so external edits/cancels take effect.
        let mut board = TaskBoard::load(&path);
        let Some(task) = board.next_todo().cloned() else {
            break;
        };
        if let Some(max) = opts.max {
            if iter >= max {
                eprintln!("\n■ reached --max {max}");
                break;
            }
        }
        iter += 1;

        board.set_state(&task.id, TaskState::Doing, OffsetDateTime::now_utc());
        board.save().ok();
        eprintln!(
            "\x1b[1m[iter {iter}] ▶ {} (P{})\x1b[0m",
            task.title, task.priority
        );

        let prompt = if task.detail.trim().is_empty() {
            task.title.clone()
        } else {
            format!("{}\n\n{}", task.title, task.detail)
        };
        if let Some(c) = run_turn(&session, &prompt, opts.yolo).await? {
            cost = c; // providers that report cost give a cumulative figure
        }

        // Advance the task (re-load so we don't clobber concurrent edits).
        let mut board = TaskBoard::load(&path);
        let next = if opts.review {
            TaskState::Review
        } else {
            TaskState::Done
        };
        board.set_state(&task.id, next, OffsetDateTime::now_utc());
        board.save().ok();
        eprintln!("\x1b[2m{} {}\x1b[0m", next.icon(), task.title);

        if let Some(b) = opts.budget {
            if cost >= b {
                eprintln!("\n■ reached --budget ${b:.2} (spent ${cost:.2})");
                break;
            }
        }
    }

    // Persist the session + print a summary.
    let snap = session.snapshot().await;
    if let Ok(store) = blumi_persist::Store::open(&config.paths.db).await {
        let _ = store.save_snapshot(&snap).await;
    }
    let c = TaskBoard::load(&path).counts();
    let summary = format!(
        "{iter} iterations · done {} · review {} · queued {} · ↑{} ↓{}{}",
        c.done,
        c.review,
        c.todo,
        snap.total_input_tokens,
        snap.total_output_tokens,
        if cost > 0.0 {
            format!(" · ${cost:.2}")
        } else {
            String::new()
        },
    );
    eprintln!("\n\x1b[1m✿ loop done — {summary}\x1b[0m");
    if opts.notify {
        notify("blumi loop finished", &summary);
    }
    Ok(())
}

/// Drive one task turn, streaming output; returns the latest reported cost (USD).
async fn run_turn(
    session: &SessionHandle,
    prompt: &str,
    yolo: bool,
) -> anyhow::Result<Option<f64>> {
    let mut events = session.subscribe();
    session
        .send(Command::UserMessage {
            text: prompt.to_string(),
            attachments: vec![],
            stream_id: None,
        })
        .await?;
    let mut stdout = std::io::stdout();
    let mut cost = None;
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
            Event::Usage {
                cost_usd: Some(c), ..
            } => cost = Some(c),
            Event::ApprovalRequest { request_id, .. } => {
                let decision = if yolo {
                    Decision::Allow
                } else {
                    Decision::Deny
                };
                session
                    .send(Command::ApproveTool {
                        request_id,
                        decision,
                        scope: ApprovalScope::Once,
                    })
                    .await?;
            }
            Event::ClarifyRequest { request_id, .. } => {
                session
                    .send(Command::AnswerClarify {
                        request_id,
                        value: String::new(),
                    })
                    .await?;
            }
            Event::Error { message, .. } => eprintln!("\x1b[31m  error: {message}\x1b[0m"),
            Event::TurnDone { .. } => {
                writeln!(stdout)?;
                break;
            }
            _ => {}
        }
    }
    Ok(cost)
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(120).collect()
}

/// Best-effort desktop notification.
fn notify(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    let cmd = (
        "osascript",
        vec![
            "-e".to_string(),
            format!("display notification {:?} with title {:?}", body, title),
        ],
    );
    #[cfg(target_os = "linux")]
    let cmd = ("notify-send", vec![title.to_string(), body.to_string()]);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let cmd: (&str, Vec<String>) = ("true", vec![]);

    let _ = std::process::Command::new(cmd.0)
        .args(cmd.1)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

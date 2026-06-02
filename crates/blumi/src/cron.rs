//! `blumi cron` — scheduled automations: run a prompt headlessly on a schedule
//! and deliver the result. The scheduling lives in `blumi-cron`; this drives
//! execution with the agent engine.

use crate::engine::build_session;
use blumi_config::BlumiConfig;
use blumi_cron::{CronStore, Schedule};
use blumi_protocol::{ApprovalScope, Command, Decision, Event};
use std::io::Write;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

fn store_path(config: &BlumiConfig) -> PathBuf {
    config.paths.home.join("cron.json")
}

pub fn add(
    config: BlumiConfig,
    name: String,
    schedule: String,
    prompt: String,
    deliver: Option<String>,
) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    let mut store = CronStore::load(store_path(&config));
    let deliver = deliver.unwrap_or_else(|| "log".to_string());
    let now = OffsetDateTime::now_utc();
    match store.add(&name, &schedule, &prompt, &deliver, now) {
        Ok(id) => {
            store.save()?;
            let desc = Schedule::parse(&schedule)
                .map(|s| s.describe())
                .unwrap_or(schedule);
            println!("added cron job '{name}' [{id}] — {desc}");
            Ok(())
        }
        Err(e) => anyhow::bail!("{e}"),
    }
}

pub fn list(config: BlumiConfig) -> anyhow::Result<()> {
    let store = CronStore::load(store_path(&config));
    if store.jobs().is_empty() {
        println!(
            "No cron jobs yet. Add one:\n  blumi cron add --name digest \
             --schedule \"daily 09:00\" --prompt \"summarize git log since yesterday\""
        );
        return Ok(());
    }
    for j in store.jobs() {
        let sched = j
            .parsed_schedule()
            .map(|s| s.describe())
            .unwrap_or_else(|_| j.schedule.clone());
        let state = if j.enabled { "" } else { "  (disabled)" };
        let last = j.last_run.as_deref().unwrap_or("never");
        println!("{}  {}{}", j.id, j.name, state);
        println!("  {sched} · deliver: {} · last: {last}", j.deliver);
        println!("  prompt: {}", first_line(&j.prompt));
    }
    Ok(())
}

pub fn remove(config: BlumiConfig, id: String) -> anyhow::Result<()> {
    let mut store = CronStore::load(store_path(&config));
    if store.remove(&id) {
        store.save()?;
        println!("removed {id}");
    } else {
        println!("no cron job matching {id:?}");
    }
    Ok(())
}

pub async fn run(config: BlumiConfig, watch: bool) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    if watch {
        println!("blumi cron: watching for due jobs every 60s (Ctrl+C to stop)…");
        loop {
            if let Err(e) = run_due(&config).await {
                eprintln!("\x1b[31mcron tick failed: {e}\x1b[0m");
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    } else if run_due(&config).await? == 0 {
        println!("no jobs due right now.");
    }
    Ok(())
}

/// Run every due job once, deliver results, and persist run times.
async fn run_due(config: &BlumiConfig) -> anyhow::Result<usize> {
    let mut store = CronStore::load(store_path(config));
    let now = OffsetDateTime::now_utc();
    let due = store.due(now);
    for job in &due {
        eprintln!("\x1b[2m▶ cron '{}' running…\x1b[0m", job.name);
        match run_job(config, &job.prompt).await {
            Ok(output) => {
                if let Err(e) = deliver(&job.deliver, &job.name, &output) {
                    eprintln!("\x1b[31m  delivery failed: {e}\x1b[0m");
                }
                store.mark_run(&job.id, OffsetDateTime::now_utc());
            }
            Err(e) => eprintln!("\x1b[31m  cron '{}' failed: {e}\x1b[0m", job.name),
        }
    }
    if !due.is_empty() {
        store.save()?;
    }
    Ok(due.len())
}

/// Execute one prompt in a fresh headless session, returning the assistant text.
async fn run_job(config: &BlumiConfig, prompt: &str) -> anyhow::Result<String> {
    // Headless: auto-approve so unattended runs don't hang on a permission prompt.
    let session = build_session(config, true, None).await?;
    let mut events = session.subscribe();
    session
        .send(Command::UserMessage {
            text: prompt.to_string(),
            attachments: vec![],
            stream_id: None,
        })
        .await?;

    let mut out = String::new();
    loop {
        let env = events.recv().await?;
        match env.event {
            Event::Token { text } => out.push_str(&text),
            Event::ApprovalRequest { request_id, .. } => {
                session
                    .send(Command::ApproveTool {
                        request_id,
                        decision: Decision::Allow,
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
            Event::TurnDone { .. } => break,
            _ => {}
        }
    }

    // Persist the run as a session (best-effort).
    if let Ok(store) = blumi_persist::Store::open(&config.paths.db).await {
        let _ = store.save_snapshot(&session.snapshot().await).await;
    }
    Ok(out)
}

/// Deliver output to a `file:<path>` (append) or `log` (stdout).
fn deliver(target: &str, name: &str, output: &str) -> anyhow::Result<()> {
    let stamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default();
    if let Some(path) = target.strip_prefix("file:") {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(f, "\n## {name} — {stamp}\n\n{output}")?;
        eprintln!("\x1b[2m  ✓ delivered to {path}\x1b[0m");
    } else {
        println!("\n## {name} — {stamp}\n\n{output}");
    }
    Ok(())
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(80).collect()
}

//! `blumi playbook` — run a YAML workflow: ordered steps (each a prompt run
//! headlessly), optional shell gates, and checkpoint/resume.

use crate::engine::build_session;
use blumi_config::BlumiConfig;
use blumi_core::{ExecRequest, Executor};
use blumi_exec::LocalExecutor;
use blumi_playbook::{Playbook, PlaybookState};
use blumi_protocol::{ApprovalScope, Command, Decision, DoneReason, Event};
use std::io::Write;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

fn state_path(config: &BlumiConfig, name: &str) -> PathBuf {
    config
        .paths
        .home
        .join("playbooks")
        .join(format!("{name}.state.json"))
}

pub async fn run(config: BlumiConfig, file: PathBuf, restart: bool) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    let pb = Playbook::load(&file).map_err(|e| anyhow::anyhow!("{e}"))?;
    let sp = state_path(&config, &pb.name);
    let mut state = if restart {
        PlaybookState::default()
    } else {
        PlaybookState::load(&sp)
    };

    println!("▶ playbook '{}' — {} step(s)", pb.name, pb.steps.len());
    let local = LocalExecutor::new(&config.paths.working_dir);

    for step in &pb.steps {
        if state.is_done(&step.name) {
            println!("  ✓ {} (already done)", step.name);
            continue;
        }
        // A gate is a host shell check; the step runs only if it exits 0.
        if let Some(gate) = &step.gate {
            let ok = local
                .exec(ExecRequest::new(gate.clone()), CancellationToken::new())
                .await
                .map(|o| o.success())
                .unwrap_or(false);
            if !ok {
                println!("  − {} (gate failed, skipped)", step.name);
                continue;
            }
        }

        println!("  ▶ {}", step.name);
        match run_prompt(&config, &step.prompt).await {
            Ok(()) => {
                state.mark_done(&step.name);
                let _ = state.save(&sp);
                println!("  ✓ {}", step.name);
            }
            Err(e) => {
                eprintln!("  ✗ {}: {e}", step.name);
                if !step.continue_on_error {
                    anyhow::bail!("playbook stopped at step '{}'", step.name);
                }
            }
        }
    }
    println!("✓ playbook '{}' complete", pb.name);
    Ok(())
}

/// Run one prompt in a fresh headless session, streaming output to stdout.
async fn run_prompt(config: &BlumiConfig, prompt: &str) -> anyhow::Result<()> {
    let session = build_session(config, true, None).await?; // headless auto-approve
    let mut events = session.subscribe();
    session
        .send(Command::UserMessage {
            text: prompt.to_string(),
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
            Event::TurnDone { reason } => {
                writeln!(stdout)?;
                if matches!(reason, DoneReason::Error | DoneReason::DoomLoop) {
                    anyhow::bail!("turn ended: {reason:?}");
                }
                break;
            }
            _ => {}
        }
    }

    if let Ok(store) = blumi_persist::Store::open(&config.paths.db).await {
        let _ = store.save_snapshot(&session.snapshot().await).await;
    }
    Ok(())
}

pub fn list(config: BlumiConfig) -> anyhow::Result<()> {
    let dirs = [
        config.paths.home.join("playbooks"),
        config.paths.working_dir.join(".blumi").join("playbooks"),
    ];
    let mut found = false;
    for dir in &dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let is_yaml = p
                .extension()
                .map(|x| x == "yaml" || x == "yml")
                .unwrap_or(false);
            if !is_yaml {
                continue;
            }
            if let Ok(pb) = Playbook::load(&p) {
                found = true;
                println!("{}  ({} steps)  {}", pb.name, pb.steps.len(), p.display());
                if !pb.description.is_empty() {
                    println!("  {}", pb.description);
                }
            }
        }
    }
    if !found {
        println!(
            "No playbooks. Add a .yaml under ~/.blumi/playbooks/ or .blumi/playbooks/ \
             then `blumi playbook run <file>`."
        );
    }
    Ok(())
}

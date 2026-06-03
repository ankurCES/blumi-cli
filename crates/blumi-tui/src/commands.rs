//! Slash-command registry + dispatch, shared by the `/` popup and `/help`.

use crate::model::{Entry, Model};
use blumi_core::SessionHandle;
use blumi_protocol::Command;

pub struct CommandDef {
    pub name: &'static str,
    pub desc: &'static str,
}

/// The command palette. (hermes also offers voice/branch/background/terminal/
/// goal/kanban — those need subsystems blumi doesn't have yet.)
pub const COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "/help",
        desc: "list commands",
    },
    CommandDef {
        name: "/clear",
        desc: "clear the view (keep session)",
    },
    CommandDef {
        name: "/new",
        desc: "start a fresh session",
    },
    CommandDef {
        name: "/resume",
        desc: "resume a session: /resume [id]",
    },
    CommandDef {
        name: "/retry",
        desc: "resend the last message",
    },
    CommandDef {
        name: "/tasks",
        desc: "toggle the run dashboard",
    },
    CommandDef {
        name: "/dashboard",
        desc: "toggle the run dashboard",
    },
    CommandDef {
        name: "/usage",
        desc: "show token usage",
    },
    CommandDef {
        name: "/board",
        desc: "show the task board (blumi loop work queue)",
    },
    CommandDef {
        name: "/memory",
        desc: "view saved memory",
    },
    CommandDef {
        name: "/skills",
        desc: "list available skills",
    },
    CommandDef {
        name: "/sessions",
        desc: "switch session (ctrl+s)",
    },
    CommandDef {
        name: "/export",
        desc: "save transcript to a file",
    },
    CommandDef {
        name: "/compact",
        desc: "compact the context now",
    },
    CommandDef {
        name: "/undo",
        desc: "undo the last file change",
    },
    CommandDef {
        name: "/yolo",
        desc: "toggle auto-approve (yolo)",
    },
    CommandDef {
        name: "/persona",
        desc: "switch persona: /persona [name]",
    },
    CommandDef {
        name: "/name",
        desc: "name this session: /name <title>",
    },
    CommandDef {
        name: "/queue",
        desc: "queue a message: /queue <msg>",
    },
    CommandDef {
        name: "/steer",
        desc: "steer the agent now: /steer <msg>",
    },
    CommandDef {
        name: "/goal",
        desc: "set a session goal: /goal <text>",
    },
    CommandDef {
        name: "/reasoning",
        desc: "toggle reasoning display",
    },
    CommandDef {
        name: "/cron",
        desc: "list scheduled jobs",
    },
    CommandDef {
        name: "/model",
        desc: "pick a model (or /model <id>)",
    },
    CommandDef {
        name: "/provider",
        desc: "pick an LLM provider (reloads the agent)",
    },
    CommandDef {
        name: "/theme",
        desc: "switch theme: /theme [name]",
    },
    CommandDef {
        name: "/status",
        desc: "session status",
    },
    CommandDef {
        name: "/stop",
        desc: "cancel the current turn",
    },
    CommandDef {
        name: "/login",
        desc: "reconfigure providers (from a shell)",
    },
    CommandDef {
        name: "/quit",
        desc: "exit blumi",
    },
];

/// Commands whose `/name` prefix matches the typed input (for the popup).
pub fn matching(input: &str) -> Vec<&'static CommandDef> {
    let head = input.split_whitespace().next().unwrap_or("/");
    COMMANDS
        .iter()
        .filter(|c| c.name.starts_with(head))
        .collect()
}

/// Run a slash command line (e.g. "/model claude-x").
pub async fn run(model: &mut Model, session: &SessionHandle, line: &str) {
    let line = line.trim().to_string();
    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim().to_string();
    model.clear_input();

    match cmd {
        "/help" => model.entries.push(Entry::Notice(help_text())),
        "/clear" => model.clear_transcript(),
        "/new" => model.request_new_session(),
        "/resume" => {
            if arg.is_empty() {
                model.dialog = Some(crate::dialog::Picker::session_picker(
                    &model.recent_sessions,
                ));
            } else {
                model.request_resume(arg);
            }
        }
        "/retry" => {
            if model.busy {
                model
                    .entries
                    .push(Entry::Notice("busy — press esc to cancel first".into()));
            } else if let Some(text) = model.last_user_text() {
                model.entries.push(Entry::User(text.clone()));
                model.busy = true;
                model.scrollback = 0;
                let _ = session
                    .send(Command::UserMessage {
                        text,
                        attachments: vec![],
                        stream_id: None,
                    })
                    .await;
            } else {
                model.entries.push(Entry::Notice("nothing to retry".into()));
            }
        }
        "/tasks" | "/dashboard" => model.show_dashboard = !model.show_dashboard,
        "/usage" => model.open_usage(),
        "/board" => model.open_board(),
        "/memory" => model.open_memory(),
        "/skills" => {
            if model.skills.is_empty() {
                model.entries.push(Entry::Notice(
                    "no skills found (add SKILL.md under ~/.blumi/skills/<name>/)".into(),
                ));
            } else {
                let mut s = String::from("skills:");
                for (n, d) in &model.skills {
                    s.push_str(&format!("\n- {n}: {d}"));
                }
                model.entries.push(Entry::Notice(s));
            }
        }
        "/sessions" => {
            model.dialog = Some(crate::dialog::Picker::session_picker(
                &model.recent_sessions,
            ))
        }
        "/export" => match model.export_transcript() {
            Ok(path) => model.entries.push(Entry::Notice(format!(
                "exported transcript → {}",
                path.display()
            ))),
            Err(e) => model
                .entries
                .push(Entry::Notice(format!("export failed: {e}"))),
        },
        "/compact" => {
            if model.busy {
                model
                    .entries
                    .push(Entry::Notice("busy — press esc to cancel first".into()));
            } else {
                let _ = session.send(Command::Compact).await;
                model
                    .entries
                    .push(Entry::Notice("compacting context…".into()));
            }
        }
        "/undo" => {
            if model.busy {
                model
                    .entries
                    .push(Entry::Notice("busy — press esc to cancel first".into()));
            } else {
                // The core replies with a Notice describing what was reverted.
                let _ = session.send(Command::Undo).await;
            }
        }
        "/yolo" => {
            model.yolo = !model.yolo;
            let _ = session.send(Command::SetYolo { on: model.yolo }).await;
            model.entries.push(Entry::Notice(
                if model.yolo {
                    "auto-approve ON — blumi will run tools without asking. /yolo again to undo"
                } else {
                    "auto-approve off — tools will ask for approval"
                }
                .into(),
            ));
        }
        "/persona" => {
            if arg.is_empty() {
                if model.personas.is_empty() {
                    model
                        .entries
                        .push(Entry::Notice("no personas configured".into()));
                } else {
                    let mut s = String::from("personas:");
                    for (n, d) in &model.personas {
                        let marker = if *n == model.persona {
                            "  ← active"
                        } else {
                            ""
                        };
                        s.push_str(&format!("\n- {n}: {d}{marker}"));
                    }
                    model.entries.push(Entry::Notice(s));
                }
            } else if model.personas.iter().any(|(n, _)| n == &arg) {
                model.persona = arg.clone();
                let _ = session.send(Command::SetPersona { name: arg }).await;
                // The core replies with a Notice confirming the switch.
            } else {
                model.entries.push(Entry::Notice(format!(
                    "unknown persona '{arg}' (try /persona to list)"
                )));
            }
        }
        "/name" => {
            if arg.is_empty() {
                model
                    .entries
                    .push(Entry::Notice("usage: /name <title>".into()));
            } else {
                model
                    .entries
                    .push(Entry::Notice(format!("session named '{arg}'")));
                model.session_title = arg;
            }
        }
        "/queue" | "/steer" => {
            if arg.is_empty() {
                model
                    .entries
                    .push(Entry::Notice(format!("usage: {cmd} <message>")));
            } else {
                let was_busy = model.busy;
                model.entries.push(Entry::User(arg.clone()));
                model.busy = true;
                model.scrollback = 0;
                let _ = session
                    .send(Command::UserMessage {
                        text: arg,
                        attachments: vec![],
                        stream_id: None,
                    })
                    .await;
                if was_busy {
                    model
                        .entries
                        .push(Entry::Notice("queued — runs after the current turn".into()));
                }
            }
        }
        "/goal" => {
            if arg.is_empty() && model.goal.is_empty() {
                model
                    .entries
                    .push(Entry::Notice("no goal set. usage: /goal <text>".into()));
            } else if arg.is_empty() {
                model
                    .entries
                    .push(Entry::Notice(format!("goal: {}", model.goal)));
            } else {
                model
                    .entries
                    .push(Entry::Notice(format!("goal set: {arg}")));
                model.goal = arg;
            }
        }
        "/reasoning" => {
            model.show_reasoning = !model.show_reasoning;
            model.entries.push(Entry::Notice(format!(
                "reasoning display {}",
                if model.show_reasoning { "on" } else { "off" }
            )));
        }
        "/cron" => {
            if model.cron_jobs.is_empty() {
                model.entries.push(Entry::Notice(
                    "no scheduled jobs. add one with `blumi cron add` from a shell".into(),
                ));
            } else {
                let mut s = String::from("scheduled jobs:");
                for (name, sched) in &model.cron_jobs {
                    s.push_str(&format!("\n- {name}: {sched}"));
                }
                s.push_str("\n(manage with `blumi cron` from a shell)");
                model.entries.push(Entry::Notice(s));
            }
        }
        "/model" => {
            if !arg.is_empty() {
                model.model_name = arg.clone();
                model.model_options.model = arg.clone();
                let _ = session.send(Command::SetModel { model: arg.clone() }).await;
                model.entries.push(Entry::Notice(format!("model → {arg}")));
            } else if model.model_options.models.is_empty() {
                model.entries.push(Entry::Notice(
                    "no suggested models for this provider — use /model <id>".into(),
                ));
            } else {
                model.dialog = Some(crate::dialog::Picker::model_picker(
                    &model.model_options.models,
                    &model.model_options.model,
                ));
            }
        }
        "/provider" => {
            if model.model_options.providers.is_empty() {
                model
                    .entries
                    .push(Entry::Notice("no providers configured".into()));
            } else {
                model.dialog = Some(crate::dialog::Picker::provider_picker(
                    &model.model_options.providers,
                    &model.model_options.provider,
                ));
            }
        }
        "/theme" => {
            if arg.is_empty() {
                model.cycle_theme();
            } else if model.set_theme(&arg) {
                model.entries.push(Entry::Notice(format!("theme: {arg}")));
            } else {
                model
                    .entries
                    .push(Entry::Notice(format!("unknown theme '{arg}'")));
            }
        }
        "/status" => model.entries.push(Entry::Notice(status_text(model))),
        "/stop" => {
            if model.busy {
                let _ = session.send(Command::Cancel).await;
            }
        }
        "/login" => model.entries.push(Entry::Notice(
            "run `blumi login` from a shell to add/switch providers".into(),
        )),
        "/quit" => model.should_quit = true,
        other => model.entries.push(Entry::Notice(format!(
            "unknown command '{other}' (try /help)"
        ))),
    }
}

fn help_text() -> String {
    let mut s = String::from("commands:");
    for c in COMMANDS {
        s.push_str(&format!("\n  {} — {}", c.name, c.desc));
    }
    s.push_str("\n(ctrl+p palette · tab focus · esc cancel/close · pgup/pgdn scroll)");
    s
}

fn status_text(model: &Model) -> String {
    format!(
        "session — persona: {} · model: {} · turns: {} · tokens ↑{} ↓{} · tasks: {} · approve: {} · dashboard: {}",
        model.persona,
        if model.model_name.is_empty() {
            "default"
        } else {
            &model.model_name
        },
        model.turn_count,
        model.input_tokens,
        model.output_tokens,
        model.todos.len(),
        if model.yolo { "auto" } else { "ask" },
        if model.show_dashboard { "on" } else { "off" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_filters_by_prefix() {
        let m = matching("/me");
        assert!(m.iter().any(|c| c.name == "/memory"));
        assert!(!m.iter().any(|c| c.name == "/model")); // /mo, not /me
                                                        // a full slash shows everything
        assert_eq!(matching("/").len(), COMMANDS.len());
    }
}

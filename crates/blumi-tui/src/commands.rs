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
        name: "/bg",
        desc: "run a task in the background (/bg <prompt>); result drops in when done",
    },
    CommandDef {
        name: "/tasks",
        desc: "toggle the right dashboard sidebar",
    },
    CommandDef {
        name: "/dashboard",
        desc: "open the full dashboard as a scrollable popup (all metrics)",
    },
    CommandDef {
        name: "/plans",
        desc: "browse proposed plans (● live · ✓ approved · ✗ rejected); ↑/↓ select, esc close",
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
        name: "/grid",
        desc: "show the grid: task distribution across local + remote peers",
    },
    CommandDef {
        name: "/accel",
        desc: "show the GPU/accelerator + embeddings execution provider",
    },
    CommandDef {
        name: "/loop",
        desc: "start/pause the autonomous task loop (/loop review to toggle gate)",
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
        desc: "toggle auto-approve — skip all permission prompts (ctrl+y)",
    },
    CommandDef {
        name: "/brain",
        desc: "local-LLM approvals: /brain off|advisory|auto",
    },
    CommandDef {
        name: "/plan",
        desc: "planning mode: /plan <task> to plan it, or /plan to toggle",
    },
    CommandDef {
        name: "/autocontinue",
        desc: "self-wake budget on the tool cap: /autocontinue <n> (0 off)",
    },
    CommandDef {
        name: "/remote",
        desc: "attach to a remote instance: /remote <name> | local | next",
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
        name: "/motion",
        desc: "motion effects: /motion [full|reduced|off]",
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

/// Toggle yolo (auto-approve): skip every permission prompt and run tools
/// straight away. Shared by the `/yolo` command and the Ctrl+Y shortcut, so
/// both stay in sync (flip local state, tell the core, announce it loudly).
pub(crate) async fn toggle_yolo(model: &mut Model, session: &SessionHandle) {
    model.yolo = !model.yolo;
    let _ = session.send(Command::SetYolo { on: model.yolo }).await;
    model.entries.push(Entry::Notice(if model.yolo {
        "⚡ yolo ON — tools run without asking (ctrl+y or /yolo to undo)".into()
    } else {
        "yolo off — tools will ask for approval".into()
    }));
    model.mark_dirty();
}

/// Run a slash command line (e.g. "/model claude-x").
pub async fn run(model: &mut Model, session: &SessionHandle, line: &str) {
    let line = line.trim().to_string();
    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim().to_string();
    model.clear_input();

    match cmd {
        "/help" => model.open_help_modal(),
        "/clear" => model.clear_transcript(),
        "/new" => model.request_new_session(),
        "/resume" => {
            if arg.is_empty() {
                model.dialog = Some(crate::dialog::Picker::session_picker(
                    &model.recent_sessions,
                    &model.remotes,
                ));
            } else {
                model.request_resume(arg);
            }
        }
        "/bg" => {
            if arg.is_empty() {
                model.entries.push(Entry::Notice(
                    "usage: /bg <prompt> — run a task in the background while you keep working"
                        .into(),
                ));
            } else {
                // The app loop owns session creation, so it spawns the job.
                model.bg_request = Some(arg.clone());
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
        "/tasks" => model.show_dashboard = !model.show_dashboard,
        "/dashboard" => model.toggle_dash_modal(),
        "/plans" => model.open_plans_view(),
        "/usage" => model.open_usage(),
        "/board" => model.open_board(),
        "/grid" => model.open_grid(),
        "/accel" => {
            let line = if model.accel.is_empty() {
                "accelerator: (unknown) — run `blumi accel doctor`".to_string()
            } else {
                model.accel.clone()
            };
            model.entries.push(Entry::Notice(line));
        }
        "/loop" => {
            if arg.eq_ignore_ascii_case("review") {
                model.loop_review = !model.loop_review;
                model.entries.push(Entry::Notice(format!(
                    "loop review-gate {}",
                    if model.loop_review { "on" } else { "off" }
                )));
            } else if model.loop_active {
                model.loop_active = false;
                model
                    .entries
                    .push(Entry::Notice("⏸ loop paused — /loop to resume".into()));
            } else if blumi_task::TaskBoard::load(&model.tasks_path)
                .next_todo()
                .is_none()
            {
                model.entries.push(Entry::Notice(
                    "no todo tasks — add some with `blumi task add` from a shell".into(),
                ));
            } else {
                if model.loop_current.is_none() {
                    model.loop_iter = 0;
                }
                model.loop_active = true;
                model.entries.push(Entry::Notice(
                    "⟳ loop started — working the task board · /loop to pause".into(),
                ));
            }
        }
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
                &model.remotes,
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
        "/yolo" => toggle_yolo(model, session).await,
        "/brain" => {
            if arg.is_empty() {
                model.entries.push(Entry::Notice(format!(
                    "🧠 brain {} — a local LLM reviews tool approvals. usage: /brain off|advisory|auto",
                    model.brain_mode
                )));
            } else if let Some(m) = blumi_core::BrainMode::parse(&arg) {
                model.brain_mode = m.label().to_string();
                let _ = session
                    .send(Command::SetBrainMode {
                        mode: m.label().into(),
                    })
                    .await;
                model.entries.push(Entry::Notice(
                    match m {
                        blumi_core::BrainMode::Off => "🧠 brain off — approvals ask you as usual",
                        blumi_core::BrainMode::Advisory => {
                            "🧠 brain advisory — it recommends; you still confirm"
                        }
                        blumi_core::BrainMode::Auto => {
                            "🧠 brain auto — it approves/denies for you (dangerous calls still ask)"
                        }
                    }
                    .into(),
                ));
            } else {
                model
                    .entries
                    .push(Entry::Notice("usage: /brain off|advisory|auto".into()));
            }
        }
        "/plan" => {
            if arg.is_empty() {
                // Toggle planning mode.
                model.plan_mode = !model.plan_mode;
                let _ = session
                    .send(Command::SetPlanMode {
                        on: model.plan_mode,
                    })
                    .await;
                model.entries.push(Entry::Notice(if model.plan_mode {
                    "◑ plan mode ON — blumi researches read-only and proposes a plan (ExitPlanMode) to approve".into()
                } else {
                    "plan mode off — changes no longer gated".into()
                }));
            } else if model.busy {
                model
                    .entries
                    .push(Entry::Notice("busy — press esc to cancel first".into()));
            } else {
                // Enter plan mode and kick off planning for the given task.
                model.plan_mode = true;
                let _ = session.send(Command::SetPlanMode { on: true }).await;
                let prompt = format!(
                    "Enter planning mode. Research the codebase READ-ONLY and do NOT make any \
                     changes yet. Produce a concise, numbered implementation plan, then call the \
                     ExitPlanMode tool with the full plan (markdown) for my approval.\n\nTask: {arg}"
                );
                model.entries.push(Entry::User(format!("◑ [plan] {arg}")));
                model.busy = true;
                model.scrollback = 0;
                let _ = session
                    .send(Command::UserMessage {
                        text: prompt,
                        attachments: vec![],
                        stream_id: None,
                    })
                    .await;
            }
        }
        "/autocontinue" | "/auto" => {
            if arg.is_empty() {
                model.entries.push(Entry::Notice(format!(
                    "↻ auto-continue: {} — when a turn hits the per-turn tool cap the runtime self-wakes (same session, bounded by tokens too). usage: /autocontinue <n>  (0 disables)",
                    if model.auto_continue == 0 {
                        "off".to_string()
                    } else {
                        format!("≤{} self-wakes", model.auto_continue)
                    }
                )));
            } else if let Ok(n) = arg.parse::<u32>() {
                model.auto_continue = n;
                let _ = session.send(Command::SetAutoContinue { n }).await;
                model.entries.push(Entry::Notice(if n == 0 {
                    "↻ auto-continue off — a turn that hits the tool cap stops and waits for you"
                        .into()
                } else {
                    format!("↻ auto-continue → ≤{n} self-wakes (still capped by the token budget)")
                }));
            } else {
                model.entries.push(Entry::Notice(
                    "usage: /autocontinue <n>   (n = max self-wakes on the tool cap, 0 disables)"
                        .into(),
                ));
            }
        }
        "/remote" => {
            let a = arg.trim();
            if a.is_empty() {
                let mut s = String::from("remote instances:");
                if model.remotes.is_empty() {
                    s.push_str("\n  (none configured — add under [remote] in settings.json)");
                } else {
                    for n in &model.remotes {
                        let open = model.tabs.iter().any(|(t, r)| *r && t == n);
                        s.push_str(&format!("\n  - {n}{}", if open { "  (open)" } else { "" }));
                    }
                }
                s.push_str("\n  usage: /remote <name> · /remote local · /remote next");
                model.entries.push(Entry::Notice(s));
            } else if a.eq_ignore_ascii_case("local") {
                model.request_tab(0);
            } else if a.eq_ignore_ascii_case("next") {
                model.cycle_tab();
            } else if model.remotes.iter().any(|n| n == a) {
                model.request_remote(a);
            } else {
                model.entries.push(Entry::Notice(format!(
                    "unknown remote '{a}' — /remote to list configured instances"
                )));
            }
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
        "/motion" => {
            use crate::motion::MotionLevel;
            let level = match arg.as_str() {
                "off" => Some(MotionLevel::Off),
                "reduced" | "low" => Some(MotionLevel::Reduced),
                "full" | "on" => Some(MotionLevel::Full),
                "" => Some(if model.motion.level() == MotionLevel::Off {
                    MotionLevel::Full
                } else {
                    MotionLevel::Off
                }),
                _ => None,
            };
            match level {
                Some(l) => {
                    model.motion.set_level(l);
                    if l != MotionLevel::Off {
                        model.motion.scene_in();
                    }
                    let name = match l {
                        MotionLevel::Full => "full",
                        MotionLevel::Reduced => "reduced",
                        MotionLevel::Off => "off",
                    };
                    model.entries.push(Entry::Notice(format!("motion: {name}")));
                }
                None => model
                    .entries
                    .push(Entry::Notice("usage: /motion [full|reduced|off]".into())),
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

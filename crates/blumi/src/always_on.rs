//! Always-on proactive discovery (PilotDeck-inspired): a gated, periodic pass
//! that asks the agent to surface candidate tasks for the workspace, adds them to
//! the board, and lands a markdown report + a `discovery` memory.
//!
//! A *sibling* of the SEDM sweep and the autonomous `blumi loop` — not a
//! replacement. **Off by default.** v1 is propose-only: the discovery turn runs
//! with `yolo = false`, so approval-requiring (mutating) tools are denied — it can
//! read + reason but not change anything. Autonomous low-risk *execution* (in a
//! git worktree / snapshot) is a deliberate follow-up.
//!
//! Safety: feature-gated (absent ⇒ never spawned); bounded by per-pass + open
//! caps + a rate-limit; the prompt never includes the grid secret; stored
//! discovery text is redacted; memories are `agent`-namespace + `origin="local"`
//! so they never diffuse off-node.

use crate::engine::build_session;
use crate::task::board_path;
use blumi_config::{AlwaysOnConfig, BlumiConfig, DiscoveryAutonomy};
use blumi_persist::SemanticMemoryImpl;
use blumi_protocol::{ApprovalScope, Command, Decision, Event};
use blumi_task::{TaskBoard, TaskState};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Title prefix that marks a board task as machine-discovered.
pub const DISCOVERED_MARKER: &str = "Discovered:";

/// Risk a discovered candidate carries (gates auto-run when that lands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Low,
    Risky,
}

impl Risk {
    pub fn label(&self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Risky => "risky",
        }
    }
    fn parse(s: &str) -> Risk {
        match s.trim().to_ascii_lowercase().as_str() {
            "risky" | "risk" | "high" => Risk::Risky,
            _ => Risk::Low,
        }
    }
}

/// One candidate task parsed from the discovery reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub priority: u8,
    pub risk: Risk,
    pub title: String,
    pub detail: String,
}

/// Periodic discovery, spawned as a background task by the gateway when enabled.
pub struct DiscoveryScheduler {
    config: BlumiConfig,
    mem: Option<Arc<SemanticMemoryImpl>>,
    last_run: StdMutex<Option<Instant>>,
}

impl DiscoveryScheduler {
    pub fn new(config: BlumiConfig, mem: Option<Arc<SemanticMemoryImpl>>) -> Self {
        DiscoveryScheduler {
            config,
            mem,
            last_run: StdMutex::new(None),
        }
    }

    /// Spawn the cadence loop (a sibling of the SEDM sweep). Cheap when idle.
    pub fn spawn(self: Arc<Self>) {
        let cadence = self.config.always_on.cadence_secs.max(60);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(cadence));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                if let Some(reason) = self.gate_blocked() {
                    tracing::debug!("always-on: skip ({reason})");
                    continue;
                }
                match self.discover_once().await {
                    Ok(n) if n > 0 => tracing::info!("always-on: proposed {n} task(s)"),
                    Ok(_) => tracing::debug!("always-on: no new candidates"),
                    Err(e) => tracing::warn!("always-on: pass failed: {e}"),
                }
                *self.last_run.lock().unwrap() = Some(Instant::now());
            }
        });
    }

    fn gate_blocked(&self) -> Option<&'static str> {
        let board = TaskBoard::load(board_path(&self.config));
        let todos = board.counts().todo as u32;
        let open = board
            .tasks()
            .iter()
            .filter(|t| is_discovered(t) && is_open(t))
            .count() as u32;
        let since = self.last_run.lock().unwrap().map(|t| t.elapsed().as_secs());
        evaluate_gates(&self.config.always_on, since, todos, open)
    }

    async fn discover_once(&self) -> anyhow::Result<usize> {
        let cfg = &self.config.always_on;
        let signals = self.build_signals();
        let prompt = build_prompt(cfg, &signals);

        // Bounded, read-only turn: `yolo = false` ⇒ mutating tools are denied.
        // Snapshot/restore the active router so building this transient session
        // doesn't clobber the main session's routing stats (Phase 1 global).
        let prev_router = blumi_core::active_router();
        let session = build_session(&self.config, false, None).await?;
        let reply = run_discovery_turn(&session, &prompt).await?;
        if let Some(r) = prev_router {
            blumi_core::set_active_router(r);
        }

        let candidates = parse_candidates(&reply, cfg.max_per_pass as usize);
        if candidates.is_empty() {
            return Ok(0);
        }

        let path = board_path(&self.config);
        let mut added = 0usize;
        for c in &candidates {
            // Re-load per add so concurrent edits aren't clobbered (loop pattern).
            let mut board = TaskBoard::load(&path);
            board.add(
                &format!("{DISCOVERED_MARKER} {}", c.title),
                &c.detail,
                c.priority,
                OffsetDateTime::now_utc(),
            );
            board.save().ok();
            added += 1;
        }

        let report = self.write_report(&signals, &candidates);
        if let Some(mem) = &self.mem {
            let titles = candidates
                .iter()
                .map(|c| c.title.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let summary = format!("discovered={added} ts={} :: {titles}", now_rfc3339());
            let _ = mem
                .add(
                    "agent",
                    "discovery",
                    &blumi_core::redact(&summary),
                    None,
                    "local",
                )
                .await;
        }
        tracing::info!("always-on: report at {}", report.display());
        Ok(added)
    }

    /// Read-only signals fed to the discovery prompt: board counts + git status.
    fn build_signals(&self) -> String {
        let board = TaskBoard::load(board_path(&self.config));
        let c = board.counts();
        let mut s = format!(
            "Board: {} todo, {} doing, {} review, {} done.\n",
            c.todo, c.doing, c.review, c.done
        );
        if let Ok(out) = std::process::Command::new("git")
            .args([
                "-C",
                &self.config.paths.working_dir.display().to_string(),
                "status",
                "--porcelain",
            ])
            .output()
        {
            if out.status.success() {
                let txt = String::from_utf8_lossy(&out.stdout);
                let lines: Vec<&str> = txt.lines().take(20).collect();
                if lines.is_empty() {
                    s.push_str("Working tree: clean.\n");
                } else {
                    s.push_str(&format!(
                        "Uncommitted changes ({}):\n{}\n",
                        lines.len(),
                        lines.join("\n")
                    ));
                }
            }
        }
        s
    }

    fn write_report(&self, signals: &str, candidates: &[Candidate]) -> PathBuf {
        let ts = OffsetDateTime::now_utc().unix_timestamp();
        let mut body = format!(
            "# Discovery report — {}\n\n## Signals\n\n{}\n\n## Proposed tasks\n\n",
            now_rfc3339(),
            signals.trim()
        );
        for c in candidates {
            body.push_str(&format!(
                "- **P{}** _{}_ — **{}**: {}\n",
                c.priority,
                c.risk.label(),
                c.title,
                c.detail
            ));
        }
        let body = blumi_core::redact(&body);

        let dir = self.config.paths.reports_dir();
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("discovery-{ts}.md"));
        atomic_write(&path, &body);
        if self.config.always_on.report_in_workspace {
            let wdir = self.config.paths.working_dir.join(".blumi").join("reports");
            std::fs::create_dir_all(&wdir).ok();
            atomic_write(&wdir.join(format!("discovery-{ts}.md")), &body);
        }
        path
    }
}

fn is_discovered(t: &blumi_task::Task) -> bool {
    t.title.starts_with(DISCOVERED_MARKER)
}

fn is_open(t: &blumi_task::Task) -> bool {
    matches!(t.state, TaskState::Todo | TaskState::Doing)
}

/// Pure gate predicate (unit-testable): `None` = run, `Some(reason)` = skip.
pub fn evaluate_gates(
    cfg: &AlwaysOnConfig,
    since_secs: Option<u64>,
    board_todos: u32,
    open_discoveries: u32,
) -> Option<&'static str> {
    if !cfg.enabled || cfg.autonomy == DiscoveryAutonomy::Off {
        return Some("disabled");
    }
    if let Some(since) = since_secs {
        if since < cfg.min_interval_secs {
            return Some("rate-limited");
        }
    }
    if cfg.skip_if_todos > 0 && board_todos >= cfg.skip_if_todos {
        return Some("board has todos");
    }
    if open_discoveries >= cfg.max_open_discoveries {
        return Some("discovery cap");
    }
    None
}

const BUILTIN_PROMPT: &str = "\
You are running an automated, read-only discovery pass for this workspace. Using \
only read-only investigation (do NOT modify anything), propose a few concrete, \
independently-shippable candidate tasks worth doing next — tests, docs, small \
fixes, follow-ups, tech-debt. Prefer high-signal, low-risk work.\n\n\
Output ONLY a fenced code block of one line per task, no prose, in EXACTLY this \
pipe-delimited format:\n\
P<1-4> | <low|risky> | <short title> | <one-line detail>\n\
where RISK is `risky` for anything touching config/providers/secrets/deletes or \
network sends, else `low`. Example:\n\
P2 | low | Add parser edge-case tests | cover empty + unicode inputs\n\n\
Context:\n{signals}";

fn build_prompt(cfg: &AlwaysOnConfig, signals: &str) -> String {
    let template = if cfg.prompt_template.trim().is_empty() {
        BUILTIN_PROMPT
    } else {
        cfg.prompt_template.as_str()
    };
    if template.contains("{signals}") {
        template.replace("{signals}", signals)
    } else {
        format!("{template}\n\nContext:\n{signals}")
    }
}

/// Parse candidate lines `P<n> | <low|risky> | title | detail` (tolerates code
/// fences + junk lines); truncates to `max`.
pub fn parse_candidates(reply: &str, max: usize) -> Vec<Candidate> {
    let mut out = Vec::new();
    for raw in reply.lines() {
        let line = raw.trim().trim_start_matches('-').trim();
        if line.starts_with("```") || line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').map(|p| p.trim()).collect();
        if parts.len() < 3 {
            continue;
        }
        let prio_tok = parts[0].trim_start_matches(['P', 'p']).trim();
        let Ok(priority) = prio_tok.parse::<u8>() else {
            continue;
        };
        let risk = Risk::parse(parts[1]);
        let title = parts[2].to_string();
        if title.is_empty() {
            continue;
        }
        let detail = parts.get(3).map(|s| s.to_string()).unwrap_or_default();
        out.push(Candidate {
            priority: priority.clamp(1, 4),
            risk,
            title,
            detail,
        });
        if out.len() >= max.max(1) {
            break;
        }
    }
    out
}

/// Drive one bounded, read-only discovery turn; collect the assistant text. Denies
/// any approval-requiring tool (mutations) and answers clarifies empty.
async fn run_discovery_turn(
    session: &blumi_core::SessionHandle,
    prompt: &str,
) -> anyhow::Result<String> {
    let mut events = session.subscribe();
    session
        .send(Command::UserMessage {
            text: prompt.to_string(),
            attachments: vec![],
            stream_id: None,
        })
        .await?;
    let mut text = String::new();
    loop {
        let env = events.recv().await?;
        match env.event {
            Event::Token { text: t } => text.push_str(&t),
            Event::ApprovalRequest { request_id, .. } => {
                session
                    .send(Command::ApproveTool {
                        request_id,
                        decision: Decision::Deny,
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
    Ok(text)
}

fn atomic_write(path: &std::path::Path, body: &str) {
    let tmp = path.with_extension("md.tmp");
    if std::fs::write(&tmp, body.as_bytes()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, autonomy: DiscoveryAutonomy) -> AlwaysOnConfig {
        AlwaysOnConfig {
            enabled,
            autonomy,
            ..Default::default()
        }
    }

    #[test]
    fn gates_block_when_disabled_or_off() {
        assert_eq!(
            evaluate_gates(&cfg(false, DiscoveryAutonomy::Propose), None, 0, 0),
            Some("disabled")
        );
        assert_eq!(
            evaluate_gates(&cfg(true, DiscoveryAutonomy::Off), None, 0, 0),
            Some("disabled")
        );
    }

    #[test]
    fn gates_rate_limit_board_and_cap() {
        let c = cfg(true, DiscoveryAutonomy::Propose); // min_interval 300, skip_if_todos 1, cap 5
                                                       // Just ran → rate-limited.
        assert_eq!(evaluate_gates(&c, Some(10), 0, 0), Some("rate-limited"));
        // Board already has a todo → skip.
        assert_eq!(evaluate_gates(&c, Some(999), 1, 0), Some("board has todos"));
        // Too many open discoveries → skip.
        assert_eq!(evaluate_gates(&c, Some(999), 0, 5), Some("discovery cap"));
        // Clear board, long since last run, room to discover → run.
        assert_eq!(evaluate_gates(&c, Some(999), 0, 0), None);
        // First run (no since) with a clear board → run.
        assert_eq!(evaluate_gates(&c, None, 0, 0), None);
    }

    #[test]
    fn parse_candidates_parses_clamps_and_truncates() {
        let reply = "```\n\
            P2 | low | Add parser tests | cover edge cases\n\
            - P9 | risky | Rotate API keys | touches secrets\n\
            garbage line without pipes\n\
            P1 | low | Fix typo in README\n\
            ```";
        let cands = parse_candidates(reply, 10);
        assert_eq!(cands.len(), 3);
        assert_eq!(cands[0].priority, 2);
        assert_eq!(cands[0].risk, Risk::Low);
        assert_eq!(cands[0].title, "Add parser tests");
        assert_eq!(cands[1].priority, 4); // 9 clamped to 4
        assert_eq!(cands[1].risk, Risk::Risky);
        assert_eq!(cands[2].detail, ""); // missing detail tolerated
    }

    #[test]
    fn parse_candidates_truncates_to_max() {
        let reply = "P1 | low | a | x\nP1 | low | b | y\nP1 | low | c | z";
        assert_eq!(parse_candidates(reply, 2).len(), 2);
    }

    #[test]
    fn build_prompt_fills_signals() {
        let c = cfg(true, DiscoveryAutonomy::Propose);
        let p = build_prompt(&c, "BOARD-STATE");
        assert!(p.contains("BOARD-STATE"));
        assert!(!p.contains("{signals}"));
    }
}

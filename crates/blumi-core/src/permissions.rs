//! Capability-based permission engine.
//!
//! Decides whether a tool call may proceed: short-circuits (yolo / read-only /
//! remembered), per-tool allow/deny globs, destructive-command detection, a
//! safe-read-only-command allowlist, and otherwise asks the user (remembering
//! the answer for the session when the user chooses that scope). Ported from
//! OpenMono's `PermissionEngine`.

use crate::brain::{Brain, BrainDecision, BrainMode};
use crate::emit::{EventEmitter, Interactor};
use blumi_config::PermissionConfig;
use blumi_protocol::{ApprovalScope, Decision, Event};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// The result of a permission check for one tool call.
#[derive(Debug)]
pub enum PermissionOutcome {
    Allow,
    Deny(String),
}

/// Internal pre-prompt classification.
enum Class {
    Allow,
    Deny(String),
    Ask { dangerous: bool, subject: String },
}

pub struct PermissionEngine {
    config: PermissionConfig,
    /// Auto-approve everything (yolo). Seeded from config, toggleable at runtime
    /// via [`set_yolo`](Self::set_yolo) (the `/yolo` command).
    yolo: AtomicBool,
    /// Tools the user approved for the rest of the session.
    remembered: Mutex<HashSet<String>>,
    /// Optional local-LLM "brain" that reviews otherwise-prompted calls.
    brain: Option<Arc<dyn Brain>>,
    /// How the brain participates (off/advisory/auto); runtime-toggleable via
    /// the `/brain` command. Encoded as [`BrainMode::as_u8`].
    brain_mode: AtomicU8,
    /// Planning mode: block mutating tools so the agent researches read-only
    /// and proposes a plan (via `ExitPlanMode`) before changing anything.
    plan_mode: AtomicBool,
}

impl PermissionEngine {
    pub fn new(config: PermissionConfig) -> Self {
        let yolo = AtomicBool::new(config.yolo);
        PermissionEngine {
            config,
            yolo,
            remembered: Mutex::new(HashSet::new()),
            brain: None,
            brain_mode: AtomicU8::new(BrainMode::Off.as_u8()),
            plan_mode: AtomicBool::new(false),
        }
    }

    /// Attach a local-LLM brain and its initial mode (builder style).
    pub fn with_brain(mut self, brain: Arc<dyn Brain>, mode: BrainMode) -> Self {
        self.brain = Some(brain);
        self.brain_mode = AtomicU8::new(mode.as_u8());
        self
    }

    /// Turn auto-approve-all on or off at runtime.
    pub fn set_yolo(&self, on: bool) {
        self.yolo.store(on, Ordering::Relaxed);
    }

    /// Whether auto-approve-all is currently on.
    pub fn is_yolo(&self) -> bool {
        self.yolo.load(Ordering::Relaxed)
    }

    /// Set the brain approval mode at runtime (the `/brain` command).
    pub fn set_brain_mode(&self, mode: BrainMode) {
        self.brain_mode.store(mode.as_u8(), Ordering::Relaxed);
    }

    /// The current brain approval mode.
    pub fn brain_mode(&self) -> BrainMode {
        BrainMode::from_u8(self.brain_mode.load(Ordering::Relaxed))
    }

    /// Whether a brain is attached (so a UI can offer the `/brain` toggle).
    pub fn has_brain(&self) -> bool {
        self.brain.is_some()
    }

    /// Enter/leave planning mode (mutating tools blocked).
    pub fn set_plan_mode(&self, on: bool) {
        self.plan_mode.store(on, Ordering::Relaxed);
    }

    /// Whether planning mode is on.
    pub fn is_plan_mode(&self) -> bool {
        self.plan_mode.load(Ordering::Relaxed)
    }

    /// Check a tool call, prompting the user if policy is inconclusive. The
    /// brain (when enabled) is consulted only on the otherwise-prompted path;
    /// `events` carries its auto-mode decisions to the UI as notices.
    pub async fn check(
        &self,
        tool_name: &str,
        is_read_only: bool,
        input: &Value,
        interactor: &Interactor,
        events: &EventEmitter,
    ) -> PermissionOutcome {
        match self.classify(tool_name, is_read_only, input) {
            Class::Allow => PermissionOutcome::Allow,
            Class::Deny(reason) => PermissionOutcome::Deny(reason),
            Class::Ask { dangerous, subject } => {
                if self.remembered.lock().unwrap().contains(tool_name) {
                    return PermissionOutcome::Allow;
                }
                let summary = if subject.is_empty() {
                    tool_name.to_string()
                } else {
                    format!("{tool_name}: {subject}")
                };

                // Consult the brain on the path that would otherwise prompt.
                let mut advice: Option<String> = None;
                if let Some(outcome) = self
                    .consult_brain(
                        tool_name,
                        &subject,
                        &summary,
                        dangerous,
                        input,
                        events,
                        &mut advice,
                    )
                    .await
                {
                    return outcome;
                }

                let (decision, scope) = interactor
                    .approve(tool_name, summary, dangerous, None, advice)
                    .await;
                match decision {
                    Decision::Allow => {
                        if scope == ApprovalScope::Session {
                            self.remembered
                                .lock()
                                .unwrap()
                                .insert(tool_name.to_string());
                        }
                        PermissionOutcome::Allow
                    }
                    Decision::Deny => PermissionOutcome::Deny("denied by user".into()),
                }
            }
        }
    }

    /// Ask the brain about a call that policy would otherwise prompt for.
    ///
    /// Returns `Some(outcome)` when the brain *resolves* the call (auto mode:
    /// a clear allow on a non-dangerous call, or a deny), emitting a notice so
    /// the decision is visible. Returns `None` to fall through to the human —
    /// setting `*advice` to the brain's recommendation so the approval card can
    /// show it (both advisory mode and auto-mode escalations). A dangerous call
    /// never auto-allows: it always escalates with the brain's note attached.
    #[allow(clippy::too_many_arguments)]
    async fn consult_brain(
        &self,
        tool_name: &str,
        subject: &str,
        summary: &str,
        dangerous: bool,
        input: &Value,
        events: &EventEmitter,
        advice: &mut Option<String>,
    ) -> Option<PermissionOutcome> {
        let mode = self.brain_mode();
        if mode == BrainMode::Off {
            return None;
        }
        let brain = self.brain.as_ref()?;
        let verdict = brain.review(tool_name, subject, dangerous, input).await;
        let word = decision_word(verdict.decision);

        match mode {
            BrainMode::Auto => match verdict.decision {
                BrainDecision::Allow if !dangerous => {
                    events.emit(Event::Notice {
                        message: format!("🧠 brain approved {summary} — {}", verdict.reason),
                    });
                    Some(PermissionOutcome::Allow)
                }
                BrainDecision::Deny => {
                    events.emit(Event::Notice {
                        message: format!("🧠 brain blocked {summary} — {}", verdict.reason),
                    });
                    Some(PermissionOutcome::Deny(format!(
                        "brain: {}",
                        verdict.reason
                    )))
                }
                // Allow-but-dangerous, or escalate → hand to the human with a note.
                _ => {
                    *advice = Some(format!("🧠 brain: {word} — {}", verdict.reason));
                    None
                }
            },
            BrainMode::Advisory => {
                *advice = Some(format!("🧠 brain suggests {word} — {}", verdict.reason));
                None
            }
            BrainMode::Off => None,
        }
    }

    fn classify(&self, tool_name: &str, is_read_only: bool, input: &Value) -> Class {
        // ExitPlanMode runs its own approval (the plan modal); never gate it,
        // and let it through even in plan mode so the agent can submit a plan.
        if tool_name == "ExitPlanMode" {
            return Class::Allow;
        }
        // Planning mode: block any mutating tool, even under yolo — the whole
        // point is to research read-only and propose a plan first.
        if self.is_plan_mode() && !is_read_only {
            return Class::Deny(
                "plan mode is on (read-only): research, then propose changes via ExitPlanMode"
                    .into(),
            );
        }
        if self.is_yolo() {
            return Class::Allow;
        }
        // Reads never mutate; allow by default.
        if is_read_only {
            return Class::Allow;
        }

        let subject = subject_of(tool_name, input);
        let rules = self.config.tools.get(tool_name);

        // Explicit deny wins over everything below.
        if let Some(rules) = rules {
            if matches_any(&rules.deny, &subject) {
                return Class::Deny(format!("denied by policy: {subject}"));
            }
        }

        // Destructive shell commands always prompt (flagged dangerous), even if
        // an allow rule would otherwise match.
        if tool_name == "Bash" && is_destructive(&subject) {
            return Class::Ask {
                dangerous: true,
                subject,
            };
        }

        // Explicit allow.
        if let Some(rules) = rules {
            if matches_any(&rules.allow, &subject) {
                return Class::Allow;
            }
        }

        // Known-safe read-only shell commands.
        if tool_name == "Bash" && is_safe_command(&subject) {
            return Class::Allow;
        }

        Class::Ask {
            dangerous: false,
            subject,
        }
    }
}

/// A short verb for a brain decision, used in advisory/notice text.
fn decision_word(d: BrainDecision) -> &'static str {
    match d {
        BrainDecision::Allow => "allow",
        BrainDecision::Deny => "deny",
        BrainDecision::Escalate => "asks you",
    }
}

/// Extract the matchable subject (command or path) from tool input.
fn subject_of(tool_name: &str, input: &Value) -> String {
    let field = match tool_name {
        "Bash" => "command",
        "FileWrite" | "FileEdit" | "FileRead" | "ApplyPatch" | "ListDirectory" => "path",
        _ => return String::new(),
    };
    input
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn matches_any(patterns: &[String], subject: &str) -> bool {
    patterns.iter().any(|p| {
        p == subject
            || glob::Pattern::new(p)
                .map(|pat| pat.matches(subject))
                .unwrap_or(false)
    })
}

/// Heuristic detection of dangerous shell commands.
fn is_destructive(cmd: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "rm -rf",
        "rm -fr",
        "rm -r /",
        "sudo ",
        "mkfs",
        "dd if=",
        ":(){",
        "git reset --hard",
        "git clean -",
        "> /dev/sd",
        "chmod -R 777",
        "shutdown",
        "reboot",
        "git push --force",
        "git push -f",
    ];
    let c = cmd.trim();
    NEEDLES.iter().any(|n| c.contains(n))
}

/// Whether a shell command is a simple, read-only invocation we can auto-allow.
fn is_safe_command(cmd: &str) -> bool {
    // Any shell metacharacter could chain into something unsafe — don't auto-allow.
    if cmd.contains("&&")
        || cmd.contains("||")
        || cmd.contains(';')
        || cmd.contains('|')
        || cmd.contains('>')
        || cmd.contains('<')
        || cmd.contains('`')
        || cmd.contains("$(")
    {
        return false;
    }
    const SAFE: &[&str] = &[
        "ls", "cat", "pwd", "echo", "head", "tail", "grep", "rg", "find", "wc", "which", "whoami",
        "date", "stat", "file", "tree", "env", "sort", "uniq", "basename", "dirname",
    ];
    let first = cmd.split_whitespace().next().unwrap_or("");
    SAFE.contains(&first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::BrainVerdict;
    use crate::emit::{EventEmitter, InteractionKind, InteractionReply, Interactor};
    use async_trait::async_trait;
    use blumi_config::ToolPermissionRules;
    use serde_json::json;
    use tokio::sync::mpsc;

    fn engine() -> PermissionEngine {
        PermissionEngine::new(PermissionConfig::default())
    }

    /// A drain-to-nowhere emitter plus the receiver kept alive so notices land.
    fn mock_events() -> (EventEmitter, mpsc::UnboundedReceiver<Event>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (EventEmitter::new(tx), rx)
    }

    /// A brain that always returns a fixed verdict (no LLM).
    struct FixedBrain(BrainDecision);
    #[async_trait]
    impl Brain for FixedBrain {
        async fn review(&self, _t: &str, _s: &str, _d: bool, _i: &Value) -> BrainVerdict {
            BrainVerdict {
                decision: self.0,
                reason: "test".into(),
            }
        }
    }

    fn brain_engine(decision: BrainDecision, mode: BrainMode) -> PermissionEngine {
        PermissionEngine::new(PermissionConfig::default())
            .with_brain(Arc::new(FixedBrain(decision)), mode)
    }

    #[test]
    fn yolo_allows_everything() {
        let e = PermissionEngine::new(PermissionConfig {
            yolo: true,
            ..Default::default()
        });
        assert!(matches!(
            e.classify("Bash", false, &json!({ "command": "rm -rf /" })),
            Class::Allow
        ));
    }

    #[test]
    fn yolo_toggles_at_runtime() {
        let e = engine(); // starts off
        assert!(matches!(
            e.classify("FileWrite", false, &json!({ "path": "src/x.rs" })),
            Class::Ask { .. }
        ));
        e.set_yolo(true);
        assert!(e.is_yolo());
        assert!(matches!(
            e.classify("FileWrite", false, &json!({ "path": "src/x.rs" })),
            Class::Allow
        ));
        e.set_yolo(false);
        assert!(!e.is_yolo());
        assert!(matches!(
            e.classify("FileWrite", false, &json!({ "path": "src/x.rs" })),
            Class::Ask { .. }
        ));
    }

    #[test]
    fn plan_mode_blocks_writes_allows_reads_and_exit() {
        let e = engine();
        e.set_plan_mode(true);
        // Mutating tools are denied (even normally-safe bash).
        assert!(matches!(
            e.classify("FileWrite", false, &json!({ "path": "a" })),
            Class::Deny(_)
        ));
        assert!(matches!(
            e.classify("Bash", false, &json!({ "command": "ls" })),
            Class::Deny(_)
        ));
        // Reads still go through.
        assert!(matches!(
            e.classify("FileRead", true, &json!({})),
            Class::Allow
        ));
        // ExitPlanMode is always allowed (it runs its own approval).
        assert!(matches!(
            e.classify("ExitPlanMode", false, &json!({ "plan": "x" })),
            Class::Allow
        ));
        // Leaving plan mode restores normal gating.
        e.set_plan_mode(false);
        assert!(matches!(
            e.classify("Bash", false, &json!({ "command": "ls" })),
            Class::Allow
        ));
    }

    #[test]
    fn read_only_is_allowed() {
        assert!(matches!(
            engine().classify("FileRead", true, &json!({})),
            Class::Allow
        ));
    }

    #[test]
    fn destructive_bash_is_dangerous_ask() {
        match engine().classify("Bash", false, &json!({ "command": "rm -rf build" })) {
            Class::Ask { dangerous, .. } => assert!(dangerous),
            _ => panic!("expected dangerous ask"),
        }
    }

    #[test]
    fn safe_command_is_allowed() {
        assert!(matches!(
            engine().classify("Bash", false, &json!({ "command": "ls -la src" })),
            Class::Allow
        ));
        // piping disqualifies the safe path
        assert!(matches!(
            engine().classify("Bash", false, &json!({ "command": "ls | rm" })),
            Class::Ask { .. }
        ));
    }

    #[test]
    fn deny_rule_blocks() {
        let mut cfg = PermissionConfig::default();
        cfg.tools.insert(
            "FileWrite".into(),
            ToolPermissionRules {
                deny: vec!["**/.env".into()],
                ..Default::default()
            },
        );
        let e = PermissionEngine::new(cfg);
        assert!(matches!(
            e.classify("FileWrite", false, &json!({ "path": "config/.env" })),
            Class::Deny(_)
        ));
    }

    #[test]
    fn unknown_write_defaults_to_ask() {
        assert!(matches!(
            engine().classify("FileWrite", false, &json!({ "path": "src/main.rs" })),
            Class::Ask {
                dangerous: false,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn session_scope_is_remembered() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let interactor = Interactor::new(tx);
        let e = engine();

        // Fake actor: reply Allow + Session once. After that it must not be asked again.
        let actor = tokio::spawn(async move {
            let req = rx.recv().await.unwrap();
            assert!(matches!(req.kind, InteractionKind::Approval { .. }));
            req.respond
                .send(InteractionReply::Approval {
                    decision: Decision::Allow,
                    scope: ApprovalScope::Session,
                })
                .unwrap();
            // If asked again, this second recv would block forever; assert none arrives.
            assert!(rx.recv().await.is_none());
        });

        let (events, _rx) = mock_events();
        let input = json!({ "command": "make build" });
        assert!(matches!(
            e.check("Bash", false, &input, &interactor, &events).await,
            PermissionOutcome::Allow
        ));
        // Second check should be auto-allowed without prompting.
        assert!(matches!(
            e.check("Bash", false, &input, &interactor, &events).await,
            PermissionOutcome::Allow
        ));

        drop(interactor); // closes the channel so the fake actor's final recv returns None
        actor.await.unwrap();
    }

    /// Auto mode: a clear allow resolves the call with no prompt and a notice.
    #[tokio::test]
    async fn brain_auto_allows_without_prompt() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let interactor = Interactor::new(tx);
        let (events, mut erx) = mock_events();
        let e = brain_engine(BrainDecision::Allow, BrainMode::Auto);

        let out = e
            .check(
                "FileWrite",
                false,
                &json!({ "path": "src/x.rs" }),
                &interactor,
                &events,
            )
            .await;
        assert!(matches!(out, PermissionOutcome::Allow));
        // No approval prompt was emitted.
        assert!(rx.try_recv().is_err());
        // A brain notice was emitted.
        assert!(matches!(erx.try_recv(), Ok(Event::Notice { .. })));
    }

    /// Auto mode: a deny resolves the call as denied, no prompt.
    #[tokio::test]
    async fn brain_auto_denies() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let interactor = Interactor::new(tx);
        let (events, _erx) = mock_events();
        let e = brain_engine(BrainDecision::Deny, BrainMode::Auto);

        let out = e
            .check(
                "FileWrite",
                false,
                &json!({ "path": "src/x.rs" }),
                &interactor,
                &events,
            )
            .await;
        assert!(matches!(out, PermissionOutcome::Deny(_)));
        assert!(rx.try_recv().is_err());
    }

    /// Auto mode: a dangerous command escalates even when the brain says allow.
    #[tokio::test]
    async fn brain_auto_escalates_dangerous() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let interactor = Interactor::new(tx);
        let (events, _erx) = mock_events();
        let e = brain_engine(BrainDecision::Allow, BrainMode::Auto);

        // Drive the prompt: the actor must receive an approval with advice set.
        let actor = tokio::spawn(async move {
            let req = rx.recv().await.unwrap();
            match req.kind {
                InteractionKind::Approval {
                    dangerous, advice, ..
                } => {
                    assert!(dangerous);
                    assert!(advice.unwrap().contains("brain"));
                }
                _ => panic!("expected approval"),
            }
            req.respond
                .send(InteractionReply::Approval {
                    decision: Decision::Deny,
                    scope: ApprovalScope::Once,
                })
                .unwrap();
        });

        let out = e
            .check(
                "Bash",
                false,
                &json!({ "command": "rm -rf build" }),
                &interactor,
                &events,
            )
            .await;
        assert!(matches!(out, PermissionOutcome::Deny(_)));
        actor.await.unwrap();
    }

    /// Advisory mode: always prompts, attaching the brain's recommendation.
    #[tokio::test]
    async fn brain_advisory_attaches_advice() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let interactor = Interactor::new(tx);
        let (events, _erx) = mock_events();
        let e = brain_engine(BrainDecision::Allow, BrainMode::Advisory);

        let actor = tokio::spawn(async move {
            let req = rx.recv().await.unwrap();
            match req.kind {
                InteractionKind::Approval { advice, .. } => {
                    assert!(advice.unwrap().contains("brain suggests allow"));
                }
                _ => panic!("expected approval"),
            }
            req.respond
                .send(InteractionReply::Approval {
                    decision: Decision::Allow,
                    scope: ApprovalScope::Once,
                })
                .unwrap();
        });

        let out = e
            .check(
                "FileWrite",
                false,
                &json!({ "path": "src/x.rs" }),
                &interactor,
                &events,
            )
            .await;
        assert!(matches!(out, PermissionOutcome::Allow));
        actor.await.unwrap();
    }
}

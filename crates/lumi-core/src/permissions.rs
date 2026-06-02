//! Capability-based permission engine.
//!
//! Decides whether a tool call may proceed: short-circuits (yolo / read-only /
//! remembered), per-tool allow/deny globs, destructive-command detection, a
//! safe-read-only-command allowlist, and otherwise asks the user (remembering
//! the answer for the session when the user chooses that scope). Ported from
//! OpenMono's `PermissionEngine`.

use crate::emit::Interactor;
use lumi_config::PermissionConfig;
use lumi_protocol::{ApprovalScope, Decision};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Mutex;

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
    /// Tools the user approved for the rest of the session.
    remembered: Mutex<HashSet<String>>,
}

impl PermissionEngine {
    pub fn new(config: PermissionConfig) -> Self {
        PermissionEngine {
            config,
            remembered: Mutex::new(HashSet::new()),
        }
    }

    /// Check a tool call, prompting the user if policy is inconclusive.
    pub async fn check(
        &self,
        tool_name: &str,
        is_read_only: bool,
        input: &Value,
        interactor: &Interactor,
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
                let (decision, scope) = interactor
                    .approve(tool_name, summary, dangerous, None)
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

    fn classify(&self, tool_name: &str, is_read_only: bool, input: &Value) -> Class {
        if self.config.yolo {
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
    use crate::emit::{InteractionKind, InteractionReply, Interactor};
    use lumi_config::ToolPermissionRules;
    use serde_json::json;
    use tokio::sync::mpsc;

    fn engine() -> PermissionEngine {
        PermissionEngine::new(PermissionConfig::default())
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

        let input = json!({ "command": "make build" });
        assert!(matches!(
            e.check("Bash", false, &input, &interactor).await,
            PermissionOutcome::Allow
        ));
        // Second check should be auto-allowed without prompting.
        assert!(matches!(
            e.check("Bash", false, &input, &interactor).await,
            PermissionOutcome::Allow
        ));

        drop(interactor); // closes the channel so the fake actor's final recv returns None
        actor.await.unwrap();
    }
}

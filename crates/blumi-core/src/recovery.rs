//! Reflex self-healing (arXiv 2606.01416): classify a failed tool result, pick a
//! budgeted recovery action, and emit targeted guidance for the model's next
//! step — failure-taxonomy → targeted-action → trace. This pairs with the
//! doom-loop guard (which owns *identical-repeat* loops); the controller owns
//! *classified* failures and never re-executes tools itself (it guides the model,
//! so there's no at-least-once double-side-effect risk).

use blumi_protocol::ResultClass;

/// Bounded recovery attempts for one turn (generalizes the doom-loop counters).
#[derive(Debug)]
pub struct RecoveryBudget {
    remaining: u32,
}

impl RecoveryBudget {
    pub fn new(n: u32) -> Self {
        RecoveryBudget { remaining: n }
    }
    pub fn remaining(&self) -> u32 {
        self.remaining
    }
    pub fn exhausted(&self) -> bool {
        self.remaining == 0
    }
    /// Consume one attempt; returns false if the budget was already empty.
    pub fn spend(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

/// The targeted recovery action chosen for a failure class (the policy table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Fix the arguments and retry (InvalidInput).
    ArgFix,
    /// Re-read the changed state, then retry (StateConflict).
    StateRepair,
    /// Retry with the tool's hint — only for idempotent (read-only) tools (Crash).
    RetryWithHint,
    /// Try an alternative tool or a narrower query (Empty result).
    AlternativeOrNarrow,
    /// Don't auto-recover — surface to the model to choose a different approach
    /// (permission/cancel, or a crash on a *mutating* tool where a blind retry
    /// could double a side effect).
    Escalate,
}

impl RecoveryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            RecoveryAction::ArgFix => "arg_fix",
            RecoveryAction::StateRepair => "state_repair",
            RecoveryAction::RetryWithHint => "retry_with_hint",
            RecoveryAction::AlternativeOrNarrow => "alternative_or_narrow",
            RecoveryAction::Escalate => "escalate",
        }
    }
}

/// Lower-case machine string for a failure class (used in traces + episodes).
pub fn class_str(class: ResultClass) -> &'static str {
    match class {
        ResultClass::Success => "success",
        ResultClass::InvalidInput => "invalid_input",
        ResultClass::PermissionDenied => "permission_denied",
        ResultClass::StateConflict => "state_conflict",
        ResultClass::Crash => "crash",
        ResultClass::Empty => "empty",
        ResultClass::Cancelled => "cancelled",
    }
}

/// Whether a failure class is worth a budgeted recovery attempt at all.
/// Permission/Cancelled/Success are never auto-recovered.
pub fn is_recoverable(class: ResultClass) -> bool {
    matches!(
        class,
        ResultClass::InvalidInput
            | ResultClass::StateConflict
            | ResultClass::Crash
            | ResultClass::Empty
    )
}

/// Map a failure class + tool idempotency to a recovery action.
pub fn action_for(class: ResultClass, idempotent: bool) -> RecoveryAction {
    match class {
        ResultClass::InvalidInput => RecoveryAction::ArgFix,
        ResultClass::StateConflict => RecoveryAction::StateRepair,
        // A crash is only safe to blind-retry on an idempotent (read-only) tool;
        // a mutating tool may have half-applied a side effect → escalate instead.
        ResultClass::Crash if idempotent => RecoveryAction::RetryWithHint,
        ResultClass::Crash => RecoveryAction::Escalate,
        ResultClass::Empty => RecoveryAction::AlternativeOrNarrow,
        _ => RecoveryAction::Escalate,
    }
}

/// Build the corrective guidance injected as a trailing user message so the
/// model takes the recovery action on its next step. Prompt-cache safe (it's a
/// window-only message, never the cached system prefix).
pub fn guidance(tool: &str, action: RecoveryAction, hint: Option<&str>) -> String {
    let base = match action {
        RecoveryAction::ArgFix => format!(
            "The `{tool}` call failed because the arguments were invalid. Correct the \
             arguments and call it again."
        ),
        RecoveryAction::StateRepair => format!(
            "The `{tool}` call failed due to a state conflict (something changed since you \
             last read it). Re-read the current state, then retry with up-to-date values."
        ),
        RecoveryAction::RetryWithHint => {
            format!("The `{tool}` call failed but the error looks transient. Retry it once.")
        }
        RecoveryAction::AlternativeOrNarrow => format!(
            "The `{tool}` call returned nothing useful. Try a different tool or a narrower, \
             more specific query."
        ),
        RecoveryAction::Escalate => format!(
            "The `{tool}` call failed and should not be blindly retried. Choose a different \
             approach; do not repeat the same call with the same arguments."
        ),
    };
    let mut s = String::from("[Self-healing — recovery guidance]\n");
    s.push_str(&base);
    if let Some(h) = hint {
        let h = h.trim();
        if !h.is_empty() {
            s.push_str("\nTool hint: ");
            s.push_str(h);
        }
    }
    s
}

/// Compact a failure into a stable episode line for memory (the `kind="failure"`
/// / `kind="recovery"` text). Redaction is applied separately by [`redact`].
pub fn episode_text(
    tool: &str,
    class: ResultClass,
    action: RecoveryAction,
    outcome: &str,
) -> String {
    format!(
        "tool={tool} failure={} action={} outcome={outcome}",
        class_str(class),
        action.as_str()
    )
}

/// Redact absolute paths + secret-looking tokens from failure text before it is
/// stored as a memory episode (privacy for cross-grid diffusion). Best-effort,
/// dependency-free: collapses `/abs/paths` to their basename and masks long
/// opaque tokens (api keys, hashes).
pub fn redact(s: &str) -> String {
    s.split_whitespace()
        .map(|tok| {
            // Mask common secret prefixes outright.
            if tok.starts_with("sk-") || tok.starts_with("ghp_") || tok.starts_with("xoxb-") {
                return "<redacted>".to_string();
            }
            // Absolute unix path → keep only the final component.
            if tok.starts_with('/') && tok.len() > 1 {
                if let Some(base) = tok.rsplit('/').next() {
                    return format!("…/{base}");
                }
            }
            // Long opaque alphanumeric run (>=24 chars, no spaces) → likely a
            // token/hash; mask it.
            let alnum = tok.chars().filter(|c| c.is_ascii_alphanumeric()).count();
            if tok.len() >= 24 && alnum >= 20 && !tok.contains('/') {
                return "<token>".to_string();
            }
            tok.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_spends_then_exhausts() {
        let mut b = RecoveryBudget::new(2);
        assert!(!b.exhausted());
        assert!(b.spend());
        assert!(b.spend());
        assert!(!b.spend());
        assert!(b.exhausted());
        assert_eq!(b.remaining(), 0);
    }

    #[test]
    fn policy_table_maps_classes() {
        assert_eq!(
            action_for(ResultClass::InvalidInput, true),
            RecoveryAction::ArgFix
        );
        assert_eq!(
            action_for(ResultClass::StateConflict, false),
            RecoveryAction::StateRepair
        );
        // Crash: retry only when idempotent, else escalate.
        assert_eq!(
            action_for(ResultClass::Crash, true),
            RecoveryAction::RetryWithHint
        );
        assert_eq!(
            action_for(ResultClass::Crash, false),
            RecoveryAction::Escalate
        );
        assert_eq!(
            action_for(ResultClass::Empty, true),
            RecoveryAction::AlternativeOrNarrow
        );
        // Never auto-recover these.
        assert!(!is_recoverable(ResultClass::PermissionDenied));
        assert!(!is_recoverable(ResultClass::Cancelled));
        assert!(!is_recoverable(ResultClass::Success));
        assert!(is_recoverable(ResultClass::Crash));
    }

    #[test]
    fn redact_masks_paths_and_tokens() {
        let r = redact("failed at /Users/ankur/secret/app.rs with key sk-abcdef123456");
        assert!(r.contains("…/app.rs"), "{r}");
        assert!(r.contains("<redacted>"), "{r}");
        assert!(!r.contains("/Users/ankur"), "{r}");
        let r2 = redact("token abcdefghijklmnopqrstuvwxyz0123");
        assert!(r2.contains("<token>"), "{r2}");
    }

    #[test]
    fn guidance_includes_tool_and_hint() {
        let g = guidance(
            "file_write",
            RecoveryAction::ArgFix,
            Some("path is required"),
        );
        assert!(g.contains("file_write"));
        assert!(g.contains("path is required"));
        assert!(g.starts_with("[Self-healing"));
    }
}

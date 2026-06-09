//! Commands a UI sends into a session actor. The only way to drive a turn.

use crate::ids::{RequestId, StreamId};
use serde::{Deserialize, Serialize};

/// Outcome of an approval prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

/// How long an approval decision applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// Just this one call.
    #[default]
    Once,
    /// Remember for the rest of the session.
    Session,
}

/// An instruction to a session actor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Start a new turn with a user message.
    UserMessage {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<String>,
        /// Set by the web server to own the resulting SSE stream.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<StreamId>,
    },
    /// Cancel the in-flight turn.
    Cancel,
    /// Resolve a pending [`crate::Event::ApprovalRequest`].
    ApproveTool {
        request_id: RequestId,
        decision: Decision,
        #[serde(default)]
        scope: ApprovalScope,
    },
    /// Resolve a pending [`crate::Event::ClarifyRequest`].
    AnswerClarify {
        request_id: RequestId,
        value: String,
    },
    /// Switch the active model mid-session.
    SetModel { model: String },
    /// Toggle auto-approve-all (yolo) at runtime.
    SetYolo { on: bool },
    /// Set the local-LLM "brain" approval mode at runtime (`off`/`advisory`/
    /// `auto`). Unknown values are ignored by the core.
    SetBrainMode { mode: String },
    /// Set cost-aware routing mode at runtime (`off`/`heuristic`/`hybrid`/
    /// `judge`). Unknown values are ignored by the core.
    SetRouterMode { mode: String },
    /// Enter/leave planning mode: while on, mutating tools are blocked (the
    /// agent must research read-only and propose a plan via `ExitPlanMode`).
    SetPlanMode { on: bool },
    /// Set the auto-continue step budget at runtime (the `/autocontinue`
    /// command). 0 disables self-wake on the per-turn iteration cap.
    SetAutoContinue { n: u32 },
    /// Set (empty clears) the session's standing goal — re-injected each turn so a
    /// long autonomous task keeps its objective across context rollovers.
    SetGoal { text: String },
    /// Force a context compaction now (manual `/compact`).
    Compact,
    /// Revert the most recent file change (manual `/undo`).
    Undo,
    /// Switch the active agent persona by name (manual `/persona`).
    SetPersona { name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_round_trips() {
        let c = Command::UserMessage {
            text: "hi".into(),
            attachments: vec![],
            stream_id: None,
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn approve_defaults_scope_once() {
        let c: Command = serde_json::from_str(
            r#"{"type":"approve_tool","request_id":"req_1","decision":"allow"}"#,
        )
        .unwrap();
        match c {
            Command::ApproveTool {
                scope, decision, ..
            } => {
                assert_eq!(scope, ApprovalScope::Once);
                assert_eq!(decision, Decision::Allow);
            }
            _ => panic!("wrong variant"),
        }
    }
}

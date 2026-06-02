//! Events the core emits to every subscribed UI, and the envelope that
//! sequences them. This is the single notification stream that replaces
//! OpenMono's dual `IOutputSink` / `IAcpEventSink` plumbing.

use crate::ids::{MessageId, RequestId, SessionId, ToolCallId};
use crate::stream::FinishReason;
use crate::tool::ArtifactRef;
use serde::{Deserialize, Serialize};

/// A sequenced event for one session. The monotonic `seq` lets late or
/// reconnecting subscribers (e.g. an SSE client sending `Last-Event-ID`)
/// replay exactly what they missed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub seq: u64,
    pub session: SessionId,
    pub event: Event,
}

/// Status of a todo item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// A single task in the agent's plan/todo list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
}

/// A selectable answer offered with a clarification request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyChoice {
    pub id: String,
    pub label: String,
}

/// Why a turn finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoneReason {
    Completed,
    Cancelled,
    Error,
    MaxIterations,
    DoomLoop,
}

/// Everything a UI needs to render a live turn. Serialized with a `type`
/// discriminator (snake_case) — this *is* the SSE event schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A new turn has begun (user message accepted).
    TurnStarted { stream_id: Option<String> },
    /// The assistant began producing a message.
    AssistantStarted { message_id: MessageId },
    /// Visible assistant text delta.
    Token { text: String },
    /// Reasoning / extended-thinking delta.
    Thinking { text: String },
    /// The assistant message finished streaming.
    AssistantFinished {
        message_id: MessageId,
        finish: FinishReason,
    },

    /// A tool call started executing.
    ToolStart {
        id: ToolCallId,
        name: String,
        summary: String,
        input: serde_json::Value,
    },
    /// Incremental output from a long-running tool (e.g. streaming bash).
    ToolProgress { id: ToolCallId, chunk: String },
    /// A tool call finished. (Lean projection of the full `ToolResult`.)
    ToolResult {
        id: ToolCallId,
        name: String,
        ok: bool,
        preview: String,
        duration_ms: u64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        artifacts: Vec<ArtifactRef>,
    },
    /// A file diff produced by a tool, for rendering.
    Diff {
        id: ToolCallId,
        path: String,
        unified: String,
        additions: u32,
        deletions: u32,
    },

    /// The agent needs the user to approve a capability before proceeding.
    ApprovalRequest {
        request_id: RequestId,
        tool: String,
        summary: String,
        dangerous: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
    },
    /// The agent needs disambiguation from the user.
    ClarifyRequest {
        request_id: RequestId,
        question: String,
        choices: Vec<ClarifyChoice>,
    },

    /// The todo/plan list changed.
    TodoUpdate { items: Vec<Todo> },
    /// Token accounting update (drives the live meter).
    Usage {
        input: u32,
        output: u32,
        total: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
    },
    /// Context was compacted/checkpointed.
    Compaction {
        messages_compressed: u32,
        checkpoint: u32,
    },

    /// The turn is complete.
    #[serde(rename = "done")]
    TurnDone { reason: DoneReason },
    /// A turn-level error.
    Error {
        kind: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hint: Option<String>,
    },
}

impl Event {
    /// The SSE `event:` name for this event (its serde tag).
    pub fn name(&self) -> &'static str {
        match self {
            Event::TurnStarted { .. } => "turn_started",
            Event::AssistantStarted { .. } => "assistant_started",
            Event::Token { .. } => "token",
            Event::Thinking { .. } => "thinking",
            Event::AssistantFinished { .. } => "assistant_finished",
            Event::ToolStart { .. } => "tool_start",
            Event::ToolProgress { .. } => "tool_progress",
            Event::ToolResult { .. } => "tool_result",
            Event::Diff { .. } => "diff",
            Event::ApprovalRequest { .. } => "approval_request",
            Event::ClarifyRequest { .. } => "clarify_request",
            Event::TodoUpdate { .. } => "todo_update",
            Event::Usage { .. } => "usage",
            Event::Compaction { .. } => "compaction",
            Event::TurnDone { .. } => "done",
            Event::Error { .. } => "error",
        }
    }

    /// Whether this event is "lossy": token/thinking deltas may be coalesced or
    /// dropped under backpressure (the ring buffer + replay heals gaps), whereas
    /// lifecycle events must be delivered.
    pub fn is_lossy(&self) -> bool {
        matches!(
            self,
            Event::Token { .. } | Event::Thinking { .. } | Event::ToolProgress { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_tag_matches_name() {
        let e = Event::Token { text: "hi".into() };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], "token");
        assert_eq!(v["text"], "hi");
        assert_eq!(e.name(), "token");
    }

    #[test]
    fn done_serializes_as_done() {
        let e = Event::TurnDone {
            reason: DoneReason::Completed,
        };
        assert_eq!(e.name(), "done");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], "done"); // serde tag matches SSE name
        assert_eq!(v["reason"], "completed");
    }

    #[test]
    fn envelope_round_trips() {
        let env = Envelope {
            seq: 7,
            session: SessionId::from("sess_1"),
            event: Event::Usage {
                input: 1,
                output: 2,
                total: 3,
                cost_usd: None,
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn lossy_classification() {
        assert!(Event::Token { text: "x".into() }.is_lossy());
        assert!(!Event::TurnDone {
            reason: DoneReason::Completed
        }
        .is_lossy());
    }
}

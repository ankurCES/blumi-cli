//! In-memory state owned by a session actor.

use lumi_protocol::{Message, SessionId, Todo, Usage};
use time::OffsetDateTime;

/// The mutable state of one conversation. Owned behind a mutex shared between
/// the actor and the in-flight turn task.
#[derive(Debug, Clone)]
pub struct SessionState {
    pub id: SessionId,
    pub messages: Vec<Message>,
    pub todos: Vec<Todo>,
    /// Active model id (mutable via `Command::SetModel`).
    pub model: String,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub total_cache_read_tokens: u32,
    /// Number of completed user→assistant turns.
    pub turn_count: u32,
    pub started_at: OffsetDateTime,
}

impl SessionState {
    pub fn new(id: SessionId, model: impl Into<String>) -> Self {
        SessionState {
            id,
            messages: Vec::new(),
            todos: Vec::new(),
            model: model.into(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cache_read_tokens: 0,
            turn_count: 0,
            started_at: OffsetDateTime::now_utc(),
        }
    }

    /// Fold a usage report into the running totals.
    pub fn record_usage(&mut self, usage: &Usage) {
        self.total_input_tokens += usage.input_tokens;
        self.total_output_tokens += usage.output_tokens;
        self.total_cache_read_tokens += usage.cache_read_tokens;
    }

    pub fn total_tokens(&self) -> u32 {
        self.total_input_tokens + self.total_output_tokens
    }

    /// A point-in-time copy for late subscribers / persistence.
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            id: self.id.clone(),
            messages: self.messages.clone(),
            todos: self.todos.clone(),
            model: self.model.clone(),
            total_input_tokens: self.total_input_tokens,
            total_output_tokens: self.total_output_tokens,
            turn_count: self.turn_count,
        }
    }
}

/// An immutable view of a session, used to bootstrap a UI that attaches after
/// some history already exists.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub messages: Vec<Message>,
    pub todos: Vec<Todo>,
    pub model: String,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub turn_count: u32,
}

//! In-memory state owned by a session actor.

use blumi_protocol::{Message, SessionId, Todo, Usage};
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
    /// Standing objective for this session (set via `Command::SetGoal` / `/goal`).
    /// Re-injected as a trailing reminder each turn so a long autonomous task
    /// keeps its objective across context rollovers.
    pub goal: Option<String>,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub total_cache_read_tokens: u32,
    /// The provider-measured prompt size (input + cache read + cache write) of
    /// the most recent request — the *real* current context-window usage, used
    /// as a floor for the compaction decision so we never overflow.
    pub last_prompt_tokens: u32,
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
            goal: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cache_read_tokens: 0,
            last_prompt_tokens: 0,
            turn_count: 0,
            started_at: OffsetDateTime::now_utc(),
        }
    }

    /// Fold a usage report into the running totals. `total_input_tokens` counts
    /// the *full* prompt (uncached input + cache read + cache write) so the
    /// input meter reflects real tokens processed — not just the uncached
    /// remainder, which is ~0 once prompt caching kicks in. Also records the
    /// latest prompt size as the live context measurement.
    pub fn record_usage(&mut self, usage: &Usage) {
        let prompt = usage.input_tokens + usage.cache_read_tokens + usage.cache_write_tokens;
        self.total_input_tokens += prompt;
        self.total_output_tokens += usage.output_tokens;
        self.total_cache_read_tokens += usage.cache_read_tokens;
        self.last_prompt_tokens = prompt;
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
            goal: self.goal.clone(),
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
    /// Standing objective, carried across reload / resume / rollover.
    pub goal: Option<String>,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub turn_count: u32,
}

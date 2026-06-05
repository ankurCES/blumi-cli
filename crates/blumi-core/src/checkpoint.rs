//! Durable-execution checkpoints — the LangGraph "checkpointer" analog.
//!
//! The core defines the sink trait; a persistence layer (e.g. `blumi-persist`)
//! implements it. The agent saves a checkpoint after every completed tool step,
//! so a crash or gateway restart can resume the turn from the last step instead
//! of replaying it from the user's message. A `None` sink = durability disabled
//! (behaviour is exactly as before).

use crate::session::SessionState;
use async_trait::async_trait;
use blumi_protocol::{Message, Todo};

/// One in-progress turn's state at a given tool step.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub session_id: String,
    /// `SessionState.turn_count` at turn start — one checkpoint row per turn.
    pub turn_seq: u32,
    /// Iteration index within the turn (the last completed tool step).
    pub step: u32,
    pub messages: Vec<Message>,
    pub todos: Vec<Todo>,
    pub model: String,
}

impl Checkpoint {
    /// Snapshot the live state for the current turn at `step`.
    pub fn from_state(st: &SessionState, step: u32) -> Self {
        Checkpoint {
            session_id: st.id.as_str().to_string(),
            turn_seq: st.turn_count,
            step,
            messages: st.messages.clone(),
            todos: st.todos.clone(),
            model: st.model.clone(),
        }
    }
}

/// Persists the in-progress turn. Implemented by the storage layer so the agent
/// loop can checkpoint without depending on a concrete database crate.
#[async_trait]
pub trait CheckpointSink: Send + Sync {
    /// Save the latest step of the current turn (overwrites the prior step).
    async fn save(&self, cp: Checkpoint);
    /// Clear the session's in-progress checkpoint (the turn finished cleanly).
    async fn done(&self, session_id: &str);
}

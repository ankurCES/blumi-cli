//! The turn-execution seam.
//!
//! The session actor (transport, sequencing, cancellation, queueing) is
//! decoupled from *how a turn is run* via [`TurnRunner`]. The real agent loop
//! (`blumi-core::agent`) implements it; tests use a mock. The runner owns its
//! heavy dependencies (LLM client, tools, executor, config); the actor only
//! hands it per-turn channels via [`TurnContext`].

use crate::emit::{EventEmitter, Interactor};
use crate::session::SessionState;
use async_trait::async_trait;
use blumi_protocol::{DoneReason, SessionId};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Per-turn channels handed to a [`TurnRunner`]. Deliberately minimal — no
/// concrete UI, no provider/tool internals.
#[derive(Clone)]
pub struct TurnContext {
    pub session_id: SessionId,
    pub events: EventEmitter,
    pub interactor: Interactor,
}

/// Runs a single turn against shared session state.
///
/// Contract: the user's message has already been appended to
/// `state.messages` and a `TurnStarted` event emitted before this is called.
/// The runner emits all subsequent events through `ctx.events` (it must **not**
/// emit `TurnDone` — the actor does that, using the returned [`DoneReason`],
/// so terminal ordering is guaranteed). Respect `ct` for cancellation.
#[async_trait]
pub trait TurnRunner: Send + Sync {
    async fn run_turn(
        &self,
        state: Arc<Mutex<SessionState>>,
        ctx: TurnContext,
        ct: CancellationToken,
    ) -> DoneReason;
}

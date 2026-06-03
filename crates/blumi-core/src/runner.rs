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

    /// Toggle auto-approve-all (yolo) at runtime. Default: no-op.
    fn set_yolo(&self, _on: bool) {}

    /// Whether auto-approve-all is currently on. Default: `false`.
    fn yolo(&self) -> bool {
        false
    }

    /// Set the local-LLM "brain" approval mode at runtime. Default: no-op.
    fn set_brain_mode(&self, _mode: crate::brain::BrainMode) {}

    /// The current brain approval mode. Default: `Off`.
    fn brain_mode(&self) -> crate::brain::BrainMode {
        crate::brain::BrainMode::Off
    }

    /// Force a context compaction now (the manual `/compact`). Emits a
    /// `Compaction` event via `events` on success. Default: no-op → `false`.
    async fn compact(
        &self,
        _state: Arc<Mutex<SessionState>>,
        _events: &EventEmitter,
        _ct: CancellationToken,
    ) -> bool {
        false
    }

    /// Revert the most recent file change (the manual `/undo`). Returns a short
    /// description of what was reverted, or `None` if there was nothing to undo.
    /// Default: `None`.
    async fn undo(&self) -> Option<String> {
        None
    }

    /// Switch the active persona by name (the `/persona` command). Returns the
    /// resolved [`Persona`] so the caller can apply its model, or `None` if the
    /// name is unknown. Default: `None`.
    fn set_persona(&self, _name: &str) -> Option<crate::Persona> {
        None
    }
}

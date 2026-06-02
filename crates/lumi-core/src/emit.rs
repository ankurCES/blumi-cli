//! Handles that tools and the agent loop use to talk to the outside world,
//! without knowing anything about *which* UI is listening.
//!
//! - [`EventEmitter`] pushes raw [`Event`]s toward the session actor, which
//!   stamps them with a sequence number and broadcasts to all subscribers.
//! - [`Interactor`] performs request/response interactions (approvals,
//!   clarifications): it emits a request and awaits a reply that arrives later
//!   via a `Command` from some UI. This is the single mechanism that unifies
//!   OpenMono's blocking TUI prompt and its async ACP pause/resume.

use lumi_protocol::{ApprovalScope, ClarifyChoice, Decision, Event, RequestId};
use tokio::sync::{mpsc, oneshot};

/// Pushes events toward the session actor. Cloneable and cheap.
#[derive(Clone)]
pub struct EventEmitter {
    tx: mpsc::UnboundedSender<Event>,
}

impl EventEmitter {
    pub fn new(tx: mpsc::UnboundedSender<Event>) -> Self {
        EventEmitter { tx }
    }

    /// Emit an event. Dropped silently if the actor has shut down.
    pub fn emit(&self, event: Event) {
        let _ = self.tx.send(event);
    }
}

/// What the user is being asked.
#[derive(Debug)]
pub enum InteractionKind {
    Approval {
        tool: String,
        summary: String,
        dangerous: bool,
        diff: Option<String>,
    },
    Clarify {
        question: String,
        choices: Vec<ClarifyChoice>,
    },
}

/// The user's reply to an interaction.
#[derive(Debug)]
pub enum InteractionReply {
    Approval {
        decision: Decision,
        scope: ApprovalScope,
    },
    Clarify(String),
}

/// A pending interaction handed to the session actor: it emits the matching
/// event and keeps `respond` until a `Command` resolves it.
pub struct InteractionRequest {
    pub id: RequestId,
    pub kind: InteractionKind,
    pub respond: oneshot::Sender<InteractionReply>,
}

/// Requests an interaction and awaits the user's reply. Cloneable and cheap.
#[derive(Clone)]
pub struct Interactor {
    tx: mpsc::UnboundedSender<InteractionRequest>,
}

impl Interactor {
    pub fn new(tx: mpsc::UnboundedSender<InteractionRequest>) -> Self {
        Interactor { tx }
    }

    /// Ask the user to approve a capability. Returns `(Deny, Once)` if no UI
    /// responds (actor gone / cancelled) — fail closed.
    pub async fn approve(
        &self,
        tool: impl Into<String>,
        summary: impl Into<String>,
        dangerous: bool,
        diff: Option<String>,
    ) -> (Decision, ApprovalScope) {
        let (respond, rx) = oneshot::channel();
        let req = InteractionRequest {
            id: RequestId::new(),
            kind: InteractionKind::Approval {
                tool: tool.into(),
                summary: summary.into(),
                dangerous,
                diff,
            },
            respond,
        };
        if self.tx.send(req).is_err() {
            return (Decision::Deny, ApprovalScope::Once);
        }
        match rx.await {
            Ok(InteractionReply::Approval { decision, scope }) => (decision, scope),
            _ => (Decision::Deny, ApprovalScope::Once),
        }
    }

    /// Ask the user to disambiguate. Returns `None` if no UI responds.
    pub async fn clarify(
        &self,
        question: impl Into<String>,
        choices: Vec<ClarifyChoice>,
    ) -> Option<String> {
        let (respond, rx) = oneshot::channel();
        let req = InteractionRequest {
            id: RequestId::new(),
            kind: InteractionKind::Clarify {
                question: question.into(),
                choices,
            },
            respond,
        };
        if self.tx.send(req).is_err() {
            return None;
        }
        match rx.await {
            Ok(InteractionReply::Clarify(v)) => Some(v),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approve_resolves_via_responder() {
        let (tx, mut rx) = mpsc::unbounded_channel::<InteractionRequest>();
        let interactor = Interactor::new(tx);

        // Simulate the actor: receive the request, reply Allow.
        let actor = tokio::spawn(async move {
            let req = rx.recv().await.unwrap();
            assert!(matches!(req.kind, InteractionKind::Approval { .. }));
            req.respond
                .send(InteractionReply::Approval {
                    decision: Decision::Allow,
                    scope: ApprovalScope::Once,
                })
                .unwrap();
        });

        let (decision, _scope) = interactor.approve("Bash", "run ls", false, None).await;
        assert_eq!(decision, Decision::Allow);
        actor.await.unwrap();
    }

    #[tokio::test]
    async fn approve_fails_closed_when_no_actor() {
        let (tx, rx) = mpsc::unbounded_channel::<InteractionRequest>();
        drop(rx); // no actor listening
        let interactor = Interactor::new(tx);
        let (decision, _) = interactor.approve("Bash", "x", true, None).await;
        assert_eq!(decision, Decision::Deny);
    }
}

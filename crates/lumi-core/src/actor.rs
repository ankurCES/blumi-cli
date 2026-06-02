//! The session actor and its handle.
//!
//! One actor task per session owns the [`SessionState`] and serializes all
//! input. UIs interact only through a cloneable [`SessionHandle`]: send
//! [`Command`]s, subscribe to the [`Envelope`] stream, replay missed events,
//! or snapshot current state. This is the single seam that replaces OpenMono's
//! dual TUI/ACP notification plumbing.

use crate::emit::{
    EventEmitter, InteractionKind, InteractionReply, InteractionRequest, Interactor,
};
use crate::eventlog::EventLog;
use crate::runner::{TurnContext, TurnRunner};
use crate::session::{SessionSnapshot, SessionState};
use lumi_protocol::{Command, Envelope, Event, Message, RequestId, SessionId, StreamId};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;

/// The command channel buffer (commands are infrequent and small).
const COMMAND_BUFFER: usize = 64;

/// Error returned when a command can't reach a shut-down session.
#[derive(Debug, thiserror::Error)]
#[error("session is closed")]
pub struct SessionClosed;

/// A cloneable handle to a running session. The only way UIs touch a session.
#[derive(Clone)]
pub struct SessionHandle {
    id: SessionId,
    command_tx: mpsc::Sender<Command>,
    log: Arc<StdMutex<EventLog>>,
    state: Arc<Mutex<SessionState>>,
}

impl SessionHandle {
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// Submit a command to the session actor.
    pub async fn send(&self, cmd: Command) -> Result<(), SessionClosed> {
        self.command_tx.send(cmd).await.map_err(|_| SessionClosed)
    }

    /// Subscribe to the live event stream. Pair with [`events_since`] for a
    /// gap-free attach.
    ///
    /// [`events_since`]: Self::events_since
    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.log.lock().expect("event log poisoned").subscribe()
    }

    /// Replay retained events with `seq > after_seq`.
    pub fn events_since(&self, after_seq: u64) -> Vec<Envelope> {
        self.log
            .lock()
            .expect("event log poisoned")
            .since(after_seq)
    }

    /// A point-in-time copy of session state (for late-attaching UIs).
    pub async fn snapshot(&self) -> SessionSnapshot {
        self.state.lock().await.snapshot()
    }
}

/// Spawn a session actor and return its handle. The actor runs until every
/// `SessionHandle` is dropped.
pub fn spawn_session(
    id: SessionId,
    model: impl Into<String>,
    runner: Arc<dyn TurnRunner>,
) -> SessionHandle {
    let state = Arc::new(Mutex::new(SessionState::new(id.clone(), model)));
    let log = Arc::new(StdMutex::new(EventLog::new(id.clone())));

    let (command_tx, command_rx) = mpsc::channel(COMMAND_BUFFER);
    let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
    let (interaction_tx, interaction_rx) = mpsc::unbounded_channel::<InteractionRequest>();
    let (turn_done_tx, turn_done_rx) = mpsc::unbounded_channel::<lumi_protocol::DoneReason>();

    let handle = SessionHandle {
        id: id.clone(),
        command_tx,
        log: log.clone(),
        state: state.clone(),
    };

    let actor = SessionActor {
        id,
        state,
        log,
        runner,
        command_rx,
        event_tx,
        event_rx,
        interaction_tx,
        interaction_rx,
        turn_done_tx,
        turn_done_rx,
        pending: HashMap::new(),
        turn_token: None,
        queued: VecDeque::new(),
    };

    tokio::spawn(actor.run());
    handle
}

struct SessionActor {
    id: SessionId,
    state: Arc<Mutex<SessionState>>,
    log: Arc<StdMutex<EventLog>>,
    runner: Arc<dyn TurnRunner>,

    command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::UnboundedSender<Event>,
    event_rx: mpsc::UnboundedReceiver<Event>,
    interaction_tx: mpsc::UnboundedSender<InteractionRequest>,
    interaction_rx: mpsc::UnboundedReceiver<InteractionRequest>,
    turn_done_tx: mpsc::UnboundedSender<lumi_protocol::DoneReason>,
    turn_done_rx: mpsc::UnboundedReceiver<lumi_protocol::DoneReason>,

    /// Interactions awaiting a user reply, keyed by request id.
    pending: HashMap<RequestId, tokio::sync::oneshot::Sender<InteractionReply>>,
    /// Cancellation token for the in-flight turn (None when idle).
    turn_token: Option<CancellationToken>,
    /// User messages received while busy, started FIFO when the turn ends.
    queued: VecDeque<(String, Option<StreamId>)>,
}

impl SessionActor {
    fn publish(&self, event: Event) -> Envelope {
        self.log.lock().expect("event log poisoned").publish(event)
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                cmd = self.command_rx.recv() => match cmd {
                    Some(cmd) => self.handle_command(cmd).await,
                    None => break, // all handles dropped
                },
                Some(event) = self.event_rx.recv() => {
                    self.publish(event);
                }
                Some(ir) = self.interaction_rx.recv() => {
                    self.handle_interaction(ir);
                }
                Some(reason) = self.turn_done_rx.recv() => {
                    self.finish_turn(reason).await;
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: Command) {
        match cmd {
            Command::UserMessage {
                text, stream_id, ..
            } => {
                if self.turn_token.is_some() {
                    self.queued.push_back((text, stream_id));
                } else {
                    self.start_turn(text, stream_id).await;
                }
            }
            Command::Cancel => {
                if let Some(tok) = &self.turn_token {
                    tok.cancel();
                }
            }
            Command::ApproveTool {
                request_id,
                decision,
                scope,
            } => {
                if let Some(resp) = self.pending.remove(&request_id) {
                    let _ = resp.send(InteractionReply::Approval { decision, scope });
                }
            }
            Command::AnswerClarify { request_id, value } => {
                if let Some(resp) = self.pending.remove(&request_id) {
                    let _ = resp.send(InteractionReply::Clarify(value));
                }
            }
            Command::SetModel { model } => {
                self.state.lock().await.model = model;
            }
        }
    }

    async fn start_turn(&mut self, text: String, stream_id: Option<StreamId>) {
        self.state.lock().await.messages.push(Message::user(text));

        let token = CancellationToken::new();
        self.turn_token = Some(token.clone());
        self.publish(Event::TurnStarted {
            stream_id: stream_id.map(|s| s.0),
        });

        let ctx = TurnContext {
            session_id: self.id.clone(),
            events: EventEmitter::new(self.event_tx.clone()),
            interactor: Interactor::new(self.interaction_tx.clone()),
        };
        let runner = self.runner.clone();
        let state = self.state.clone();
        let done = self.turn_done_tx.clone();
        tokio::spawn(async move {
            let reason = runner.run_turn(state, ctx, token).await;
            let _ = done.send(reason);
        });
    }

    async fn finish_turn(&mut self, reason: lumi_protocol::DoneReason) {
        // Drain any events the turn emitted before signalling completion, so
        // TurnDone is always the last event of the turn.
        while let Ok(event) = self.event_rx.try_recv() {
            self.publish(event);
        }
        self.publish(Event::TurnDone { reason });
        self.turn_token = None;
        self.state.lock().await.turn_count += 1;

        if let Some((text, stream_id)) = self.queued.pop_front() {
            self.start_turn(text, stream_id).await;
        }
    }

    fn handle_interaction(&mut self, ir: InteractionRequest) {
        let event = match &ir.kind {
            InteractionKind::Approval {
                tool,
                summary,
                dangerous,
                diff,
            } => Event::ApprovalRequest {
                request_id: ir.id.clone(),
                tool: tool.clone(),
                summary: summary.clone(),
                dangerous: *dangerous,
                diff: diff.clone(),
            },
            InteractionKind::Clarify { question, choices } => Event::ClarifyRequest {
                request_id: ir.id.clone(),
                question: question.clone(),
                choices: choices.clone(),
            },
        };
        self.publish(event);
        self.pending.insert(ir.id, ir.respond);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::TurnContext;
    use async_trait::async_trait;
    use lumi_protocol::{Decision, DoneReason};

    /// Emits two tokens then completes — unless cancelled first.
    struct MockRunner;

    #[async_trait]
    impl TurnRunner for MockRunner {
        async fn run_turn(
            &self,
            state: Arc<Mutex<SessionState>>,
            ctx: TurnContext,
            ct: CancellationToken,
        ) -> DoneReason {
            ctx.events.emit(Event::Token {
                text: "hello ".into(),
            });
            if ct.is_cancelled() {
                return DoneReason::Cancelled;
            }
            ctx.events.emit(Event::Token {
                text: "world".into(),
            });
            state
                .lock()
                .await
                .messages
                .push(Message::assistant("hello world"));
            DoneReason::Completed
        }
    }

    /// Asks for approval, then echoes the decision into an assistant message.
    struct ApprovalRunner;

    #[async_trait]
    impl TurnRunner for ApprovalRunner {
        async fn run_turn(
            &self,
            state: Arc<Mutex<SessionState>>,
            ctx: TurnContext,
            _ct: CancellationToken,
        ) -> DoneReason {
            let (decision, _scope) = ctx.interactor.approve("Bash", "run rm", true, None).await;
            let text = format!("decision={decision:?}");
            state.lock().await.messages.push(Message::assistant(text));
            DoneReason::Completed
        }
    }

    async fn collect_until_done(rx: &mut broadcast::Receiver<Envelope>) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let env = rx.recv().await.unwrap();
            let is_done = matches!(env.event, Event::TurnDone { .. });
            events.push(env.event);
            if is_done {
                break;
            }
        }
        events
    }

    #[tokio::test]
    async fn runs_a_turn_and_broadcasts_in_order() {
        let h = spawn_session(SessionId::from("s1"), "m", Arc::new(MockRunner));
        let mut rx = h.subscribe();
        h.send(Command::UserMessage {
            text: "hi".into(),
            attachments: vec![],
            stream_id: None,
        })
        .await
        .unwrap();

        let events = collect_until_done(&mut rx).await;
        // turn_started, token, token, done
        assert!(matches!(events[0], Event::TurnStarted { .. }));
        assert!(matches!(events[1], Event::Token { .. }));
        assert!(matches!(
            events.last().unwrap(),
            Event::TurnDone {
                reason: DoneReason::Completed
            }
        ));

        let snap = h.snapshot().await;
        assert_eq!(snap.turn_count, 1);
        // user + assistant
        assert_eq!(snap.messages.len(), 2);
    }

    #[tokio::test]
    async fn replay_via_events_since_is_gap_free() {
        let h = spawn_session(SessionId::from("s2"), "m", Arc::new(MockRunner));
        let mut rx = h.subscribe();
        h.send(Command::UserMessage {
            text: "hi".into(),
            attachments: vec![],
            stream_id: None,
        })
        .await
        .unwrap();
        let _ = collect_until_done(&mut rx).await;

        // A late subscriber can replay everything from seq 0.
        let replay = h.events_since(0);
        assert!(replay.len() >= 4);
        assert_eq!(replay[0].seq, 1);
        assert!(matches!(
            replay.last().unwrap().event,
            Event::TurnDone { .. }
        ));
    }

    #[tokio::test]
    async fn approval_round_trips_through_commands() {
        let h = spawn_session(SessionId::from("s3"), "m", Arc::new(ApprovalRunner));
        let mut rx = h.subscribe();
        h.send(Command::UserMessage {
            text: "go".into(),
            attachments: vec![],
            stream_id: None,
        })
        .await
        .unwrap();

        // Wait for the approval request, capture its id.
        let req_id = loop {
            let env = rx.recv().await.unwrap();
            if let Event::ApprovalRequest {
                request_id,
                dangerous,
                ..
            } = env.event
            {
                assert!(dangerous);
                break request_id;
            }
        };

        h.send(Command::ApproveTool {
            request_id: req_id,
            decision: Decision::Allow,
            scope: lumi_protocol::ApprovalScope::Once,
        })
        .await
        .unwrap();

        let _ = collect_until_done(&mut rx).await;
        let snap = h.snapshot().await;
        assert_eq!(snap.messages.last().unwrap().text(), "decision=Allow");
    }
}

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
use blumi_protocol::{Command, Decision, Envelope, Event, Message, RequestId, SessionId, StreamId};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;

/// The command channel buffer (commands are infrequent and small).
const COMMAND_BUFFER: usize = 64;

/// Injected to resume a turn that stopped at the per-turn tool-iteration cap.
/// Guides the model to continue from where it left off without redoing work.
const AUTO_CONTINUE_PROMPT: &str = "Continue. You reached the per-turn tool limit, \
not the end of the task — resume exactly where you left off, do not repeat steps \
you have already completed, and keep working until the task is genuinely done \
(then stop).";

/// Shown when a context rollover (compaction) refreshes the auto-continue token
/// budget so the turn keeps going instead of pausing at the cumulative ceiling.
const ROLLOVER_NOTICE: &str =
    "↻ context rolled over — auto-continue token budget refreshed, continuing";

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
    spawn_session_seeded(SessionState::new(id, model), runner)
}

/// Spawn a session actor seeded with an existing [`SessionState`] (e.g. a
/// resumed conversation). The actor adopts the state's id, model, and messages.
pub fn spawn_session_seeded(seed: SessionState, runner: Arc<dyn TurnRunner>) -> SessionHandle {
    let id = seed.id.clone();
    let state = Arc::new(Mutex::new(seed));
    let log = Arc::new(StdMutex::new(EventLog::new(id.clone())));

    let (command_tx, command_rx) = mpsc::channel(COMMAND_BUFFER);
    let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
    let (interaction_tx, interaction_rx) = mpsc::unbounded_channel::<InteractionRequest>();
    let (turn_done_tx, turn_done_rx) = mpsc::unbounded_channel::<blumi_protocol::DoneReason>();

    let handle = SessionHandle {
        id: id.clone(),
        command_tx,
        log: log.clone(),
        state: state.clone(),
    };

    // Give the runner the session's stable emitter/interactor up front so it can
    // start background work before the first turn (e.g. a remote attach's live
    // SSE reader). No-op for ordinary local runners.
    runner.on_attach(
        state.clone(),
        EventEmitter::new(event_tx.clone()),
        Interactor::new(interaction_tx.clone()),
    );

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
        plan_pending: HashSet::new(),
        turn_token: None,
        queued: VecDeque::new(),
        auto_continues: 0,
        turn_tokens: 0,
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
    turn_done_tx: mpsc::UnboundedSender<blumi_protocol::DoneReason>,
    turn_done_rx: mpsc::UnboundedReceiver<blumi_protocol::DoneReason>,

    /// Interactions awaiting a user reply, keyed by request id.
    pending: HashMap<RequestId, tokio::sync::oneshot::Sender<InteractionReply>>,
    /// Request ids that are plan reviews (so an `Allow` exits plan mode).
    plan_pending: HashSet<RequestId>,
    /// Cancellation token for the in-flight turn (None when idle).
    turn_token: Option<CancellationToken>,
    /// User messages received while busy, started FIFO when the turn ends.
    queued: VecDeque<(String, Option<StreamId>)>,
    /// Consecutive auto-continuations since the last user message / completion
    /// (bounded by the runner's `auto_continue_budget`).
    auto_continues: u32,
    /// Billed tokens (input+output) accumulated across the current self-woken
    /// sequence, for the token ceiling. Reset when a turn truly ends.
    turn_tokens: u32,
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
                    let rolled_over = self.account_event(&event);
                    self.publish(event);
                    if rolled_over {
                        self.publish(Event::Notice {
                            message: ROLLOVER_NOTICE.into(),
                        });
                    }
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
                    // A fresh user instruction resets the auto-continue budget.
                    self.auto_continues = 0;
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
                // Approving a plan review exits plan mode so the agent's next
                // (mutating) tools are allowed. Done here, in the actor, so it
                // lands well before the turn streams its next request — no race
                // with the client having to also toggle the mode.
                let approved_plan =
                    self.plan_pending.remove(&request_id) && decision == Decision::Allow;
                if let Some(resp) = self.pending.remove(&request_id) {
                    let _ = resp.send(InteractionReply::Approval { decision, scope });
                }
                if approved_plan {
                    self.runner.set_plan_mode(false);
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
            Command::SetYolo { on } => {
                self.runner.set_yolo(on);
            }
            Command::SetBrainMode { mode } => {
                if let Some(m) = crate::brain::BrainMode::parse(&mode) {
                    self.runner.set_brain_mode(m);
                }
            }
            Command::SetRouterMode { mode } => {
                if let Some(m) = crate::router::RouterMode::parse(&mode) {
                    self.runner.set_router_mode(m);
                }
            }
            Command::SetPlanMode { on } => {
                self.runner.set_plan_mode(on);
            }
            Command::SetAutoContinue { n } => {
                self.runner.set_auto_continue(n);
            }
            Command::SetGoal { text } => {
                let t = text.trim();
                self.state.lock().await.goal = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
            }
            Command::Compact => {
                if self.turn_token.is_some() {
                    self.publish(Event::Error {
                        kind: "busy".into(),
                        message: "cannot compact while a turn is running".into(),
                        hint: Some("press esc to cancel first".into()),
                    });
                } else {
                    let events = EventEmitter::new(self.event_tx.clone());
                    let did = self
                        .runner
                        .compact(self.state.clone(), &events, CancellationToken::new())
                        .await;
                    // Drain the Compaction event the runner emitted, if any.
                    while let Ok(event) = self.event_rx.try_recv() {
                        self.publish(event);
                    }
                    if !did {
                        self.publish(Event::Notice {
                            message: "nothing to compact yet — history is still small".into(),
                        });
                    }
                }
            }
            Command::Undo => {
                if self.turn_token.is_some() {
                    self.publish(Event::Error {
                        kind: "busy".into(),
                        message: "cannot undo while a turn is running".into(),
                        hint: Some("press esc to cancel first".into()),
                    });
                } else {
                    let msg = self
                        .runner
                        .undo()
                        .await
                        .unwrap_or_else(|| "nothing to undo".to_string());
                    self.publish(Event::Notice { message: msg });
                }
            }
            Command::SetPersona { name } => match self.runner.set_persona(&name) {
                Some(p) => {
                    if let Some(model) = p.model.filter(|m| !m.is_empty()) {
                        self.state.lock().await.model = model;
                    }
                    let suffix = if p.description.is_empty() {
                        String::new()
                    } else {
                        format!(" — {}", p.description)
                    };
                    self.publish(Event::Notice {
                        message: format!("persona → {}{suffix}", p.name),
                    });
                }
                None => {
                    self.publish(Event::Notice {
                        message: format!("unknown persona '{name}'"),
                    });
                }
            },
        }
    }

    async fn start_turn(&mut self, text: String, stream_id: Option<StreamId>) {
        self.start_turn_inner(text, stream_id, true).await;
    }

    /// Start a turn. `announce` emits `TurnStarted` (true for user-initiated
    /// turns); auto-continuations pass `false` so the whole self-woken sequence
    /// reads as one seamless turn (one TurnStarted … one TurnDone).
    async fn start_turn_inner(
        &mut self,
        text: String,
        stream_id: Option<StreamId>,
        announce: bool,
    ) {
        self.state.lock().await.messages.push(Message::user(text));

        let token = CancellationToken::new();
        self.turn_token = Some(token.clone());
        if announce {
            self.publish(Event::TurnStarted {
                stream_id: stream_id.map(|s| s.0),
            });
        }

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

    /// Per-turn token accounting from a streamed event. Returns true on a
    /// context rollover (compaction) so the caller can narrate it.
    fn account_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Usage { total, .. } => {
                self.turn_tokens = self.turn_tokens.saturating_add(*total);
                false
            }
            // A rollover frees the context, so reset the cumulative token tally:
            // the auto-continue token ceiling becomes per-epoch and a long task
            // no longer pauses right after a rollover. The step budget still
            // bounds the turn, so this can't loop forever.
            Event::Compaction { .. } if self.runner.wake_on_rollover() => {
                self.turn_tokens = 0;
                true
            }
            _ => false,
        }
    }

    async fn finish_turn(&mut self, reason: blumi_protocol::DoneReason) {
        use blumi_protocol::DoneReason;
        // Drain any events the turn emitted before deciding what's next, so
        // ordering is preserved (and keep the token tally accurate).
        while let Ok(event) = self.event_rx.try_recv() {
            let rolled_over = self.account_event(&event);
            self.publish(event);
            if rolled_over {
                self.publish(Event::Notice {
                    message: ROLLOVER_NOTICE.into(),
                });
            }
        }
        self.turn_token = None;

        // Self-wake: a turn that stopped *only* because it hit the per-turn tool
        // cap (not finished / errored / looped) and has no pending user message
        // is continued automatically in the same session — no work or context
        // is lost, and the user needn't nudge it between turns. We do NOT emit
        // TurnDone for the spent segment (nor TurnStarted for the next), so the
        // whole self-woken sequence reads as one seamless turn: `busy` stays on
        // and consumers that wait for TurnDone see only the real end.
        //
        // Bounded two ways so it stays token-effective: a step budget AND a
        // token ceiling — whichever is hit first stops the self-wake.
        let budget = self.runner.auto_continue_budget();
        let token_budget = self.runner.auto_continue_token_budget();
        let token_exhausted = token_budget > 0 && self.turn_tokens >= token_budget;
        let auto = reason == DoneReason::MaxIterations
            && budget > 0
            && self.queued.is_empty()
            && self.auto_continues < budget
            && !token_exhausted;
        if auto {
            self.auto_continues += 1;
            self.publish(Event::Notice {
                message: format!(
                    "↻ continuing automatically ({}/{}, ~{}k tok) — picking up where it left off",
                    self.auto_continues,
                    budget,
                    self.turn_tokens / 1000
                ),
            });
            self.start_turn_inner(AUTO_CONTINUE_PROMPT.to_string(), None, false)
                .await;
            return;
        }

        // Stopped at a cap with auto-continue on: say why it paused, then end.
        if reason == DoneReason::MaxIterations && budget > 0 {
            let why = if token_exhausted {
                format!(
                    "paused — auto-continue hit its ~{}k-token budget",
                    token_budget / 1000
                )
            } else if self.auto_continues >= budget {
                format!("paused after {budget} auto-continuations")
            } else {
                String::new()
            };
            if !why.is_empty() {
                self.publish(Event::Notice {
                    message: format!("{why} — send a message to keep going"),
                });
            }
        }

        // End the (possibly multi-segment) turn.
        self.publish(Event::TurnDone { reason });
        self.state.lock().await.turn_count += 1;
        self.auto_continues = 0;
        self.turn_tokens = 0;

        // A queued user message runs next (the user is steering).
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
                advice,
            } => Event::ApprovalRequest {
                request_id: ir.id.clone(),
                tool: tool.clone(),
                summary: summary.clone(),
                dangerous: *dangerous,
                diff: diff.clone(),
                advice: advice.clone(),
            },
            InteractionKind::Clarify { question, choices } => Event::ClarifyRequest {
                request_id: ir.id.clone(),
                question: question.clone(),
                choices: choices.clone(),
            },
            InteractionKind::Plan { plan } => Event::PlanReview {
                request_id: ir.id.clone(),
                plan: plan.clone(),
            },
        };
        if matches!(ir.kind, InteractionKind::Plan { .. }) {
            self.plan_pending.insert(ir.id.clone());
        }
        self.publish(event);
        self.pending.insert(ir.id, ir.respond);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::TurnContext;
    use async_trait::async_trait;
    use blumi_protocol::{Decision, DoneReason};

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

    /// Always stops at the per-turn cap (MaxIterations), with an auto-continue
    /// budget — to exercise the actor's self-wake.
    struct CapRunner {
        budget: u32,
        token_budget: u32,
        per_call_tokens: u32,
        calls: std::sync::atomic::AtomicU32,
    }

    #[async_trait]
    impl TurnRunner for CapRunner {
        async fn run_turn(
            &self,
            _state: Arc<Mutex<SessionState>>,
            ctx: TurnContext,
            _ct: CancellationToken,
        ) -> DoneReason {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.per_call_tokens > 0 {
                ctx.events.emit(Event::Usage {
                    input: self.per_call_tokens,
                    output: 0,
                    total: self.per_call_tokens,
                    context: self.per_call_tokens,
                    cost_usd: None,
                });
            }
            DoneReason::MaxIterations
        }
        fn auto_continue_budget(&self) -> u32 {
            self.budget
        }
        fn auto_continue_token_budget(&self) -> u32 {
            self.token_budget
        }
    }

    /// Stops at the cap each call AND rolls over (emits a `Compaction`) — to
    /// exercise the rollover token-budget reset.
    struct RolloverRunner {
        budget: u32,
        token_budget: u32,
        per_call_tokens: u32,
        calls: std::sync::atomic::AtomicU32,
    }

    #[async_trait]
    impl TurnRunner for RolloverRunner {
        async fn run_turn(
            &self,
            _state: Arc<Mutex<SessionState>>,
            ctx: TurnContext,
            _ct: CancellationToken,
        ) -> DoneReason {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ctx.events.emit(Event::Usage {
                input: self.per_call_tokens,
                output: 0,
                total: self.per_call_tokens,
                context: self.per_call_tokens,
                cost_usd: None,
            });
            ctx.events.emit(Event::Compaction {
                messages_compressed: 4,
                checkpoint: 0,
                tokens_after: 10,
            });
            DoneReason::MaxIterations
        }
        fn auto_continue_budget(&self) -> u32 {
            self.budget
        }
        fn auto_continue_token_budget(&self) -> u32 {
            self.token_budget
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
            let (decision, _scope) = ctx
                .interactor
                .approve("Bash", "run rm", true, None, None)
                .await;
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
    async fn auto_continues_then_pauses_when_budget_spent() {
        let runner = Arc::new(CapRunner {
            budget: 2,
            token_budget: 0,
            per_call_tokens: 0,
            calls: std::sync::atomic::AtomicU32::new(0),
        });
        let h = spawn_session(SessionId::from("ac"), "m", runner.clone());
        let mut rx = h.subscribe();
        h.send(Command::UserMessage {
            text: "do a big task".into(),
            attachments: vec![],
            stream_id: None,
        })
        .await
        .unwrap();

        let events = collect_until_done(&mut rx).await;

        // The self-woken sequence reads as ONE turn: a single TurnStarted and a
        // single TurnDone bracket all the segments.
        let started = events
            .iter()
            .filter(|e| matches!(e, Event::TurnStarted { .. }))
            .count();
        let done = events
            .iter()
            .filter(|e| matches!(e, Event::TurnDone { .. }))
            .count();
        assert_eq!(started, 1, "one TurnStarted for the whole sequence");
        assert_eq!(done, 1, "one TurnDone at the real end");

        // Original turn + 2 auto-continuations actually ran.
        assert_eq!(runner.calls.load(std::sync::atomic::Ordering::SeqCst), 3);

        // It narrated the two continuations and the final pause.
        let notices = events
            .iter()
            .filter(|e| matches!(e, Event::Notice { .. }))
            .count();
        assert!(notices >= 3, "continuation + pause notices, got {notices}");
    }

    #[tokio::test]
    async fn auto_continue_stops_on_token_budget() {
        // High step budget, low token ceiling: ~100 tok/segment, cap 250 → it
        // should stop on tokens (after 3 segments), not on the step count.
        let runner = Arc::new(CapRunner {
            budget: 100,
            token_budget: 250,
            per_call_tokens: 100,
            calls: std::sync::atomic::AtomicU32::new(0),
        });
        let h = spawn_session(SessionId::from("tok"), "m", runner.clone());
        let mut rx = h.subscribe();
        h.send(Command::UserMessage {
            text: "big task".into(),
            attachments: vec![],
            stream_id: None,
        })
        .await
        .unwrap();

        let events = collect_until_done(&mut rx).await;

        // 100 + 100 (continue) then 300 ≥ 250 stops → 3 calls, well under 100.
        assert_eq!(runner.calls.load(std::sync::atomic::Ordering::SeqCst), 3);
        // It paused specifically for the token budget.
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Notice { message } if message.contains("token"))));
    }

    #[tokio::test]
    async fn rollover_refreshes_token_budget_and_keeps_going() {
        // Same numbers as the token-budget test (which stops at 3 calls on the
        // 250 ceiling) — but each call also rolls over (compaction), which resets
        // the token tally, so it runs to the STEP budget instead of pausing on
        // tokens. Proves auto-wake survives a context rollover.
        let runner = Arc::new(RolloverRunner {
            budget: 5,
            token_budget: 250,
            per_call_tokens: 100,
            calls: std::sync::atomic::AtomicU32::new(0),
        });
        let h = spawn_session(SessionId::from("ro"), "m", runner.clone());
        let mut rx = h.subscribe();
        h.send(Command::UserMessage {
            text: "big task".into(),
            attachments: vec![],
            stream_id: None,
        })
        .await
        .unwrap();

        let events = collect_until_done(&mut rx).await;

        // 1 original + 5 auto-continues = 6 calls (step budget), NOT 3 (the token
        // ceiling) — the rollover reset the token tally each segment.
        assert_eq!(
            runner.calls.load(std::sync::atomic::Ordering::SeqCst),
            6,
            "ran to the step budget; the rollover reset the token ceiling"
        );
        // It narrated the rollover refresh.
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Notice { message } if message.contains("rolled over"))));
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
            scope: blumi_protocol::ApprovalScope::Once,
        })
        .await
        .unwrap();

        let _ = collect_until_done(&mut rx).await;
        let snap = h.snapshot().await;
        assert_eq!(snap.messages.last().unwrap().text(), "decision=Allow");
    }
}

//! Remote-instance attach (ralph-tui style): drive a remote `blumi web` server
//! from the local TUI as if it were a local session.
//!
//! A [`RemoteRunner`] implements [`TurnRunner`], so a remote instance becomes an
//! ordinary [`SessionHandle`] the TUI can switch to via its existing live
//! session-swap. The runner speaks the same HTTP/SSE contract the web UI uses:
//!
//!   - a persistent `GET /api/chat/stream` (SSE) whose `Event`s are re-emitted
//!     into the local session, so the TUI renders the remote turn live;
//!   - `POST /api/chat/send` to start a turn, `POST /api/chat/cancel` to stop it;
//!   - approval/clarify requests from the remote are forwarded to the *local*
//!     approval cards, and the user's decision is `POST`ed back to the remote.
//!
//! Auth: open instances need no credentials; password-protected ones log in
//! once (`POST /api/login`) and reuse the session cookie.

use async_trait::async_trait;
use blumi_config::RemoteInstance;
use blumi_core::{
    spawn_session_seeded, EventEmitter, Interactor, SessionHandle, SessionState, TurnContext,
    TurnRunner,
};
use blumi_protocol::{ApprovalScope, Decision, DoneReason, Event, Message, Role, SessionId};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

/// Proxies turns to a remote `blumi web` server.
pub struct RemoteRunner {
    base: String,
    client: reqwest::Client,
    /// Session cookie after login (shared with the reader task).
    cookie: Arc<AsyncMutex<Option<String>>>,
    password: String,
    /// Reader started flag (started lazily on the first turn).
    reader_started: AtomicBool,
    /// Highest SSE `seq` seen, so reconnects don't replay old events.
    last_seq: Arc<AtomicU64>,
    /// Fired by the reader when the turn the runner is awaiting completes.
    turn_done: Arc<Notify>,
    /// True while a local turn is awaiting its remote completion (so historical
    /// replay doesn't satisfy a future wait).
    awaiting: Arc<AtomicBool>,
    /// Cancels the reader when the session (and thus the runner) is dropped.
    cancel: CancellationToken,
}

impl RemoteRunner {
    pub fn new(inst: &RemoteInstance) -> Self {
        let base = inst.url.trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .user_agent("blumi-tui")
            .build()
            .unwrap_or_default();
        RemoteRunner {
            base,
            client,
            cookie: Arc::new(AsyncMutex::new(None)),
            password: inst.password.clone(),
            reader_started: AtomicBool::new(false),
            last_seq: Arc::new(AtomicU64::new(0)),
            turn_done: Arc::new(Notify::new()),
            awaiting: Arc::new(AtomicBool::new(false)),
            cancel: CancellationToken::new(),
        }
    }

    /// Log in if the instance is password-protected, caching the session cookie.
    async fn ensure_login(&self) {
        if self.password.is_empty() {
            return;
        }
        if self.cookie.lock().await.is_some() {
            return;
        }
        let url = format!("{}/api/login", self.base);
        if let Ok(resp) = self
            .client
            .post(&url)
            .json(&json!({ "password": self.password }))
            .send()
            .await
        {
            if let Some(cookie) = resp
                .headers()
                .get(reqwest::header::SET_COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(';').next())
            {
                *self.cookie.lock().await = Some(cookie.to_string());
            }
        }
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> anyhow::Result<()> {
        let url = format!("{}{path}", self.base);
        let mut req = self.client.post(&url).json(&body);
        if let Some(c) = self.cookie.lock().await.as_ref() {
            req = req.header(reqwest::header::COOKIE, c);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("remote returned {}", resp.status());
        }
        Ok(())
    }

    /// Start the persistent SSE reader once, capturing the (stable) local
    /// event/interaction channels and session state.
    fn ensure_reader(
        &self,
        events: EventEmitter,
        interactor: Interactor,
        state: &Arc<Mutex<SessionState>>,
    ) {
        if self.reader_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let reader = Reader {
            base: self.base.clone(),
            client: self.client.clone(),
            cookie: self.cookie.clone(),
            events,
            interactor,
            state: state.clone(),
            last_seq: self.last_seq.clone(),
            turn_done: self.turn_done.clone(),
            awaiting: self.awaiting.clone(),
            cancel: self.cancel.clone(),
        };
        tokio::spawn(reader.run());
    }

    /// Fetch the remote's current transcript so attaching shows the existing
    /// conversation instead of a blank pane (and re-attaching after a TUI
    /// restart re-pulls it). Best-effort: returns empty on any failure.
    async fn fetch_transcript(&self) -> Vec<Message> {
        self.ensure_login().await;
        let url = format!("{}/api/messages", self.base);
        let mut req = self.client.get(&url);
        if let Some(c) = self.cookie.lock().await.as_ref() {
            req = req.header(reqwest::header::COOKIE, c);
        }
        let Ok(resp) = req.send().await else {
            return Vec::new();
        };
        let Ok(body) = resp.json::<serde_json::Value>().await else {
            return Vec::new();
        };
        let items = body
            .get("messages")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for it in items {
            let role = it.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let text = it
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if text.trim().is_empty() {
                continue;
            }
            match role {
                "user" => out.push(Message::user(text)),
                "assistant" => out.push(Message::assistant(text)),
                // tool/system results aren't reconstructed for the seeded view.
                _ => {}
            }
        }
        out
    }
}

impl Drop for RemoteRunner {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[async_trait]
impl TurnRunner for RemoteRunner {
    fn on_attach(
        &self,
        state: Arc<Mutex<SessionState>>,
        events: EventEmitter,
        interactor: Interactor,
    ) {
        // Start the live SSE reader immediately on attach so the remote's turns
        // stream without waiting for a first local message. Login already ran in
        // connect()/fetch_transcript, so the cookie is ready.
        self.ensure_reader(events, interactor, &state);
    }

    async fn run_turn(
        &self,
        state: Arc<Mutex<SessionState>>,
        ctx: TurnContext,
        ct: CancellationToken,
    ) -> DoneReason {
        self.ensure_login().await;
        self.ensure_reader(ctx.events.clone(), ctx.interactor.clone(), &state);

        // The actor already appended the user's message; forward its text.
        let text = {
            let s = state.lock().await;
            s.messages
                .iter()
                .rev()
                .find(|m| m.role == Role::User)
                .map(|m| m.text())
                .unwrap_or_default()
        };

        self.awaiting.store(true, Ordering::SeqCst);
        if let Err(e) = self.post("/api/chat/send", json!({ "text": text })).await {
            self.awaiting.store(false, Ordering::SeqCst);
            ctx.events.emit(Event::Error {
                kind: "remote".into(),
                message: format!("could not reach remote: {e}"),
                hint: Some("check the URL/password with /remote".into()),
            });
            return DoneReason::Error;
        }

        tokio::select! {
            _ = ct.cancelled() => {
                let _ = self.post("/api/chat/cancel", json!({})).await;
                self.awaiting.store(false, Ordering::SeqCst);
                DoneReason::Cancelled
            }
            _ = self.turn_done.notified() => DoneReason::Completed,
        }
    }
}

/// The persistent reader task: re-emits the remote's event stream locally.
struct Reader {
    base: String,
    client: reqwest::Client,
    cookie: Arc<AsyncMutex<Option<String>>>,
    events: EventEmitter,
    interactor: Interactor,
    state: Arc<Mutex<SessionState>>,
    last_seq: Arc<AtomicU64>,
    turn_done: Arc<Notify>,
    awaiting: Arc<AtomicBool>,
    cancel: CancellationToken,
}

impl Reader {
    async fn run(self) {
        let mut backoff_ms = 500u64;
        // Last error surfaced to the user, so a given failure is reported once
        // (on change) instead of spamming on every reconnect attempt.
        let mut last_err: Option<String> = None;
        loop {
            if self.cancel.is_cancelled() {
                return;
            }
            match self.connect_and_read().await {
                ReadOutcome::Cancelled => return,
                // Connected fine (then the stream dropped) — clear the latch so a
                // later failure is reported again.
                ReadOutcome::Disconnected => last_err = None,
                // Couldn't connect (refused / wrong host) or an HTTP error —
                // surface it ONCE so a misconfigured remote isn't a silent blank
                // pane, then keep retrying quietly.
                ReadOutcome::Failed(msg) => {
                    if last_err.as_deref() != Some(msg.as_str()) {
                        self.events.emit(Event::Error {
                            kind: "remote".into(),
                            message: msg.clone(),
                            hint: Some(
                                "check the instance URL/password with /remote — the gateway \
                                 binds its LAN IP, not 127.0.0.1"
                                    .into(),
                            ),
                        });
                        last_err = Some(msg);
                    }
                }
            }
            // Reconnect with a capped backoff; last_seq avoids replaying events.
            tokio::select! {
                _ = self.cancel.cancelled() => return,
                _ = tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)) => {}
            }
            backoff_ms = (backoff_ms * 2).min(8_000);
        }
    }

    async fn connect_and_read(&self) -> ReadOutcome {
        let url = format!("{}/api/chat/stream", self.base);
        let mut req = self.client.get(&url);
        let last = self.last_seq.load(Ordering::SeqCst);
        if last > 0 {
            req = req.header("last-event-id", last.to_string());
        }
        if let Some(c) = self.cookie.lock().await.as_ref() {
            req = req.header(reqwest::header::COOKIE, c);
        }

        let resp = match req.send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => return ReadOutcome::Failed(format!("remote stream returned {}", r.status())),
            // Couldn't even connect (refused / DNS / wrong host) — reported by
            // run() so a misconfigured remote isn't a silent blank pane.
            Err(_) => return ReadOutcome::Failed(format!("can't reach remote at {}", self.base)),
        };

        let mut stream = resp.bytes_stream().eventsource();
        let mut acc = String::new();
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => return ReadOutcome::Cancelled,
                next = stream.next() => match next {
                    None => return ReadOutcome::Disconnected,
                    Some(Err(_)) => return ReadOutcome::Disconnected,
                    Some(Ok(msg)) => {
                        if let Ok(seq) = msg.id.parse::<u64>() {
                            if seq <= self.last_seq.load(Ordering::SeqCst) {
                                continue; // already seen (replay dedup)
                            }
                            self.last_seq.store(seq, Ordering::SeqCst);
                        }
                        let Ok(event) = serde_json::from_str::<Event>(&msg.data) else {
                            continue;
                        };
                        self.handle(event, &mut acc).await;
                    }
                }
            }
        }
    }

    async fn handle(&self, event: Event, acc: &mut String) {
        match event {
            // The local actor emits its own TurnStarted/TurnDone around run_turn.
            Event::TurnStarted { .. } => acc.clear(),
            Event::TurnDone { .. } => {
                if !acc.is_empty() {
                    self.state
                        .lock()
                        .await
                        .messages
                        .push(Message::assistant(std::mem::take(acc)));
                }
                // Only wake a turn the runner is actually awaiting (skip replay).
                if self.awaiting.swap(false, Ordering::SeqCst) {
                    self.turn_done.notify_one();
                }
            }
            Event::Token { ref text } => {
                acc.push_str(text);
                self.events.emit(event);
            }
            // Forward approvals to the local cards, then answer the remote.
            Event::ApprovalRequest {
                request_id,
                tool,
                summary,
                dangerous,
                diff,
                advice,
            } => {
                let summary = match advice {
                    Some(a) => format!("{summary}\n{a}"),
                    None => summary,
                };
                let (decision, scope) = self
                    .interactor
                    .approve(tool, summary, dangerous, diff, None)
                    .await;
                let _ = self
                    .post_remote(
                        "/api/approval/respond",
                        json!({
                            "request_id": request_id,
                            "decision": decision_str(decision),
                            "scope": scope_str(scope),
                        }),
                    )
                    .await;
            }
            Event::ClarifyRequest {
                request_id,
                question,
                choices,
            } => {
                if let Some(value) = self.interactor.clarify(question, choices).await {
                    let _ = self
                        .post_remote(
                            "/api/clarify/respond",
                            json!({ "request_id": request_id, "value": value }),
                        )
                        .await;
                }
            }
            // Everything else renders locally as-is.
            other => self.events.emit(other),
        }
    }

    async fn post_remote(&self, path: &str, body: serde_json::Value) -> anyhow::Result<()> {
        let url = format!("{}{path}", self.base);
        let mut req = self.client.post(&url).json(&body);
        if let Some(c) = self.cookie.lock().await.as_ref() {
            req = req.header(reqwest::header::COOKIE, c);
        }
        req.send().await?;
        Ok(())
    }
}

enum ReadOutcome {
    Cancelled,
    /// Connected, then the stream dropped — retry quietly.
    Disconnected,
    /// Never connected (refused / wrong host) or an HTTP error — surfaced once.
    Failed(String),
}

fn decision_str(d: Decision) -> &'static str {
    match d {
        Decision::Allow => "allow",
        Decision::Deny => "deny",
    }
}

fn scope_str(s: ApprovalScope) -> &'static str {
    match s {
        ApprovalScope::Once => "once",
        ApprovalScope::Session => "session",
    }
}

/// Build a [`SessionHandle`] backed by a [`RemoteRunner`] for `inst`, seeded with
/// the remote's current transcript so the attach shows the existing conversation
/// (the live phone/grid chats) rather than a blank pane.
pub async fn connect(inst: &RemoteInstance) -> SessionHandle {
    let runner = Arc::new(RemoteRunner::new(inst));
    let id = SessionId::from(format!("remote:{}", inst.name));
    let mut seed = SessionState::new(id, "remote");
    seed.messages = runner.fetch_transcript().await;
    spawn_session_seeded(seed, runner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_and_scope_serialize_to_api_strings() {
        assert_eq!(decision_str(Decision::Allow), "allow");
        assert_eq!(decision_str(Decision::Deny), "deny");
        assert_eq!(scope_str(ApprovalScope::Once), "once");
        assert_eq!(scope_str(ApprovalScope::Session), "session");
    }

    #[test]
    fn base_url_is_normalized() {
        let r = RemoteRunner::new(&RemoteInstance {
            name: "box".into(),
            url: "http://host:8080/".into(),
            password: String::new(),
        });
        assert_eq!(r.base, "http://host:8080");
    }

    #[test]
    fn sse_data_round_trips_into_event() {
        // The remote serializes `Event` with a `type` tag; the reader parses it back.
        let data = r#"{"type":"token","text":"hi"}"#;
        let ev: Event = serde_json::from_str(data).unwrap();
        assert!(matches!(ev, Event::Token { text } if text == "hi"));

        let data = r#"{"type":"notice","message":"hello"}"#;
        let ev: Event = serde_json::from_str(data).unwrap();
        assert!(matches!(ev, Event::Notice { message } if message == "hello"));
    }
}

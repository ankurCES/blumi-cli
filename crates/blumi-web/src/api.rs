//! JSON API + SSE handlers.
//!
//! Client→server actions are discrete POSTs; the agent stream is server→client
//! SSE. Every SSE event carries the monotonic `seq` as its id, so a reconnecting
//! client sends `Last-Event-ID` and we replay the gap before attaching live —
//! the same gap-free attach the TUI gets from the broadcast log.

use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::Json;
use blumi_protocol::{ApprovalScope, Command, Decision, Envelope, RequestId};
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio::sync::broadcast::error::RecvError;

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "model": state.config.model,
        "models": state.config.models,
        "working_dir": state.config.working_dir,
        "version": state.config.version,
    }))
}

pub async fn models(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "model": state.config.model, "models": state.config.models }))
}

pub async fn sessions(State(state): State<AppState>) -> Json<Value> {
    let list = match &state.store {
        Some(store) => store.list_sessions(50).await.unwrap_or_default(),
        None => Vec::new(),
    };
    let arr: Vec<Value> = list
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "title": m.title,
                "model": m.model,
                "updated_at": m.updated_at,
                "message_count": m.message_count,
                "input_tokens": m.input_tokens,
                "output_tokens": m.output_tokens,
            })
        })
        .collect();
    Json(json!({ "sessions": arr }))
}

#[derive(Deserialize)]
pub struct SendBody {
    pub text: String,
}

pub async fn chat_send(State(state): State<AppState>, Json(body): Json<SendBody>) -> Json<Value> {
    let ok = state
        .session
        .send(Command::UserMessage {
            text: body.text,
            attachments: vec![],
            stream_id: None,
        })
        .await
        .is_ok();
    Json(json!({ "ok": ok }))
}

pub async fn chat_cancel(State(state): State<AppState>) -> Json<Value> {
    let ok = state.session.send(Command::Cancel).await.is_ok();
    Json(json!({ "ok": ok }))
}

#[derive(Deserialize)]
pub struct ModelBody {
    pub model: String,
}

pub async fn set_model(State(state): State<AppState>, Json(body): Json<ModelBody>) -> Json<Value> {
    let ok = state
        .session
        .send(Command::SetModel { model: body.model })
        .await
        .is_ok();
    Json(json!({ "ok": ok }))
}

#[derive(Deserialize)]
pub struct ApprovalBody {
    pub request_id: RequestId,
    pub decision: Decision,
    #[serde(default)]
    pub scope: ApprovalScope,
}

pub async fn approval_respond(
    State(state): State<AppState>,
    Json(body): Json<ApprovalBody>,
) -> Json<Value> {
    let ok = state
        .session
        .send(Command::ApproveTool {
            request_id: body.request_id,
            decision: body.decision,
            scope: body.scope,
        })
        .await
        .is_ok();
    Json(json!({ "ok": ok }))
}

#[derive(Deserialize)]
pub struct ClarifyBody {
    pub request_id: RequestId,
    pub value: String,
}

pub async fn clarify_respond(
    State(state): State<AppState>,
    Json(body): Json<ClarifyBody>,
) -> Json<Value> {
    let ok = state
        .session
        .send(Command::AnswerClarify {
            request_id: body.request_id,
            value: body.value,
        })
        .await
        .is_ok();
    Json(json!({ "ok": ok }))
}

/// SSE: replay missed events (`Last-Event-ID`) then stream live ones.
pub async fn chat_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let last = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    // Subscribe before reading the backlog so nothing slips through the gap.
    let mut rx = state.session.subscribe();
    let backlog = state.session.events_since(last);

    let stream = async_stream::stream! {
        let mut high = last;
        for env in backlog {
            if env.seq > high {
                high = env.seq;
                yield Ok(to_sse(&env));
            }
        }
        loop {
            match rx.recv().await {
                Ok(env) => {
                    if env.seq > high {
                        high = env.seq;
                        yield Ok(to_sse(&env));
                    }
                }
                Err(RecvError::Lagged(_)) => continue, // healed by seq dedup + replay
                Err(RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn to_sse(env: &Envelope) -> SseEvent {
    let data = serde_json::to_string(&env.event).unwrap_or_else(|_| "{}".into());
    SseEvent::default()
        .id(env.seq.to_string())
        .event(env.event.name())
        .data(data)
}

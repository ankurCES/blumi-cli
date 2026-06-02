//! JSON API + SSE handlers.
//!
//! Client→server actions are discrete POSTs; the agent stream is server→client
//! SSE. Every SSE event carries the monotonic `seq` as its id, so a reconnecting
//! client sends `Last-Event-ID` and we replay the gap before attaching live.
//! Handlers act on the *current* session, so live session switch/resume just
//! re-points `AppState` and clients re-subscribe.

use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::Json;
use blumi_protocol::{ApprovalScope, Command, Decision, Envelope, RequestId, Role};
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
        "persona": state.config.persona,
        "context_size": state.config.context_size,
        "auth_required": state.auth().is_some(),
    }))
}

pub async fn models(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "model": state.config.model, "models": state.config.models }))
}

pub async fn personas(State(state): State<AppState>) -> Json<Value> {
    let list: Vec<Value> = state
        .config
        .personas
        .iter()
        .map(|(name, desc)| json!({ "name": name, "description": desc }))
        .collect();
    Json(json!({ "personas": list, "active": state.config.persona }))
}

#[derive(Deserialize)]
pub struct PersonaBody {
    pub name: String,
}

pub async fn set_persona(
    State(state): State<AppState>,
    Json(body): Json<PersonaBody>,
) -> Json<Value> {
    let ok = state
        .current()
        .await
        .send(Command::SetPersona { name: body.name })
        .await
        .is_ok();
    Json(json!({ "ok": ok }))
}

pub async fn sessions(State(state): State<AppState>) -> Json<Value> {
    let arr: Vec<Value> = state
        .provider()
        .list()
        .await
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "title": s.title,
                "model": s.model,
                "message_count": s.message_count,
            })
        })
        .collect();
    Json(json!({ "sessions": arr }))
}

pub async fn session_new(State(state): State<AppState>) -> Json<Value> {
    match state.provider().create().await {
        Ok(handle) => {
            state.swap(handle).await;
            Json(json!({ "ok": true }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
pub struct ResumeBody {
    pub id: String,
}

pub async fn session_resume(
    State(state): State<AppState>,
    Json(body): Json<ResumeBody>,
) -> Json<Value> {
    match state.provider().resume(&body.id).await {
        Ok(handle) => {
            state.swap(handle).await;
            Json(json!({ "ok": true }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// Rebuild the agent in place (self-evolution) so newly written skills + config
/// edits take effect, preserving the conversation. The client calls this when it
/// sees a `reload` event, then re-subscribes + restores the transcript.
pub async fn session_reload(State(state): State<AppState>) -> Json<Value> {
    match state.reload_current().await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// The current session's transcript (for restore-on-load and after a switch).
pub async fn messages(State(state): State<AppState>) -> Json<Value> {
    let snap = state.current().await.snapshot().await;
    let arr: Vec<Value> = snap
        .messages
        .iter()
        .filter_map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                Role::System => return None,
            };
            Some(json!({ "role": role, "text": m.text(), "tool_name": m.tool_name }))
        })
        .collect();
    Json(json!({
        "messages": arr,
        "model": snap.model,
        "input_tokens": snap.total_input_tokens,
        "output_tokens": snap.total_output_tokens,
        "turn_count": snap.turn_count,
    }))
}

#[derive(Deserialize)]
pub struct SendBody {
    pub text: String,
}

pub async fn chat_send(State(state): State<AppState>, Json(body): Json<SendBody>) -> Json<Value> {
    let ok = state
        .current()
        .await
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
    let ok = state.current().await.send(Command::Cancel).await.is_ok();
    Json(json!({ "ok": ok }))
}

pub async fn compact(State(state): State<AppState>) -> Json<Value> {
    let ok = state.current().await.send(Command::Compact).await.is_ok();
    Json(json!({ "ok": ok }))
}

pub async fn undo(State(state): State<AppState>) -> Json<Value> {
    let ok = state.current().await.send(Command::Undo).await.is_ok();
    Json(json!({ "ok": ok }))
}

#[derive(Deserialize)]
pub struct YoloBody {
    pub on: bool,
}

pub async fn set_yolo(State(state): State<AppState>, Json(body): Json<YoloBody>) -> Json<Value> {
    let ok = state
        .current()
        .await
        .send(Command::SetYolo { on: body.on })
        .await
        .is_ok();
    Json(json!({ "ok": ok }))
}

#[derive(Deserialize)]
pub struct ModelBody {
    pub model: String,
}

pub async fn set_model(State(state): State<AppState>, Json(body): Json<ModelBody>) -> Json<Value> {
    let ok = state
        .current()
        .await
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
        .current()
        .await
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
        .current()
        .await
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

    let session = state.current().await;
    // Subscribe before reading the backlog so nothing slips through the gap.
    let mut rx = session.subscribe();
    let backlog = session.events_since(last);

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

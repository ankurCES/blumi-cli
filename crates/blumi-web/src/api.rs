//! JSON API + SSE handlers.
//!
//! Client→server actions are discrete POSTs; the agent stream is server→client
//! SSE. Every SSE event carries the monotonic `seq` as its id, so a reconnecting
//! client sends `Last-Event-ID` and we replay the gap before attaching live.
//! Handlers act on the *current* session, so live session switch/resume just
//! re-points `AppState` and clients re-subscribe.

use crate::AppState;
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use blumi_protocol::{ApprovalScope, Command, Decision, Envelope, Event, RequestId, Role};
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio::sync::broadcast::error::RecvError;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "uptime_secs": state.uptime_secs(),
        // Whether a service manager will auto-restart us on crash (self-recovery).
        "service_managed": state.mgmt().restart_capability() == "service",
    }))
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
        "voice_enabled": state.mgmt().voice_config().is_some(),
        // The machine name, so a remote client (e.g. blugo) can label this
        // gateway with the host it's running on.
        "hostname": whoami::fallible::hostname().unwrap_or_else(|_| "blumi".to_string()),
    }))
}

pub async fn models(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "options": state.mgmt().model_options() }))
}

#[derive(Deserialize)]
pub struct ProviderBody {
    pub provider: String,
    /// Optional API key to store for this provider (for unconfigured ones).
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Switch the active provider: persist it (+ a default model, + an optional key),
/// then rebuild the session so the new provider's client is used. The
/// conversation is preserved.
pub async fn provider_set(
    State(state): State<AppState>,
    Json(body): Json<ProviderBody>,
) -> Json<Value> {
    if let Err(e) = state
        .mgmt()
        .set_provider(&body.provider, body.api_key.as_deref())
    {
        return Json(json!({ "ok": false, "error": e.to_string() }));
    }
    match state.reload_current().await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
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
    // A present `Last-Event-ID` means "reconnect — replay the gap to heal".
    // An absent one means "fresh connect": the client already loaded the
    // transcript via /api/messages, so we send only *live* events and skip the
    // history backlog (replaying it would duplicate messages on the client).
    let last = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let session = state.current().await;
    // Subscribe before reading the backlog so nothing slips through the gap.
    let mut rx = session.subscribe();
    let backlog = session.events_since(last.unwrap_or(0));
    let head = backlog.last().map(|e| e.seq).unwrap_or(0);
    let replay = last.is_some();
    let start = last.unwrap_or(head); // fresh connect starts at the current head

    let stream = async_stream::stream! {
        let mut high = start;
        if replay {
            for env in backlog {
                if env.seq > high {
                    high = env.seq;
                    yield Ok(to_sse(&env));
                }
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

// ── Control center (cron / skills / memory / usage) ─────────────────────────

pub async fn cron_list(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "jobs": state.mgmt().cron_list().await }))
}

#[derive(Deserialize)]
pub struct CronAddBody {
    pub name: String,
    pub schedule: String,
    pub prompt: String,
}

pub async fn cron_add(State(state): State<AppState>, Json(b): Json<CronAddBody>) -> Json<Value> {
    match state.mgmt().cron_add(&b.name, &b.schedule, &b.prompt).await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
pub struct CronRemoveBody {
    pub id: String,
}

pub async fn cron_remove(
    State(state): State<AppState>,
    Json(b): Json<CronRemoveBody>,
) -> Json<Value> {
    match state.mgmt().cron_remove(&b.id).await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

pub async fn skills(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "skills": state.mgmt().skills() }))
}

pub async fn tasks(State(state): State<AppState>) -> Json<Value> {
    Json(state.mgmt().tasks())
}

/// Discovered grid peers: `{ self: {...}, peers: [...] }` (or disabled).
pub async fn grid_peers(State(state): State<AppState>) -> Json<Value> {
    Json(state.mgmt().grid_peers())
}

/// Loop status: { running, iter, current }.
pub async fn loop_status(State(state): State<AppState>) -> Json<Value> {
    let st = state.loop_status().read().await.clone();
    Json(json!({ "running": st.running, "iter": st.iter, "current": st.current }))
}

#[derive(Deserialize, Default)]
pub struct LoopBody {
    #[serde(default)]
    pub review: bool,
    /// "local" (default) or "grid". In grid mode the loop dispatches each task
    /// to a live peer (round-robin), falling back to local when no peers exist.
    #[serde(default)]
    pub mode: Option<String>,
}

/// Start the autonomous loop: work the task board top-down against the current
/// session (streaming over SSE). No-op if already running. Uses the session's
/// own yolo/approval policy — approvals still surface to clients.
pub async fn loop_start(State(state): State<AppState>, Json(body): Json<LoopBody>) -> Json<Value> {
    {
        let mut st = state.loop_status().write().await;
        if st.running {
            return Json(json!({ "ok": false, "error": "loop already running" }));
        }
        st.running = true;
        st.iter = 0;
        st.current = String::new();
    }
    let runner = state.clone();
    let grid = body.mode.as_deref() == Some("grid");
    tokio::spawn(async move { run_loop(runner, body.review, grid).await });
    Json(json!({ "ok": true }))
}

pub async fn loop_stop(State(state): State<AppState>) -> Json<Value> {
    state.loop_status().write().await.running = false;
    Json(json!({ "ok": true }))
}

/// The loop body: pull the next todo, run it on the current session, advance it,
/// repeat until the board is empty or someone stops it.
async fn run_loop(state: AppState, review: bool, grid: bool) {
    let mut rr: usize = 0; // round-robin cursor over live peers
    let mut fails: usize = 0; // consecutive grid dispatch failures
    loop {
        if !state.loop_status().read().await.running {
            break;
        }

        // Grid mode: dispatch the next todo to a live peer (round-robin), which
        // runs it on its own runtime. Falls back to local when no peers exist.
        if grid {
            let peers = state.mgmt().grid_peer_ids();
            if !peers.is_empty() {
                let Some(todo) = state.mgmt().task_peek_next() else {
                    break;
                };
                let id = todo["id"].as_str().unwrap_or_default().to_string();
                let peer = peers[rr % peers.len()].clone();
                rr += 1;
                {
                    let mut st = state.loop_status().write().await;
                    st.iter += 1;
                    st.current = format!("{} @ {peer}", todo["title"].as_str().unwrap_or_default());
                }
                let res = state.mgmt().grid_dispatch(&id, &peer, review).await;
                if res["ok"].as_bool() == Some(true) {
                    fails = 0;
                } else {
                    fails += 1;
                    // Every live peer failed this task — stop to avoid a hot loop.
                    if fails > peers.len() {
                        break;
                    }
                }
                continue;
            }
            // No live peers → fall through to local execution.
        }

        let Some(todo) = state.mgmt().task_next() else {
            break;
        };
        let id = todo["id"].as_str().unwrap_or_default().to_string();
        let prompt = todo["prompt"].as_str().unwrap_or_default().to_string();
        {
            let mut st = state.loop_status().write().await;
            st.iter += 1;
            st.current = todo["title"].as_str().unwrap_or_default().to_string();
        }

        let session = state.current().await;
        let mut events = session.subscribe();
        if session
            .send(Command::UserMessage {
                text: prompt,
                attachments: vec![],
                stream_id: None,
            })
            .await
            .is_err()
        {
            break;
        }

        // Wait for this turn to finish (or a stop request).
        loop {
            tokio::select! {
                r = events.recv() => match r {
                    Ok(env) => {
                        if matches!(env.event, Event::TurnDone { .. }) { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                    if !state.loop_status().read().await.running { break; }
                }
            }
        }

        state.mgmt().task_advance(&id, review);
    }
    state.loop_status().write().await.running = false;
}

// --- Grid: dispatch (orchestrator) + run (peer) ----------------------------

#[derive(Deserialize)]
pub struct GridDispatchBody {
    pub task_id: String,
    pub peer_id: String,
    #[serde(default)]
    pub review: bool,
}

/// Orchestrator-side: dispatch a board task to a grid peer (human-authed). The
/// task is claimed (doing + owner), run on the peer's runtime, then advanced or
/// released. Returns the dispatch status.
pub async fn grid_dispatch(
    State(state): State<AppState>,
    Json(body): Json<GridDispatchBody>,
) -> Json<Value> {
    Json(
        state
            .mgmt()
            .grid_dispatch(&body.task_id, &body.peer_id, body.review)
            .await,
    )
}

#[derive(Deserialize)]
pub struct GridRunBody {
    pub prompt: String,
}

/// Peer-side grid execution: run `prompt` as one turn on THIS node's session and
/// return when it finishes. Authenticated by the shared grid secret
/// (`X-Blumi-Grid` header), not the human password. Runs autonomously (yolo)
/// since there's no human at the peer to answer approvals.
pub async fn grid_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GridRunBody>,
) -> Response {
    let Some(secret) = state.grid_secret() else {
        return (StatusCode::NOT_FOUND, "grid disabled").into_response();
    };
    let presented = headers
        .get("x-blumi-grid")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !constant_eq(secret.as_bytes(), presented.as_bytes()) {
        return (StatusCode::UNAUTHORIZED, "grid auth required").into_response();
    }
    if body.prompt.trim().is_empty() {
        return Json(json!({ "ok": false, "error": "empty prompt" })).into_response();
    }

    let session = state.current().await;
    let mut events = session.subscribe();
    // Run autonomously — no human here to answer approval prompts.
    let _ = session.send(Command::SetYolo { on: true }).await;
    if session
        .send(Command::UserMessage {
            text: body.prompt,
            attachments: vec![],
            stream_id: None,
        })
        .await
        .is_err()
    {
        return Json(json!({ "ok": false, "error": "session unavailable" })).into_response();
    }

    // Wait for the turn to finish, bounded by a generous timeout.
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(900));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            r = events.recv() => match r {
                Ok(env) => {
                    if matches!(env.event, Event::TurnDone { .. }) {
                        return Json(json!({ "ok": true, "summary": "completed" })).into_response();
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => {
                    return Json(json!({ "ok": false, "error": "stream closed" }))
                        .into_response();
                }
            },
            _ = &mut deadline => {
                return Json(json!({ "ok": false, "error": "timed out" })).into_response();
            }
        }
    }
}

/// Constant-time byte compare (length is allowed to leak, contents are not).
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// --- Self-management endpoints ----------------------------------------------

/// GET /api/self/config → `{ settings: { …secrets redacted… } }`.
pub async fn self_config_get(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "settings": state.mgmt().self_config_get() }))
}

#[derive(Deserialize)]
pub struct SelfConfigSetBody {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub reload: bool,
}

/// POST /api/self/config `{ key, value, reload? }`.
pub async fn self_config_set(
    State(state): State<AppState>,
    Json(body): Json<SelfConfigSetBody>,
) -> Json<Value> {
    match state.mgmt().self_config_set(&body.key, &body.value) {
        Ok(msg) => {
            let reloaded = body.reload && state.reload_current().await.is_ok();
            Json(json!({ "ok": true, "message": msg, "reloaded": reloaded }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
pub struct SkillWriteBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub reload: bool,
}

/// POST /api/skills `{ name, description, instructions, reload? }` (create/update).
pub async fn skills_write(
    State(state): State<AppState>,
    Json(body): Json<SkillWriteBody>,
) -> Json<Value> {
    match state
        .mgmt()
        .skill_write(&body.name, &body.description, &body.instructions)
    {
        Ok(()) => {
            let reloaded = body.reload && state.reload_current().await.is_ok();
            Json(json!({ "ok": true, "reloaded": reloaded }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
pub struct SkillDeleteBody {
    pub name: String,
    #[serde(default)]
    pub reload: bool,
}

/// POST /api/skills/delete `{ name, reload? }`.
pub async fn skills_delete(
    State(state): State<AppState>,
    Json(body): Json<SkillDeleteBody>,
) -> Json<Value> {
    match state.mgmt().skill_delete(&body.name) {
        Ok(()) => {
            let reloaded = body.reload && state.reload_current().await.is_ok();
            Json(json!({ "ok": true, "reloaded": reloaded }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(Deserialize, Default)]
pub struct RestartBody {
    #[serde(default)]
    pub confirm: bool,
}

/// POST /api/self/restart `{ confirm }` — restart the gateway service (requires
/// `confirm:true`). Degrades to an in-place reload when not service-managed.
pub async fn self_restart(
    State(state): State<AppState>,
    Json(body): Json<RestartBody>,
) -> Json<Value> {
    if !body.confirm {
        return Json(json!({ "ok": false, "error": "restart requires confirm:true" }));
    }
    match state.mgmt().restart_capability() {
        "service" => Json(state.mgmt().restart()),
        "foreground" => {
            let _ = state.reload_current().await;
            Json(json!({
                "ok": true, "mode": "reload",
                "detail": "not under a service manager — reloaded in place instead of restarting"
            }))
        }
        _ => Json(json!({
            "ok": false, "mode": "unsupported", "error": "restart not supported on this host"
        })),
    }
}

/// POST /api/self/recover — try a reload; escalate to a restart if it fails or
/// hangs (a wedged session).
pub async fn self_recover(State(state): State<AppState>) -> Json<Value> {
    let reload =
        tokio::time::timeout(std::time::Duration::from_secs(10), state.reload_current()).await;
    match reload {
        Ok(Ok(())) => Json(json!({ "ok": true, "action": "reload" })),
        _ => {
            if state.mgmt().restart_capability() == "service" {
                let out = state.mgmt().restart();
                Json(json!({ "ok": true, "action": "restart", "detail": out }))
            } else {
                Json(json!({
                    "ok": false, "action": "reload_failed",
                    "error": "reload failed and no service manager to restart"
                }))
            }
        }
    }
}

/// Aggregate runtime status for the dashboard: uptime + the active config +
/// usage snapshot (cost/tokens). Live context/tokens also stream over SSE.
pub async fn status(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "uptime_secs": state.uptime_secs(),
        "model": state.config.model,
        "version": state.config.version,
        "working_dir": state.config.working_dir,
        "context_size": state.config.context_size,
        "usage": state.mgmt().usage().await,
    }))
}

pub async fn memory_get(State(state): State<AppState>) -> Json<Value> {
    let (memory, user) = state.mgmt().memory();
    Json(json!({ "memory": memory, "user": user }))
}

#[derive(Deserialize)]
pub struct MemorySetBody {
    pub which: String,
    pub content: String,
}

pub async fn memory_set(
    State(state): State<AppState>,
    Json(b): Json<MemorySetBody>,
) -> Json<Value> {
    match state.mgmt().memory_set(&b.which, &b.content) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

pub async fn usage(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "usage": state.mgmt().usage().await }))
}

pub async fn settings_get(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "settings": state.mgmt().settings_view() }))
}

pub async fn settings_set(
    State(state): State<AppState>,
    Json(patch): Json<crate::SettingsPatch>,
) -> Json<Value> {
    match state.mgmt().settings_apply(patch) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

// ── Voice (STT / TTS) ───────────────────────────────────────────────────────

/// POST /api/voice/transcribe — raw audio body in, `{ text }` out.
pub async fn voice_transcribe(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(cfg) = state.mgmt().voice_config() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "voice not enabled" })),
        )
            .into_response();
    };
    let mime = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("audio/webm")
        .to_string();
    let ext = mime
        .rsplit('/')
        .next()
        .unwrap_or("webm")
        .split(';')
        .next()
        .unwrap_or("webm");
    match blumi_voice::transcribe(&cfg, body.to_vec(), &format!("audio.{ext}"), &mime).await {
        Ok(text) => Json(json!({ "text": text })).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct SpeakBody {
    pub text: String,
}

/// POST /api/voice/speak — `{ text }` in, audio bytes out.
pub async fn voice_speak(State(state): State<AppState>, Json(b): Json<SpeakBody>) -> Response {
    let Some(cfg) = state.mgmt().voice_config() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "voice not enabled").into_response();
    };
    match blumi_voice::synthesize(&cfg, &b.text).await {
        Ok(bytes) => ([(CONTENT_TYPE, "audio/mpeg")], bytes).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

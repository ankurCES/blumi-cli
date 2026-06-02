//! axum server + embedded React build.
//!
//! The server holds one [`SessionHandle`] in state; the React UI drives turns
//! with discrete POSTs and watches the agent via a single SSE stream. This is
//! the same event/command core the TUI uses — the web is just another
//! subscriber, so both UIs stay in lockstep with zero duplicated agent logic.

mod api;
mod assets;

use axum::routing::{get, post};
use axum::Router;
use blumi_core::SessionHandle;
use blumi_persist::Store;
use std::net::SocketAddr;
use std::sync::Arc;

/// Static info the server reports at `/api/config` (and the UI displays).
#[derive(Clone)]
pub struct WebConfig {
    pub model: String,
    pub models: Vec<String>,
    pub working_dir: String,
    pub version: String,
    /// Available personas as (name, description).
    pub personas: Vec<(String, String)>,
    /// The active persona name at startup.
    pub persona: String,
}

/// Shared server state — cheaply cloneable (a handle + Arcs).
#[derive(Clone)]
pub struct AppState {
    pub session: SessionHandle,
    pub store: Option<Arc<Store>>,
    pub config: Arc<WebConfig>,
}

/// Build the axum router for a given state.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(api::health))
        .route("/api/config", get(api::config))
        .route("/api/models", get(api::models))
        .route("/api/model/set", post(api::set_model))
        .route("/api/personas", get(api::personas))
        .route("/api/persona/set", post(api::set_persona))
        .route("/api/sessions", get(api::sessions))
        .route("/api/chat/send", post(api::chat_send))
        .route("/api/chat/cancel", post(api::chat_cancel))
        .route("/api/yolo", post(api::set_yolo))
        .route("/api/chat/stream", get(api::chat_stream))
        .route("/api/approval/respond", post(api::approval_respond))
        .route("/api/clarify/respond", post(api::clarify_respond))
        .fallback(assets::static_handler)
        .with_state(state)
}

/// Serve the web UI + API on `addr` until the process exits.
pub async fn serve(
    session: SessionHandle,
    store: Option<Arc<Store>>,
    config: WebConfig,
    addr: SocketAddr,
) -> anyhow::Result<()> {
    let state = AppState {
        session,
        store,
        config: Arc::new(config),
    };
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    tracing::info!("blumi web serving on http://{local}");
    axum::serve(listener, app).await?;
    Ok(())
}

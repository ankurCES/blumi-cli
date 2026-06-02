//! axum server + embedded React build.
//!
//! The server holds the *current* [`SessionHandle`] (swappable for live
//! switch/resume); the React UI drives turns with discrete POSTs and watches the
//! agent via a single SSE stream. This is the same event/command core the TUI
//! uses — the web is just another subscriber, so both UIs stay in lockstep.

mod api;
mod assets;
mod auth;

pub use auth::Auth;

use axum::routing::{get, post};
use axum::Router;
use blumi_core::SessionHandle;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    /// Model context window (for the usage bar).
    pub context_size: u32,
}

/// A stored session summary for the sidebar.
#[derive(Clone)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub model: String,
    pub message_count: i64,
}

/// Creates / resumes / lists / saves sessions for the web server — the seam the
/// binary implements over the engine + the persistence store.
#[async_trait::async_trait]
pub trait SessionProvider: Send + Sync {
    async fn create(&self) -> anyhow::Result<SessionHandle>;
    async fn resume(&self, id: &str) -> anyhow::Result<SessionHandle>;
    /// Rebuild the agent in place (self-evolution): re-read config + re-scan
    /// skills, seeded with the live snapshot so the conversation is preserved.
    async fn reload(&self, snapshot: blumi_core::SessionSnapshot) -> anyhow::Result<SessionHandle>;
    async fn list(&self) -> Vec<SessionInfo>;
    async fn save(&self, handle: &SessionHandle);
}

/// A scheduled cron job, for the control center.
#[derive(Clone, serde::Serialize)]
pub struct CronJobInfo {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub prompt: String,
}

/// A discovered skill (name + description + body), for the control center.
#[derive(Clone, serde::Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// Per-model usage rollup.
#[derive(Clone, serde::Serialize)]
pub struct ModelUsage {
    pub model: String,
    pub sessions: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Aggregate usage insights across stored sessions.
#[derive(Clone, Default, serde::Serialize)]
pub struct UsageStats {
    pub sessions: u64,
    pub messages: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub by_model: Vec<ModelUsage>,
}

/// Control-center data + actions (cron, skills, memory, usage) — the seam the
/// binary implements over the cron store, skill catalog, memory files, and the
/// persistence store.
#[async_trait::async_trait]
pub trait Management: Send + Sync {
    async fn cron_list(&self) -> Vec<CronJobInfo>;
    async fn cron_add(&self, name: &str, schedule: &str, prompt: &str) -> anyhow::Result<()>;
    async fn cron_remove(&self, id: &str) -> anyhow::Result<()>;
    fn skills(&self) -> Vec<SkillInfo>;
    /// Returns (MEMORY.md, USER.md) contents.
    fn memory(&self) -> (String, String);
    /// `which` is "memory" or "user".
    fn memory_set(&self, which: &str, content: &str) -> anyhow::Result<()>;
    async fn usage(&self) -> UsageStats;
}

/// Shared server state.
#[derive(Clone)]
pub struct AppState {
    session: Arc<RwLock<SessionHandle>>,
    provider: Arc<dyn SessionProvider>,
    mgmt: Arc<dyn Management>,
    pub config: Arc<WebConfig>,
    auth: Option<Arc<Auth>>,
}

impl AppState {
    /// A clone of the current session handle.
    pub(crate) async fn current(&self) -> SessionHandle {
        self.session.read().await.clone()
    }

    /// Persist the current session, then make `next` current.
    pub(crate) async fn swap(&self, next: SessionHandle) {
        let old = self.session.read().await.clone();
        self.provider.save(&old).await;
        *self.session.write().await = next;
    }

    /// Rebuild the current session in place (self-evolution): snapshot it, ask
    /// the provider to reload (fresh config + skills) seeded from that snapshot,
    /// then swap. The conversation is preserved.
    pub(crate) async fn reload_current(&self) -> anyhow::Result<()> {
        let snapshot = self.current().await.snapshot().await;
        let next = self.provider.reload(snapshot).await?;
        self.swap(next).await;
        Ok(())
    }

    pub(crate) fn provider(&self) -> &Arc<dyn SessionProvider> {
        &self.provider
    }

    pub(crate) fn mgmt(&self) -> &Arc<dyn Management> {
        &self.mgmt
    }

    pub(crate) fn auth(&self) -> Option<&Arc<Auth>> {
        self.auth.as_ref()
    }
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
        .route("/api/session/new", post(api::session_new))
        .route("/api/session/resume", post(api::session_resume))
        .route("/api/session/reload", post(api::session_reload))
        .route("/api/messages", get(api::messages))
        .route("/api/chat/send", post(api::chat_send))
        .route("/api/chat/cancel", post(api::chat_cancel))
        .route("/api/compact", post(api::compact))
        .route("/api/undo", post(api::undo))
        .route("/api/yolo", post(api::set_yolo))
        .route("/api/chat/stream", get(api::chat_stream))
        .route("/api/approval/respond", post(api::approval_respond))
        .route("/api/clarify/respond", post(api::clarify_respond))
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/cron", get(api::cron_list).post(api::cron_add))
        .route("/api/cron/remove", post(api::cron_remove))
        .route("/api/skills", get(api::skills))
        .route("/api/memory", get(api::memory_get).post(api::memory_set))
        .route("/api/usage", get(api::usage))
        .fallback(assets::static_handler)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .with_state(state)
}

/// Serve the web UI + API on `addr`, sourcing sessions from `provider`. When
/// `auth` is `Some`, every data route requires a session cookie.
pub async fn serve(
    provider: Arc<dyn SessionProvider>,
    mgmt: Arc<dyn Management>,
    config: WebConfig,
    addr: SocketAddr,
    auth: Option<Auth>,
) -> anyhow::Result<()> {
    let session = provider.create().await?;
    let state = AppState {
        session: Arc::new(RwLock::new(session)),
        provider,
        mgmt,
        config: Arc::new(config),
        auth: auth.map(Arc::new),
    };
    let app = router(state.clone());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    tracing::info!("blumi web serving on http://{local}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    // Persist the active session on shutdown (best-effort).
    state.provider.save(&state.current().await).await;
    Ok(())
}

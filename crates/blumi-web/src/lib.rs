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
use std::collections::HashMap;
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
    /// Create a fresh session bound to a caller-chosen id (for dedicated dispatch
    /// threads addressed by a stable id). Default: unsupported.
    async fn create_with_id(&self, _id: &str) -> anyhow::Result<SessionHandle> {
        anyhow::bail!("create_with_id not supported")
    }
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

/// A selectable provider for the header picker.
#[derive(Clone, serde::Serialize)]
pub struct ProviderOption {
    pub name: String,
    pub label: String,
    /// Whether it has a usable key (or needs none) — unready ones are disabled.
    pub ready: bool,
}

/// Active provider/model + suggestions for the header picker (read live).
#[derive(Clone, Default, serde::Serialize)]
pub struct ModelOptions {
    pub provider: String,
    pub model: String,
    /// Suggested model ids for the active provider (the current model included).
    pub models: Vec<String>,
    pub providers: Vec<ProviderOption>,
}

/// Editable settings exposed to the control center. Secrets are never sent to
/// the client — only a `*_set` flag indicates whether one is configured.
#[derive(Clone, Default, serde::Serialize)]
pub struct SettingsView {
    pub voice: VoiceView,
    pub gateway: GatewayView,
    pub brain: BrainView,
}

/// Local-LLM "brain" approval settings (claudectl-style).
#[derive(Clone, Default, serde::Serialize)]
pub struct BrainView {
    /// "off" | "advisory" | "auto".
    pub mode: String,
    /// Provider name the brain judges with (empty = reuse main client).
    pub provider: String,
    /// Model id the brain judges with (empty = reuse main model).
    pub model: String,
}

#[derive(Clone, Default, serde::Serialize)]
pub struct VoiceView {
    pub enabled: bool,
    pub stt_base_url: String,
    pub stt_model: String,
    pub tts_provider: String,
    pub tts_base_url: String,
    pub tts_model: String,
    pub tts_voice: String,
    pub api_key_set: bool,
    pub tts_api_key_set: bool,
}

#[derive(Clone, Default, serde::Serialize)]
pub struct GatewayView {
    pub yolo: bool,
    pub telegram_token_set: bool,
    pub discord_token_set: bool,
    pub slack_bot_token_set: bool,
    pub slack_app_token_set: bool,
    pub whatsapp_token_set: bool,
    pub whatsapp_phone_number_id: String,
    pub whatsapp_verify_token: String,
}

/// A settings update from the control center. All fields optional; secret fields
/// are applied only when non-empty (blank = keep the existing secret).
#[derive(Default, serde::Deserialize)]
pub struct SettingsPatch {
    pub voice_enabled: Option<bool>,
    pub stt_base_url: Option<String>,
    pub stt_model: Option<String>,
    pub voice_api_key: Option<String>,
    pub tts_provider: Option<String>,
    pub tts_base_url: Option<String>,
    pub tts_model: Option<String>,
    pub tts_voice: Option<String>,
    pub tts_api_key: Option<String>,
    pub gateway_yolo: Option<bool>,
    pub telegram_token: Option<String>,
    pub discord_token: Option<String>,
    pub slack_bot_token: Option<String>,
    pub slack_app_token: Option<String>,
    pub whatsapp_token: Option<String>,
    pub whatsapp_phone_number_id: Option<String>,
    pub whatsapp_verify_token: Option<String>,
    pub brain_mode: Option<String>,
    pub brain_provider: Option<String>,
    pub brain_model: Option<String>,
}

/// Control-center data + actions (cron, skills, memory, usage, settings) — the
/// seam the binary implements over the cron store, skill catalog, memory files,
/// the persistence store, and settings.json.
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
    /// Current voice + gateway settings (secrets redacted to `*_set` flags).
    fn settings_view(&self) -> SettingsView;
    /// Apply a settings patch to settings.json.
    fn settings_apply(&self, patch: SettingsPatch) -> anyhow::Result<()>;
    /// The live voice config (read fresh), or `None` when voice is disabled.
    fn voice_config(&self) -> Option<blumi_voice::VoiceConfig>;
    /// Active provider/model + suggestions + selectable providers (read live).
    fn model_options(&self) -> ModelOptions;
    /// Persist the active provider (+ a default model) to settings.json, plus an
    /// optional API key for that provider. The caller reloads to apply it.
    fn set_provider(&self, provider: &str, api_key: Option<&str>) -> anyhow::Result<()>;
    /// The persistent task board as JSON (`{ tasks: [...], counts: {...} }`).
    fn tasks(&self) -> serde_json::Value;
    /// Take the highest-priority todo, mark it doing, and return
    /// `{ id, prompt, title }` — or `None` when the board has no todos.
    fn task_next(&self) -> Option<serde_json::Value>;
    /// Advance a task to done (or review).
    fn task_advance(&self, id: &str, review: bool);
    /// Discovered grid peers as JSON (`{ self: {...}, peers: [...] }`), or
    /// `{ enabled: false, peers: [] }` when the grid is disabled. Default off.
    fn grid_peers(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false, "peers": [] })
    }
    /// Live grid peer ids (mDNS fullnames) for round-robin dispatch. Empty when
    /// the grid is disabled or no peers are online.
    fn grid_peer_ids(&self) -> Vec<String> {
        Vec::new()
    }
    /// Per-peer metrics: query each live peer's `/api/grid/node` and return a
    /// JSON array of `{ id, name, host, port, online, metrics }`. Default: empty.
    async fn grid_peer_metrics(&self) -> serde_json::Value {
        serde_json::json!([])
    }
    /// The next todo as `{ id, prompt, title }` WITHOUT marking it doing
    /// (read-only peek), or `None`. Used by grid dispatch to claim with an owner.
    fn task_peek_next(&self) -> Option<serde_json::Value> {
        None
    }
    /// Dispatch a task to a grid peer: claim it (doing + owner), run it on the
    /// peer's runtime, then advance (done/review) or release (→ todo) it.
    /// Returns a JSON status. Default: grid disabled.
    async fn grid_dispatch(
        &self,
        _task_id: &str,
        _peer_id: &str,
        _review: bool,
    ) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "grid disabled" })
    }

    /// Delegate a free-form prompt over the grid: run it on `target` ("all" or
    /// empty = every live peer concurrently; else a peer name / host / host:port)
    /// and return each peer's output. Deterministic — does NOT depend on the
    /// model choosing to call a tool. Default: grid disabled.
    async fn grid_delegate(&self, _prompt: &str, _target: &str) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "grid disabled" })
    }

    /// Receive a memory diffused from a grid peer (SEDM cross-peer knowledge
    /// diffusion). Re-admitted locally through the dedup gate and tagged with the
    /// sender's `origin` so it never re-diffuses. Default: memory disabled.
    async fn grid_memory_ingest(
        &self,
        _namespace: &str,
        _kind: &str,
        _text: &str,
        _origin: &str,
    ) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "memory disabled" })
    }

    /// Embed texts with this node's local embedder — serves grid-embed offload
    /// (`POST /api/grid/embed`) from CPU peers. Default: `None` (no embedder).
    async fn embed(&self, _texts: Vec<String>) -> Option<Vec<Vec<f32>>> {
        None
    }

    // --- Knowledge base / memory browser (UI) ---

    /// Code-KB totals (files/symbols/vectors) + ingest-job state. Default: off.
    async fn knowledge_status(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false })
    }
    /// Indexed sources `[{ source, files, symbols }]`. Default: empty.
    async fn knowledge_sources(&self) -> serde_json::Value {
        serde_json::json!({ "sources": [] })
    }
    /// Hybrid code search → `{ hits: [{ path, name, kind, start_line, snippet }] }`.
    async fn knowledge_search(&self, _query: &str, _limit: u32) -> serde_json::Value {
        serde_json::json!({ "hits": [] })
    }
    /// Typed code-graph query: relation = callers | callees | impact |
    /// implementers. Default: disabled.
    async fn knowledge_graph(
        &self,
        _relation: &str,
        _symbol: &str,
        _limit: u32,
    ) -> serde_json::Value {
        serde_json::json!({ "hits": [] })
    }
    /// Retrospection status for this node: enabled/hours + watermark + run-log +
    /// recent consolidated learnings. Default: disabled.
    async fn retrospect_status(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false })
    }
    /// Compact retrospection summary for grid fan-out (last run + run count).
    fn retrospect_summary(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false })
    }
    /// Trigger a retrospection pass now — differential, or a full `rebuild`
    /// (reset the watermark + replay all history). Default: disabled.
    async fn retrospect_run(&self, _rebuild: bool) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "memory disabled" })
    }
    /// Start a background ingest of `path`. Default: disabled.
    async fn knowledge_ingest(&self, _path: &str) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "knowledge disabled" })
    }
    /// Remove an indexed source by label. Default: disabled.
    async fn knowledge_remove(&self, _source: &str) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "knowledge disabled" })
    }
    /// Semantic search over long-term memory → `{ hits: [{ namespace, text }] }`.
    async fn memory_search(&self, _query: &str, _limit: u32) -> serde_json::Value {
        serde_json::json!({ "hits": [] })
    }

    /// The proposed-plan history (the `/plans` browser) → `{ plans: [...] }`.
    async fn plans(&self) -> serde_json::Value {
        serde_json::json!({ "plans": [] })
    }

    /// A query-centred memory subgraph → `{ nodes: [...], edges: [...] }`
    /// (SEDM memories + similarity edges) for the mobile graph view.
    async fn memory_graph(&self, _query: &str, _limit: u32) -> serde_json::Value {
        serde_json::json!({ "nodes": [], "edges": [] })
    }

    /// White-box editor: list individual memory entries → `{ entries: [...] }`.
    async fn memory_list(
        &self,
        _namespace: Option<&str>,
        _status: Option<&str>,
        _limit: u32,
    ) -> serde_json::Value {
        serde_json::json!({ "entries": [] })
    }
    /// Pin/unpin an entry (exempt from eviction + consolidation).
    async fn memory_pin(&self, _id: i64, _pinned: bool) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "memory disabled" })
    }
    /// Delete an entry.
    async fn memory_delete(&self, _id: i64) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "memory disabled" })
    }
    /// Replace an entry's text (re-embeds + resyncs FTS).
    async fn memory_update(&self, _id: i64, _text: &str) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "memory disabled" })
    }

    /// Self-healing summary → `{ counts: {...}, recent: [...] }`: recovery,
    /// evolution, and proposal episodes for the `/heal` views. Default: empty.
    async fn heal_status(&self) -> serde_json::Value {
        serde_json::json!({ "counts": {}, "recent": [] })
    }

    /// Cost-aware routing status → `{ mode, light, heavy, judge, saved_usd, … }`
    /// for the routing dashboard. Default: routing off.
    async fn route_status(&self) -> serde_json::Value {
        serde_json::json!({ "mode": "off" })
    }

    /// Always-on discovery status → `{ enabled, autonomy, recent: [...],
    /// reports: [...] }`. Default: disabled.
    async fn always_on_status(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false, "recent": [], "reports": [] })
    }

    /// Read-only git views for the web git panel → `{ ok, text }`. Default: empty.
    async fn git_status(&self) -> serde_json::Value {
        serde_json::json!({ "ok": false, "text": "" })
    }
    async fn git_diff(&self) -> serde_json::Value {
        serde_json::json!({ "ok": false, "text": "" })
    }
    async fn git_log(&self) -> serde_json::Value {
        serde_json::json!({ "ok": false, "text": "" })
    }

    // --- Web Push (#209d) ---

    /// The VAPID public key (browser `applicationServerKey`), base64url. Empty
    /// string ⇒ web push is unavailable. Default: empty.
    async fn push_public_key(&self) -> String {
        String::new()
    }
    /// Register a browser subscription `{ endpoint, keys: { p256dh, auth } }`.
    /// Returns `{ ok, count }`. Default: unsupported.
    async fn push_subscribe(&self, _sub: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "push unavailable" })
    }
    /// Remove a subscription by `endpoint`. Returns `{ ok }`. Default: unsupported.
    async fn push_unsubscribe(&self, _endpoint: &str) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "push unavailable" })
    }

    // --- FCM (blugo phone push) ---

    /// Register a blugo device FCM token. Returns `{ ok, count }`. Default: unsupported.
    async fn fcm_register(&self, _token: &str) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "fcm unavailable" })
    }
    /// Remove a device FCM token. Returns `{ ok }`. Default: unsupported.
    async fn fcm_unregister(&self, _token: &str) -> serde_json::Value {
        serde_json::json!({ "ok": false, "error": "fcm unavailable" })
    }
    /// Push an interactive turn completion to registered blugo phones via FCM.
    /// `data` (e.g. `session_id`, `node`) routes a notification tap. Gated only on
    /// the FCM service-account file existing — no settings. Default: no-op.
    async fn notify_turn(&self, _title: &str, _body: &str, _data: serde_json::Value) {}

    // --- Self-management ---

    /// The whole settings.json as JSON, with every secret redacted (for the
    /// self-config editor). Default: empty.
    fn self_config_get(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    /// Set one dotted config `key` to `value` (validated as a BlumiConfig +
    /// atomically written). `Ok(message)` or an error. Default: unsupported.
    fn self_config_set(&self, _key: &str, _value: &str) -> anyhow::Result<String> {
        anyhow::bail!("self-config not supported")
    }
    /// Create/update a skill from name/description/instructions. Default: unsupported.
    fn skill_write(
        &self,
        _name: &str,
        _description: &str,
        _instructions: &str,
    ) -> anyhow::Result<()> {
        anyhow::bail!("skills not writable")
    }
    /// Delete a skill by name. Default: unsupported.
    fn skill_delete(&self, _name: &str) -> anyhow::Result<()> {
        anyhow::bail!("skills not writable")
    }
    /// What a restart would do here: "service" (out-of-process relaunch),
    /// "foreground" (no manager — caller should degrade to a reload), or
    /// "unsupported". Default: unsupported.
    fn restart_capability(&self) -> &'static str {
        "unsupported"
    }
    /// Schedule an out-of-process restart of the gateway service. Non-blocking;
    /// returns a JSON outcome. Default: unsupported.
    fn restart(&self) -> serde_json::Value {
        serde_json::json!({ "ok": false, "mode": "unsupported" })
    }
    /// This node's detected compute accelerator ("apple-coreml" | "cuda" | "cpu"),
    /// surfaced in `/api/status` + grid metrics so the fleet can route GPU work.
    /// Default "cpu"; the binary overrides it with real detection (`blumi-llm`).
    fn accel(&self) -> &'static str {
        "cpu"
    }
}

/// Autonomous-loop state, surfaced over `/api/loop/status`.
#[derive(Clone, Default, serde::Serialize)]
pub struct LoopStatus {
    pub running: bool,
    pub iter: u32,
    pub current: String,
}

/// Shared server state.
#[derive(Clone)]
pub struct AppState {
    session: Arc<RwLock<SessionHandle>>,
    provider: Arc<dyn SessionProvider>,
    mgmt: Arc<dyn Management>,
    pub config: Arc<WebConfig>,
    auth: Option<Arc<Auth>>,
    started: std::time::Instant,
    loop_status: Arc<RwLock<LoopStatus>>,
    /// Shared grid secret (when the grid is enabled), used to authenticate
    /// peer→peer `/api/grid/run` requests. `None` = grid disabled.
    grid_secret: Option<Arc<String>>,
    /// Bumped on every session swap so a live SSE stream re-points to the new
    /// session instead of going silent on the old (now-detached) one.
    swaps: Arc<tokio::sync::watch::Sender<u64>>,
    /// Live **dedicated** sessions (blugo dispatch threads), keyed by session id.
    /// Kept apart from `session` (the active workbench session) so a client can
    /// drive a specific session concurrently without swapping the active one.
    dispatch: Arc<RwLock<HashMap<String, SessionHandle>>>,
}

impl AppState {
    /// A clone of the current session handle.
    pub(crate) async fn current(&self) -> SessionHandle {
        self.session.read().await.clone()
    }

    /// Resolve a session by id for concurrent (dispatch) use. `None`/empty or the
    /// active id ⇒ the active session. Otherwise a dedicated session from the
    /// dispatch registry, opened on demand (resume if it exists on disk, else
    /// create a fresh session pinned to `id`) and given a turn-complete watcher.
    /// Never changes the active session.
    pub(crate) async fn resolve_or_open(&self, id: Option<&str>) -> anyhow::Result<SessionHandle> {
        let id = match id {
            Some(i) if !i.is_empty() => i,
            _ => return Ok(self.current().await),
        };
        if self.current().await.id().as_str() == id {
            return Ok(self.current().await);
        }
        if let Some(h) = self.dispatch.read().await.get(id).cloned() {
            return Ok(h);
        }
        let handle = match self.provider.resume(id).await {
            Ok(h) => h,
            Err(_) => self.provider.create_with_id(id).await?,
        };
        self.spawn_dispatch_watcher(handle.clone());
        self.dispatch
            .write()
            .await
            .insert(id.to_string(), handle.clone());
        Ok(handle)
    }

    /// Watch a dedicated dispatch session: on each turn completion, FCM the phone
    /// with a reply preview (tagged `kind: "dispatch"`) and persist the thread.
    fn spawn_dispatch_watcher(&self, handle: SessionHandle) {
        let st = self.clone();
        tokio::spawn(async move {
            let mut rx = handle.subscribe();
            loop {
                match rx.recv().await {
                    Ok(env) => {
                        if matches!(env.event, blumi_protocol::Event::TurnDone { .. }) {
                            let snap = handle.snapshot().await;
                            let preview = snap
                                .messages
                                .iter()
                                .rev()
                                .find(|m| {
                                    matches!(m.role, blumi_protocol::Role::Assistant)
                                        && !m.text().trim().is_empty()
                                })
                                .map(|m| m.text())
                                .unwrap_or_default();
                            let host = whoami::fallible::hostname()
                                .unwrap_or_else(|_| "blumi".to_string());
                            let data = serde_json::json!({
                                "session_id": handle.id().as_str(),
                                "node": host,
                                "kind": "dispatch",
                            });
                            st.mgmt().notify_turn(&host, &preview, data).await;
                            st.provider.save(&handle).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    /// Persist the current session, then make `next` current.
    pub(crate) async fn swap(&self, next: SessionHandle) {
        let old = self.session.read().await.clone();
        self.provider.save(&old).await;
        *self.session.write().await = next;
        // Wake live SSE streams so they re-subscribe to the new session.
        self.swaps.send_modify(|g| *g += 1);
    }

    /// A receiver that fires whenever the current session is swapped.
    pub(crate) fn session_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.swaps.subscribe()
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

    pub(crate) fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub(crate) fn loop_status(&self) -> &Arc<RwLock<LoopStatus>> {
        &self.loop_status
    }

    /// The shared grid secret, when the grid is enabled.
    pub(crate) fn grid_secret(&self) -> Option<&str> {
        self.grid_secret.as_deref().map(|s| s.as_str())
    }
}

/// Build the axum router for a given state.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(api::health))
        .route("/api/config", get(api::config))
        .route("/api/models", get(api::models))
        .route("/api/model/set", post(api::set_model))
        .route("/api/provider/set", post(api::provider_set))
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
        .route("/api/plan/mode", post(api::set_plan_mode))
        .route("/api/brain/mode", post(api::set_brain_mode))
        .route("/api/autocontinue", post(api::set_autocontinue))
        .route("/api/chat/stream", get(api::chat_stream))
        .route("/api/approval/respond", post(api::approval_respond))
        .route("/api/clarify/respond", post(api::clarify_respond))
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/cron", get(api::cron_list).post(api::cron_add))
        .route("/api/cron/remove", post(api::cron_remove))
        .route("/api/skills", get(api::skills).post(api::skills_write))
        .route("/api/skills/delete", post(api::skills_delete))
        .route("/api/tasks", get(api::tasks))
        .route("/api/grid/peers", get(api::grid_peers))
        .route("/api/grid/dispatch", post(api::grid_dispatch))
        .route("/api/grid/run", post(api::grid_run))
        .route("/api/grid/node", get(api::grid_node))
        .route("/api/grid/metrics", get(api::grid_metrics))
        .route("/api/grid/delegate", post(api::grid_delegate))
        .route("/api/grid/memory", post(api::grid_memory))
        .route("/api/grid/embed", post(api::grid_embed))
        .route("/api/knowledge/status", get(api::knowledge_status))
        .route("/api/knowledge/sources", get(api::knowledge_sources))
        .route("/api/knowledge/search", post(api::knowledge_search))
        .route("/api/knowledge/graph", post(api::knowledge_graph))
        .route("/api/retrospect", get(api::retrospect_get))
        .route("/api/retrospect/run", post(api::retrospect_run))
        .route("/api/knowledge/ingest", post(api::knowledge_ingest))
        .route("/api/knowledge/remove", post(api::knowledge_remove))
        .route("/api/memory/search", post(api::memory_search))
        .route("/api/plans", get(api::plans))
        .route("/api/heal", get(api::heal_status))
        .route("/api/route", get(api::route_status))
        .route("/api/always-on", get(api::always_on_status))
        .route("/api/git/status", get(api::git_status))
        .route("/api/git/diff", get(api::git_diff))
        .route("/api/git/log", get(api::git_log))
        .route("/api/memory/graph", post(api::memory_graph))
        .route("/api/memory/list", post(api::memory_list))
        .route("/api/memory/pin", post(api::memory_pin))
        .route("/api/memory/delete", post(api::memory_delete))
        .route("/api/memory/update", post(api::memory_update))
        .route("/api/push/key", get(api::push_key))
        .route("/api/push/subscribe", post(api::push_subscribe))
        .route("/api/push/unsubscribe", post(api::push_unsubscribe))
        .route("/api/push/fcm/register", post(api::fcm_register))
        .route("/api/push/fcm/unregister", post(api::fcm_unregister))
        .route(
            "/api/self/config",
            get(api::self_config_get).post(api::self_config_set),
        )
        .route("/api/self/reload", post(api::session_reload))
        .route("/api/self/restart", post(api::self_restart))
        .route("/api/self/recover", post(api::self_recover))
        .route("/api/memory", get(api::memory_get).post(api::memory_set))
        .route("/api/usage", get(api::usage))
        .route("/api/status", get(api::status))
        .route("/api/loop/start", post(api::loop_start))
        .route("/api/loop/stop", post(api::loop_stop))
        .route("/api/loop/status", get(api::loop_status))
        .route(
            "/api/settings",
            get(api::settings_get).post(api::settings_set),
        )
        .route("/api/voice/transcribe", post(api::voice_transcribe))
        .route("/api/voice/speak", post(api::voice_speak))
        .fallback(assets::static_handler)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .with_state(state)
}

/// Serve the web UI + API on `addr`, sourcing sessions from `provider`. When
/// `auth` is `Some`, every data route requires a session cookie.
/// Grid-info provider for the agent's `grid_status` tool: serializes the live
/// grid metrics ({ self, peers, totals }) from `AppState`.
struct GridInfoState {
    state: AppState,
}

#[async_trait::async_trait]
impl blumi_core::GridInfo for GridInfoState {
    async fn snapshot(&self) -> String {
        let v = api::grid_metrics_value(&self.state).await;
        serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".to_string())
    }
}

pub async fn serve(
    provider: Arc<dyn SessionProvider>,
    mgmt: Arc<dyn Management>,
    config: WebConfig,
    addr: SocketAddr,
    auth: Option<Auth>,
    grid_secret: Option<String>,
) -> anyhow::Result<()> {
    let session = provider.create().await?;
    let state = AppState {
        session: Arc::new(RwLock::new(session)),
        provider,
        mgmt,
        config: Arc::new(config),
        auth: auth.map(Arc::new),
        started: std::time::Instant::now(),
        loop_status: Arc::new(RwLock::new(LoopStatus::default())),
        grid_secret: grid_secret.map(Arc::new),
        swaps: Arc::new(tokio::sync::watch::channel(0u64).0),
        dispatch: Arc::new(RwLock::new(HashMap::new())),
    };
    // Self-management: react to the agent's Reload/Restart events server-side, so
    // `reload_self` / `restart_gateway` work for every client (incl. the phone)
    // without client changes. (The TUI handles these in its own loop.)
    {
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let mut rx = st.current().await.subscribe();
                loop {
                    match rx.recv().await {
                        Ok(env) => match env.event {
                            blumi_protocol::Event::Reload { .. } => {
                                let _ = st.reload_current().await;
                                break; // re-subscribe to the swapped-in session
                            }
                            blumi_protocol::Event::Restart { .. } => {
                                if st.mgmt().restart_capability() == "service" {
                                    let _ = st.mgmt().restart(); // manager relaunches us
                                } else {
                                    let _ = st.reload_current().await;
                                    break;
                                }
                            }
                            // Phone push: when an interactive turn finishes, FCM the
                            // registered blugo devices with a reply preview (no-op
                            // unless the FCM service account is present).
                            blumi_protocol::Event::TurnDone { .. } => {
                                let handle = st.current().await;
                                let snap = handle.snapshot().await;
                                let preview = snap
                                    .messages
                                    .iter()
                                    .rev()
                                    .find(|m| {
                                        matches!(m.role, blumi_protocol::Role::Assistant)
                                            && !m.text().trim().is_empty()
                                    })
                                    .map(|m| m.text())
                                    .unwrap_or_default();
                                let host = whoami::fallible::hostname()
                                    .unwrap_or_else(|_| "blumi".to_string());
                                let data = serde_json::json!({
                                    "session_id": handle.id().as_str(),
                                    "node": host,
                                    "kind": "turn",
                                });
                                st.mgmt().notify_turn(&host, &preview, data).await;
                            }
                            _ => {}
                        },
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        });
    }

    // Expose live grid metrics to the agent's `grid_status` tool (chat answers).
    blumi_core::set_grid_info(Arc::new(GridInfoState {
        state: state.clone(),
    }));

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

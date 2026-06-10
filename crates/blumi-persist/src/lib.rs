//! SQLite persistence (sqlx) for sessions and messages, with FTS5 search.
//!
//! The source of truth: one transactional store holding session metadata,
//! messages (full `Message` JSON for exact resume, plus indexed text), and an
//! FTS5 virtual table for cross-session search. Saving a session snapshot is
//! idempotent (replace its messages). JSONL export lives elsewhere.

mod memory_store;
pub use memory_store::{MemoryEntry, MemoryParams, SemanticMemoryImpl};

use async_trait::async_trait;
use blumi_core::SessionSnapshot;
use blumi_protocol::{Message, Role, Todo};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Summary row for a session list.
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub model: String,
    pub updated_at: String,
    pub message_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// A loaded session with its full message history.
#[derive(Debug, Clone)]
pub struct StoredSession {
    pub meta: SessionMeta,
    pub messages: Vec<Message>,
    /// Standing objective (`/goal`), restored into the resumed session.
    pub goal: Option<String>,
}

/// One full-text search hit (deduplicated to one per session).
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub session_id: String,
    pub title: String,
    pub snippet: String,
}

/// An in-progress turn loaded for resume (durable execution).
#[derive(Debug, Clone)]
pub struct StoredCheckpoint {
    pub messages: Vec<Message>,
    pub todos: Vec<Todo>,
    pub model: String,
    pub turn_seq: u32,
    pub step: u32,
}

/// A stored proposed-plan (the `/plans` browser).
#[derive(Debug, Clone)]
pub struct StoredPlan {
    pub id: i64,
    pub title: String,
    pub content: String,
    /// `"live"` | `"approved"` | `"rejected"`.
    pub status: String,
    pub created_at: String,
}

/// The persistence store.
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Open (creating if needed) a file-backed store and run migrations.
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;
        Self::migrate(&pool).await?;
        Ok(Store { pool })
    }

    /// An in-memory store (single connection) for tests.
    pub async fn open_in_memory() -> Result<Self, StoreError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        Self::migrate(&pool).await?;
        Ok(Store { pool })
    }

    async fn migrate(pool: &SqlitePool) -> Result<(), StoreError> {
        sqlx::migrate!("./migrations").run(pool).await?;
        Ok(())
    }

    /// Shared pool, for sibling modules in this crate (e.g. semantic memory).
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Insert or replace a session and all its messages.
    pub async fn save_snapshot(&self, snap: &SessionSnapshot) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        let title = derive_title(&snap.messages);
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO sessions (id, title, model, created_at, updated_at, input_tokens, output_tokens, goal)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                title=excluded.title, model=excluded.model, updated_at=excluded.updated_at,
                input_tokens=excluded.input_tokens, output_tokens=excluded.output_tokens,
                goal=excluded.goal",
        )
        .bind(snap.id.as_str())
        .bind(&title)
        .bind(&snap.model)
        .bind(&now)
        .bind(&now)
        .bind(snap.total_input_tokens as i64)
        .bind(snap.total_output_tokens as i64)
        .bind(snap.goal.as_deref().unwrap_or(""))
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(snap.id.as_str())
            .execute(&mut *tx)
            .await?;

        for (i, m) in snap.messages.iter().enumerate() {
            let json = serde_json::to_string(m)?;
            let ts = m.timestamp.format(&Rfc3339).unwrap_or_default();
            sqlx::query(
                "INSERT INTO messages (session_id, ordinal, role, content, json, ts)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(snap.id.as_str())
            .bind(i as i64)
            .bind(role_str(m.role))
            .bind(m.text())
            .bind(json)
            .bind(ts)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Most-recently-updated sessions first.
    pub async fn list_sessions(&self, limit: i64) -> Result<Vec<SessionMeta>, StoreError> {
        let rows = sqlx::query(
            "SELECT s.id, s.title, s.model, s.updated_at, s.input_tokens, s.output_tokens,
                    COUNT(m.id) AS cnt
             FROM sessions s LEFT JOIN messages m ON m.session_id = s.id
             GROUP BY s.id ORDER BY s.updated_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(row_to_meta).collect())
    }

    pub async fn load_session(&self, id: &str) -> Result<Option<StoredSession>, StoreError> {
        let srow = sqlx::query(
            "SELECT id, title, model, updated_at, input_tokens, output_tokens, goal, 0 AS cnt
             FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(srow) = srow else {
            return Ok(None);
        };

        let mrows = sqlx::query("SELECT json FROM messages WHERE session_id = ? ORDER BY ordinal")
            .bind(id)
            .fetch_all(&self.pool)
            .await?;
        let mut messages = Vec::with_capacity(mrows.len());
        for r in &mrows {
            let json: String = r.get("json");
            if let Ok(m) = serde_json::from_str::<Message>(&json) {
                messages.push(m);
            }
        }

        let mut meta = row_to_meta(&srow);
        meta.message_count = messages.len() as i64;
        let goal: String = srow.get("goal");
        Ok(Some(StoredSession {
            meta,
            messages,
            goal: (!goal.is_empty()).then_some(goal),
        }))
    }

    /// Messages across *all* sessions whose timestamp is strictly after `since`
    /// (RFC3339), oldest-first, capped at `limit` — the differential feed for
    /// retrospection (daily replay of only what's new). Returns
    /// `(session_id, Message)` pairs in global time order; group by session id.
    pub async fn messages_since(
        &self,
        since: &str,
        limit: i64,
    ) -> Result<Vec<(String, Message)>, StoreError> {
        let rows = sqlx::query(
            "SELECT session_id, json FROM messages
             WHERE ts > ? ORDER BY ts, ordinal LIMIT ?",
        )
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let sid: String = r.get("session_id");
            let json: String = r.get("json");
            if let Ok(m) = serde_json::from_str::<Message>(&json) {
                out.push((sid, m));
            }
        }
        Ok(out)
    }

    /// Full-text search over message content; one hit per session, best first.
    pub async fn search(&self, query: &str, limit: i64) -> Result<Vec<SearchHit>, StoreError> {
        let fts_query = to_fts_query(query);
        if fts_query.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query(
            "SELECT m.session_id AS sid, s.title AS title,
                    snippet(messages_fts, 0, '[', ']', '…', 12) AS snip
             FROM messages_fts
             JOIN messages m ON m.id = messages_fts.rowid
             JOIN sessions s ON s.id = m.session_id
             WHERE messages_fts MATCH ? ORDER BY rank LIMIT ?",
        )
        .bind(&fts_query)
        .bind(limit * 4) // over-fetch, then dedup by session
        .fetch_all(&self.pool)
        .await?;

        let mut seen = HashSet::new();
        let mut hits = Vec::new();
        for r in &rows {
            let sid: String = r.get("sid");
            if seen.insert(sid.clone()) {
                hits.push(SearchHit {
                    session_id: sid,
                    title: r.get("title"),
                    snippet: r.get("snip"),
                });
                if hits.len() as i64 >= limit {
                    break;
                }
            }
        }
        Ok(hits)
    }

    pub async fn delete_session(&self, id: &str) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Durable-execution checkpoints (LangGraph-checkpointer analog) -------

    /// Save (overwrite) the in-progress checkpoint for the current turn.
    pub async fn save_checkpoint(&self, cp: &blumi_core::Checkpoint) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        let messages_json = serde_json::to_string(&cp.messages)?;
        let todos_json = serde_json::to_string(&cp.todos)?;
        sqlx::query(
            "INSERT INTO checkpoints
                (session_id, turn_seq, step, messages_json, todos_json, model, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, 'in_progress', ?)
             ON CONFLICT(session_id, turn_seq) DO UPDATE SET
                step=excluded.step, messages_json=excluded.messages_json,
                todos_json=excluded.todos_json, model=excluded.model,
                status='in_progress', created_at=excluded.created_at",
        )
        .bind(&cp.session_id)
        .bind(cp.turn_seq as i64)
        .bind(cp.step as i64)
        .bind(messages_json)
        .bind(todos_json)
        .bind(&cp.model)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The latest in-progress checkpoint for a session (for resume), if any.
    pub async fn take_incomplete(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredCheckpoint>, StoreError> {
        let Some(row) = sqlx::query(
            "SELECT turn_seq, step, messages_json, todos_json, model
             FROM checkpoints WHERE session_id = ? AND status = 'in_progress'
             ORDER BY turn_seq DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let messages: Vec<Message> = serde_json::from_str(&row.get::<String, _>("messages_json"))?;
        let todos: Vec<Todo> =
            serde_json::from_str(&row.get::<String, _>("todos_json")).unwrap_or_default();
        Ok(Some(StoredCheckpoint {
            messages,
            todos,
            model: row.get("model"),
            turn_seq: row.get::<i64, _>("turn_seq") as u32,
            step: row.get::<i64, _>("step") as u32,
        }))
    }

    /// Clear a session's checkpoint (the turn completed cleanly).
    pub async fn clear_checkpoint(&self, session_id: &str) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM checkpoints WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Proposed-plan history (the `/plans` browser) ----------------------

    /// Record a resolved plan. When `approved`, any existing `live` plan is
    /// demoted to `approved` and this one becomes the new `live`; otherwise it's
    /// stored `rejected`. Returns the new row id.
    pub async fn save_plan(
        &self,
        title: &str,
        content: &str,
        approved: bool,
    ) -> Result<i64, StoreError> {
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        let mut tx = self.pool.begin().await?;
        let status = if approved {
            sqlx::query("UPDATE plans SET status = 'approved' WHERE status = 'live'")
                .execute(&mut *tx)
                .await?;
            "live"
        } else {
            "rejected"
        };
        let res = sqlx::query(
            "INSERT INTO plans (title, content, status, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(title)
        .bind(content)
        .bind(status)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(res.last_insert_rowid())
    }

    /// The most recent `limit` plans, oldest-first (chronological).
    pub async fn list_plans(&self, limit: i64) -> Result<Vec<StoredPlan>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, title, content, status, created_at FROM
                 (SELECT id, title, content, status, created_at FROM plans ORDER BY id DESC LIMIT ?)
             ORDER BY id ASC",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| StoredPlan {
                id: r.get("id"),
                title: r.get("title"),
                content: r.get("content"),
                status: r.get("status"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    /// Self-healing summary for the `/heal` views (TUI overlay + gateway
    /// `/api/heal`): per-kind counts + recent recovery/evolution episodes
    /// (newest first). Pure SQL over the `agent` memory namespace — no embedder
    /// needed, so the TUI can show it without loading the model.
    pub async fn heal_summary(&self, limit: i64) -> serde_json::Value {
        use serde_json::json;
        let kinds = ["recovery", "evolution", "evolution_proposal", "failure"];
        let mut counts = serde_json::Map::new();
        for k in kinds {
            let c: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM memories \
                 WHERE status='active' AND namespace='agent' AND kind = ?",
            )
            .bind(k)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
            counts.insert(k.to_string(), json!(c));
        }
        let rows = sqlx::query(
            "SELECT kind, text, created_at, hits FROM memories \
             WHERE status='active' AND namespace='agent' \
                   AND kind IN ('recovery','evolution','evolution_proposal') \
             ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        let recent: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                json!({
                    "kind": r.get::<String, _>("kind"),
                    "text": r.get::<String, _>("text"),
                    "at": r.get::<String, _>("created_at"),
                    "hits": r.get::<i64, _>("hits"),
                })
            })
            .collect();
        json!({ "counts": counts, "recent": recent })
    }
}

/// Adapts a shared [`Store`] to the core [`blumi_core::CheckpointSink`] trait so
/// the agent loop can persist checkpoints without depending on this crate.
/// Best-effort: storage errors are swallowed (durability must never break a turn).
pub struct CheckpointSinkImpl(pub Arc<Store>);

#[async_trait]
impl blumi_core::CheckpointSink for CheckpointSinkImpl {
    async fn save(&self, cp: blumi_core::Checkpoint) {
        let _ = self.0.save_checkpoint(&cp).await;
    }
    async fn done(&self, session_id: &str) {
        let _ = self.0.clear_checkpoint(session_id).await;
    }
}

fn row_to_meta(r: &sqlx::sqlite::SqliteRow) -> SessionMeta {
    SessionMeta {
        id: r.get("id"),
        title: r.get("title"),
        model: r.get("model"),
        updated_at: r.get("updated_at"),
        message_count: r.get("cnt"),
        input_tokens: r.get("input_tokens"),
        output_tokens: r.get("output_tokens"),
    }
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

/// Title = first line of the first user message, capped.
fn derive_title(messages: &[Message]) -> String {
    let first = messages
        .iter()
        .find(|m| m.role == Role::User)
        .map(|m| m.text())
        .unwrap_or_default();
    let line = first.lines().next().unwrap_or("").trim();
    if line.chars().count() > 60 {
        let truncated: String = line.chars().take(60).collect();
        format!("{truncated}…")
    } else if line.is_empty() {
        "(untitled)".to_string()
    } else {
        line.to_string()
    }
}

/// Turn free text into a safe FTS5 query: each whitespace term quoted (phrase)
/// and AND-ed. Avoids FTS5 syntax errors from arbitrary input.
pub(crate) fn to_fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plan_history_status_transitions() {
        let store = Store::open_in_memory().await.unwrap();
        store.save_plan("Plan A", "step one", false).await.unwrap(); // rejected
        store.save_plan("Plan B", "do it", true).await.unwrap(); // live
        store.save_plan("Plan C", "revise", true).await.unwrap(); // live (B → approved)
        let plans = store.list_plans(10).await.unwrap();
        assert_eq!(plans.len(), 3);
        // Oldest-first.
        assert_eq!(plans[0].title, "Plan A");
        assert_eq!(plans[0].status, "rejected");
        assert_eq!(plans[1].status, "approved"); // demoted
        assert_eq!(plans[2].status, "live"); // newest approved
    }

    #[tokio::test]
    async fn checkpoint_round_trip() {
        let store = Store::open_in_memory().await.unwrap();
        let sid = "s-cp";
        assert!(store.take_incomplete(sid).await.unwrap().is_none());

        let cp = blumi_core::Checkpoint {
            session_id: sid.to_string(),
            turn_seq: 1,
            step: 0,
            messages: vec![Message::user("hi".to_string())],
            todos: vec![],
            model: "m".to_string(),
        };
        store.save_checkpoint(&cp).await.unwrap();

        // Overwrite the same turn at a later step.
        let mut cp2 = cp.clone();
        cp2.step = 1;
        cp2.messages.push(Message::assistant("yo".to_string()));
        store.save_checkpoint(&cp2).await.unwrap();

        let got = store.take_incomplete(sid).await.unwrap().unwrap();
        assert_eq!(got.step, 1);
        assert_eq!(got.messages.len(), 2);

        store.clear_checkpoint(sid).await.unwrap();
        assert!(store.take_incomplete(sid).await.unwrap().is_none());
    }
    use blumi_protocol::{Message, SessionId};

    fn snapshot(id: &str, msgs: Vec<Message>) -> SessionSnapshot {
        SessionSnapshot {
            id: SessionId::from(id),
            messages: msgs,
            todos: vec![],
            model: "m".into(),
            goal: None,
            total_input_tokens: 10,
            total_output_tokens: 5,
            turn_count: 1,
        }
    }

    #[tokio::test]
    async fn save_load_and_list() {
        let store = Store::open_in_memory().await.unwrap();
        let snap = snapshot(
            "s1",
            vec![
                Message::user("how do I parse JSON in Rust?"),
                Message::assistant("use serde_json"),
            ],
        );
        store.save_snapshot(&snap).await.unwrap();

        let loaded = store.load_session("s1").await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.meta.title, "how do I parse JSON in Rust?");
        assert_eq!(loaded.messages[1].text(), "use serde_json");

        let list = store.list_sessions(10).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].message_count, 2);
    }

    #[tokio::test]
    async fn save_is_idempotent() {
        let store = Store::open_in_memory().await.unwrap();
        let snap = snapshot("s1", vec![Message::user("hello")]);
        store.save_snapshot(&snap).await.unwrap();
        store.save_snapshot(&snap).await.unwrap();
        let loaded = store.load_session("s1").await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 1); // not duplicated
    }

    #[tokio::test]
    async fn fts_search_finds_session() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_snapshot(&snapshot(
                "s1",
                vec![Message::user("the quick brown fox jumps over the lazy dog")],
            ))
            .await
            .unwrap();
        store
            .save_snapshot(&snapshot(
                "s2",
                vec![Message::user("rust ownership and borrowing")],
            ))
            .await
            .unwrap();

        let hits = store.search("borrowing", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "s2");
        assert!(hits[0].snippet.contains("borrowing"));

        // porter stemming: "jump" matches "jumps"
        let hits = store.search("jump", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "s1");
    }

    #[tokio::test]
    async fn delete_removes_session_and_fts() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_snapshot(&snapshot("s1", vec![Message::user("findme please")]))
            .await
            .unwrap();
        store.delete_session("s1").await.unwrap();
        assert!(store.load_session("s1").await.unwrap().is_none());
        assert!(store.search("findme", 10).await.unwrap().is_empty());
    }
}

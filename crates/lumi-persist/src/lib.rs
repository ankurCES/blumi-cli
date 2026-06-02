//! SQLite persistence (sqlx) for sessions and messages, with FTS5 search.
//!
//! The source of truth: one transactional store holding session metadata,
//! messages (full `Message` JSON for exact resume, plus indexed text), and an
//! FTS5 virtual table for cross-session search. Saving a session snapshot is
//! idempotent (replace its messages). JSONL export lives elsewhere.

use lumi_core::SessionSnapshot;
use lumi_protocol::{Message, Role};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
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
}

/// One full-text search hit (deduplicated to one per session).
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub session_id: String,
    pub title: String,
    pub snippet: String,
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

    /// Insert or replace a session and all its messages.
    pub async fn save_snapshot(&self, snap: &SessionSnapshot) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        let title = derive_title(&snap.messages);
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO sessions (id, title, model, created_at, updated_at, input_tokens, output_tokens)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                title=excluded.title, model=excluded.model, updated_at=excluded.updated_at,
                input_tokens=excluded.input_tokens, output_tokens=excluded.output_tokens",
        )
        .bind(snap.id.as_str())
        .bind(&title)
        .bind(&snap.model)
        .bind(&now)
        .bind(&now)
        .bind(snap.total_input_tokens as i64)
        .bind(snap.total_output_tokens as i64)
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
            "SELECT id, title, model, updated_at, input_tokens, output_tokens, 0 AS cnt
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
        Ok(Some(StoredSession { meta, messages }))
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
fn to_fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumi_protocol::{Message, SessionId};

    fn snapshot(id: &str, msgs: Vec<Message>) -> SessionSnapshot {
        SessionSnapshot {
            id: SessionId::from(id),
            messages: msgs,
            todos: vec![],
            model: "m".into(),
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

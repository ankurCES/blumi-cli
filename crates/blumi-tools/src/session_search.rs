//! SessionSearch: full-text search across past sessions (SQLite FTS5).
//!
//! Unlike the other built-ins this one needs a [`Store`], so it isn't part of
//! [`crate::register_builtin_tools`] — the binary constructs it with the open
//! store and registers it alongside the rest (mirroring how memory/MCP tools
//! are wired).

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_persist::Store;
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize, JsonSchema)]
pub struct SessionSearchInput {
    /// Full-text query (SQLite FTS5 syntax) matched against past session messages.
    pub query: String,
    /// Maximum number of sessions to return (default 10, max 50).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Search prior blumi sessions stored in the SQLite history.
pub struct SessionSearch {
    store: Arc<Store>,
}

impl SessionSearch {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TypedTool for SessionSearch {
    type Input = SessionSearchInput;

    fn name(&self) -> &str {
        "SessionSearch"
    }
    fn description(&self) -> &str {
        "Search your past blumi sessions by message content (full-text). Use it to recall \
         earlier work, decisions, or code from previous conversations."
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn run(
        &self,
        input: Self::Input,
        _ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let limit = input.limit.unwrap_or(10).clamp(1, 50) as i64;
        let hits = self
            .store
            .search(&input.query, limit)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        if hits.is_empty() {
            return Ok(ToolResult::success(format!(
                "No past sessions match '{}'.",
                input.query
            )));
        }

        let mut preview = format!("Found {} session(s) for '{}':\n", hits.len(), input.query);
        for h in &hits {
            preview.push_str(&format!(
                "• [{}] {} — {}\n",
                h.session_id, h.title, h.snippet
            ));
        }
        let payload = serde_json::Value::Array(
            hits.iter()
                .map(|h| {
                    serde_json::json!({
                        "session_id": h.session_id,
                        "title": h.title,
                        "snippet": h.snippet,
                    })
                })
                .collect(),
        );
        Ok(ToolResult::success(preview).with_payload(payload))
    }
}

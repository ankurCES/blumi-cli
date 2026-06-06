//! code_search: hybrid (embeddings + FTS5) search over the code knowledge base.
//!
//! Like [`crate::SessionSearch`], it needs a store, so the binary constructs it
//! with an open [`KnowledgeStore`] and registers it when the code KB is enabled.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_knowledge::KnowledgeStore;
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize, JsonSchema)]
pub struct CodeSearchInput {
    /// What to find in the indexed codebase — natural language ("where is auth
    /// handled") or symbol/keywords ("PermissionEngine new").
    pub query: String,
    /// Maximum number of code hits to return (default 8, max 30).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Search the code knowledge base.
pub struct CodeSearch {
    store: Arc<KnowledgeStore>,
}

impl CodeSearch {
    pub fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TypedTool for CodeSearch {
    type Input = CodeSearchInput;

    fn name(&self) -> &str {
        "code_search"
    }
    fn description(&self) -> &str {
        "Search the indexed code knowledge base for relevant functions, types, and code by \
         meaning or keywords (hybrid embeddings + full-text). Returns file:line + a snippet per \
         hit — use it to locate where something is implemented before reading or editing. If it \
         returns nothing, the repo may need indexing first: `blumi knowledge ingest <path>`."
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
        let limit = input.limit.unwrap_or(8).clamp(1, 30) as usize;
        let hits = self.store.search(&input.query, limit).await;
        if hits.is_empty() {
            return Ok(ToolResult::success(format!(
                "No code matches '{}'. If you haven't indexed this repo yet, run \
                 `blumi knowledge ingest <path>`.",
                input.query
            )));
        }
        let mut preview = format!("{} code hit(s) for '{}':\n", hits.len(), input.query);
        for h in &hits {
            preview.push_str(&format!(
                "\n• {}:{} [{}] {}\n",
                h.path, h.start_line, h.kind, h.name
            ));
            for line in h.snippet.lines().take(8) {
                preview.push_str("    ");
                preview.push_str(line);
                preview.push('\n');
            }
        }
        let payload = serde_json::to_value(&hits).unwrap_or_default();
        Ok(ToolResult::success(preview).with_payload(payload))
    }
}

//! Graph-memory tools over the code knowledge base: `code_neighbors` (what
//! connects to a symbol) and `code_path` (shortest reference path between two).
//! These answer structural questions with a tiny subgraph instead of re-reading
//! whole files — faster retrieval, far fewer tokens.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_knowledge::KnowledgeStore;
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize, JsonSchema)]
pub struct CodeNeighborsInput {
    /// The symbol name (function / type / class …) whose graph neighbors to list.
    pub symbol: String,
    /// Max neighbors to return (default 20, max 60).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// List a symbol's reference-graph neighbors.
pub struct CodeNeighbors {
    store: Arc<KnowledgeStore>,
}

impl CodeNeighbors {
    pub fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TypedTool for CodeNeighbors {
    type Input = CodeNeighborsInput;

    fn name(&self) -> &str {
        "code_neighbors"
    }
    fn description(&self) -> &str {
        "Graph memory: list the code symbols directly connected to a symbol — what \
         references it and what it references — as file:line entries. Much cheaper than \
         reading files; use it to learn what depends on / relates to something before \
         editing. Needs an ingested repo (`blumi knowledge ingest <path>`)."
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
        let limit = input.limit.unwrap_or(20).clamp(1, 60) as usize;
        let hits = self.store.neighbors(&input.symbol, limit).await;
        if hits.is_empty() {
            return Ok(ToolResult::success(format!(
                "No graph neighbors for '{}'. Index the repo first with \
                 `blumi knowledge ingest <path>`, or check the symbol name with code_search.",
                input.symbol
            )));
        }
        let mut out = format!("{} neighbor(s) of '{}':\n", hits.len(), input.symbol);
        for h in &hits {
            out.push_str(&format!(
                "• {}:{} [{}] {}\n",
                h.path, h.start_line, h.kind, h.name
            ));
        }
        let payload = serde_json::to_value(&hits).unwrap_or_default();
        Ok(ToolResult::success(out).with_payload(payload))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct CodePathInput {
    /// Start symbol name.
    pub from: String,
    /// Target symbol name.
    pub to: String,
}

/// Shortest reference path between two symbols.
pub struct CodePath {
    store: Arc<KnowledgeStore>,
}

impl CodePath {
    pub fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TypedTool for CodePath {
    type Input = CodePathInput;

    fn name(&self) -> &str {
        "code_path"
    }
    fn description(&self) -> &str {
        "Graph memory: the shortest reference path between two code symbols \
         (e.g. how `auth` connects to the database layer), as a chain of symbol \
         names. Use it to understand how parts of the codebase relate without \
         reading the files in between."
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
        let path = self.store.shortest_path(&input.from, &input.to, 8).await;
        if path.is_empty() {
            return Ok(ToolResult::success(format!(
                "No reference path found from '{}' to '{}' (within 8 hops).",
                input.from, input.to
            )));
        }
        let hops = path.len().saturating_sub(1);
        Ok(ToolResult::success(format!(
            "Path ({hops} hop(s)): {}",
            path.join(" → ")
        )))
    }
}

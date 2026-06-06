//! code_retrieve: fetch indexed symbols (with snippets) for a file path.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_knowledge::KnowledgeStore;
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize, JsonSchema)]
pub struct CodeRetrieveInput {
    /// File path (or a substring of it) whose indexed symbols to retrieve.
    pub path: String,
    /// Optionally retrieve just one symbol by name.
    #[serde(default)]
    pub symbol: Option<String>,
}

/// Retrieve indexed code from the knowledge base by path/symbol.
pub struct CodeRetrieve {
    store: Arc<KnowledgeStore>,
}

impl CodeRetrieve {
    pub fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TypedTool for CodeRetrieve {
    type Input = CodeRetrieveInput;

    fn name(&self) -> &str {
        "code_retrieve"
    }
    fn description(&self) -> &str {
        "Retrieve indexed code symbols (with their source snippets) for a file path from the code \
         knowledge base — optionally a single symbol by name. Pair it with code_search: search to \
         find where something lives, retrieve to read it. For the full current file, use FileRead."
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
        let hits = self
            .store
            .retrieve(&input.path, input.symbol.as_deref())
            .await;
        if hits.is_empty() {
            return Ok(ToolResult::success(format!(
                "No indexed symbols for '{}'{}. Try code_search, or ingest the repo with \
                 `blumi knowledge ingest <path>`.",
                input.path,
                input
                    .symbol
                    .as_deref()
                    .map(|s| format!(" (symbol '{s}')"))
                    .unwrap_or_default()
            )));
        }
        let mut out = format!("{} symbol(s) from '{}':\n", hits.len(), input.path);
        for h in &hits {
            out.push_str(&format!(
                "\n• {}:{}-{} [{}] {}\n",
                h.path, h.start_line, h.end_line, h.kind, h.name
            ));
            out.push_str(&h.snippet);
            out.push('\n');
        }
        let payload = serde_json::to_value(&hits).unwrap_or_default();
        Ok(ToolResult::success(out).with_payload(payload))
    }
}

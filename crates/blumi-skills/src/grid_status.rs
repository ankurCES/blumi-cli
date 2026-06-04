//! The `grid_status` tool: a live snapshot of the distributed grid, so the agent
//! can answer questions in chat about connected peers, their health, task
//! metrics (local vs. handed-off), token usage, and loop state.
//!
//! The data comes from a process-global [`blumi_core::GridInfo`] provider the
//! gateway registers at startup (it owns the peer registry + state). When the
//! grid isn't live (e.g. a one-shot run, or grid disabled), the tool says so.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GridStatusInput {}

/// Reports the live grid snapshot.
#[derive(Default)]
pub struct GridStatusTool;

impl GridStatusTool {
    pub fn new() -> Self {
        GridStatusTool
    }
}

#[async_trait]
impl TypedTool for GridStatusTool {
    type Input = GridStatusInput;

    fn name(&self) -> &str {
        "grid_status"
    }

    fn description(&self) -> &str {
        "Get a live snapshot of the blumi grid: connected/available peers and their health \
         (online/offline), task metrics (local vs. handed-off to remote peers), token usage per \
         node, loop state, and grid-wide totals — returned as JSON. Use it to answer questions \
         about peers, available capacity, job/loop status, and token usage across the fleet."
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn run(
        &self,
        _input: GridStatusInput,
        _ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        match blumi_core::grid_info() {
            Some(p) => Ok(ToolResult::success(p.snapshot().await)),
            None => Ok(ToolResult::success(
                "Grid info isn't available here. The grid is only live inside a running \
                 `blumi serve` gateway with grid.enabled = true in settings.json."
                    .to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata() {
        let t = GridStatusTool::new();
        assert_eq!(t.name(), "grid_status");
        assert!(t.is_concurrency_safe());
    }
}

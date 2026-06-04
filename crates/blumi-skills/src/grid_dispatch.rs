//! The `grid_dispatch` tool: run a self-contained job on another machine in the
//! blumi grid and bring the result back.
//!
//! This is the agent-facing way to do *true* distributed work from a single
//! prompt: the model calls `grid_dispatch` once per independent job, each call
//! executes on a grid peer (a named one, or the next peer round-robin) on its own
//! runtime, and returns the output tagged with which machine ran it. The model
//! then collates the results. Unlike sub-agent delegation (which only spills to a
//! peer when the local concurrency cap is exceeded), every `grid_dispatch` call
//! goes to a peer — so work reliably spreads across the fleet.
//!
//! Backed by a process-global [`blumi_core::GridDispatch`] hook the gateway
//! registers at startup (it owns the peer registry + grid secret). When the grid
//! isn't live (one-shot run, grid disabled, or no peers online), the tool says so
//! and the model can fall back to running the job locally.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GridDispatchInput {
    /// The complete, self-contained job to run on a remote grid machine. It must
    /// not depend on local files or prior context — the peer runs it in its own
    /// workspace/session. Tell it to report whatever result you need back.
    pub prompt: String,
    /// Optional: target a specific peer by name or host substring (e.g.
    /// "ubuntu", "mac-air"). Omit to let the grid pick the next peer round-robin
    /// (the right choice when fanning many jobs across the fleet).
    #[serde(default)]
    pub peer: Option<String>,
}

/// Dispatches a job to a grid peer for remote execution.
#[derive(Default)]
pub struct GridDispatchTool;

impl GridDispatchTool {
    pub fn new() -> Self {
        GridDispatchTool
    }
}

#[async_trait]
impl TypedTool for GridDispatchTool {
    type Input = GridDispatchInput;

    fn name(&self) -> &str {
        "grid_dispatch"
    }

    fn description(&self) -> &str {
        "Run a self-contained job on ANOTHER machine in the blumi grid and return its result \
         (tagged with which machine ran it). Call this once per independent job to distribute \
         work across the fleet — each call runs on a peer's own runtime (round-robin by default, \
         or pass `peer` to target one), so a single request can fan out across all machines and \
         you then collate the results. The job prompt must be self-contained (the peer runs it in \
         its own workspace). Returns an error message if no grid peer is available — fall back to \
         running the job locally in that case."
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn run(
        &self,
        input: GridDispatchInput,
        _ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let Some(hook) = blumi_core::grid_dispatch() else {
            return Ok(ToolResult::success(
                "grid_dispatch isn't available here. It's only live inside a running \
                 `blumi serve` gateway with grid.enabled = true and at least one peer online. \
                 Run this job locally instead."
                    .to_string(),
            ));
        };
        match hook.dispatch(input.peer.as_deref(), &input.prompt).await {
            Ok((peer, output)) => Ok(ToolResult::success(format!(
                "[executed remotely on grid peer: {peer}]\n{output}"
            ))),
            Err(e) => Ok(ToolResult::success(format!(
                "grid_dispatch could not run remotely: {e}. Run this job locally instead."
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata() {
        let t = GridDispatchTool::new();
        assert_eq!(t.name(), "grid_dispatch");
        assert!(t.is_concurrency_safe());
    }
}

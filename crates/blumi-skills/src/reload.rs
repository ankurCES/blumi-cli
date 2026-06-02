//! The `reload_self` tool: rebuild the agent in place so self-written skills and
//! config edits take effect (self-evolution), keeping the current conversation.
//!
//! It emits [`Event::Reload`]; the host (TUI/web) rebuilds the session — which
//! re-reads `settings.json` and re-discovers skills — and seeds it with the
//! existing transcript so context is preserved. `break_turn` ends the current
//! turn so the swap happens at a clean boundary. Hosts that can't reload (e.g.
//! one-shot `run`) simply ignore the event.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::{Event, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ReloadInput {
    /// Optional note on what changed (shown to the user), e.g. "added the
    /// pdf-wrangler skill".
    #[serde(default)]
    pub reason: String,
}

/// Requests an in-place reload of the agent.
#[derive(Default)]
pub struct ReloadTool;

impl ReloadTool {
    pub fn new() -> Self {
        ReloadTool
    }
}

#[async_trait]
impl TypedTool for ReloadTool {
    type Input = ReloadInput;

    fn name(&self) -> &str {
        "reload_self"
    }

    fn description(&self) -> &str {
        "Reload yourself to apply newly written skills (manage_skill) and config edits \
         (self_config). This rebuilds the agent — re-reading settings.json and re-scanning skills — \
         while keeping the current conversation. Call it after you change a skill or your config so \
         the change becomes active. Some process-level settings may still need a full relaunch."
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn run(
        &self,
        input: ReloadInput,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let reason = if input.reason.trim().is_empty() {
            "self-evolution".to_string()
        } else {
            input.reason.trim().to_string()
        };
        ctx.events.emit(Event::Reload {
            reason: reason.clone(),
        });
        Ok(
            ToolResult::success(format!("reloading the agent to apply changes ({reason})…"))
                .with_break_turn(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata() {
        let t = ReloadTool::new();
        assert_eq!(t.name(), "reload_self");
        assert!(!t.is_concurrency_safe());
    }
}

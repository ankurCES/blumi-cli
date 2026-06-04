//! The `restart_gateway` tool: restart the whole gateway process (a full
//! relaunch), not just a session rebuild.
//!
//! It emits [`Event::Restart`]; the host (the `blumi serve` gateway) performs an
//! out-of-process restart via its service manager (launchd/systemd). Use it only
//! when `reload_self` isn't enough — e.g. a process-level setting changed (bind
//! host/port, auth key) or to recover from a wedged state. In-flight turns are
//! interrupted. Hosts that aren't service-managed (foreground/one-shot) ignore
//! it or downgrade to a reload.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::{Event, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct RestartInput {
    /// Optional note on why you're restarting (shown to the user).
    #[serde(default)]
    pub reason: String,
}

/// Requests a full restart of the gateway process.
#[derive(Default)]
pub struct RestartGatewayTool;

impl RestartGatewayTool {
    pub fn new() -> Self {
        RestartGatewayTool
    }
}

#[async_trait]
impl TypedTool for RestartGatewayTool {
    type Input = RestartInput;

    fn name(&self) -> &str {
        "restart_gateway"
    }

    fn description(&self) -> &str {
        "Restart the whole gateway service (a full process relaunch). Use only when reload_self \
         isn't enough — e.g. a process-level setting changed (bind host/port, the auth key) or to \
         recover from a wedged state. The service manager brings blumi back up automatically. \
         In-flight turns are interrupted, so prefer reload_self for skill/config changes."
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn run(
        &self,
        input: RestartInput,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let reason = if input.reason.trim().is_empty() {
            "self-restart".to_string()
        } else {
            input.reason.trim().to_string()
        };
        ctx.events.emit(Event::Restart {
            reason: reason.clone(),
        });
        Ok(ToolResult::success(format!("restarting the gateway ({reason})…")).with_break_turn())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata() {
        let t = RestartGatewayTool::new();
        assert_eq!(t.name(), "restart_gateway");
        assert!(!t.is_concurrency_safe());
    }
}

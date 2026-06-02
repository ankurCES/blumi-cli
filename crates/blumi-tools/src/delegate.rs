//! The `delegate` tool: hand a sub-task to a specialized sub-agent.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::{Capability, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DelegateInput {
    /// Which sub-agent to run: "Explore" or "Plan" (read-only), "Coder"
    /// (edits + shell), "Verify" (runs checks), or "general-purpose".
    pub agent_type: String,
    /// The self-contained task for the sub-agent. Include all needed context;
    /// the sub-agent starts a fresh conversation.
    pub prompt: String,
}

pub struct Delegate;

#[async_trait]
impl TypedTool for Delegate {
    type Input = DelegateInput;

    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "Delegate a self-contained sub-task to a specialized sub-agent that runs with its own \
         restricted toolset and budget, then returns its final result. Agents: Explore and Plan \
         (read-only investigation/planning), Coder (makes edits), Verify (runs checks), \
         general-purpose (full toolset). Use for parallelizable or well-scoped subtasks."
    }

    fn required_capabilities(&self, input: &DelegateInput) -> Vec<Capability> {
        vec![Capability::AgentSpawn {
            agent: input.agent_type.clone(),
        }]
    }

    async fn run(
        &self,
        input: DelegateInput,
        ctx: &ToolContext,
        ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let spawner = ctx
            .spawner
            .as_ref()
            .ok_or_else(|| ToolError::Execution("sub-agent delegation is not available".into()))?;

        let output = spawner
            .spawn(
                &input.agent_type,
                &input.prompt,
                ctx.events.clone(),
                ctx.interactor.clone(),
                ct,
            )
            .await?;

        Ok(ToolResult::success(output))
    }
}

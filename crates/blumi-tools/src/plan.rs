//! ExitPlanMode: present a plan and wait for the user's approval before the
//! agent leaves read-only planning mode and starts changing things.
//!
//! The permission engine special-cases this tool (never gated, allowed even in
//! plan mode) because it runs its *own* approval — the plan modal — via the
//! interactor. On approval the actor exits plan mode, so the agent's subsequent
//! mutating tools are allowed.

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize, JsonSchema)]
pub struct ExitPlanModeInput {
    /// The proposed implementation plan, as markdown (numbered steps preferred).
    pub plan: String,
}

pub struct ExitPlanMode;

#[async_trait]
impl TypedTool for ExitPlanMode {
    type Input = ExitPlanModeInput;

    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "Present your implementation plan to the user for approval and leave \
         planning mode. Use this when you are in plan mode and have finished \
         researching (read-only). Pass the complete plan as markdown in `plan`. \
         If the user approves, planning mode ends and you may make changes; if \
         they reject, stay read-only, revise, and call ExitPlanMode again."
    }

    // Not read-only (it transitions out of plan mode), but the permission engine
    // never prompts for it separately — the plan modal IS the approval. Run it
    // serially (it blocks on a human), so it is not concurrency-safe.
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let plan = input.plan.trim();
        if plan.is_empty() {
            return Ok(ToolResult::invalid_input(
                "the plan was empty",
                "put the full plan as markdown in the `plan` field",
            ));
        }
        if ctx.interactor.review_plan(plan.to_string()).await {
            Ok(ToolResult::success(
                "Plan approved by the user. Planning mode is now off — proceed with the implementation.",
            ))
        } else {
            Ok(ToolResult::success(
                "Plan rejected by the user. Stay in planning mode (read-only): revise the plan or ask what to change, then call ExitPlanMode again.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blumi_core::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn empty_plan_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::testutil::ctx(dir.path());
        let t = blumi_core::Typed(ExitPlanMode);
        let res = Tool::execute(&t, json!({ "plan": "  " }), &c, CancellationToken::new())
            .await
            .unwrap();
        assert!(res.is_error());
    }
}

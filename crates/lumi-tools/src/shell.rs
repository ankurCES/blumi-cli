//! The Bash tool: run a shell command through the executor.

use async_trait::async_trait;
use lumi_core::{ExecRequest, ToolContext, ToolError, TypedTool};
use lumi_protocol::{Capability, SideEffect, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize, JsonSchema)]
pub struct BashInput {
    /// The shell command to run.
    pub command: String,
    /// Optional timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

pub struct Bash;

#[async_trait]
impl TypedTool for Bash {
    type Input = BashInput;

    fn name(&self) -> &str {
        "Bash"
    }
    fn description(&self) -> &str {
        "Run a shell command in the working directory and return its combined output."
    }
    fn required_capabilities(&self, input: &Self::Input) -> Vec<Capability> {
        vec![Capability::process_exec(&input.command)]
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let mut req = ExecRequest::new(input.command.clone());
        if let Some(secs) = input.timeout_secs {
            req = req.timeout(Duration::from_secs(secs));
        }
        let out = ctx
            .executor
            .exec(req, ct)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let mut preview = String::new();
        if !out.stdout.is_empty() {
            preview.push_str(out.stdout.trim_end());
        }
        if !out.stderr.is_empty() {
            if !preview.is_empty() {
                preview.push('\n');
            }
            preview.push_str("[stderr] ");
            preview.push_str(out.stderr.trim_end());
        }
        if out.timed_out {
            preview.push_str("\n[command timed out]");
        }
        if out.exit_code != 0 {
            preview.push_str(&format!("\n[exit code: {}]", out.exit_code));
        }
        if preview.is_empty() {
            preview.push_str("(no output)");
        }

        Ok(ToolResult::success(preview)
            .with_side_effects(vec![SideEffect::process_spawn(input.command, None)]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::ctx;
    use serde_json::json;

    #[tokio::test]
    async fn runs_and_reports_output() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let b = lumi_core::Typed(Bash);
        let res = lumi_core::Tool::execute(
            &b,
            json!({ "command": "echo hi" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(res.model_preview.contains("hi"));
    }

    #[tokio::test]
    async fn reports_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let b = lumi_core::Typed(Bash);
        let res = lumi_core::Tool::execute(
            &b,
            json!({ "command": "exit 2" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(res.model_preview.contains("exit code: 2"));
    }
}

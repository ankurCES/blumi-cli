//! ListDirectory: enumerate a directory through the executor.

use crate::path::resolve;
use async_trait::async_trait;
use lumi_core::{ToolContext, ToolError, TypedTool};
use lumi_protocol::{Capability, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize, JsonSchema)]
pub struct ListDirInput {
    /// Directory to list (default: the working directory).
    #[serde(default)]
    pub path: Option<String>,
}

pub struct ListDirectory;

#[async_trait]
impl TypedTool for ListDirectory {
    type Input = ListDirInput;

    fn name(&self) -> &str {
        "ListDirectory"
    }
    fn description(&self) -> &str {
        "List the entries of a directory (directories first, then files)."
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn required_capabilities(&self, input: &Self::Input) -> Vec<Capability> {
        vec![Capability::file_read(input.path.clone().unwrap_or_else(|| ".".into()))]
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let rel = input.path.unwrap_or_else(|| ".".into());
        let path = resolve(&ctx.working_dir, &rel);
        let entries = ctx
            .executor
            .list_dir(&path)
            .await
            .map_err(|e| ToolError::Execution(format!("could not list {}: {e}", path.display())))?;

        if entries.is_empty() {
            return Ok(ToolResult::empty(format!("{rel} is empty")));
        }
        let mut out = String::new();
        for e in &entries {
            if e.is_dir {
                out.push_str(&format!("{}/\n", e.name));
            } else {
                out.push_str(&format!("{} ({} bytes)\n", e.name, e.size));
            }
        }
        Ok(ToolResult::success(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::ctx;
    use serde_json::json;

    #[tokio::test]
    async fn lists_entries_dirs_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("z.txt"), "x").unwrap();
        let c = ctx(dir.path());
        let t = lumi_core::Typed(ListDirectory);
        let res = lumi_core::Tool::execute(&t, json!({}), &c, CancellationToken::new())
            .await
            .unwrap();
        let p = res.model_preview;
        let sub = p.find("sub/").unwrap();
        let z = p.find("z.txt").unwrap();
        assert!(sub < z, "directories should be listed first");
    }
}

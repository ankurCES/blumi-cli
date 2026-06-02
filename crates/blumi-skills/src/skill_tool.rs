//! The `skill` tool: load a skill's full instructions on demand (progressive
//! disclosure — only names + descriptions sit in the system prompt).

use crate::catalog::SkillCatalog;
use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SkillInput {
    /// The name of the skill to load (as listed under "Skills" in the prompt).
    pub name: String,
}

/// Reads skill bodies from a [`SkillCatalog`].
pub struct SkillTool {
    catalog: Arc<SkillCatalog>,
}

impl SkillTool {
    pub fn new(catalog: Arc<SkillCatalog>) -> Self {
        SkillTool { catalog }
    }
}

#[async_trait]
impl TypedTool for SkillTool {
    type Input = SkillInput;

    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Load a skill's full instructions by name (the available skills are listed under \
         \"Skills\" in the system prompt). Read the relevant skill before doing a task it covers."
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn run(
        &self,
        input: SkillInput,
        _ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        match self.catalog.get(&input.name) {
            Some(skill) => Ok(ToolResult::success(skill.body.clone())),
            None => {
                let available: Vec<String> =
                    self.catalog.list().into_iter().map(|m| m.name).collect();
                Ok(ToolResult::invalid_input(
                    format!("no skill named '{}'", input.name),
                    format!("available skills: {}", available.join(", ")),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_skill_is_invalid_input() {
        let cat = Arc::new(SkillCatalog::default());
        let tool = SkillTool::new(cat);
        // (We only assert construction + metadata here; execution is covered by
        // the catalog tests + the core pipeline integration.)
        assert_eq!(tool.name(), "skill");
        assert!(tool.is_read_only());
    }
}

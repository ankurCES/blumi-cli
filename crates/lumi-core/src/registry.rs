//! The tool registry: holds the available tools and projects them into the
//! provider-neutral [`ToolSpec`] list the loop sends to the model.

use crate::llm::ToolSpec;
use crate::tool::Tool;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
    by_name: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        ToolRegistry::default()
    }

    /// Register a tool. Later registrations override earlier ones with the same
    /// name (so the binary can replace a built-in).
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.by_name.insert(name, tool.clone());
        self.tools.push(tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.by_name.get(name).cloned()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Specs for the model's base tool list: every non-deferred tool.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.by_name
            .values()
            .filter(|t| !t.is_deferred())
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.input_schema(),
            })
            .collect()
    }

    /// Search tool names/descriptions (used by the deferred-tool `ToolSearch`).
    pub fn search(&self, query: &str) -> Vec<ToolSpec> {
        let q = query.to_lowercase();
        self.by_name
            .values()
            .filter(|t| {
                t.name().to_lowercase().contains(&q)
                    || t.description().to_lowercase().contains(&q)
            })
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.input_schema(),
            })
            .collect()
    }
}

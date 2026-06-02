//! TodoWrite: maintain the agent's task checklist (surfaced to UIs as events).

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::{Event, Todo, TodoStatus, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize, JsonSchema)]
pub struct TodoWriteInput {
    pub todos: Vec<TodoItemInput>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TodoItemInput {
    pub content: String,
    /// One of `pending`, `in_progress`, `completed`.
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_status() -> String {
    "pending".into()
}

pub struct TodoWrite;

#[async_trait]
impl TypedTool for TodoWrite {
    type Input = TodoWriteInput;

    fn name(&self) -> &str {
        "TodoWrite"
    }
    fn description(&self) -> &str {
        "Record or update the task checklist for the current work. Pass the full list each time."
    }
    // No workspace mutation — safe to auto-allow and run speculatively.
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let items: Vec<Todo> = input
            .todos
            .iter()
            .enumerate()
            .map(|(i, t)| Todo {
                id: format!("t{}", i + 1),
                content: t.content.clone(),
                status: parse_status(&t.status),
            })
            .collect();

        let summary = format!("Updated {} todo(s)", items.len());
        let payload = serde_json::to_value(&items).unwrap_or_default();
        ctx.events.emit(Event::TodoUpdate { items });
        Ok(ToolResult::success(summary).with_payload(payload))
    }
}

fn parse_status(s: &str) -> TodoStatus {
    match s {
        "in_progress" | "in-progress" => TodoStatus::InProgress,
        "completed" | "done" => TodoStatus::Completed,
        _ => TodoStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::ctx;
    use serde_json::json;

    #[tokio::test]
    async fn records_todos() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let t = blumi_core::Typed(TodoWrite);
        let res = blumi_core::Tool::execute(
            &t,
            json!({ "todos": [
                { "content": "scaffold", "status": "completed" },
                { "content": "build core", "status": "in_progress" }
            ] }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(res.model_preview.contains("Updated 2 todo"));
        assert!(res.machine_payload.is_some());
    }
}

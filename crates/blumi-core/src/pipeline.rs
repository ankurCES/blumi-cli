//! The tool execution pipeline.
//!
//! Wraps a single tool call with the cross-cutting concerns every UI shares:
//! lookup, permission check, lifecycle events, diff emission, and error
//! classification. (Schema validation, caching, hooks, and artifact spill are
//! later-phase layers; the Typed adapter already rejects malformed arguments.)

use crate::permissions::{PermissionEngine, PermissionOutcome};
use crate::registry::ToolRegistry;
use crate::tool::ToolContext;
use crate::ToolError;
use blumi_protocol::{Event, ToolCall, ToolResult};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Run one tool call through the pipeline, always returning a `ToolResult`
/// (errors are classified into results, never propagated) and emitting the
/// `ToolStart` / `Diff` / `ToolResult` lifecycle events.
pub async fn execute_tool_call(
    registry: &ToolRegistry,
    perms: &PermissionEngine,
    ctx: &ToolContext,
    call: &ToolCall,
    ct: CancellationToken,
) -> ToolResult {
    let Some(tool) = registry.get(&call.name) else {
        let result = ToolResult::invalid_input(
            format!("unknown tool: {}", call.name),
            "call one of the registered tools",
        );
        emit_result(ctx, call, &result, 0);
        return result;
    };

    // Permission gate.
    if let PermissionOutcome::Deny(reason) = perms
        .check(
            tool.name(),
            tool.is_read_only(),
            &call.arguments,
            &ctx.interactor,
            &ctx.events,
        )
        .await
    {
        let result = ToolResult::permission_denied(format!("permission denied: {reason}"));
        emit_result(ctx, call, &result, 0);
        return result;
    }

    ctx.events.emit(Event::ToolStart {
        id: call.id.clone(),
        name: call.name.clone(),
        summary: summarize(&call.name, &call.arguments),
        input: call.arguments.clone(),
    });

    let start = Instant::now();
    let result = match tool.execute(call.arguments.clone(), ctx, ct).await {
        Ok(r) => r,
        Err(ToolError::Cancelled) => ToolResult::cancelled(),
        Err(ToolError::InvalidInput(m)) => {
            ToolResult::invalid_input(m, "fix the arguments and try again")
        }
        Err(e) => ToolResult::error(e.to_string()),
    };
    let duration_ms = start.elapsed().as_millis() as u64;

    if let Some(diff) = &result.diff {
        let (additions, deletions) = count_diff(diff);
        ctx.events.emit(Event::Diff {
            id: call.id.clone(),
            path: call
                .arguments
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string(),
            unified: diff.clone(),
            additions,
            deletions,
        });
    }

    emit_result(ctx, call, &result, duration_ms);
    result
}

fn emit_result(ctx: &ToolContext, call: &ToolCall, result: &ToolResult, duration_ms: u64) {
    ctx.events.emit(Event::ToolResult {
        id: call.id.clone(),
        name: call.name.clone(),
        ok: !result.is_error(),
        preview: truncate(&result.model_preview, 2000),
        duration_ms,
        artifacts: result.artifacts.clone(),
    });
}

/// A short, human-readable summary of a call for the UI card header.
fn summarize(tool: &str, args: &serde_json::Value) -> String {
    match tool {
        "Bash" => args
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        "FileRead" | "FileWrite" | "FileEdit" | "ListDirectory" | "ApplyPatch" => args
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or(tool)
            .to_string(),
        "Glob" | "Grep" => args
            .get("pattern")
            .and_then(|p| p.as_str())
            .unwrap_or(tool)
            .to_string(),
        _ => tool.to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}… (+{} more bytes)", &s[..end], s.len() - end)
    }
}

/// Count added/removed lines in a unified diff (ignoring the +++/--- headers).
fn count_diff(diff: &str) -> (u32, u32) {
    let mut add = 0;
    let mut del = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        match line.as_bytes().first() {
            Some(b'+') => add += 1,
            Some(b'-') => del += 1,
            _ => {}
        }
    }
    (add, del)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_picks_relevant_field() {
        assert_eq!(
            summarize("Bash", &serde_json::json!({ "command": "ls" })),
            "ls"
        );
        assert_eq!(
            summarize("FileWrite", &serde_json::json!({ "path": "a.txt" })),
            "a.txt"
        );
    }

    #[test]
    fn counts_diff_lines() {
        let diff = "--- a\n+++ b\n@@\n-old\n+new\n+extra\n unchanged\n";
        assert_eq!(count_diff(diff), (2, 1));
    }

    #[test]
    fn truncate_is_utf8_safe() {
        let s = "é".repeat(10); // 2 bytes each
        let t = truncate(&s, 5);
        assert!(t.starts_with('é') || t.starts_with("é"));
        assert!(t.contains("more bytes"));
    }
}

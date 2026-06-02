//! Glob and Grep.
//!
//! For Phase 1 these operate on the local filesystem directly (the `glob` crate
//! and a recursive regex walk). When remote execution backends land they'll be
//! routed through the executor.

use crate::path::resolve;
use async_trait::async_trait;
use lumi_core::{ToolContext, ToolError, TypedTool};
use lumi_protocol::{Capability, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::Path;
use tokio_util::sync::CancellationToken;

const MAX_GLOB_RESULTS: usize = 500;
const MAX_GREP_MATCHES: usize = 200;
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", "dist", "build", ".lumi"];

// ---------------------------------------------------------------------------
// Glob
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub struct GlobInput {
    /// Glob pattern, e.g. `**/*.rs`.
    pub pattern: String,
    /// Base directory (default: the working directory).
    #[serde(default)]
    pub path: Option<String>,
}

pub struct Glob;

#[async_trait]
impl TypedTool for Glob {
    type Input = GlobInput;

    fn name(&self) -> &str {
        "Glob"
    }
    fn description(&self) -> &str {
        "Find files matching a glob pattern (e.g. `src/**/*.rs`)."
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn required_capabilities(&self, _input: &Self::Input) -> Vec<Capability> {
        vec![]
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let base = resolve(&ctx.working_dir, input.path.as_deref().unwrap_or("."));
        let full = base.join(&input.pattern);
        let pattern = full.to_string_lossy().into_owned();
        let working = ctx.working_dir.clone();

        let matches = tokio::task::spawn_blocking(move || -> Result<Vec<String>, String> {
            let mut out = Vec::new();
            let paths = glob::glob(&pattern).map_err(|e| e.to_string())?;
            for entry in paths.flatten() {
                if entry.is_file() {
                    let rel = entry.strip_prefix(&working).unwrap_or(&entry);
                    out.push(rel.display().to_string());
                    if out.len() >= MAX_GLOB_RESULTS {
                        break;
                    }
                }
            }
            out.sort();
            Ok(out)
        })
        .await
        .map_err(|e| ToolError::Execution(e.to_string()))?
        .map_err(ToolError::InvalidInput)?;

        if matches.is_empty() {
            return Ok(ToolResult::empty(format!(
                "no files match `{}`",
                input.pattern
            )));
        }
        Ok(ToolResult::success(matches.join("\n")))
    }
}

// ---------------------------------------------------------------------------
// Grep
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub struct GrepInput {
    /// Regular expression to search file contents for.
    pub pattern: String,
    /// Base directory (default: the working directory).
    #[serde(default)]
    pub path: Option<String>,
}

pub struct Grep;

#[async_trait]
impl TypedTool for Grep {
    type Input = GrepInput;

    fn name(&self) -> &str {
        "Grep"
    }
    fn description(&self) -> &str {
        "Search file contents by regular expression, returning `path:line: text` matches."
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn required_capabilities(&self, _input: &Self::Input) -> Vec<Capability> {
        vec![]
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let re = regex::Regex::new(&input.pattern)
            .map_err(|e| ToolError::InvalidInput(format!("invalid regex: {e}")))?;
        let base = resolve(&ctx.working_dir, input.path.as_deref().unwrap_or("."));
        let working = ctx.working_dir.clone();

        let matches = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            walk_grep(&base, &re, &working, &mut out);
            out
        })
        .await
        .map_err(|e| ToolError::Execution(e.to_string()))?;

        if matches.is_empty() {
            return Ok(ToolResult::empty(format!(
                "no matches for `{}`",
                input.pattern
            )));
        }
        let truncated = matches.len() >= MAX_GREP_MATCHES;
        let mut body = matches.join("\n");
        if truncated {
            body.push_str(&format!("\n… (showing first {MAX_GREP_MATCHES} matches)"));
        }
        Ok(ToolResult::success(body))
    }
}

fn walk_grep(dir: &Path, re: &regex::Regex, working: &Path, out: &mut Vec<String>) {
    if out.len() >= MAX_GREP_MATCHES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_GREP_MATCHES {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => walk_grep(&path, re, working, out),
            Ok(ft) if ft.is_file() => {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let rel = path.strip_prefix(working).unwrap_or(&path);
                    for (i, line) in text.lines().enumerate() {
                        if re.is_match(line) {
                            out.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim()));
                            if out.len() >= MAX_GREP_MATCHES {
                                return;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::ctx;
    use serde_json::json;

    #[tokio::test]
    async fn glob_finds_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("b.txt"), "x").unwrap();
        let c = ctx(dir.path());
        let g = lumi_core::Typed(Glob);
        let res = lumi_core::Tool::execute(
            &g,
            json!({ "pattern": "*.rs" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(res.model_preview.contains("a.rs"));
        assert!(!res.model_preview.contains("b.txt"));
    }

    #[tokio::test]
    async fn grep_matches_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "let x = 1;\nlet y = 2;\n").unwrap();
        let c = ctx(dir.path());
        let g = lumi_core::Typed(Grep);
        let res = lumi_core::Tool::execute(
            &g,
            json!({ "pattern": "let y" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(res.model_preview.contains("a.rs:2:"));
        assert!(res.model_preview.contains("let y = 2;"));
    }
}

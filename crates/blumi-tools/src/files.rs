//! File read / write / edit tools. All I/O goes through the executor.

use crate::path::resolve;
use async_trait::async_trait;
use blumi_core::{FileChange, ToolContext, ToolError, TypedTool};
use blumi_protocol::{Capability, SideEffect, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use similar::TextDiff;
use std::path::Path;
use tokio_util::sync::CancellationToken;

/// Record a file's prior contents in the undo journal before a mutation
/// (best-effort; absent journal or unreadable file just records a create).
async fn journal_before(ctx: &ToolContext, path: &Path, op: &str) {
    if let Some(journal) = &ctx.journal {
        let before = ctx.executor.read_file(path).await.ok();
        journal.record(FileChange {
            path: path.to_path_buf(),
            before,
            op: op.to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// FileRead
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub struct FileReadInput {
    /// Absolute path to the file (a relative path is resolved against the
    /// working directory). Accepts `path` or `file_path`.
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    pub path: String,
    /// 1-based line to start at (default 1).
    #[serde(default)]
    pub offset: Option<usize>,
    /// Maximum number of lines to read.
    #[serde(default)]
    pub limit: Option<usize>,
}

pub struct FileRead;

#[async_trait]
impl TypedTool for FileRead {
    type Input = FileReadInput;

    fn name(&self) -> &str {
        "FileRead"
    }
    fn description(&self) -> &str {
        "Read a text file, returned with line numbers. Supports offset/limit for large files."
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn required_capabilities(&self, input: &Self::Input) -> Vec<Capability> {
        vec![Capability::file_read(&input.path)]
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let path = resolve(&ctx.working_dir, &input.path);
        let bytes =
            ctx.executor.read_file(&path).await.map_err(|e| {
                ToolError::Execution(format!("could not read {}: {e}", path.display()))
            })?;
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            return Ok(ToolResult::empty("(empty file)"));
        }
        let start = input.offset.unwrap_or(1).max(1) - 1;
        if start >= lines.len() {
            return Ok(ToolResult::empty(format!(
                "(offset {} past end of file)",
                start + 1
            )));
        }
        let end = input
            .limit
            .map(|l| (start + l).min(lines.len()))
            .unwrap_or(lines.len());

        let mut out = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            out.push_str(&format!("{:>6}\t{}\n", start + i + 1, line));
        }
        Ok(ToolResult::success(out))
    }
}

// ---------------------------------------------------------------------------
// FileWrite
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub struct FileWriteInput {
    /// Absolute path to write, created or overwritten (a relative path is
    /// resolved against the working directory). Accepts `path` or `file_path`.
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    pub path: String,
    /// File contents (this call's chunk when appending).
    #[serde(alias = "contents", alias = "text")]
    pub content: String,
    /// Append `content` to the file instead of overwriting it. Use this to write
    /// a file too large to emit in one response across several calls: write the
    /// first part normally, then call again with the same `path` and
    /// `append: true` for each remaining part. A missing file is created.
    #[serde(default)]
    pub append: bool,
}

pub struct FileWrite;

#[async_trait]
impl TypedTool for FileWrite {
    type Input = FileWriteInput;

    fn name(&self) -> &str {
        "FileWrite"
    }
    fn description(&self) -> &str {
        "Create a new file or overwrite an existing one with the given contents. \
         For content too large to emit in a single response, write it in parts: \
         the first call normally, then call again with the same `path` and \
         `append: true` to append each remaining part. Never truncate or \
         summarize the content to make it fit — append more parts instead."
    }
    fn required_capabilities(&self, input: &Self::Input) -> Vec<Capability> {
        vec![Capability::file_write(&input.path)]
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let path = resolve(&ctx.working_dir, &input.path);
        let chunk_len = input.content.len();
        journal_before(ctx, &path, if input.append { "append" } else { "write" }).await;
        // Append is a read-modify-write so it works across every executor; the
        // journal above already captured the prior contents for undo.
        let bytes: Vec<u8> = if input.append {
            let mut existing = ctx.executor.read_file(&path).await.unwrap_or_default();
            existing.extend_from_slice(input.content.as_bytes());
            existing
        } else {
            input.content.into_bytes()
        };
        let total = bytes.len();
        ctx.executor.write_file(&path, &bytes).await.map_err(|e| {
            ToolError::Execution(format!("could not write {}: {e}", path.display()))
        })?;
        let msg = if input.append {
            format!(
                "Appended {chunk_len} bytes to {} (total {total} bytes)",
                input.path
            )
        } else {
            format!("Wrote {total} bytes to {}", input.path)
        };
        Ok(ToolResult::success(msg)
            .with_side_effects(vec![SideEffect::file_write(input.path, total as u64)]))
    }
}

// ---------------------------------------------------------------------------
// FileEdit
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub struct FileEditInput {
    /// Absolute path to the file to edit (a relative path is resolved against
    /// the working directory). Accepts `path` or `file_path`.
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    pub path: String,
    /// Exact text to replace.
    pub old_string: String,
    /// Replacement text.
    pub new_string: String,
    /// Replace all occurrences instead of requiring a unique match.
    #[serde(default)]
    pub replace_all: bool,
}

pub struct FileEdit;

#[async_trait]
impl TypedTool for FileEdit {
    type Input = FileEditInput;

    fn name(&self) -> &str {
        "FileEdit"
    }
    fn description(&self) -> &str {
        "Replace an exact string in a file. By default the match must be unique; \
         set replace_all to replace every occurrence."
    }
    fn required_capabilities(&self, input: &Self::Input) -> Vec<Capability> {
        vec![Capability::file_write(&input.path)]
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let path = resolve(&ctx.working_dir, &input.path);
        let bytes =
            ctx.executor.read_file(&path).await.map_err(|e| {
                ToolError::Execution(format!("could not read {}: {e}", path.display()))
            })?;
        let original = String::from_utf8_lossy(&bytes).into_owned();

        let count = original.matches(&input.old_string).count();
        if count == 0 {
            return Ok(ToolResult::invalid_input(
                "old_string not found in file",
                "read the file first and copy the exact text (including whitespace) to replace",
            ));
        }
        if count > 1 && !input.replace_all {
            return Ok(ToolResult::state_conflict(
                format!("old_string matches {count} times"),
                "add surrounding context to make the match unique, or set replace_all=true",
            ));
        }

        let updated = if input.replace_all {
            original.replace(&input.old_string, &input.new_string)
        } else {
            original.replacen(&input.old_string, &input.new_string, 1)
        };

        // Record the original for /undo (we already have it in `bytes`).
        if let Some(journal) = &ctx.journal {
            journal.record(FileChange {
                path: path.clone(),
                before: Some(bytes.clone()),
                op: "edit".to_string(),
            });
        }

        ctx.executor
            .write_file(&path, updated.as_bytes())
            .await
            .map_err(|e| {
                ToolError::Execution(format!("could not write {}: {e}", path.display()))
            })?;

        let diff = TextDiff::from_lines(&original, &updated)
            .unified_diff()
            .context_radius(3)
            .header(&input.path, &input.path)
            .to_string();

        Ok(
            ToolResult::success(format!("Edited {} ({count} replacement(s))", input.path))
                .with_diff(diff)
                .with_side_effects(vec![SideEffect::file_write(
                    input.path,
                    updated.len() as u64,
                )]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::ctx;
    use serde_json::json;

    #[test]
    fn file_tools_accept_file_path_alias() {
        // Anthropic models emit `file_path` (their Write/Edit convention) — it
        // must parse as `path` instead of erroring "missing field `path`".
        let w: FileWriteInput =
            serde_json::from_value(json!({ "file_path": "/abs/x.rs", "content": "hi" })).unwrap();
        assert_eq!(w.path, "/abs/x.rs");
        let w2: FileWriteInput =
            serde_json::from_value(json!({ "path": "/abs/y.rs", "contents": "hi" })).unwrap();
        assert_eq!(w2.content, "hi");
        let e: FileEditInput = serde_json::from_value(
            json!({ "file_path": "/abs/z.rs", "old_string": "a", "new_string": "b" }),
        )
        .unwrap();
        assert_eq!(e.path, "/abs/z.rs");
        let r: FileReadInput = serde_json::from_value(json!({ "file_path": "/abs/r.rs" })).unwrap();
        assert_eq!(r.path, "/abs/r.rs");
    }

    #[tokio::test]
    async fn execute_coerces_provider_field_names() {
        // The full execute() path normalizes whatever field names the model/
        // provider wrote (Anthropic editor: file_path/file_text/old_str/new_str;
        // camelCase; double-encoded string args) to the tool's schema keys — so a
        // tool call never fails with "missing field `path`" across model types.
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());

        // FileWrite via Anthropic `create` style (file_path + file_text).
        let w = blumi_core::Typed(FileWrite);
        let res = blumi_core::Tool::execute(
            &w,
            json!({ "command": "create", "file_path": "a.txt", "file_text": "v1\n" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!res.is_error(), "create-style write should parse + run");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "v1\n"
        );

        // FileEdit via `str_replace` style (old_str / new_str) + camelCase path.
        let e = blumi_core::Typed(FileEdit);
        let res = blumi_core::Tool::execute(
            &e,
            json!({ "filePath": "a.txt", "old_str": "v1", "new_str": "v2" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!res.is_error(), "str_replace-style edit should parse + run");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "v2\n"
        );

        // Double-encoded JSON-string arguments (some OpenAI-style gateways).
        let res = blumi_core::Tool::execute(
            &w,
            json!(r#"{"path":"b.txt","content":"hi"}"#),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!res.is_error(), "stringified args should parse + run");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "hi"
        );
    }

    #[tokio::test]
    async fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());

        let w = blumi_core::Typed(FileWrite);
        let res = blumi_core::Tool::execute(
            &w,
            json!({ "path": "a.txt", "content": "line1\nline2\n" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!res.is_error());

        let r = blumi_core::Typed(FileRead);
        let res =
            blumi_core::Tool::execute(&r, json!({ "path": "a.txt" }), &c, CancellationToken::new())
                .await
                .unwrap();
        assert!(res.model_preview.contains("1\tline1"));
        assert!(res.model_preview.contains("2\tline2"));
    }

    #[tokio::test]
    async fn journals_write_and_edit_for_undo() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let journal = Arc::new(blumi_core::ChangeJournal::new());
        let mut c = ctx(dir.path());
        c.journal = Some(journal.clone());

        // Creating a new file records before = None (so undo deletes it).
        let w = blumi_core::Typed(FileWrite);
        blumi_core::Tool::execute(
            &w,
            json!({ "path": "n.txt", "content": "v1" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        // Editing records before = the prior contents (so undo restores them).
        let e = blumi_core::Typed(FileEdit);
        blumi_core::Tool::execute(
            &e,
            json!({ "path": "n.txt", "old_string": "v1", "new_string": "v2" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();

        let edit = journal.pop().unwrap();
        assert_eq!(edit.op, "edit");
        assert_eq!(edit.before.as_deref(), Some(b"v1".as_slice()));
        let create = journal.pop().unwrap();
        assert_eq!(create.op, "write");
        assert!(create.before.is_none());
    }

    #[tokio::test]
    async fn write_append_accumulates() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let w = blumi_core::Typed(FileWrite);

        // First chunk: a normal write creates the file.
        blumi_core::Tool::execute(
            &w,
            json!({ "path": "plan.md", "content": "part 1\n" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        // Subsequent chunks append rather than overwrite — this is how a large
        // file is written across several calls without one giant tool call.
        let res = blumi_core::Tool::execute(
            &w,
            json!({ "path": "plan.md", "content": "part 2\n", "append": true }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!res.is_error());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("plan.md")).unwrap(),
            "part 1\npart 2\n"
        );
    }

    #[tokio::test]
    async fn append_creates_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let w = blumi_core::Typed(FileWrite);
        // `append: true` on a non-existent file just creates it.
        blumi_core::Tool::execute(
            &w,
            json!({ "path": "fresh.md", "content": "hello", "append": true }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("fresh.md")).unwrap(),
            "hello"
        );
    }

    #[tokio::test]
    async fn edit_produces_diff() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();

        let e = blumi_core::Typed(FileEdit);
        let res = blumi_core::Tool::execute(
            &e,
            json!({ "path": "a.txt", "old_string": "world", "new_string": "blumi" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!res.is_error());
        assert!(res.diff.is_some());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "hello blumi\n"
        );
    }

    #[tokio::test]
    async fn edit_rejects_missing_and_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        std::fs::write(dir.path().join("a.txt"), "x x x").unwrap();
        let e = blumi_core::Typed(FileEdit);

        let missing = blumi_core::Tool::execute(
            &e,
            json!({ "path": "a.txt", "old_string": "zzz", "new_string": "y" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(missing.class, blumi_protocol::ResultClass::InvalidInput);

        let ambiguous = blumi_core::Tool::execute(
            &e,
            json!({ "path": "a.txt", "old_string": "x", "new_string": "y" }),
            &c,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(ambiguous.class, blumi_protocol::ResultClass::StateConflict);
    }
}

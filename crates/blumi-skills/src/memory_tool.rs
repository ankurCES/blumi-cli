//! The `memory` tool: persist long-term memory across sessions (hermes-style).
//!
//! Two line-delimited stores — `MEMORY.md` (the agent's own notes) and
//! `USER.md` (facts about the user). Writes go straight to disk; the in-session
//! prompt snapshot stays frozen (so the prefix cache holds), and the next
//! session picks up the changes.

use async_trait::async_trait;
use blumi_core::{SemanticMemory, ToolContext, ToolError, TypedTool};
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryInput {
    /// "add" (remember a fact), "query" (semantic search of long-term memory),
    /// "replace" (swap the entry containing `old_text`), "remove" (delete it),
    /// or "read" (list the current file entries).
    pub action: String,
    /// Which store: "memory" (your own notes) or "user" (facts about the user).
    /// Defaults to "memory".
    #[serde(default)]
    pub target: Option<String>,
    /// Entry text for add/replace, or the search text for `query`.
    #[serde(default)]
    pub content: Option<String>,
    /// A substring identifying the entry to replace/remove.
    #[serde(default)]
    pub old_text: Option<String>,
}

/// Reads/writes the two memory files, and (when configured) mirrors writes into
/// the semantic vector store + answers `query` with cross-session recall.
pub struct MemoryTool {
    memory_md: PathBuf,
    user_md: PathBuf,
    semantic: Option<Arc<dyn SemanticMemory>>,
}

impl MemoryTool {
    pub fn new(memory_md: PathBuf, user_md: PathBuf) -> Self {
        MemoryTool {
            memory_md,
            user_md,
            semantic: None,
        }
    }

    /// Attach the semantic memory store so `add` also writes an embedded,
    /// dedup-gated memory and `query` can search across sessions.
    pub fn with_semantic(mut self, semantic: Arc<dyn SemanticMemory>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    fn path_for(&self, target: Option<&str>) -> &Path {
        match target {
            Some("user") => &self.user_md,
            _ => &self.memory_md,
        }
    }
}

fn read_entries(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim_start_matches("- ").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn write_entries(path: &Path, entries: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = entries
        .iter()
        .map(|e| format!("- {e}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, format!("{body}\n"))
}

fn listing(target: &str, entries: &[String]) -> String {
    if entries.is_empty() {
        return format!("{target} memory is empty.");
    }
    let mut s = format!("{target} memory ({} entries):\n", entries.len());
    for e in entries {
        s.push_str(&format!("- {e}\n"));
    }
    s
}

#[async_trait]
impl TypedTool for MemoryTool {
    type Input = MemoryInput;

    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Persist and recall long-term memory across sessions. action: add | query | replace | \
         remove | read. `add` remembers a fact (also embedded for semantic recall, with \
         near-duplicates merged automatically); `query` does a semantic search of everything \
         remembered (set `content` to the search text). target: \"memory\" (your own notes) or \
         \"user\" (durable facts/preferences about the user). Relevant memories are also surfaced \
         automatically each turn, so use `query` for targeted lookups and `add` to capture \
         decisions, conventions, and preferences worth keeping."
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn run(
        &self,
        input: MemoryInput,
        _ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let target = input.target.as_deref().unwrap_or("memory");
        let path = self.path_for(input.target.as_deref()).to_path_buf();
        let mut entries = read_entries(&path);

        match input.action.as_str() {
            "read" => {}
            "query" => {
                let q = input
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .ok_or_else(|| ToolError::InvalidInput("query requires `content`".into()))?;
                let Some(sem) = &self.semantic else {
                    return Ok(ToolResult::success(
                        "Semantic memory is disabled; only file-based memory (action=read) is \
                         available.",
                    ));
                };
                let hits = sem.query(None, q, 8).await;
                if hits.is_empty() {
                    return Ok(ToolResult::success(format!("No memories found for '{q}'.")));
                }
                let mut out = format!("{} memories for '{q}':\n", hits.len());
                for h in &hits {
                    out.push_str(&format!(
                        "- [{}] {}\n",
                        h.namespace,
                        h.text.replace('\n', " ")
                    ));
                }
                return Ok(ToolResult::success(out));
            }
            "add" => {
                let content = input
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .ok_or_else(|| ToolError::InvalidInput("add requires `content`".into()))?;
                if !entries.iter().any(|e| e == content) {
                    entries.push(content.to_string());
                }
                // Mirror into the semantic store (embedded + dedup-gated) so it
                // is recallable across sessions, not just folded into the prompt.
                if let Some(sem) = &self.semantic {
                    let (ns, kind) = if target == "user" {
                        ("user", "fact")
                    } else {
                        ("agent", "note")
                    };
                    sem.remember(ns, kind, content).await;
                }
            }
            "replace" => {
                let old = input
                    .old_text
                    .as_deref()
                    .ok_or_else(|| ToolError::InvalidInput("replace requires `old_text`".into()))?;
                let content =
                    input.content.as_deref().map(str::trim).ok_or_else(|| {
                        ToolError::InvalidInput("replace requires `content`".into())
                    })?;
                match entries.iter_mut().find(|e| e.contains(old)) {
                    Some(e) => *e = content.to_string(),
                    None => {
                        return Ok(ToolResult::invalid_input(
                            format!("no entry contains '{old}'"),
                            "check the current entries with action=read",
                        ))
                    }
                }
            }
            "remove" => {
                let old = input
                    .old_text
                    .as_deref()
                    .ok_or_else(|| ToolError::InvalidInput("remove requires `old_text`".into()))?;
                let before = entries.len();
                entries.retain(|e| !e.contains(old));
                if entries.len() == before {
                    return Ok(ToolResult::invalid_input(
                        format!("no entry contains '{old}'"),
                        "check the current entries with action=read",
                    ));
                }
            }
            other => {
                return Ok(ToolResult::invalid_input(
                    format!("unknown action '{other}'"),
                    "use add, query, replace, remove, or read",
                ))
            }
        }

        if input.action != "read" {
            if let Err(e) = write_entries(&path, &entries) {
                return Ok(ToolResult::error(format!("could not write memory: {e}")));
            }
        }
        Ok(ToolResult::success(listing(target, &entries)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_modify_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mem = dir.path().join("MEMORY.md");
        write_entries(&mem, &["likes rust".into(), "prefers tabs".into()]).unwrap();
        let entries = read_entries(&mem);
        assert_eq!(entries, vec!["likes rust", "prefers tabs"]);

        // replace by substring
        let mut e = entries.clone();
        if let Some(x) = e.iter_mut().find(|x| x.contains("tabs")) {
            *x = "prefers spaces".into();
        }
        write_entries(&mem, &e).unwrap();
        assert_eq!(read_entries(&mem)[1], "prefers spaces");
    }
}

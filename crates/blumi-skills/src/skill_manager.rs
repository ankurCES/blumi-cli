//! The `manage_skill` tool: the agent authors its own skills (self-improvement).
//!
//! Skills are written as `<user-skills-dir>/<name>/SKILL.md` (the same layout
//! [`crate::SkillCatalog`] discovers). Writes are jailed to the user skills dir
//! by validating that the name is a plain slug — no path separators, no `..` —
//! so the agent can never escape the directory. Newly written skills become
//! active after a `reload_self` (or the next session).

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SkillManagerInput {
    /// "create" / "update" (write a SKILL.md) or "delete" (remove it).
    pub action: String,
    /// Skill name — a slug of letters, digits, '-' or '_' (this is also the
    /// directory name). Used to load the skill later via the `skill` tool.
    pub name: String,
    /// One-line description shown in the skill listing (create/update).
    #[serde(default)]
    pub description: String,
    /// The skill's full instructions — the markdown body the `skill` tool
    /// returns (create/update).
    #[serde(default)]
    pub instructions: String,
}

/// Creates / updates / deletes skills under the user skills directory.
pub struct SkillManager {
    dir: PathBuf,
}

impl SkillManager {
    /// `dir` is the user skills directory (e.g. `~/.blumi/skills`).
    pub fn new(dir: PathBuf) -> Self {
        SkillManager { dir }
    }
}

/// A safe skill slug: non-empty, only `[A-Za-z0-9_-]`, not all dots.
fn valid_slug(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Render a SKILL.md with YAML frontmatter the catalog parses.
fn render_skill_md(name: &str, description: &str, instructions: &str) -> String {
    // Keep the description single-line so the frontmatter stays valid.
    let desc = description.replace('\n', " ");
    format!(
        "---\nname: {name}\ndescription: {desc}\n---\n\n{}\n",
        instructions.trim()
    )
}

#[async_trait]
impl TypedTool for SkillManager {
    type Input = SkillManagerInput;

    fn name(&self) -> &str {
        "manage_skill"
    }

    fn description(&self) -> &str {
        "Author your own skills (self-improvement). action: create | update | delete. A skill is \
         reusable instructions for a recurring task, stored as SKILL.md and listed under \"Skills\" \
         in the system prompt. Provide `name` (a slug), `description` (one line), and `instructions` \
         (the full markdown body). After writing, call `reload_self` to load it into the session."
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn run(
        &self,
        input: SkillManagerInput,
        _ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        if !valid_slug(&input.name) {
            return Ok(ToolResult::invalid_input(
                format!("invalid skill name '{}'", input.name),
                "use a slug of letters, digits, '-' or '_' (max 64 chars), e.g. \"pdf-wrangler\"",
            ));
        }
        let skill_dir = self.dir.join(&input.name);
        let skill_md = skill_dir.join("SKILL.md");

        match input.action.as_str() {
            "create" | "update" => {
                let desc = input.description.trim();
                if desc.is_empty() {
                    return Ok(ToolResult::invalid_input(
                        "create/update requires a `description`",
                        "give a one-line summary of what the skill is for",
                    ));
                }
                if input.instructions.trim().is_empty() {
                    return Ok(ToolResult::invalid_input(
                        "create/update requires `instructions`",
                        "give the full markdown body the skill should provide",
                    ));
                }
                if let Err(e) = std::fs::create_dir_all(&skill_dir) {
                    return Ok(ToolResult::error(format!(
                        "could not create skill dir: {e}"
                    )));
                }
                let body = render_skill_md(&input.name, desc, &input.instructions);
                if let Err(e) = atomic_write(&skill_md, &body) {
                    return Ok(ToolResult::error(format!("could not write skill: {e}")));
                }
                Ok(ToolResult::success(format!(
                    "skill '{}' written to {}. Call `reload_self` to load it into this session.",
                    input.name,
                    skill_md.display()
                )))
            }
            "delete" => {
                if !skill_md.is_file() {
                    return Ok(ToolResult::invalid_input(
                        format!("no skill named '{}'", input.name),
                        "list available skills first, or check the name",
                    ));
                }
                // Remove the whole skill directory (SKILL.md + any linked files).
                if let Err(e) = std::fs::remove_dir_all(&skill_dir) {
                    return Ok(ToolResult::error(format!("could not delete skill: {e}")));
                }
                Ok(ToolResult::success(format!(
                    "skill '{}' deleted. Call `reload_self` to drop it from this session.",
                    input.name
                )))
            }
            other => Ok(ToolResult::invalid_input(
                format!("unknown action '{other}'"),
                "use create, update, or delete",
            )),
        }
    }
}

/// Write `contents` to `path` atomically (temp file in the same dir + rename).
fn atomic_write(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SkillCatalog;

    #[test]
    fn slug_validation() {
        assert!(valid_slug("pdf-wrangler"));
        assert!(valid_slug("my_skill1"));
        assert!(!valid_slug(""));
        assert!(!valid_slug("../escape"));
        assert!(!valid_slug("a/b"));
        assert!(!valid_slug("dot.dot"));
        assert!(!valid_slug(&"x".repeat(65)));
    }

    #[test]
    fn rendered_skill_is_discoverable() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("greeter");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let md = render_skill_md("greeter", "Greets people", "Say a warm hello.");
        atomic_write(&skill_dir.join("SKILL.md"), &md).unwrap();

        let cat = SkillCatalog::load(&[dir.path().to_path_buf()]);
        let sk = cat.get("greeter").expect("skill discovered");
        assert_eq!(sk.description, "Greets people");
        assert!(sk.body.contains("Say a warm hello."));
        // The temp file must not have been left behind.
        assert!(!skill_dir.join("SKILL.md.tmp").exists());
    }
}

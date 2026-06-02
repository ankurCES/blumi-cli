//! System-prompt assembly: base instructions + project context (BLUMI.md) +
//! the frozen memory snapshot.

use blumi_config::BlumiConfig;
use blumi_skills::MemorySnapshot;

const BASE: &str = "\
You are blumi, a local-first coding assistant that works directly in the user's \
project. You have tools to read, search, write, and edit files and to run shell \
commands. Prefer acting with your tools over guessing: read files before editing \
them, and verify changes when you can.

Guidelines:
- Keep responses concise. Explain what you did, not what you are about to do.
- Make the smallest change that satisfies the request; match surrounding style.
- Use FileEdit for targeted changes and FileWrite for new files.
- Use Bash for builds, tests, and inspection. Don't run destructive commands \
unless asked.
- When a task has multiple steps, track them with TodoWrite.";

const SELF_EVOLUTION: &str = "\
# Self-evolution

You can extend and reconfigure yourself, then reload to apply the changes:
- `manage_skill` — author reusable skills (SKILL.md) for tasks you expect to \
recur. When you find yourself working out a non-obvious procedure, capture it as \
a skill so future sessions can load it via the `skill` tool.
- `self_config` — read and edit your own settings.json (e.g. tune sampling, add \
a persona). Edits are validated before they're saved, so a bad value is rejected \
rather than breaking you.
- `reload_self` — rebuild yourself so newly written skills and config edits take \
effect, keeping the current conversation. Call it after a `manage_skill` or \
`self_config` change.

Evolve deliberately: prefer small, well-described skills and minimal config \
changes, and tell the user what you changed and why.";

/// Build the full system prompt for a session. `skills_section` is the
/// available-skills listing (empty when there are none).
pub fn build_system_prompt(
    config: &BlumiConfig,
    memory: &MemorySnapshot,
    skills_section: &str,
) -> String {
    let mut s = String::with_capacity(2048);
    s.push_str(BASE);
    s.push_str("\n\n");
    s.push_str(SELF_EVOLUTION);
    s.push_str(&format!(
        "\n\nWorking directory: {}\n",
        config.paths.working_dir.display()
    ));

    let blumi_md = config.paths.working_dir.join("BLUMI.md");
    if let Ok(content) = std::fs::read_to_string(&blumi_md) {
        let content = content.trim();
        if !content.is_empty() {
            s.push_str("\n# Project context (BLUMI.md)\n\n");
            s.push_str(content);
            s.push('\n');
        }
    }

    if !skills_section.is_empty() {
        s.push('\n');
        s.push_str(skills_section);
    }

    let mem = memory.to_prompt_section();
    if !mem.is_empty() {
        s.push('\n');
        s.push_str(&mem);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn advertises_self_evolution() {
        let config = BlumiConfig::default();
        let memory = MemorySnapshot::load(
            &PathBuf::from("/nonexistent/MEMORY.md"),
            &PathBuf::from("/nonexistent/USER.md"),
        );
        let prompt = build_system_prompt(&config, &memory, "");
        assert!(prompt.contains("# Self-evolution"));
        assert!(prompt.contains("manage_skill"));
        assert!(prompt.contains("self_config"));
        assert!(prompt.contains("reload_self"));
    }
}

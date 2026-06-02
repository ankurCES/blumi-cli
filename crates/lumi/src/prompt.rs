//! System-prompt assembly: base instructions + project context (LUMI.md) +
//! the frozen memory snapshot.

use lumi_config::LumiConfig;
use lumi_skills::MemorySnapshot;

const BASE: &str = "\
You are lumi, a local-first coding assistant that works directly in the user's \
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

/// Build the full system prompt for a session.
pub fn build_system_prompt(config: &LumiConfig, memory: &MemorySnapshot) -> String {
    let mut s = String::with_capacity(2048);
    s.push_str(BASE);
    s.push_str(&format!(
        "\n\nWorking directory: {}\n",
        config.paths.working_dir.display()
    ));

    let lumi_md = config.paths.working_dir.join("LUMI.md");
    if let Ok(content) = std::fs::read_to_string(&lumi_md) {
        let content = content.trim();
        if !content.is_empty() {
            s.push_str("\n# Project context (LUMI.md)\n\n");
            s.push_str(content);
            s.push('\n');
        }
    }

    let mem = memory.to_prompt_section();
    if !mem.is_empty() {
        s.push('\n');
        s.push_str(&mem);
    }
    s
}

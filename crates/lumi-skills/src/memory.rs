//! Dual memory: `MEMORY.md` (agent notes) + `USER.md` (user profile).
//!
//! Loaded as a **frozen snapshot at session start** and folded into the system
//! prompt. Mid-session writes go to disk but do not alter the snapshot, so the
//! provider's prompt-prefix cache stays valid for the whole session. The next
//! session picks up the updated files.

use std::path::Path;

/// Cap on each memory file folded into the prompt (keeps the prefix bounded).
const MAX_SECTION_BYTES: usize = 16 * 1024;

/// A point-in-time copy of the agent + user memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub agent_memory: String,
    pub user_profile: String,
}

impl MemorySnapshot {
    /// Load both files (missing/unreadable files become empty), sanitized and
    /// length-capped.
    pub fn load(memory_md: &Path, user_md: &Path) -> Self {
        MemorySnapshot {
            agent_memory: sanitize(&read(memory_md)),
            user_profile: sanitize(&read(user_md)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.agent_memory.is_empty() && self.user_profile.is_empty()
    }

    /// Render the snapshot as a system-prompt section (empty string if nothing).
    pub fn to_prompt_section(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        if !self.user_profile.is_empty() {
            out.push_str("# About the user\n\n");
            out.push_str(&self.user_profile);
            out.push_str("\n\n");
        }
        if !self.agent_memory.is_empty() {
            out.push_str("# Things you've learned (memory)\n\n");
            out.push_str(&self.agent_memory);
            out.push_str("\n\n");
        }
        out.push_str(
            "The sections above are a snapshot from the start of this session; \
             treat them as background, verify before relying on specifics.\n",
        );
        out
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Deterministic sanitization: normalize newlines, trim, cap length. Determinism
/// matters because this feeds the cached prompt prefix.
fn sanitize(raw: &str) -> String {
    let normalized = raw.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.len() <= MAX_SECTION_BYTES {
        return trimmed.to_string();
    }
    let mut end = MAX_SECTION_BYTES;
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…(truncated)", &trimmed[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_yield_empty_snapshot() {
        let snap = MemorySnapshot::load(Path::new("/no/such/MEMORY.md"), Path::new("/no/USER.md"));
        assert!(snap.is_empty());
        assert_eq!(snap.to_prompt_section(), "");
    }

    #[test]
    fn loads_and_renders_section() {
        let dir = tempfile::tempdir().unwrap();
        let mem = dir.path().join("MEMORY.md");
        let usr = dir.path().join("USER.md");
        std::fs::write(&mem, "prefers Rust\r\n").unwrap();
        std::fs::write(&usr, "  name: Ankur  ").unwrap();

        let snap = MemorySnapshot::load(&mem, &usr);
        assert_eq!(snap.agent_memory, "prefers Rust");
        assert_eq!(snap.user_profile, "name: Ankur");

        let section = snap.to_prompt_section();
        assert!(section.contains("About the user"));
        assert!(section.contains("name: Ankur"));
        assert!(section.contains("memory"));
        assert!(section.contains("prefers Rust"));
    }

    #[test]
    fn long_memory_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let mem = dir.path().join("MEMORY.md");
        std::fs::write(&mem, "x".repeat(MAX_SECTION_BYTES + 100)).unwrap();
        let snap = MemorySnapshot::load(&mem, Path::new("/no/USER.md"));
        assert!(snap.agent_memory.len() <= MAX_SECTION_BYTES + 20);
        assert!(snap.agent_memory.ends_with("…(truncated)"));
    }
}

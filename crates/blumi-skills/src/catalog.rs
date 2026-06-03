//! Discovery of SKILL.md skills (agentskills.io layout: `<dir>/<name>/SKILL.md`
//! with optional YAML frontmatter `name:` / `description:` then a markdown body).

/// A discovered skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// Name + description only — for the system-prompt listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
}

/// All skills found across the configured directories.
#[derive(Debug, Default)]
pub struct SkillCatalog {
    skills: Vec<Skill>,
}

impl SkillCatalog {
    /// Scan each directory for `*/SKILL.md`. Later directories override earlier
    /// ones on name collision (so a project skill shadows a user skill).
    pub fn load(dirs: &[std::path::PathBuf]) -> Self {
        let mut skills: Vec<Skill> = Vec::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let skill_md = entry.path().join("SKILL.md");
                if !skill_md.is_file() {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&skill_md) else {
                    continue;
                };
                let fallback = entry.file_name().to_string_lossy().into_owned();
                let skill = parse_skill(&text, &fallback);
                // de-dup by name (override)
                skills.retain(|s| s.name != skill.name);
                skills.push(skill);
            }
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        SkillCatalog { skills }
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn list(&self) -> Vec<SkillMeta> {
        self.skills
            .iter()
            .map(|s| SkillMeta {
                name: s.name.clone(),
                description: s.description.clone(),
            })
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// The system-prompt section advertising available skills, or empty.
    pub fn prompt_section(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut s = String::from(
            "# Skills\n\nThese skills are available. Call the `skill` tool with a name to load \
             its full instructions before using it:\n",
        );
        for skill in &self.skills {
            s.push_str(&format!(
                "- {}: {}\n",
                skill.name,
                one_line(&skill.description)
            ));
        }
        s
    }
}

/// The two frontmatter keys blumi reads. Everything else (license, version,
/// `allowed-tools`, nested maps, …) is accepted and ignored — serde_yaml does
/// not deny unknown fields — so real-world SKILL.md files parse cleanly.
#[derive(serde::Deserialize, Default)]
struct Frontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// Parse a SKILL.md: optional `---` YAML frontmatter, then the markdown body.
/// Uses a real YAML parser so multi-line/quoted descriptions, colons in values,
/// and extra keys all work; on any YAML error we degrade to the directory name
/// + first body line rather than dropping the skill.
fn parse_skill(text: &str, fallback_name: &str) -> Skill {
    let mut front = "";
    let mut body = text;

    if let Some(rest) = text.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            front = &rest[..end];
            body = rest[end + 4..].trim_start_matches('\n');
        }
    }

    let fm: Frontmatter = serde_yaml::from_str(front).unwrap_or_default();

    let name = fm
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    let description = fm
        .description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| {
            body.lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .unwrap_or("")
                .to_string()
        });

    Skill {
        name,
        description,
        body: body.trim().to_string(),
    }
}

fn one_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_frontmatter() {
        let text =
            "---\nname: pdf-wrangler\ndescription: Work with PDFs\n---\n\n# PDF\n\nDo the thing.";
        let s = parse_skill(text, "dir-name");
        assert_eq!(s.name, "pdf-wrangler");
        assert_eq!(s.description, "Work with PDFs");
        assert!(s.body.contains("Do the thing."));
        assert!(!s.body.contains("name:"));
    }

    #[test]
    fn tolerates_real_world_frontmatter() {
        // Extra keys, a quoted multi-line description with colons, a nested map —
        // none of which the old line-parser could handle.
        let text = "---\n\
            name: db-expert\n\
            description: \"Use when: migrations, indexes; covers Postgres & MySQL\"\n\
            license: MIT\n\
            allowed-tools: [Bash, FileEdit]\n\
            metadata:\n  author: someone\n  version: 2\n\
            ---\n\n# DB\n\nBody here.";
        let s = parse_skill(text, "dir");
        assert_eq!(s.name, "db-expert");
        assert_eq!(
            s.description,
            "Use when: migrations, indexes; covers Postgres & MySQL"
        );
        assert!(s.body.contains("Body here."));
        assert!(!s.body.contains("license"));
    }

    #[test]
    fn malformed_frontmatter_degrades_gracefully() {
        // Not valid YAML after the fence → fall back, don't drop the skill.
        let text = "---\nname: : : oops\n\tbad: indent\n---\n\nReal first line.";
        let s = parse_skill(text, "fallback-name");
        assert_eq!(s.name, "fallback-name");
        assert_eq!(s.description, "Real first line.");
    }

    #[test]
    fn falls_back_without_frontmatter() {
        let s = parse_skill("# Title\n\nFirst real line here.", "my-skill");
        assert_eq!(s.name, "my-skill");
        assert_eq!(s.description, "First real line here.");
    }

    #[test]
    fn loads_and_lists_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let sk = dir.path().join("greeter");
        fs::create_dir_all(&sk).unwrap();
        fs::write(
            sk.join("SKILL.md"),
            "---\nname: greeter\ndescription: Greets\n---\nSay hi.",
        )
        .unwrap();

        let cat = SkillCatalog::load(&[dir.path().to_path_buf()]);
        assert!(!cat.is_empty());
        assert_eq!(cat.list()[0].name, "greeter");
        assert!(cat.get("greeter").unwrap().body.contains("Say hi."));
        assert!(cat.prompt_section().contains("greeter: Greets"));
    }
}

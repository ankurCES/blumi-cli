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

/// Parse a SKILL.md: optional `---` frontmatter with `name:`/`description:`,
/// then the markdown body. Falls back to the directory name + first body line.
fn parse_skill(text: &str, fallback_name: &str) -> Skill {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut body = text;

    if let Some(rest) = text.strip_prefix("---") {
        // find the closing fence
        if let Some(end) = rest.find("\n---") {
            let front = &rest[..end];
            body = rest[end + 4..].trim_start_matches('\n');
            for line in front.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
                    match k.trim() {
                        "name" => name = Some(v),
                        "description" => description = Some(v),
                        _ => {}
                    }
                }
            }
        }
    }

    let name = name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    let description = description.filter(|d| !d.is_empty()).unwrap_or_else(|| {
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

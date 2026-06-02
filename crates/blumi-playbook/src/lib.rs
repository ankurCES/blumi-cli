//! YAML playbooks: ordered multi-step workflows with optional gates and
//! checkpoint/resume.
//!
//! This crate is the pure model + parsing + checkpoint layer; the binary runs a
//! step's `prompt` as a headless agent session and evaluates `gate` shell
//! checks, so this stays dependency-light and fully testable.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum PlaybookError {
    #[error("could not read playbook: {0}")]
    Io(String),
    #[error("invalid playbook: {0}")]
    Parse(String),
}

/// One step in a playbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub name: String,
    /// The prompt to run for this step.
    pub prompt: String,
    /// Optional shell command; the step runs only if it exits 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// Keep going if this step errors (default: stop the playbook).
    #[serde(default)]
    pub continue_on_error: bool,
}

/// A named, ordered workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playbook {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<Step>,
}

impl Playbook {
    /// Parse a playbook from YAML.
    pub fn from_yaml(yaml: &str) -> Result<Playbook, PlaybookError> {
        let pb: Playbook =
            serde_yaml::from_str(yaml).map_err(|e| PlaybookError::Parse(e.to_string()))?;
        if pb.name.trim().is_empty() {
            return Err(PlaybookError::Parse("playbook needs a name".into()));
        }
        if pb.steps.is_empty() {
            return Err(PlaybookError::Parse(
                "playbook needs at least one step".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        for s in &pb.steps {
            if s.name.trim().is_empty() {
                return Err(PlaybookError::Parse("every step needs a name".into()));
            }
            if !seen.insert(&s.name) {
                return Err(PlaybookError::Parse(format!(
                    "duplicate step name '{}'",
                    s.name
                )));
            }
        }
        Ok(pb)
    }

    /// Load + parse a playbook file.
    pub fn load(path: impl AsRef<Path>) -> Result<Playbook, PlaybookError> {
        let text =
            std::fs::read_to_string(path.as_ref()).map_err(|e| PlaybookError::Io(e.to_string()))?;
        Self::from_yaml(&text)
    }
}

/// Persisted run progress, so a re-run resumes after the last completed step.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PlaybookState {
    pub completed: BTreeSet<String>,
}

impl PlaybookState {
    /// Load state from `path` (missing/invalid → empty).
    pub fn load(path: impl AsRef<Path>) -> Self {
        std::fs::read_to_string(path.as_ref())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(path, body)
    }

    pub fn is_done(&self, step: &str) -> bool {
        self.completed.contains(step)
    }

    pub fn mark_done(&mut self, step: &str) {
        self.completed.insert(step.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = "\
name: ship
description: build and verify
steps:
  - name: implement
    prompt: do the thing
  - name: test
    prompt: run tests
    gate: test -f Cargo.toml
  - name: summary
    prompt: summarize
    continue_on_error: true
";

    #[test]
    fn parses_playbook() {
        let pb = Playbook::from_yaml(YAML).unwrap();
        assert_eq!(pb.name, "ship");
        assert_eq!(pb.steps.len(), 3);
        assert_eq!(pb.steps[1].gate.as_deref(), Some("test -f Cargo.toml"));
        assert!(pb.steps[2].continue_on_error);
        assert!(!pb.steps[0].continue_on_error);
    }

    #[test]
    fn rejects_invalid() {
        assert!(Playbook::from_yaml("name: x\nsteps: []").is_err()); // no steps
        assert!(Playbook::from_yaml("steps:\n  - {name: a, prompt: p}").is_err()); // no name
        let dup = "name: x\nsteps:\n  - {name: a, prompt: p}\n  - {name: a, prompt: q}";
        assert!(Playbook::from_yaml(dup).is_err()); // duplicate step
    }

    #[test]
    fn state_resumes() {
        let mut st = PlaybookState::default();
        assert!(!st.is_done("implement"));
        st.mark_done("implement");
        assert!(st.is_done("implement"));
        // serde roundtrip
        let json = serde_json::to_string(&st).unwrap();
        let back: PlaybookState = serde_json::from_str(&json).unwrap();
        assert!(back.is_done("implement"));
        assert!(!back.is_done("test"));
    }
}

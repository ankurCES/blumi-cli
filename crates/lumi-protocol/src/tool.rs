//! The result of executing a tool. A faithful port of OpenMono's rich
//! `ToolResult` record: a model-facing preview, an optional structured
//! payload, a classification (for retry/error handling), and side-channel
//! data (artifacts, diffs, images, side effects).

use crate::message::ImageRef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How a tool execution turned out. Drives the agent loop's error handling
/// and whether a retry hint is surfaced to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultClass {
    Success,
    /// The model passed bad arguments; it should fix and retry.
    InvalidInput,
    /// Blocked by the permission engine.
    PermissionDenied,
    /// The world changed underneath the call (e.g. file modified since read).
    StateConflict,
    /// The tool itself crashed/errored.
    Crash,
    /// Succeeded but produced nothing useful.
    Empty,
    /// Cancelled before completion.
    Cancelled,
}

impl ResultClass {
    pub fn is_success(self) -> bool {
        matches!(self, ResultClass::Success | ResultClass::Empty)
    }
}

/// A reference to a stored artifact (large output spilled to disk).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub kind: String,
    pub bytes: u64,
    pub path: String,
}

/// A recorded side effect of a tool (for journaling / undo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideEffect {
    pub kind: String,
    pub target: String,
    #[serde(default)]
    pub meta: BTreeMap<String, String>,
}

impl SideEffect {
    pub fn file_write(path: impl Into<String>, bytes: u64) -> Self {
        SideEffect {
            kind: "file_write".into(),
            target: path.into(),
            meta: BTreeMap::from([("bytes".to_string(), bytes.to_string())]),
        }
    }
    pub fn file_delete(path: impl Into<String>) -> Self {
        SideEffect {
            kind: "file_delete".into(),
            target: path.into(),
            meta: BTreeMap::new(),
        }
    }
    pub fn process_spawn(command: impl Into<String>, pid: Option<u32>) -> Self {
        let mut meta = BTreeMap::new();
        if let Some(pid) = pid {
            meta.insert("pid".to_string(), pid.to_string());
        }
        SideEffect {
            kind: "process_spawn".into(),
            target: command.into(),
            meta,
        }
    }
}

/// The outcome of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// Text shown to the model (and, by default, the user).
    pub model_preview: String,
    /// Optional machine-readable payload for downstream tooling/UIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_payload: Option<serde_json::Value>,
    pub class: ResultClass,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub side_effects: Vec<SideEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Unified diff produced by an edit, for rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageRef>,
    /// A hint to the model on how to recover from a non-success result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_hint: Option<String>,
    /// If true, end the turn and surface the preview to the user (plan mode).
    #[serde(default)]
    pub break_turn: bool,
}

impl ToolResult {
    fn of(preview: impl Into<String>, class: ResultClass) -> Self {
        ToolResult {
            model_preview: preview.into(),
            machine_payload: None,
            class,
            artifacts: Vec::new(),
            side_effects: Vec::new(),
            warnings: Vec::new(),
            diff: None,
            images: Vec::new(),
            retry_hint: None,
            break_turn: false,
        }
    }

    pub fn success(preview: impl Into<String>) -> Self {
        Self::of(preview, ResultClass::Success)
    }
    pub fn empty(preview: impl Into<String>) -> Self {
        Self::of(preview, ResultClass::Empty)
    }
    pub fn error(preview: impl Into<String>) -> Self {
        Self::of(preview, ResultClass::Crash)
    }
    pub fn invalid_input(preview: impl Into<String>, retry_hint: impl Into<String>) -> Self {
        Self::of(preview, ResultClass::InvalidInput).with_retry_hint(retry_hint)
    }
    pub fn permission_denied(preview: impl Into<String>) -> Self {
        Self::of(preview, ResultClass::PermissionDenied)
    }
    pub fn state_conflict(preview: impl Into<String>, retry_hint: impl Into<String>) -> Self {
        Self::of(preview, ResultClass::StateConflict).with_retry_hint(retry_hint)
    }
    pub fn cancelled() -> Self {
        Self::of("Operation was cancelled", ResultClass::Cancelled)
    }

    pub fn is_error(&self) -> bool {
        !self.class.is_success()
    }

    pub fn with_payload(mut self, payload: serde_json::Value) -> Self {
        self.machine_payload = Some(payload);
        self
    }
    pub fn with_retry_hint(mut self, hint: impl Into<String>) -> Self {
        self.retry_hint = Some(hint.into());
        self
    }
    pub fn with_diff(mut self, diff: impl Into<String>) -> Self {
        self.diff = Some(diff.into());
        self
    }
    pub fn with_images(mut self, images: Vec<ImageRef>) -> Self {
        self.images = images;
        self
    }
    pub fn with_artifacts(mut self, artifacts: Vec<ArtifactRef>) -> Self {
        self.artifacts = artifacts;
        self
    }
    pub fn with_side_effects(mut self, effects: Vec<SideEffect>) -> Self {
        self.side_effects = effects;
        self
    }
    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }
    pub fn with_break_turn(mut self) -> Self {
        self.break_turn = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_is_not_error() {
        let r = ToolResult::success("done");
        assert!(!r.is_error());
        assert_eq!(r.class, ResultClass::Success);
    }

    #[test]
    fn invalid_input_carries_hint_and_is_error() {
        let r = ToolResult::invalid_input("bad path", "use an absolute path");
        assert!(r.is_error());
        assert_eq!(r.retry_hint.as_deref(), Some("use an absolute path"));
    }

    #[test]
    fn builders_compose() {
        let r = ToolResult::success("edited")
            .with_diff("@@ -1 +1 @@")
            .with_side_effects(vec![SideEffect::file_write("/tmp/a", 3)]);
        assert!(r.diff.is_some());
        assert_eq!(r.side_effects.len(), 1);
    }

    #[test]
    fn round_trips() {
        let r = ToolResult::success("ok").with_payload(serde_json::json!({"n": 1}));
        let json = serde_json::to_string(&r).unwrap();
        let back: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}

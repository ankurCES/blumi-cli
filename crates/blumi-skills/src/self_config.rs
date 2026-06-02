//! The `self_config` tool: the agent edits its own configuration
//! (self-evolution). Writes go to the user `settings.json`.
//!
//! Safety: every write is **validated before it lands** — the proposed JSON must
//! still deserialize as a [`blumi_config::BlumiConfig`], so the agent can change
//! a value but cannot corrupt the file or brick itself with a type error. The
//! write is atomic (temp + rename) and `0600`. Changes take effect after a
//! `reload_self` (or the next session).

use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SelfConfigInput {
    /// "get" (show current settings), "set" (set one key), or "add_persona".
    pub action: String,
    /// Dotted key for "set", e.g. "llm.temperature" or "ui.theme".
    #[serde(default)]
    pub key: String,
    /// Value for "set", as JSON (`0.5`, `true`, `"anthropic"`, `["a","b"]`).
    /// Bare text that isn't valid JSON is stored as a string.
    #[serde(default)]
    pub value: String,
    /// Persona name (for "add_persona") — the key it's stored under.
    #[serde(default)]
    pub name: String,
    /// Persona description (for "add_persona").
    #[serde(default)]
    pub description: String,
    /// Persona instructions appended to the system prompt (for "add_persona").
    #[serde(default)]
    pub instructions: String,
    /// Optional model id the persona switches to (for "add_persona").
    #[serde(default)]
    pub model: String,
}

/// Reads/edits the user `settings.json`.
pub struct SelfConfig {
    path: PathBuf,
}

impl SelfConfig {
    /// `path` is the user settings file (e.g. `~/.blumi/settings.json`).
    pub fn new(path: PathBuf) -> Self {
        SelfConfig { path }
    }
}

/// Read settings.json as a JSON object (empty object if missing/invalid).
fn read_root(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()))
}

/// Set a dotted `key` (creating intermediate objects) to `val`.
fn set_dotted(root: &mut Value, key: &str, val: Value) -> Result<(), String> {
    let parts: Vec<&str> = key.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err("empty key".into());
    }
    let mut cur = root;
    for p in &parts[..parts.len() - 1] {
        let obj = cur
            .as_object_mut()
            .ok_or("cannot descend into a non-object value")?;
        cur = obj
            .entry((*p).to_string())
            .or_insert_with(|| Value::Object(Default::default()));
        if !cur.is_object() {
            *cur = Value::Object(Default::default());
        }
    }
    let last = parts[parts.len() - 1];
    cur.as_object_mut()
        .ok_or("cannot set a key on a non-object value")?
        .insert(last.to_string(), val);
    Ok(())
}

/// The proposed settings must still deserialize as a `BlumiConfig`.
fn validate(root: &Value) -> Result<(), String> {
    serde_json::from_value::<blumi_config::BlumiConfig>(root.clone())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Atomically write settings (temp + rename), `0600` on unix.
fn write_settings(path: &Path, root: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(root).unwrap_or_else(|_| "{}".into());
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}

const APPLY_HINT: &str = "Call `reload_self` to apply the change to this session.";

#[async_trait]
impl TypedTool for SelfConfig {
    type Input = SelfConfigInput;

    fn name(&self) -> &str {
        "self_config"
    }

    fn description(&self) -> &str {
        "Edit your own configuration (self-evolution), persisted to settings.json. \
         action: get (show current settings) | set (set one dotted `key` to a JSON `value`, e.g. \
         key=\"llm.temperature\" value=\"0.3\") | add_persona (define a reusable persona from \
         `name`/`description`/`instructions` and optional `model`). Edits are validated before they \
         land — an invalid change is rejected, never written. After a change, call `reload_self`."
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn run(
        &self,
        input: SelfConfigInput,
        _ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        match input.action.as_str() {
            "get" => {
                let root = read_root(&self.path);
                let text = serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".into());
                Ok(ToolResult::success(format!(
                    "current settings.json:\n{text}"
                )))
            }
            "set" => {
                if input.key.trim().is_empty() {
                    return Ok(ToolResult::invalid_input(
                        "set requires a `key`",
                        "e.g. key=\"llm.temperature\" value=\"0.3\"",
                    ));
                }
                // Parse the value as JSON; fall back to a bare string.
                let parsed: Value = serde_json::from_str(&input.value)
                    .unwrap_or_else(|_| Value::String(input.value.clone()));
                let mut root = read_root(&self.path);
                if let Err(e) = set_dotted(&mut root, &input.key, parsed) {
                    return Ok(ToolResult::invalid_input(
                        format!("cannot set '{}': {e}", input.key),
                        "check the key path",
                    ));
                }
                if let Err(e) = validate(&root) {
                    return Ok(ToolResult::invalid_input(
                        format!("that change would make the config invalid: {e}"),
                        "the file was left unchanged — fix the value type and retry",
                    ));
                }
                if let Err(e) = write_settings(&self.path, &root) {
                    return Ok(ToolResult::error(format!("could not write settings: {e}")));
                }
                Ok(ToolResult::success(format!(
                    "set {} in {}. {APPLY_HINT}",
                    input.key,
                    self.path.display()
                )))
            }
            "add_persona" => {
                let name = input.name.trim();
                if name.is_empty() {
                    return Ok(ToolResult::invalid_input(
                        "add_persona requires a `name`",
                        "give the persona a short name",
                    ));
                }
                if input.description.trim().is_empty() || input.instructions.trim().is_empty() {
                    return Ok(ToolResult::invalid_input(
                        "add_persona requires `description` and `instructions`",
                        "describe the persona and give its system-prompt instructions",
                    ));
                }
                let mut persona = serde_json::Map::new();
                persona.insert(
                    "description".into(),
                    Value::String(input.description.clone()),
                );
                persona.insert(
                    "instructions".into(),
                    Value::String(input.instructions.clone()),
                );
                if !input.model.trim().is_empty() {
                    persona.insert("model".into(), Value::String(input.model.clone()));
                }

                let mut root = read_root(&self.path);
                let personas = root
                    .as_object_mut()
                    .expect("read_root returns an object")
                    .entry("personas")
                    .or_insert_with(|| Value::Object(Default::default()));
                if !personas.is_object() {
                    *personas = Value::Object(Default::default());
                }
                personas
                    .as_object_mut()
                    .unwrap()
                    .insert(name.to_string(), Value::Object(persona));

                if let Err(e) = validate(&root) {
                    return Ok(ToolResult::invalid_input(
                        format!("that persona would make the config invalid: {e}"),
                        "the file was left unchanged",
                    ));
                }
                if let Err(e) = write_settings(&self.path, &root) {
                    return Ok(ToolResult::error(format!("could not write settings: {e}")));
                }
                Ok(ToolResult::success(format!(
                    "persona '{name}' added. Switch to it with the persona picker / `/persona`. {APPLY_HINT}"
                )))
            }
            other => Ok(ToolResult::invalid_input(
                format!("unknown action '{other}'"),
                "use get, set, or add_persona",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_dotted_creates_nested() {
        let mut root = Value::Object(Default::default());
        set_dotted(&mut root, "llm.temperature", serde_json::json!(0.3)).unwrap();
        assert_eq!(root["llm"]["temperature"], serde_json::json!(0.3));
    }

    #[test]
    fn set_dotted_coerces_intermediate_scalar() {
        let mut root = serde_json::json!({ "llm": 5 });
        set_dotted(&mut root, "llm.model", serde_json::json!("x")).unwrap();
        assert_eq!(root["llm"]["model"], serde_json::json!("x"));
    }

    #[test]
    fn validate_rejects_bad_type() {
        // `personas` must be a map, not a number.
        let bad = serde_json::json!({ "personas": 5 });
        assert!(validate(&bad).is_err());
        // A valid partial passes.
        let ok = serde_json::json!({ "llm": { "temperature": 0.3 } });
        assert!(validate(&ok).is_ok());
    }

    #[test]
    fn write_then_read_roundtrips_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut root = read_root(&path); // missing → {}
        set_dotted(&mut root, "persona", serde_json::json!("coder")).unwrap();
        validate(&root).unwrap();
        write_settings(&path, &root).unwrap();

        let back = read_root(&path);
        assert_eq!(back["persona"], serde_json::json!("coder"));
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn add_persona_shape_validates() {
        let mut root = Value::Object(Default::default());
        let personas = root
            .as_object_mut()
            .unwrap()
            .entry("personas")
            .or_insert_with(|| Value::Object(Default::default()));
        personas.as_object_mut().unwrap().insert(
            "pirate".into(),
            serde_json::json!({ "description": "Arr", "instructions": "Talk like a pirate." }),
        );
        assert!(validate(&root).is_ok());
    }
}

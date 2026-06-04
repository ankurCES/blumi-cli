//! The tool abstraction.
//!
//! Tools implement [`Tool`] directly, or — more ergonomically — implement
//! [`TypedTool`] (with a typed, `JsonSchema`-deriving input) and are wrapped in
//! [`Typed`] to become a `Tool`. We use a wrapper rather than a blanket
//! `impl<T: TypedTool> Tool for T` because the latter conflicts (under
//! coherence) with hand-written `Tool` impls such as the MCP adapter.

use crate::emit::{EventEmitter, Interactor};
use crate::error::ToolError;
use crate::exec::Executor;
use async_trait::async_trait;
use blumi_protocol::{Capability, SessionId, ToolResult};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Spawns sub-agents (the `delegate` tool's backend). Implemented in the core
/// over the same machinery as the top-level agent; child agents get a
/// restricted toolset and their own budget.
#[async_trait]
pub trait SubAgentSpawner: Send + Sync {
    /// The available sub-agent type names (for discovery / error messages).
    fn agent_types(&self) -> Vec<String>;

    /// Run a sub-agent of `agent_type` on `prompt`, returning its final text.
    /// `interactor` is the parent's, so child approvals still reach the user.
    async fn spawn(
        &self,
        agent_type: &str,
        prompt: &str,
        events: EventEmitter,
        interactor: Interactor,
        ct: CancellationToken,
    ) -> Result<String, ToolError>;
}

/// A recorded file mutation, so `/undo` can revert it (LIFO).
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    /// Prior contents, or `None` if the file did not exist (a fresh create).
    pub before: Option<Vec<u8>>,
    /// Short operation label (e.g. `"write"`, `"edit"`).
    pub op: String,
}

/// An in-session, last-in-first-out journal of file mutations backing `/undo`.
/// File-writing tools push a [`FileChange`] before they mutate; the actor pops
/// and reverts on `Command::Undo`.
#[derive(Default)]
pub struct ChangeJournal {
    entries: std::sync::Mutex<Vec<FileChange>>,
}

impl ChangeJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a mutation about to happen.
    pub fn record(&self, change: FileChange) {
        self.entries.lock().expect("journal poisoned").push(change);
    }

    /// Take the most recent mutation for reverting.
    pub fn pop(&self) -> Option<FileChange> {
        self.entries.lock().expect("journal poisoned").pop()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().expect("journal poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Everything a tool needs at execution time. Notably it carries an
/// [`Executor`] (so file/shell ops respect the active backend) and channels to
/// the user — never a concrete UI.
#[derive(Clone)]
pub struct ToolContext {
    pub session_id: SessionId,
    pub working_dir: PathBuf,
    pub executor: Arc<dyn Executor>,
    pub events: EventEmitter,
    pub interactor: Interactor,
    /// Present when sub-agent delegation is available.
    pub spawner: Option<Arc<dyn SubAgentSpawner>>,
    /// Present when undo journaling is active; file tools record prior state here.
    pub journal: Option<Arc<ChangeJournal>>,
}

/// A tool the model can call. Object-safe (via `async_trait`) so the registry
/// can hold `Arc<dyn Tool>`.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON Schema for the tool's arguments.
    fn input_schema(&self) -> serde_json::Value;

    /// May run concurrently with other concurrency-safe tools.
    fn is_concurrency_safe(&self) -> bool {
        false
    }
    /// Does not mutate the workspace (safe to run speculatively while streaming).
    fn is_read_only(&self) -> bool {
        false
    }
    /// Only surfaced to the model on demand (via ToolSearch), not in the base
    /// tool list.
    fn is_deferred(&self) -> bool {
        false
    }

    /// Capabilities this specific invocation needs (checked by the pipeline's
    /// permission layer before `execute`).
    fn required_capabilities(&self, _input: &serde_json::Value) -> Vec<Capability> {
        Vec::new()
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
        ct: CancellationToken,
    ) -> Result<ToolResult, ToolError>;
}

/// Build a JSON Schema for a tool input type.
pub fn schema_for<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(T))
        .unwrap_or_else(|_| serde_json::json!({ "type": "object" }))
}

/// Parse tool arguments into a typed value, mapping failures to `InvalidInput`.
/// The message is made actionable (the raw serde error like "missing field
/// `path`" alone tends to make models retry the same mistake) and echoes back
/// the keys the model actually sent so it can self-correct.
pub fn parse_input<T: DeserializeOwned>(input: serde_json::Value) -> Result<T, ToolError> {
    let sent = match &input {
        serde_json::Value::Object(m) if !m.is_empty() => format!(
            " You sent these keys: {{{}}}.",
            m.keys().cloned().collect::<Vec<_>>().join(", ")
        ),
        serde_json::Value::Object(_) => " You sent an empty object.".to_string(),
        serde_json::Value::Null => " You sent no arguments.".to_string(),
        _ => " You sent a non-object value; arguments must be a JSON object.".to_string(),
    };
    serde_json::from_value(input).map_err(|e| {
        let raw = e.to_string();
        let hint = if raw.contains("missing field") {
            " — include every required field as a JSON object matching the tool's schema. \
             For file tools, pass the file path as `path` (absolute preferred) and the file \
             contents as `content`."
        } else {
            " — send the arguments as a JSON object matching the tool's schema."
        };
        ToolError::InvalidInput(format!("invalid tool arguments: {raw}{hint}{sent}"))
    })
}

/// Semantic synonyms for tool-argument fields that models name differently
/// depending on the provider/convention — e.g. Anthropic's built-in text-editor
/// tool teaches models `file_text` / `old_str` / `new_str`, while others emit
/// `contents` or `filename`. Keyed by the schema's canonical field name. Pure
/// case/separator differences (`filePath`, `file-path`, `File_Path`) are handled
/// generically by [`coerce_tool_input`], so this lists only different *words*.
fn arg_synonyms(canonical: &str) -> &'static [&'static str] {
    match canonical {
        "path" => &[
            "file_path",
            "filepath",
            "file",
            "filename",
            "file_name",
            "path_to_file",
            "pathname",
            "target_file",
            "target_path",
        ],
        "content" => &[
            "contents",
            "text",
            "file_text",
            "file_contents",
            "data",
            "body",
            "new_content",
        ],
        "old_string" => &["old_str", "old", "old_text", "search_string", "find"],
        "new_string" => &["new_str", "new", "new_text", "replacement"],
        _ => &[],
    }
}

/// Normalize model-written tool arguments so they deserialize regardless of the
/// provider's field-naming convention. For each property the tool's schema
/// declares but the input is missing, find an equivalent key the model actually
/// sent — matching case-/separator-insensitively (so `filePath`, `File_Path`,
/// `file-path` all map to `file_path`) or via [`arg_synonyms`] (`file_text` →
/// `content`, `old_str` → `old_string`, …) — and copy its value to the canonical
/// name. Canonical keys already present are never overwritten. Also unwraps a
/// double-encoded JSON-string argument object (some OpenAI-style gateways send
/// the arguments as a string rather than an object).
pub fn coerce_tool_input(
    mut input: serde_json::Value,
    schema: &serde_json::Value,
) -> serde_json::Value {
    // Some gateways double-encode the arguments as a JSON string.
    if let serde_json::Value::String(s) = &input {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
            if parsed.is_object() {
                input = parsed;
            }
        }
    }
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return input;
    };
    let prop_names: Vec<String> = props.keys().cloned().collect();
    let serde_json::Value::Object(map) = &mut input else {
        return input;
    };

    // Loose key form: lowercase, drop every non-alphanumeric char.
    fn canon(k: &str) -> String {
        k.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect()
    }

    for key in &prop_names {
        let want = canon(key);
        let syns: Vec<String> = arg_synonyms(key).iter().map(|s| canon(s)).collect();
        // Non-canonical keys the model sent that mean this field — matched
        // case-/separator-insensitively or via the synonym table. We exclude keys
        // that are themselves schema properties so we never steal one field for
        // another.
        let candidates: Vec<String> = map
            .keys()
            .filter(|k| !prop_names.iter().any(|p| p == *k))
            .filter(|k| {
                let ck = canon(k);
                ck == want || syns.contains(&ck)
            })
            .cloned()
            .collect();
        if candidates.is_empty() {
            continue;
        }
        // Adopt the first synonym's value under the canonical name (only if the
        // model didn't already use the canonical name).
        if !map.contains_key(key) {
            if let Some(v) = map.get(&candidates[0]).cloned() {
                map.insert(key.clone(), v);
            }
        }
        // Drop every synonym spelling: a leftover serde-`alias` synonym (e.g.
        // `file_path` alongside the `path` we just set) would otherwise fail
        // deserialization as a duplicate field.
        for cand in candidates {
            map.remove(&cand);
        }
    }
    input
}

/// Ergonomic tool definition: implement this with a typed input and wrap the
/// value in [`Typed`] when registering.
#[async_trait]
pub trait TypedTool: Send + Sync + 'static {
    type Input: DeserializeOwned + JsonSchema + Send;

    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn is_concurrency_safe(&self) -> bool {
        false
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_deferred(&self) -> bool {
        false
    }
    fn required_capabilities(&self, _input: &Self::Input) -> Vec<Capability> {
        Vec::new()
    }

    async fn run(
        &self,
        input: Self::Input,
        ctx: &ToolContext,
        ct: CancellationToken,
    ) -> Result<ToolResult, ToolError>;
}

/// Adapts a [`TypedTool`] into a [`Tool`].
pub struct Typed<T>(pub T);

#[async_trait]
impl<T: TypedTool> Tool for Typed<T> {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn input_schema(&self) -> serde_json::Value {
        schema_for::<T::Input>()
    }
    fn is_concurrency_safe(&self) -> bool {
        self.0.is_concurrency_safe()
    }
    fn is_read_only(&self) -> bool {
        self.0.is_read_only()
    }
    fn is_deferred(&self) -> bool {
        self.0.is_deferred()
    }
    fn required_capabilities(&self, input: &serde_json::Value) -> Vec<Capability> {
        let input = coerce_tool_input(input.clone(), &self.input_schema());
        match serde_json::from_value::<T::Input>(input) {
            Ok(typed) => self.0.required_capabilities(&typed),
            Err(_) => Vec::new(),
        }
    }
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
        ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        // Normalize provider-specific field names (file_path/file_text/old_str/…)
        // to the tool's canonical schema keys before deserializing, so tool calls
        // parse regardless of which model/provider wrote them.
        let input = coerce_tool_input(input, &self.input_schema());
        let typed = parse_input::<T::Input>(input)?;
        self.0.run(typed, ctx, ct).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, JsonSchema)]
    struct EchoInput {
        text: String,
    }

    struct Echo;

    #[async_trait]
    impl TypedTool for Echo {
        type Input = EchoInput;
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echoes its input"
        }
        fn is_read_only(&self) -> bool {
            true
        }
        async fn run(
            &self,
            input: EchoInput,
            _ctx: &ToolContext,
            _ct: CancellationToken,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::success(input.text))
        }
    }

    #[test]
    fn typed_tool_exposes_schema_and_flags() {
        let t = Typed(Echo);
        assert_eq!(t.name(), "echo");
        assert!(t.is_read_only());
        let schema = t.input_schema();
        // The generated object schema should mention the `text` property.
        assert!(schema.to_string().contains("text"));
    }

    #[test]
    fn parse_input_rejects_bad_args() {
        let r = parse_input::<EchoInput>(serde_json::json!({ "wrong": 1 }));
        assert!(matches!(r, Err(ToolError::InvalidInput(_))));
    }

    // A file-tool-shaped input for exercising argument coercion independent of
    // the blumi-tools crate.
    #[derive(Deserialize, JsonSchema)]
    struct FileyInput {
        path: String,
        content: String,
        #[serde(default)]
        old_string: String,
        #[serde(default)]
        new_string: String,
        #[serde(default)]
        replace_all: bool,
    }

    fn coerce_filey(v: serde_json::Value) -> serde_json::Value {
        coerce_tool_input(v, &schema_for::<FileyInput>())
    }

    #[test]
    fn coerce_accepts_anthropic_editor_and_camelcase_field_names() {
        // Anthropic's built-in editor convention: file_path / file_text.
        let a: FileyInput = serde_json::from_value(coerce_filey(serde_json::json!({
            "command": "create",
            "file_path": "/abs/x.rs",
            "file_text": "hello",
        })))
        .unwrap();
        assert_eq!(a.path, "/abs/x.rs");
        assert_eq!(a.content, "hello");

        // str_replace convention: old_str / new_str.
        let b: FileyInput = serde_json::from_value(coerce_filey(serde_json::json!({
            "path": "/abs/y.rs",
            "content": "x",
            "old_str": "a",
            "new_str": "b",
        })))
        .unwrap();
        assert_eq!(b.old_string, "a");
        assert_eq!(b.new_string, "b");

        // camelCase / PascalCase / kebab-case all fold to the snake_case key.
        let c: FileyInput = serde_json::from_value(coerce_filey(serde_json::json!({
            "filePath": "/abs/z.rs",
            "Content": "c",
            "replaceAll": true,
        })))
        .unwrap();
        assert_eq!(c.path, "/abs/z.rs");
        assert_eq!(c.content, "c");
        assert!(c.replace_all);
    }

    #[test]
    fn coerce_unwraps_double_encoded_string_arguments() {
        // Some OpenAI-style gateways pass `arguments` as a JSON string.
        let v = coerce_filey(serde_json::Value::String(
            r#"{"file_path":"/a.rs","content":"x"}"#.to_string(),
        ));
        let parsed: FileyInput = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.path, "/a.rs");
        assert_eq!(parsed.content, "x");
    }

    #[test]
    fn coerce_is_noop_when_canonical_keys_present() {
        let v = coerce_filey(serde_json::json!({ "path": "/a", "content": "b" }));
        assert_eq!(v["path"], "/a");
        assert_eq!(v["content"], "b");
        // No spurious optional keys were synthesized.
        assert!(v.get("old_string").is_none());
    }

    #[test]
    fn parse_input_error_echoes_sent_keys() {
        // `file_path` is not a serde alias on this struct, so without coercion the
        // error must name the keys we received so the model can self-correct.
        let r = parse_input::<FileyInput>(serde_json::json!({ "file_path": "/a", "content": "b" }));
        match r {
            Err(ToolError::InvalidInput(m)) => {
                assert!(m.contains("missing field"), "explains failure: {m}");
                assert!(m.contains("file_path"), "echoes sent keys: {m}");
            }
            _ => panic!("expected InvalidInput"),
        }
    }

    #[test]
    fn change_journal_is_lifo() {
        let j = ChangeJournal::new();
        assert!(j.is_empty());
        j.record(FileChange {
            path: PathBuf::from("a.txt"),
            before: None,
            op: "write".into(),
        });
        j.record(FileChange {
            path: PathBuf::from("b.txt"),
            before: Some(b"old".to_vec()),
            op: "edit".into(),
        });
        assert_eq!(j.len(), 2);
        // Last in, first out.
        let top = j.pop().unwrap();
        assert_eq!(top.path, PathBuf::from("b.txt"));
        assert_eq!(top.before.as_deref(), Some(b"old".as_slice()));
        let next = j.pop().unwrap();
        assert_eq!(next.path, PathBuf::from("a.txt"));
        assert!(next.before.is_none());
        assert!(j.pop().is_none());
    }
}

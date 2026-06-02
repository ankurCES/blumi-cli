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
pub fn parse_input<T: DeserializeOwned>(input: serde_json::Value) -> Result<T, ToolError> {
    serde_json::from_value(input).map_err(|e| ToolError::InvalidInput(e.to_string()))
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
        match serde_json::from_value::<T::Input>(input.clone()) {
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

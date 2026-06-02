//! The blumi core: the UI-agnostic brain.
//!
//! Defines the three extension points every other crate plugs into —
//! [`Tool`], [`LlmClient`], and [`Executor`] — plus the session actor that
//! drives the agent loop and broadcasts a single [`blumi_protocol::Event`]
//! stream. UIs (TUI, web) are just subscribers; they never see internals.

mod actor;
mod agent;
mod context;
mod emit;
mod error;
mod eventlog;
mod exec;
mod llm;
mod permissions;
mod pipeline;
mod registry;
mod runner;
mod session;
mod subagent;
mod tool;

pub use actor::{spawn_session, SessionClosed, SessionHandle};
pub use agent::AgentTurnRunner;
pub use context::ContextManager;
pub use emit::{EventEmitter, InteractionKind, InteractionReply, InteractionRequest, Interactor};
pub use error::{ExecError, LlmError, ToolError};
pub use eventlog::EventLog;
pub use exec::{DirEntry, ExecOutput, ExecRequest, Executor};
pub use llm::{LlmClient, LlmOptions, ProviderCaps, ToolSpec};
pub use permissions::{PermissionEngine, PermissionOutcome};
pub use pipeline::execute_tool_call;
pub use registry::ToolRegistry;
pub use runner::{TurnContext, TurnRunner};
pub use session::{SessionSnapshot, SessionState};
pub use subagent::{builtin_agents, AgentDef, AgentSpawner};
pub use tool::{
    parse_input, schema_for, ChangeJournal, FileChange, SubAgentSpawner, Tool, ToolContext, Typed,
    TypedTool,
};

// Re-export the protocol vocabulary so downstream crates can depend on just
// `blumi-core` for the common types.
pub use blumi_protocol as protocol;

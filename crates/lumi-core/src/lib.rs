//! The lumi core: the UI-agnostic brain.
//!
//! Defines the three extension points every other crate plugs into —
//! [`Tool`], [`LlmClient`], and [`Executor`] — plus the session actor that
//! drives the agent loop and broadcasts a single [`lumi_protocol::Event`]
//! stream. UIs (TUI, web) are just subscribers; they never see internals.

mod actor;
mod emit;
mod error;
mod eventlog;
mod exec;
mod llm;
mod runner;
mod session;
mod tool;

pub use actor::{spawn_session, SessionClosed, SessionHandle};
pub use emit::{
    EventEmitter, InteractionKind, InteractionReply, InteractionRequest, Interactor,
};
pub use error::{ExecError, LlmError, ToolError};
pub use eventlog::EventLog;
pub use exec::{ExecOutput, ExecRequest, Executor};
pub use llm::{LlmClient, LlmOptions, ProviderCaps};
pub use runner::{TurnContext, TurnRunner};
pub use session::{SessionSnapshot, SessionState};
pub use tool::{parse_input, schema_for, Tool, ToolContext, Typed, TypedTool};

// Re-export the protocol vocabulary so downstream crates can depend on just
// `lumi-core` for the common types.
pub use lumi_protocol as protocol;

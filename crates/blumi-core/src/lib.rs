//! The blumi core: the UI-agnostic brain.
//!
//! Defines the three extension points every other crate plugs into —
//! [`Tool`], [`LlmClient`], and [`Executor`] — plus the session actor that
//! drives the agent loop and broadcasts a single [`blumi_protocol::Event`]
//! stream. UIs (TUI, web) are just subscribers; they never see internals.

mod actor;
mod agent;
mod brain;
mod checkpoint;
mod context;
mod emit;
mod error;
mod eventlog;
mod exec;
pub mod fcm;
mod hooks;
mod llm;
mod memory;
mod permissions;
mod persona;
mod pipeline;
pub mod push;
mod recovery;
mod registry;
mod router;
mod runner;
mod session;
mod subagent;
mod tool;

pub use actor::{spawn_session, spawn_session_seeded, SessionClosed, SessionHandle};
pub use agent::AgentTurnRunner;
pub use brain::{Brain, BrainDecision, BrainMode, BrainVerdict, LocalBrain};
pub use checkpoint::{Checkpoint, CheckpointSink};
pub use context::{summarize_history, ContextManager};
pub use emit::{EventEmitter, InteractionKind, InteractionReply, InteractionRequest, Interactor};
pub use error::{ExecError, LlmError, ToolError};
pub use eventlog::EventLog;
pub use exec::{DirEntry, ExecOutput, ExecRequest, Executor};
pub use hooks::run_prompt_hooks;
pub use llm::{EmbeddingClient, LlmClient, LlmOptions, ProviderCaps, ToolSpec};
pub use memory::{RecalledMemory, SemanticMemory};
pub use permissions::{PermissionEngine, PermissionOutcome};
pub use persona::{builtin_personas, Persona};
pub use pipeline::execute_tool_call;
pub use recovery::{action_for, redact, RecoveryAction, RecoveryBudget};
pub use registry::ToolRegistry;
pub use router::{
    active_router, active_router_mode, active_router_status, set_active_router, Judge, Router,
    RouterMode, RouterStats, Tier, TierClient,
};
pub use runner::{TurnContext, TurnRunner};
pub use session::{SessionSnapshot, SessionState};
pub use subagent::{
    builtin_agents, grid_dispatch, grid_embed, grid_info, set_grid_dispatch, set_grid_embed,
    set_grid_info, set_grid_overflow, AgentDef, AgentSpawner, GridDispatch, GridEmbed, GridInfo,
    GridOverflow, DEFAULT_MAX_LOCAL_AGENTS,
};
pub use tool::{
    coerce_tool_input, parse_input, schema_for, ChangeJournal, FileChange, SubAgentSpawner, Tool,
    ToolContext, Typed, TypedTool,
};

// Re-export the protocol vocabulary so downstream crates can depend on just
// `blumi-core` for the common types.
pub use blumi_protocol as protocol;

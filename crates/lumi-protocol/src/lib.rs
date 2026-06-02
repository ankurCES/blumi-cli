//! Wire contract shared by the lumi core and every UI.
//!
//! Pure serde types — no behavior. The core emits [`Event`]s (wrapped in an
//! [`Envelope`] with a monotonic sequence number) and accepts [`Command`]s;
//! both the TUI and the web server are just subscribers over a channel
//! carrying these types. Keeping this crate dependency-light and
//! behavior-free is what lets every other crate agree on one vocabulary.

mod capability;
mod command;
mod event;
mod ids;
mod message;
mod stream;
mod tool;

pub use capability::Capability;
pub use command::{ApprovalScope, Command, Decision};
pub use event::{ClarifyChoice, DoneReason, Envelope, Event, Todo, TodoStatus};
pub use ids::{MessageId, RequestId, SessionId, StreamId, ToolCallId};
pub use message::{ContentPart, ImageData, ImageRef, Message, Role, ToolCall};
pub use stream::{FinishReason, StreamChunk, ToolCallDelta, Usage};
pub use tool::{ArtifactRef, ResultClass, SideEffect, ToolResult};

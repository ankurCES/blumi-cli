//! Generic LSP client + a code-intelligence tool.
//!
//! A small JSON-RPC-over-stdio client ([`LspClient`]) drives any configured
//! language server, and [`LspTool`] exposes definitions / references / hover /
//! document symbols to the agent. Servers are configured per file-extension, so
//! it's language-agnostic (the C#-specific Roslyn tool from OpenMono is gone).

mod client;
mod framing;
mod tool;

pub use client::LspClient;
pub use tool::{LspServer, LspTool};

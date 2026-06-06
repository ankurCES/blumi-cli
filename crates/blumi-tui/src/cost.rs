//! Cost estimation for the dashboard's spend analytics.
//!
//! The price table now lives in `blumi-config` so the core, the task board, and
//! the routing stats can all share one source of truth. This re-export keeps the
//! TUI's existing `crate::cost::{estimate, is_priced}` call sites working.

pub use blumi_config::pricing::{estimate, is_priced};

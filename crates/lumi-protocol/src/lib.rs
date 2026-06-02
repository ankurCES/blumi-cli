//! Wire contract shared by the lumi core and every UI.
//!
//! Pure serde types — no behavior. The core emits [`Event`]s and accepts
//! [`Command`]s; both the TUI and the web server are just subscribers over a
//! channel carrying these types.

// Module contents land in task #2.

//! Execution backends for lumi.
//!
//! [`LocalExecutor`] runs commands and file ops on the host. Docker and SSH
//! backends (behind feature flags) will implement the same [`Executor`] trait
//! so tools need no changes to run in a container or on a remote host.

mod local;

pub use local::LocalExecutor;

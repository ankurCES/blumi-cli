//! Execution backends for blumi.
//!
//! [`LocalExecutor`] runs commands and file ops on the host; [`DockerExecutor`]
//! sandboxes command execution inside a container (files via a bind mount).
//! Both implement the same [`blumi_core::Executor`] trait, so tools need no
//! changes to run locally or in a container. (SSH/remote backends will plug in
//! the same way.)

mod docker;
mod local;

pub use docker::{DockerExecutor, CONTAINER_WORKDIR};
pub use local::LocalExecutor;

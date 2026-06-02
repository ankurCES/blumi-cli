//! The execution backend abstraction.
//!
//! Every file and shell tool runs through an [`Executor`] rather than touching
//! the filesystem/process API directly. That's what lets the *same* tools run
//! locally, in a Docker container, or over SSH by swapping the backend — the
//! abstraction OpenMono lacked (ported from hermes' `BaseEnvironment`).

use crate::error::ExecError;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// A command to run in the backend.
#[derive(Debug, Clone)]
pub struct ExecRequest {
    /// The command line, run via the backend's shell.
    pub command: String,
    /// Working directory (relative to the backend root, or absolute).
    pub cwd: Option<String>,
    /// Extra environment variables for this command.
    pub env: BTreeMap<String, String>,
    /// Optional wall-clock timeout.
    pub timeout: Option<Duration>,
}

impl ExecRequest {
    pub fn new(command: impl Into<String>) -> Self {
        ExecRequest {
            command: command.into(),
            cwd: None,
            env: BTreeMap::new(),
            timeout: None,
        }
    }

    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    pub fn cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

/// The result of running a command.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.exit_code == 0 && !self.timed_out
    }
}

/// An execution environment: runs commands and reads/writes files within some
/// filesystem namespace (the local host, a container, a remote machine, ...).
#[async_trait]
pub trait Executor: Send + Sync {
    /// Prepare the environment (e.g. snapshot a login shell's environment).
    /// Called once when a session starts. Default is a no-op.
    async fn init_session(&self) -> Result<(), ExecError> {
        Ok(())
    }

    /// Run a command to completion.
    async fn exec(&self, req: ExecRequest, ct: CancellationToken) -> Result<ExecOutput, ExecError>;

    /// Read a file's raw bytes (binary-safe, for images etc.).
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, ExecError>;

    /// Write a file's raw bytes, creating parent directories as needed.
    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<(), ExecError>;

    /// Whether a path exists in the backend.
    async fn exists(&self, path: &Path) -> Result<bool, ExecError>;

    /// Tear down the environment. Default is a no-op.
    async fn cleanup(&self) -> Result<(), ExecError> {
        Ok(())
    }

    /// The root working directory commands run in (the backend's notion of cwd).
    fn working_dir(&self) -> &Path;
}

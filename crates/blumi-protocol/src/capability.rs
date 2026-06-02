//! Capabilities a tool requests, checked by the permission engine before
//! execution. Modeled on OpenMono's `Capability` hierarchy.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A discrete permission a tool needs for a specific invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    /// Read a file at `path`.
    FileRead { path: PathBuf },
    /// Create/modify/delete a file at `path`.
    FileWrite { path: PathBuf },
    /// Execute a process / shell command.
    ProcessExec { command: String },
    /// Make an outbound network request to `host`.
    NetworkEgress { host: String },
    /// Mutate a version-control repo (commit, reset, push, ...).
    VcsMutation { repo: String, op: String },
    /// Spawn a sub-agent of the given kind.
    AgentSpawn { agent: String },
}

impl Capability {
    pub fn file_read(path: impl Into<PathBuf>) -> Self {
        Capability::FileRead { path: path.into() }
    }
    pub fn file_write(path: impl Into<PathBuf>) -> Self {
        Capability::FileWrite { path: path.into() }
    }
    pub fn process_exec(command: impl Into<String>) -> Self {
        Capability::ProcessExec {
            command: command.into(),
        }
    }

    /// A short human label used in approval prompts.
    pub fn label(&self) -> String {
        match self {
            Capability::FileRead { path } => format!("read {}", path.display()),
            Capability::FileWrite { path } => format!("write {}", path.display()),
            Capability::ProcessExec { command } => format!("run `{command}`"),
            Capability::NetworkEgress { host } => format!("network to {host}"),
            Capability::VcsMutation { repo, op } => format!("vcs {op} in {repo}"),
            Capability::AgentSpawn { agent } => format!("spawn {agent} agent"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_round_trips() {
        let c = Capability::file_write("/tmp/x");
        let json = serde_json::to_string(&c).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }
}

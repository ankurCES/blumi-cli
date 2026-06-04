//! Sub-agent definitions and the spawner that runs them.
//!
//! A sub-agent is a fresh agent loop with a restricted toolset, its own system
//! prompt, and its own iteration budget. The `delegate` tool calls
//! [`SubAgentSpawner::spawn`]; the [`AgentSpawner`] here implements it by
//! building a child [`AgentTurnRunner`] over the same provider/executor.

use crate::agent::AgentTurnRunner;
use crate::emit::{EventEmitter, Interactor};
use crate::error::ToolError;
use crate::exec::Executor;
use crate::llm::{LlmClient, LlmOptions};
use crate::permissions::PermissionEngine;
use crate::registry::ToolRegistry;
use crate::runner::{TurnContext, TurnRunner};
use crate::session::SessionState;
use crate::tool::SubAgentSpawner;
use async_trait::async_trait;
use blumi_protocol::{Event, Message, Role, SessionId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

/// Default cap on concurrent local sub-agents (overridable via
/// `llm.max_local_agents`).
pub const DEFAULT_MAX_LOCAL_AGENTS: usize = 4;

/// A hook the gateway sets so that, when the local sub-agent cap is reached,
/// excess delegations run on an available grid peer instead of waiting. The
/// implementation lives in the binary (it owns the peer registry + grid secret);
/// blumi-core only calls it.
#[async_trait]
pub trait GridOverflow: Send + Sync {
    /// Try to run `prompt` (a delegation of `agent_type`) on a remote grid peer.
    /// `Some(output)` if a peer accepted and produced output; `None` if no peer
    /// is available (the caller then waits for a local slot).
    async fn try_remote(&self, agent_type: &str, prompt: &str) -> Option<String>;
}

static GRID_OVERFLOW: StdRwLock<Option<Arc<dyn GridOverflow>>> = StdRwLock::new(None);

/// Register the process-global grid-overflow hook (called by the gateway at
/// startup when the grid is enabled). Replaces any previous hook.
pub fn set_grid_overflow(hook: Arc<dyn GridOverflow>) {
    if let Ok(mut g) = GRID_OVERFLOW.write() {
        *g = Some(hook);
    }
}

/// The current grid-overflow hook, if one is registered.
fn grid_overflow() -> Option<Arc<dyn GridOverflow>> {
    GRID_OVERFLOW.read().ok().and_then(|g| g.clone())
}

/// A sub-agent template.
#[derive(Debug, Clone)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    /// Tool names this agent may use (`["*"]` = all). `delegate` is always
    /// excluded to prevent unbounded nesting.
    pub allowed_tools: Vec<String>,
    pub max_turns: u32,
}

impl AgentDef {
    fn new(
        name: &str,
        description: &str,
        system_prompt: &str,
        allowed_tools: &[&str],
        max_turns: u32,
    ) -> Self {
        AgentDef {
            name: name.to_string(),
            description: description.to_string(),
            system_prompt: system_prompt.to_string(),
            allowed_tools: allowed_tools.iter().map(|s| s.to_string()).collect(),
            max_turns,
        }
    }
}

const READ_TOOLS: [&str; 4] = ["FileRead", "Glob", "Grep", "ListDirectory"];

/// The built-in sub-agent roster.
pub fn builtin_agents() -> Vec<AgentDef> {
    vec![
        AgentDef::new(
            "general-purpose",
            "A capable general agent with the full toolset.",
            "You are a focused general-purpose sub-agent. Complete the delegated task and \
             report concise, useful results to the caller.",
            &["*"],
            100,
        ),
        AgentDef::new(
            "Explore",
            "Read-only investigation of the codebase.",
            "You are an exploration sub-agent. Investigate the codebase and report findings \
             concisely with file paths. You must NOT modify any files.",
            &READ_TOOLS,
            60,
        ),
        AgentDef::new(
            "Plan",
            "Read-only planning and design.",
            "You are a planning sub-agent. Produce a concise, actionable plan. Investigate as \
             needed but do NOT modify any files.",
            &READ_TOOLS,
            60,
        ),
        AgentDef::new(
            "Coder",
            "Implements changes with write + shell tools.",
            "You are a coding sub-agent. Implement the requested change, then report what you \
             changed (files + a short summary).",
            &[
                "FileRead",
                "FileWrite",
                "FileEdit",
                "Bash",
                "Glob",
                "Grep",
                "ListDirectory",
                "TodoWrite",
            ],
            120,
        ),
        AgentDef::new(
            "Verify",
            "Runs checks/tests and reports pass/fail.",
            "You are a verification sub-agent. Run the relevant checks or tests and report a \
             concise pass/fail with the key evidence.",
            &["FileRead", "Glob", "Grep", "Bash", "ListDirectory"],
            80,
        ),
    ]
}

/// Spawns sub-agents over a shared provider/registry/executor.
pub struct AgentSpawner {
    llm: Arc<dyn LlmClient>,
    registry: Arc<ToolRegistry>,
    perms: Arc<PermissionEngine>,
    executor: Arc<dyn Executor>,
    options: LlmOptions,
    context_size: u32,
    working_dir: PathBuf,
    agents: HashMap<String, AgentDef>,
    /// Monotonic id source for the "active agents" UI pane.
    next_agent_id: AtomicU64,
    /// Caps concurrent local sub-agents; excess overflow to the grid or wait.
    sem: Arc<Semaphore>,
}

impl AgentSpawner {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmClient>,
        registry: Arc<ToolRegistry>,
        perms: Arc<PermissionEngine>,
        executor: Arc<dyn Executor>,
        options: LlmOptions,
        context_size: u32,
        working_dir: PathBuf,
        agents: Vec<AgentDef>,
    ) -> Self {
        let agents = agents.into_iter().map(|a| (a.name.clone(), a)).collect();
        AgentSpawner {
            llm,
            registry,
            perms,
            executor,
            options,
            context_size,
            working_dir,
            agents,
            next_agent_id: AtomicU64::new(0),
            sem: Arc::new(Semaphore::new(DEFAULT_MAX_LOCAL_AGENTS)),
        }
    }

    /// Set the max concurrent local sub-agents (from `llm.max_local_agents`).
    /// Clamped to at least 1.
    pub fn with_max_local_agents(mut self, n: u32) -> Self {
        self.sem = Arc::new(Semaphore::new((n as usize).max(1)));
        self
    }

    /// Build the restricted child runner + run one delegation to completion,
    /// returning its final assistant output. Emits no lifecycle events — the
    /// caller ([`SubAgentSpawner::spawn`]) owns AgentStart/AgentDone.
    async fn run_local(
        &self,
        def: &AgentDef,
        prompt: &str,
        interactor: Interactor,
        ct: CancellationToken,
    ) -> String {
        // Restricted toolset; never include `delegate` (no nested sub-agents).
        let child_registry = Arc::new(self.registry.subset(&def.allowed_tools, &["delegate"]));
        let child = AgentTurnRunner::new(
            self.llm.clone(),
            child_registry,
            self.perms.clone(),
            self.executor.clone(),
            self.options.clone(),
            def.max_turns,
            self.context_size,
            def.system_prompt.clone(),
            self.working_dir.clone(),
        );
        let state = Arc::new(Mutex::new(SessionState::new(
            SessionId::new(),
            self.options.model.clone(),
        )));
        state
            .lock()
            .await
            .messages
            .push(Message::user(prompt.to_string()));
        // The child's own events are swallowed (kept out of the parent
        // transcript); approvals still reach the user via the parent interactor.
        let (qtx, _qrx) = tokio::sync::mpsc::unbounded_channel();
        let child_ctx = TurnContext {
            session_id: state.lock().await.id.clone(),
            events: EventEmitter::new(qtx),
            interactor,
        };
        child.run_turn(state.clone(), child_ctx, ct).await;
        let st = state.lock().await;
        st.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant && !m.text().trim().is_empty())
            .map(|m| m.text())
            .unwrap_or_else(|| "(sub-agent produced no output)".to_string())
    }
}

#[async_trait]
impl SubAgentSpawner for AgentSpawner {
    fn agent_types(&self) -> Vec<String> {
        let mut v: Vec<String> = self.agents.keys().cloned().collect();
        v.sort();
        v
    }

    async fn spawn(
        &self,
        agent_type: &str,
        prompt: &str,
        events: EventEmitter,
        interactor: Interactor,
        ct: CancellationToken,
    ) -> Result<String, ToolError> {
        let def = self.agents.get(agent_type).ok_or_else(|| {
            ToolError::InvalidInput(format!(
                "unknown agent type '{agent_type}' (available: {})",
                self.agent_types().join(", ")
            ))
        })?;

        let agent_id = format!(
            "a{}",
            self.next_agent_id.fetch_add(1, Ordering::Relaxed) + 1
        );

        // Take a local slot without blocking. If the cap is reached, prefer
        // running on a grid peer (when an overflow hook is set + a peer is free);
        // otherwise wait for a local slot (backpressure). One instance therefore
        // never runs more than `max_local_agents` sub-agents at once.
        match self.sem.clone().try_acquire_owned() {
            Ok(_permit) => {
                events.emit(Event::AgentStart {
                    id: agent_id.clone(),
                    agent_type: agent_type.to_string(),
                    task: summarize_task(prompt),
                });
                let output = self.run_local(def, prompt, interactor, ct).await;
                events.emit(Event::AgentDone {
                    id: agent_id,
                    agent_type: agent_type.to_string(),
                    ok: true,
                    summary: summarize_task(&output),
                });
                Ok(output)
            }
            Err(_) => {
                if let Some(hook) = grid_overflow() {
                    events.emit(Event::AgentStart {
                        id: agent_id.clone(),
                        agent_type: agent_type.to_string(),
                        task: summarize_task(prompt),
                    });
                    if let Some(output) = hook.try_remote(agent_type, prompt).await {
                        events.emit(Event::AgentDone {
                            id: agent_id,
                            agent_type: agent_type.to_string(),
                            ok: true,
                            summary: format!("⟶ remote · {}", summarize_task(&output)),
                        });
                        return Ok(output);
                    }
                    // No peer took it — fall back to a local slot (may block).
                    let _permit = self
                        .sem
                        .clone()
                        .acquire_owned()
                        .await
                        .expect("spawner semaphore is never closed");
                    let output = self.run_local(def, prompt, interactor, ct).await;
                    events.emit(Event::AgentDone {
                        id: agent_id,
                        agent_type: agent_type.to_string(),
                        ok: true,
                        summary: summarize_task(&output),
                    });
                    Ok(output)
                } else {
                    // No overflow hook: wait for a local slot, then run.
                    let _permit = self
                        .sem
                        .clone()
                        .acquire_owned()
                        .await
                        .expect("spawner semaphore is never closed");
                    events.emit(Event::AgentStart {
                        id: agent_id.clone(),
                        agent_type: agent_type.to_string(),
                        task: summarize_task(prompt),
                    });
                    let output = self.run_local(def, prompt, interactor, ct).await;
                    events.emit(Event::AgentDone {
                        id: agent_id,
                        agent_type: agent_type.to_string(),
                        ok: true,
                        summary: summarize_task(&output),
                    });
                    Ok(output)
                }
            }
        }
    }
}

/// First non-empty line of `s`, trimmed to a sensible label length.
fn summarize_task(s: &str) -> String {
    let line = s
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = line.trim_start_matches(['#', '-', '*', ' ']);
    if line.chars().count() > 80 {
        let cut: String = line.chars().take(79).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ExecError, LlmError};
    use crate::exec::{DirEntry, ExecOutput, ExecRequest};
    use crate::llm::ToolSpec;
    use blumi_config::PermissionConfig;
    use blumi_protocol::{FinishReason, StreamChunk};
    use futures::stream;
    use std::path::Path;

    #[test]
    fn builtin_roster_has_expected_agents() {
        let names: Vec<String> = builtin_agents().into_iter().map(|a| a.name).collect();
        for expected in ["general-purpose", "Explore", "Plan", "Coder", "Verify"] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn explore_is_read_only() {
        let explore = builtin_agents()
            .into_iter()
            .find(|a| a.name == "Explore")
            .unwrap();
        assert!(!explore
            .allowed_tools
            .iter()
            .any(|t| t == "FileWrite" || t == "Bash"));
    }

    #[test]
    fn local_agent_cap_sets_semaphore_capacity() {
        // Default cap.
        assert_eq!(
            test_spawner().sem.available_permits(),
            DEFAULT_MAX_LOCAL_AGENTS
        );
        // Configured cap.
        assert_eq!(
            test_spawner()
                .with_max_local_agents(2)
                .sem
                .available_permits(),
            2
        );
        // Zero is clamped to at least 1 so delegations can still run.
        assert_eq!(
            test_spawner()
                .with_max_local_agents(0)
                .sem
                .available_permits(),
            1
        );
    }

    /// A provider that returns one assistant line and stops (no tool calls).
    struct MockLlm;
    #[async_trait]
    impl LlmClient for MockLlm {
        async fn stream_chat(
            &self,
            _m: &[Message],
            _t: &[ToolSpec],
            _o: &LlmOptions,
            _ct: CancellationToken,
        ) -> Result<futures::stream::BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError>
        {
            let chunks = vec![
                Ok(StreamChunk::Text {
                    text: "done by sub-agent".into(),
                }),
                Ok(StreamChunk::Done {
                    reason: FinishReason::Stop,
                }),
            ];
            Ok(Box::pin(stream::iter(chunks)))
        }
    }

    /// An executor that does nothing (sub-agent test never runs tools).
    struct NoopExec(PathBuf);
    #[async_trait]
    impl Executor for NoopExec {
        async fn exec(
            &self,
            _r: ExecRequest,
            _ct: CancellationToken,
        ) -> Result<ExecOutput, ExecError> {
            Ok(ExecOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
            })
        }
        async fn read_file(&self, _p: &Path) -> Result<Vec<u8>, ExecError> {
            Ok(Vec::new())
        }
        async fn write_file(&self, _p: &Path, _c: &[u8]) -> Result<(), ExecError> {
            Ok(())
        }
        async fn exists(&self, _p: &Path) -> Result<bool, ExecError> {
            Ok(false)
        }
        async fn list_dir(&self, _p: &Path) -> Result<Vec<DirEntry>, ExecError> {
            Ok(Vec::new())
        }
        fn working_dir(&self) -> &Path {
            &self.0
        }
    }

    fn test_spawner() -> AgentSpawner {
        AgentSpawner::new(
            Arc::new(MockLlm),
            Arc::new(ToolRegistry::new()),
            Arc::new(PermissionEngine::new(PermissionConfig::default())),
            Arc::new(NoopExec(PathBuf::from("/tmp"))),
            LlmOptions::default(),
            8192,
            PathBuf::from("/tmp"),
            builtin_agents(),
        )
    }

    fn channels() -> (EventEmitter, Interactor) {
        let (etx, _erx) = tokio::sync::mpsc::unbounded_channel();
        let (itx, _irx) = tokio::sync::mpsc::unbounded_channel();
        (EventEmitter::new(etx), Interactor::new(itx))
    }

    #[tokio::test]
    async fn spawn_runs_child_and_returns_output() {
        let s = test_spawner();
        let (events, interactor) = channels();
        let out = s
            .spawn(
                "Explore",
                "find things",
                events,
                interactor,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(out, "done by sub-agent");
    }

    #[tokio::test]
    async fn unknown_agent_type_errors() {
        let s = test_spawner();
        let (events, interactor) = channels();
        let r = s
            .spawn("Nope", "x", events, interactor, CancellationToken::new())
            .await;
        assert!(matches!(r, Err(ToolError::InvalidInput(_))));
    }
}

//! Layered configuration for blumi.
//!
//! Precedence (low → high): built-in defaults < `~/.blumi/settings.json` <
//! `<project>/.blumi/settings.json` < `BLUMI_*` environment variables. Resolved
//! filesystem [`Paths`] are computed at load time, not read from files.

mod paths;
pub mod pricing;
mod provider;

pub use paths::Paths;
pub use provider::{default_providers, ProviderConfig, ProviderKind};

use figment::{
    providers::{Env, Format, Json, Serialized},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    // Boxed: figment::Error is ~200 bytes, which would bloat every Result.
    #[error("failed to load configuration: {0}")]
    Figment(Box<figment::Error>),
}

/// Sampler / model settings for the active provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Key into [`BlumiConfig::providers`], e.g. `"local"`, `"anthropic"`.
    pub provider: String,
    /// Model id for that provider (empty = let the provider pick / probe).
    pub model: String,
    pub context_size: u32,
    pub max_output_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    /// Iterations the agent loop may take in a single turn.
    pub max_iterations: u32,
    /// How many times the runtime may auto-continue a turn that stopped only
    /// because it hit `max_iterations` (it self-wakes in the same session, so
    /// no work/context is lost). 0 disables auto-continue. Bounds runaway work.
    pub max_auto_continue: u32,
    /// Token ceiling (billed input+output) for one self-woken sequence. Whichever
    /// of this or `max_auto_continue` is hit first stops the auto-continue — so
    /// runaway *spend* is bounded, not just the step count. 0 = no token cap.
    pub max_auto_continue_tokens: u32,
    /// When the context is compacted mid-turn (a rollover), refresh the
    /// auto-continue step + token budgets so a long task keeps going across the
    /// rollover instead of pausing at the budget. Default on.
    pub wake_on_rollover: bool,
    /// Max concurrent **local** sub-agents (delegations) this instance runs at
    /// once. Excess delegations either run on an available grid peer (when the
    /// grid is enabled) or wait for a local slot. Bounds local resource use.
    pub max_local_agents: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            provider: "local".into(),
            model: String::new(),
            context_size: 131_072,
            max_output_tokens: 16_384,
            temperature: 0.7,
            top_p: 0.8,
            top_k: 20,
            max_iterations: 25,
            max_auto_continue: 12,
            max_auto_continue_tokens: 400_000,
            wake_on_rollover: true,
            max_local_agents: 4,
        }
    }
}

/// Allow/Deny/Ask rule lists for one tool (patterns matched by the engine).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPermissionRules {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub ask: Vec<String>,
}

/// Permission policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionConfig {
    /// Per-tool rules, keyed by tool name.
    pub tools: BTreeMap<String, ToolPermissionRules>,
    /// Auto-approve everything (the TUI's YOLO mode default).
    pub yolo: bool,
}

fn default_true() -> bool {
    true
}

/// A configurable agent persona (keyed by name in [`BlumiConfig::personas`]).
/// Merged over the built-in roster, so this can override a built-in or add new.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonaConfig {
    pub description: String,
    /// Instructions appended to the base system prompt.
    pub instructions: String,
    /// Optional model id to switch to when this persona activates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional sampling temperature override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// Which execution backend tools run through, and its options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutorConfig {
    /// `"local"` (host), `"docker"` (container), or `"ssh"` (remote host).
    pub backend: String,
    /// Image used by the docker backend.
    pub docker_image: String,
    /// SSH destination (`user@host` or alias) for the ssh backend.
    pub ssh_host: String,
    /// Absolute remote working directory for the ssh backend.
    pub ssh_workdir: String,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        ExecutorConfig {
            backend: "local".into(),
            docker_image: "debian:stable-slim".into(),
            ssh_host: String::new(),
            ssh_workdir: String::new(),
        }
    }
}

/// Local-LLM "brain" that reviews tool approvals (claudectl-style). When `mode`
/// is `advisory` it annotates approval prompts with a recommendation; when
/// `auto` it resolves them directly (escalating only on uncertainty or danger).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrainConfig {
    /// `"off"` | `"advisory"` | `"auto"`.
    pub mode: String,
    /// Provider name (from [`BlumiConfig::providers`]) the brain judges with.
    /// Empty = reuse the main agent's provider/client. Use a cheap local model
    /// (e.g. `ollama`) here to keep judging fast and free.
    pub provider: String,
    /// Model id the brain judges with. Empty = reuse the main agent's model.
    pub model: String,
}

impl Default for BrainConfig {
    fn default() -> Self {
        BrainConfig {
            mode: "off".into(),
            provider: String::new(),
            model: String::new(),
        }
    }
}

/// A routing tier target: which provider + model a tier uses. Empty fields reuse
/// the active `llm.*` (so a tier can override just the model, or just the provider).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TierTarget {
    /// Provider name (key into [`BlumiConfig::providers`]). Empty = reuse active.
    pub provider: String,
    /// Model id. Empty = reuse the active model.
    pub model: String,
}

/// Tunable thresholds for the fast routing heuristic (no LLM call).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeuristicConfig {
    /// Prompt chars at/above which a turn is "heavy" outright.
    pub heavy_chars: u32,
    /// Prompt chars at/below which a turn is "light" outright.
    pub light_chars: u32,
    /// #tools available at/above which lean heavy (rich toolset ⇒ harder task).
    pub heavy_tool_count: u32,
    /// Iteration index at/above which a turn escalates to heavy (deep loops are
    /// where cheap models stall). 0 disables iteration-based escalation.
    pub escalate_iteration: u32,
    /// Substrings that force heavy (matched case-insensitively).
    pub heavy_keywords: Vec<String>,
    /// Substrings that force light (matched case-insensitively).
    pub light_keywords: Vec<String>,
}

impl Default for HeuristicConfig {
    fn default() -> Self {
        let to_vec = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        HeuristicConfig {
            heavy_chars: 1500,
            light_chars: 280,
            heavy_tool_count: 40,
            escalate_iteration: 6,
            heavy_keywords: to_vec(&[
                "refactor",
                "architect",
                "debug",
                "why",
                "design",
                "prove",
                "root cause",
                "migrate",
            ]),
            light_keywords: to_vec(&[
                "rename",
                "format",
                "typo",
                "list",
                "what is",
                "summarize",
                "translate",
                "lint",
            ]),
        }
    }
}

/// Cost-aware model routing. Per turn, a fast heuristic (and, on ambiguous turns,
/// a local "judge" model) picks a difficulty tier and routes to a light vs
/// flagship model; delegated sub-agents default to a cheaper tier. Every part
/// degrades gracefully — `mode = "off"` (the default) = today's behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RouterConfig {
    /// `"off"` | `"heuristic"` | `"hybrid"` | `"judge"`.
    pub mode: String,
    /// The light/cheap tier. Empty provider/model = reuse the active `llm.*`.
    pub light: TierTarget,
    /// The flagship/heavy tier. Empty provider/model = reuse the active `llm.*`.
    pub heavy: TierTarget,
    /// Tier delegated sub-agents default to: `"light"` | `"heavy"` | `"inherit"`.
    pub subagent_tier: String,
    /// Fast-path heuristic thresholds.
    pub heuristics: HeuristicConfig,
    /// The judge model for ambiguous turns (hybrid/judge modes). Empty
    /// provider/model = reuse `brain.*`, then the active `llm.*`.
    pub judge: TierTarget,
    /// Allow the light tier to target a grid peer's local model (grid synergy).
    pub prefer_grid_light: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        RouterConfig {
            mode: "off".into(),
            light: TierTarget::default(),
            heavy: TierTarget::default(),
            subagent_tier: "light".into(),
            heuristics: HeuristicConfig::default(),
            judge: TierTarget::default(),
            prefer_grid_light: false,
        }
    }
}

/// A remote blumi instance the TUI can attach to as a tab (ralph-tui style).
/// Each is a running `blumi web` server reachable over HTTP/SSE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RemoteInstance {
    /// Short label shown on the tab.
    pub name: String,
    /// Base URL of the remote `blumi web` server, e.g. `http://10.0.0.5:8080`.
    pub url: String,
    /// Password for the remote (if it has auth enabled). Blank = open instance.
    pub password: String,
}

/// Remote instances for the TUI's tab bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RemoteConfig {
    pub instances: Vec<RemoteInstance>,
}

/// Distributed-grid settings. Several `blumi serve` gateways that share one
/// `secret` form a grid; the orchestrator (the instance the phone connects to)
/// hands tasks off to peers for execution on their own runtimes. The secret
/// authenticates peer↔peer traffic — it is NEVER advertised over mDNS (only a
/// non-sensitive `grid_id` digest is) and is kept out of git (settings.json is
/// written 0600). It can also be supplied via `BLUMI_GRID__SECRET`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GridConfig {
    /// Master switch. Off = behave exactly as today (no browse, no /api/grid/*).
    pub enabled: bool,
    /// Shared secret. Same value on every node = same grid. Empty + enabled =
    /// refuse to form a grid (fail closed).
    pub secret: String,
    /// Public, non-sensitive grid identity advertised in mDNS TXT so peers only
    /// surface same-grid instances. Blank = derived from the secret digest.
    pub grid_id: String,
    /// Optional friendly label for this node in the peer list / UI. Blank =
    /// fall back to the machine hostname.
    pub node_name: String,
    /// Static peer addresses (`IP` or `IP:port`, default port 7777) seeded into
    /// the grid registry in addition to mDNS discovery. Lets the grid work when
    /// multicast/mDNS is blocked (e.g. macOS resets a re-signed binary's
    /// Local-Network permission). The shared secret still authenticates dispatch.
    pub peers: Vec<String>,
}

/// Local-embeddings settings — the backend for semantic memory + code search.
/// `backend = "local"` runs a bundled ONNX model (the `local-embeddings` build
/// feature); `"openai"` calls a configured OpenAI-compatible `/embeddings`
/// endpoint (e.g. Ollama / llama.cpp); `"grid"` offloads to a peer. On by
/// default with the bundled local model so semantic memory works fully offline
/// out of the box (first use downloads the ~90 MB model once, then it's cached).
/// Set `enabled = false` for a lean install — callers fall back to FTS5 search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    /// Master switch. Off = no embeddings (callers fall back to FTS5 search).
    pub enabled: bool,
    /// "local" (bundled ONNX), "openai" (a configured endpoint), or "grid".
    pub backend: String,
    /// For `backend = "openai"`: the providers-map key whose base_url/key to use.
    pub provider: String,
    /// Embedding model id (e.g. "bge-small-en-v1.5" or "nomic-embed-text").
    pub model: String,
    /// Expected vector dimensionality (sanity-checked against the client).
    pub dim: u32,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        EmbeddingConfig {
            enabled: true,
            backend: "local".into(),
            provider: String::new(),
            model: "bge-small-en-v1.5".into(),
            dim: 384,
        }
    }
}

/// GPU / accelerator settings. When a GPU is detected, the bundled ONNX embedder
/// runs on it — Apple CoreML/Metal on Apple Silicon (default builds), NVIDIA CUDA
/// with the opt-in `gpu-cuda` build — and falls back to CPU automatically (ort
/// appends a CPU provider). `mode = "auto"` picks the best available; force a
/// specific path with `"cpu"`, `"apple"`, or `"cuda"`. A requested provider that
/// wasn't compiled in degrades to CPU with a warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccelerationConfig {
    /// "auto" (detect), "cpu", "apple" (CoreML/Metal), or "cuda".
    pub mode: String,
    /// Override the execution provider for the bundled embedder specifically.
    /// "auto" follows `mode`; otherwise "cpu" / "apple" / "cuda".
    pub embeddings_accel: String,
}

impl Default for AccelerationConfig {
    fn default() -> Self {
        AccelerationConfig {
            mode: "auto".into(),
            embeddings_accel: "auto".into(),
        }
    }
}

/// Semantic long-term memory (the LangGraph "Store" + SEDM governance). Layered
/// on the embeddings backend above; when embeddings are disabled it degrades to
/// FTS5 keyword recall. Disable `enabled` to fall back to file-only MEMORY.md.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Master switch for the vector Store + per-turn RAG injection.
    pub enabled: bool,
    /// How many relevant memories to inject as background context per turn.
    pub recall_k: u32,
    /// Cosine similarity at/above which a new write MERGES into the existing
    /// memory (SEDM write-admission dedup) instead of inserting a duplicate.
    pub dedup_threshold: f32,
    /// Max active memories kept per namespace; lowest-utility are evicted past
    /// this (SEDM eviction). 0 = unbounded.
    pub max_per_namespace: u32,
    /// Diffuse high-utility, non-`user` memories to grid peers (SEDM cross-peer
    /// knowledge diffusion). The `user` namespace never diffuses (privacy).
    pub diffuse: bool,
    /// Seconds between background consolidation/eviction + diffusion sweeps.
    pub sweep_secs: u64,
    /// Opt-in: during the sweep, ask the LLM to classify same-topic memory pairs
    /// and supersede the outdated side (conflict resolution). Off by default — it
    /// adds bounded background LLM calls.
    #[serde(default)]
    pub resolve_conflicts: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig {
            enabled: true,
            recall_k: 5,
            dedup_threshold: 0.92,
            max_per_namespace: 2000,
            diffuse: true,
            sweep_secs: 60,
            resolve_conflicts: false,
        }
    }
}

/// Self-healing evolution autonomy (how the failure-pattern miner may act).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HealEvolve {
    /// Never mine failures or propose changes.
    Off,
    /// Mine failures + surface proposed changes for human approval only.
    Propose,
    /// Auto-apply LOW-RISK changes (e.g. a new recovery skill) with a notice;
    /// risky changes (config/providers/permissions/secrets/deletes) still need
    /// approval. This is the user-selected default.
    #[default]
    Auto,
}

/// Self-healing + self-evolving agent controls: the in-turn reflex recovery
/// layer (failure taxonomy → budgeted action → verify → trace), failure→fix
/// memory learning, and the evolution miner. Every part degrades gracefully —
/// disabled or absent subsystems just mean today's behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HealConfig {
    /// Master switch for in-turn recovery + learning + evolution.
    pub enabled: bool,
    /// Max budgeted recovery attempts per turn (0 = no auto-recovery).
    pub recovery_budget: u32,
    /// Confirm recoveries across steps: a guided recovery is stored as a
    /// *pending hypothesis* and only promoted to a recallable fix when the same
    /// tool succeeds on a later iteration — ground truth that the fix worked, not
    /// just that one was suggested. On by default so learned fixes reflect
    /// observed outcomes (the credit-assignment keystone). Free — no extra LLM.
    pub verify: bool,
    /// Write failure→fix episodes to memory + recall them on similar failures.
    pub learn: bool,
    /// Evolution autonomy: off | propose | auto.
    pub evolve: HealEvolve,
    /// Redact absolute paths / secret-looking tokens from stored failure text.
    pub redact_paths: bool,
}

impl Default for HealConfig {
    fn default() -> Self {
        HealConfig {
            enabled: true,
            recovery_budget: 2,
            verify: true,
            learn: true,
            evolve: HealEvolve::Auto,
            redact_paths: true,
        }
    }
}

/// Sandbox backend for the RPL Fever-Dream simulation (Phase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RplSandbox {
    /// Predict branch outcomes with sub-agents, no real execution (MVP, safe).
    #[default]
    Dry,
    /// Execute branches in throwaway git worktrees, then discard (V2).
    Worktree,
}

/// Raskolnikov's Psychological Loop (RPL-Judgement): an adversarial, regret-
/// minimizing pre-execution review. When a planned tool batch's blast radius
/// clears `blast_threshold`, the agent branches the plan, scores each branch's
/// paranoia, submits the best to an adversarial "Porfiry" judge, and only then
/// actuates — writing the predicted-vs-actual Error Delta back to memory.
/// Opt-in: it trades extra LLM calls for fewer catastrophic actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RplConfig {
    /// Master switch (off by default — RPL multiplies LLM calls).
    pub enabled: bool,
    /// Severity (0–100) a mutating batch must reach to trigger the full loop;
    /// below it, the cheap path runs as today.
    pub blast_threshold: u8,
    /// Tree-of-Thoughts branches simulated per review (clamped 1..=5 at use).
    pub branches: u8,
    /// Max Porfiry reject → re-plan rounds before falling back to escalation.
    pub max_defend_rounds: u8,
    /// Model for the Porfiry judge (empty = reuse the brain / main model).
    pub judge_model: String,
    /// Simulation backend (dry predictions vs. worktree execution).
    pub sandbox: RplSandbox,
}

impl Default for RplConfig {
    fn default() -> Self {
        RplConfig {
            enabled: false,
            blast_threshold: 40,
            branches: 3,
            max_defend_rounds: 2,
            judge_model: String::new(),
            sandbox: RplSandbox::Dry,
        }
    }
}

/// Always-on discovery autonomy (mirrors [`HealEvolve`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryAutonomy {
    /// Never run a discovery pass.
    #[default]
    Off,
    /// Discover candidate tasks → add them to the board (Todo) + write a report.
    Propose,
    /// Like `propose`, plus auto-run low-risk discoveries in isolation. (The
    /// auto-run path is a follow-up; today `auto` behaves like `propose`.)
    Auto,
}

/// Always-on proactive discovery: the gateway periodically (and only when gated
/// allows) asks the agent to surface candidate tasks for the workspace, adds them
/// to the board, and lands a markdown report. Off by default ⇒ zero behaviour
/// change. A sibling of the SEDM sweep + the autonomous loop, not a replacement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlwaysOnConfig {
    /// Master switch. `false` (default) ⇒ the scheduler is never even spawned.
    pub enabled: bool,
    /// How the pass may act: off | propose | auto.
    pub autonomy: DiscoveryAutonomy,
    /// Seconds between discovery passes (floored in code).
    pub cadence_secs: u64,
    /// Minimum seconds between two passes regardless of cadence (rate-limit).
    pub min_interval_secs: u64,
    /// Skip a pass while the board already has at least this many `todo` tasks
    /// (don't pile work on a busy board). 0 = never skip on that account.
    pub skip_if_todos: u32,
    /// Skip while at/above this many open (todo+doing) `Discovered:` tasks.
    pub max_open_discoveries: u32,
    /// Cap candidate tasks proposed per pass (the parser truncates).
    pub max_per_pass: u32,
    /// Override the discovery prompt (the `{signals}` placeholder is filled with
    /// board/git context). Empty = the built-in prompt.
    pub prompt_template: String,
    /// Also write each report into the workspace (`.blumi/reports/`), not just
    /// `~/.blumi/reports/`.
    pub report_in_workspace: bool,
}

impl Default for AlwaysOnConfig {
    fn default() -> Self {
        AlwaysOnConfig {
            enabled: false,
            autonomy: DiscoveryAutonomy::Off,
            cadence_secs: 900,
            min_interval_secs: 300,
            skip_if_todos: 1,
            max_open_discoveries: 5,
            max_per_pass: 3,
            prompt_template: String::new(),
            report_in_workspace: false,
        }
    }
}

/// Native-lite code knowledge base. When enabled, `blumi knowledge ingest` and
/// the `code_search` / `code_retrieve` tools index repos into `knowledge.db`
/// (FTS5 + embeddings when available). Disable to drop the code-KB tools.
/// How blumi builds the code reference graph (`knowledge.graph.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GraphMode {
    /// Build no graph.
    Off,
    /// Name-co-occurrence heuristic — the always-available default (Tier 0).
    #[default]
    Lite,
    /// Typed, scope-resolved structural graph (tree-sitter; needs the
    /// `code-graph` build feature, else falls back to `lite`). Tier 1.
    Structural,
}

/// Code reference-graph settings (under `knowledge.graph`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KnowledgeGraphConfig {
    /// How edges are built. Default `lite` (today's behavior).
    pub mode: GraphMode,
    /// Resolve `import`/`use` statements when building structural edges.
    pub resolve_imports: bool,
    /// Cap outgoing edges per symbol (noise control). 0 = uncapped.
    pub max_edges_per_symbol: u32,
    /// Feed a symbol's caller fan-in into the RPL blast radius.
    pub rpl_impact: bool,
}

impl Default for KnowledgeGraphConfig {
    fn default() -> Self {
        KnowledgeGraphConfig {
            mode: GraphMode::Lite,
            resolve_imports: true,
            max_edges_per_symbol: 64,
            rpl_impact: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KnowledgeConfig {
    /// Master switch for the code-KB tools + CLI.
    pub enabled: bool,
    /// Skip files larger than this (KiB) during ingest. 0 = no cap.
    pub max_file_kb: u64,
    /// Extra path substrings to exclude (beyond .gitignore + default noise dirs).
    pub exclude: Vec<String>,
    /// Code reference-graph settings (Tier-0 `lite` by default).
    pub graph: KnowledgeGraphConfig,
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        KnowledgeConfig {
            enabled: true,
            max_file_kb: 256,
            exclude: Vec::new(),
            graph: KnowledgeGraphConfig::default(),
        }
    }
}

/// Project-workspace discovery for the TUI's left sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// Parent directories scanned for git-repo projects. Empty = scan the parent
    /// of the current project so sibling repos show up automatically.
    pub roots: Vec<String>,
}

/// Git identity stamped on commits the agent makes (via `git`/`gh`). Applied as
/// `GIT_AUTHOR_*`/`GIT_COMMITTER_*` on every command, overriding repo/host
/// config. Empty `author_name` disables the override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    pub author_name: String,
    pub author_email: String,
}

impl Default for GitConfig {
    fn default() -> Self {
        GitConfig {
            author_name: "Blumi".into(),
            author_email: "ankur.nairit@gmail.com".into(),
        }
    }
}

/// Web UI / server settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WebSettings {
    /// argon2 password hash. When set (or when binding to a non-loopback
    /// address), the web UI requires login. Set it with `blumi web --password`.
    pub password_hash: String,
}

/// Voice (speech-to-text + text-to-speech) over OpenAI-compatible audio APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct VoiceSettings {
    /// Master switch for voice features in the web UI / gateways.
    pub enabled: bool,
    /// API key for the audio endpoints (blank for local servers).
    pub api_key: String,
    /// Transcription base URL, e.g. `https://api.openai.com/v1`.
    pub stt_base_url: String,
    /// Transcription model, e.g. `whisper-1`.
    pub stt_model: String,
    /// TTS provider: `"openai"` (default) or `"elevenlabs"`.
    pub tts_provider: String,
    /// Speech base URL (provider default used when blank).
    pub tts_base_url: String,
    /// Speech model, e.g. `tts-1` (OpenAI) or `eleven_multilingual_v2`.
    pub tts_model: String,
    /// Voice name (OpenAI, e.g. `alloy`) or voice id (ElevenLabs).
    pub tts_voice: String,
    /// Separate key for TTS (falls back to `api_key`) — e.g. an ElevenLabs key.
    pub tts_api_key: String,
}

/// Messaging-gateway settings (run blumi as a bot). Tokens may also be passed
/// on the command line, which override these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GatewayConfig {
    /// Auto-approve tool calls in gateway sessions. Off by default: a bot has no
    /// human to approve, so write/exec tools are denied unless this is on. Turn
    /// it on only with a sandboxed executor (e.g. docker).
    pub yolo: bool,
    pub telegram: TelegramGatewayConfig,
    pub discord: DiscordGatewayConfig,
    pub slack: SlackGatewayConfig,
    pub whatsapp: WhatsappGatewayConfig,
}

/// Telegram bot settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TelegramGatewayConfig {
    /// Bot token from @BotFather.
    pub token: String,
    /// If non-empty, only these chat ids are served (an allow-list).
    pub allowed_chats: Vec<i64>,
    /// Handle voice: transcribe inbound voice notes and speak replies (TTS).
    /// **Off by default** — also requires global `voice.*` to be configured.
    #[serde(default)]
    pub voice: bool,
}

/// Discord bot settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DiscordGatewayConfig {
    /// Bot token from the Discord developer portal.
    pub token: String,
    /// If non-empty, only these channel ids are served.
    pub allowed_channels: Vec<String>,
}

/// Slack bot settings (Socket Mode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SlackGatewayConfig {
    /// Bot token (`xoxb-…`).
    pub bot_token: String,
    /// App-level token (`xapp-…`) for Socket Mode.
    pub app_token: String,
}

/// WhatsApp Cloud API settings (webhook-based).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WhatsappGatewayConfig {
    /// Permanent access token for the WhatsApp Cloud API.
    pub token: String,
    /// Phone-number id to send from.
    pub phone_number_id: String,
    /// Token used to verify the webhook subscription (you choose this).
    pub verify_token: String,
    /// Port the inbound webhook server listens on.
    pub webhook_port: u16,
}

/// A language server for code-intelligence, keyed by name in
/// [`BlumiConfig::lsp_servers`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Executable to spawn, e.g. `rust-analyzer`.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions (without the dot) this server handles, e.g. `["rs"]`.
    pub extensions: Vec<String>,
    /// LSP languageId; defaults to the first extension if empty.
    #[serde(default)]
    pub language_id: String,
}

/// An external MCP (Model Context Protocol) server launched over stdio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Executable to spawn, e.g. `npx`.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl McpServerConfig {
    fn npx(pkg: &str, extra: &[&str], enabled: bool) -> Self {
        let mut args = vec!["-y".to_string(), pkg.to_string()];
        args.extend(extra.iter().map(|s| s.to_string()));
        McpServerConfig {
            command: "npx".into(),
            args,
            env: BTreeMap::new(),
            enabled,
        }
    }
    fn uvx(pkg: &str, extra: &[&str], enabled: bool) -> Self {
        let mut args = vec![pkg.to_string()];
        args.extend(extra.iter().map(|s| s.to_string()));
        McpServerConfig {
            command: "uvx".into(),
            args,
            env: BTreeMap::new(),
            enabled,
        }
    }
    fn with_env(mut self, kvs: &[(&str, &str)]) -> Self {
        for (k, v) in kvs {
            self.env.insert((*k).to_string(), (*v).to_string());
        }
        self
    }
}

/// The default no-config MCP servers, seeded into `settings.json` on first run.
/// `{workspace}` in args is replaced with the session's working directory at
/// connect time (see the engine). These need Node (`npx`) and/or `uv` (`uvx`)
/// at runtime; a missing runtime just makes the server skip with a log line.
pub fn default_mcp_servers() -> BTreeMap<String, McpServerConfig> {
    use McpServerConfig as M;
    BTreeMap::from([
        (
            "filesystem".into(),
            M::npx(
                "@modelcontextprotocol/server-filesystem",
                &["{workspace}"],
                true,
            ),
        ),
        (
            "memory".into(),
            M::npx("@modelcontextprotocol/server-memory", &[], true),
        ),
        ("fetch".into(), M::uvx("mcp-server-fetch", &[], true)),
        (
            "git".into(),
            M::uvx("mcp-server-git", &["--repository", "{workspace}"], true),
        ),
        (
            "sequentialthinking".into(),
            M::npx(
                "@modelcontextprotocol/server-sequentialthinking",
                &[],
                false,
            ),
        ),
        ("time".into(), M::uvx("mcp-server-time", &[], false)),
    ])
}

/// One configurable (keyed) server the user can add with `blumi mcp add`.
#[derive(Debug, Clone)]
pub struct McpCatalogEntry {
    pub name: String,
    pub description: String,
    /// Env vars the user must fill (keys present, values blank placeholders).
    pub required_env: Vec<String>,
    pub server: McpServerConfig,
}

/// A curated catalog of popular MCP servers that need configuration (API keys),
/// for `blumi mcp catalog` / `blumi mcp add` (sourced from awesome-mcp-servers).
/// Added disabled with blank env placeholders the user fills in.
pub fn default_mcp_catalog() -> Vec<McpCatalogEntry> {
    use McpServerConfig as M;
    fn e(name: &str, description: &str, env: &[&str], server: McpServerConfig) -> McpCatalogEntry {
        McpCatalogEntry {
            name: name.into(),
            description: description.into(),
            required_env: env.iter().map(|s| s.to_string()).collect(),
            server,
        }
    }
    vec![
        e(
            "github",
            "GitHub repos, issues, and PRs",
            &["GITHUB_PERSONAL_ACCESS_TOKEN"],
            M::npx("@modelcontextprotocol/server-github", &[], false)
                .with_env(&[("GITHUB_PERSONAL_ACCESS_TOKEN", "")]),
        ),
        e(
            "brave-search",
            "Web search via the Brave Search API",
            &["BRAVE_API_KEY"],
            M::npx("@modelcontextprotocol/server-brave-search", &[], false)
                .with_env(&[("BRAVE_API_KEY", "")]),
        ),
        e(
            "slack",
            "Read/post Slack messages",
            &["SLACK_BOT_TOKEN", "SLACK_TEAM_ID"],
            M::npx("@modelcontextprotocol/server-slack", &[], false)
                .with_env(&[("SLACK_BOT_TOKEN", ""), ("SLACK_TEAM_ID", "")]),
        ),
        e(
            "gitlab",
            "GitLab projects, issues, and MRs",
            &["GITLAB_PERSONAL_ACCESS_TOKEN"],
            M::npx("@modelcontextprotocol/server-gitlab", &[], false)
                .with_env(&[("GITLAB_PERSONAL_ACCESS_TOKEN", "")]),
        ),
        e(
            "postgres",
            "Read-only Postgres queries (set the connection URL in args)",
            &[],
            M::npx(
                "@modelcontextprotocol/server-postgres",
                &["postgresql://localhost/mydb"],
                false,
            ),
        ),
        e(
            "gdrive",
            "Search + read Google Drive files (OAuth setup required)",
            &["GDRIVE_CREDENTIALS_PATH"],
            M::npx("@modelcontextprotocol/server-gdrive", &[], false)
                .with_env(&[("GDRIVE_CREDENTIALS_PATH", "")]),
        ),
        e(
            "sentry",
            "Retrieve and analyze Sentry issues",
            &["SENTRY_AUTH_TOKEN"],
            M::uvx("mcp-server-sentry", &[], false).with_env(&[("SENTRY_AUTH_TOKEN", "")]),
        ),
        e(
            "puppeteer",
            "Headless browser automation + screenshots",
            &[],
            M::npx("@modelcontextprotocol/server-puppeteer", &[], false),
        ),
    ]
}

/// One lifecycle hook: a shell command, optionally gated by a `matcher` (for
/// tool events). The command runs in the workspace with the triggering payload
/// (e.g. the user's prompt) piped to its stdin.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HookDef {
    /// Shell command (`sh -c <command>`). Empty = ignored.
    pub command: String,
    /// For tool events, only fire when the tool name contains this (empty = any).
    pub matcher: String,
    /// Seconds before the hook is abandoned (0 = a sane default).
    pub timeout_secs: u64,
}

/// Lifecycle hooks (Claude-Code-style extension points). v1 implements
/// `user_prompt_submit` — each command's stdout is injected as background context
/// for the turn (cache-safe trailing message). `pre_tool_use` is reserved: the
/// tool-blocking path lands in a later, security-reviewed pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HooksConfig {
    /// Run when the user submits a prompt; stdout becomes background context.
    pub user_prompt_submit: Vec<HookDef>,
    /// Reserved — run before a tool call (can block). Not yet wired.
    pub pre_tool_use: Vec<HookDef>,
}

/// Proactive completion notifications (off by default). When `enabled`, blumi
/// fans out a short message when an autonomous run finishes (`blumi loop` or
/// always-on discovery) to whichever channels you turn on: an OS desktop
/// notification, a gateway-bot message, and/or browser Web Push. The browser
/// in-tab alert and the blugo phone notification ride the live event stream and
/// don't need this config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifyConfig {
    /// Master switch. Off ⇒ no proactive notifications (the `blumi loop --notify`
    /// flag still fires a one-off desktop notification for that run).
    pub enabled: bool,
    /// Which completion events fire: any of `loop`, `discovery`, `turn`. Empty ⇒
    /// the default set (`loop` + `discovery`).
    pub on: Vec<String>,
    /// Post an OS desktop notification (macOS `osascript` / Linux `notify-send`).
    pub desktop: bool,
    /// Proactively message a gateway bot (Telegram / Discord / Slack / WhatsApp).
    pub bot: Option<NotifyBot>,
    /// Push to subscribed browsers via Web Push (VAPID). Only reaches browsers
    /// served over a secure context (HTTPS or `http://localhost`).
    pub web_push: bool,
    /// FCM push to the blugo phone app. Enabled automatically when the service
    /// account file exists (`~/.blumi/fcm-service-account.json`) — no flag needed;
    /// this struct only carries optional overrides.
    #[serde(default)]
    pub fcm: FcmConfig,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            on: Vec::new(),
            desktop: true,
            bot: None,
            web_push: false,
            fcm: FcmConfig::default(),
        }
    }
}

/// FCM (Firebase Cloud Messaging) push for the blugo phone app. Push is gated on
/// the **presence** of the service-account file, so this is on-by-default with no
/// settings; these fields are optional overrides only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FcmConfig {
    /// GCP project id. Empty ⇒ read `project_id` from the service-account JSON.
    pub project_id: String,
    /// Path override for the service account. Empty ⇒ `paths.fcm_service_account()`
    /// (`~/.blumi/fcm-service-account.json`).
    pub service_account_path: String,
}

impl NotifyConfig {
    /// Does trigger `kind` fire? Empty `on` ⇒ the default `loop` + `discovery` set.
    pub fn fires(&self, kind: &str) -> bool {
        if self.on.is_empty() {
            matches!(kind, "loop" | "discovery")
        } else {
            self.on.iter().any(|k| k == kind)
        }
    }
}

/// A gateway-bot target for completion notifications. Credentials come from the
/// matching `gateway.*` config; this just picks the transport and destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NotifyBot {
    /// `telegram` | `discord` | `slack` | `whatsapp`.
    pub transport: String,
    /// Telegram chat id, Discord channel id, Slack channel, or WhatsApp recipient
    /// phone (E.164).
    pub target: String,
}

/// The full blumi configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlumiConfig {
    pub llm: LlmConfig,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub permissions: PermissionConfig,
    /// External MCP servers to launch and expose as tools, keyed by name.
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    /// Enable sending images to vision-capable models.
    pub vision_enabled: bool,
    pub verbose: bool,
    /// Active agent persona name (must exist in built-ins or [`Self::personas`]).
    pub persona: String,
    /// Configured personas, merged over the built-in roster, keyed by name.
    #[serde(default)]
    pub personas: BTreeMap<String, PersonaConfig>,
    /// Execution backend (host vs docker sandbox).
    pub executor: ExecutorConfig,
    /// Language servers for the `Lsp` code-intel tool, keyed by name.
    #[serde(default)]
    pub lsp_servers: BTreeMap<String, LspServerConfig>,
    /// Messaging gateways (Telegram/Discord/Slack/WhatsApp bots).
    #[serde(default)]
    pub gateway: GatewayConfig,
    /// Web UI / server settings (auth).
    #[serde(default)]
    pub web: WebSettings,
    /// Voice (speech-to-text + text-to-speech).
    #[serde(default)]
    pub voice: VoiceSettings,
    /// Local-LLM "brain" that reviews tool approvals.
    #[serde(default)]
    pub brain: BrainConfig,
    /// Cost-aware model routing (heuristic + optional local judge).
    #[serde(default)]
    pub router: RouterConfig,
    /// Remote blumi instances the TUI can attach to as tabs.
    #[serde(default)]
    pub remote: RemoteConfig,
    /// Distributed-grid settings (peer discovery + task hand-off).
    #[serde(default)]
    pub grid: GridConfig,
    /// Local-embeddings settings (semantic memory + code search backend).
    #[serde(default)]
    pub embeddings: EmbeddingConfig,
    /// GPU / accelerator settings (embedder execution provider; auto-detect).
    #[serde(default)]
    pub acceleration: AccelerationConfig,
    /// Semantic long-term memory (vector Store + RAG + SEDM governance).
    #[serde(default)]
    pub memory: MemoryConfig,
    /// Self-healing + self-evolving agent controls (reflex recovery + learning).
    #[serde(default)]
    pub heal: HealConfig,
    /// Raskolnikov's Psychological Loop: adversarial pre-execution review of
    /// high-blast actions (off by default — opt-in, costs extra LLM calls).
    #[serde(default)]
    pub rpl: RplConfig,
    /// Always-on proactive discovery (off by default).
    #[serde(default)]
    pub always_on: AlwaysOnConfig,
    /// Lifecycle hooks (UserPromptSubmit context injection; off by default).
    #[serde(default)]
    pub hooks: HooksConfig,
    /// Proactive completion notifications (desktop / bot / web push; off by default).
    #[serde(default)]
    pub notify: NotifyConfig,
    /// Native-lite code knowledge base (code_search / code_retrieve + CLI).
    #[serde(default)]
    pub knowledge: KnowledgeConfig,
    /// Project-workspace discovery for the TUI sidebar.
    #[serde(default)]
    pub workspaces: WorkspaceConfig,
    /// Git identity for commits the agent makes.
    #[serde(default)]
    pub git: GitConfig,
    /// Resolved at load time; never serialized to/from files.
    #[serde(skip)]
    pub paths: Paths,
}

impl Default for BlumiConfig {
    fn default() -> Self {
        BlumiConfig {
            llm: LlmConfig::default(),
            providers: default_providers(),
            permissions: PermissionConfig::default(),
            mcp_servers: BTreeMap::new(),
            vision_enabled: false,
            verbose: false,
            persona: "default".into(),
            personas: BTreeMap::new(),
            executor: ExecutorConfig::default(),
            lsp_servers: BTreeMap::new(),
            gateway: GatewayConfig::default(),
            web: WebSettings::default(),
            voice: VoiceSettings::default(),
            brain: BrainConfig::default(),
            router: RouterConfig::default(),
            remote: RemoteConfig::default(),
            grid: GridConfig::default(),
            embeddings: EmbeddingConfig::default(),
            acceleration: AccelerationConfig::default(),
            memory: MemoryConfig::default(),
            heal: HealConfig::default(),
            rpl: RplConfig::default(),
            always_on: AlwaysOnConfig::default(),
            hooks: HooksConfig::default(),
            notify: NotifyConfig::default(),
            knowledge: KnowledgeConfig::default(),
            workspaces: WorkspaceConfig::default(),
            git: GitConfig::default(),
            paths: Paths::default(),
        }
    }
}

impl BlumiConfig {
    /// Load config with the standard layering, resolving paths against
    /// `working_dir`. `home_override` (e.g. from `BLUMI_HOME`) overrides `~/.blumi`.
    pub fn load(
        working_dir: impl AsRef<Path>,
        home_override: Option<PathBuf>,
    ) -> Result<Self, ConfigError> {
        let working_dir = working_dir.as_ref();
        let home = home_override
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".blumi")));

        let mut fig = Figment::from(Serialized::defaults(BlumiConfig::default()));
        if let Some(home) = &home {
            fig = fig.merge(Json::file(home.join("settings.json")));
        }
        fig = fig
            .merge(Json::file(working_dir.join(".blumi").join("settings.json")))
            .merge(Env::prefixed("BLUMI_").split("__"));

        let mut cfg: BlumiConfig = fig
            .extract()
            .map_err(|e| ConfigError::Figment(Box::new(e)))?;
        cfg.paths = Paths::resolve(home_override, working_dir);
        Ok(cfg)
    }

    /// The config for the currently-selected provider, if present.
    pub fn active_provider(&self) -> Option<&ProviderConfig> {
        self.providers.get(&self.llm.provider)
    }

    /// True when no settings file exists yet — treat as first run (→ onboarding).
    pub fn is_first_run(&self) -> bool {
        !self.paths.settings_json().exists()
    }
}

/// Persist an onboarding choice into `settings.json`, merging with any existing
/// content: sets the active provider + model, and (when given) the provider's
/// `base_url` and `api_key`. Written `0600` on unix.
pub fn write_provider_setup(
    paths: &Paths,
    provider: &str,
    kind: ProviderKind,
    model: &str,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> std::io::Result<()> {
    use std::io::Write;

    let path = paths.settings_json();
    let mut root: serde_json::Value = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    root["llm"]["provider"] = serde_json::json!(provider);
    root["llm"]["model"] = serde_json::json!(model);
    // Write kind explicitly so the entry is self-contained (independent of
    // whether the loader deep-merges with the built-in preset).
    root["providers"][provider]["kind"] =
        serde_json::to_value(kind).unwrap_or(serde_json::Value::Null);
    if let Some(b) = base_url {
        root["providers"][provider]["base_url"] = serde_json::json!(b);
    }
    if let Some(k) = api_key {
        root["providers"][provider]["api_key"] = serde_json::json!(k);
    }

    std::fs::create_dir_all(&paths.home)?;
    let body = serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string());
    let mut file = std::fs::File::create(&path)?;
    file.write_all(body.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use figment::Jail;

    #[test]
    fn defaults_are_local_first() {
        let cfg = BlumiConfig::default();
        assert_eq!(cfg.llm.provider, "local");
        assert_eq!(cfg.llm.max_iterations, 25);
        assert_eq!(
            cfg.active_provider().unwrap().kind,
            ProviderKind::OpenaiCompat
        );
    }

    #[test]
    #[allow(clippy::result_large_err)] // figment's Jail closure returns figment::Error
    fn project_settings_override_and_env_wins() {
        Jail::expect_with(|jail| {
            jail.create_dir(".blumi")?;
            jail.create_file(
                ".blumi/settings.json",
                r#"{ "llm": { "provider": "anthropic", "model": "claude-x" } }"#,
            )?;
            let cfg = BlumiConfig::load(jail.directory(), Some(jail.directory().join("home")))
                .expect("load");
            assert_eq!(cfg.llm.provider, "anthropic");
            assert_eq!(cfg.llm.model, "claude-x");
            // presets still present after override
            assert!(cfg.providers.contains_key("local"));

            // env overrides file
            jail.set_env("BLUMI_LLM__MODEL", "claude-y");
            let cfg2 = BlumiConfig::load(jail.directory(), Some(jail.directory().join("home")))
                .expect("load");
            assert_eq!(cfg2.llm.model, "claude-y");
            Ok(())
        });
    }

    #[test]
    #[allow(clippy::result_large_err)]
    fn onboarding_write_and_reload() {
        Jail::expect_with(|jail| {
            let home = jail.directory().join("home");
            let paths = Paths::resolve(Some(home.clone()), jail.directory());
            assert!(!paths.settings_json().exists());

            write_provider_setup(
                &paths,
                "azure-foundry",
                ProviderKind::AnthropicFoundry,
                "claude-sonnet",
                Some("https://r.services.ai.azure.com"),
                Some("azkey"),
            )
            .unwrap();

            let cfg = BlumiConfig::load(jail.directory(), Some(home)).unwrap();
            assert!(!cfg.is_first_run());
            assert_eq!(cfg.llm.provider, "azure-foundry");
            assert_eq!(cfg.llm.model, "claude-sonnet");
            let p = cfg.active_provider().unwrap();
            assert_eq!(p.kind, ProviderKind::AnthropicFoundry);
            assert_eq!(
                p.base_url.as_deref(),
                Some("https://r.services.ai.azure.com")
            );
            assert_eq!(p.resolve_api_key().as_deref(), Some("azkey"));
            Ok(())
        });
    }

    #[test]
    #[allow(clippy::result_large_err)]
    fn parses_mcp_servers() {
        Jail::expect_with(|jail| {
            jail.create_dir(".blumi")?;
            jail.create_file(
                ".blumi/settings.json",
                r#"{ "mcp_servers": { "fs": { "command": "npx",
                   "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"] } } }"#,
            )?;
            let cfg = BlumiConfig::load(jail.directory(), Some(jail.directory().join("home")))
                .expect("load");
            let fs = cfg.mcp_servers.get("fs").expect("fs server");
            assert_eq!(fs.command, "npx");
            assert_eq!(fs.args.len(), 3);
            assert!(fs.enabled); // default
            Ok(())
        });
    }

    #[test]
    fn paths_resolve_relative_to_args() {
        let cfg = BlumiConfig::load("/work/proj", Some(PathBuf::from("/data/blumi"))).unwrap();
        assert_eq!(cfg.paths.home, PathBuf::from("/data/blumi"));
        assert_eq!(cfg.paths.working_dir, PathBuf::from("/work/proj"));
    }
}

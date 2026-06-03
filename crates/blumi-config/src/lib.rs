//! Layered configuration for blumi.
//!
//! Precedence (low → high): built-in defaults < `~/.blumi/settings.json` <
//! `<project>/.blumi/settings.json` < `BLUMI_*` environment variables. Resolved
//! filesystem [`Paths`] are computed at load time, not read from files.

mod paths;
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

/// Project-workspace discovery for the TUI's left sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// Parent directories scanned for git-repo projects. Empty = scan the parent
    /// of the current project so sibling repos show up automatically.
    pub roots: Vec<String>,
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
    /// Remote blumi instances the TUI can attach to as tabs.
    #[serde(default)]
    pub remote: RemoteConfig,
    /// Project-workspace discovery for the TUI sidebar.
    #[serde(default)]
    pub workspaces: WorkspaceConfig,
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
            remote: RemoteConfig::default(),
            workspaces: WorkspaceConfig::default(),
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

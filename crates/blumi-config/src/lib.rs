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

/// The full blumi configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlumiConfig {
    pub llm: LlmConfig,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub permissions: PermissionConfig,
    /// Enable sending images to vision-capable models.
    pub vision_enabled: bool,
    pub verbose: bool,
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
            vision_enabled: false,
            verbose: false,
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
    fn paths_resolve_relative_to_args() {
        let cfg = BlumiConfig::load("/work/proj", Some(PathBuf::from("/data/blumi"))).unwrap();
        assert_eq!(cfg.paths.home, PathBuf::from("/data/blumi"));
        assert_eq!(cfg.paths.working_dir, PathBuf::from("/work/proj"));
    }
}

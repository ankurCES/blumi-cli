//! Provider configuration and the built-in preset catalog.
//!
//! A single OpenAI-compatible client covers most providers via `base_url`;
//! Anthropic and Gemini have native clients. The catalog ships presets so the
//! common providers work with just an API key in the environment.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which client implementation talks to a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI-compatible `/chat/completions` (OpenAI, OpenRouter, DeepSeek,
    /// Ollama, llama.cpp, MiniMax, Nous, Kimi, NIM, HF, custom).
    OpenaiCompat,
    /// Native Anthropic `/v1/messages`.
    Anthropic,
    /// Native Google Gemini.
    Gemini,
}

/// Configuration for one named provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    /// Base URL (including any `/v1`). Optional for providers with a fixed host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// API key, if set directly in config (discouraged; prefer `api_key_env`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Environment variable to read the API key from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

impl ProviderConfig {
    /// The effective API key: explicit `api_key`, else the value of
    /// `api_key_env` from the environment, else `None`.
    pub fn resolve_api_key(&self) -> Option<String> {
        if let Some(k) = &self.api_key {
            return Some(k.clone());
        }
        self.api_key_env
            .as_ref()
            .and_then(|var| std::env::var(var).ok())
    }

    fn openai_compat(base_url: &str, key_env: Option<&str>) -> Self {
        ProviderConfig {
            kind: ProviderKind::OpenaiCompat,
            base_url: Some(base_url.to_string()),
            api_key: None,
            api_key_env: key_env.map(str::to_string),
        }
    }
}

/// The built-in provider presets. Merged under any user-supplied providers.
pub fn default_providers() -> BTreeMap<String, ProviderConfig> {
    use ProviderConfig as P;
    BTreeMap::from([
        // Local-first defaults (no key needed).
        (
            "local".into(),
            P::openai_compat("http://localhost:7474/v1", None),
        ),
        (
            "ollama".into(),
            P::openai_compat("http://localhost:11434/v1", None),
        ),
        // Hosted, OpenAI-compatible.
        (
            "openai".into(),
            P::openai_compat("https://api.openai.com/v1", Some("OPENAI_API_KEY")),
        ),
        (
            "openrouter".into(),
            P::openai_compat("https://openrouter.ai/api/v1", Some("OPENROUTER_API_KEY")),
        ),
        (
            "deepseek".into(),
            P::openai_compat("https://api.deepseek.com/v1", Some("DEEPSEEK_API_KEY")),
        ),
        (
            "minimax".into(),
            P::openai_compat("https://api.minimaxi.chat/v1", Some("MINIMAX_API_KEY")),
        ),
        (
            "groq".into(),
            P::openai_compat("https://api.groq.com/openai/v1", Some("GROQ_API_KEY")),
        ),
        // Native clients.
        (
            "anthropic".into(),
            P {
                kind: ProviderKind::Anthropic,
                base_url: Some("https://api.anthropic.com".into()),
                api_key: None,
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
            },
        ),
        (
            "gemini".into(),
            P {
                kind: ProviderKind::Gemini,
                base_url: Some("https://generativelanguage.googleapis.com".into()),
                api_key: None,
                api_key_env: Some("GEMINI_API_KEY".into()),
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_local_and_native() {
        let p = default_providers();
        assert_eq!(p["local"].kind, ProviderKind::OpenaiCompat);
        assert_eq!(p["anthropic"].kind, ProviderKind::Anthropic);
        assert_eq!(p["gemini"].kind, ProviderKind::Gemini);
        assert!(p["local"].api_key_env.is_none());
    }

    #[test]
    fn resolve_api_key_prefers_explicit() {
        let mut c = ProviderConfig::openai_compat("http://x/v1", Some("DEFINITELY_UNSET_VAR_XYZ"));
        assert_eq!(c.resolve_api_key(), None);
        c.api_key = Some("sk-123".into());
        assert_eq!(c.resolve_api_key().as_deref(), Some("sk-123"));
    }
}

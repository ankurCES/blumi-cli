//! Build a concrete [`LlmClient`] from a [`ProviderConfig`].

use crate::{AnthropicClient, OpenAiCompatClient};
use blumi_config::{ProviderConfig, ProviderKind};
use blumi_core::{LlmClient, LlmError};
use std::sync::Arc;

/// Construct the right client for a provider config, resolving its API key from
/// the environment as needed.
pub fn build_client(provider: &ProviderConfig) -> Result<Arc<dyn LlmClient>, LlmError> {
    let base_url = provider
        .base_url
        .clone()
        .ok_or_else(|| LlmError::Other(anyhow::anyhow!("provider has no base_url")))?;
    let api_key = provider.resolve_api_key();

    match provider.kind {
        ProviderKind::OpenaiCompat => Ok(Arc::new(OpenAiCompatClient::new(base_url, api_key))),
        ProviderKind::Anthropic => Ok(Arc::new(AnthropicClient::new(base_url, api_key))),
        ProviderKind::Gemini => Err(LlmError::Other(anyhow::anyhow!(
            "the Gemini client lands in Phase 3"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_openai_compat() {
        let cfg = ProviderConfig {
            kind: ProviderKind::OpenaiCompat,
            base_url: Some("http://localhost:7474/v1".into()),
            api_key: None,
            api_key_env: None,
        };
        assert!(build_client(&cfg).is_ok());
    }

    #[test]
    fn gemini_not_yet_supported() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Gemini,
            base_url: Some("https://x".into()),
            api_key: None,
            api_key_env: None,
        };
        assert!(build_client(&cfg).is_err());
    }
}

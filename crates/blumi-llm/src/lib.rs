//! LLM provider clients.
//!
//! A single [`OpenAiCompatClient`] covers most providers via `base_url`;
//! [`AnthropicClient`] is native. [`build_client`] picks the right one from a
//! provider config; [`MockLlmClient`] scripts responses for tests/offline use.
//! [`build_embeddings_client`] builds the embeddings backend (a bundled local
//! ONNX model, an OpenAI-compatible endpoint, or — later — a grid peer).

mod anthropic;
mod embeddings_openai;
mod gemini;
mod mock;
mod openai;
mod registry;
mod retry;

#[cfg(feature = "local-embeddings")]
mod embeddings_local;

pub use anthropic::AnthropicClient;
pub use embeddings_openai::OpenAiEmbeddingClient;
pub use gemini::GeminiClient;
pub use mock::MockLlmClient;
pub use openai::OpenAiCompatClient;
pub use registry::build_client;

#[cfg(feature = "local-embeddings")]
pub use embeddings_local::LocalEmbeddingClient;

use std::sync::Arc;

/// Build the embeddings client from config, or `None` when disabled / not
/// available (callers then fall back to FTS5). Never errors — embeddings are an
/// enhancement, never a hard dependency.
pub fn build_embeddings_client(
    config: &blumi_config::BlumiConfig,
) -> Option<Arc<dyn blumi_core::EmbeddingClient>> {
    let cfg = &config.embeddings;
    if !cfg.enabled {
        return None;
    }
    match cfg.backend.as_str() {
        "openai" => {
            let provider = config.providers.get(&cfg.provider)?;
            let base_url = provider.base_url.clone()?;
            let key = provider.resolve_api_key();
            Some(Arc::new(OpenAiEmbeddingClient::new(
                base_url,
                key,
                cfg.model.clone(),
                cfg.dim as usize,
            )))
        }
        "local" => build_local_embeddings(&config.paths.models_dir, &cfg.model),
        // "grid" (offload embedding to a peer) is wired in a later phase.
        other => {
            tracing::warn!("unknown embeddings backend '{other}'; embeddings disabled");
            None
        }
    }
}

#[cfg(feature = "local-embeddings")]
fn build_local_embeddings(
    cache_dir: &std::path::Path,
    model: &str,
) -> Option<Arc<dyn blumi_core::EmbeddingClient>> {
    match LocalEmbeddingClient::new(model, cache_dir.to_path_buf()) {
        Ok(c) => Some(Arc::new(c)),
        Err(e) => {
            tracing::warn!("local embeddings unavailable: {e}");
            None
        }
    }
}

#[cfg(not(feature = "local-embeddings"))]
fn build_local_embeddings(
    _cache_dir: &std::path::Path,
    _model: &str,
) -> Option<Arc<dyn blumi_core::EmbeddingClient>> {
    tracing::warn!(
        "embeddings backend 'local' requested but the `local-embeddings` feature was not \
         compiled in; set embeddings.backend to 'openai' or rebuild with --features local-embeddings"
    );
    None
}

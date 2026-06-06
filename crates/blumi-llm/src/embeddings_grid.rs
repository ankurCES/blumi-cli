//! Grid embeddings backend (`embeddings.backend = "grid"`).
//!
//! Offloads embedding to a stronger GPU peer via the process-global `GridEmbed`
//! hook (registered by the gateway, which owns the peer registry + grid secret).
//! Falls back to the local bundled embedder when no peer is available — or, on a
//! lean node with no bundled embedder, returns an error so callers degrade to
//! FTS5. Recall is timeout-bounded upstream, so a slow/absent peer never stalls a turn.

use async_trait::async_trait;
use blumi_core::{EmbeddingClient, LlmError};
use std::sync::Arc;

pub struct GridEmbeddingClient {
    /// Local fallback (the bundled embedder), if compiled/available.
    local: Option<Arc<dyn EmbeddingClient>>,
    dim: usize,
    model_id: String,
}

impl GridEmbeddingClient {
    pub fn new(local: Option<Arc<dyn EmbeddingClient>>, model_id: &str, dim: usize) -> Self {
        GridEmbeddingClient {
            local,
            dim,
            model_id: model_id.to_string(),
        }
    }
}

#[async_trait]
impl EmbeddingClient for GridEmbeddingClient {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        // Offload to a GPU peer when the grid hook is live and a peer answers.
        if let Some(hook) = blumi_core::grid_embed() {
            if let Some(v) = hook.embed_remote(texts).await {
                if v.len() == texts.len() {
                    return Ok(v);
                }
            }
        }
        // Fall back to the local embedder (if any).
        match &self.local {
            Some(l) => l.embed(texts).await,
            None => Err(LlmError::Other(anyhow::anyhow!(
                "grid embeddings: no peer available and no local embedder compiled in"
            ))),
        }
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn ready(&self) -> bool {
        // Gate on the local fallback's readiness when present (so a cold local
        // model doesn't block); grid-only nodes report ready and rely on the
        // hook + the upstream recall timeout.
        self.local.as_ref().map(|l| l.ready()).unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedLocal(usize);
    #[async_trait]
    impl EmbeddingClient for FixedLocal {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
            Ok(texts.iter().map(|_| vec![0.1; self.0]).collect())
        }
        fn dim(&self) -> usize {
            self.0
        }
        fn model_id(&self) -> &str {
            "fixed"
        }
        fn ready(&self) -> bool {
            true
        }
    }

    // No grid hook is registered in unit tests (the gateway sets it), so these
    // exercise the local-fallback path.
    #[tokio::test]
    async fn falls_back_to_local_when_no_peer() {
        let c = GridEmbeddingClient::new(Some(Arc::new(FixedLocal(384))), "bge-small-en-v1.5", 384);
        let v = c.embed(&["a".into(), "b".into()]).await.unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].len(), 384);
        assert_eq!(c.dim(), 384);
        assert!(c.ready());
    }

    #[tokio::test]
    async fn errors_when_no_peer_and_no_local() {
        let c = GridEmbeddingClient::new(None, "bge-small-en-v1.5", 384);
        assert!(c.embed(&["a".into()]).await.is_err());
        assert!(c.embed(&[]).await.unwrap().is_empty());
    }
}

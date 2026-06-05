//! Bundled local embedding model via fastembed (ONNX Runtime).
//!
//! Compiled only with the `local-embeddings` feature. The model is downloaded
//! once into the models cache dir on first use; thereafter it runs fully
//! offline. Embedding is CPU-bound + synchronous, so it runs on a blocking
//! thread to avoid stalling the async runtime.

use async_trait::async_trait;
use blumi_core::{EmbeddingClient, LlmError};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct LocalEmbeddingClient {
    inner: Arc<Mutex<fastembed::TextEmbedding>>,
    model_id: String,
    dim: usize,
}

impl LocalEmbeddingClient {
    /// Load the model (downloading on first use) into `cache_dir`.
    pub fn new(model_id: &str, cache_dir: PathBuf) -> Result<Self, LlmError> {
        use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
        let (model, dim) = match model_id {
            "all-MiniLM-L6-v2" | "all-minilm-l6-v2" => (EmbeddingModel::AllMiniLML6V2, 384),
            _ => (EmbeddingModel::BGESmallENV15, 384),
        };
        let opts = InitOptions::new(model)
            .with_cache_dir(cache_dir)
            .with_show_download_progress(false);
        let emb = TextEmbedding::try_new(opts).map_err(|e| {
            LlmError::Other(anyhow::anyhow!("load embedding model '{model_id}': {e}"))
        })?;
        Ok(LocalEmbeddingClient {
            inner: Arc::new(Mutex::new(emb)),
            model_id: model_id.to_string(),
            dim,
        })
    }
}

#[async_trait]
impl EmbeddingClient for LocalEmbeddingClient {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let docs: Vec<String> = texts.to_vec();
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let guard = inner
                .lock()
                .map_err(|_| LlmError::Other(anyhow::anyhow!("embedding lock poisoned")))?;
            guard
                .embed(docs, None)
                .map_err(|e| LlmError::Other(anyhow::anyhow!("embed: {e}")))
        })
        .await
        .map_err(|e| LlmError::Other(anyhow::anyhow!("embed task join: {e}")))?
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Downloads the model (~90 MB) on first run, so it's ignored by default. Run
    // manually: cargo test -p blumi-llm --features local-embeddings -- --ignored
    #[tokio::test]
    #[ignore]
    async fn embeds_locally() {
        let dir = std::env::temp_dir().join("blumi-embed-test");
        let client = LocalEmbeddingClient::new("bge-small-en-v1.5", dir).unwrap();
        assert_eq!(client.dim(), 384);
        let v = client
            .embed(&["hello world".to_string(), "rust ownership".to_string()])
            .await
            .unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].len(), 384);
    }
}

//! Bundled local embedding model via fastembed (ONNX Runtime).
//!
//! Compiled only with the `local-embeddings` feature. The model is loaded
//! **lazily** on first use — the (~90 MB) download + load runs on a blocking
//! thread, so enabling embeddings by default never blocks startup and an
//! offline node simply degrades to FTS5 (the first `embed` errors, callers
//! fall back). Once loaded it stays resident and runs fully offline.

use crate::Accelerator;
use async_trait::async_trait;
use blumi_core::{EmbeddingClient, LlmError};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

pub struct LocalEmbeddingClient {
    /// Lazily-initialised model: empty until the first embed downloads/loads it.
    cell: OnceCell<Arc<Mutex<fastembed::TextEmbedding>>>,
    model_id: String,
    cache_dir: PathBuf,
    dim: usize,
    /// Execution provider the ONNX session runs on (Apple CoreML / CUDA / CPU).
    /// ort silently falls back to CPU if the GPU provider can't register.
    accel: Accelerator,
}

impl LocalEmbeddingClient {
    /// Prepare the client WITHOUT loading the model. `dim` is derived from the
    /// model id up front (so the vector store can size itself); the actual model
    /// download/load is deferred to the first [`embed`](Self::embed) call.
    /// `accel` selects the ONNX execution provider for the session.
    pub fn new(model_id: &str, cache_dir: PathBuf, accel: Accelerator) -> Result<Self, LlmError> {
        // All bundled models are 384-dim; kept as a match so adding a model that
        // isn't forces a conscious dim update.
        let dim = match model_id {
            "all-MiniLM-L6-v2" | "all-minilm-l6-v2" => 384,
            _ => 384, // bge-small-en-v1.5 (default)
        };
        Ok(LocalEmbeddingClient {
            cell: OnceCell::new(),
            model_id: model_id.to_string(),
            cache_dir,
            dim,
            accel,
        })
    }

    /// The execution provider this client is configured to use.
    pub fn accelerator(&self) -> Accelerator {
        self.accel
    }

    /// Get the model, loading it (downloading on first ever use) if needed.
    async fn model(&self) -> Result<Arc<Mutex<fastembed::TextEmbedding>>, LlmError> {
        self.cell
            .get_or_try_init(|| async {
                let model_id = self.model_id.clone();
                let cache_dir = self.cache_dir.clone();
                let accel = self.accel;
                tokio::task::spawn_blocking(move || {
                    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
                    let model = match model_id.as_str() {
                        "all-MiniLM-L6-v2" | "all-minilm-l6-v2" => EmbeddingModel::AllMiniLML6V2,
                        _ => EmbeddingModel::BGESmallENV15,
                    };
                    // ort appends a CPU provider and silently falls back, so an
                    // unavailable GPU degrades to CPU rather than failing the load.
                    let opts = InitOptions::new(model)
                        .with_cache_dir(cache_dir)
                        .with_show_download_progress(false)
                        .with_execution_providers(crate::accel::execution_providers(accel));
                    TextEmbedding::try_new(opts)
                        .map(|e| {
                            tracing::info!(
                                accel = %accel, model = %model_id,
                                "bundled embedder loaded"
                            );
                            Arc::new(Mutex::new(e))
                        })
                        .map_err(|e| LlmError::Other(anyhow::anyhow!("load embedding model: {e}")))
                })
                .await
                .map_err(|e| LlmError::Other(anyhow::anyhow!("embed model load join: {e}")))?
            })
            .await
            .cloned()
    }
}

#[async_trait]
impl EmbeddingClient for LocalEmbeddingClient {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let model = self.model().await?;
        let docs: Vec<String> = texts.to_vec();
        tokio::task::spawn_blocking(move || {
            let guard = model
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

    /// Ready only once the model has finished its one-time load/download — a
    /// non-blocking peek at the lazy cell, so per-turn recall never stalls
    /// waiting on the cold start (the background warmup drives the load).
    fn ready(&self) -> bool {
        self.cell.get().is_some()
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
        // detect() exercises the real GPU EP on capable builds; CPU otherwise.
        let client =
            LocalEmbeddingClient::new("bge-small-en-v1.5", dir, crate::accel::detect()).unwrap();
        assert_eq!(client.dim(), 384);
        let v = client
            .embed(&["hello world".to_string(), "rust ownership".to_string()])
            .await
            .unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].len(), 384);
    }
}

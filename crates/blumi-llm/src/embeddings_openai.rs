//! OpenAI-compatible embeddings client (`POST {base_url}/embeddings`).
//!
//! Works with OpenAI, Ollama (`/v1/embeddings`, e.g. `nomic-embed-text`),
//! llama.cpp, and any compatible endpoint. The agent only ever sees the
//! provider-neutral [`EmbeddingClient`] trait.

use async_trait::async_trait;
use blumi_core::{EmbeddingClient, LlmError};
use serde_json::json;

pub struct OpenAiEmbeddingClient {
    http: reqwest::Client,
    /// Base URL including any `/v1` suffix.
    base_url: String,
    api_key: Option<String>,
    model: String,
    dim: usize,
}

impl OpenAiEmbeddingClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        dim: usize,
    ) -> Self {
        OpenAiEmbeddingClient {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            model: model.into(),
            dim,
        }
    }
}

#[async_trait]
impl EmbeddingClient for OpenAiEmbeddingClient {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/embeddings", self.base_url);
        let mut req = self
            .http
            .post(&url)
            .json(&json!({ "model": self.model, "input": texts }));
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(LlmError::Provider {
                status: status.as_u16(),
                message,
            });
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::Stream(e.to_string()))?;
        let data = v
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| LlmError::Stream("embeddings response missing data[]".into()))?;
        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let emb = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| LlmError::Stream("embeddings item missing embedding[]".into()))?;
            out.push(
                emb.iter()
                    .filter_map(|x| x.as_f64().map(|f| f as f32))
                    .collect(),
            );
        }
        Ok(out)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

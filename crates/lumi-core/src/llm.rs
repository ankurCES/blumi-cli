//! The LLM provider abstraction. Implementations live in `lumi-llm`.

use crate::error::LlmError;
use async_trait::async_trait;
use futures::stream::BoxStream;
use lumi_protocol::{Message, StreamChunk};
use tokio_util::sync::CancellationToken;

/// Per-request model settings, derived from `LlmConfig`.
#[derive(Debug, Clone)]
pub struct LlmOptions {
    pub model: String,
    pub max_output_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    /// Request extended thinking / reasoning (if the model supports it).
    pub thinking: bool,
    /// Apply provider prompt-cache breakpoints (Anthropic `cache_control`).
    pub prompt_cache: bool,
}

impl Default for LlmOptions {
    fn default() -> Self {
        LlmOptions {
            model: String::new(),
            max_output_tokens: 16_384,
            temperature: 0.7,
            top_p: 0.8,
            top_k: 20,
            thinking: false,
            prompt_cache: true,
        }
    }
}

/// What a provider/model supports, so the loop can adapt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProviderCaps {
    pub prompt_caching: bool,
    pub thinking: bool,
    pub vision: bool,
}

/// A provider-neutral tool definition. The loop builds these from the registry;
/// each provider client formats them into its own wire shape.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's parameters.
    pub parameters: serde_json::Value,
}

/// A streaming chat completion client. One per provider implementation; the
/// agent loop only ever sees this trait.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Start a streaming completion. `tools` is the provider-neutral tool list
    /// (empty to disable tool calling); the client formats it for its wire API.
    ///
    /// The returned stream yields [`StreamChunk`]s until a terminal
    /// `StreamChunk::Done` (or an error). Cancelling `ct` aborts the request.
    async fn stream_chat(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        options: &LlmOptions,
        ct: CancellationToken,
    ) -> Result<BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError>;

    /// Capabilities of this client/model.
    fn caps(&self) -> ProviderCaps {
        ProviderCaps::default()
    }
}

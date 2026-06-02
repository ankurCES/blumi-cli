//! LLM provider clients.
//!
//! A single [`OpenAiCompatClient`] covers most providers via `base_url`;
//! [`AnthropicClient`] is native. [`build_client`] picks the right one from a
//! provider config; [`MockLlmClient`] scripts responses for tests/offline use.

mod anthropic;
mod mock;
mod openai;
mod registry;
mod retry;

pub use anthropic::AnthropicClient;
pub use mock::MockLlmClient;
pub use openai::OpenAiCompatClient;
pub use registry::build_client;

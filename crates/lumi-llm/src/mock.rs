//! A scripted [`LlmClient`] for tests and offline runs.

use async_trait::async_trait;
use futures::stream::BoxStream;
use lumi_core::{LlmClient, LlmError, LlmOptions, ProviderCaps, ToolSpec};
use lumi_protocol::{Message, StreamChunk};
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Replays a fixed list of [`StreamChunk`]s. If multiple scripts are queued,
/// each call consumes the next (so a multi-turn loop can be driven precisely).
pub struct MockLlmClient {
    scripts: Mutex<std::collections::VecDeque<Vec<StreamChunk>>>,
    caps: ProviderCaps,
}

impl MockLlmClient {
    /// One script replayed on every call.
    pub fn new(chunks: Vec<StreamChunk>) -> Self {
        let mut q = std::collections::VecDeque::new();
        q.push_back(chunks);
        MockLlmClient { scripts: Mutex::new(q), caps: ProviderCaps::default() }
    }

    /// A queue of scripts; call N returns script N (last repeats once exhausted
    /// it returns just a Done).
    pub fn scripted(scripts: Vec<Vec<StreamChunk>>) -> Self {
        MockLlmClient { scripts: Mutex::new(scripts.into()), caps: ProviderCaps::default() }
    }

    fn next_script(&self) -> Vec<StreamChunk> {
        let mut q = self.scripts.lock().unwrap();
        if q.len() > 1 {
            q.pop_front().unwrap()
        } else {
            q.front().cloned().unwrap_or_else(|| {
                vec![StreamChunk::Done { reason: lumi_protocol::FinishReason::Stop }]
            })
        }
    }
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn stream_chat(
        &self,
        _messages: &[Message],
        _tools: &[ToolSpec],
        _options: &LlmOptions,
        _ct: CancellationToken,
    ) -> Result<BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError> {
        let chunks = self.next_script();
        let stream = futures::stream::iter(chunks.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }

    fn caps(&self) -> ProviderCaps {
        self.caps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use lumi_protocol::FinishReason;

    #[tokio::test]
    async fn replays_script() {
        let client = MockLlmClient::new(vec![
            StreamChunk::Text { text: "hi".into() },
            StreamChunk::Done { reason: FinishReason::Stop },
        ]);
        let mut s = client
            .stream_chat(&[], &[], &LlmOptions::default(), CancellationToken::new())
            .await
            .unwrap();
        let mut texts = Vec::new();
        while let Some(Ok(chunk)) = s.next().await {
            if let StreamChunk::Text { text } = chunk {
                texts.push(text);
            }
        }
        assert_eq!(texts, vec!["hi"]);
    }
}

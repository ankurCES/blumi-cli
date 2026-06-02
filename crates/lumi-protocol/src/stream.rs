//! Types produced by an [`crate`] provider while streaming a completion.
//!
//! A provider client yields a sequence of [`StreamChunk`]s; the agent loop
//! accumulates them into assistant text, reasoning, tool calls, and usage.

use serde::{Deserialize, Serialize};

/// One incremental piece of a streamed model response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamChunk {
    /// Extended-thinking / reasoning delta.
    Thinking { text: String },
    /// Visible assistant text delta.
    Text { text: String },
    /// A fragment of a tool call (accumulated by `index`).
    ToolCall(ToolCallDelta),
    /// Token accounting (may arrive mid- or end-of-stream).
    Usage(Usage),
    /// Terminal marker for the stream.
    Done { reason: FinishReason },
}

/// A fragment of a tool call. Providers emit these incrementally; fragments
/// sharing an `index` belong to the same call and are concatenated.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolCallDelta {
    /// Provider-assigned position used to group fragments of the same call.
    pub index: u32,
    /// The call id (usually present only on the first fragment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The tool name (usually present only on the first fragment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A piece of the JSON-encoded arguments string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments_fragment: Option<String>,
}

/// Token usage for a completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Tokens served from the provider's prompt cache (read hit).
    #[serde(default)]
    pub cache_read_tokens: u32,
    /// Tokens written into the provider's prompt cache.
    #[serde(default)]
    pub cache_write_tokens: u32,
}

impl Usage {
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

/// Why a stream ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural stop / end of message.
    Stop,
    /// Hit the max output length.
    Length,
    /// Stopped to make tool calls.
    ToolCalls,
    /// Stopped by a content filter.
    ContentFilter,
    /// Provider error.
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_round_trips() {
        let chunks = vec![
            StreamChunk::Thinking { text: "hm".into() },
            StreamChunk::Text {
                text: "hello".into(),
            },
            StreamChunk::ToolCall(ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("Bash".into()),
                arguments_fragment: Some("{\"cmd\":".into()),
            }),
            StreamChunk::Usage(Usage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            }),
            StreamChunk::Done {
                reason: FinishReason::ToolCalls,
            },
        ];
        for c in chunks {
            let json = serde_json::to_string(&c).unwrap();
            let back: StreamChunk = serde_json::from_str(&json).unwrap();
            assert_eq!(c, back);
        }
    }

    #[test]
    fn usage_total() {
        let u = Usage {
            input_tokens: 100,
            output_tokens: 40,
            ..Default::default()
        };
        assert_eq!(u.total(), 140);
    }
}

//! OpenAI-compatible `/chat/completions` streaming client.
//!
//! Covers OpenAI, OpenRouter, DeepSeek, Ollama, llama.cpp, MiniMax, Groq, and
//! any compatible endpoint via `base_url`. Emits raw [`ToolCallDelta`]
//! fragments; the agent loop accumulates them.

use crate::retry::send_with_retry;
use async_stream::stream;
use async_trait::async_trait;
use blumi_core::{LlmClient, LlmError, LlmOptions, ProviderCaps, ToolSpec};
use blumi_protocol::{
    ContentPart, FinishReason, ImageData, Message, Role, StreamChunk, ToolCallDelta, Usage,
};
use eventsource_stream::Eventsource;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

pub struct OpenAiCompatClient {
    http: reqwest::Client,
    /// Base URL including any `/v1` suffix.
    base_url: String,
    api_key: Option<String>,
}

impl OpenAiCompatClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        OpenAiCompatClient {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatClient {
    async fn stream_chat(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        options: &LlmOptions,
        ct: CancellationToken,
    ) -> Result<BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError> {
        let body = build_body(messages, tools, options);
        let mut req = self.http.post(self.endpoint()).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = send_with_retry(req, &ct).await?;
        let mut events = resp.bytes_stream().eventsource();

        let stream = stream! {
            loop {
                tokio::select! {
                    _ = ct.cancelled() => {
                        yield Err(LlmError::Cancelled);
                        break;
                    }
                    next = events.next() => {
                        match next {
                            None => break,
                            Some(Err(e)) => { yield Err(LlmError::Stream(e.to_string())); break; }
                            Some(Ok(event)) => {
                                if event.data == "[DONE]" { break; }
                                match serde_json::from_str::<Value>(&event.data) {
                                    Ok(v) => {
                                        for chunk in map_openai_chunk(&v) {
                                            yield Ok(chunk);
                                        }
                                    }
                                    Err(e) => { yield Err(LlmError::Stream(e.to_string())); }
                                }
                            }
                        }
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }

    fn caps(&self) -> ProviderCaps {
        ProviderCaps {
            prompt_caching: false,
            thinking: true,
            vision: true,
        }
    }
}

/// Build the `/chat/completions` request body. `top_k` is intentionally omitted
/// (strict providers reject unknown params; servers that want it use defaults).
fn build_body(messages: &[Message], tools: &[ToolSpec], options: &LlmOptions) -> Value {
    let mut body = json!({
        "model": options.model,
        "messages": map_messages(messages),
        "stream": true,
        "stream_options": { "include_usage": true },
        "temperature": options.temperature,
        "top_p": options.top_p,
        "max_tokens": options.max_output_tokens,
    });
    if !tools.is_empty() {
        body["tools"] = map_tools(tools);
    }
    body
}

fn map_tools(tools: &[ToolSpec]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect(),
    )
}

fn map_messages(messages: &[Message]) -> Value {
    Value::Array(messages.iter().map(map_message).collect())
}

fn map_message(m: &Message) -> Value {
    let role = match m.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut obj = json!({ "role": role });

    match m.role {
        Role::Tool => {
            obj["content"] = json!(m.text());
            if let Some(id) = &m.tool_call_id {
                obj["tool_call_id"] = json!(id.as_str());
            }
        }
        Role::Assistant => {
            obj["content"] = json!(m.text());
            if !m.tool_calls.is_empty() {
                obj["tool_calls"] = Value::Array(
                    m.tool_calls
                        .iter()
                        .map(|tc| {
                            json!({
                                "id": tc.id.as_str(),
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string(),
                                }
                            })
                        })
                        .collect(),
                );
            }
        }
        _ => {
            obj["content"] = map_content(&m.content);
        }
    }
    obj
}

/// User content: a plain string for text-only, else the multimodal array form.
fn map_content(parts: &[ContentPart]) -> Value {
    let has_image = parts.iter().any(|p| matches!(p, ContentPart::Image(_)));
    if !has_image {
        let text: String = parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        return json!(text);
    }
    let arr: Vec<Value> = parts
        .iter()
        .map(|p| match p {
            ContentPart::Text { text } => json!({ "type": "text", "text": text }),
            ContentPart::Image(img) => {
                let url = match &img.data {
                    ImageData::Url { url } => url.clone(),
                    ImageData::Base64 { data } => {
                        format!("data:{};base64,{}", img.media_type, data)
                    }
                };
                json!({ "type": "image_url", "image_url": { "url": url } })
            }
        })
        .collect();
    Value::Array(arr)
}

/// Map one streamed SSE JSON chunk to zero or more [`StreamChunk`]s. Pure, so
/// it can be unit-tested without a server.
fn map_openai_chunk(v: &Value) -> Vec<StreamChunk> {
    let mut out = Vec::new();

    if let Some(choice) = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
    {
        if let Some(delta) = choice.get("delta") {
            // DeepSeek-style reasoning.
            if let Some(r) = delta.get("reasoning_content").and_then(Value::as_str) {
                if !r.is_empty() {
                    out.push(StreamChunk::Thinking {
                        text: r.to_string(),
                    });
                }
            }
            if let Some(c) = delta.get("content").and_then(Value::as_str) {
                if !c.is_empty() {
                    out.push(StreamChunk::Text {
                        text: c.to_string(),
                    });
                }
            }
            if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
                for (i, tc) in tcs.iter().enumerate() {
                    let index = tc.get("index").and_then(Value::as_u64).unwrap_or(i as u64) as u32;
                    let func = tc.get("function");
                    out.push(StreamChunk::ToolCall(ToolCallDelta {
                        index,
                        id: tc.get("id").and_then(Value::as_str).map(String::from),
                        name: func
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .map(String::from),
                        arguments_fragment: func
                            .and_then(|f| f.get("arguments"))
                            .and_then(Value::as_str)
                            .map(String::from),
                    }));
                }
            }
        }
        if let Some(fr) = choice.get("finish_reason").and_then(Value::as_str) {
            out.push(StreamChunk::Done {
                reason: map_finish(fr),
            });
        }
    }

    if let Some(usage) = v.get("usage").filter(|u| !u.is_null()) {
        let cache_read = usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        out.push(StreamChunk::Usage(Usage {
            input_tokens: usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            output_tokens: usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            cache_read_tokens: cache_read,
            cache_write_tokens: 0,
        }));
    }

    out
}

fn map_finish(s: &str) -> FinishReason {
    match s {
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blumi_protocol::ToolCall;

    #[test]
    fn maps_text_delta() {
        let v = json!({ "choices": [{ "delta": { "content": "hello" } }] });
        let chunks = map_openai_chunk(&v);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text { text } if text == "hello"));
    }

    #[test]
    fn maps_tool_call_fragments() {
        let first = json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 0, "id": "call_1", "function": { "name": "Bash", "arguments": "{\"cmd\":" } }
        ] } }] });
        let chunks = map_openai_chunk(&first);
        match &chunks[0] {
            StreamChunk::ToolCall(d) => {
                assert_eq!(d.index, 0);
                assert_eq!(d.id.as_deref(), Some("call_1"));
                assert_eq!(d.name.as_deref(), Some("Bash"));
                assert_eq!(d.arguments_fragment.as_deref(), Some("{\"cmd\":"));
            }
            _ => panic!("expected tool call"),
        }
    }

    #[test]
    fn maps_finish_and_usage() {
        let v = json!({
            "choices": [{ "delta": {}, "finish_reason": "tool_calls" }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 5,
                       "prompt_tokens_details": { "cached_tokens": 8 } }
        });
        let chunks = map_openai_chunk(&v);
        assert!(matches!(
            chunks[0],
            StreamChunk::Done {
                reason: FinishReason::ToolCalls
            }
        ));
        match chunks[1] {
            StreamChunk::Usage(u) => {
                assert_eq!(u.input_tokens, 12);
                assert_eq!(u.output_tokens, 5);
                assert_eq!(u.cache_read_tokens, 8);
            }
            _ => panic!("expected usage"),
        }
    }

    #[test]
    fn assistant_tool_calls_serialize_to_openai_shape() {
        let m = Message::assistant_tool_calls(
            None,
            vec![ToolCall {
                id: "call_1".into(),
                name: "Bash".into(),
                arguments: json!({ "cmd": "ls" }),
            }],
        );
        let v = map_message(&m);
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["tool_calls"][0]["function"]["name"], "Bash");
        // arguments are serialized as a JSON string
        assert_eq!(
            v["tool_calls"][0]["function"]["arguments"],
            "{\"cmd\":\"ls\"}"
        );
    }

    #[test]
    fn image_content_uses_multimodal_array() {
        let parts = vec![
            ContentPart::text("look:"),
            ContentPart::Image(blumi_protocol::ImageRef {
                media_type: "image/png".into(),
                data: ImageData::Base64 {
                    data: "AAAA".into(),
                },
            }),
        ];
        let v = map_content(&parts);
        assert!(v.is_array());
        assert_eq!(v[0]["type"], "text");
        assert_eq!(v[1]["type"], "image_url");
        assert!(v[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }
}

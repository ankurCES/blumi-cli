//! Native Anthropic `/v1/messages` streaming client.
//!
//! Handles the content-block model, role coalescing (tool results become
//! `user` `tool_result` blocks), prompt caching (`cache_control` on the system
//! block and the final message block), and extended thinking.

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

const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicClient {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl AnthropicClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        AnthropicClient {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn stream_chat(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        options: &LlmOptions,
        ct: CancellationToken,
    ) -> Result<BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError> {
        let body = build_body(messages, tools, options);
        let mut req = self
            .http
            .post(self.endpoint())
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body);
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }

        let resp = send_with_retry(req, &ct).await?;
        let mut events = resp.bytes_stream().eventsource();

        let stream = stream! {
            loop {
                tokio::select! {
                    _ = ct.cancelled() => { yield Err(LlmError::Cancelled); break; }
                    next = events.next() => match next {
                        None => break,
                        Some(Err(e)) => { yield Err(LlmError::Stream(e.to_string())); break; }
                        Some(Ok(event)) => {
                            let data: Value = match serde_json::from_str(&event.data) {
                                Ok(v) => v,
                                Err(_) => continue, // ping / non-JSON keepalive
                            };
                            for chunk in map_event(&event.event, &data) {
                                yield Ok(chunk);
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
            prompt_caching: true,
            thinking: true,
            vision: true,
        }
    }
}

fn build_body(messages: &[Message], tools: &[ToolSpec], options: &LlmOptions) -> Value {
    let mut body = json!({
        "model": options.model,
        "max_tokens": options.max_output_tokens,
        "temperature": options.temperature,
        "top_p": options.top_p,
        "stream": true,
        "messages": build_messages(messages, options.prompt_cache),
    });

    if let Some(system) = build_system(messages, options.prompt_cache) {
        body["system"] = system;
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters,
                    })
                })
                .collect(),
        );
    }
    if options.thinking {
        // A modest default thinking budget; refined per-model later.
        body["thinking"] = json!({ "type": "enabled", "budget_tokens": 4096 });
    }
    body
}

/// Collect all system messages into a single cached system block array.
fn build_system(messages: &[Message], cache: bool) -> Option<Value> {
    let text: String = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.text())
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.is_empty() {
        return None;
    }
    let mut block = json!({ "type": "text", "text": text });
    if cache {
        block["cache_control"] = json!({ "type": "ephemeral" });
    }
    Some(Value::Array(vec![block]))
}

/// Build the messages array, coalescing consecutive same-role turns and turning
/// tool results into `user`/`tool_result` blocks.
fn build_messages(messages: &[Message], cache: bool) -> Value {
    let mut out: Vec<Value> = Vec::new();

    for m in messages.iter().filter(|m| m.role != Role::System) {
        let role = if m.role == Role::Assistant {
            "assistant"
        } else {
            "user"
        };
        let blocks = blocks_for(m);
        if blocks.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last["role"] == role => {
                if let Some(arr) = last["content"].as_array_mut() {
                    arr.extend(blocks);
                }
            }
            _ => out.push(json!({ "role": role, "content": blocks })),
        }
    }

    // Cache breakpoint on the final content block of the conversation.
    if cache {
        if let Some(last) = out.last_mut() {
            if let Some(arr) = last["content"].as_array_mut() {
                if let Some(block) = arr.last_mut() {
                    block["cache_control"] = json!({ "type": "ephemeral" });
                }
            }
        }
    }

    Value::Array(out)
}

fn blocks_for(m: &Message) -> Vec<Value> {
    match m.role {
        Role::Tool => {
            let id = m.tool_call_id.as_ref().map(|i| i.as_str()).unwrap_or("");
            vec![json!({ "type": "tool_result", "tool_use_id": id, "content": m.text() })]
        }
        Role::Assistant => {
            let mut blocks = Vec::new();
            let text = m.text();
            if !text.is_empty() {
                blocks.push(json!({ "type": "text", "text": text }));
            }
            for tc in &m.tool_calls {
                blocks.push(json!({
                    "type": "tool_use",
                    "id": tc.id.as_str(),
                    "name": tc.name,
                    "input": tc.arguments,
                }));
            }
            blocks
        }
        _ => m
            .content
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => json!({ "type": "text", "text": text }),
                ContentPart::Image(img) => match &img.data {
                    ImageData::Base64 { data } => json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": img.media_type, "data": data }
                    }),
                    ImageData::Url { url } => json!({
                        "type": "image",
                        "source": { "type": "url", "url": url }
                    }),
                },
            })
            .collect(),
    }
}

/// Map one Anthropic SSE event to zero or more [`StreamChunk`]s. Pure.
fn map_event(event: &str, data: &Value) -> Vec<StreamChunk> {
    match event {
        "message_start" => {
            let usage = &data["message"]["usage"];
            vec![StreamChunk::Usage(Usage {
                input_tokens: usage["input_tokens"].as_u64().unwrap_or(0) as u32,
                output_tokens: 0,
                cache_read_tokens: usage["cache_read_input_tokens"].as_u64().unwrap_or(0) as u32,
                cache_write_tokens: usage["cache_creation_input_tokens"].as_u64().unwrap_or(0)
                    as u32,
            })]
        }
        "content_block_start" => {
            let block = &data["content_block"];
            if block["type"] == "tool_use" {
                let index = data["index"].as_u64().unwrap_or(0) as u32;
                vec![StreamChunk::ToolCall(ToolCallDelta {
                    index,
                    id: block["id"].as_str().map(String::from),
                    name: block["name"].as_str().map(String::from),
                    arguments_fragment: None,
                })]
            } else {
                vec![]
            }
        }
        "content_block_delta" => {
            let index = data["index"].as_u64().unwrap_or(0) as u32;
            let delta = &data["delta"];
            match delta["type"].as_str() {
                Some("text_delta") => delta["text"]
                    .as_str()
                    .map(|t| {
                        vec![StreamChunk::Text {
                            text: t.to_string(),
                        }]
                    })
                    .unwrap_or_default(),
                Some("thinking_delta") => delta["thinking"]
                    .as_str()
                    .map(|t| {
                        vec![StreamChunk::Thinking {
                            text: t.to_string(),
                        }]
                    })
                    .unwrap_or_default(),
                Some("input_json_delta") => delta["partial_json"]
                    .as_str()
                    .map(|j| {
                        vec![StreamChunk::ToolCall(ToolCallDelta {
                            index,
                            id: None,
                            name: None,
                            arguments_fragment: Some(j.to_string()),
                        })]
                    })
                    .unwrap_or_default(),
                _ => vec![],
            }
        }
        "message_delta" => {
            let mut out = Vec::new();
            if let Some(out_tokens) = data["usage"]["output_tokens"].as_u64() {
                out.push(StreamChunk::Usage(Usage {
                    input_tokens: 0,
                    output_tokens: out_tokens as u32,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                }));
            }
            if let Some(reason) = data["delta"]["stop_reason"].as_str() {
                out.push(StreamChunk::Done {
                    reason: map_stop(reason),
                });
            }
            out
        }
        "error" => {
            let msg = data["error"]["message"]
                .as_str()
                .unwrap_or("anthropic error");
            vec![
                StreamChunk::Text {
                    text: format!("\n[error: {msg}]"),
                },
                StreamChunk::Done {
                    reason: FinishReason::Error,
                },
            ]
        }
        _ => vec![],
    }
}

fn map_stop(s: &str) -> FinishReason {
    match s {
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        _ => FinishReason::Stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blumi_protocol::ToolCall;

    #[test]
    fn system_is_extracted_and_cached() {
        let msgs = vec![Message::system("be brief"), Message::user("hi")];
        let sys = build_system(&msgs, true).unwrap();
        assert_eq!(sys[0]["text"], "be brief");
        assert_eq!(sys[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn tool_result_becomes_user_block_and_coalesces() {
        let msgs = vec![
            Message::user("run it"),
            Message::assistant_tool_calls(
                None,
                vec![ToolCall {
                    id: "t1".into(),
                    name: "Bash".into(),
                    arguments: json!({}),
                }],
            ),
            Message::tool_result(blumi_protocol::ToolCallId::from("t1"), "Bash", "done"),
        ];
        let built = build_messages(&msgs, false);
        let arr = built.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[1]["content"][0]["type"], "tool_use");
        assert_eq!(arr[2]["role"], "user");
        assert_eq!(arr[2]["content"][0]["type"], "tool_result");
        assert_eq!(arr[2]["content"][0]["tool_use_id"], "t1");
    }

    #[test]
    fn maps_text_and_tool_events() {
        let start = json!({ "index": 1, "content_block": { "type": "tool_use", "id": "tu_1", "name": "Bash" } });
        match &map_event("content_block_start", &start)[0] {
            StreamChunk::ToolCall(d) => {
                assert_eq!(d.index, 1);
                assert_eq!(d.name.as_deref(), Some("Bash"));
            }
            _ => panic!("expected tool call"),
        }

        let delta = json!({ "index": 1, "delta": { "type": "input_json_delta", "partial_json": "{\"x\":1}" } });
        match &map_event("content_block_delta", &delta)[0] {
            StreamChunk::ToolCall(d) => {
                assert_eq!(d.arguments_fragment.as_deref(), Some("{\"x\":1}"))
            }
            _ => panic!("expected fragment"),
        }

        let text = json!({ "index": 0, "delta": { "type": "text_delta", "text": "hello" } });
        assert!(
            matches!(&map_event("content_block_delta", &text)[0], StreamChunk::Text { text } if text == "hello")
        );
    }

    #[test]
    fn maps_stop_reason() {
        let md = json!({ "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 7 } });
        let chunks = map_event("message_delta", &md);
        assert!(matches!(chunks[0], StreamChunk::Usage(u) if u.output_tokens == 7));
        assert!(matches!(
            chunks[1],
            StreamChunk::Done {
                reason: FinishReason::ToolCalls
            }
        ));
    }
}

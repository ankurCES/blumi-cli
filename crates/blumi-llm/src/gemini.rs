//! Native Google Gemini streaming client (generativelanguage v1beta).
//!
//! Maps blumi's message/tool model onto Gemini's `contents` + `functionCall` /
//! `functionResponse` shape, and the streamed `GenerateContentResponse` back to
//! [`StreamChunk`]s. Gemini emits complete function calls (not fragments), so
//! each is given a synthesized id/index for the loop's accumulator.

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

pub struct GeminiClient {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl GeminiClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        GeminiClient {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
        }
    }

    fn endpoint(&self, model: &str) -> String {
        format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
            self.base_url, model
        )
    }
}

#[async_trait]
impl LlmClient for GeminiClient {
    async fn stream_chat(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        options: &LlmOptions,
        ct: CancellationToken,
    ) -> Result<BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError> {
        let body = build_body(messages, tools, options);
        let mut req = self.http.post(self.endpoint(&options.model)).json(&body);
        if let Some(key) = &self.api_key {
            req = req.header("x-goog-api-key", key);
        }

        let resp = send_with_retry(req, &ct).await?;
        let mut events = resp.bytes_stream().eventsource();

        let stream = stream! {
            let mut tool_idx: u32 = 0;
            loop {
                tokio::select! {
                    _ = ct.cancelled() => { yield Err(LlmError::Cancelled); break; }
                    next = events.next() => match next {
                        None => break,
                        Some(Err(e)) => { yield Err(LlmError::Stream(e.to_string())); break; }
                        Some(Ok(event)) => {
                            if event.data.trim().is_empty() { continue; }
                            match serde_json::from_str::<Value>(&event.data) {
                                Ok(v) => {
                                    for chunk in map_gemini_chunk(&v, &mut tool_idx) {
                                        yield Ok(chunk);
                                    }
                                }
                                Err(e) => { yield Err(LlmError::Stream(e.to_string())); }
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
            thinking: false,
            vision: true,
        }
    }
}

fn build_body(messages: &[Message], tools: &[ToolSpec], options: &LlmOptions) -> Value {
    let mut body = json!({
        "contents": map_contents(messages),
        "generationConfig": {
            "temperature": options.temperature,
            "topP": options.top_p,
            "maxOutputTokens": options.max_output_tokens,
        },
    });
    if let Some(sys) = system_instruction(messages) {
        body["systemInstruction"] = sys;
    }
    if !tools.is_empty() {
        let decls: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": sanitize_schema(&t.parameters),
                })
            })
            .collect();
        body["tools"] = json!([{ "functionDeclarations": decls }]);
    }
    body
}

/// Collect system messages into Gemini's `systemInstruction`.
fn system_instruction(messages: &[Message]) -> Option<Value> {
    let text: String = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.text())
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.is_empty() {
        None
    } else {
        Some(json!({ "parts": [{ "text": text }] }))
    }
}

fn map_contents(messages: &[Message]) -> Value {
    let contents: Vec<Value> = messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(map_message)
        .collect();
    Value::Array(contents)
}

fn map_message(m: &Message) -> Value {
    match m.role {
        Role::Tool => json!({
            "role": "user",
            "parts": [{
                "functionResponse": {
                    "name": m.tool_name.clone().unwrap_or_default(),
                    "response": { "result": m.text() },
                }
            }]
        }),
        Role::Assistant => {
            let mut parts: Vec<Value> = Vec::new();
            let text = m.text();
            if !text.is_empty() {
                parts.push(json!({ "text": text }));
            }
            for tc in &m.tool_calls {
                parts.push(json!({ "functionCall": { "name": tc.name, "args": tc.arguments } }));
            }
            if parts.is_empty() {
                parts.push(json!({ "text": "" }));
            }
            json!({ "role": "model", "parts": parts })
        }
        _ => json!({ "role": "user", "parts": map_parts(&m.content) }),
    }
}

fn map_parts(parts: &[ContentPart]) -> Value {
    let arr: Vec<Value> = parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(json!({ "text": text })),
            ContentPart::Image(img) => match &img.data {
                ImageData::Base64 { data } => Some(json!({
                    "inlineData": { "mimeType": img.media_type, "data": data }
                })),
                // Gemini fileData needs an uploaded URI; arbitrary URLs are skipped.
                ImageData::Url { .. } => None,
            },
        })
        .collect();
    if arr.is_empty() {
        json!([{ "text": "" }])
    } else {
        Value::Array(arr)
    }
}

/// Strip JSON-Schema keywords Gemini's function-declaration parser rejects.
fn sanitize_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if matches!(
                    k.as_str(),
                    "$schema" | "additionalProperties" | "title" | "definitions" | "$defs"
                ) {
                    continue;
                }
                out.insert(k.clone(), sanitize_schema(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_schema).collect()),
        other => other.clone(),
    }
}

/// Map one streamed Gemini response object to zero or more [`StreamChunk`]s.
/// `tool_idx` supplies a unique index/id per function call across the stream.
fn map_gemini_chunk(v: &Value, tool_idx: &mut u32) -> Vec<StreamChunk> {
    let mut out = Vec::new();

    if let Some(cand) = v
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
    {
        if let Some(parts) = cand.pointer("/content/parts").and_then(Value::as_array) {
            for part in parts {
                if let Some(t) = part.get("text").and_then(Value::as_str) {
                    if !t.is_empty() {
                        out.push(StreamChunk::Text {
                            text: t.to_string(),
                        });
                    }
                }
                if let Some(fc) = part.get("functionCall") {
                    let name = fc.get("name").and_then(Value::as_str).map(String::from);
                    let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                    out.push(StreamChunk::ToolCall(ToolCallDelta {
                        index: *tool_idx,
                        id: Some(format!("call_{tool_idx}")),
                        name,
                        arguments_fragment: Some(args.to_string()),
                    }));
                    *tool_idx += 1;
                }
            }
        }
        if let Some(fr) = cand.get("finishReason").and_then(Value::as_str) {
            out.push(StreamChunk::Done {
                reason: map_finish(fr),
            });
        }
    }

    if let Some(um) = v.get("usageMetadata") {
        out.push(StreamChunk::Usage(Usage {
            input_tokens: um
                .get("promptTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            output_tokens: um
                .get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            cache_read_tokens: um
                .get("cachedContentTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            cache_write_tokens: 0,
        }));
    }

    out
}

fn map_finish(s: &str) -> FinishReason {
    match s {
        "MAX_TOKENS" => FinishReason::Length,
        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blumi_protocol::ToolCall;

    #[test]
    fn maps_text_part() {
        let v = json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "hi" }] } }] });
        let mut idx = 0;
        let chunks = map_gemini_chunk(&v, &mut idx);
        assert!(matches!(&chunks[0], StreamChunk::Text { text } if text == "hi"));
    }

    #[test]
    fn maps_function_call_with_synthesized_id() {
        let v = json!({ "candidates": [{ "content": { "parts": [
            { "functionCall": { "name": "Bash", "args": { "cmd": "ls" } } }
        ] } }] });
        let mut idx = 0;
        let chunks = map_gemini_chunk(&v, &mut idx);
        match &chunks[0] {
            StreamChunk::ToolCall(d) => {
                assert_eq!(d.index, 0);
                assert_eq!(d.id.as_deref(), Some("call_0"));
                assert_eq!(d.name.as_deref(), Some("Bash"));
                assert_eq!(d.arguments_fragment.as_deref(), Some("{\"cmd\":\"ls\"}"));
            }
            _ => panic!("expected tool call"),
        }
        assert_eq!(idx, 1);
    }

    #[test]
    fn maps_finish_and_usage() {
        let v = json!({
            "candidates": [{ "content": { "parts": [] }, "finishReason": "MAX_TOKENS" }],
            "usageMetadata": { "promptTokenCount": 10, "candidatesTokenCount": 4, "cachedContentTokenCount": 3 }
        });
        let mut idx = 0;
        let chunks = map_gemini_chunk(&v, &mut idx);
        assert!(matches!(
            chunks[0],
            StreamChunk::Done {
                reason: FinishReason::Length
            }
        ));
        match chunks[1] {
            StreamChunk::Usage(u) => {
                assert_eq!(u.input_tokens, 10);
                assert_eq!(u.output_tokens, 4);
                assert_eq!(u.cache_read_tokens, 3);
            }
            _ => panic!("expected usage"),
        }
    }

    #[test]
    fn body_maps_roles_and_system() {
        let msgs = vec![
            Message::system("be terse"),
            Message::user("hello"),
            Message::assistant_tool_calls(
                None,
                vec![ToolCall {
                    id: "x".into(),
                    name: "Bash".into(),
                    arguments: json!({"cmd":"ls"}),
                }],
            ),
            Message::tool_result("x".into(), "Bash", "a.txt"),
        ];
        let body = build_body(&msgs, &[], &LlmOptions::default());
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be terse");
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[0]["role"], "user"); // hello
        assert_eq!(contents[1]["role"], "model"); // assistant functionCall
        assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "Bash");
        assert_eq!(contents[2]["role"], "user"); // tool result
        assert_eq!(contents[2]["parts"][0]["functionResponse"]["name"], "Bash");
    }

    #[test]
    fn schema_is_sanitized() {
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "additionalProperties": false,
            "title": "Foo",
            "properties": { "cmd": { "type": "string", "title": "Cmd" } }
        });
        let s = sanitize_schema(&schema);
        assert!(s.get("$schema").is_none());
        assert!(s.get("additionalProperties").is_none());
        assert!(s.get("title").is_none());
        assert!(s["properties"]["cmd"].get("title").is_none());
        assert_eq!(s["type"], "object");
        assert_eq!(s["properties"]["cmd"]["type"], "string");
    }
}

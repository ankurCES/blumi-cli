//! Conversation messages and their content parts.

use crate::ids::ToolCallId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Who authored a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// How an image's bytes are carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageData {
    /// Base64-encoded bytes (no data: prefix).
    Base64 { data: String },
    /// A URL the provider can fetch.
    Url { url: String },
}

/// A reference to an image, used for multimodal input and tool results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    /// MIME type, e.g. `image/png`.
    pub media_type: String,
    #[serde(flatten)]
    pub data: ImageData,
}

/// A single piece of message content. A message may contain several parts
/// (e.g. text plus one or more images).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image(ImageRef),
}

impl ContentPart {
    pub fn text(s: impl Into<String>) -> Self {
        ContentPart::Text { text: s.into() }
    }
}

/// A tool invocation requested by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    /// Parsed arguments. (Providers stream these as a JSON *string* fragment;
    /// the streaming accumulator parses them into a value before this is built.)
    pub arguments: serde_json::Value,
}

/// One message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(default)]
    pub content: Vec<ContentPart>,
    /// Tool calls emitted by an assistant message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For `Role::Tool` messages: which call this is the result of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<ToolCallId>,
    /// For `Role::Tool` messages: the tool's name (convenience for rendering).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
}

impl Message {
    fn new(role: Role, content: Vec<ContentPart>) -> Self {
        Message {
            role,
            content,
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            timestamp: OffsetDateTime::now_utc(),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self::new(Role::System, vec![ContentPart::text(text)])
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, vec![ContentPart::text(text)])
    }

    pub fn user_parts(parts: Vec<ContentPart>) -> Self {
        Self::new(Role::User, parts)
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, vec![ContentPart::text(text)])
    }

    /// An assistant message that only carries tool calls (no visible text).
    pub fn assistant_tool_calls(text: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        let content = text.map(|t| vec![ContentPart::text(t)]).unwrap_or_default();
        let mut m = Self::new(Role::Assistant, content);
        m.tool_calls = tool_calls;
        m
    }

    /// A tool-result message answering a specific tool call.
    pub fn tool_result(
        call_id: ToolCallId,
        tool_name: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        let mut m = Self::new(Role::Tool, vec![ContentPart::text(text)]);
        m.tool_call_id = Some(call_id);
        m.tool_name = Some(tool_name.into());
        m
    }

    /// Concatenated text of all text parts (ignores images).
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image(_) => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_round_trips() {
        let m = Message::user("hello");
        let json = serde_json::to_string(&m).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
        assert_eq!(back.text(), "hello");
    }

    #[test]
    fn tool_result_carries_metadata() {
        let m = Message::tool_result(ToolCallId::from("call_1"), "Bash", "ok");
        assert_eq!(m.role, Role::Tool);
        assert_eq!(m.tool_call_id.as_ref().unwrap().as_str(), "call_1");
        assert_eq!(m.tool_name.as_deref(), Some("Bash"));
    }

    #[test]
    fn image_ref_flattens() {
        let img = ImageRef {
            media_type: "image/png".into(),
            data: ImageData::Base64 { data: "AAAA".into() },
        };
        let v = serde_json::to_value(&img).unwrap();
        assert_eq!(v["media_type"], "image/png");
        assert_eq!(v["kind"], "base64");
        assert_eq!(v["data"], "AAAA");
    }
}

//! Strongly-typed, string-backed identifiers.
//!
//! Each is a transparent newtype over `String` so it serializes as a plain
//! string but cannot be accidentally swapped for a different kind of id.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Generate a fresh, prefixed, random id (`<prefix>_<uuid-no-dashes>`).
            pub fn new() -> Self {
                Self(format!(
                    "{}_{}",
                    $prefix,
                    uuid::Uuid::new_v4().simple()
                ))
            }

            /// Wrap an existing string without generating a new value.
            pub fn from_string(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

string_id!(
    /// Identifies a conversation/session.
    SessionId, "sess"
);
string_id!(
    /// Identifies a single message within a session.
    MessageId, "msg"
);
string_id!(
    /// Identifies a tool call requested by the model.
    ToolCallId, "call"
);
string_id!(
    /// Identifies a pending interaction (approval or clarification) awaiting a reply.
    RequestId, "req"
);
string_id!(
    /// Identifies one streaming turn, minted when a turn starts. Used for SSE
    /// stream ownership in the web server.
    StreamId, "strm"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_prefixed_and_unique() {
        let a = SessionId::new();
        let b = SessionId::new();
        assert!(a.as_str().starts_with("sess_"));
        assert_ne!(a, b);
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = ToolCallId::from("call_abc");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"call_abc\"");
        let back: ToolCallId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}

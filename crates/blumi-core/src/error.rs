//! Error types for the core's three extension points.

/// An error from executing a tool. Note that *expected* failures (bad input,
/// permission denied, etc.) are normally returned as a [`blumi_protocol::ToolResult`]
/// with a non-success class; `ToolError` is for unexpected/internal failures,
/// which the pipeline converts into a `Crash` result.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// An error from an LLM provider client.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("provider error ({status}): {message}")]
    Provider { status: u16, message: String },
    #[error("stream decode error: {0}")]
    Stream(String),
    #[error("request cancelled")]
    Cancelled,
    #[error("no provider configured")]
    NoProvider,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl LlmError {
    /// Whether retrying the same request might succeed (transport hiccups,
    /// 429s, 5xx). Used by the retry/backoff wrapper.
    pub fn is_retryable(&self) -> bool {
        match self {
            LlmError::Transport(_) | LlmError::Stream(_) => true,
            LlmError::Provider { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

/// An error from an execution backend.
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("io error: {0}")]
    Io(String),
    #[error("command timed out after {0:?}")]
    Timeout(std::time::Duration),
    #[error("backend unavailable: {0}")]
    Unavailable(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<std::io::Error> for ExecError {
    fn from(e: std::io::Error) -> Self {
        ExecError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification() {
        assert!(LlmError::Transport("reset".into()).is_retryable());
        assert!(LlmError::Provider {
            status: 503,
            message: "x".into()
        }
        .is_retryable());
        assert!(LlmError::Provider {
            status: 429,
            message: "x".into()
        }
        .is_retryable());
        assert!(!LlmError::Provider {
            status: 400,
            message: "x".into()
        }
        .is_retryable());
        assert!(!LlmError::Cancelled.is_retryable());
    }
}

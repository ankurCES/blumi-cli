//! Initial-request retry with exponential backoff, shared by provider clients.
//!
//! Only the *connection* is retried (network errors, 429, 5xx). Once a stream
//! is flowing, a mid-flight error is surfaced — replaying a partial completion
//! is unsafe.

use lumi_core::LlmError;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Backoff schedule between attempts (so up to 4 total tries).
const BACKOFF: [Duration; 3] =
    [Duration::from_secs(1), Duration::from_secs(4), Duration::from_secs(16)];

/// Send `request` (cloned per attempt), retrying transient failures. Cancels
/// promptly if `ct` fires. Returns the successful response or a classified
/// [`LlmError`].
pub async fn send_with_retry(
    request: reqwest::RequestBuilder,
    ct: &CancellationToken,
) -> Result<reqwest::Response, LlmError> {
    let mut attempt = 0usize;
    loop {
        let try_req = request
            .try_clone()
            .ok_or_else(|| LlmError::Transport("request not cloneable".into()))?;

        let send = try_req.send();
        let result = tokio::select! {
            _ = ct.cancelled() => return Err(LlmError::Cancelled),
            r = send => r,
        };

        match result {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                let status = resp.status().as_u16();
                let retryable = status == 429 || status >= 500;
                let message = resp.text().await.unwrap_or_default();
                if retryable && attempt < BACKOFF.len() {
                    wait(BACKOFF[attempt], ct).await?;
                    attempt += 1;
                    continue;
                }
                return Err(LlmError::Provider { status, message });
            }
            Err(e) => {
                if attempt < BACKOFF.len() {
                    wait(BACKOFF[attempt], ct).await?;
                    attempt += 1;
                    continue;
                }
                return Err(LlmError::Transport(e.to_string()));
            }
        }
    }
}

async fn wait(d: Duration, ct: &CancellationToken) -> Result<(), LlmError> {
    tokio::select! {
        _ = ct.cancelled() => Err(LlmError::Cancelled),
        _ = tokio::time::sleep(d) => Ok(()),
    }
}

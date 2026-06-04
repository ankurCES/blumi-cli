//! Grid dispatch client — the orchestrator side of task hand-off.
//!
//! When the orchestrator dispatches a task to a peer, it POSTs the task prompt
//! to the peer's `POST /api/grid/run` (authenticated with the shared grid secret
//! via the `X-Blumi-Grid` header). That endpoint runs the prompt as one turn on
//! the peer's own session/runtime and returns when the turn finishes, so this
//! client just makes one request and awaits the result — no SSE stream to parse.

use super::Peer;
use std::time::Duration;

/// A lean client for talking to one peer's grid surface.
pub struct Client {
    base: String,
    secret: String,
    http: reqwest::Client,
}

impl Client {
    pub fn for_peer(peer: &Peer, secret: &str) -> Self {
        Self {
            base: peer.base_url(),
            secret: secret.to_string(),
            http: reqwest::Client::builder()
                .user_agent("blumi-grid")
                .build()
                .unwrap_or_default(),
        }
    }

    /// Run `prompt` as one turn on the peer and block until it completes.
    /// `idle_timeout` bounds the whole request. Returns the peer's summary on
    /// success; `Err` on auth failure, a non-OK body, or a network/timeout error.
    pub async fn run_task(&self, prompt: String, idle_timeout: Duration) -> anyhow::Result<String> {
        let resp = self
            .http
            .post(format!("{}/api/grid/run", self.base))
            .header("x-blumi-grid", &self.secret)
            .json(&serde_json::json!({ "prompt": prompt }))
            .timeout(idle_timeout)
            .send()
            .await?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("peer returned HTTP {status}");
        }
        if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("peer reported failure");
            anyhow::bail!("{err}");
        }
        // Prefer the peer's full assistant output (used by grid overflow);
        // fall back to the short summary.
        let out = body
            .get("output")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| body.get("summary").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        Ok(out)
    }

    /// Fetch this peer's live metrics (`GET /api/grid/node`).
    pub async fn node_metrics(&self, timeout: Duration) -> anyhow::Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{}/api/grid/node", self.base))
            .header("x-blumi-grid", &self.secret)
            .timeout(timeout)
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("peer returned HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }
}

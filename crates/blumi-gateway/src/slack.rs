//! Slack transport (Socket Mode): open a WebSocket via `apps.connections.open`,
//! ACK each events-api envelope immediately (Slack requires an ack within ~3s),
//! then handle the message and reply with `chat.postMessage`.

use crate::{split_message, GatewayCore};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// A comfortable chunk size for Slack (its hard cap is far higher).
const MAX_MSG: usize = 3000;

/// Options for the Slack gateway (Socket Mode).
pub struct SlackOptions {
    /// Bot token (`xoxb-…`) used to post messages.
    pub bot_token: String,
    /// App-level token (`xapp-…`) used to open the Socket Mode connection.
    pub app_token: String,
}

/// Run the Slack gateway forever, reconnecting on disconnect.
pub async fn run_slack(core: Arc<GatewayCore>, opts: SlackOptions) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let opts = Arc::new(opts);
    loop {
        if let Err(e) = run_once(&core, &opts, &http).await {
            tracing::warn!("slack connection ended: {e}; reconnecting in 5s");
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn run_once(
    core: &Arc<GatewayCore>,
    opts: &Arc<SlackOptions>,
    http: &reqwest::Client,
) -> anyhow::Result<()> {
    let url = open_connection(http, &opts.app_token).await?;
    let (ws, _) = connect_async(url).await?;
    let (sink, mut stream) = ws.split();
    let sink = Arc::new(Mutex::new(sink));

    while let Some(frame) = stream.next().await {
        let v: Value = match frame? {
            WsMessage::Text(t) => serde_json::from_str(&t)?,
            WsMessage::Ping(p) => {
                sink.lock().await.send(WsMessage::Pong(p)).await?;
                continue;
            }
            WsMessage::Close(_) => break,
            _ => continue,
        };
        match v["type"].as_str() {
            Some("hello") => tracing::info!("slack socket mode connected"),
            // Slack asks us to reconnect (the socket is about to close).
            Some("disconnect") => break,
            Some("events_api") => {
                // ACK first (within Slack's deadline), then handle async.
                if let Some(env_id) = v["envelope_id"].as_str() {
                    sink.lock()
                        .await
                        .send(WsMessage::Text(
                            json!({ "envelope_id": env_id }).to_string(),
                        ))
                        .await?;
                }
                dispatch_event(&v["payload"]["event"], core, opts, http);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Open a Socket Mode WebSocket URL with the app-level token.
async fn open_connection(http: &reqwest::Client, app_token: &str) -> anyhow::Result<String> {
    let resp: Value = http
        .post("https://slack.com/api/apps.connections.open")
        .header("Authorization", format!("Bearer {app_token}"))
        .send()
        .await?
        .json()
        .await?;
    if resp["ok"].as_bool() != Some(true) {
        anyhow::bail!(
            "apps.connections.open failed: {}",
            resp["error"].as_str().unwrap_or("unknown")
        );
    }
    resp["url"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("no url in apps.connections.open response"))
}

/// Extract (channel, text) from a Slack event if it's a plain human message.
fn extract_event(event: &Value) -> Option<(String, String)> {
    if event["type"].as_str() != Some("message") {
        return None;
    }
    // Skip bot messages and any subtype (edits, joins, our own posts) to avoid
    // loops and noise.
    if event["bot_id"].is_string() || event["subtype"].is_string() {
        return None;
    }
    let channel = event["channel"].as_str()?.to_string();
    let text = event["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some((channel, text))
}

fn dispatch_event(
    event: &Value,
    core: &Arc<GatewayCore>,
    opts: &Arc<SlackOptions>,
    http: &reqwest::Client,
) {
    let Some((channel, text)) = extract_event(event) else {
        return;
    };
    let core = core.clone();
    let opts = opts.clone();
    let http = http.clone();
    tokio::spawn(async move {
        if text == "/reset" {
            core.reset(&channel).await;
            let _ = post_message(
                &http,
                &opts.bot_token,
                &channel,
                "context cleared — fresh start.",
            )
            .await;
            return;
        }
        let reply = core
            .handle(&channel, &text)
            .await
            .unwrap_or_else(|e| format!("⚠ {e}"));
        for chunk in split_message(&reply, MAX_MSG) {
            if let Err(e) = post_message(&http, &opts.bot_token, &channel, &chunk).await {
                tracing::warn!("slack postMessage failed: {e}");
                break;
            }
        }
    });
}

async fn post_message(
    http: &reqwest::Client,
    bot_token: &str,
    channel: &str,
    text: &str,
) -> anyhow::Result<()> {
    let resp: Value = http
        .post("https://slack.com/api/chat.postMessage")
        .header("Authorization", format!("Bearer {bot_token}"))
        .json(&json!({ "channel": channel, "text": text }))
        .send()
        .await?
        .json()
        .await?;
    if resp["ok"].as_bool() != Some(true) {
        anyhow::bail!(
            "chat.postMessage failed: {}",
            resp["error"].as_str().unwrap_or("unknown")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_message() {
        let e = json!({ "type": "message", "channel": "C1", "text": "hi there", "user": "U1" });
        assert_eq!(
            extract_event(&e),
            Some(("C1".to_string(), "hi there".to_string()))
        );
    }

    #[test]
    fn ignores_bot_and_subtypes() {
        let bot = json!({"type":"message","channel":"C1","text":"x","bot_id":"B1"});
        assert_eq!(extract_event(&bot), None);
        let edit = json!({"type":"message","channel":"C1","text":"x","subtype":"message_changed"});
        assert_eq!(extract_event(&edit), None);
        let other = json!({"type":"reaction_added","channel":"C1"});
        assert_eq!(extract_event(&other), None);
    }
}

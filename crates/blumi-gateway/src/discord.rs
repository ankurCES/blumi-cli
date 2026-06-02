//! Discord transport: connect to the Gateway WebSocket, identify, keep a
//! heartbeat alive (in its own task so long agent turns don't drop the
//! connection), and turn each MESSAGE_CREATE into a reply via the REST API.

use crate::{split_message, GatewayCore};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

/// Discord's per-message cap.
const MAX_MSG: usize = 2000;
const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
/// GUILDS | GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT.
const INTENTS: u64 = (1 << 0) | (1 << 9) | (1 << 12) | (1 << 15);
const API: &str = "https://discord.com/api/v10";

/// Options for the Discord gateway.
pub struct DiscordOptions {
    /// Bot token from the Discord developer portal.
    pub token: String,
    /// If non-empty, only these channel ids are served.
    pub allowed_channels: Vec<String>,
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Sink = Arc<Mutex<SplitSink<Ws, WsMessage>>>;

/// Run the Discord gateway forever, reconnecting on any drop.
pub async fn run_discord(core: Arc<GatewayCore>, opts: DiscordOptions) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let opts = Arc::new(opts);
    loop {
        if let Err(e) = run_once(&core, &opts, &http).await {
            tracing::warn!("discord connection ended: {e}; reconnecting in 5s");
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn run_once(
    core: &Arc<GatewayCore>,
    opts: &Arc<DiscordOptions>,
    http: &reqwest::Client,
) -> anyhow::Result<()> {
    let (ws, _) = connect_async(GATEWAY_URL).await?;
    let (sink, mut stream) = ws.split();
    let sink: Sink = Arc::new(Mutex::new(sink));

    // First frame is Hello (op 10) with the heartbeat interval.
    let hello = next_json(&mut stream)
        .await?
        .ok_or_else(|| anyhow::anyhow!("discord closed before Hello"))?;
    let interval = hello["d"]["heartbeat_interval"].as_u64().unwrap_or(45_000);

    // Identify.
    let identify = json!({
        "op": 2,
        "d": {
            "token": opts.token,
            "intents": INTENTS,
            "properties": { "os": "linux", "browser": "blumi", "device": "blumi" }
        }
    });
    sink.lock()
        .await
        .send(WsMessage::Text(identify.to_string()))
        .await?;

    // Heartbeat task: op 1 with the last sequence, every `interval` ms.
    let seq: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let hb = {
        let sink = sink.clone();
        let seq = seq.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(interval));
            loop {
                tick.tick().await;
                let s = *seq.lock().await;
                if sink
                    .lock()
                    .await
                    .send(WsMessage::Text(json!({ "op": 1, "d": s }).to_string()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        })
    };

    let result = read_loop(&mut stream, &sink, &seq, core, opts, http).await;
    hb.abort();
    result
}

async fn read_loop(
    stream: &mut SplitStream<Ws>,
    sink: &Sink,
    seq: &Arc<Mutex<Option<u64>>>,
    core: &Arc<GatewayCore>,
    opts: &Arc<DiscordOptions>,
    http: &reqwest::Client,
) -> anyhow::Result<()> {
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
        if let Some(s) = v["s"].as_u64() {
            *seq.lock().await = Some(s);
        }
        match v["op"].as_u64() {
            Some(0) if v["t"] == "MESSAGE_CREATE" => dispatch_message(&v["d"], core, opts, http),
            // The server asked for an immediate heartbeat.
            Some(1) => {
                let s = *seq.lock().await;
                sink.lock()
                    .await
                    .send(WsMessage::Text(json!({ "op": 1, "d": s }).to_string()))
                    .await?;
            }
            // Reconnect / invalid session → drop and let the outer loop redial.
            Some(7) | Some(9) => break,
            _ => {}
        }
    }
    Ok(())
}

/// Extract (channel_id, content) from a MESSAGE_CREATE if it's a human message
/// we should answer.
fn extract_message(d: &Value, allowed: &[String]) -> Option<(String, String)> {
    if d["author"]["bot"].as_bool().unwrap_or(false) {
        return None; // ignore bots (incl. ourselves) to avoid loops
    }
    let channel_id = d["channel_id"].as_str()?.to_string();
    let content = d["content"].as_str().unwrap_or("").trim().to_string();
    if content.is_empty() {
        return None;
    }
    if !allowed.is_empty() && !allowed.contains(&channel_id) {
        return None;
    }
    Some((channel_id, content))
}

/// Handle one message in its own task so the read loop (and heartbeat) keep going.
fn dispatch_message(
    d: &Value,
    core: &Arc<GatewayCore>,
    opts: &Arc<DiscordOptions>,
    http: &reqwest::Client,
) {
    let Some((channel_id, content)) = extract_message(d, &opts.allowed_channels) else {
        return;
    };
    let core = core.clone();
    let opts = opts.clone();
    let http = http.clone();
    tokio::spawn(async move {
        if content == "/reset" {
            core.reset(&channel_id).await;
            let _ = send_message(
                &http,
                &opts.token,
                &channel_id,
                "context cleared — fresh start.",
            )
            .await;
            return;
        }
        let typing = {
            let http = http.clone();
            let token = opts.token.clone();
            let channel_id = channel_id.clone();
            tokio::spawn(async move {
                loop {
                    let _ = send_typing(&http, &token, &channel_id).await;
                    tokio::time::sleep(Duration::from_secs(8)).await;
                }
            })
        };
        let reply = core
            .handle(&channel_id, &content)
            .await
            .unwrap_or_else(|e| format!("⚠ {e}"));
        typing.abort();
        for chunk in split_message(&reply, MAX_MSG) {
            if let Err(e) = send_message(&http, &opts.token, &channel_id, &chunk).await {
                tracing::warn!("discord send failed: {e}");
                break;
            }
        }
    });
}

async fn next_json(stream: &mut SplitStream<Ws>) -> anyhow::Result<Option<Value>> {
    while let Some(frame) = stream.next().await {
        if let WsMessage::Text(t) = frame? {
            return Ok(Some(serde_json::from_str(&t)?));
        }
    }
    Ok(None)
}

async fn send_message(
    http: &reqwest::Client,
    token: &str,
    channel_id: &str,
    text: &str,
) -> anyhow::Result<()> {
    http.post(format!("{API}/channels/{channel_id}/messages"))
        .header("Authorization", format!("Bot {token}"))
        .json(&json!({ "content": text }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn send_typing(http: &reqwest::Client, token: &str, channel_id: &str) -> anyhow::Result<()> {
    http.post(format!("{API}/channels/{channel_id}/typing"))
        .header("Authorization", format!("Bot {token}"))
        .send()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_include_message_content() {
        assert_eq!(INTENTS & (1 << 15), 1 << 15); // MESSAGE_CONTENT
        assert_eq!(INTENTS & (1 << 9), 1 << 9); // GUILD_MESSAGES
    }

    #[test]
    fn extracts_human_message() {
        let d = json!({
            "channel_id": "123",
            "content": "hello bot",
            "author": { "bot": false }
        });
        assert_eq!(
            extract_message(&d, &[]),
            Some(("123".to_string(), "hello bot".to_string()))
        );
    }

    #[test]
    fn ignores_bots_and_empty() {
        let bot = json!({"channel_id":"1","content":"hi","author":{"bot":true}});
        assert_eq!(extract_message(&bot, &[]), None);
        let empty = json!({"channel_id":"1","content":"   ","author":{"bot":false}});
        assert_eq!(extract_message(&empty, &[]), None);
    }

    #[test]
    fn respects_channel_allow_list() {
        let d = json!({"channel_id":"42","content":"hi","author":{"bot":false}});
        assert!(extract_message(&d, &["99".to_string()]).is_none());
        assert!(extract_message(&d, &["42".to_string()]).is_some());
    }
}

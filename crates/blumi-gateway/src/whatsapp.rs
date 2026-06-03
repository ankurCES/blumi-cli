//! WhatsApp transport (Meta Cloud API): an inbound webhook server (GET to verify
//! the subscription, POST for messages) + outbound sends via the Graph API.
//!
//! Unlike the polling/socket gateways, WhatsApp pushes to a public URL, so this
//! runs a small HTTP server. Point Meta's webhook at `https://<host>/webhook`
//! (e.g. via a tunnel) with the same verify token.

use crate::{split_message, GatewayCore};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

/// WhatsApp's per-message text cap.
const MAX_MSG: usize = 4096;
const GRAPH: &str = "https://graph.facebook.com/v21.0";

/// Options for the WhatsApp gateway.
pub struct WhatsappOptions {
    /// Permanent access token for the Cloud API.
    pub token: String,
    /// Phone-number id to send from.
    pub phone_number_id: String,
    /// Token used to verify the webhook subscription (you choose this).
    pub verify_token: String,
    /// Port the inbound webhook server listens on.
    pub port: u16,
}

struct WaState {
    core: Arc<GatewayCore>,
    http: reqwest::Client,
    opts: WhatsappOptions,
}

/// Run the WhatsApp webhook server (until the process is stopped).
pub async fn run_whatsapp(core: Arc<GatewayCore>, opts: WhatsappOptions) -> anyhow::Result<()> {
    let port = opts.port;
    let state = Arc::new(WaState {
        core,
        http: reqwest::Client::new(),
        opts,
    });
    let app = Router::new()
        .route("/webhook", get(verify).post(incoming))
        .with_state(state);

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("whatsapp webhook listening on http://{addr}/webhook");
    axum::serve(listener, app).await?;
    Ok(())
}

/// GET /webhook — Meta's subscription handshake: echo `hub.challenge` when the
/// mode is `subscribe` and the verify token matches.
async fn verify(
    State(state): State<Arc<WaState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mode = params.get("hub.mode").map(String::as_str);
    let token = params.get("hub.verify_token").map(String::as_str);
    let challenge = params.get("hub.challenge").cloned().unwrap_or_default();
    if mode == Some("subscribe") && token == Some(state.opts.verify_token.as_str()) {
        (axum::http::StatusCode::OK, challenge)
    } else {
        (axum::http::StatusCode::FORBIDDEN, String::new())
    }
}

/// An inbound message: text, or an audio note (media id) to transcribe.
#[derive(Debug, PartialEq)]
struct Inbound {
    from: String,
    text: Option<String>,
    audio_id: Option<String>,
}

/// POST /webhook — message notifications. Ack 200 immediately, handle async.
async fn incoming(State(state): State<Arc<WaState>>, body: axum::body::Bytes) -> impl IntoResponse {
    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        for msg in extract_messages(&v) {
            let state = state.clone();
            tokio::spawn(async move { handle(state, msg).await });
        }
    }
    (axum::http::StatusCode::OK, "EVENT_RECEIVED")
}

async fn handle(state: Arc<WaState>, inbound: Inbound) {
    let from = inbound.from;
    if inbound.text.as_deref() == Some("/reset") {
        state.core.reset(&from).await;
        let _ = send_text(&state, &from, "context cleared — fresh start.").await;
        return;
    }

    // Text directly, or transcribe an inbound audio note.
    let text = if let Some(t) = inbound.text {
        t
    } else if let Some(media_id) = inbound.audio_id {
        match transcribe_audio(&state, &media_id).await {
            Ok(t) => {
                let _ = send_text(&state, &from, &format!("🎙 “{t}”")).await;
                t
            }
            Err(e) => {
                let _ = send_text(&state, &from, &format!("⚠ couldn't transcribe: {e}")).await;
                return;
            }
        }
    } else {
        return;
    };
    if text.is_empty() {
        return;
    }

    let reply = state
        .core
        .handle(&from, &text)
        .await
        .unwrap_or_else(|e| format!("⚠ {e}"));
    for chunk in split_message(&reply, MAX_MSG) {
        if let Err(e) = send_text(&state, &from, &chunk).await {
            tracing::warn!("whatsapp send failed: {e}");
            break;
        }
    }

    // Speak the reply too, if TTS is configured.
    if let Some(audio) = state.core.synthesize(&reply).await {
        if let Err(e) = send_audio(&state, &from, audio).await {
            tracing::warn!("whatsapp send audio failed: {e}");
        }
    }
}

/// Download a media object (2 steps: resolve URL, then fetch) and transcribe it.
async fn transcribe_audio(state: &WaState, media_id: &str) -> anyhow::Result<String> {
    let meta: Value = state
        .http
        .get(format!("{GRAPH}/{media_id}"))
        .bearer_auth(&state.opts.token)
        .send()
        .await?
        .json()
        .await?;
    let url = meta["url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no media url"))?;
    let mime = meta["mime_type"]
        .as_str()
        .unwrap_or("audio/ogg")
        .to_string();
    let bytes = state
        .http
        .get(url)
        .bearer_auth(&state.opts.token)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();
    let ext = ext_from_mime(&mime);
    state
        .core
        .transcribe(bytes, &format!("audio.{ext}"), &mime)
        .await
}

/// Upload synthesized speech, then send it as an audio message (2 steps).
async fn send_audio(state: &WaState, to: &str, audio: Vec<u8>) -> anyhow::Result<()> {
    let part = reqwest::multipart::Part::bytes(audio)
        .file_name("reply.mp3")
        .mime_str("audio/mpeg")?;
    let form = reqwest::multipart::Form::new()
        .text("messaging_product", "whatsapp")
        .part("file", part);
    let up: Value = state
        .http
        .post(format!("{GRAPH}/{}/media", state.opts.phone_number_id))
        .bearer_auth(&state.opts.token)
        .multipart(form)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let media_id = up["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("media upload returned no id"))?;
    state
        .http
        .post(format!("{GRAPH}/{}/messages", state.opts.phone_number_id))
        .bearer_auth(&state.opts.token)
        .json(&json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "audio",
            "audio": { "id": media_id }
        }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Best-effort file extension from a mime type (`audio/ogg; codecs=opus` → `ogg`).
fn ext_from_mime(mime: &str) -> &str {
    let base = mime.split(';').next().unwrap_or("");
    match base.split_once('/') {
        Some((_, ext)) if !ext.is_empty() => ext,
        _ => "ogg",
    }
}

/// Pull inbound messages (text or audio) out of a webhook payload, skipping
/// status updates and other types.
fn extract_messages(body: &Value) -> Vec<Inbound> {
    let mut out = Vec::new();
    for entry in body["entry"].as_array().into_iter().flatten() {
        for change in entry["changes"].as_array().into_iter().flatten() {
            for msg in change["value"]["messages"].as_array().into_iter().flatten() {
                let Some(from) = msg["from"].as_str() else {
                    continue;
                };
                match msg["type"].as_str() {
                    Some("text") => {
                        let text = msg["text"]["body"].as_str().unwrap_or("").trim();
                        if !text.is_empty() {
                            out.push(Inbound {
                                from: from.to_string(),
                                text: Some(text.to_string()),
                                audio_id: None,
                            });
                        }
                    }
                    // Voice notes and uploaded audio both arrive as "audio".
                    Some("audio") => {
                        if let Some(id) = msg["audio"]["id"].as_str() {
                            out.push(Inbound {
                                from: from.to_string(),
                                text: None,
                                audio_id: Some(id.to_string()),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

async fn send_text(state: &WaState, to: &str, text: &str) -> anyhow::Result<()> {
    state
        .http
        .post(format!("{GRAPH}/{}/messages", state.opts.phone_number_id))
        .header("Authorization", format!("Bearer {}", state.opts.token))
        .json(&json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "text",
            "text": { "body": text }
        }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_messages() {
        let body = json!({
            "entry": [{
                "changes": [{
                    "value": {
                        "messages": [
                            { "from": "15551234567", "type": "text", "text": { "body": "hello" } }
                        ]
                    }
                }]
            }]
        });
        let msgs = extract_messages(&body);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].from, "15551234567");
        assert_eq!(msgs[0].text.as_deref(), Some("hello"));
        assert!(msgs[0].audio_id.is_none());
    }

    #[test]
    fn extracts_audio_messages() {
        let body = json!({
            "entry": [{ "changes": [{ "value": { "messages": [
                { "from": "1", "type": "audio", "audio": { "id": "media-123", "mime_type": "audio/ogg; codecs=opus" } }
            ] } }] }]
        });
        let msgs = extract_messages(&body);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].audio_id.as_deref(), Some("media-123"));
        assert!(msgs[0].text.is_none());
    }

    #[test]
    fn skips_statuses_and_other_types() {
        let status = json!({
            "entry": [{ "changes": [{ "value": { "statuses": [{ "status": "delivered" }] } }] }]
        });
        assert!(extract_messages(&status).is_empty());

        let image = json!({
            "entry": [{ "changes": [{ "value": { "messages": [
                { "from": "1", "type": "image", "image": { "id": "x" } }
            ] } }] }]
        });
        assert!(extract_messages(&image).is_empty());
    }

    #[test]
    fn mime_to_extension() {
        assert_eq!(ext_from_mime("audio/ogg; codecs=opus"), "ogg");
        assert_eq!(ext_from_mime("audio/mpeg"), "mpeg");
        assert_eq!(ext_from_mime("garbage"), "ogg");
    }
}

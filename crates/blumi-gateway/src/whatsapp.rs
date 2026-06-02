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

/// POST /webhook — message notifications. Ack 200 immediately, handle async.
async fn incoming(State(state): State<Arc<WaState>>, body: axum::body::Bytes) -> impl IntoResponse {
    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        for (from, text) in extract_messages(&v) {
            let state = state.clone();
            tokio::spawn(async move { handle(state, from, text).await });
        }
    }
    (axum::http::StatusCode::OK, "EVENT_RECEIVED")
}

async fn handle(state: Arc<WaState>, from: String, text: String) {
    if text == "/reset" {
        state.core.reset(&from).await;
        let _ = send_text(&state, &from, "context cleared — fresh start.").await;
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
}

/// Pull (sender, text) out of a webhook payload, skipping status updates and
/// non-text messages.
fn extract_messages(body: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in body["entry"].as_array().into_iter().flatten() {
        for change in entry["changes"].as_array().into_iter().flatten() {
            for msg in change["value"]["messages"].as_array().into_iter().flatten() {
                if msg["type"].as_str() != Some("text") {
                    continue;
                }
                let (Some(from), Some(text)) = (msg["from"].as_str(), msg["text"]["body"].as_str())
                else {
                    continue;
                };
                let text = text.trim();
                if !text.is_empty() {
                    out.push((from.to_string(), text.to_string()));
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
        assert_eq!(
            extract_messages(&body),
            vec![("15551234567".to_string(), "hello".to_string())]
        );
    }

    #[test]
    fn skips_statuses_and_non_text() {
        // A delivery-status callback has no `messages` array.
        let status = json!({
            "entry": [{ "changes": [{ "value": { "statuses": [{ "status": "delivered" }] } }] }]
        });
        assert!(extract_messages(&status).is_empty());

        // Non-text (image) is skipped.
        let image = json!({
            "entry": [{ "changes": [{ "value": { "messages": [
                { "from": "1", "type": "image", "image": { "id": "x" } }
            ] } }] }]
        });
        assert!(extract_messages(&image).is_empty());
    }
}

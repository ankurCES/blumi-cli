//! Completion notifications — the *server-side push* half of #209.
//!
//! When an autonomous run finishes (`blumi loop` or always-on discovery), fan
//! out a short message to the channels enabled in `notify.*`:
//!
//! - **desktop** — an OS notification on the box blumi runs on (macOS
//!   `osascript` / Linux `notify-send`).
//! - **bot** — a proactive message to a configured gateway transport
//!   (Telegram / Discord / Slack / WhatsApp), reusing the `gateway.*` creds.
//! - **web push** — to subscribed browsers via Web Push (wired in a later pass).
//!
//! The *browser in-tab alert* and the *blugo phone notification* don't go
//! through here — they ride the live event stream ([`blumi_protocol::Event`])
//! that those clients already consume. This module is what reaches you when no
//! client is open.
//!
//! Everything is **best-effort**: a channel that fails is logged, never fatal.

use blumi_config::{BlumiConfig, NotifyBot};

/// A built gateway-bot HTTP request: where to POST, an optional `Authorization`
/// header value, and the JSON body. Returned by [`build_bot_request`] so the
/// transport mapping can be unit-tested without doing any I/O.
#[derive(Debug, Clone, PartialEq)]
pub struct BotRequest {
    pub url: String,
    /// Value for the `Authorization` header, if the transport needs one.
    pub auth: Option<String>,
    pub body: serde_json::Value,
}

/// Map a [`NotifyBot`] + the gateway creds to an HTTP request, mirroring each
/// transport's existing send path. Returns `None` when the transport is unknown
/// or its credentials / target are missing (so a half-configured bot is a no-op,
/// never an error). Pure — no network, no clock.
pub fn build_bot_request(cfg: &BlumiConfig, bot: &NotifyBot, text: &str) -> Option<BotRequest> {
    let g = &cfg.gateway;
    let target = bot.target.trim();
    if target.is_empty() {
        return None;
    }
    match bot.transport.trim() {
        "telegram" => {
            let token = g.telegram.token.trim();
            if token.is_empty() {
                return None;
            }
            let chat_id: i64 = target.parse().ok()?;
            Some(BotRequest {
                url: format!("https://api.telegram.org/bot{token}/sendMessage"),
                auth: None,
                body: serde_json::json!({ "chat_id": chat_id, "text": text }),
            })
        }
        "discord" => {
            let token = g.discord.token.trim();
            if token.is_empty() {
                return None;
            }
            Some(BotRequest {
                url: format!("https://discord.com/api/v10/channels/{target}/messages"),
                auth: Some(format!("Bot {token}")),
                body: serde_json::json!({ "content": text }),
            })
        }
        "slack" => {
            let token = g.slack.bot_token.trim();
            if token.is_empty() {
                return None;
            }
            Some(BotRequest {
                url: "https://slack.com/api/chat.postMessage".to_string(),
                auth: Some(format!("Bearer {token}")),
                body: serde_json::json!({ "channel": target, "text": text }),
            })
        }
        "whatsapp" => {
            let token = g.whatsapp.token.trim();
            let pnid = g.whatsapp.phone_number_id.trim();
            if token.is_empty() || pnid.is_empty() {
                return None;
            }
            Some(BotRequest {
                url: format!("https://graph.facebook.com/v21.0/{pnid}/messages"),
                auth: Some(format!("Bearer {token}")),
                body: serde_json::json!({
                    "messaging_product": "whatsapp",
                    "to": target,
                    "type": "text",
                    "text": { "body": text }
                }),
            })
        }
        _ => None,
    }
}

/// Fan out a completion notification per `cfg.notify`.
///
/// `kind` is the trigger (`"loop"` / `"discovery"` / `"turn"`); it's matched
/// against `notify.on`. `force_desktop` fires the desktop channel regardless of
/// config — that's the `blumi loop --notify` flag, which is an explicit one-off
/// even when proactive notifications are otherwise off.
pub async fn notify_completion(
    cfg: &BlumiConfig,
    kind: &str,
    title: &str,
    body: &str,
    force_desktop: bool,
) {
    let n = &cfg.notify;
    let gated = n.enabled && n.fires(kind);

    if force_desktop || (gated && n.desktop) {
        desktop(title, body);
    }
    if !gated {
        return;
    }
    if let Some(bot) = &n.bot {
        let text = if body.is_empty() {
            title.to_string()
        } else {
            format!("{title}\n{body}")
        };
        if let Some(req) = build_bot_request(cfg, bot, &text) {
            send_bot(req).await;
        }
    }
    // Web push (VAPID) fans out from the gateway's subscription store — wired in
    // a later sub-phase; `notify.web_push` is read there.
}

/// POST a built bot request (best-effort).
async fn send_bot(req: BotRequest) {
    let client = reqwest::Client::new();
    let mut rb = client.post(&req.url).json(&req.body);
    if let Some(auth) = &req.auth {
        rb = rb.header("Authorization", auth);
    }
    match rb.send().await {
        Ok(r) if !r.status().is_success() => {
            tracing::warn!("notify bot POST failed: HTTP {}", r.status());
        }
        Err(e) => tracing::warn!("notify bot POST error: {e}"),
        _ => {}
    }
}

/// Best-effort OS desktop notification (macOS `osascript`, Linux `notify-send`).
pub fn desktop(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    let cmd = (
        "osascript",
        vec![
            "-e".to_string(),
            format!("display notification {body:?} with title {title:?}"),
        ],
    );
    #[cfg(target_os = "linux")]
    let cmd = ("notify-send", vec![title.to_string(), body.to_string()]);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let cmd: (&str, Vec<String>) = ("true", vec![]);

    let _ = std::process::Command::new(cmd.0)
        .args(cmd.1)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use blumi_config::BlumiConfig;

    fn cfg_with_gateway() -> BlumiConfig {
        let mut c = BlumiConfig::default();
        c.gateway.telegram.token = "TG123".into();
        c.gateway.discord.token = "DS123".into();
        c.gateway.slack.bot_token = "xoxb-1".into();
        c.gateway.whatsapp.token = "WA123".into();
        c.gateway.whatsapp.phone_number_id = "5551234".into();
        c
    }

    fn bot(transport: &str, target: &str) -> NotifyBot {
        NotifyBot {
            transport: transport.into(),
            target: target.into(),
        }
    }

    #[test]
    fn telegram_request_has_chat_id_and_no_auth() {
        let c = cfg_with_gateway();
        let r = build_bot_request(&c, &bot("telegram", "777"), "hi").unwrap();
        assert_eq!(r.url, "https://api.telegram.org/botTG123/sendMessage");
        assert!(r.auth.is_none());
        assert_eq!(r.body["chat_id"], 777);
        assert_eq!(r.body["text"], "hi");
    }

    #[test]
    fn discord_request_uses_bot_auth_and_channel_url() {
        let c = cfg_with_gateway();
        let r = build_bot_request(&c, &bot("discord", "42"), "hi").unwrap();
        assert_eq!(r.url, "https://discord.com/api/v10/channels/42/messages");
        assert_eq!(r.auth.as_deref(), Some("Bot DS123"));
        assert_eq!(r.body["content"], "hi");
    }

    #[test]
    fn slack_request_uses_bearer_auth() {
        let c = cfg_with_gateway();
        let r = build_bot_request(&c, &bot("slack", "C1"), "hi").unwrap();
        assert_eq!(r.url, "https://slack.com/api/chat.postMessage");
        assert_eq!(r.auth.as_deref(), Some("Bearer xoxb-1"));
        assert_eq!(r.body["channel"], "C1");
    }

    #[test]
    fn whatsapp_request_targets_phone_number_id() {
        let c = cfg_with_gateway();
        let r = build_bot_request(&c, &bot("whatsapp", "15551230000"), "hi").unwrap();
        assert_eq!(r.url, "https://graph.facebook.com/v21.0/5551234/messages");
        assert_eq!(r.auth.as_deref(), Some("Bearer WA123"));
        assert_eq!(r.body["text"]["body"], "hi");
        assert_eq!(r.body["to"], "15551230000");
    }

    #[test]
    fn missing_token_or_target_or_unknown_transport_is_none() {
        let c = cfg_with_gateway();
        // Unknown transport.
        assert!(build_bot_request(&c, &bot("signal", "x"), "hi").is_none());
        // Empty target.
        assert!(build_bot_request(&c, &bot("telegram", ""), "hi").is_none());
        // Missing credential.
        let blank = BlumiConfig::default();
        assert!(build_bot_request(&blank, &bot("telegram", "777"), "hi").is_none());
        // Non-numeric telegram chat id.
        assert!(build_bot_request(&c, &bot("telegram", "notanid"), "hi").is_none());
    }

    #[test]
    fn fires_default_set_and_explicit_on() {
        let mut n = blumi_config::NotifyConfig::default();
        assert!(n.fires("loop"));
        assert!(n.fires("discovery"));
        assert!(!n.fires("turn"));
        n.on = vec!["turn".into()];
        assert!(n.fires("turn"));
        assert!(!n.fires("loop"));
    }
}

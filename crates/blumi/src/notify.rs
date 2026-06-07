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
use std::path::{Path, PathBuf};

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
    if n.web_push {
        send_web_push(cfg, title, body).await;
    }
    // Phone push (blugo) via FCM — fires when the service account is present.
    if fcm_sa_path(cfg).is_some() {
        send_fcm(cfg, title, body, None).await;
    }
}

/// Push an interactive **turn** completion to the blugo phone(s) via FCM.
///
/// Unlike [`notify_completion`], this is gated **only** on the FCM service-account
/// file existing (it's the phone dispatch channel) — it ignores `notify.enabled` /
/// `notify.on` and never touches desktop/bot, so dispatch push works out of the
/// box with zero config and adds no desktop/bot noise. `data` (e.g. `session_id`,
/// `node`) rides along so a notification tap can open the right thread.
pub async fn notify_turn(cfg: &BlumiConfig, title: &str, body: &str, data: serde_json::Value) {
    if fcm_sa_path(cfg).is_some() {
        send_fcm(cfg, title, body, Some(data)).await;
    }
}

/// VAPID `sub` contact (a `mailto:`/`https:` URL, per RFC 8292). A generic
/// placeholder — never the user's real email, to avoid leaking it to push hosts.
const WEB_PUSH_CONTACT: &str = "mailto:notify@blumi.local";

/// Push to every subscribed browser via Web Push (VAPID). Only reaches browsers
/// served over a secure context (HTTPS / `http://localhost`); on a plain-HTTP LAN
/// there are simply no subscriptions. Prunes subscriptions a push service reports
/// as gone (404/410). Best-effort.
async fn send_web_push(cfg: &BlumiConfig, title: &str, body: &str) {
    let path = cfg.paths.push_store();
    let store = match blumi_core::push::load_or_init(&path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("web push: store init failed: {e}");
            return;
        }
    };
    if store.subscriptions.is_empty() {
        return;
    }
    let payload = serde_json::json!({ "title": title, "body": body })
        .to_string()
        .into_bytes();
    let client = reqwest::Client::new();
    let mut dead = Vec::new();
    for sub in &store.subscriptions {
        let req = match blumi_core::push::build_push_request(
            &store.vapid_private,
            WEB_PUSH_CONTACT,
            sub,
            &payload,
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("web push: build failed: {e}");
                continue;
            }
        };
        let mut rb = client.post(&req.url).body(req.body);
        for (k, v) in &req.headers {
            rb = rb.header(k, v);
        }
        match rb.send().await {
            Ok(r) => {
                let code = r.status().as_u16();
                if code == 404 || code == 410 {
                    dead.push(sub.endpoint.clone()); // subscription expired/gone
                } else if !r.status().is_success() {
                    tracing::warn!("web push: HTTP {code}");
                }
            }
            Err(e) => tracing::warn!("web push: send error: {e}"),
        }
    }
    for endpoint in dead {
        let _ = blumi_core::push::remove_subscription(&path, &endpoint);
    }
}

// --- FCM (blugo phone push, HTTP v1) ---------------------------------------

/// The Firebase service-account fields we need to mint an access token.
#[derive(serde::Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    token_uri: String,
    project_id: String,
}

/// Resolve the service-account path (config override, else the default
/// `~/.blumi/fcm-service-account.json`), returning `Some` only if it exists.
/// Its presence is what turns FCM push on — no settings flag.
fn fcm_sa_path(cfg: &BlumiConfig) -> Option<PathBuf> {
    let over = cfg.notify.fcm.service_account_path.trim();
    let p = if over.is_empty() {
        cfg.paths.fcm_service_account()
    } else {
        PathBuf::from(over)
    };
    p.exists().then_some(p)
}

fn load_sa(path: &Path) -> Option<ServiceAccount> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Truncate a notification body to a sane heads-up length.
fn preview(s: &str) -> String {
    const MAX: usize = 140;
    let s = s.trim();
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let head: String = s.chars().take(MAX).collect();
        format!("{}…", head.trim_end())
    }
}

/// FCM v1 requires `data` to be a flat string→string map. Coerce each top-level
/// value to a string (strings as-is, everything else via its JSON form).
fn stringify_values(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, val) in map {
                let s = match val {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null => continue,
                    other => other.to_string(),
                };
                out.insert(k, serde_json::Value::String(s));
            }
            serde_json::Value::Object(out)
        }
        _ => serde_json::json!({}),
    }
}

/// Process-wide cache of the current FCM OAuth2 access token + its expiry (unix
/// secs). The token is valid ~1h; reuse it instead of minting a JWT per push.
fn fcm_token_cache() -> &'static std::sync::Mutex<Option<(String, u64)>> {
    static C: std::sync::OnceLock<std::sync::Mutex<Option<(String, u64)>>> =
        std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(None))
}

/// Mint (or reuse) a Google OAuth2 access token for FCM via the service-account
/// JWT-bearer flow. Best-effort: `None` on any failure (logged, never fatal).
async fn fcm_access_token(sa: &ServiceAccount) -> Option<String> {
    if let Ok(cache) = fcm_token_cache().lock() {
        if let Some((tok, exp)) = cache.as_ref() {
            if now_unix() + 60 < *exp {
                return Some(tok.clone());
            }
        }
    }

    let iat = now_unix();
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        scope: &'a str,
        aud: &'a str,
        iat: u64,
        exp: u64,
    }
    let claims = Claims {
        iss: &sa.client_email,
        scope: "https://www.googleapis.com/auth/firebase.messaging",
        aud: &sa.token_uri,
        iat,
        exp: iat + 3600,
    };
    let key = match jsonwebtoken::EncodingKey::from_rsa_pem(sa.private_key.as_bytes()) {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("fcm: bad service-account private key: {e}");
            return None;
        }
    };
    let jwt = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &key,
    )
    .ok()?;

    let resp = reqwest::Client::new()
        .post(&sa.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", jwt.as_str()),
        ])
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!("fcm: token endpoint HTTP {}", resp.status());
        return None;
    }
    #[derive(serde::Deserialize)]
    struct TokResp {
        access_token: String,
        #[serde(default)]
        expires_in: u64,
    }
    let t: TokResp = resp.json().await.ok()?;
    let ttl = if t.expires_in == 0 {
        3600
    } else {
        t.expires_in
    };
    if let Ok(mut cache) = fcm_token_cache().lock() {
        *cache = Some((t.access_token.clone(), now_unix() + ttl));
    }
    Some(t.access_token)
}

/// Send a notification + data message to every registered blugo device via the
/// FCM HTTP v1 API. Prunes tokens FCM reports as gone (404). Best-effort.
async fn send_fcm(cfg: &BlumiConfig, title: &str, body: &str, data: Option<serde_json::Value>) {
    let Some(sa_path) = fcm_sa_path(cfg) else {
        return;
    };
    let Some(sa) = load_sa(&sa_path) else {
        tracing::warn!("fcm: could not parse {}", sa_path.display());
        return;
    };
    let project = {
        let over = cfg.notify.fcm.project_id.trim();
        if over.is_empty() {
            sa.project_id.clone()
        } else {
            over.to_string()
        }
    };
    if project.is_empty() {
        return;
    }
    let store_path = cfg.paths.fcm_store();
    let devices = blumi_core::fcm::list_devices(&store_path);
    if devices.is_empty() {
        return;
    }
    let Some(token) = fcm_access_token(&sa).await else {
        return;
    };

    let url = format!("https://fcm.googleapis.com/v1/projects/{project}/messages:send");
    let data_obj = stringify_values(data.unwrap_or_else(|| serde_json::json!({})));
    let client = reqwest::Client::new();
    let mut dead = Vec::new();
    for d in &devices {
        let msg = serde_json::json!({
            "message": {
                "token": d.token,
                "notification": { "title": title, "body": preview(body) },
                "data": data_obj,
                "android": { "priority": "high" },
            }
        });
        match client
            .post(&url)
            .bearer_auth(&token)
            .json(&msg)
            .send()
            .await
        {
            Ok(r) => {
                let code = r.status().as_u16();
                if code == 404 {
                    dead.push(d.token.clone()); // UNREGISTERED / NOT_FOUND
                } else if !r.status().is_success() {
                    tracing::warn!("fcm: HTTP {code}");
                }
            }
            Err(e) => tracing::warn!("fcm: send error: {e}"),
        }
    }
    for t in dead {
        let _ = blumi_core::fcm::remove_device(&store_path, &t);
    }
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

    #[test]
    fn preview_truncates_long_bodies() {
        assert_eq!(preview("  hi  "), "hi");
        let long = "a".repeat(200);
        let p = preview(&long);
        assert!(p.ends_with('…'));
        assert!(p.chars().count() <= 141); // 140 chars + ellipsis
    }

    #[test]
    fn stringify_values_coerces_to_strings_and_drops_null() {
        let v = serde_json::json!({
            "session_id": "dispatch",
            "n": 7,
            "flag": true,
            "skip": null,
        });
        let out = stringify_values(v);
        assert_eq!(out["session_id"], "dispatch");
        assert_eq!(out["n"], "7");
        assert_eq!(out["flag"], "true");
        assert!(out.get("skip").is_none());
        // Non-object input becomes an empty object.
        assert_eq!(
            stringify_values(serde_json::json!("x")),
            serde_json::json!({})
        );
    }
}

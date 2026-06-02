//! Telegram transport: long-poll `getUpdates`, drive a turn, `sendMessage` the
//! reply. Only a bot token is needed (no public URL), which is why it's the
//! simplest gateway.

use crate::{split_message, GatewayCore};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

/// Telegram's hard per-message cap.
const MAX_MSG: usize = 4096;

/// Options for the Telegram gateway.
pub struct TelegramOptions {
    /// Bot token from @BotFather.
    pub token: String,
    /// If non-empty, only these chat ids are served (an allow-list).
    pub allowed_chats: Vec<i64>,
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    // Missing `Option` fields deserialize to `None` without a `Default` bound on T.
    result: Option<T>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct Update {
    update_id: i64,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    text: Option<String>,
    chat: Chat,
}

#[derive(Deserialize)]
struct Chat {
    id: i64,
}

/// Run the Telegram gateway forever (long-poll loop). Network errors are logged
/// and retried so a transient blip never kills the bot.
pub async fn run_telegram(core: Arc<GatewayCore>, opts: TelegramOptions) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let base = format!("https://api.telegram.org/bot{}", opts.token);

    // Confirm the token up front (and surface a clear error if it's wrong).
    match get_me(&client, &base).await {
        Ok(name) => tracing::info!("telegram gateway online as @{name}"),
        Err(e) => anyhow::bail!("telegram getMe failed (check the token): {e}"),
    }

    let mut offset: i64 = 0;
    loop {
        let updates = match get_updates(&client, &base, offset).await {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!("telegram getUpdates failed: {e}; retrying");
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };
        for update in updates {
            offset = offset.max(update.update_id + 1);
            let Some(msg) = update.message else { continue };
            let (Some(text), chat_id) = (msg.text, msg.chat.id) else {
                continue;
            };
            if !opts.allowed_chats.is_empty() && !opts.allowed_chats.contains(&chat_id) {
                tracing::debug!("ignoring message from non-allowed chat {chat_id}");
                continue;
            }
            handle_message(&client, &base, &core, chat_id, text.trim()).await;
        }
    }
}

async fn handle_message(
    client: &reqwest::Client,
    base: &str,
    core: &Arc<GatewayCore>,
    chat_id: i64,
    text: &str,
) {
    let key = chat_id.to_string();

    // A couple of chat commands.
    if text == "/start" || text == "/help" {
        let _ = send_message(
            client,
            base,
            chat_id,
            "✿ blumi here. Send me a message and I'll work on it. /reset starts a fresh conversation.",
        )
        .await;
        return;
    }
    if text == "/reset" {
        core.reset(&key).await;
        let _ = send_message(client, base, chat_id, "context cleared — fresh start.").await;
        return;
    }
    if text.is_empty() {
        return;
    }

    // Keep a "typing…" indicator alive while the agent works.
    let typing = {
        let client = client.clone();
        let base = base.to_string();
        tokio::spawn(async move {
            loop {
                let _ = send_typing(&client, &base, chat_id).await;
                tokio::time::sleep(Duration::from_secs(4)).await;
            }
        })
    };

    let reply = core
        .handle(&key, text)
        .await
        .unwrap_or_else(|e| format!("⚠ {e}"));
    typing.abort();

    for chunk in split_message(&reply, MAX_MSG) {
        if let Err(e) = send_message(client, base, chat_id, &chunk).await {
            tracing::warn!("telegram sendMessage failed: {e}");
            break;
        }
    }
}

async fn get_me(client: &reqwest::Client, base: &str) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct Me {
        username: String,
    }
    let resp: ApiResponse<Me> = client
        .get(format!("{base}/getMe"))
        .send()
        .await?
        .json()
        .await?;
    match resp.result {
        Some(me) if resp.ok => Ok(me.username),
        _ => anyhow::bail!(resp.description.unwrap_or_else(|| "unknown error".into())),
    }
}

async fn get_updates(
    client: &reqwest::Client,
    base: &str,
    offset: i64,
) -> anyhow::Result<Vec<Update>> {
    let resp: ApiResponse<Vec<Update>> = client
        .get(format!("{base}/getUpdates"))
        .query(&[
            ("offset", offset.to_string()),
            ("timeout", "30".to_string()),
        ])
        .timeout(Duration::from_secs(60))
        .send()
        .await?
        .json()
        .await?;
    Ok(resp.result.unwrap_or_default())
}

async fn send_message(
    client: &reqwest::Client,
    base: &str,
    chat_id: i64,
    text: &str,
) -> anyhow::Result<()> {
    client
        .post(format!("{base}/sendMessage"))
        .json(&serde_json::json!({ "chat_id": chat_id, "text": text }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn send_typing(client: &reqwest::Client, base: &str, chat_id: i64) -> anyhow::Result<()> {
    client
        .post(format!("{base}/sendChatAction"))
        .json(&serde_json::json!({ "chat_id": chat_id, "action": "typing" }))
        .send()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_update() {
        let json = r#"{"ok":true,"result":[
            {"update_id":42,"message":{"text":"hello","chat":{"id":777}}}
        ]}"#;
        let resp: ApiResponse<Vec<Update>> = serde_json::from_str(json).unwrap();
        let updates = resp.result.unwrap();
        assert_eq!(updates[0].update_id, 42);
        let msg = updates[0].message.as_ref().unwrap();
        assert_eq!(msg.text.as_deref(), Some("hello"));
        assert_eq!(msg.chat.id, 777);
    }

    #[test]
    fn tolerates_non_text_updates() {
        // e.g. a sticker / photo with no `text` and an edited_message we ignore.
        let json = r#"{"ok":true,"result":[{"update_id":7,"message":{"chat":{"id":1}}}]}"#;
        let resp: ApiResponse<Vec<Update>> = serde_json::from_str(json).unwrap();
        let updates = resp.result.unwrap();
        assert!(updates[0].message.as_ref().unwrap().text.is_none());
    }
}

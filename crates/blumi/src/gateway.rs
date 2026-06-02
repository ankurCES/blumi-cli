//! `blumi gateway` — run blumi as a messaging bot. Each platform drives the same
//! headless session core (one conversation per chat).

use crate::engine::build_session;
use async_trait::async_trait;
use blumi_config::BlumiConfig;
use blumi_core::SessionHandle;
use blumi_gateway::{
    DiscordOptions, GatewayCore, SessionSpawner, SlackOptions, TelegramOptions, WhatsappOptions,
};
use std::sync::Arc;

/// Spawns headless sessions for the gateway over `build_session`.
struct GatewaySpawner {
    config: BlumiConfig,
    yolo: bool,
}

#[async_trait]
impl SessionSpawner for GatewaySpawner {
    async fn spawn(&self) -> anyhow::Result<SessionHandle> {
        build_session(&self.config, self.yolo, None).await
    }
}

/// Pick the token from the flag, else config; error if neither is set.
fn resolve_token(flag: Option<String>, configured: &str, what: &str) -> anyhow::Result<String> {
    flag.filter(|t| !t.trim().is_empty())
        .or_else(|| Some(configured.to_string()))
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("no {what} (pass --token or set it in settings.json)"))
}

pub async fn run_telegram(config: BlumiConfig, token: Option<String>) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    let token = resolve_token(token, &config.gateway.telegram.token, "telegram token")?;
    let allowed_chats = config.gateway.telegram.allowed_chats.clone();
    let yolo = config.gateway.yolo;

    crate::branding::banner();
    eprintln!(
        "  blumi telegram gateway — {} mode  (Ctrl+C to stop)",
        if yolo {
            "auto-approve"
        } else {
            "safe (read-only tools)"
        }
    );

    let spawner = Arc::new(GatewaySpawner { config, yolo });
    let core = Arc::new(GatewayCore::new(spawner, yolo));
    blumi_gateway::run_telegram(
        core,
        TelegramOptions {
            token,
            allowed_chats,
        },
    )
    .await
}

pub async fn run_discord(config: BlumiConfig, token: Option<String>) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    let token = resolve_token(token, &config.gateway.discord.token, "discord token")?;
    let allowed_channels = config.gateway.discord.allowed_channels.clone();
    let yolo = config.gateway.yolo;

    crate::branding::banner();
    eprintln!(
        "  blumi discord gateway — {} mode  (Ctrl+C to stop)",
        if yolo {
            "auto-approve"
        } else {
            "safe (read-only tools)"
        }
    );

    let spawner = Arc::new(GatewaySpawner { config, yolo });
    let core = Arc::new(GatewayCore::new(spawner, yolo));
    blumi_gateway::run_discord(
        core,
        DiscordOptions {
            token,
            allowed_channels,
        },
    )
    .await
}

pub async fn run_slack(
    config: BlumiConfig,
    bot_token: Option<String>,
    app_token: Option<String>,
) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    let bot_token = resolve_token(
        bot_token,
        &config.gateway.slack.bot_token,
        "slack bot token",
    )?;
    let app_token = resolve_token(
        app_token,
        &config.gateway.slack.app_token,
        "slack app token",
    )?;
    let yolo = config.gateway.yolo;

    crate::branding::banner();
    eprintln!(
        "  blumi slack gateway — {} mode  (Ctrl+C to stop)",
        if yolo {
            "auto-approve"
        } else {
            "safe (read-only tools)"
        }
    );

    let spawner = Arc::new(GatewaySpawner { config, yolo });
    let core = Arc::new(GatewayCore::new(spawner, yolo));
    blumi_gateway::run_slack(
        core,
        SlackOptions {
            bot_token,
            app_token,
        },
    )
    .await
}

pub async fn run_whatsapp(config: BlumiConfig, port: Option<u16>) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    let wa = config.gateway.whatsapp.clone();
    let token = resolve_token(None, &wa.token, "whatsapp token")?;
    if wa.phone_number_id.trim().is_empty() {
        anyhow::bail!("no whatsapp phone_number_id (set gateway.whatsapp.phone_number_id)");
    }
    if wa.verify_token.trim().is_empty() {
        anyhow::bail!("no whatsapp verify_token (set gateway.whatsapp.verify_token)");
    }
    // Flag > config > 8080 default.
    let port = port
        .or(Some(wa.webhook_port).filter(|p| *p != 0))
        .unwrap_or(8080);
    let yolo = config.gateway.yolo;

    crate::branding::banner();
    eprintln!("  blumi whatsapp gateway — webhook on :{port}/webhook  (Ctrl+C to stop)");

    let spawner = Arc::new(GatewaySpawner { config, yolo });
    let core = Arc::new(GatewayCore::new(spawner, yolo));
    blumi_gateway::run_whatsapp(
        core,
        WhatsappOptions {
            token,
            phone_number_id: wa.phone_number_id,
            verify_token: wa.verify_token,
            port,
        },
    )
    .await
}

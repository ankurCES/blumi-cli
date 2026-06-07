//! `blumi gateway` — run blumi as a messaging bot. Each platform drives the same
//! headless session core (one conversation per chat).

use crate::engine::build_session;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use anyhow::Context;
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

/// Build the gateway core (a per-chat session map) from config, wiring voice
/// (STT for inbound audio + TTS for replies) when it's enabled.
fn build_core(config: BlumiConfig, yolo: bool) -> Arc<GatewayCore> {
    let voice = if config.voice.enabled {
        Some(crate::web::to_voice_config(&config))
    } else {
        None
    };
    let spawner = Arc::new(GatewaySpawner { config, yolo });
    Arc::new(GatewayCore::new(spawner, yolo, voice))
}

pub async fn run_telegram(config: BlumiConfig, token: Option<String>) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    let token = resolve_token(token, &config.gateway.telegram.token, "telegram token")?;
    let allowed_chats = config.gateway.telegram.allowed_chats.clone();
    let voice = config.gateway.telegram.voice;
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

    let core = build_core(config, yolo);
    blumi_gateway::run_telegram(
        core,
        TelegramOptions {
            token,
            allowed_chats,
            voice,
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

    let core = build_core(config, yolo);
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

    let core = build_core(config, yolo);
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

    let core = build_core(config, yolo);
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

// ── Run-all + service management (mirrors `blumi serve`) ─────────────────────

/// launchd job label for the messaging gateway service (macOS).
#[cfg(target_os = "macos")]
const GW_LABEL: &str = "com.blumi.gateway";

/// Names of every transport whose credentials are configured.
fn configured_transports(config: &BlumiConfig) -> Vec<&'static str> {
    let g = &config.gateway;
    let mut v = Vec::new();
    if !g.telegram.token.trim().is_empty() {
        v.push("telegram");
    }
    if !g.discord.token.trim().is_empty() {
        v.push("discord");
    }
    if !g.slack.bot_token.trim().is_empty() && !g.slack.app_token.trim().is_empty() {
        v.push("slack");
    }
    if !g.whatsapp.token.trim().is_empty() && !g.whatsapp.phone_number_id.trim().is_empty() {
        v.push("whatsapp");
    }
    v
}

/// Run **every configured** messaging transport concurrently — the service entry
/// point (`blumi gateway run`). Each transport supervises its own retries; if one
/// exits, the others keep running. Errors if no transport is configured.
pub async fn run_all(config: BlumiConfig) -> anyhow::Result<()> {
    config.paths.ensure_dirs().ok();
    let names = configured_transports(&config);
    if names.is_empty() {
        anyhow::bail!(
            "no messaging transports configured — set a token under \
             gateway.telegram / gateway.discord / gateway.slack / gateway.whatsapp"
        );
    }
    eprintln!(
        "  blumi gateway — running [{}] in {} mode  (Ctrl+C to stop)",
        names.join(", "),
        if config.gateway.yolo {
            "auto-approve"
        } else {
            "safe (read-only tools)"
        }
    );

    let mut set = tokio::task::JoinSet::new();
    if names.contains(&"telegram") {
        let c = config.clone();
        set.spawn(async move { ("telegram", run_telegram(c, None).await) });
    }
    if names.contains(&"discord") {
        let c = config.clone();
        set.spawn(async move { ("discord", run_discord(c, None).await) });
    }
    if names.contains(&"slack") {
        let c = config.clone();
        set.spawn(async move { ("slack", run_slack(c, None, None).await) });
    }
    if names.contains(&"whatsapp") {
        let c = config.clone();
        set.spawn(async move { ("whatsapp", run_whatsapp(c, None).await) });
    }
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((name, Ok(()))) => tracing::warn!("gateway transport {name} exited"),
            Ok((name, Err(e))) => tracing::error!("gateway transport {name} failed: {e}"),
            Err(e) => tracing::error!("gateway task join error: {e}"),
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn gw_plist_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(crate::serve::home_dir()?
        .join("Library/LaunchAgents")
        .join(format!("{GW_LABEL}.plist")))
}

#[cfg(target_os = "macos")]
pub fn install(config: &BlumiConfig) -> anyhow::Result<()> {
    if configured_transports(config).is_empty() {
        anyhow::bail!(
            "no messaging transports configured — set a token (e.g. gateway.telegram.token) first"
        );
    }
    let exe = crate::serve::exe()?;
    let log = config.paths.home.join("gateway.log");
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| config.paths.home.display().to_string());
    let plist = gw_plist_path()?;
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{GW_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string><string>gateway</string><string>run</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>WorkingDirectory</key><string>{cwd}</string>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        log = log.display(),
    );
    std::fs::write(&plist, body).with_context(|| format!("writing {}", plist.display()))?;
    let p = plist.display().to_string();
    let _ = crate::serve::run_cmd("launchctl", &["unload", "-w", &p]); // ignore "not loaded"
    crate::serve::run_cmd("launchctl", &["load", "-w", &p])?;
    println!(
        "✿ gateway installed + started → [{}]  (logs: {})",
        configured_transports(config).join(", "),
        log.display()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> anyhow::Result<()> {
    let plist = gw_plist_path()?;
    if plist.exists() {
        let _ = crate::serve::run_cmd("launchctl", &["unload", "-w", &plist.display().to_string()]);
        std::fs::remove_file(&plist).ok();
    }
    println!("✿ removed the gateway service.");
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn service(action: &str) -> anyhow::Result<()> {
    crate::serve::run_cmd("launchctl", &[action, GW_LABEL])?;
    println!("✿ {action} {GW_LABEL}");
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_state() -> String {
    let installed = gw_plist_path().map(|p| p.exists()).unwrap_or(false);
    if !installed {
        return "not installed".to_string();
    }
    let loaded = std::process::Command::new("launchctl")
        .args(["list", GW_LABEL])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if loaded {
        "running".to_string()
    } else {
        "installed (stopped)".to_string()
    }
}

#[cfg(target_os = "linux")]
fn gw_unit_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(crate::serve::home_dir()?.join(".config/systemd/user/blumi-gateway.service"))
}

#[cfg(target_os = "linux")]
pub fn install(config: &BlumiConfig) -> anyhow::Result<()> {
    if configured_transports(config).is_empty() {
        anyhow::bail!(
            "no messaging transports configured — set a token (e.g. gateway.telegram.token) first"
        );
    }
    let exe = crate::serve::exe()?;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| config.paths.home.display().to_string());
    let unit = gw_unit_path()?;
    if let Some(parent) = unit.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = format!(
        "[Unit]\nDescription=blumi messaging gateway\nAfter=network-online.target\n\n\
         [Service]\nExecStart={exe} gateway run\nWorkingDirectory={cwd}\nRestart=always\nRestartSec=3\n\n\
         [Install]\nWantedBy=default.target\n"
    );
    std::fs::write(&unit, body).with_context(|| format!("writing {}", unit.display()))?;
    crate::serve::run_cmd("systemctl", &["--user", "daemon-reload"])?;
    crate::serve::run_cmd("systemctl", &["--user", "enable", "--now", "blumi-gateway"])?;
    println!(
        "✿ gateway installed + started → [{}]",
        configured_transports(config).join(", ")
    );
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> anyhow::Result<()> {
    let _ = crate::serve::run_cmd(
        "systemctl",
        &["--user", "disable", "--now", "blumi-gateway"],
    );
    if let Ok(unit) = gw_unit_path() {
        std::fs::remove_file(&unit).ok();
    }
    let _ = crate::serve::run_cmd("systemctl", &["--user", "daemon-reload"]);
    println!("✿ removed the gateway service.");
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn service(action: &str) -> anyhow::Result<()> {
    crate::serve::run_cmd("systemctl", &["--user", action, "blumi-gateway"])?;
    println!("✿ {action} blumi-gateway");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_state() -> String {
    let installed = gw_unit_path().map(|p| p.exists()).unwrap_or(false);
    if !installed {
        return "not installed".to_string();
    }
    let active = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "blumi-gateway"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false);
    if active {
        "running".to_string()
    } else {
        "installed (stopped)".to_string()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn install(_config: &BlumiConfig) -> anyhow::Result<()> {
    anyhow::bail!("service install isn't supported on this OS — run `blumi gateway run` under a process manager")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn uninstall() -> anyhow::Result<()> {
    anyhow::bail!("service management isn't supported on this OS")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn service(_action: &str) -> anyhow::Result<()> {
    anyhow::bail!("service management isn't supported on this OS")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn service_state() -> String {
    "unsupported on this OS".to_string()
}

/// Print gateway-service status: service state + which transports are configured.
pub fn status(config: &BlumiConfig) {
    println!("gateway service: {}", service_state());
    let configured = configured_transports(config);
    println!(
        "  transports: {}",
        if configured.is_empty() {
            "(none configured)".to_string()
        } else {
            configured.join(", ")
        }
    );
    println!(
        "  logs: {}",
        config.paths.home.join("gateway.log").display()
    );
}

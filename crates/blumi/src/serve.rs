//! `blumi serve` — run blumi as an always-on gateway for the **blugo** mobile app.
//!
//! This reuses the existing blumi-web HTTP/SSE server (so the phone app is just a
//! second client of the same API the browser UI uses) and adds OS-level service
//! management — launchd on macOS, `systemd --user` on Linux — so it keeps running
//! across logout/reboot, plus a `pair` helper that sets the login password and
//! prints the LAN URL for blugo's connect screen.

use crate::ServeCmd;
use anyhow::{anyhow, Context};
use blumi_config::BlumiConfig;
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::Command;

const LABEL: &str = "com.blumi.serve"; // launchd job label (also the systemd unit base)
const DEFAULT_PORT: u16 = 7777;

pub async fn run(config: BlumiConfig, action: ServeCmd) -> anyhow::Result<()> {
    match action {
        ServeCmd::Run {
            host,
            port,
            password,
        } => {
            // Default to the LAN IP so the phone can reach it (loopback-only is
            // still available with `--host 127.0.0.1`).
            let host = host.or_else(|| primary_lan_ip().map(|ip| ip.to_string()));
            crate::web::run(config, host, password, port).await
        }
        ServeCmd::Pair { password } => pair(&config, password),
        ServeCmd::Install { host, port } => install(&config, host, port),
        ServeCmd::Uninstall => uninstall(),
        ServeCmd::Start => service("start"),
        ServeCmd::Stop => service("stop"),
        ServeCmd::Status => status(&config),
    }
}

/// The machine's primary LAN IPv4. Opens a UDP socket toward a public address to
/// learn the default-route source IP — no packets are actually sent.
fn primary_lan_ip() -> Option<IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

fn lan_url() -> String {
    let ip = primary_lan_ip()
        .map(|i| i.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    format!("http://{ip}:{DEFAULT_PORT}")
}

/// Set/confirm the password and print the LAN URL for blugo to connect to.
fn pair(config: &BlumiConfig, password: Option<String>) -> anyhow::Result<()> {
    let has_pw = !config.web.password_hash.trim().is_empty();
    match password {
        Some(pw) if !pw.trim().is_empty() => {
            let hash = blumi_web::Auth::hash_password(&pw)?;
            crate::web::persist_password_hash(&config.paths.settings_json(), &hash)?;
            println!("✿ password set.");
        }
        _ if has_pw => println!("✿ a password is already set (use --password to change it)."),
        _ => {
            return Err(anyhow!(
                "set a password to pair a device:\n  blumi serve pair --password <password>"
            ))
        }
    }
    let url = lan_url();
    println!("\n  Connect blugo to:  {url}");
    println!("  Log in with the password you just set.");
    println!("\n  Keep the gateway running in the background with:");
    println!("    blumi serve install");
    Ok(())
}

// ── Service management ──────────────────────────────────────────────────────

fn run_cmd(bin: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(bin)
        .args(args)
        .status()
        .with_context(|| format!("running `{bin}`"))?;
    if !status.success() {
        return Err(anyhow!("`{bin} {}` failed", args.join(" ")));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn exe() -> anyhow::Result<String> {
    Ok(std::env::current_exe()?.display().to_string())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn home_dir() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn resolve_host(host: Option<String>) -> String {
    host.unwrap_or_else(|| {
        primary_lan_ip()
            .map(|i| i.to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string())
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn require_password(config: &BlumiConfig) -> anyhow::Result<()> {
    if config.web.password_hash.trim().is_empty() {
        return Err(anyhow!(
            "set a password before installing the LAN gateway:\n  \
             blumi serve pair --password <password>"
        ));
    }
    Ok(())
}

// ── macOS (launchd LaunchAgent) ─────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn plist_path() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn install(config: &BlumiConfig, host: Option<String>, port: Option<u16>) -> anyhow::Result<()> {
    require_password(config)?;
    let host = resolve_host(host);
    let port = port.unwrap_or(DEFAULT_PORT);
    let exe = exe()?;
    let log = config.paths.home.join("serve.log");
    // Run the service in the directory it was installed from (so the task board
    // + dispatched work use a real workspace, not `/`).
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| config.paths.home.display().to_string());
    let plist = plist_path()?;
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string><string>serve</string><string>run</string>
    <string>--host</string><string>{host}</string>
    <string>--port</string><string>{port}</string>
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
    let _ = run_cmd("launchctl", &["unload", "-w", &p]); // ignore "not loaded"
    run_cmd("launchctl", &["load", "-w", &p])?;
    println!(
        "✿ installed + started → http://{host}:{port}  (logs: {})",
        log.display()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall() -> anyhow::Result<()> {
    let plist = plist_path()?;
    if plist.exists() {
        let _ = run_cmd("launchctl", &["unload", "-w", &plist.display().to_string()]);
        std::fs::remove_file(&plist).ok();
    }
    println!("✿ removed the gateway service.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn service(action: &str) -> anyhow::Result<()> {
    run_cmd("launchctl", &[action, LABEL])?;
    println!("✿ {action} {LABEL}");
    Ok(())
}

// ── Linux (systemd --user) ──────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn unit_path() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join(".config/systemd/user/blumi-serve.service"))
}

#[cfg(target_os = "linux")]
fn install(config: &BlumiConfig, host: Option<String>, port: Option<u16>) -> anyhow::Result<()> {
    require_password(config)?;
    let host = resolve_host(host);
    let port = port.unwrap_or(DEFAULT_PORT);
    let exe = exe()?;
    // Run the service in the directory it was installed from (real workspace).
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| config.paths.home.display().to_string());
    let unit = unit_path()?;
    if let Some(parent) = unit.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = format!(
        "[Unit]\nDescription=blumi gateway for the blugo app\nAfter=network-online.target\n\n\
         [Service]\nExecStart={exe} serve run --host {host} --port {port}\nWorkingDirectory={cwd}\nRestart=always\nRestartSec=3\n\n\
         [Install]\nWantedBy=default.target\n"
    );
    std::fs::write(&unit, body).with_context(|| format!("writing {}", unit.display()))?;
    run_cmd("systemctl", &["--user", "daemon-reload"])?;
    run_cmd("systemctl", &["--user", "enable", "--now", "blumi-serve"])?;
    println!("✿ installed + started → http://{host}:{port}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall() -> anyhow::Result<()> {
    let _ = run_cmd("systemctl", &["--user", "disable", "--now", "blumi-serve"]);
    if let Ok(unit) = unit_path() {
        std::fs::remove_file(&unit).ok();
    }
    let _ = run_cmd("systemctl", &["--user", "daemon-reload"]);
    println!("✿ removed the gateway service.");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service(action: &str) -> anyhow::Result<()> {
    run_cmd("systemctl", &["--user", action, "blumi-serve"])?;
    println!("✿ {action} blumi-serve");
    Ok(())
}

// ── Other platforms ─────────────────────────────────────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn install(_config: &BlumiConfig, _host: Option<String>, _port: Option<u16>) -> anyhow::Result<()> {
    Err(anyhow!(
        "service install isn't supported on this OS — run `blumi serve run` (e.g. under a process manager)"
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn uninstall() -> anyhow::Result<()> {
    Err(anyhow!("service management isn't supported on this OS"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn service(_action: &str) -> anyhow::Result<()> {
    Err(anyhow!("service management isn't supported on this OS"))
}

// ── Status (platform-agnostic via the lock file) ────────────────────────────

fn pid_alive(pid: i64) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn status(config: &BlumiConfig) -> anyhow::Result<()> {
    let lock = config.paths.home.join("web.lock");
    match std::fs::read_to_string(&lock) {
        Ok(s) => {
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
            let url = v.get("url").and_then(|x| x.as_str());
            let pid = v.get("pid").and_then(|x| x.as_i64());
            let alive = pid.map(pid_alive).unwrap_or(false);
            println!(
                "gateway: {}",
                if alive {
                    "running"
                } else {
                    "not running (stale lock)"
                }
            );
            if let Some(u) = url {
                println!("  url: {u}");
            }
            if let Some(p) = pid {
                println!("  pid: {p}");
            }
        }
        Err(_) => println!("gateway: not running (no lock file)"),
    }
    Ok(())
}

// --- Service control (used by `restart_gateway` + the self-restart endpoint) --

/// Which service manager supervises this gateway (best-effort). Variants are
/// platform-specific, so some are unconstructed on a given target.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceManager {
    Launchd,
    SystemdUser,
    None,
}

/// Detect whether blumi is installed as a background service on this OS. Returns
/// `None` when no service is installed (e.g. a foreground `serve run`), in which
/// case a restart should degrade to an in-place reload.
pub fn detect_manager() -> ServiceManager {
    #[cfg(target_os = "macos")]
    {
        if plist_path().map(|p| p.exists()).unwrap_or(false) {
            return ServiceManager::Launchd;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if unit_path().map(|p| p.exists()).unwrap_or(false) {
            return ServiceManager::SystemdUser;
        }
    }
    ServiceManager::None
}

/// Restart this gateway out-of-process via its service manager. Spawns a detached
/// helper that waits briefly (so an in-flight HTTP response can flush) then kicks
/// the service — the manager kills this instance and starts a fresh one. Returns
/// immediately; the helper outlives this process. Errors when not service-managed.
pub fn restart_self(mgr: ServiceManager) -> anyhow::Result<()> {
    let inner = match mgr {
        ServiceManager::Launchd => {
            format!("sleep 0.75; launchctl kickstart -k gui/$(id -u)/{LABEL}")
        }
        ServiceManager::SystemdUser => {
            "sleep 0.75; systemctl --user restart blumi-serve".to_string()
        }
        ServiceManager::None => return Err(anyhow!("not running under a service manager")),
    };
    // Background + detach via the shell so the helper survives this process being
    // killed by the restart.
    let script = format!("({inner}) </dev/null >/dev/null 2>&1 &");
    Command::new("sh")
        .arg("-c")
        .arg(script)
        .spawn()
        .map(|_| ())
        .map_err(|e| anyhow!("could not spawn restart helper: {e}"))
}

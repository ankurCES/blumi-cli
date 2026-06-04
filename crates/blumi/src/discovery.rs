//! LAN auto-discovery beacon.
//!
//! Every running gateway advertises itself over mDNS/DNS-SD as
//! `_blumi._tcp.local.`, so the **blugo** mobile app (and any Bonjour/Zeroconf
//! browser) can find blumi instances on the same Wi-Fi without typing an IP.
//!
//! The beacon carries only host + port + a little metadata (machine name,
//! version, whether a password is required) — never any secret. The client
//! still authenticates with the password. Discovery is best-effort: any failure
//! (no multicast, IPv6-only, etc.) is silently ignored — the gateway runs fine
//! without it and manual "add by IP" still works.

use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::net::{IpAddr, Ipv4Addr};

pub(crate) const SERVICE_TYPE: &str = "_blumi._tcp.local.";

/// Holds the live mDNS registration; unregisters and shuts the daemon down when
/// dropped (i.e. when the server stops).
pub struct Beacon {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Beacon {
    /// The mDNS fullname this beacon registered (e.g. `mac._blumi._tcp.local.`),
    /// used by the grid browser to exclude our own advertisement.
    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl Drop for Beacon {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

/// Advertise this gateway on the LAN. Keep the returned [`Beacon`] alive for as
/// long as the server runs. Returns `None` when there's nothing to advertise
/// (loopback-only bind, IPv6, or registration failure) — never panics.
///
/// When `grid_id` is `Some`, a non-sensitive `grid` TXT key is published so grid
/// peers can find each other (the secret itself is never advertised). `node_name`
/// (when non-empty) is the friendly label peers display; it falls back to the
/// hostname.
pub fn advertise(
    bind_ip: IpAddr,
    port: u16,
    auth_required: bool,
    grid_id: Option<&str>,
    node_name: Option<&str>,
) -> Option<Beacon> {
    // Resolve a concrete LAN IPv4 to publish. A wildcard bind (0.0.0.0) is
    // turned into the primary LAN address; loopback isn't reachable by phones.
    let ip: Ipv4Addr = match bind_ip {
        IpAddr::V4(v4) if v4.is_loopback() => return None,
        IpAddr::V4(v4) if v4.is_unspecified() => primary_lan_ipv4()?,
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => return None,
    };

    let raw = whoami::fallible::hostname().unwrap_or_else(|_| "blumi".to_string());
    // Instance/host labels can't contain a trailing `.local`; keep a clean stem.
    let base = {
        let b = raw.trim_end_matches('.').trim_end_matches(".local");
        if b.is_empty() {
            "blumi"
        } else {
            b
        }
    };
    let host_name = format!("{base}.local.");
    let version = env!("CARGO_PKG_VERSION");
    let auth = if auth_required { "required" } else { "none" };
    // Friendly label peers display: the configured node_name, else the hostname.
    let display_name = node_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(raw.as_str());
    let mut props: Vec<(&str, &str)> = vec![
        ("name", display_name),
        ("version", version),
        ("auth", auth),
        ("path", "/"),
    ];
    // Non-sensitive grid identity so same-grid peers can find each other; the
    // shared secret is never advertised.
    if let Some(gid) = grid_id.filter(|g| !g.is_empty()) {
        props.push(("grid", gid));
    }

    let daemon = ServiceDaemon::new().ok()?;
    let info = ServiceInfo::new(
        SERVICE_TYPE,
        base,
        &host_name,
        IpAddr::V4(ip),
        port,
        &props[..],
    )
    .ok()?;
    let fullname = info.get_fullname().to_string();
    daemon.register(info).ok()?;
    Some(Beacon { daemon, fullname })
}

/// The primary LAN IPv4 via the UDP "connect" trick (no packets are sent).
fn primary_lan_ipv4() -> Option<Ipv4Addr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v4) => Some(v4),
        IpAddr::V6(_) => None,
    }
}

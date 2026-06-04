//! Distributed grid: peer discovery (mDNS browse) + a live peer registry.
//!
//! Every `blumi serve` already *advertises* itself over mDNS (see
//! [`crate::discovery`]). When the grid is enabled, the gateway also *browses*
//! `_blumi._tcp` and keeps a registry of same-grid peers, which the orchestrator
//! (the instance the phone connects to) dispatches tasks to.
//!
//! Trust is a **shared secret**: nodes that share `grid.secret` form one grid.
//! The secret is never put on the wire by discovery — only a non-reversible
//! `grid_id` digest is advertised, so peers can tell who is in the same grid
//! without exposing the secret. The secret itself is presented (and verified)
//! only when one node actually talks to another's `/api/grid/*` surface.

pub mod client;

use blumi_config::GridConfig;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

// Peers are kept until mDNS reports them gone (ServiceRemoved); the live grid
// metrics endpoint confirms each peer's real-time reachability when queried. A
// short last-seen TTL was removed — mDNS re-resolution backs off, so it wrongly
// aged out peers that were still advertising.

/// Public, non-sensitive grid identity used for mDNS advertising + browse
/// filtering. An explicit `grid_id` wins; otherwise it is a short, non-reversible
/// digest of the secret (so "same secret ⇒ same grid_id" happens automatically).
/// Returns `None` when the grid is disabled or has no secret (fail closed).
pub fn grid_id(cfg: &GridConfig) -> Option<String> {
    if !cfg.enabled || cfg.secret.trim().is_empty() {
        return None;
    }
    let explicit = cfg.grid_id.trim();
    if !explicit.is_empty() {
        return Some(explicit.to_string());
    }
    let digest = Sha256::digest(cfg.secret.as_bytes());
    Some(digest.iter().take(6).map(|b| format!("{b:02x}")).collect())
}

/// A discovered grid peer.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Peer {
    /// Stable key: the mDNS fullname (e.g. `mac-mini._blumi._tcp.local.`).
    pub id: String,
    /// Friendly name from the `name` TXT (falls back to the host stem).
    pub name: String,
    /// Resolved LAN IPv4.
    pub host: Ipv4Addr,
    pub port: u16,
    pub version: String,
    /// From the `auth` TXT: true when the peer requires a login.
    pub auth_required: bool,
    /// From the `grid` TXT — the peer's grid_id.
    pub grid_id: String,
    pub online: bool,
}

impl Peer {
    /// Base URL for talking to this peer's gateway.
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// A thread-safe registry of discovered peers, keyed by mDNS fullname.
#[derive(Default)]
pub struct PeerRegistry {
    inner: Mutex<HashMap<String, Peer>>,
}

impl PeerRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Insert or refresh a peer (sets `online = true`).
    pub fn upsert(&self, p: Peer) {
        if let Ok(mut m) = self.inner.lock() {
            m.insert(p.id.clone(), p);
        }
    }

    /// Mark a peer offline (on a `ServiceRemoved` event).
    pub fn mark_offline(&self, id: &str) {
        if let Ok(mut m) = self.inner.lock() {
            if let Some(p) = m.get_mut(id) {
                p.online = false;
            }
        }
    }

    /// Currently-online peers (per mDNS Resolved/Removed), sorted by id.
    pub fn live(&self) -> Vec<Peer> {
        let mut out = Vec::new();
        if let Ok(m) = self.inner.lock() {
            out.extend(m.values().filter(|p| p.online).cloned());
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Look up a peer by id (mDNS fullname).
    pub fn get(&self, id: &str) -> Option<Peer> {
        self.inner.lock().ok()?.get(id).cloned()
    }
}

/// What the gateway shares about its own grid membership, held by the web
/// management layer so the `/api/grid/peers` endpoint can render it.
pub struct GridShared {
    pub grid_id: String,
    pub node_name: String,
    pub registry: Arc<PeerRegistry>,
}

impl GridShared {
    /// `{ self: { node_name, grid_id, version }, peers: [...] }`.
    pub fn peers_json(&self, version: &str) -> serde_json::Value {
        serde_json::json!({
            "self": {
                "node_name": self.node_name,
                "grid_id": self.grid_id,
                "version": version,
            },
            "peers": self.registry.live(),
        })
    }
}

/// Browse `_blumi._tcp.local.` and feed same-grid peers into `registry` until
/// `cancel` fires. Blocking — run on a dedicated thread. Best-effort: returns on
/// daemon failure (the grid just stays empty), never panics.
pub fn browse_into(
    our_grid_id: String,
    self_id: Option<String>,
    registry: Arc<PeerRegistry>,
    cancel: CancellationToken,
) {
    let Ok(daemon) = ServiceDaemon::new() else {
        return;
    };
    let Ok(rx) = daemon.browse(crate::discovery::SERVICE_TYPE) else {
        let _ = daemon.shutdown();
        return;
    };
    loop {
        if cancel.is_cancelled() {
            let _ = daemon.shutdown();
            return;
        }
        // Short timeout so cancellation is checked promptly.
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(ServiceEvent::ServiceResolved(rs)) => {
                // Same grid only — and never our own advertisement.
                let grid = rs.get_property_val_str("grid").unwrap_or("");
                if grid != our_grid_id {
                    continue;
                }
                if self_id.as_deref() == Some(rs.get_fullname()) {
                    continue;
                }
                let Some(ip) = rs.get_addresses_v4().into_iter().find(|a| !a.is_loopback()) else {
                    continue;
                };
                let name = rs
                    .get_property_val_str("name")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| host_stem(rs.get_fullname()));
                registry.upsert(Peer {
                    id: rs.get_fullname().to_string(),
                    name,
                    host: ip,
                    port: rs.get_port(),
                    version: rs.get_property_val_str("version").unwrap_or("").to_string(),
                    auth_required: rs.get_property_val_str("auth") == Some("required"),
                    grid_id: grid.to_string(),
                    online: true,
                });
            }
            Ok(ServiceEvent::ServiceRemoved(_ty, fullname)) => registry.mark_offline(&fullname),
            Ok(_) => {} // SearchStarted / ServiceFound / SearchStopped
            Err(flume::RecvTimeoutError::Timeout) => {}
            Err(flume::RecvTimeoutError::Disconnected) => {
                let _ = daemon.shutdown();
                return;
            }
        }
    }
}

/// The instance stem of an mDNS fullname (`mac._blumi._tcp.local.` → `mac`).
fn host_stem(fullname: &str) -> String {
    fullname.split('.').next().unwrap_or(fullname).to_string()
}

/// Grid-overflow hook for blumi-core's `AgentSpawner`: when an instance hits its
/// local sub-agent cap, excess delegations run on an available grid peer instead
/// of waiting. Registered process-globally by the gateway when the grid is on.
pub struct GridOverflowHook {
    pub registry: Arc<PeerRegistry>,
    pub secret: String,
}

#[async_trait::async_trait]
impl blumi_core::GridOverflow for GridOverflowHook {
    async fn try_remote(&self, _agent_type: &str, prompt: &str) -> Option<String> {
        // Pick the first live peer (registry.live() is sorted/stable). v1 keeps
        // selection simple; least-busy routing is a later refinement.
        let peer = self.registry.live().into_iter().next()?;
        let client = client::Client::for_peer(&peer, &self.secret);
        match client
            .run_task(prompt.to_string(), Duration::from_secs(900))
            .await
        {
            Ok(out) if !out.trim().is_empty() => Some(out),
            _ => None,
        }
    }
}

/// Explicit per-job dispatch hook for the `grid_dispatch` agent tool: run a
/// self-contained job on a chosen (or round-robin) grid peer and return its
/// output. This is what lets a single chat prompt fan work across the whole grid
/// — the model calls it once per job, so distribution doesn't depend on the
/// local sub-agent cap being exceeded (the [`GridOverflowHook`] limitation).
pub struct GridDispatchHook {
    pub registry: Arc<PeerRegistry>,
    pub secret: String,
    /// Round-robin cursor over live peers when no peer is named.
    pub cursor: AtomicUsize,
}

#[async_trait::async_trait]
impl blumi_core::GridDispatch for GridDispatchHook {
    async fn dispatch(&self, peer: Option<&str>, prompt: &str) -> Result<(String, String), String> {
        let peers = self.registry.live();
        if peers.is_empty() {
            return Err("no live grid peers to dispatch to".to_string());
        }
        // Choose a peer: by name/host match if requested, else round-robin.
        let chosen = match peer.map(str::trim).filter(|p| !p.is_empty()) {
            Some(want) => {
                let w = want.to_lowercase();
                peers
                    .iter()
                    .find(|p| {
                        p.name.to_lowercase().contains(&w) || p.id.to_lowercase().contains(&w)
                    })
                    .cloned()
                    .ok_or_else(|| format!("no live grid peer matching '{want}'"))?
            }
            None => {
                let idx = self.cursor.fetch_add(1, Ordering::Relaxed) % peers.len();
                peers[idx].clone()
            }
        };
        let client = client::Client::for_peer(&chosen, &self.secret);
        match client
            .run_task(prompt.to_string(), Duration::from_secs(900))
            .await
        {
            Ok(out) if !out.trim().is_empty() => Ok((chosen.name.clone(), out)),
            Ok(_) => Err(format!("peer '{}' returned no output", chosen.name)),
            Err(e) => Err(format!("peer '{}' failed: {e}", chosen.name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, secret: &str, id: &str) -> GridConfig {
        GridConfig {
            enabled,
            secret: secret.into(),
            grid_id: id.into(),
            node_name: String::new(),
        }
    }

    #[test]
    fn grid_id_fails_closed_when_disabled_or_empty() {
        assert_eq!(grid_id(&cfg(false, "s", "")), None);
        assert_eq!(grid_id(&cfg(true, "", "")), None);
        assert_eq!(grid_id(&cfg(true, "   ", "")), None);
    }

    #[test]
    fn grid_id_derives_stable_digest_from_secret() {
        let a = grid_id(&cfg(true, "hunter2", "")).unwrap();
        let b = grid_id(&cfg(true, "hunter2", "")).unwrap();
        assert_eq!(a, b, "same secret ⇒ same grid_id");
        assert_eq!(a.len(), 12, "6 bytes hex-encoded");
        assert_ne!(a, "hunter2", "digest must not be the secret");
        let c = grid_id(&cfg(true, "different", "")).unwrap();
        assert_ne!(a, c, "different secrets ⇒ different grid_id");
    }

    #[test]
    fn explicit_grid_id_wins() {
        assert_eq!(
            grid_id(&cfg(true, "s", "team-alpha")).as_deref(),
            Some("team-alpha")
        );
    }

    #[test]
    fn registry_upsert_live_offline() {
        let reg = PeerRegistry::new();
        assert!(reg.live().is_empty());
        reg.upsert(Peer {
            id: "a._blumi._tcp.local.".into(),
            name: "a".into(),
            host: Ipv4Addr::new(10, 0, 0, 2),
            port: 7777,
            version: "0".into(),
            auth_required: true,
            grid_id: "g".into(),
            online: true,
        });
        assert_eq!(reg.live().len(), 1);
        let p = reg.get("a._blumi._tcp.local.").expect("peer present");
        assert_eq!(p.base_url(), "http://10.0.0.2:7777");
        reg.mark_offline("a._blumi._tcp.local.");
        assert!(reg.live().is_empty());
    }
}

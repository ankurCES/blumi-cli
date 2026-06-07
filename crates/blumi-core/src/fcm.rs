//! FCM device-token registry — the blugo phone tokens this gateway pushes to.
//!
//! State lives in a JSON file under `~/.blumi/fcm.json` so it survives restarts
//! and is shared across processes (the gateway registers tokens; the notifier
//! reads them to send). Kept separate from `push.json` (which holds the VAPID
//! private key + browser subscriptions) so the FCM surface is independent.
//!
//! This module only stores tokens — building/sending the FCM HTTP v1 request
//! (service account → OAuth2 → `messages:send`) lives in the binary's notifier,
//! next to the existing web-push sender.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One registered device (the FCM registration token blugo obtained).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FcmDevice {
    pub token: String,
}

/// Persisted set of device tokens to push to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FcmStore {
    #[serde(default)]
    pub devices: Vec<FcmDevice>,
}

impl FcmStore {
    fn load(path: &Path) -> FcmStore {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Register (or de-dup) a device token; returns the new device count.
pub fn add_device(path: &Path, token: &str) -> Result<usize> {
    let mut store = FcmStore::load(path);
    store.devices.retain(|d| d.token != token);
    store.devices.push(FcmDevice {
        token: token.to_string(),
    });
    let n = store.devices.len();
    store.save(path)?;
    Ok(n)
}

/// Remove a device token; returns whether one was removed.
pub fn remove_device(path: &Path, token: &str) -> Result<bool> {
    let mut store = FcmStore::load(path);
    let before = store.devices.len();
    store.devices.retain(|d| d.token != token);
    let removed = store.devices.len() != before;
    if removed {
        store.save(path)?;
    }
    Ok(removed)
}

/// List registered device tokens (empty if no store yet).
pub fn list_devices(path: &Path) -> Vec<FcmDevice> {
    FcmStore::load(path).devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedup_and_remove() {
        let dir = std::env::temp_dir().join(format!("blumi-fcm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("fcm.json");
        let _ = std::fs::remove_file(&path);

        assert_eq!(add_device(&path, "tok-a").unwrap(), 1);
        assert_eq!(add_device(&path, "tok-b").unwrap(), 2);
        // Same token replaces, not duplicates.
        assert_eq!(add_device(&path, "tok-a").unwrap(), 2);
        assert_eq!(list_devices(&path).len(), 2);
        assert!(remove_device(&path, "tok-a").unwrap());
        assert!(!remove_device(&path, "tok-missing").unwrap());
        assert_eq!(list_devices(&path).len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn list_is_empty_without_store() {
        let path = std::env::temp_dir().join(format!("blumi-fcm-none-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(list_devices(&path).is_empty());
    }
}

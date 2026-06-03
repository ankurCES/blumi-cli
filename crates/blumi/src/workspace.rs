//! Project-workspace discovery for the TUI left sidebar.
//!
//! Sources, in priority order: the current project, **pinned** entries the user
//! saved, **recent** projects opened before, and a **scan** of configured root
//! folders for git repos (defaulting to the parent of the current project, so
//! sibling repos appear automatically). All persisted in `~/.blumi/workspaces.json`.

use blumi_config::BlumiConfig;
use blumi_tui::Workspace;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default)]
    pinned: Vec<String>,
    #[serde(default)]
    recent: Vec<String>,
}

fn registry_path(config: &BlumiConfig) -> PathBuf {
    config.paths.home.join("workspaces.json")
}

fn load_registry(config: &BlumiConfig) -> Registry {
    std::fs::read_to_string(registry_path(config))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_registry(config: &BlumiConfig, reg: &Registry) {
    let path = registry_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(body) = serde_json::to_string_pretty(reg) {
        std::fs::write(path, body).ok();
    }
}

/// The display name for a workspace path (its final component).
fn label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Record `path` as recently opened (most-recent first, capped, deduped).
pub fn record_recent(config: &BlumiConfig, path: &str) {
    let mut reg = load_registry(config);
    reg.recent.retain(|p| p != path);
    reg.recent.insert(0, path.to_string());
    reg.recent.truncate(20);
    save_registry(config, &reg);
}

/// Discover workspaces for the sidebar: current + pinned + recent + scanned.
pub fn discover(config: &BlumiConfig) -> Vec<Workspace> {
    let reg = load_registry(config);
    let pinned: BTreeSet<String> = reg.pinned.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut out: Vec<Workspace> = Vec::new();

    let add = |path: String, out: &mut Vec<Workspace>, seen: &mut BTreeSet<String>| {
        let path = path.trim_end_matches('/').to_string();
        if path.is_empty() || !seen.insert(path.clone()) {
            return;
        }
        out.push(Workspace {
            name: label(&path),
            pinned: pinned.contains(&path),
            path,
        });
    };

    // Current project first.
    add(
        config.paths.working_dir.display().to_string(),
        &mut out,
        &mut seen,
    );
    // Pinned, then recent.
    for p in reg.pinned.iter().chain(reg.recent.iter()) {
        add(p.clone(), &mut out, &mut seen);
    }
    // Scan roots (default: the parent of the current project).
    let roots: Vec<PathBuf> = if config.workspaces.roots.is_empty() {
        config
            .paths
            .working_dir
            .parent()
            .map(|p| vec![p.to_path_buf()])
            .unwrap_or_default()
    } else {
        config.workspaces.roots.iter().map(PathBuf::from).collect()
    };
    for root in roots {
        if let Ok(entries) = std::fs::read_dir(&root) {
            let mut repos: Vec<String> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && p.join(".git").exists())
                .map(|p| p.display().to_string())
                .collect();
            repos.sort();
            for p in repos {
                add(p, &mut out, &mut seen);
            }
        }
    }

    // Pinned float to the top (current stays first).
    if out.len() > 1 {
        out[1..].sort_by(|a, b| b.pinned.cmp(&a.pinned).then(a.name.cmp(&b.name)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_is_basename() {
        assert_eq!(label("/a/b/blumi-cli"), "blumi-cli");
        assert_eq!(label("/a/b/blumi-cli/"), "blumi-cli");
    }
}

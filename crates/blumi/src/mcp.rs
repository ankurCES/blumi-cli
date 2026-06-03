//! `blumi mcp` — manage MCP servers: the default no-config set, a catalog of
//! configurable (keyed) servers, and enable/disable/remove. Edits
//! `~/.blumi/settings.json` directly as JSON (preserving every other key) with
//! the same atomic 0600 write the rest of the config uses.

use crate::McpCmd;
use anyhow::Context;
use blumi_config::{default_mcp_catalog, default_mcp_servers, BlumiConfig};
use serde_json::{Map, Value};
use std::path::Path;

pub fn run(action: McpCmd, config: &BlumiConfig) -> anyhow::Result<()> {
    let path = config.paths.settings_json();
    match action {
        McpCmd::List => {
            if config.mcp_servers.is_empty() {
                println!("no MCP servers configured — `blumi mcp defaults` to add the default set");
            }
            for (name, s) in &config.mcp_servers {
                let mark = if s.enabled { "●" } else { "○" };
                println!("{mark} {name:<20} {} {}", s.command, s.args.join(" "));
            }
            Ok(())
        }
        McpCmd::Catalog => {
            println!("configurable servers — `blumi mcp add <name>`, then set the env keys:\n");
            for e in default_mcp_catalog() {
                let keys = if e.required_env.is_empty() {
                    "no keys".to_string()
                } else {
                    e.required_env.join(", ")
                };
                println!(
                    "  {:<14} {}\n  {:<14} ({keys})\n",
                    e.name, e.description, ""
                );
            }
            Ok(())
        }
        McpCmd::Defaults => {
            let mut root = load(&path);
            let servers = servers_mut(&mut root);
            let mut added = 0;
            for (name, cfg) in default_mcp_servers() {
                if !servers.contains_key(&name) {
                    servers.insert(name, serde_json::to_value(cfg)?);
                    added += 1;
                }
            }
            save(&path, &root)?;
            println!(
                "✿ seeded {added} default MCP server(s) → {}",
                path.display()
            );
            Ok(())
        }
        McpCmd::Add { name } => {
            let entry = default_mcp_catalog()
                .into_iter()
                .find(|e| e.name == name)
                .with_context(|| format!("'{name}' not in the catalog — `blumi mcp catalog`"))?;
            let mut root = load(&path);
            servers_mut(&mut root).insert(name.clone(), serde_json::to_value(&entry.server)?);
            save(&path, &root)?;
            print!("✿ added '{name}' (disabled). ");
            if entry.required_env.is_empty() {
                println!("`blumi mcp enable {name}`");
            } else {
                println!(
                    "set {} in settings.json, then `blumi mcp enable {name}`",
                    entry.required_env.join(", ")
                );
            }
            Ok(())
        }
        McpCmd::Enable { name } => set_enabled(&path, &name, true),
        McpCmd::Disable { name } => set_enabled(&path, &name, false),
        McpCmd::Remove { name } => {
            let mut root = load(&path);
            let removed = servers_mut(&mut root).remove(&name).is_some();
            save(&path, &root)?;
            println!(
                "{}",
                if removed {
                    format!("✿ removed '{name}'")
                } else {
                    format!("'{name}' was not configured")
                }
            );
            Ok(())
        }
    }
}

fn set_enabled(path: &Path, name: &str, on: bool) -> anyhow::Result<()> {
    let mut root = load(path);
    let servers = servers_mut(&mut root);
    let entry = servers
        .get_mut(name)
        .with_context(|| format!("'{name}' is not configured — `blumi mcp list`"))?;
    if let Some(obj) = entry.as_object_mut() {
        obj.insert("enabled".into(), Value::Bool(on));
    }
    save(path, &root)?;
    println!("✿ {} '{name}'", if on { "enabled" } else { "disabled" });
    Ok(())
}

fn load(path: &Path) -> Value {
    let v = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if v.is_object() {
        v
    } else {
        serde_json::json!({})
    }
}

fn servers_mut(root: &mut Value) -> &mut Map<String, Value> {
    if !root.get("mcp_servers").is_some_and(Value::is_object) {
        root["mcp_servers"] = serde_json::json!({});
    }
    root["mcp_servers"].as_object_mut().expect("just ensured")
}

fn save(path: &Path, root: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(root)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).ok();
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

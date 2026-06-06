//! Self-healing evolution miner (the "Adaptive" observe→evolve loop).
//!
//! Periodically clusters recurring tool failures recorded by the reflex layer
//! (`kind="recovery"` episodes in the `agent` memory namespace). When a
//! (tool, failure-class) cluster crosses a frequency threshold, it synthesizes a
//! **recovery skill** — a low-risk, autonomous change (a brand-new `SKILL.md`,
//! never a config/secret/delete edit). Per the autonomy policy:
//!
//! - `Auto` → write the skill now + record an `evolution` audit (loads on next reload).
//! - `Propose` → record an `evolution_proposal` audit only (surfaced for a human).
//! - `Off` → do nothing.
//!
//! Everything it can do is low-risk by construction, so nothing here ever
//! bypasses approval; richer (risky) remediations would route through an
//! ApprovalRequest instead.

use blumi_config::HealEvolve;
use blumi_persist::SemanticMemoryImpl;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Run one evolution pass. Returns a human-readable log of what it did/proposed
/// (also recorded as audit memories). `min_cluster` failures of the same kind
/// before a remediation is synthesized.
pub async fn mine_once(
    mem: &SemanticMemoryImpl,
    skills_dir: &Path,
    mode: HealEvolve,
    min_cluster: usize,
) -> Vec<String> {
    if matches!(mode, HealEvolve::Off) {
        return Vec::new();
    }

    // Cluster recent recovery episodes by (tool, failure-class).
    let episodes = mem.episodes_by_kind("recovery", 300).await;
    let mut clusters: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for ep in &episodes {
        if let Some((tool, failure, action)) = parse_episode(ep) {
            let e = clusters
                .entry((tool, failure))
                .or_insert((0, action.clone()));
            e.0 += 1;
            if !action.is_empty() {
                e.1 = action;
            }
        }
    }

    // Markers for already-handled clusters (don't re-evolve the same pattern).
    let mut handled: HashSet<String> = HashSet::new();
    for t in mem.episodes_by_kind("evolution", 200).await {
        if let Some(m) = marker_of(&t) {
            handled.insert(m);
        }
    }
    for t in mem.episodes_by_kind("evolution_proposal", 200).await {
        if let Some(m) = marker_of(&t) {
            handled.insert(m);
        }
    }

    let mut log = Vec::new();
    for ((tool, failure), (count, action)) in clusters {
        if count < min_cluster {
            continue;
        }
        let marker = format!("{tool}/{failure}");
        if handled.contains(&marker) {
            continue;
        }
        let skill_name = slug(&format!("recover-{tool}-{failure}"));
        let desc = format!("Recovery playbook: handle `{tool}` {failure} failures");
        let instructions = format!(
            "# Recovering from `{tool}` {failure}\n\n\
             The `{tool}` tool has failed with **{failure}** {count} times in recent runs. \
             When it fails this way again:\n\n\
             - {guidance}\n\
             - Reuse the known-good arguments/state from prior successful calls.\n\
             - If it still fails, stop and report exactly what's blocking you.\n\n\
             _Authored automatically by blumi self-healing (cluster `{marker}`)._\n",
            guidance = action_guidance(&action),
        );

        match mode {
            HealEvolve::Auto => match blumi_skills::skill_manager::write_skill(
                skills_dir,
                &skill_name,
                &desc,
                &instructions,
            ) {
                Ok(_) => {
                    let audit = format!(
                        "evolved skill={skill_name} cluster={marker} count={count} mode=auto"
                    );
                    // origin="local" → audit never diffuses (peers must not think
                    // they've already evolved a cluster they haven't).
                    mem.add("agent", "evolution", &audit, None, "local").await;
                    log.push(audit);
                }
                Err(e) => log.push(format!("evolve: skill write failed for {marker}: {e}")),
            },
            HealEvolve::Propose => {
                let audit = format!(
                    "proposed skill={skill_name} cluster={marker} count={count} (approve to apply)"
                );
                mem.add("agent", "evolution_proposal", &audit, None, "local")
                    .await;
                log.push(audit);
            }
            HealEvolve::Off => {}
        }
    }
    log
}

/// Parse a `"tool=X failure=Y action=Z outcome=W"` episode into its parts.
fn parse_episode(text: &str) -> Option<(String, String, String)> {
    let mut tool = None;
    let mut failure = None;
    let mut action = String::new();
    for tok in text.split_whitespace() {
        if let Some(v) = tok.strip_prefix("tool=") {
            tool = Some(v.to_string());
        } else if let Some(v) = tok.strip_prefix("failure=") {
            failure = Some(v.to_string());
        } else if let Some(v) = tok.strip_prefix("action=") {
            action = v.to_string();
        }
    }
    Some((tool?, failure?, action))
}

/// Pull the `cluster=tool/failure` marker out of an audit line.
fn marker_of(text: &str) -> Option<String> {
    text.split_whitespace()
        .find_map(|t| t.strip_prefix("cluster="))
        .map(|s| s.to_string())
}

/// Human-actionable line for a recovery action code.
fn action_guidance(action: &str) -> &'static str {
    match action {
        "arg_fix" => "Re-check and correct the arguments before calling it (a required field was likely missing or malformed).",
        "state_repair" => "Re-read the current state first, then call it with the up-to-date values.",
        "retry_with_hint" => "Retry once; the failure is usually transient.",
        "alternative_or_narrow" => "Use a different tool or a narrower, more specific query.",
        _ => "Choose a different approach rather than repeating the same call.",
    }
}

/// Lowercase slug for a skill directory name (letters/digits/'-'/'_', max 64).
fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let out = out.trim_matches('-');
    out.chars().take(64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_episode_and_marker() {
        let (t, f, a) =
            parse_episode("tool=file_write failure=invalid_input action=arg_fix outcome=recovered")
                .unwrap();
        assert_eq!(t, "file_write");
        assert_eq!(f, "invalid_input");
        assert_eq!(a, "arg_fix");
        assert_eq!(
            marker_of("evolved skill=recover-x cluster=file_write/invalid_input count=4 mode=auto"),
            Some("file_write/invalid_input".to_string())
        );
    }

    #[test]
    fn slug_is_valid() {
        assert_eq!(
            slug("recover-file_write-invalid_input"),
            "recover-file-write-invalid-input"
        );
        assert!(slug("Tool!!Crash")
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }
}

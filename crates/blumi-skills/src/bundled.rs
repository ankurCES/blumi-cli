//! Skills bundled into the binary (curated third-party SKILL.md collections),
//! unpacked into `~/.blumi/skills` on first run and via `blumi skills sync`.
//!
//! Idempotent and **non-clobbering**: blumi only writes/refreshes skill dirs it
//! owns (marked with a `.bundled` file) and never touches user-authored skills.
//! A `.bundled-version` stamp lets an upgraded binary refresh its own skills
//! without re-scanning on every launch.
//!
//! Sources (MIT or BSD-3-Clause; see NOTICE): obra/superpowers (`sp-*`),
//! leonxlnx/taste-skill (`taste-*`), jeffallan/claude-skills (`cs-*`),
//! udapy/rust-agentic-skills (`ras-*`), flutter/skills (`flutter-*`),
//! dart-lang/skills (`dart-*`), workos/auth.md (`workos-auth`).

use include_dir::{include_dir, Dir};
use std::path::Path;

static BUNDLED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/bundled/skills");

/// Bumped whenever the vendored snapshot changes, so an upgraded binary
/// re-materializes its bundled skills. v2 adds flutter/skills, dart-lang/skills,
/// and udapy/rust-agentic-skills; v3 adds workos/auth.md (`workos-auth`).
const BUNDLE_VERSION: &str = "3";
/// Ownership marker written into each bundled skill dir.
const MARKER: &str = ".bundled";

/// Unpack bundled skills into `skills_dir` (e.g. `~/.blumi/skills`). Returns how
/// many skills were written/refreshed.
///
/// - missing dir → write it (+ marker)
/// - exists, ours (has marker) → refresh only if the bundle version changed
/// - exists, NOT ours → leave it alone (the user owns that name)
///
/// `force` (used by `blumi skills sync`) runs the full sweep even when the
/// version stamp is current, so deleted bundled skills get restored. On a normal
/// launch (`force=false`) an up-to-date stamp short-circuits immediately.
pub fn sync_bundled_skills(skills_dir: &Path, force: bool) -> std::io::Result<usize> {
    std::fs::create_dir_all(skills_dir)?;
    let stamp = skills_dir.join(".bundled-version");
    let up_to_date = std::fs::read_to_string(&stamp)
        .map(|s| s.trim() == BUNDLE_VERSION)
        .unwrap_or(false);
    if up_to_date && !force {
        return Ok(0);
    }

    let mut written = 0;
    for entry in BUNDLED.dirs() {
        let Some(name) = entry
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            continue;
        };
        let dest = skills_dir.join(&name);
        let exists = dest.exists();
        let owned = dest.join(MARKER).exists();

        if exists && !owned {
            continue; // user owns this name — never clobber
        }
        if exists && owned && up_to_date && !force {
            continue; // ours and current (an explicit `force` sweep refreshes anyway)
        }
        if exists {
            std::fs::remove_dir_all(&dest)?;
        }
        write_dir(entry, skills_dir)?;
        std::fs::write(dest.join(MARKER), BUNDLE_VERSION.as_bytes())?;
        written += 1;
    }

    std::fs::write(&stamp, BUNDLE_VERSION.as_bytes())?;
    Ok(written)
}

/// Recursively materialize an embedded dir under `base`.
fn write_dir(dir: &Dir, base: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(base.join(dir.path()))?;
    for f in dir.files() {
        std::fs::write(base.join(f.path()), f.contents())?;
    }
    for d in dir.dirs() {
        write_dir(d, base)?;
    }
    Ok(())
}

/// Number of skills bundled into this binary.
pub fn bundled_count() -> usize {
    BUNDLED.dirs().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_is_non_empty() {
        // 93 original + 27 from flutter/dart/rust-agentic (v2).
        assert!(bundled_count() >= 110, "expected the vendored skills");
    }

    #[test]
    fn new_repo_skills_are_present() {
        let names: Vec<String> = BUNDLED
            .dirs()
            .filter_map(|d| {
                d.path()
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .collect();
        assert!(
            names.iter().any(|n| n == "flutter-add-widget-test"),
            "flutter skill"
        );
        assert!(
            names.iter().any(|n| n == "dart-use-pattern-matching"),
            "dart skill"
        );
        assert!(
            names.iter().any(|n| n == "ras-rust-core"),
            "rust-agentic skill"
        );
        assert!(
            names.iter().any(|n| n == "workos-auth"),
            "workos auth.md skill"
        );
    }

    #[test]
    fn sync_writes_then_is_idempotent_and_non_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");

        let n = sync_bundled_skills(&skills, false).unwrap();
        assert_eq!(n, bundled_count());
        // A known skill landed with its SKILL.md + ownership marker.
        let sample = skills.join("sp-test-driven-development");
        assert!(sample.join("SKILL.md").is_file());
        assert!(sample.join(".bundled").is_file());

        // Second run: stamp current → no-op.
        assert_eq!(sync_bundled_skills(&skills, false).unwrap(), 0);

        // A user skill of the same kind of name is never touched.
        let user = skills.join("my-skill");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(user.join("SKILL.md"), "---\nname: mine\n---\nx").unwrap();
        sync_bundled_skills(&skills, true).unwrap(); // force sweep
        assert_eq!(
            std::fs::read_to_string(user.join("SKILL.md")).unwrap(),
            "---\nname: mine\n---\nx"
        );

        // force restores a deleted bundled skill.
        std::fs::remove_dir_all(&sample).unwrap();
        assert!(sync_bundled_skills(&skills, true).unwrap() >= 1);
        assert!(sample.join("SKILL.md").is_file());
    }
}

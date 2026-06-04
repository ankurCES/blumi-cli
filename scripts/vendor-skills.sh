#!/usr/bin/env bash
# Vendor third-party SKILL.md skills into crates/blumi-skills/bundled/skills/.
# The result is committed and embedded into the binary (include_dir); re-run to
# refresh. Text files only (images/binaries skipped). Each skill is namespaced by
# a repo prefix to avoid cross-repo name collisions; repos whose skills already
# carry a topic prefix upstream (flutter-/dart-) use an empty prefix and are
# vendored as-is. Each repo's LICENSE is copied into ../bundled/licenses/.
#
# Sources + licenses (see ../NOTICE for attribution):
#   sp-      obra/superpowers              MIT
#   taste-   leonxlnx/taste-skill          MIT
#   cs-      jeffallan/claude-skills       MIT
#   ras-     udapy/rust-agentic-skills     MIT
#   (none)   flutter/skills                BSD-3-Clause   (dirs already flutter-*)
#   (none)   dart-lang/skills              BSD-3-Clause   (dirs already dart-*)
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/crates/blumi-skills/bundled/skills"
LIC="$ROOT/crates/blumi-skills/bundled/licenses"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

rm -rf "$DEST" "$LIC"
mkdir -p "$DEST" "$LIC"

# triples: "prefix:org/repo:licstem"  (empty prefix → vendor dir names as-is)
for triple in \
  "sp:obra/superpowers:superpowers" \
  "taste:leonxlnx/taste-skill:taste-skill" \
  "cs:jeffallan/claude-skills:claude-skills" \
  "ras:udapy/rust-agentic-skills:rust-agentic-skills" \
  ":flutter/skills:flutter-skills" \
  ":dart-lang/skills:dart-skills" ; do
  prefix="$(printf '%s' "$triple" | cut -d: -f1)"
  repo="$(printf '%s' "$triple" | cut -d: -f2)"
  lic="$(printf '%s' "$triple" | cut -d: -f3)"
  echo "→ $repo"
  clone="$TMP/$lic"
  git clone --depth 1 -q "https://github.com/$repo" "$clone"
  [ -f "$clone/LICENSE" ] && cp "$clone/LICENSE" "$LIC/$lic-LICENSE"
  for skilldir in "$clone"/skills/*/; do
    [ -f "${skilldir}SKILL.md" ] || continue
    name="$(basename "$skilldir")"
    if [ -n "$prefix" ]; then out="$DEST/${prefix}-${name}"; else out="$DEST/${name}"; fi
    mkdir -p "$out"
    ( cd "$skilldir" && find . -type f -print ) | while IFS= read -r f; do
      case "$f" in
        *.png|*.jpg|*.jpeg|*.webp|*.gif|*.svg|*.mp4|*.mov|*.pdf|*.zip|*.ico) continue ;;
      esac
      mkdir -p "$out/$(dirname "$f")"
      cp "${skilldir}${f#./}" "$out/$f"
    done
  done
done

# workos/auth.md (MIT) — not a skills/ repo: AUTH.md is itself the skill manifest.
# Vendor it as the `workos-auth` skill (frontmatter + AUTH.md), keeping the
# service/provider implementation guides under references/.
echo "→ workos/auth.md"
clone="$TMP/workos-auth"
git clone --depth 1 -q "https://github.com/workos/auth.md" "$clone"
[ -f "$clone/LICENSE" ] && cp "$clone/LICENSE" "$LIC/workos-auth-LICENSE"
out="$DEST/workos-auth"
mkdir -p "$out/references"
# Keep the description free of ": " (invalid in an unquoted YAML scalar) and
# quote it, so the frontmatter parses cleanly.
authmd_desc="WorkOS auth.md — how an agent authenticates to a service via agentic registration (discover, register, claim with OTP when there is no user identity or no ID-JAG, exchange an ID-JAG for an access token, call the API, handle revocation). Load before implementing agent-to-service authentication, or when an API returns 401 with a WWW-Authenticate resource_metadata pointer."
{ printf -- '---\nname: workos-auth\ndescription: "%s"\n---\n\n' "$authmd_desc"; cat "$clone/AUTH.md"; } > "$out/SKILL.md"
[ -f "$clone/agent-services/README.md" ] && cp "$clone/agent-services/README.md" "$out/references/agent-services.md"
[ -f "$clone/agent-providers/README.md" ] && cp "$clone/agent-providers/README.md" "$out/references/agent-providers.md"

echo "vendored $(find "$DEST" -name SKILL.md | wc -l | tr -d ' ') skills into $DEST"
du -sh "$DEST"

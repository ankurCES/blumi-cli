#!/usr/bin/env bash
# Vendor third-party SKILL.md skills into crates/blumi-skills/bundled/skills/.
# The result is committed and embedded into the binary (include_dir); re-run to
# refresh. Text files only (images/binaries skipped). Each skill is namespaced
# by repo prefix to avoid cross-repo name collisions.
#
# Sources (all MIT-licensed):
#   sp-    obra/superpowers
#   taste- leonxlnx/taste-skill
#   cs-    jeffallan/claude-skills
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/crates/blumi-skills/bundled/skills"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

rm -rf "$DEST"
mkdir -p "$DEST"

for pair in "sp:obra/superpowers" "taste:leonxlnx/taste-skill" "cs:jeffallan/claude-skills"; do
  prefix="${pair%%:*}"
  repo="${pair#*:}"
  echo "→ $repo"
  git clone --depth 1 -q "https://github.com/$repo" "$TMP/$prefix"
  for skilldir in "$TMP/$prefix"/skills/*/; do
    [ -f "${skilldir}SKILL.md" ] || continue
    name="$(basename "$skilldir")"
    out="$DEST/${prefix}-${name}"
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

echo "vendored $(find "$DEST" -name SKILL.md | wc -l | tr -d ' ') skills into $DEST"
du -sh "$DEST"

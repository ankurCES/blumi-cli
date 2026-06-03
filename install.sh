#!/bin/sh
# blumi installer.
#
#   curl -fsSL https://raw.githubusercontent.com/ankurCES/blumi-cli/main/install.sh | sh
#
# Installs the `blumi` binary into ~/.local/bin (override with BLUMI_INSTALL_DIR).
# Downloads a prebuilt release for your platform when available, otherwise builds
# from source with cargo (needs a Rust toolchain — https://rustup.rs).
set -eu

REPO="ankurCES/blumi-cli"
REPO_URL="https://github.com/${REPO}"
BIN="blumi"
BIN_DIR="${BLUMI_INSTALL_DIR:-$HOME/.local/bin}"

pink='\033[38;5;205m'; cyan='\033[38;5;43m'; dim='\033[2m'; off='\033[0m'
say() { printf '%b\n' "$1"; }
err() { printf '%b\n' "${pink}error:${off} $1" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

say "${pink}    ✿${off}"
say "${pink}  ❀ ${cyan}◉${pink} ❀${off}   ${dim}blumi installer${off}"
say "${pink}    ✿${off}"

# ── Detect platform → Rust target triple ───────────────────────────────────
os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Darwin) os_t="apple-darwin" ;;
  Linux)  os_t="unknown-linux-gnu" ;;
  *) err "unsupported OS '$os' — build from source:\n  cargo install --git ${REPO_URL} blumi" ;;
esac
case "$arch" in
  x86_64|amd64)  arch_t="x86_64" ;;
  arm64|aarch64) arch_t="aarch64" ;;
  *) err "unsupported architecture '$arch'" ;;
esac
target="${arch_t}-${os_t}"
say "${dim}platform: ${target}${off}"

have curl || have wget || err "curl or wget is required"
dl() { if have curl; then curl -fsSL "$1" -o "$2"; else wget -qO "$2" "$1"; fi; }

mkdir -p "$BIN_DIR"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT

# ── Try a prebuilt release, else build from source ─────────────────────────
asset="blumi-${target}.tar.gz"
url="${REPO_URL}/releases/latest/download/${asset}"
how=""

if dl "$url" "$tmp/$asset" 2>/dev/null; then
  say "downloading prebuilt ${asset}…"
  tar -xzf "$tmp/$asset" -C "$tmp"
  src="$(find "$tmp" -type f -name "$BIN" | head -n1 || true)"
  [ -n "$src" ] || err "release archive did not contain '$BIN'"
  cp "$src" "$BIN_DIR/$BIN"
  chmod 0755 "$BIN_DIR/$BIN"
  how="prebuilt"
else
  say "${dim}no prebuilt binary for ${target} — building from source…${off}"
  have cargo || err "no prebuilt binary and 'cargo' was not found.\n  install Rust (https://rustup.rs), then re-run this installer."
  cargo install --git "$REPO_URL" --locked --force --root "$tmp/cargo" "$BIN"
  cp "$tmp/cargo/bin/$BIN" "$BIN_DIR/$BIN"
  how="source"
fi

say ""
say "${cyan}✓${off} installed ${BIN} (${how}) → ${BIN_DIR}/${BIN}"

# ── PATH hint ──────────────────────────────────────────────────────────────
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    say ""
    say "${dim}${BIN_DIR} is not on your PATH — add this to your shell rc:${off}"
    say "  export PATH=\"${BIN_DIR}:\$PATH\""
    ;;
esac

say ""
say "Next: run ${pink}${BIN}${off} to start, or ${pink}${BIN} login${off} to set up a provider."

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

# The blumi flower (half-block raster of the logo). Needs a truecolor terminal;
# degrades to blocks on 256-color terms.
say ''
say '        \033[38;2;195;112;207m\033[49m▄\033[38;2;186;117;217m\033[49m▄      \033[0m'
say '    \033[38;2;212;103;187m\033[49m▄\033[38;2;212;103;187;48;2;203;107;197m▀\033[38;2;195;112;207m\033[49m▄\033[38;2;195;112;207;48;2;186;117;217m▀\033[38;2;186;117;217;48;2;178;121;227m▀\033[38;2;178;121;227;48;2;169;126;238m▀\033[38;2;169;126;238;48;2;161;131;248m▀\033[38;2;153;132;255m\033[49m▄\033[38;2;153;132;255;48;2;147;125;255m▀\033[38;2;141;118;255m\033[49m▄  \033[0m'
say '    \033[38;2;195;112;207m\033[49m▄\033[38;2;195;112;207;48;2;186;117;217m▀\033[38;2;186;117;217;48;2;178;121;227m▀\033[38;2;178;121;227;48;2;169;126;238m▀\033[38;2;169;126;238;48;2;104;255;214m▀\033[38;2;161;131;248;48;2;104;255;214m▀\033[38;2;153;132;255;48;2;147;125;255m▀\033[38;2;147;125;255;48;2;141;118;255m▀\033[38;2;141;118;255;48;2;135;111;255m▀\033[38;2;129;104;255m\033[49m▄  \033[0m'
say '   \033[38;2;195;112;207;48;2;186;117;217m▀\033[38;2;186;117;217;48;2;178;121;227m▀\033[38;2;178;121;227;48;2;169;126;238m▀\033[38;2;169;126;238;48;2;161;131;248m▀\033[38;2;104;255;214;48;2;104;255;214m▀\033[38;2;14;17;22;48;2;14;17;22m▀\033[38;2;14;17;22;48;2;14;17;22m▀\033[38;2;104;255;214;48;2;104;255;214m▀\033[38;2;135;111;255;48;2;129;104;255m▀\033[38;2;129;104;255;48;2;122;97;255m▀\033[38;2;122;97;255;48;2;116;91;255m▀\033[38;2;116;91;255;48;2;110;84;255m▀ \033[0m'
say '    \033[38;2;169;126;238m\033[49m▀\033[38;2;161;131;248;48;2;153;132;255m▀\033[38;2;153;132;255;48;2;147;125;255m▀\033[38;2;147;125;255;48;2;141;118;255m▀\033[38;2;104;255;214;48;2;135;111;255m▀\033[38;2;104;255;214;48;2;129;104;255m▀\033[38;2;129;104;255;48;2;122;97;255m▀\033[38;2;122;97;255;48;2;116;91;255m▀\033[38;2;116;91;255;48;2;110;84;255m▀\033[38;2;110;84;255m\033[49m▀  \033[0m'
say '    \033[38;2;153;132;255m\033[49m▀\033[38;2;147;125;255;48;2;141;118;255m▀\033[38;2;141;118;255m\033[49m▀\033[38;2;135;111;255;48;2;129;104;255m▀\033[38;2;129;104;255;48;2;122;97;255m▀\033[38;2;122;97;255;48;2;116;91;255m▀\033[38;2;116;91;255;48;2;110;84;255m▀\033[38;2;110;84;255m\033[49m▀\033[38;2;107;93;252;48;2;106;119;246m▀\033[38;2;106;119;246m\033[49m▀  \033[0m'
say '        \033[38;2;116;91;255m\033[49m▀\033[38;2;110;84;255m\033[49m▀      \033[0m'
say ''
say "  ${pink}blumi${off} ${dim}installer${off}"

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

# macOS (esp. 26+) kills a binary at runtime when its code signature doesn't
# match its bytes ("killed: Code Signature Invalid") — which happens to a
# downloaded or copied binary. Re-sign ad-hoc so it runs. Best-effort.
if [ "$(uname -s)" = "Darwin" ] && have codesign; then
  codesign --force --sign - "$BIN_DIR/$BIN" >/dev/null 2>&1 || true
fi

say ""
say "${cyan}✓${off} installed ${BIN} (${how}) → ${BIN_DIR}/${BIN}"

# ── Runtimes for the default MCP servers (uv for python, node for npx) ──────
# Auto-installed so the bundled MCP servers work on a fresh machine. Best-effort
# and idempotent; never fails the install. Set BLUMI_SKIP_RUNTIMES=1 to skip.
if [ -z "${BLUMI_SKIP_RUNTIMES:-}" ]; then
  run_sh() { if have curl; then curl -fsSL "$1" | "$2"; else wget -qO- "$1" | "$2"; fi; }

  if ! have uvx && ! have uv; then
    say "${dim}installing uv (python runner for MCP)…${off}"
    run_sh "https://astral.sh/uv/install.sh" sh >/dev/null 2>&1 \
      || say "${dim}  uv: skipped — install manually for python MCP servers${off}"
    case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) PATH="$HOME/.local/bin:$PATH" ;; esac
  fi

  if ! have npx; then
    if have bash; then
      say "${dim}installing node via fnm (npx runner for MCP)…${off}"
      run_sh "https://fnm.vercel.app/install" bash >/dev/null 2>&1 || true
      for d in "$HOME/.local/share/fnm" "$HOME/.fnm"; do [ -d "$d" ] && PATH="$d:$PATH"; done
      if have fnm; then
        fnm install --lts >/dev/null 2>&1 || true
        eval "$(fnm env 2>/dev/null)" 2>/dev/null || true
      fi
    fi
    have npx || say "${dim}  node: install Node 18+ for the npx-based MCP servers${off}"
  fi
fi

# ── Pre-populate ~/.blumi: bundled skills + default MCP servers ─────────────
if "$BIN_DIR/$BIN" skills sync >/dev/null 2>&1; then
  say "${cyan}✓${off} bundled skills ready"
fi
if "$BIN_DIR/$BIN" mcp defaults >/dev/null 2>&1; then
  say "${cyan}✓${off} default MCP servers configured"
fi

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

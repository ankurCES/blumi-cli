# blumi

<p align="center">
  <img src="assets/blumi-logo.svg" alt="blumi" width="520">
</p>

<p align="center">
  <em>A local-first, provider-agnostic agentic coding companion —<br>
  one Rust core, three faces: a terminal UI, a web UI, and a phone app.</em>
</p>

<p align="center">
  <a href="https://github.com/ankurCES/blumi-cli/actions/workflows/ci.yml"><img src="https://github.com/ankurCES/blumi-cli/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/ankurCES/blumi-cli/wiki"><img src="https://img.shields.io/badge/docs-wiki-7c5cff" alt="Wiki"></a>
</p>

`blumi` is a single Rust binary whose UI-agnostic core emits one typed event stream, so every
surface shows the same session: a [crush](https://github.com/charmbracelet/crush)-inspired
**terminal UI**, an embedded React **web UI**, an always-on **gateway**, and **blugo** — a
Flutter **phone app** that's a 1:1 mirror of the TUI, optimized for foldables.

|  Terminal UI (`blumi tui`) | Phone app (blugo) |
|---|---|
| <img src="docs/screenshots/tui-landing.png" alt="blumi TUI" width="460"> | <img src="docs/screenshots/app-chat.png" alt="blugo chat" width="230"> |

> 📖 **Full setup & help lives in the [Wiki](https://github.com/ankurCES/blumi-cli/wiki)** —
> installation, configuration, the always-on gateway, the mobile app, the distributed grid,
> voice, self-management, and troubleshooting, each with step-by-step guides for different setups.

---

# Part 1 — blumi CLI

The agent itself: a terminal UI, a one-shot headless runner, an embedded web UI, and an
always-on gateway. One binary, no daemon required.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/ankurCES/blumi-cli/main/install.sh | sh
```

Installs the `blumi` binary into `~/.local/bin` (override with `BLUMI_INSTALL_DIR`) — a prebuilt
release when available, otherwise a `cargo` build from source (needs a Rust toolchain,
https://rustup.rs).

<details>
<summary>Other ways to install</summary>

```sh
# from source with cargo (needs Rust)
cargo install --git https://github.com/ankurCES/blumi-cli --locked blumi

# or clone and build
git clone https://github.com/ankurCES/blumi-cli && cd blumi-cli
cargo install --path crates/blumi --locked
```
</details>

See **[Installation](https://github.com/ankurCES/blumi-cli/wiki/Installation)** for per-OS notes.

## Quick start

```sh
blumi login          # pick a provider, paste a key/endpoint, choose a model
blumi                # start the terminal UI (default when attached to a TTY)
blumi run "explain src/main.rs"     # one-shot / pipeable / headless
blumi web            # embedded React web UI + HTTP/SSE server
```

Configuration lives in `~/.blumi/settings.json` (and a per-project `.blumi/`). Providers, keys,
models, personas, permissions, executor, voice, and the grid are all set there or via `blumi
login` / the in-app settings. See **[Configuration](https://github.com/ankurCES/blumi-cli/wiki/Configuration)**.

<p align="center">
  <img src="docs/screenshots/tui-dashboard.png" alt="blumi TUI 3-pane dashboard" width="880"><br>
  <em>Fold-open / wide layout: explorer │ chat │ agent rail.</em>
</p>

## Surfaces

| Command | What it does |
|---|---|
| `blumi` / `blumi tui` | Interactive terminal UI (default on a TTY) |
| `blumi run "<prompt>"` | One-shot / headless / pipeable agent run |
| `blumi web` | Embedded React web UI + HTTP/SSE server |
| `blumi serve` | **Always-on gateway** for the blugo phone app (run/pair/install/start/stop/status) |
| `blumi loop` | Autonomously work the task board: select → run → advance, repeat |
| `blumi task` | Manage the task board (the work queue for `blumi loop`) |
| `blumi gateway` | Run as a messaging bot (Telegram/Discord/Slack/WhatsApp) |
| `blumi cron` / `playbook` / `skills` / `mcp` / `session` / `stats` | Automations & management |

Run `blumi <command> --help` for any subcommand. Full reference:
**[CLI Usage](https://github.com/ankurCES/blumi-cli/wiki/CLI-Usage)**.

## Always-on gateway

Run blumi as a background service so a phone (or browser) can reach it over your LAN:

```sh
blumi serve pair                       # set a password, print the LAN URL + QR for blugo
blumi serve install --host <LAN-ip>    # install as a launchd (macOS) / systemd-user (Linux) service
blumi serve status                     # is it up? URL + pid
```

It auto-advertises over mDNS (`_blumi._tcp`) so blugo discovers it on the same Wi-Fi.
Guide: **[Gateway (blumi serve)](https://github.com/ankurCES/blumi-cli/wiki/Gateway)**.

## Grid (distributed)

Several gateways that share one **grid secret** form a *grid*: they auto-discover each other and
hand off tasks for execution on remote runtimes (orchestrator-dispatch). Enable per node in
`settings.json`:

```json
"grid": { "enabled": true, "secret": "one-shared-secret" }
```

Same secret = same grid. Guide: **[Grid (distributed)](https://github.com/ankurCES/blumi-cli/wiki/Grid)**.

## Workspace layout

```
crates/
  blumi-protocol   wire contract: Command / Event / Message / ToolResult (pure serde)
  blumi-core       the brain: traits + session actor (agent loop) + context mgmt + permissions
  blumi-llm        provider clients (OpenAI-compatible, Anthropic, Gemini, …)
  blumi-tools      built-in tools + JSON-Schema validation + execution pipeline
  blumi-exec       execution backends (Local; Docker/SSH feature-gated)
  blumi-mcp        MCP client (rmcp) + tool adapters
  blumi-lsp        generic LSP client (feature-gated)
  blumi-persist    SQLite (sqlx): sessions, messages, checkpoints, FTS5 search
  blumi-skills     SKILL.md skills + dual memory (MEMORY/USER) + self-management tools
  blumi-cron       scheduler → headless sessions → delivery
  blumi-gateway    messaging gateways + voice (feature-gated)
  blumi-task       persistent task board (the queue for `blumi loop`)
  blumi-config     layered configuration (figment)
  blumi-tui        ratatui terminal UI
  blumi-web        axum server + embedded React build
  blumi            the binary (clap) — incl. `serve` gateway + `grid` discovery/dispatch
blugo/             the Flutter phone app (outside the cargo workspace)
```

## Develop

```sh
cargo build
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

CI runs the Rust gate + the blugo Flutter gate on every push/PR (`.github/workflows/ci.yml`).
The web UI lives in `crates/blumi-web/frontend` (React + Vite + TS); its built `dist/` is
committed and embedded via `rust-embed`, so a plain `cargo build` needs no JS toolchain.
Contributing notes: **[Development](https://github.com/ankurCES/blumi-cli/wiki/Development)**.

---

# Part 2 — blugo (phone app)

**blugo** is a Flutter app that mirrors the TUI on your phone, talking to a `blumi serve` gateway
over your LAN (REST + SSE, token auth). Built for the Pixel 9 Pro Fold — single-pane in portrait,
multi-pane when unfolded.

| Chat · markdown & code | Approvals · thinking | Control center |
|---|---|---|
| <img src="docs/screenshots/app-chat.png" alt="blugo chat" width="240"> | <img src="docs/screenshots/app-approval.png" alt="blugo approval card" width="240"> | <img src="docs/screenshots/app-control.png" alt="blugo control center" width="240"> |

**Highlights:** streaming chat with markdown + syntax-highlighted code, tool cards, approval /
clarify / plan cards, the animated thinking mascot, sessions, a control center
(model / persona / theme / YOLO / voice / tasks / usage / skills / memory), LAN auto-discovery of
gateways, multiple saved instances, and voice (ElevenLabs / OpenAI TTS + Whisper STT).

## Connect it

1. On your machine: `blumi serve pair` then `blumi serve install --host <LAN-ip>`.
2. Open blugo → it auto-discovers gateways on your Wi-Fi (or add one by IP) → enter the password.
3. Chat. The same session is live in the TUI, the web UI, and the phone at once.

## Build & run

```sh
cd blugo
flutter pub get
flutter run -d <device>          # debug to a connected device
flutter build apk --release      # signed release APK (see blugo/README.md)
```

Details, signing, and on-device tips: **[blugo/README.md](blugo/README.md)** and the
**[Mobile App](https://github.com/ankurCES/blumi-cli/wiki/Mobile-App)** wiki page.

---

## Status

Active development; usable end-to-end. The core spine (session actor + single event stream), the
TUI, the embedded web UI, the full provider matrix (OpenAI-compatible, Anthropic, Azure Foundry,
Gemini), sub-agents, MCP, SKILL.md skills + dual memory, FTS5 session search, cron, Docker/SSH
executors, LSP, playbooks, messaging gateways + voice, the task board + autonomous `blumi loop`,
the local-LLM approval **brain**, the **`blumi serve` gateway**, the **blugo** phone app, and the
**grid** (peer discovery + task hand-off, landing incrementally) are all in place.

Permissions are interactive by default; a toggleable **YOLO mode** skips prompts (`ctrl+y` /
`/yolo` in the TUI, the web header toggle, or `--yolo` for headless runs). When a turn stops only
because it hit the per-turn tool cap, the runtime **auto-continues** in the same session and
narrates each step, bounded by a step budget and a token ceiling — so long tasks finish without
nudging. See the [Wiki](https://github.com/ankurCES/blumi-cli/wiki) for the full feature tour.

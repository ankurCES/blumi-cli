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
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License: Apache-2.0"></a>
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

### 🌐 One grid, every machine you own

blumi turns the idle compute on your LAN into a single **distributed AI grid**. Point several
`blumi serve` gateways at the same secret and they auto-discover each other; then **fan one task
across all of them** — from the terminal, the web UI, or right from your phone — and each machine
runs its share and reports back, tagged by hostname and OS. When compute is expensive, none of
yours sits idle. → jump to **[Grid (distributed)](#grid-distributed)**.

<p align="center">
  <img src="docs/screenshots/grid-desk.jpg" alt="blumi running at once across a MacBook Air, a MacBook Pro, a Linux laptop, and a foldable phone" width="900"><br>
  <em>One job, four faces: a MacBook Air (Apple Silicon), a MacBook Pro (the orchestrator), a Linux laptop (x86_64),
  and the <strong>blugo</strong> phone app — every node running blumi, sharing the work.</em>
</p>

<p align="center">
  <img src="docs/screenshots/grid-delegate-tab.jpg" alt="blugo Grid tab: delegate a task across peers, with per-machine results" width="250">
  &nbsp;&nbsp;
  <img src="docs/screenshots/grid-phone-in-hand.jpg" alt="driving the grid from the phone with three machines computing behind it" width="380">
</p>
<p align="center">
  <em>Delegate a task across the grid from your phone (left) — pick a peer or broadcast to all —
  and watch every machine answer (right). No model tool-calling required.</em>
</p>

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

## Architecture

One **UI-agnostic core** emits a single typed event stream, so the terminal UI, the web UI, and
the blugo phone app are all just renderers of the same session. A turn flows **Command → session
actor → tools → grid**, and streams back as **Events** that re-render every surface.

<p align="center">
  <img src="docs/diagrams/tui-architecture.png" alt="blumi TUI architecture — ratatui MVU UI, the blumi-core session actor, tools/runtime, and the grid, with the numbered request (Command) and response (Event) flow" width="940">
</p>

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

<p align="center">
  <img src="docs/diagrams/grid-flow.gif" alt="Animated network flow — a task sent from the blugo phone app fans out across the grid to every peer, and the results return to the requester" width="900"><br>
  <em>Grid task execution: a task from <strong>blugo</strong> → the orchestrator → fanned to every live peer → results return, tagged by machine.</em>
</p>

Several gateways that share one **grid secret** form a *grid*: they auto-discover each other on the
LAN and hand work off for execution on remote runtimes (orchestrator-dispatch). Discovery is mDNS
(`_blumi._tcp`) with **optional static peers** for networks where multicast is locked down. Every
result comes back tagged with the machine that produced it (hostname + OS), and live runs stream
into any TUI/blugo via `/remote` attach.

**Three ways to distribute work across the grid:**

- **From the phone — blugo's `Grid` tab** *(deterministic, model-independent)*: pick *all peers* or
  one, type a task, tap **Delegate over grid** → each machine runs it and reports back. It's a
  direct dispatch over the API, so it works on **any model** (no tool-calling required).
- **From chat — the `grid_dispatch` tool**: the agent spreads sub-tasks across peers and collates
  the per-machine results into one reply (terminal, web, or phone).
- **Distributed task board — `blumi loop` (grid mode)**: round-robins the task board across live
  peers; the board shows which machine ran (and is running) each task.

Enable per node in `settings.json` — **same secret = same grid**:

```json
"grid": {
  "enabled": true,
  "secret": "one-shared-secret-on-every-node",
  "peers": ["10.0.0.150:7777", "10.0.0.113:7777"]
}
```

`peers` is optional (mDNS finds peers automatically); list `IP:port` of the other nodes when
multicast is unavailable. Restart each gateway after editing —
`launchctl kickstart -k gui/$(id -u)/com.blumi.serve` (macOS) /
`systemctl --user restart blumi-serve` (Linux). Full walkthrough:
**[Grid (distributed)](https://github.com/ankurCES/blumi-cli/wiki/Grid)**.

<p align="center">
  <img src="docs/screenshots/grid-delegate-tab.jpg" alt="blugo Grid delegation tab with per-machine results" width="250">
  &nbsp;
  <img src="docs/screenshots/grid-dispatch-phone.jpg" alt="grid_dispatch fan-out across peers in blugo chat" width="250">
  &nbsp;
  <img src="docs/screenshots/grid-leaderboard-phone.jpg" alt="per-machine grid benchmark leaderboard in blugo" width="250">
</p>
<p align="center">
  <em>From the phone: the <strong>Grid delegation tab</strong> with live per-machine results (left),
  a chat-driven <code>grid_dispatch</code> fan-out (center), and a per-machine leaderboard (right).</em>
</p>

<p align="center">
  <img src="docs/screenshots/grid-tui-mac.png" alt="blumi TUI executing on the macOS (Apple Silicon) peer" width="430">
  &nbsp;
  <img src="docs/screenshots/grid-tui-linux.png" alt="blumi TUI executing on the Linux (x86_64) peer" width="430"><br>
  <em>The same job executing live on two peers — macOS / Apple Silicon (left) and Linux / x86_64 (right).</em>
</p>

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
(model / persona / theme / YOLO / voice / tasks / **grid** / usage / skills / memory), a **Grid tab**
that delegates a task across your LAN grid and shows each machine's result (works on any model),
LAN auto-discovery of gateways, multiple saved instances, and voice (ElevenLabs / OpenAI TTS +
Whisper STT).

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
**distributed grid** (LAN peer discovery + chat / phone / loop task delegation, each result tagged
by machine) are all in place.

Permissions are interactive by default; a toggleable **YOLO mode** skips prompts (`ctrl+y` /
`/yolo` in the TUI, the web header toggle, or `--yolo` for headless runs). When a turn stops only
because it hit the per-turn tool cap, the runtime **auto-continues** in the same session and
narrates each step, bounded by a step budget and a token ceiling — so long tasks finish without
nudging. See the [Wiki](https://github.com/ankurCES/blumi-cli/wiki) for the full feature tour.

---

## License

Licensed under the **[Apache License 2.0](LICENSE)** © 2026 ankurCES — see [`LICENSE`](LICENSE)
and [`NOTICE`](NOTICE). Permissive, with an explicit patent grant. Contributions welcome.

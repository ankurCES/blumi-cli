# blumi-cli

```
      ✿
    ❀ ◉ ❀
      ✿
   b l u m i
```

`blumi` — a local-first, provider-agnostic agentic coding assistant. A single Rust binary with
a crush-inspired terminal UI and a packaged React web UI, both driven by one shared, UI-agnostic
core that emits a single typed event stream.

> Ground-up Rust rewrite of OpenMonoAgent.ai, folding in features from hermes-agent, with TUI
> design from crush and a web UI inspired by hermes-webui. See the plan for full details.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/ankurCES/blumi-cli/main/install.sh | sh
```

This installs the `blumi` binary into `~/.local/bin` (override with `BLUMI_INSTALL_DIR`). It
downloads a prebuilt release for your platform when one is available, and otherwise builds from
source with `cargo` (a Rust toolchain — https://rustup.rs — is required for the source path).

Then run `blumi login` to pick a provider and key, and `blumi` to start.

<details>
<summary>Other ways to install</summary>

```sh
# straight from source with cargo (needs Rust)
cargo install --git https://github.com/ankurCES/blumi-cli --locked blumi

# or clone and build
git clone https://github.com/ankurCES/blumi-cli && cd blumi-cli
cargo install --path crates/blumi --locked
```
</details>

## Surfaces

| Command | What it does |
|---|---|
| `blumi` / `blumi tui` | Interactive terminal UI (default when attached to a TTY) |
| `blumi run "<prompt>"` | One-shot / headless / pipeable agent run |
| `blumi web` / `blumi serve` | Embedded React web UI + HTTP/SSE server |
| `blumi cron`, `blumi skills`, `blumi memory`, `blumi config`, `blumi session` | Management subcommands |

## Workspace layout

```
crates/
  blumi-protocol   wire contract: Command / Event / Message / ToolResult (pure serde)
  blumi-core       the brain: traits + session actor (agent loop) + context mgmt + permissions
  blumi-llm        provider clients (OpenAI-compatible, Anthropic, Gemini, …)
  blumi-tools      built-in tools + JSON-Schema validation + layered execution pipeline
  blumi-exec       execution backends (Local; Docker/SSH feature-gated)
  blumi-mcp        MCP client (rmcp) + tool adapters
  blumi-lsp        generic LSP client (feature-gated)
  blumi-persist    SQLite (sqlx): sessions, messages, checkpoints, FTS5 search
  blumi-skills     SKILL.md skills + dual memory (MEMORY/USER)
  blumi-cron       scheduler → headless sessions → delivery
  blumi-gateway    messaging gateways + voice (feature-gated)
  blumi-config     layered configuration (figment)
  blumi-tui        ratatui terminal UI
  blumi-web        axum server + embedded React build
  blumi            the binary (clap)
```

## Develop

```sh
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The web UI lives in `crates/blumi-web/frontend` (React + Vite + TS); its built `dist/` is
committed and embedded via `rust-embed`, so a plain `cargo build` needs no JS toolchain. To work
on the UI: `cd crates/blumi-web/frontend && npm install && npm run build`.

## Status

Active development; the binary is usable end-to-end. Implemented: the core spine (session actor +
single event stream), the crush-inspired TUI, the embedded React web UI (axum + SSE), the full
provider matrix (OpenAI-compatible, Anthropic, Azure Foundry, Gemini), sub-agents, MCP, SKILL.md
skills + dual memory, FTS5 session search, cron, Docker/SSH executors, LSP, playbooks, messaging
gateways (Telegram/Discord/Slack/WhatsApp) + voice, a persistent task board with an autonomous
`blumi loop` (also runnable in-TUI), a local-LLM **approval brain** (`/brain`
off/advisory/auto), and **remote-instance tabs** to drive other `blumi web` servers from the TUI.

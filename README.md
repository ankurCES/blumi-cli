# lumi-cli

`lumi` — a local-first, provider-agnostic agentic coding assistant. A single Rust binary with
a crush-inspired terminal UI and a packaged React web UI, both driven by one shared, UI-agnostic
core that emits a single typed event stream.

> Ground-up Rust rewrite of OpenMonoAgent.ai, folding in features from hermes-agent, with TUI
> design from crush and a web UI inspired by hermes-webui. See the plan for full details.

## Surfaces

| Command | What it does |
|---|---|
| `lumi` / `lumi tui` | Interactive terminal UI (default when attached to a TTY) |
| `lumi run "<prompt>"` | One-shot / headless / pipeable agent run |
| `lumi web` / `lumi serve` | Embedded React web UI + HTTP/SSE server |
| `lumi cron`, `lumi skills`, `lumi memory`, `lumi config`, `lumi session` | Management subcommands |

## Workspace layout

```
crates/
  lumi-protocol   wire contract: Command / Event / Message / ToolResult (pure serde)
  lumi-core       the brain: traits + session actor (agent loop) + context mgmt + permissions
  lumi-llm        provider clients (OpenAI-compatible, Anthropic, Gemini, …)
  lumi-tools      built-in tools + JSON-Schema validation + layered execution pipeline
  lumi-exec       execution backends (Local; Docker/SSH feature-gated)
  lumi-mcp        MCP client (rmcp) + tool adapters
  lumi-lsp        generic LSP client (feature-gated)
  lumi-persist    SQLite (sqlx): sessions, messages, checkpoints, FTS5 search
  lumi-skills     SKILL.md skills + dual memory (MEMORY/USER)
  lumi-cron       scheduler → headless sessions → delivery
  lumi-gateway    messaging gateways + voice (feature-gated)
  lumi-config     layered configuration (figment)
  lumi-tui        ratatui terminal UI
  lumi-web        axum server + embedded React build
  lumi            the binary (clap)
```

## Build

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Status

Early development. Phase 0 (scaffolding) + Phase 1 (core spine) in progress.

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

## Build

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Status

Early development. Phase 0 (scaffolding) + Phase 1 (core spine) in progress.

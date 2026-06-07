# Changelog

All notable changes to **blumi** (and the **blugo** companion app) are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The Rust workspace and the blugo app share the version number.

## [Unreleased]

### Added

- **Smart (cost-aware) model routing** (PilotDeck-inspired) — per turn, a fast
  heuristic (and, on ambiguous turns, a local "judge" model) picks a difficulty
  **tier** and routes to a light vs flagship model; delegated **sub-agents default
  to the cheap tier**. Config `router` (`mode` = off|heuristic|hybrid|judge,
  `light`/`heavy`/`judge` provider+model, `heuristics`, `subagent_tier`,
  `prefer_grid_light`); default **off**. Live toggle + savings view: TUI `/route`
  (per-tier counts + `$ saved` vs all-heavy), `GET /api/route`,
  `Command::SetRouterMode`. Model swaps are gated to tier changes (prompt-cache
  safe); the judge fails safe to the light tier.
- **White-box memory editor** — list / view / **pin** / delete / edit individual
  semantic-memory entries (not just the MEMORY.md/USER.md files). Pinned entries
  are exempt from SEDM eviction + consolidation; editing re-embeds + resyncs FTS5.
  `POST /api/memory/{list,pin,delete,update}` (migration `0007` adds a `pinned`
  column). Editing/pinning a `user`-namespace entry stays local (never diffuses).
- **Always-on proactive discovery** (PilotDeck-inspired; **off by default**) — the
  gateway periodically (gated by cadence / rate-limit / board-busy / open-cap)
  runs one **read-only** turn to surface candidate tasks, adds them to the board as
  `Discovered:` todos, and lands a redacted markdown report (`~/.blumi/reports/`) +
  an `agent`-namespace `discovery` memory. Config `always_on`
  (`enabled`/`autonomy`/`cadence_secs`/…). Surfaced via `GET /api/always-on`, a
  `blumi serve status` line, and the TUI `/discoveries` overlay. (Autonomous
  low-risk *execution* in a worktree/snapshot is a planned follow-up.)
- **Per-task cost telemetry** — each board `Task` now accumulates `input_tokens` /
  `output_tokens` / `cost_usd` (priced from the model's list price); `blumi loop`
  records the per-task token delta, surfaced in the TUI `/board` ($/task + total)
  and `/api/tasks`. The model price map moved to `blumi_config::pricing` so
  routing, per-task cost, and the TUI meter share one source of truth.
- **Unified `blumi serve` + web UI** — `blumi serve` already serves the embedded
  React UI; `blumi web` is now framed as a localhost dev shortcut (+ a `--port`
  flag), and the Web-UI URL is printed by `serve pair` / `install` / `status`.

- **TUI `/open-workspace`** — a file-browser popup to open any folder as a
  workspace: `↑/↓` move, `→` enter a folder, `←`/backspace go up, **space** opens
  the highlighted folder as a workspace (keep browsing), **enter** opens + closes,
  `esc` cancels. Git repos are flagged; opened folders appear in the left
  workspace pane immediately and are persisted to recents.

- **Grid-embed offload transport** — `embeddings.backend = "grid"` now routes
  embedding to the strongest GPU peer via a `GridEmbed` hook + secret-authed
  `POST /api/grid/embed`, with a TTL-cached peer choice and a local fallback
  (a lean node degrades to FTS5 when no peer is up). Closes the v0.2.0 follow-up.
- **Cross-step recovery confirmation** — a guided recovery is marked `verified`
  only when the retried tool actually succeeds on a later step (ground truth, not
  just "a fix was suggested"); the confirmed fix's utility is reinforced. Toggle
  with `heal.verify` (the field's meaning is now cross-step confirmation, no LLM).
- **TUI `/heal` overlay** — a self-healing summary (recovery / evolution / proposal
  counts + recent items) via a new `Store::heal_summary`, alongside the existing
  inline `⚕ self-heal` traces and the blugo Heal tab / `/api/heal`.

### Fixed

- **NVIDIA CUDA build on Linux** (`BLUMI_CUDA=1`) — two issues:
  - *Build:* pin `ort-sys` to `=2.0.0-rc.9` and restore `--locked` on the
    installer's CUDA path. `ort`'s range dependency on `ort-sys` floated to rc.12
    on a non-locked resolve, whose `download-binaries` build is broken
    (TLS-feature / ureq mismatch).
  - *Runtime:* CUDA's ONNX Runtime is a **shared** lib, so `cargo install` (binary
    only) left `libonnxruntime.so` unresolvable → every `blumi` invocation failed
    with "error while loading shared libraries". The installer now ships the `.so`
    next to the binary (`copy-dylibs` + `$ORIGIN` rpath) and **verifies the binary
    loads**, auto-falling back to a lean (CPU) build otherwise — so a reinstall can
    never leave a binary that won't start.
  Apple CoreML builds were unaffected (statically linked, already `--locked`). For
  Linux GPU the reliable path remains a local server (Ollama) for LLM + embeddings.

## [0.2.0] — 2026-06-06

First release with a tracked changelog. Adds GPU/accelerator support and a
self-healing, self-evolving agent layer on top of the existing graph-SEDM memory.

### Added

- **GPU / MLX acceleration.** Runtime accelerator detection (`Apple CoreML/Metal`,
  `NVIDIA CUDA`, or `CPU`); the bundled ONNX embedder runs on the GPU when present
  and falls back to CPU automatically. Apple CoreML is on by default on Apple
  Silicon; NVIDIA CUDA is opt-in.
- **`blumi accel {detect,status,doctor}`** — inspect detected hardware, the active
  execution provider, and copy-paste setup hints for local GPU servers.
- **Local-GPU-server backends** — `local-mlx` / `local-cuda` provider presets
  (plus `ollama`) so embeddings *and* LLM inference can run on a local GPU server
  (MLX / vLLM / llama.cpp / Ollama) via the OpenAI-compatible backend.
- **GPU-aware grid** — each node reports its accelerator in `/api/grid/metrics`
  with a `strongest_node` summary (CUDA > Apple CoreML > CPU); surfaced in the TUI
  (`/accel`), `/api/status`, and the blugo Status/Grid panels.
- **Self-healing reflex recovery** (after arXiv 2606.01416) — failed tool results
  are classified, given a budgeted/targeted recovery action, and emitted as
  `Event::Recovery` traces (`⚕ self-heal …` inline in the TUI). Only idempotent
  tools auto-retry; composes with the existing doom-loop guard.
- **Failure→fix memory learning** — recoveries are stored as episodes in the
  `agent` namespace (so they diffuse across the grid); a similar future failure
  recalls the known fix. Paths/secrets are redacted before storage.
- **Self-evolution** — a miner clusters recurring failures into auto-written
  recovery skills (low-risk; risky changes require approval), scheduled on the
  gateway sweep. Audited and surfaced via `GET /api/heal` and the blugo **Heal** tab.
- **Config:** `AccelerationConfig` (`acceleration.mode` / `embeddings_accel`) and
  `HealConfig` (`heal.enabled` / `recovery_budget` / `verify` / `learn` /
  `evolve` / `redact_paths`).
- **Installer:** detects an NVIDIA GPU and supports `BLUMI_CUDA=1` to build the
  in-process CUDA embedder (with automatic fall-back to a lean build).

### Changed

- The bundled ONNX embedder (`fastembed`/`ort`) is now **Apple-Silicon-default**
  and **opt-in elsewhere** (`--features local-embeddings` for CPU, `--features
  gpu-cuda` for NVIDIA). Linux/Windows/CI builds stay lean (FTS5 fallback) and no
  longer perform a multi-GB native link by default.
- ONNX Runtime (`ort`) logs are floored at `warn` by default (its per-allocation
  `DEBUG` spam is muted unless you name `ort` in `RUST_LOG`).

### Fixed

- **CoreML release link on Apple Silicon** — link `libclang_rt.osx.a` so ort's
  CoreML execution provider resolves `___isPlatformVersionAtLeast`; `cargo install`
  (release) now links instead of failing with an undefined symbol.
- **Linux build no longer freezes low-RAM/headless boxes** — the heavy embedder is
  no longer in the default build (see *Changed*), removing the release-link memory
  spike that could hang a machine.

### Known follow-ups

- Grid embeddings **offload transport** (`embeddings.backend = "grid"`) currently
  degrades to the local embedder; per-node accelerator reporting + `strongest_node`
  selection are in place, the peer-routed POST is pending.
- Brain-verification of recovered trajectories is scaffolded but off by default
  (`heal.verify = false`); the `Event::Recovery.verified` field is reported as
  `null` until it lands.

## [0.1.0]

Initial development series (pre-changelog) — CLI + TUI, web UI, always-on gateway,
messaging gateways, the distributed grid, durable execution, graph-SEDM semantic
memory, the native code knowledge base, and the blugo phone app. See the git
history for details.

[Unreleased]: https://github.com/ankurCES/blumi-cli/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/ankurCES/blumi-cli/releases/tag/v0.2.0
[0.1.0]: https://github.com/ankurCES/blumi-cli/commits/main

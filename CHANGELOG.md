# Changelog

All notable changes to **blumi** (and the **blugo** companion app) are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The Rust workspace shares one version (the `blumi` CLI + `v*` tags); the **blugo** app
tracks its own Flutter version (`x.y.z+build`).

## [Unreleased]

### Added

- **Structural code graph (opt-in).** A new `code-graph` build feature +
  `knowledge.graph.mode = "structural"` upgrades the code knowledge base from
  name-co-occurrence edges to a **typed, scope-resolved** graph via tree-sitter
  for **Rust, Python, Go, JavaScript, and TypeScript**: declarations carry a
  fully-qualified name / parent / signature, and reference sites resolve into
  typed `code_edges` (`call` / `type` / `implements` / `contains`; `resolved=1`
  when unambiguous). A unified **`code_graph`** agent tool exposes `callers` /
  `callees` / `impact` / `implementers` — `impact` being the transitive *change
  blast radius* of a symbol — and the same queries are surfaced via
  `blumi knowledge callers|callees|impact|implementers <symbol>`, the gateway
  `POST /api/knowledge/graph` endpoint, the TUI `/knowledge` hot-spots, and the
  blugo Code tab's per-result **Impact** sheet. The default build stays
  native-lite (regex symbols, no C grammars); the tool degrades gracefully over
  the lite graph.
- **Graph-aware code search.** When the structural graph is built, `code_search`
  fills any spare result slots with typed neighbors (callees/callers) of the top
  hits — surfacing related code the keyword/vector pass missed, without displacing
  direct matches.
- **RPL blast radius reads the code graph.** With `knowledge.graph.rpl_impact`
  (default on), editing a heavily-referenced file raises the RPL severity (file
  fan-in folds into the blast radius), so the adversarial Porfiry review is
  likelier to fire on high-impact edits.
- **Pinned session goal.** `/goal` now sets a standing objective stored on the
  session and re-injected as a cache-safe reminder every turn (`Command::SetGoal`),
  so a long autonomous task keeps its objective across context rollovers instead
  of relying on the compaction summary to retain it.
- **Memory conflict resolver (opt-in).** With `memory.resolve_conflicts` on, the
  background memory sweep asks the LLM to classify same-topic memory pairs that
  didn't merge on write and supersedes the outdated side — wiring the conflict
  taxonomy (`conflict_candidates` / `supersede`) to an actuator at last. Bounded
  per tick, conservative (ambiguous → leave both untouched), off by default.

### Changed

- **Auto-wake on context rollover.** A long / autonomous turn no longer stalls at
  the auto-continue token ceiling right after a context compaction: a rollover now
  resets the cumulative token tally (`llm.wake_on_rollover`, default **on**), so
  the task keeps going across rollovers without a manual nudge. The per-turn step
  budget still bounds the turn.

### Fixed

- **Memory fitness guards.** `reward` / `note_used` now update only *active*
  memories, so a row superseded mid-turn can't accrue value/hits from a stale
  recalled-id list (matching eviction/consolidation).

## [0.5.0] — 2026-06-08

### Added

- **Self-improving agent memory — credit assignment, value-based fitness, and a
  structure-aware recall ranker.** The agentic long-term memory now learns from
  *observed outcomes*, not merely from retrieval:
  - **Probationary failure→fix learning** — a guided recovery is stored as a
    *pending hypothesis* and only promoted to a recallable / mineable / diffusable
    fix once the same tool is observed to succeed (cross-step verification, now on
    by default), with provenance. Unverified guesses never masquerade as fixes;
    stale hypotheses are reaped by the sweep.
  - **Value-based fitness** — every memory carries a learned `value` distinct from
    retrieval `utility`: rewarded when it was in context for a productive step,
    decayed on failures, and corroborated across grid nodes (consensus). Eviction
    now drops the lowest-**value** memory, so genuinely-useful memories survive
    instead of merely frequently-retrieved ones.
  - **Structure-aware recall** — recall re-ranks a wider candidate pool by
    memory-graph degree (hub-suppression, so generic "matches-everything" memories
    stop dominating) and is seeded from the last couple of user turns, not one line.
  - **Curation everywhere** — the SEDM consolidation / eviction / graph / evolution
    sweep now runs for standalone `blumi run` and `blumi tui`, not only the
    gateway, so memory is actually curated and the recall graph gets built.
  - **Conflict + diffusion groundwork** — reversible `supersede` + conflict
    detection land as the substrate for resolving contradictions; diffusion now
    shares the highest-**value** memories and raises value on cross-node agreement.
- **RPL-Judgement — an opt-in adversarial, regret-minimizing reasoning loop
  (“Raskolnikov’s Psychological Loop”).** Before a high-blast tool batch touches
  the live system, blumi maps its **blast radius**, submits the plan to an
  adversarial **“Porfiry”** LLM judge that must approve it (on rejection the plan
  is bounced and re-planned, bounded by a defend budget), executes the survivor
  through the normal typed pipeline, then writes the predicted-vs-actual **Error
  Delta** (“regret”) back to memory — where it feeds value-based fitness. Off by
  default (`rpl.enabled`); it trades extra LLM calls for fewer catastrophic
  actions. A standard agent maximizes success; an RPL agent minimizes regret.
- **blugo mascot logo** — the blugo app icon is now the armed-hornet "beekeeper"
  mascot bursting out of the blumi flower bloom (the mascot composited over the
  flower on the rose-dark brand background). The welcome network-diagram **hub**
  node wears this same mascot logo, while the satellite gateway nodes keep the
  plain flower glyph. blugo-only — the blumi CLI/README brand is unchanged.
- **blugo "Bloom" live wallpaper** — a native Android live wallpaper bundled in
  the app (`BloomWallpaperService`): the eight-petal blumi flower blooms
  petal-by-petal into the logo (flower + **blumi** wordmark) on the Living-Rose
  dark background, then the **wasp mascot fades in, centred over the bloom**. It
  replays whenever the wallpaper becomes visible and, on foldables, the moment
  the device is **opened** — detected via the hinge-angle sensor (fold open →
  bloom). Settles to a static scene to spare battery. Pick it under *Wallpaper →
  Live wallpapers → "blumi Bloom"*.

## [0.4.0] — 2026-06-07

### Added

- **blugo Dispatch + FCM push** — a Telegram-style way to chat with each node
  and get pinged when it replies, even with the app backgrounded. On by default
  on the LAN; zero config beyond dropping the Firebase files in place.
  - **Dispatch surface** — a `Dispatch` entry on the welcome diagram (per-node
    action) and a dedicated **inbox** screen (a row per saved gateway + a
    **Broadcast** channel). Each per-node thread is a **dedicated, isolated
    session** (separate from the workbench chat), reusing the full chat UI
    (markdown, tool/approval/plan cards, voice). **Broadcast** fans one message
    to every saved gateway and shows each node's reply card.
  - **FCM (Firebase Cloud Messaging), HTTP v1** — the gateway pushes a reply
    preview to the phone on turn completion (dispatch *and* the main chat), so a
    backgrounded/killed app still gets notified. **Gateway:** a device-token
    registry (`~/.blumi/fcm.json`), `POST /api/push/fcm/register|unregister`, and
    an FCM v1 sender that mints a Google OAuth2 token from a service account
    (RS256 JWT, cached) — enabled automatically when
    `~/.blumi/fcm-service-account.json` is present, a silent no-op otherwise.
    **blugo:** `firebase_messaging` integration that registers its token with
    every gateway and routes a notification tap to the right dispatch thread
    (graceful no-op without Firebase config, falling back to local notifications).
  - **Notification status-bar icon** — a purpose-built white, alpha-only
    **flower-outline** small icon (`ic_stat_blumi`), wired into both the OS-drawn
    FCM path (`default_notification_icon` + a rose `default_notification_color`)
    and the local-notifications plugin. Android masks small icons to their alpha
    channel, so the full-colour launcher icon previously collapsed to a white
    blob; it now shows the blumi flower.
  - **Launch behaviour** — a cold start (after the app is killed) now lands on
    the **network-diagram menu** instead of silently reconnecting to the last
    gateway; a warm resume (background→foreground) keeps the live state and
    returns you to the last screen. Tapping a saved node still reconnects
    instantly via its stored token.
  - **Welcome hub = the blumi flower → Dispatch.** Every node on the
    network-diagram menu now wears the **eight-petal logo flower** (a faithful
    port of `blumi-logo.svg`) instead of the old five-petal glyph — a large,
    prominent centre hub flanked by smaller satellite gateways. The centre hub
    opens **Dispatch** directly (adding a gateway has its own ＋ button; the now
    redundant top-right Dispatch button was removed). The open plays a short
    "**Entering bluuuum mode…**" splash — the same logo bloom spins on the
    current theme background while a brand glow blooms out from behind it and
    washes the screen in the Living-Rose gradient, which then **recedes back to
    the dark background** as the text fades in. Tap to skip; bypassed entirely
    under reduce-motion.
  - **Concurrent dedicated sessions (gateway)** — `/api/chat/send`,
    `/api/messages`, and `/api/chat/stream` accept an optional `session_id` so a
    client can drive a specific session **concurrently** with the active one
    (a small dispatch-session registry); the dispatch SSE is pinned and never
    follows workbench swaps. The `session_id`-less paths are unchanged.

### Changed

- **blugo — full UI/UX redesign.** The phone app is rebuilt on a proper design
  system and gets a brand-new welcome experience.
  - **Welcome = an interactive grid network diagram.** Instead of a list+form,
    the connect screen now draws a radial hub-and-spoke map (native
    CustomPainter): **this device** at the hub, saved gateways orbiting it on
    gradient spokes, and auto-discovered (mDNS) gateways as **dashed/dotted**
    nodes with a ＋ badge. Each node carries a canvas-drawn Living-Rose flower
    glyph; a radar sweep animates while scanning. **Tap a saved node** →
    Connect · Edit · Delete-on-the-side; **tap a discovered node** → a connect
    sheet with the password auto-focused and name/host/port pre-filled from
    discovery; **＋** adds a gateway by IP. New `AppController.editServer`
    supports renaming and re-pointing a saved gateway.
  - **Design-system foundation.** A new `lib/ui/kit/` — `BlumiTokens`
    (`ThemeExtension`: the Living-Rose ramp, status colors, AA-safe muted text,
    radii/spacing, brand gradient), a shared motion language (durations,
    `PressableScale`, staggered entrances, cross-fades, reduced-motion aware),
    and a reusable widget vocabulary (cards, section headers, status dots/pills,
    badges, gradient buttons, empty states, sheets/dialogs). `theme.dart` now
    attaches the tokens and full **component themes** (cards, inputs with a focus
    ring, buttons, chips, tabs, dialogs, sheets, snackbars, …) to all six
    palettes.
  - **Every screen restyled.** Home gets a gradient wordmark header, a
    `ListView.builder` transcript with `RepaintBoundary`s, accent-bordered chat
    bubbles, kit tool/approval/clarify/plan cards, an animated context meter, and
    a themed composer; connect↔home cross-fades. The control center gets a
    gradient header, **leading-icon tabs grouped into Agent · Work · Grid ·
    Knowledge**, uppercase section headers, and de-duplicated grid metrics
    (removed from Settings — the Grid tab owns them). The command palette moves
    onto the shared bottom-sheet with pressable rows; markdown code/inline blocks
    are token-ized (atom-one-dark kept for syntax).
  - Accessibility & perf: `Semantics` on icon nodes, AA-contrast muted text,
    reduced-motion short-circuits, `RepaintBoundary` on animated leaves.
  - Verified on the Pixel 9 Pro Fold; `blugo` build bumped to `1.0.0+3`.

## [0.3.0] — 2026-06-07

### Added

- **TUI `/knowledge` + `/memories` overlays** — the terminal UI reaches parity with
  the web Control Center's knowledge/memory views. **`/knowledge`** shows the code
  knowledge base (indexed files / symbols / vectors + per-source counts);
  **`/memories`** browses semantic long-term memory entries (namespace / kind /
  utility / hit-count, pinned marked ★). Both are read-only, any-key-to-close popups
  (mirroring `/heal`); they degrade to a notice when `knowledge`/`memory` is disabled.
  (The existing `/memory` still views the MEMORY.md / USER.md files.)
- **Messaging gateway as a managed service** — `blumi gateway` now mirrors
  `blumi serve`'s service layer. **`blumi gateway run`** launches every *configured*
  transport (Telegram / Discord / Slack / WhatsApp) concurrently in one process; and
  **`install` / `uninstall` / `start` / `stop` / `status`** register it as a launchd
  (`com.blumi.gateway`) or systemd-user (`blumi-gateway`) service with **auto-start +
  crash/reboot restart** (KeepAlive · Restart=always). `status` reports the service
  state + which transports are configured; logs at `~/.blumi/gateway.log`. The
  existing single-transport commands (`blumi gateway telegram`, …) still work for
  foreground use.
- **Duplicate-bot guard (Telegram)** — the Telegram gateway now detects a **409
  Conflict** (another consumer polling the same token — e.g. a stray bot on a grid
  peer) and logs a **loud warning** to `gateway.log`, then backs off and keeps
  retrying. Previously a 409 parsed as an empty update batch and was *invisible* —
  the cause of silent double-replies when two nodes ran the same token.
- **Telegram voice toggle** — `gateway.telegram.voice` (**off by default**) gates the
  Telegram bot's voice handling: inbound voice-note transcription **and** spoken
  (TTS) replies. With it off, a voice note gets a short "voice is off" reply and the
  bot answers in text only. Set `"gateway": { "telegram": { "voice": true } }` to
  re-enable (still needs global `voice.*` configured). *Note: this flips the prior
  always-on behavior — Telegram voice is now opt-in.*
- **Lifecycle hooks** (Claude-Code-style) — two events, **off by default**:
  - **`user_prompt_submit`** runs shell commands when you submit a prompt; each
    command's stdout is injected as cache-safe background context for the turn (the
    prompt is piped to stdin, cwd = workspace, per-hook timeout).
  - **`pre_tool_use`** runs *ahead of permission policy* before a tool executes: the
    `{tool, input}` payload is piped to stdin and a **non-zero exit blocks** the call
    (stderr/stdout becomes the denial reason), while a spawn error or timeout **fails
    open** (allows) so a broken guardrail can't brick the agent. A `matcher` scopes a
    hook by tool name (substring; empty = all). Hooks are read on session build, so a
    newly added one applies on the next `reload_self`/restart.
- **Completion notifications** (`notify`, **off by default**) — when an autonomous
  run finishes (`blumi loop` or always-on discovery), blumi fans out a short alert
  to the channels you enable. First wave (server-side push — reaches you with no app
  open): an **OS desktop** notification (macOS `osascript` / Linux `notify-send`) and
  a proactive **gateway-bot** message (Telegram / Discord / Slack / WhatsApp) to a
  configured chat/channel, reusing the `gateway.*` credentials. Config:
  `notify { enabled, on:[loop,discovery,turn], desktop, bot:{transport,target},
  web_push }`. `blumi loop --notify` still fires a one-off desktop notification even
  when `notify` is off. Second wave (live-stream surface, needs no config): a
  **browser in-tab alert** — when a turn you started finishes while the web tab is
  **backgrounded**, blumi flashes the title, badges the favicon, plays a short ping,
  and drops a click-to-focus toast (silent while the tab is focused); and a **blugo
  phone notification** (`flutter_local_notifications`) — a heads-up local
  notification when a turn completes while the app is **backgrounded** (Android 13+
  runtime permission requested on first launch). Final wave (`notify.web_push`):
  **browser Web Push** (VAPID + RFC 8291) — a keypair + subscription store under
  `~/.blumi/push.json`, `GET /api/push/key` + `POST /api/push/{subscribe,unsubscribe}`,
  a service-worker `push` handler, and an **Enable** button in the Control Center.
  Pushes fire on `loop` / `discovery` / `turn` completion to every subscribed
  browser. ⚠️ Browser Web Push requires a **secure context** (HTTPS or
  `http://localhost`), so over a plain-HTTP LAN it stays dormant until you add TLS.
- **Web Control Center panels** — the browser/phone Control Center gains four
  tabs over the new backends: **routing** (tiers + `$ saved`), **entries**
  (white-box memory: pin / edit / delete), **discovery** (always-on status +
  reports), and **git** (read-only status / diff / log). One `dist` rebuild.
- **Workspace create/clone wizard (TUI)** — `/new-workspace <path>` creates a
  folder (+ `git init`) and opens it; `/clone-workspace <url> [dir]` git-clones a
  repo and opens it. Both append to the workspace pane (extends `/open-workspace`).
- **Web git panel (read-only)** — `GET /api/git/{status,diff,log}` expose the
  workspace's git status / diff --stat / recent log (behind the gateway password)
  so the browser/phone can review what the agent changed. (Staging/commit + the
  React panel are follow-ups.)
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

[Unreleased]: https://github.com/ankurCES/blumi-cli/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/ankurCES/blumi-cli/releases/tag/v0.4.0
[0.3.0]: https://github.com/ankurCES/blumi-cli/releases/tag/v0.3.0
[0.2.0]: https://github.com/ankurCES/blumi-cli/releases/tag/v0.2.0
[0.1.0]: https://github.com/ankurCES/blumi-cli/commits/main

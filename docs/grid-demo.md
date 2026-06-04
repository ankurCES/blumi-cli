# Grid demo — "Grid Briefing"

A small, genuinely useful product that shows the blumi grid working **end‑to‑end
in real time**: you give a topic, the **fleet researches facets in parallel**,
and the **origin machine assembles** them into one polished briefing.

It exercises the real mechanism:
**parallel `delegate` sub‑agents → the local cap of 4 → overflow to grid peers →
results returned to the origin → assembled into a single artifact.** No
shared‑filesystem assumptions: each peer produces *text*, the origin writes the
final file.

## Prerequisites
- A grid that's up (see [Grid](https://github.com/ankurCES/blumi-cli/wiki/Grid)):
  ≥1 peer online with the **same `grid.secret`**. Verify: blugo → Control Center
  → **Grid** shows the peer online, or `GET /api/grid/metrics`.
- A provider configured on **every** node (each peer runs real turns).

## The test prompt

Paste this into the chat (blugo, the web UI, or `blumi tui`) on the **origin**
machine — the one your phone connects to. Replace `<TOPIC>`:

```
You are the orchestrator on a blumi grid. Produce a polished research briefing on
"<TOPIC>" by working in parallel across the grid.

1. Break the topic into 6 distinct facets (e.g. background, key players, current
   state, risks, opportunities, what's next).
2. For EACH facet, in a SINGLE batch, call the `delegate` tool (agent_type
   "general-purpose") with a focused prompt asking for a tight ~150-word,
   self-contained write-up of that facet (prose only, no tools). Issue all 6
   delegations together so they run concurrently — the local cap is 4, so the
   extras run on grid peers.
3. When the sub-agents return, assemble the write-ups into one clean markdown
   briefing: a title, a 3-bullet executive summary, then one section per facet.
   Write it to ./grid-briefing.md.
4. Call grid_status and tell me, in 2 lines, which facets ran locally vs. on a
   remote peer, plus grid-wide token usage.
```

Good topics: *"the James Webb Space Telescope's biggest discoveries"*,
*"WebAssembly outside the browser"*, *"how RAFT consensus works"*, your product's
competitive landscape, etc.

## Watch it happen in real time
- **TUI:** the right‑pane **active agents** list — overflowed sub‑agents show a
  `⟶ remote` marker; `/grid` shows task distribution.
- **blugo:** Control Center → **Grid** — peers online, per‑node tasks + tokens,
  grid‑wide totals (tap ⟳ to refresh).
- **In chat:** just ask *"grid status"* anytime — the agent calls the
  `grid_status` tool and summarizes peers, health, tasks (local vs remote), and
  token usage.

## What success looks like
- `./grid-briefing.md` is written **on the origin** with all 6 facets assembled.
- `grid_status` / the Grid view shows **>1 node online** and a **non‑zero remote
  task / token count** — i.e. some facets were genuinely produced by a peer and
  assembled back on the origin.

## Variations
- **More fan‑out:** ask for 10 facets to push more work onto peers.
- **Autonomous:** add the facets as board tasks (`blumi task add …`) and run the
  loop in grid mode to round‑robin them across peers (note: the loop distributes
  *execution*; the briefing assembly above is the part that collects + composes
  results on the origin).

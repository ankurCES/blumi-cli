---
name: grid-ops
description: Answer questions about the distributed blumi grid — connected/available peers and their health, task metrics (local vs. handed-off to remote peers, incoming & outgoing), per-node token usage, loop/job status, and grid-wide totals. Use whenever the user asks about peers, fleet capacity, who is online, or where work ran.
metadata:
  last_modified: Wed, 04 Jun 2026 18:30:00 GMT
---

# Querying the blumi grid

When the user asks anything about the grid — "what peers are connected?", "is
mac-2 online?", "how many tasks ran remotely?", "token usage across the fleet?",
"what's the job/loop status?", "how much capacity is available?" — call the
**`grid_status`** tool. It returns a JSON snapshot; read it and answer in plain
language.

## The snapshot shape

```json
{
  "self":  { "uptime_secs", "model", "turns",
             "tokens": {"input","output"},
             "counts": {"todo","doing","review","done","cancelled"},
             "tasks_total", "tasks_remote", "tasks_local",
             "loop": {"running","iter","current"} },
  "peers": [ { "id", "name", "host", "port", "online",
               "metrics": { ...same shape as self, or null if offline } } ],
  "totals": { "nodes_online", "tokens": {"input","output"}, "tasks_total" }
}
```

## How to read it

- **Available / connected peers + health:** `peers[]` — each has `name`, `host`,
  and `online` (true = reachable). `online:false` (or `metrics:null`) = offline.
  `totals.nodes_online` counts self + reachable peers.
- **Task metrics, local vs. remote:**
  - `self.tasks_local` = tasks running on this node; `self.tasks_remote` = tasks
    this node **handed off** to peers (outgoing).
  - A peer's `metrics.tasks_local` = work currently running **on that peer**
    (incoming, from this node's perspective).
  - `self.counts` / each peer's `counts` give the todo/doing/review/done breakdown.
- **Token usage:** `self.tokens` and each peer's `metrics.tokens`
  (input/output); `totals.tokens` is the grid-wide sum across online nodes.
- **Job / loop status:** `loop.running` (is the autonomous loop active),
  `loop.iter` (iteration), `loop.current` (the task title in flight).

## Answering well

- Summarize, don't dump JSON. e.g. "2 nodes online (this one + mac-2). 3 tasks
  doing here, 1 handed to mac-2. ~12k/4k tokens used across the grid. Loop idle."
- If `grid_status` reports the grid isn't available, say the grid is only live
  inside a running `blumi serve` gateway with `grid.enabled = true`.
- For "send/dispatch this to a peer", that's the autonomous loop in grid mode or
  the dispatch endpoint — `grid_status` is read-only (reporting), not dispatch.

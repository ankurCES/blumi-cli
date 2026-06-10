# RPL V2 — Worktree-Sandbox Executor (real Phase 2)

Status: **planned** (#231). This is the implementation plan for the one Tier-3
audit item deliberately deferred — the only RPL phase that does not yet run.

## 1. Goal & current state

RPL-Judgement (`crates/blumi-core/src/rpl.rs`) is a 5-phase, regret-minimizing
pre-execution pass. Today **phases 1, 3, 4, 5 run; phase 2 does not**:

| Phase | Name | Status | Where |
|---|---|---|---|
| 1 | Hypothesis (blast radius) | ✅ live | `agent.rs` RPL hook (`BlastRadius::assess` + impact-oracle fan-in) |
| 2 | **Fever Dream (branch simulation)** | ❌ **dead** | types exist (`ParanoiaScore`, `choose_least_regret`, `RplSandbox::Worktree`, `RplConfig.branches`) but **no runtime reads them** |
| 3 | Anticipating Judgment (Porfiry) | ✅ live | `rpl_porfiry(&tool_calls, &blast, &ct)` — judges the **single real plan** |
| 4 | The Strike (actuate) | ✅ live | normal tool execution after approval |
| 5 | The Confession (error delta) | ✅ live | predicted-vs-actual → `SemanticMemory` |

The audit's finding: the runner jumps straight from blast assessment
(`agent.rs:726-750`) to the judge (`agent.rs:764`). The "branch → score paranoia
→ choose least-regret" loop the module docstring advertises is absent, and
`RplSandbox::Worktree` is a settable config value that no-ops.

**This plan wires phase 2 with a real worktree sandbox**: trial the planned
action(s) in throwaway git worktrees, observe the *real* outcome, score it, and
choose the least-regret branch — then execute the winner on the normal, audited
path.

## 2. Design overview — "trial in sandbox, then execute live"

The key safety decision: **the sandbox is advisory only.** It never mutates the
live workspace. The flow when phase 2 fires:

```
plan (tool_calls) ─▶ generate up to N branches (ToT alternatives)
                       │
                       ▼  for each branch:
                     create throwaway worktree ▶ run the branch's batch there
                       ▼                              ▼
                     observe outcome (diff / build / errors)  ───▶ ParanoiaScore
                       ▼
                     discard worktree
                       │
                       ▼
                  choose_least_regret(scores) ─▶ winning branch
                       │
                       ▼
                  Porfiry judges the winner ─▶ Strike: execute the winner LIVE
                                                (normal audited execution path)
```

The winner re-executes through the existing execution pipeline — the sandbox was
purely an observation to *choose* a branch, so the live mutation still flows
through the normal permission/journal/checkpoint machinery. We never "apply the
worktree diff back" (that would bypass auditing).

## 3. Components

### 3.1 `SandboxFactory` trait (core) + binary impl

The runner holds `executor: Arc<dyn Executor>` (`agent.rs:43`) but only as a
trait object — it cannot construct a *new* `LocalExecutor` rooted at a different
dir (the concrete executors live in `blumi-exec`). So mirror the existing
`ImpactOracle` pattern (core trait, binary impl, injected via a `with_*`
builder):

```rust
// blumi-core/src/rpl.rs  (or a new rpl_sandbox.rs)
#[async_trait]
pub trait SandboxFactory: Send + Sync {
    /// Open a throwaway sandbox seeded from the current workspace state. The
    /// returned handle exposes an Executor rooted in an isolated tree; dropping
    /// it (or calling `discard`) cleans up. `None` if a sandbox can't be made
    /// (non-git workspace, dirty tree that can't be snapshotted, etc.).
    async fn open(&self) -> Option<Box<dyn Sandbox>>;
}

#[async_trait]
pub trait Sandbox: Send + Sync {
    fn executor(&self) -> Arc<dyn Executor>;     // rooted in the worktree
    fn root(&self) -> &Path;
    /// A quick health probe in the sandbox after a trial batch (e.g. the
    /// project's check command). Returns (ok, summary).
    async fn probe(&self) -> (bool, String);
    async fn discard(self: Box<Self>);            // best-effort cleanup
}
```

Binary impl (`crates/blumi/src/engine.rs`, next to `KnowledgeImpactOracle` /
`KnowledgeFitness`): `WorktreeSandboxFactory { repo_root, check_cmd }` whose
`open()` does the git worktree dance (§3.2) and returns a `WorktreeSandbox`
holding a `LocalExecutor` rooted at the worktree. Inject via
`runner.with_sandbox_factory(...)`, gated on `config.rpl.sandbox == Worktree`.

### 3.2 Worktree lifecycle

```
git rev-parse --is-inside-work-tree     # gate: must be a git repo, else None
tmp = <repo>/.git/blumi-sandboxes/<rand>   # inside .git so it's ignored
git worktree add --detach <tmp> HEAD     # isolated checkout at HEAD
# seed uncommitted state (HEAD != working tree):
git stash create                          # snapshot dirty tracked files (no stash stack mutation)
git -C <tmp> stash apply <sha>  (or: copy the diff)   # so the sandbox sees the agent's in-progress edits
... run the trial batch via a LocalExecutor rooted at <tmp> ...
git worktree remove --force <tmp>         # discard (RAII guard ensures this on panic)
```

Wrinkles to handle (each has a fail-open fallback to `Dry`):
- **Dirty working tree.** `git worktree add HEAD` checks out the *commit*, not
  uncommitted edits the agent already made this session. Seed them with
  `git stash create` (produces a commit object without touching the stash stack)
  then apply into the worktree. If seeding fails → fall back to `Dry`.
- **Untracked files** the batch depends on: `git stash create -u` includes them,
  or copy them explicitly.
- **Cleanup on panic:** a `Drop` guard that shells `git worktree remove --force`;
  plus a startup sweep of stale `.git/blumi-sandboxes/*`.
- **Concurrency:** unique random subdir per sandbox; safe for the gateway's
  concurrent sessions.

### 3.3 Branch generation (Tree-of-Thoughts)

Branch 0 is always the **actual plan** (`tool_calls`). Branches 1..N are
LLM-proposed safer alternatives, generated with one structured completion
(reuse `self.llm`, or a sub-agent via `subagent.rs`):

> "This batch has blast radius {blast.declaration()}. Propose up to {N-1}
> alternative approaches that achieve the same intent with a smaller, more
> reversible blast radius. Return each as a concrete tool-call batch."

Clamp to `RplConfig.branches` (1..=5). If generation fails or yields nothing,
degrade to single-branch (just branch 0) — still a real sandbox observation.

### 3.4 Scoring — `ParanoiaScore` from real outcomes

For each branch, after the sandbox trial, build a `ParanoiaScore` from
*observed* signals (not predictions):
- did every tool call succeed in the sandbox? (failures = high regret)
- `git -C <tmp> diff --stat` surface area (lines/files actually changed)
- `sandbox.probe()` — the project check command passed/failed (e.g.
  `cargo check`, configurable; empty = skip)
- realized blast vs predicted blast (the phase-5 Error-Delta, computed early)

`choose_least_regret(&scores)` (already implemented + unit-tested,
`rpl.rs:186`) picks the winner.

### 3.5 Adopt the winner

The winning branch's `tool_calls` replace the batch that proceeds to phase 3/4.
If the winner ≠ branch 0, emit an `Event::Notice` ("RPL chose a safer branch:
…") and continue to the Porfiry judge with the winner. Live execution is the
normal path.

## 4. Integration point

All inside the existing RPL hook in `agent.rs` (currently `:706-794`), between
`should_review(...)` (`:751`) and `rpl_porfiry(...)` (`:764`):

```rust
if blast.should_review(any_mutating, rpl.blast_threshold) {
    // NEW phase 2: only when sandbox == Worktree AND a factory is wired.
    let chosen = if let Some(factory) = &self.sandbox_factory {
        self.rpl_fever_dream(&tool_calls, &blast, rpl.branches, factory, &ct).await
    } else {
        tool_calls.clone()   // Dry: today's behavior, judge the real plan
    };
    let (verdict, risk) = self.rpl_porfiry(&chosen, &blast, &ct).await;
    // … unchanged …  (on approval, `chosen` becomes the executed batch)
}
```

`rpl_fever_dream` is the new method orchestrating §3.3→3.5. Returns the winning
`Vec<ToolCall>` (= the input when sandboxing is unavailable/fails — fail-open).

## 5. Config & gating

Already present in `RplConfig` (`blumi-config/lib.rs`): `branches: 3`,
`sandbox: Dry|Worktree`, `enabled: false`, `blast_threshold: 40`. No new config.
Phase 2 runs **only** when `enabled && sandbox == Worktree && factory wired &&
blast ≥ threshold` — a triply-gated, opt-in path. Default behavior is byte-for-
byte unchanged.

## 6. Safety invariants (non-negotiable)

1. **The live workspace is never mutated by the sandbox.** Worktrees are
   isolated; the winner re-executes through the normal audited pipeline.
2. **Fail-open.** Any sandbox/branch/probe error → fall back to the Dry judge on
   the *real* plan. A sandbox problem must never block or corrupt a turn.
3. **Always clean up.** RAII guard + `git worktree remove --force` + a stale-
   sandbox sweep at startup.
4. **Commands are only filesystem-isolated.** A `Bash` step in a worktree still
   shares network/system state — document this; recommend the Docker executor
   for command-heavy high-blast batches. (Phase B can choose to sandbox
   file-write-only batches and judge-only the command ones.)
5. **Non-git / un-snapshottable → Dry.** No silent partial sandboxing.

## 7. Phasing (each independently shippable + gated)

- **Phase A — sandbox infra.** `SandboxFactory`/`Sandbox` traits (core) +
  `WorktreeSandboxFactory` (binary): create/seed/run/probe/discard a worktree
  with a rooted `LocalExecutor`. Unit + integration tests against a temp git
  repo (no RPL yet). *Deliverable: a tested, standalone worktree sandbox.*
- **Phase B — single-branch verify.** Wire into the RPL hook: sandbox-trial the
  **actual** plan, fold the observed outcome into the Porfiry prompt
  ("sandbox result: applied cleanly / check failed: …"). Makes `RplSandbox::
  Worktree` live with real signal, minimal orchestration. *Highest value/effort.*
- **Phase C — multi-branch ToT.** Branch generation (§3.3), per-branch scoring,
  `choose_least_regret`, adopt the winner. Wires `branches` + the remaining
  dead primitives. *Closes #231 fully.*
- **Phase D — surfacing + docs.** Events for "simulated N branches / chose
  branch k"; TUI `/rpl` + blugo notice; CHANGELOG + this doc → "implemented";
  flip `rpl.sandbox` docs.

A useful stopping point exists after **Phase B** (real sandbox observation) if
multi-branch ToT proves lower-value than expected in practice.

## 8. Open questions / risks

- **Dirty-tree seeding** is the fiddliest part (`git stash create` + apply vs a
  manual tracked-file copy). Prototype both in Phase A.
- **Cost/latency:** N branches × (gen + trial + probe) per high-blast review.
  Bounded by `branches` (≤5) and the opt-in triple gate, but a `cargo check`
  probe on a big repo is seconds. Make `check_cmd` configurable + skippable;
  consider a per-review time budget.
- **Worktree vs Docker:** the worktree isolates the filesystem only. For true
  command isolation the Docker executor is stronger — consider letting
  `sandbox = Worktree` compose with the docker executor later.
- **`git worktree` availability** (old git, submodules, bare repos) — detect and
  fall back to Dry.

## 9. Critical files

- `crates/blumi-core/src/rpl.rs` — `SandboxFactory`/`Sandbox` traits; reuse
  `ParanoiaScore` / `choose_least_regret` (already there).
- `crates/blumi-core/src/agent.rs` — `sandbox_factory` field + `with_sandbox_factory`
  builder; the new `rpl_fever_dream` method; the hook edit at ~`:751`.
- `crates/blumi/src/engine.rs` — `WorktreeSandboxFactory` impl + wiring (next to
  `KnowledgeImpactOracle` / `KnowledgeFitness`), gated on `config.rpl.sandbox`.
- `crates/blumi-exec/src/local.rs` — confirm `LocalExecutor::new(root)` can be
  re-rooted at a worktree path (likely already; else add a constructor).
- `crates/blumi-config/src/lib.rs` — (optional) `RplConfig.sandbox_check_cmd`.

## 10. Acceptance criteria

- With `rpl.enabled = true`, `rpl.sandbox = "worktree"`, a high-blast file-edit
  batch is trialled in a worktree; a batch that breaks `cargo check` in the
  sandbox is steered/rejected **without touching the live tree**; a clean batch
  proceeds and executes live normally.
- Non-git workspace, or any sandbox error, transparently falls back to the Dry
  judge (turn completes; no leftover worktrees).
- `git worktree list` is clean after a turn (no leaked sandboxes), including
  after a simulated panic.
- Default config (`sandbox = "dry"`) behaves byte-for-byte as before.

# Design: Structural Code Graph (typed, resolved) for blumi-knowledge

- **Status:** Proposed
- **Owner:** ankurCES
- **Crates touched:** `blumi-knowledge`, `blumi-config`, `blumi-tools`, `blumi-core` (RPL), `blumi` (engine/CLI), plus surfacing in `blumi-tui` / `blumi-web` / blugo.
- **Target:** an opt-in, feature-gated upgrade of the existing code graph from a name-co-occurrence heuristic to a **typed, scope-resolved structural graph** (calls / imports / inheritance / types), wired into retrieval ranking and the RPL blast radius — without losing the "native-lite, cheap, language-agnostic" default.

---

## 1. Summary

blumi already ships a code graph, but it is **Tier 0**: edges mean "symbol A's snippet text contains a token equal to symbol B's name." This design adds **Tier 1**: a real parse (tree-sitter) → typed, best-effort-resolved edges, behind a Cargo feature and a config mode, with the regex/co-occurrence path kept as the always-available fallback. It then uses the typed graph in two high-value places: **graph-aware retrieval ranking** (expand + re-rank search hits along edges) and the **RPL blast radius** (editing a high-fan-in symbol raises review severity).

The unit of retrieval stays the **symbol (declaration)**; edges connect declarations. We do not become a compiler — resolution is "stack-graphs-lite," explicitly marked `resolved=1|0`. Compiler-grade resolution (LSP/SCIP) is a later **Tier 2**.

---

## 2. Motivation

### Goals
1. Answer **structural** questions precisely: *who calls X? what does X call? what implements trait T? what's the blast radius of editing X?*
2. Make retrieval **structure-aware**: when a query lands on a symbol, surface its callers/callees/types, not just embedding-neighbors.
3. Feed real impact into **RPL** so "blast radius" is literal (caller count), not just a capability heuristic.
4. Preserve blumi's **native-lite ethos**: lean/CI build and existing installs unchanged; structural mode is opt-in like GPU/embeddings.

### Non-goals
- Full type inference / overload resolution / generic instantiation (that's Tier 2 via LSP/SCIP).
- Expression / statement-level nodes or a control-flow / data-flow graph (CPG). We model **declaration-to-declaration** typed edges only.
- Replacing the semantic memory or the LSP code-intel tools — this complements both.

---

## 3. Background: current architecture (Tier 0)

| Piece | Where | What it is |
|---|---|---|
| Symbols | `blumi-knowledge/src/extract.rs` | per-language **regex** decls (Rust/Py/JS-TS/Dart/Go + generic) + a 50-line chunk fallback; `Symbol { name, kind, start_line, end_line, snippet }` |
| Files | `code_files` (`0001_knowledge.sql`) | `path, lang, sha, symbols, indexed_at`; **diff-aware** via `sha` |
| Vectors / FTS | `code_vec`, `code_fts` | per-symbol embedding (cosine) + FTS5(name, snippet) |
| Edges | `code_edges(src,dst)` (`0002_graph.sql`) | built by `build_graph()`: edge iff `src`'s snippet contains a token `== dst.name` (len ≥ 4, not a stop-ident, target not a >8-def "god node"). **Untyped, unresolved.** |
| Queries | `lib.rs` | `search` (FTS→vector hybrid), `retrieve`, `neighbors`, `shortest_path`, `hubs` |
| Tools | `blumi-tools/src/{code_search,code_retrieve,code_graph}.rs` | `code_search`, `code_retrieve`, `code_neighbors`, `code_path` (registered in `blumi/src/engine.rs` when `knowledge.enabled`) |
| Ingest | `KnowledgeStore::ingest_path` | walks (gitignore-aware), `backfill_vectors`, sha-diff skip, writes symbols + vecs |

**Limitations of Tier 0:** edges are untyped and **unresolved** — all same-named symbols collapse into one `by_name` bucket, so `foo()` the fn, `foo` the field, and another module's `foo` are indistinguishable; comments and shadowing create false edges; the `>8` god-node cutoff is crude; only top-level decls participate.

---

## 4. Design overview — a tiered quality ladder

Default stays lean; precision is opt-in and gated exactly like ONNX/CUDA.

| Tier | `graph.mode` | Cargo feature | Edges | Resolution |
|---|---|---|---|---|
| 0 · lite (today) | `lite` (default) | none | name co-occurrence, untyped | none |
| **1 · structural (this doc)** | `structural` | `code-graph` | typed: `call` / `import` / `extends` / `implements` / `type` / `contains` | scope + import aware, best-effort (`resolved` flag) |
| 2 · resolved (future) | `resolved` | `code-graph` + LSP/SCIP | compiler-grade | language server / SCIP index |
| — · off | `off` | — | none | — |

`structural` requested without the `code-graph` feature → warn once, fall back to `lite`.

---

## 5. Data model — migration `0003_graph_typed.sql`

```sql
-- Typed, resolved code edges. An edge means: the body of declaration `src`
-- structurally references declaration `dst` with relation `kind`. `resolved=1`
-- when scope/import resolution found a unique target; `0` when it is a
-- name-heuristic fallback (Tier-0 compatible). `count` = number of reference
-- sites (e.g. call sites) collapsed into this edge.
ALTER TABLE code_edges ADD COLUMN kind     TEXT    NOT NULL DEFAULT 'ref';
ALTER TABLE code_edges ADD COLUMN resolved INTEGER NOT NULL DEFAULT 0;
ALTER TABLE code_edges ADD COLUMN count    INTEGER NOT NULL DEFAULT 1;

-- The (src,dst) PK can't hold multiple relation kinds; rebuild with kind in key.
-- SQLite can't alter a PK in place, so recreate (cheap — the graph is rebuilt
-- from symbols on the next ingest anyway).
CREATE TABLE code_edges_new (
    src      INTEGER NOT NULL REFERENCES code_symbols(id) ON DELETE CASCADE,
    dst      INTEGER NOT NULL REFERENCES code_symbols(id) ON DELETE CASCADE,
    kind     TEXT    NOT NULL DEFAULT 'ref',
    resolved INTEGER NOT NULL DEFAULT 0,
    count    INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (src, dst, kind)
);
INSERT OR IGNORE INTO code_edges_new (src,dst,kind,resolved,count)
    SELECT src,dst,'ref',0,1 FROM code_edges;
DROP TABLE code_edges;
ALTER TABLE code_edges_new RENAME TO code_edges;
CREATE INDEX IF NOT EXISTS idx_code_edges_src  ON code_edges(src, kind);
CREATE INDEX IF NOT EXISTS idx_code_edges_dst  ON code_edges(dst, kind);  -- "who calls X"

-- Symbol enrichment for resolution + display (all nullable → no backfill).
ALTER TABLE code_symbols ADD COLUMN fqname    TEXT;  -- module-qualified name
ALTER TABLE code_symbols ADD COLUMN parent_id INTEGER REFERENCES code_symbols(id) ON DELETE SET NULL;
ALTER TABLE code_symbols ADD COLUMN signature TEXT;  -- one-line decl signature
```

Edge-kind taxonomy: `call`, `import`, `extends`, `implements`, `type` (field/param/return type reference), `contains` (mirrors `parent_id` for scope), `ref` (unresolved fallback / Tier-0).

> **Decision:** model **declaration → declaration** edges with a `count`, not per-call-site rows. Site-level granularity (a `code_refs` table with line numbers) is deferred — it's a 10× row-count increase for marginal agent value; `count` + the snippet are enough to answer "who calls X, roughly how often."

---

## 6. Parser layer (`code-graph` feature)

New module `blumi-knowledge/src/extract_ts.rs`, compiled only under `--features code-graph`. Dependencies (feature-gated in `blumi-knowledge/Cargo.toml`): `tree-sitter` + grammar crates `tree-sitter-rust`, `-python`, `-javascript`, `-typescript`, `-go` (the languages blumi itself + common targets use). Each grammar is a small compiled C blob; gating keeps them out of the default build.

```rust
/// Structural extraction result for one file.
pub struct Parsed {
    pub decls: Vec<Decl>,   // declarations (superset of today's Symbol)
    pub sites: Vec<Site>,   // unresolved reference sites, to be resolved later
    pub imports: Vec<Import>,
}
pub struct Decl { pub name, kind, fqname, parent_path, start_line, end_line, signature, snippet }
pub struct Site { pub from_line, name: String, qualifier: Option<String>, kind: EdgeKind } // call/type/extends/...
pub struct Import { pub local: String, pub module: String, pub original: Option<String> }
```

`extract_structural(path, content, lang) -> Option<Parsed>` returns `None` when no grammar is bundled for `lang` (caller falls back to today's `extract()`). Implemented with tree-sitter **queries** (`.scm`) per language: one query captures declarations (with their span + an enclosing-decl path for `parent_id`/`fqname`), another captures reference sites (call expressions, type identifiers, `use`/`import`, `impl … for`, `extends`/`implements`).

`ingest_path` integration: for each file, if `mode == structural` and a grammar exists → `extract_structural` (store decls as `code_symbols` with `fqname`/`parent_id`/`signature`, stash `sites`/`imports` for the resolve pass); else → today's `extract()` path unchanged.

---

## 7. Resolution algorithm (the core of Tier 1)

Resolution runs as a **second pass** after all symbols in the ingest batch are written (a linker-style two-phase: declare everything, then resolve references). Replaces `build_graph()` body when `mode == structural`; the co-occurrence builder stays for `lite`.

```
build_graph_structural():
  1. Load global tables:
       fqname_index:  fqname           -> symbol_id          (unique)
       name_index:    name             -> [symbol_id]        (fallback, ambiguous)
       file_decls:    file_id          -> [symbol_id] (+ local scopes)
       file_imports:  file_id          -> { local -> (module, original) }
  2. For each reference Site in declaration D (file F):
       a. resolve_target(site, F):
            - local scope of D / enclosing decls in F        (name_index ∩ file_decls[F])
            - F's imports: map site.qualifier/name -> module -> fqname_index
            - global fqname_index by qualified path
            - global name_index by bare name:
                * unique  -> that symbol_id      (resolved = 1)
                * ambiguous, same source+lang, ≤ N -> keep all as resolved=0 edges
                * else    -> unresolved
       b. if target(s): UPSERT edge (D.id -> target, site.kind, resolved, count += 1)
       c. else: optional `ref` edge to best name match (resolved=0) — recall safety net
  3. Also emit `contains` edges from parent_id, and `implements`/`extends` from inheritance sites.
```

**Honesty about precision.** Tree-sitter is *syntactic* — it gives us real call expressions and scopes but **no type inference**. A method call `x.foo()` resolves `foo` among method decls by name (best-effort), not by `x`'s type. So:
- `resolved = 1` only when the target is unambiguous after scope+import resolution.
- `resolved = 0` for name-fallback edges (still better than Tier 0 because they're *typed* and emitted only at real call/type sites, not from comment text).
- We **never** claim completeness. Tools and UIs label edges resolved vs heuristic. Tier 2 (LSP/SCIP) is where `x.foo()` resolves through `x`'s type.

This is strictly a superset of Tier 0 quality: typed + site-anchored + scope-aware, with the same name-fallback as a floor.

---

## 8. Incremental indexing

Ingest is already sha-diff-aware (unchanged files skip). Edges, however, are cross-file (D in F may call a decl in G).

- **v1 (this design):** after an ingest batch, run a **full** `build_graph_structural()`. At native-lite scale (blumi itself ≈ low-tens-of-thousands of symbols) a full resolve is sub-second-ish and dependency-free. Simple and always-correct.
- **v2 (later optimization, noted not built):** on changed file F — delete F's outgoing edges (cascade via symbol delete), re-extract+re-resolve F's sites, and re-resolve any **previously-unresolved** sites whose bare name now matches a new/changed decl. Bounded re-resolution avoids the full sweep on large monorepos.

`log()` the rebuild cost so a future large-repo user sees when to want v2.

---

## 9. Query API + agent tools

New/extended methods on `KnowledgeStore` (return `CodeHit`, the existing type):

```rust
callers(name, kind_filter, limit)   -> Vec<CodeHit>   // edges where dst = name
callees(name, kind_filter, limit)   -> Vec<CodeHit>   // edges where src = name
implementers(trait_name, limit)     -> Vec<CodeHit>   // kind='implements'
impact(name, max_depth, cap)        -> Impact          // transitive callers (BFS, bounded)
neighbors(name, kind_filter, limit) -> Vec<CodeHit>   // extend existing with kind filter
```

**Tool surface** — add **one** unified tool to keep the count low (the registry already has code_search/retrieve/neighbors/path):

```
code_graph { relation: "callers"|"callees"|"impact"|"implementers"|"neighbors"|"path",
             symbol: string, to?: string, kind?: string, limit?: int }
```

`relation:"impact"` is the headline — "what breaks if I change `symbol`": transitive callers, bounded depth, returned as a ranked list + a count. Keep `code_neighbors`/`code_path` as thin aliases for back-compat, or fold them in and update `engine.rs` registration.

---

## 10. Retrieval ranking integration (semantic memory ↔ graph)

Make `search` structure-aware, reusing the **hub-suppression ranker** built for semantic memory (`memory_store.rs::recall`):

```
search_graph(query, k):
  seeds   = search(query, k)                       # today's FTS→vector hybrid
  cands   = seeds ∪ {1-hop callers/callees/types of each seed}
  score(c)= α·semantic_sim(c, query)
          + β·edge_weight(relation to a seed)       # call > type > ref
          + γ·1/(1+ln(1+degree(c)))                 # hub suppression (reuse memory ranker)
  return top-k by score, each tagged with provenance ("called by <seed>")
```

This is the MemGraphRAG "structure-aware retrieval" pattern, and it reuses the exact hub-suppression formula already shipped for memory. Behind `graph.mode != lite` (lite has no typed edges to expand meaningfully).

**Value-fitness cross-link (optional, P8).** The agent already rewards the *memories* it used in productive turns (step-4 work). Add a `value REAL DEFAULT 1.0` to `code_symbols` and, when a recalled code symbol participates in a productive turn, reward it — so frequently-useful symbols rank up and the code graph *learns*, mirroring memory fitness. This is the concrete answer to "semantic memory ranks the graph results."

---

## 11. RPL blast-radius integration (high-leverage reuse)

The RPL `BlastRadius` (`blumi-core/src/rpl.rs`) currently scores severity from declared capabilities. When `graph.rpl_impact` is on and a `FileWrite`/edit targets a file/symbol the graph knows:

```
let fan_in = knowledge.impact(symbol, depth=3, cap=200).len();
severity += min(fan_in, CAP) * W;   // editing a 40-caller fn → higher severity → Porfiry reviews it
```

Wiring: the agent gate (where `BlastRadius::assess` is called) optionally consults the knowledge store for edited symbols and folds `fan_in` into severity (or a new `BlastRadius::with_impact(n)`). Bounded (depth ≤ 3, capped), feature+config gated, and a no-op when the graph is absent — so it can never slow or block a turn when unavailable. This makes "blast radius" literal and is the cleanest synthesis of this design with the RPL work.

---

## 12. Config & feature flags

`KnowledgeConfig` (`blumi-config/src/lib.rs`) gains a sub-struct:

```jsonc
"knowledge": {
  "enabled": true,
  "max_file_kb": 256,
  "exclude": [],
  "graph": {
    "mode": "lite",              // off | lite | structural   (default lite = today)
    "resolve_imports": true,
    "max_edges_per_symbol": 64,  // cap fan-out noise
    "rpl_impact": true           // feed code_impact into the RPL blast radius
  }
}
```

Cargo: `blumi-knowledge` gets a `code-graph` feature (tree-sitter + grammar crates); re-exported by `blumi` as `code-graph` and added to the installer's opt-in set (like `BLUMI_CUDA`). Default build = no feature = `lite` only. `mode:"structural"` without the feature → one-time warn + fall back to `lite`.

---

## 13. Surfacing (parity)

- **CLI** (`blumi knowledge`): extend with `callers`/`callees`/`impact`/`implementers` (today has `neighbors`/`path`/`hubs`).
- **TUI** `/knowledge`: a graph view — neighbors + impact for the selected symbol.
- **blugo** Code tab: a "relations" panel (callers/callees/impact) per symbol result.
- **Gateway**: `POST /api/knowledge/graph` mirroring the tool (behind the password).
- **Docs**: wiki `Memory-and-Knowledge.md` gains a "Code graph" section; `Configuration.md` the `graph` block; `CHANGELOG`.

---

## 14. Testing strategy

- **Extractor (per language).** Fixture files → assert decls (kind/parent/fqname/signature). Start with Rust; add Py/JS/TS/Go in the fan-out phase.
- **Resolution.** A caller→callee fixture asserts a `call` edge with `resolved=1` to the *correct* symbol id; a same-name-in-two-modules fixture asserts disambiguation (no false cross-module edge); an unresolved method call asserts `resolved=0` (not dropped, not falsely resolved).
- **`impact`.** Build a small chain `a→b→c`; assert `impact(c)` ⊇ {a,b} within depth.
- **Fallback.** With the feature **off**, the lite path is byte-for-byte today's behavior (existing tests stay green).
- **RPL.** Unit-test the gate: a high-fan-in edit raises severity past `blast_threshold` (extends the RPL test suite).
- **Gates.** `cargo fmt`; `cargo clippy -p blumi-knowledge -p blumi-config -p blumi-tools -p blumi-core -p blumi --all-targets -- -D warnings` **with and without** `--features code-graph`; `cargo test` for the same set; one `cargo build --features code-graph` to confirm grammars link.

---

## 15. Phased delivery plan

| Phase | Scope | Acceptance |
|---|---|---|
| **P0** | Schema `0003`, `KnowledgeConfig.graph`, `code-graph` feature scaffold, `mode` plumbing | builds with/without feature; `lite` == today; existing tests green |
| **P1** | `extract_ts.rs` for **Rust** (decls + sites + imports) behind the feature | Rust fixture → symbols with kind/parent/fqname/signature |
| **P2** | `build_graph_structural()` two-pass resolver; typed `call`/`type`/`implements`/`contains` edges (Rust) | fixture asserts resolved caller→callee; disambiguation test; lite unchanged |
| **P3** | `callers`/`callees`/`impact`/`implementers` + unified `code_graph` tool + CLI subcmds | `code_impact` works on the dogfood index (blumi itself) |
| **P4** | RPL blast-radius hook (`graph.rpl_impact`) | gate unit test: high-fan-in edit → higher severity |
| **P5** | graph-aware `search` ranking (expand + hub-suppressed re-rank, provenance) | A/B on dogfood queries surfaces callers/callees |
| **P6** | fan out languages: Python, JS/TS, Go | per-language extractor + resolution fixtures |
| **P7** | surfacing: TUI `/knowledge`, blugo Code tab, gateway endpoint | parity demo |
| **P8** | value-fitness link (`code_symbols.value`) + docs/CHANGELOG/wiki | symbols used in productive turns rank up; docs shipped |

**Recommended first slice:** **P0 → P1 → P2 → P3 → P4 for Rust only**, dogfooded by indexing blumi itself. That proves the entire vertical — parse → resolve → typed edges → tool → agent value (impact + RPL) — on one language before fanning out. Ship it behind the feature so `main` stays releasable throughout.

---

## 16. Alternatives considered

| Option | Verdict |
|---|---|
| **Stay regex, add call-site regexes** | Cheap, no deps, but still unresolved + brittle; doesn't deliver typed/resolved edges. Rejected as the primary path (kept as the `lite` fallback). |
| **tree-sitter + lite resolver (chosen)** | In-process, deterministic, incremental, no running servers; gives typed + scope-resolved edges. Best balance for an offline, language-broad agent. Accepts "not compiler-grade." |
| **LSP harvest** (reuse `#78` client) | Compiler-grade refs, but needs a **running language server per language at index time** — heavy, fragile for batch/headless indexing, and not all languages configured. Better as **Tier 2 enrichment** on top, not the base. |
| **SCIP / stack-graphs index** | Excellent precision; heavier to produce and language-specific tooling. Good Tier-2 import path later. |

---

## 17. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Build weight / grammar C deps | Feature-gated (`code-graph`), default off; CI + lean installs stay regex. |
| Resolution false positives | `resolved` flag; tools label heuristic edges; `resolved=1` only when unambiguous. |
| Cross-file / incremental staleness | Full rebuild v1 (cheap at scale); bounded incremental v2 noted. |
| Tool sprawl | One unified `code_graph` tool, not five. |
| RPL latency from impact query | depth ≤ 3, capped, feature+config gated, no-op when absent. |
| Language coverage gaps | tree-sitter top langs, regex `lite` fallback elsewhere (existing pattern). |

---

## 18. Open questions

1. Bundle which grammars by default under `code-graph` — just blumi's stack (Rust/Py/JS/TS/Go), or a wider set? (Lean: blumi's stack first.)
2. Add `code_symbols.value` (the fitness link, P8) now or after P5 proves ranking value?
3. Keep `code_neighbors`/`code_path` as separate tools or fold entirely into `code_graph`? (Lean: fold, alias for one release.)
4. Should `impact` power a `/undo`-style "show me what this edit could break" preview in the TUI before a write?

---

## Appendix A — worked example (Rust)

```rust
// file: src/auth.rs
pub fn verify(token: &str) -> bool { check_sig(token) }   // call site: check_sig
fn check_sig(t: &str) -> bool { /* … */ true }
```

Tier 0 edge: `verify —ref→ check_sig` (because the snippet text contains "check_sig"), plus noise from any comment mentioning either name.

Tier 1 edges: `verify —call(resolved=1)→ check_sig` (resolved via same-file scope), `contains`: (enclosing module) → verify, check_sig. `impact("check_sig")` ⇒ `{verify, …transitive callers}`. Editing `check_sig` → RPL sees fan-in ≥ 1 → severity bump.

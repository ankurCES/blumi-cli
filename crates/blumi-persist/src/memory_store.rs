//! Semantic long-term memory store (the [`blumi_core::SemanticMemory`] impl).
//!
//! Backed by the same SQLite DB as sessions/checkpoints (`memories` +
//! `memory_vectors` + `memories_fts`, migration 0004). Vector search is a
//! pure-Rust brute-force cosine over normalized f32 BLOBs — justified at this
//! scale (a 384-dim dot product over thousands of rows is sub-millisecond,
//! ≪ LLM latency) and dependency-free; an ANN index can slot in behind this
//! same API later. When no embeddings client is available (or it errors), every
//! path degrades to FTS5 keyword search.
//!
//! On top of recall/write it implements the SEDM governance the grid needs:
//! - **write admission** — a near-duplicate write merges (bumps utility) instead
//!   of inserting (`add` → [`best_match`](SemanticMemoryImpl::best_match)).
//! - **utility scoring** — every recall/merge bumps `hits` + `utility`.
//! - **consolidation / eviction** — [`consolidate`] folds dup clusters into the
//!   highest-utility member; [`evict`] soft-deletes the weakest past a cap.
//! - **diffusion** — [`high_utility`] exports locally-authored, non-`user`
//!   memories for the gateway to push to peers; [`ingest_remote`] re-admits
//!   received ones (origin-tagged so they never re-diffuse).

use crate::{to_fts_query, Store};
use async_trait::async_trait;
use blumi_core::{EmbeddingClient, RecalledMemory, SemanticMemory};
use sqlx::Row;
use std::collections::HashSet;
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Tuning knobs for the store (mirrors `blumi_config::MemoryConfig`, kept
/// config-dep-free here).
#[derive(Debug, Clone)]
pub struct MemoryParams {
    /// Cosine ≥ this on write merges into the existing memory (dedup admission).
    pub dedup_threshold: f32,
    /// Minimum cosine for a memory to be injected as background RAG context.
    pub recall_floor: f32,
    /// Max active memories per namespace before eviction (0 = unbounded).
    pub max_per_namespace: u32,
}

impl Default for MemoryParams {
    fn default() -> Self {
        MemoryParams {
            dedup_threshold: 0.92,
            recall_floor: 0.35,
            max_per_namespace: 2000,
        }
    }
}

/// Semantic memory over the shared [`Store`], with an optional embeddings
/// backend (absent → FTS5 fallback everywhere).
/// A full memory entry for the white-box editor (list/view/edit/pin/delete).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub namespace: String,
    pub kind: String,
    pub text: String,
    /// Authoring node id (`""` = local).
    pub origin: String,
    pub source_session: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub hits: i64,
    pub last_used_at: Option<String>,
    pub utility: f64,
    /// Learned fitness (outcome-driven), distinct from engagement `utility`.
    pub value: f64,
    pub status: String,
    pub pinned: bool,
}

fn row_to_entry(r: &sqlx::sqlite::SqliteRow) -> MemoryEntry {
    MemoryEntry {
        id: r.get("id"),
        namespace: r.get("namespace"),
        kind: r.get("kind"),
        text: r.get("text"),
        origin: r.get("origin"),
        source_session: r.get("source_session"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        hits: r.get("hits"),
        last_used_at: r.get("last_used_at"),
        utility: r.get("utility"),
        value: r.get("value"),
        status: r.get("status"),
        pinned: r.get::<i64, _>("pinned") != 0,
    }
}

pub struct SemanticMemoryImpl {
    store: Arc<Store>,
    embedder: Option<Arc<dyn EmbeddingClient>>,
    params: MemoryParams,
}

impl SemanticMemoryImpl {
    pub fn new(
        store: Arc<Store>,
        embedder: Option<Arc<dyn EmbeddingClient>>,
        params: MemoryParams,
    ) -> Self {
        SemanticMemoryImpl {
            store,
            embedder,
            params,
        }
    }

    fn pool(&self) -> &sqlx::SqlitePool {
        self.store.pool()
    }

    /// Embed + L2-normalize one text. Returns `None` when embeddings are off,
    /// the model is still doing its one-time cold load (we NEVER block the hot
    /// path on that — the background warmup loads it and [`backfill_vectors`]
    /// fills in any memories written meanwhile), or it errors. Callers then fall
    /// back to FTS5 / a vector-less insert.
    /// Embed arbitrary texts with this node's embedder — used by the gateway to
    /// SERVE grid-embed offload requests from CPU peers (`/api/grid/embed`).
    /// `None` when no embedder is configured/ready. Vectors are NOT normalized
    /// here (the caller stores them through the same admission path the local
    /// embedder uses, which normalizes on write).
    pub async fn embed_texts(&self, texts: &[String]) -> Option<Vec<Vec<f32>>> {
        let emb = self.embedder.as_ref()?;
        if !emb.ready() {
            return None;
        }
        emb.embed(texts).await.ok()
    }

    async fn embed_one(&self, text: &str) -> Option<Vec<f32>> {
        let emb = self.embedder.as_ref()?;
        if !emb.ready() {
            return None;
        }
        let mut v = emb
            .embed(&[text.to_string()])
            .await
            .ok()?
            .into_iter()
            .next()?;
        normalize(&mut v);
        Some(v)
    }

    /// Write-admission gate: merge a near-duplicate (≥ dedup_threshold) instead
    /// of inserting. Returns the stored or merged-into id.
    pub async fn add(
        &self,
        namespace: &str,
        kind: &str,
        text: &str,
        source_session: Option<&str>,
        origin: &str,
    ) -> Option<i64> {
        self.add_inner(namespace, kind, text, source_session, origin)
            .await
            .map(|(id, _merged)| id)
    }

    /// Like [`add`](Self::add), but also reports whether the write *merged* into
    /// an existing near-duplicate (`true`) rather than inserting a new row —
    /// used by [`ingest_remote`](Self::ingest_remote) to detect cross-node
    /// corroboration (consensus).
    async fn add_inner(
        &self,
        namespace: &str,
        kind: &str,
        text: &str,
        source_session: Option<&str>,
        origin: &str,
    ) -> Option<(i64, bool)> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(qvec) = self.embed_one(text).await {
            if let Some((id, sim)) = self.best_match(namespace, &qvec).await {
                if sim >= self.params.dedup_threshold {
                    self.merge(id).await;
                    return Some((id, true));
                }
            }
            let id = self
                .insert(namespace, kind, text, source_session, origin, "active")
                .await?;
            self.insert_vector(id, &qvec).await;
            Some((id, false))
        } else {
            // No embeddings: dedup on exact text, else insert (no vector).
            if let Some(id) = self.find_exact(namespace, text).await {
                self.merge(id).await;
                return Some((id, true));
            }
            let id = self
                .insert(namespace, kind, text, source_session, origin, "active")
                .await?;
            Some((id, false))
        }
    }

    /// Re-admit a memory diffused from a peer (origin = sender node id, so it is
    /// never re-diffused). Goes through the same dedup gate.
    pub async fn ingest_remote(
        &self,
        namespace: &str,
        kind: &str,
        text: &str,
        origin: &str,
    ) -> bool {
        match self.add_inner(namespace, kind, text, None, origin).await {
            Some((id, merged)) => {
                // Cross-node consensus: a peer independently holds this memory,
                // so corroboration raises its learned value (a fix confirmed on
                // N nodes outranks one seen once) — F10.
                if merged {
                    self.reward(&[id], 0.3).await;
                }
                true
            }
            None => false,
        }
    }

    /// Persist a memory as a *pending hypothesis*: embedded and stored but
    /// excluded from recall/mining/diffusion/consolidation/eviction (all of which
    /// filter `status='active'`) until [`promote`](Self::promote)d once its
    /// outcome is observed. No dedup — each attempt is its own hypothesis; stale
    /// ones are reaped by [`prune_pending`](Self::prune_pending). Used for guided
    /// recoveries whose fix is not yet proven to work.
    pub async fn add_pending(
        &self,
        namespace: &str,
        kind: &str,
        text: &str,
        source_session: Option<&str>,
        origin: &str,
    ) -> Option<i64> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let id = self
            .insert(namespace, kind, text, source_session, origin, "pending")
            .await?;
        // Embed now so promotion makes it instantly recallable (active_vectors
        // joins on status='active', so the vector simply isn't surfaced yet).
        if let Some(qvec) = self.embed_one(text).await {
            self.insert_vector(id, &qvec).await;
        }
        Some(id)
    }

    /// Promote a pending memory to active (recallable) once its outcome is
    /// observed to be good: flips status, reinforces utility, and records
    /// `provenance` (the evidence it worked) into `source_session` if unset.
    /// Idempotent — only acts on a still-`pending` row.
    pub async fn promote(&self, id: i64, provenance: Option<&str>) {
        let now = now();
        let _ = sqlx::query(
            "UPDATE memories
                SET status = 'active', utility = utility + 0.5, hits = hits + 1,
                    last_used_at = ?, updated_at = ?,
                    source_session = COALESCE(NULLIF(source_session, ''), ?)
              WHERE id = ? AND status = 'pending'",
        )
        .bind(&now)
        .bind(&now)
        .bind(provenance)
        .bind(id)
        .execute(self.pool())
        .await;
    }

    /// Reap pending hypotheses never promoted within `max_age_secs` (their fix was
    /// never observed to work), so unconfirmed episodes don't accumulate. Vectors
    /// cascade via the FK. Returns the number pruned.
    pub async fn prune_pending(&self, max_age_secs: i64) -> usize {
        let cutoff = (OffsetDateTime::now_utc() - time::Duration::seconds(max_age_secs))
            .format(&Rfc3339)
            .unwrap_or_default();
        sqlx::query("DELETE FROM memories WHERE status = 'pending' AND created_at < ?")
            .bind(cutoff)
            .execute(self.pool())
            .await
            .map(|r| r.rows_affected() as usize)
            .unwrap_or(0)
    }

    async fn insert(
        &self,
        namespace: &str,
        kind: &str,
        text: &str,
        source_session: Option<&str>,
        origin: &str,
        status: &str,
    ) -> Option<i64> {
        let now = now();
        let res = sqlx::query(
            "INSERT INTO memories
                (namespace, kind, text, origin, source_session, created_at, updated_at,
                 hits, last_used_at, utility, status)
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, NULL, 1.0, ?)",
        )
        .bind(namespace)
        .bind(kind)
        .bind(text)
        .bind(origin)
        .bind(source_session)
        .bind(&now)
        .bind(&now)
        .bind(status)
        .execute(self.pool())
        .await
        .ok()?;
        Some(res.last_insert_rowid())
    }

    async fn insert_vector(&self, id: i64, v: &[f32]) {
        let model = self
            .embedder
            .as_ref()
            .map(|e| e.model_id().to_string())
            .unwrap_or_default();
        let _ = sqlx::query(
            "INSERT OR REPLACE INTO memory_vectors (memory_id, model, dim, vec) VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(model)
        .bind(v.len() as i64)
        .bind(vec_to_blob(v))
        .execute(self.pool())
        .await;
    }

    async fn find_exact(&self, namespace: &str, text: &str) -> Option<i64> {
        sqlx::query_scalar::<_, i64>(
            "SELECT id FROM memories WHERE namespace = ? AND text = ? AND status = 'active' LIMIT 1",
        )
        .bind(namespace)
        .bind(text)
        .fetch_optional(self.pool())
        .await
        .ok()
        .flatten()
    }

    /// Best cosine match (and its score) for `qvec` among active memories in `ns`.
    async fn best_match(&self, namespace: &str, qvec: &[f32]) -> Option<(i64, f32)> {
        self.active_vectors(Some(namespace))
            .await
            .into_iter()
            .map(|(id, _ns, _text, v)| (id, dot(qvec, &v)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Load `(id, namespace, text, vec)` for active memories (optionally one ns).
    async fn active_vectors(
        &self,
        namespace: Option<&str>,
    ) -> Vec<(i64, String, String, Vec<f32>)> {
        let rows = match namespace {
            Some(ns) => {
                sqlx::query(
                    "SELECT m.id AS id, m.namespace AS ns, m.text AS text, v.vec AS vec
                     FROM memories m JOIN memory_vectors v ON v.memory_id = m.id
                     WHERE m.status = 'active' AND m.namespace = ?",
                )
                .bind(ns)
                .fetch_all(self.pool())
                .await
            }
            None => {
                sqlx::query(
                    "SELECT m.id AS id, m.namespace AS ns, m.text AS text, v.vec AS vec
                     FROM memories m JOIN memory_vectors v ON v.memory_id = m.id
                     WHERE m.status = 'active'",
                )
                .fetch_all(self.pool())
                .await
            }
        };
        rows.unwrap_or_default()
            .iter()
            .map(|r| {
                let blob: Vec<u8> = r.get("vec");
                (
                    r.get::<i64, _>("id"),
                    r.get::<String, _>("ns"),
                    r.get::<String, _>("text"),
                    blob_to_vec(&blob),
                )
            })
            .collect()
    }

    async fn merge(&self, id: i64) {
        let _ = sqlx::query(
            "UPDATE memories SET hits = hits + 1, utility = utility + 0.5, updated_at = ? WHERE id = ?",
        )
        .bind(now())
        .bind(id)
        .execute(self.pool())
        .await;
    }

    /// Adjust the learned *value* (fitness) of memories by `delta` (may be
    /// negative), clamped at zero. ids are i64 from our own DB → safe to inline.
    /// This is the outcome-driven signal eviction ranks by — distinct from
    /// `utility`, which only measures retrieval engagement.
    pub async fn reward(&self, ids: &[i64], delta: f64) {
        if ids.is_empty() || delta == 0.0 {
            return;
        }
        let inlist = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE memories SET value = MAX(0.0, value + ?), updated_at = ?
             WHERE id IN ({inlist})"
        );
        let _ = sqlx::query(&sql)
            .bind(delta)
            .bind(now())
            .execute(self.pool())
            .await;
    }

    /// Resolve a conflict by superseding `loser` with `winner`: the loser is
    /// marked `superseded` (excluded from recall/mining/diffusion like any
    /// non-active row, but kept for audit/restore — reversible, never deleted),
    /// with the winner recorded as provenance. The conflict-resolution actuator
    /// for the mutually-exclusive case (F6).
    pub async fn supersede(&self, loser: i64, winner: i64) {
        let _ = sqlx::query(
            "UPDATE memories
                SET status = 'superseded', updated_at = ?,
                    source_session = COALESCE(NULLIF(source_session, ''), ?)
              WHERE id = ? AND status = 'active'",
        )
        .bind(now())
        .bind(format!("superseded_by:{winner}"))
        .bind(loser)
        .execute(self.pool())
        .await;
    }

    /// Candidate *conflict* pairs in `namespace`: active memories whose cosine
    /// falls in the band `[lo, hi)` — same topic, but not near-duplicates (those
    /// already merge on write). The LLM conflict resolver classifies these
    /// (mutually-exclusive / temporal / granularity) and acts via [`supersede`]
    /// / [`update_memory_text`]. Bounded by `limit` to cap resolver cost.
    pub async fn conflict_candidates(
        &self,
        namespace: &str,
        lo: f32,
        hi: f32,
        limit: usize,
    ) -> Vec<(i64, String, i64, String)> {
        let vecs = self.active_vectors(Some(namespace)).await;
        let mut out = Vec::new();
        for i in 0..vecs.len() {
            for j in (i + 1)..vecs.len() {
                let s = dot(&vecs[i].3, &vecs[j].3);
                if s >= lo && s < hi {
                    out.push((vecs[i].0, vecs[i].2.clone(), vecs[j].0, vecs[j].2.clone()));
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        out
    }

    /// Core search shared by recall (floored) and explicit query (floor 0).
    async fn search_inner(
        &self,
        namespace: Option<&str>,
        query: &str,
        k: usize,
        floor: f32,
    ) -> Vec<RecalledMemory> {
        if k == 0 || query.trim().is_empty() {
            return vec![];
        }
        match self.embed_one(query).await {
            Some(qvec) => {
                let mut scored: Vec<RecalledMemory> = self
                    .active_vectors(namespace)
                    .await
                    .into_iter()
                    .map(|(id, ns, text, v)| RecalledMemory {
                        id,
                        namespace: ns,
                        text,
                        score: dot(&qvec, &v),
                    })
                    .filter(|r| r.score >= floor)
                    .collect();
                scored.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                scored.truncate(k);
                scored
            }
            // No embeddings (or transient error) → keyword fallback.
            None => self.search_fts(namespace, query, k).await,
        }
    }

    async fn search_fts(
        &self,
        namespace: Option<&str>,
        query: &str,
        k: usize,
    ) -> Vec<RecalledMemory> {
        let fts = to_fts_query(query);
        if fts.is_empty() {
            return vec![];
        }
        let rows = match namespace {
            Some(ns) => {
                sqlx::query(
                    "SELECT m.id AS id, m.namespace AS ns, m.text AS text
                     FROM memories_fts f JOIN memories m ON m.id = f.rowid
                     WHERE f.text MATCH ? AND m.status = 'active' AND m.namespace = ?
                     ORDER BY rank LIMIT ?",
                )
                .bind(&fts)
                .bind(ns)
                .bind(k as i64)
                .fetch_all(self.pool())
                .await
            }
            None => {
                sqlx::query(
                    "SELECT m.id AS id, m.namespace AS ns, m.text AS text
                     FROM memories_fts f JOIN memories m ON m.id = f.rowid
                     WHERE f.text MATCH ? AND m.status = 'active'
                     ORDER BY rank LIMIT ?",
                )
                .bind(&fts)
                .bind(k as i64)
                .fetch_all(self.pool())
                .await
            }
        };
        rows.unwrap_or_default()
            .iter()
            .map(|r| RecalledMemory {
                id: r.get("id"),
                namespace: r.get("ns"),
                text: r.get("text"),
                score: 1.0,
            })
            .collect()
    }

    // --- SEDM governance --------------------------------------------------

    /// Distinct active namespaces (for sweeping).
    pub async fn namespaces(&self) -> Vec<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT namespace FROM memories WHERE status = 'active'",
        )
        .fetch_all(self.pool())
        .await
        .unwrap_or_default()
    }

    /// Count active memories in a namespace.
    pub async fn count(&self, namespace: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM memories WHERE namespace = ? AND status = 'active'",
        )
        .bind(namespace)
        .fetch_one(self.pool())
        .await
        .unwrap_or(0)
    }

    // --- White-box editor (list / view / pin / delete / edit) ---

    /// List memory entries for the editor. `namespace`/`status` = `None` ⇒ all;
    /// pinned-first, then highest-utility, then most-recently-updated.
    pub async fn list_memories(
        &self,
        namespace: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> Vec<MemoryEntry> {
        let rows = sqlx::query(
            "SELECT id, namespace, kind, text, origin, source_session, created_at,
                    updated_at, hits, last_used_at, utility, value, status, pinned
             FROM memories
             WHERE (?1 IS NULL OR namespace = ?1) AND (?2 IS NULL OR status = ?2)
             ORDER BY pinned DESC, utility DESC, updated_at DESC
             LIMIT ?3",
        )
        .bind(namespace)
        .bind(status)
        .bind(limit.clamp(1, 2000))
        .fetch_all(self.pool())
        .await
        .unwrap_or_default();
        rows.iter().map(row_to_entry).collect()
    }

    /// Fetch a single entry by id.
    pub async fn get_memory(&self, id: i64) -> Option<MemoryEntry> {
        let row = sqlx::query(
            "SELECT id, namespace, kind, text, origin, source_session, created_at,
                    updated_at, hits, last_used_at, utility, value, status, pinned
             FROM memories WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .ok()??;
        Some(row_to_entry(&row))
    }

    /// Pin/unpin an entry. Pinned entries are exempt from eviction + consolidation
    /// (see `evict`/`consolidate`). Returns whether a row was updated.
    pub async fn set_pinned(&self, id: i64, pinned: bool) -> bool {
        sqlx::query("UPDATE memories SET pinned = ?, updated_at = ? WHERE id = ?")
            .bind(pinned as i64)
            .bind(now())
            .bind(id)
            .execute(self.pool())
            .await
            .map(|r| r.rows_affected() > 0)
            .unwrap_or(false)
    }

    /// Hard-delete an entry: its vector cascades (FK `ON DELETE CASCADE`), the
    /// `memories_ad` trigger removes it from FTS5, and we clean up graph edges.
    pub async fn delete_memory(&self, id: i64) -> bool {
        let _ = sqlx::query("DELETE FROM memory_edges WHERE src = ? OR dst = ?")
            .bind(id)
            .bind(id)
            .execute(self.pool())
            .await;
        sqlx::query("DELETE FROM memories WHERE id = ?")
            .bind(id)
            .execute(self.pool())
            .await
            .map(|r| r.rows_affected() > 0)
            .unwrap_or(false)
    }

    /// Replace an entry's text: keep the external-content FTS5 index in sync (no
    /// update trigger exists, so mirror delete+insert) and re-embed (best-effort;
    /// a cold embedder defers to `backfill_vectors`). Returns whether it updated.
    pub async fn update_memory_text(&self, id: i64, new_text: &str) -> bool {
        let new_text = new_text.trim();
        if new_text.is_empty() {
            return false;
        }
        let Some(old) = sqlx::query("SELECT text FROM memories WHERE id = ?")
            .bind(id)
            .fetch_optional(self.pool())
            .await
            .ok()
            .flatten()
        else {
            return false;
        };
        let old_text: String = old.get("text");
        let updated = sqlx::query("UPDATE memories SET text = ?, updated_at = ? WHERE id = ?")
            .bind(new_text)
            .bind(now())
            .bind(id)
            .execute(self.pool())
            .await
            .map(|r| r.rows_affected() > 0)
            .unwrap_or(false);
        if !updated {
            return false;
        }
        // Mirror the insert/delete triggers for the changed row.
        let _ = sqlx::query(
            "INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', ?, ?)",
        )
        .bind(id)
        .bind(&old_text)
        .execute(self.pool())
        .await;
        let _ = sqlx::query("INSERT INTO memories_fts(rowid, text) VALUES (?, ?)")
            .bind(id)
            .bind(new_text)
            .execute(self.pool())
            .await;
        if let Some(v) = self.embed_one(new_text).await {
            self.insert_vector(id, &v).await;
        }
        true
    }

    /// Fold near-duplicate clusters in `namespace` into the highest-utility
    /// member (losers → `status='merged'`, utility/hits folded into the keeper).
    /// Returns how many were merged. No-op without vectors. Pinned rows are exempt.
    pub async fn consolidate(&self, namespace: &str) -> usize {
        let rows = sqlx::query(
            "SELECT m.id AS id, m.utility AS utility, m.hits AS hits, v.vec AS vec
             FROM memories m JOIN memory_vectors v ON v.memory_id = m.id
             WHERE m.status = 'active' AND m.namespace = ? AND m.pinned = 0
             ORDER BY m.utility DESC, m.id ASC",
        )
        .bind(namespace)
        .fetch_all(self.pool())
        .await
        .unwrap_or_default();
        let items: Vec<(i64, f64, i64, Vec<f32>)> = rows
            .iter()
            .map(|r| {
                let blob: Vec<u8> = r.get("vec");
                (
                    r.get::<i64, _>("id"),
                    r.get::<f64, _>("utility"),
                    r.get::<i64, _>("hits"),
                    blob_to_vec(&blob),
                )
            })
            .collect();

        let mut removed: HashSet<i64> = HashSet::new();
        let mut merged = 0usize;
        for i in 0..items.len() {
            if removed.contains(&items[i].0) {
                continue;
            }
            for j in (i + 1)..items.len() {
                if removed.contains(&items[j].0) {
                    continue;
                }
                if dot(&items[i].3, &items[j].3) >= self.params.dedup_threshold {
                    self.mark_merged(items[j].0, items[i].0, items[j].1, items[j].2)
                        .await;
                    removed.insert(items[j].0);
                    merged += 1;
                }
            }
        }
        merged
    }

    async fn mark_merged(&self, loser: i64, keeper: i64, loser_utility: f64, loser_hits: i64) {
        let now = now();
        let _ = sqlx::query("UPDATE memories SET status = 'merged', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(loser)
            .execute(self.pool())
            .await;
        let _ = sqlx::query(
            "UPDATE memories SET hits = hits + ?, utility = utility + ?, updated_at = ? WHERE id = ?",
        )
        .bind(loser_hits)
        .bind(loser_utility * 0.5)
        .bind(&now)
        .bind(keeper)
        .execute(self.pool())
        .await;
    }

    /// Soft-evict the lowest-utility active memories past `cap` in `namespace`.
    /// Returns how many were evicted. `cap = 0` disables eviction.
    pub async fn evict(&self, namespace: &str, cap: u32) -> usize {
        if cap == 0 {
            return 0;
        }
        let count = self.count(namespace).await;
        let over = count - cap as i64;
        if over <= 0 {
            return 0;
        }
        sqlx::query(
            "UPDATE memories SET status = 'evicted'
             WHERE id IN (
                 SELECT id FROM memories
                 WHERE namespace = ? AND status = 'active' AND pinned = 0
                 ORDER BY value ASC, utility ASC, updated_at ASC LIMIT ?
             )",
        )
        .bind(namespace)
        .bind(over)
        .execute(self.pool())
        .await
        .map(|r| r.rows_affected() as usize)
        .unwrap_or(0)
    }

    /// Backfill vectors for active memories that lack one — e.g. written during
    /// the model's cold start (when [`embed_one`](Self::embed_one) returns
    /// `None`). No-op until the model is ready; bounded per call. Returns how
    /// many were embedded.
    pub async fn backfill_vectors(&self, limit: i64) -> usize {
        if self.embedder.as_ref().map(|e| e.ready()) != Some(true) {
            return 0;
        }
        let rows = sqlx::query(
            "SELECT m.id AS id, m.text AS text
             FROM memories m LEFT JOIN memory_vectors v ON v.memory_id = m.id
             WHERE m.status = 'active' AND v.memory_id IS NULL
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .unwrap_or_default();
        let mut n = 0;
        for r in &rows {
            let id: i64 = r.get("id");
            let text: String = r.get("text");
            if let Some(v) = self.embed_one(&text).await {
                self.insert_vector(id, &v).await;
                n += 1;
            }
        }
        n
    }

    /// One governance sweep over every namespace: backfill missing vectors, then
    /// consolidate near-dupes, then evict the weakest. Returns `(merged, evicted)`.
    pub async fn sweep(&self) -> (usize, usize) {
        self.backfill_vectors(64).await;
        let cap = self.params.max_per_namespace;
        let mut merged = 0;
        let mut evicted = 0;
        for ns in self.namespaces().await {
            merged += self.consolidate(&ns).await;
            evicted += self.evict(&ns, cap).await;
        }
        // Reap pending hypotheses never confirmed within a week (their fix was
        // never observed to work) so unconfirmed episodes don't pile up.
        self.prune_pending(7 * 24 * 3600).await;
        // Rebuild the similarity graph (enrichment for graph recall + the view).
        let _ = self.build_memory_graph(0.55, 6).await;
        (merged, evicted)
    }

    /// Locally-authored, non-`user` memories above `min_utility`, for diffusion.
    pub async fn high_utility(
        &self,
        min_utility: f64,
        limit: i64,
    ) -> Vec<(String, String, String)> {
        let rows = sqlx::query(
            "SELECT namespace, kind, text FROM memories
             WHERE status = 'active' AND origin = '' AND namespace NOT LIKE 'user%'
                   AND utility >= ?
             ORDER BY value DESC, utility DESC LIMIT ?",
        )
        .bind(min_utility)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .unwrap_or_default();
        rows.iter()
            .map(|r| {
                (
                    r.get::<String, _>("namespace"),
                    r.get::<String, _>("kind"),
                    r.get::<String, _>("text"),
                )
            })
            .collect()
    }

    /// Recent active episode texts of a given `kind` in the `agent` namespace
    /// (used by the self-healing evolution miner over `recovery`/`failure`).
    pub async fn episodes_by_kind(&self, kind: &str, limit: i64) -> Vec<String> {
        let rows = sqlx::query(
            "SELECT text FROM memories
             WHERE status = 'active' AND namespace = 'agent' AND kind = ?
             ORDER BY created_at DESC LIMIT ?",
        )
        .bind(kind)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .unwrap_or_default();
        rows.iter().map(|r| r.get::<String, _>("text")).collect()
    }

    /// A compact summary of self-healing activity for the `/api/heal` view +
    /// `/heal` overlays. Delegates to [`Store::heal_summary`] (one source of truth).
    pub async fn heal_summary(&self, limit: i64) -> serde_json::Value {
        self.store.heal_summary(limit).await
    }

    // --- Memory graph (similarity edges over SEDM) ----------------------

    /// Rebuild the memory similarity graph: link each memory to its top-`top_k`
    /// neighbors with cosine ≥ `threshold`. Pure enrichment — never touches the
    /// memories themselves. Powers graph-augmented recall + the graph view.
    pub async fn build_memory_graph(&self, threshold: f32, top_k: usize) -> usize {
        let vecs = self.active_vectors(None).await;
        let n = vecs.len();
        let mut edges: std::collections::HashMap<(i64, i64), f32> =
            std::collections::HashMap::new();
        for i in 0..n {
            let (id_i, _, _, vi) = &vecs[i];
            let mut sims: Vec<(i64, f32)> = Vec::new();
            for (j, (id_j, _, _, vj)) in vecs.iter().enumerate() {
                if i == j {
                    continue;
                }
                let s = dot(vi, vj);
                if s >= threshold {
                    sims.push((*id_j, s));
                }
            }
            sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            sims.truncate(top_k);
            for (id_j, s) in sims {
                let (a, b) = if *id_i < id_j {
                    (*id_i, id_j)
                } else {
                    (id_j, *id_i)
                };
                let e = edges.entry((a, b)).or_insert(0.0);
                if s > *e {
                    *e = s;
                }
            }
        }
        let Ok(mut tx) = self.pool().begin().await else {
            return 0;
        };
        let _ = sqlx::query("DELETE FROM memory_edges")
            .execute(&mut *tx)
            .await;
        for ((a, b), w) in &edges {
            let _ = sqlx::query(
                "INSERT OR REPLACE INTO memory_edges (src, dst, weight) VALUES (?, ?, ?)",
            )
            .bind(a)
            .bind(b)
            .bind(*w as f64)
            .execute(&mut *tx)
            .await;
        }
        let _ = tx.commit().await;
        edges.len()
    }

    /// Neighbor memory ids (+ weight) of any of `ids`, strongest first.
    async fn neighbor_ids(&self, ids: &[i64], limit: usize) -> Vec<(i64, f32)> {
        if ids.is_empty() {
            return vec![];
        }
        // ids are i64 from our own DB → safe to inline.
        let inlist = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let q = format!(
            "SELECT nid, weight FROM (
                 SELECT dst AS nid, weight FROM memory_edges WHERE src IN ({inlist})
                 UNION
                 SELECT src AS nid, weight FROM memory_edges WHERE dst IN ({inlist})
             ) ORDER BY weight DESC LIMIT {limit}"
        );
        let rows = sqlx::query(&q)
            .fetch_all(self.pool())
            .await
            .unwrap_or_default();
        let seed: std::collections::HashSet<i64> = ids.iter().copied().collect();
        rows.iter()
            .map(|r| (r.get::<i64, _>("nid"), r.get::<f64, _>("weight") as f32))
            .filter(|(id, _)| !seed.contains(id))
            .collect()
    }

    /// Memory-graph degree (number of similarity links) per id — used to
    /// down-weight over-connected "hub" memories at recall. ids are i64 from our
    /// own DB → safe to inline. Missing ids ⇒ degree 0 (graph empty / isolated).
    async fn degrees(&self, ids: &[i64]) -> std::collections::HashMap<i64, i64> {
        let mut out = std::collections::HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let inlist = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let q = format!(
            "SELECT nid, COUNT(*) AS d FROM (
                 SELECT src AS nid FROM memory_edges WHERE src IN ({inlist})
                 UNION ALL
                 SELECT dst AS nid FROM memory_edges WHERE dst IN ({inlist})
             ) GROUP BY nid"
        );
        for r in sqlx::query(&q)
            .fetch_all(self.pool())
            .await
            .unwrap_or_default()
        {
            out.insert(r.get::<i64, _>("nid"), r.get::<i64, _>("d"));
        }
        out
    }

    async fn node_info(&self, id: i64) -> Option<(String, String)> {
        sqlx::query("SELECT namespace AS ns, text FROM memories WHERE id = ? AND status = 'active'")
            .bind(id)
            .fetch_optional(self.pool())
            .await
            .ok()
            .flatten()
            .map(|r| (r.get::<String, _>("ns"), r.get::<String, _>("text")))
    }

    /// A query-centred subgraph for the memory-graph view: the top-`k` memories
    /// for `query` (seeds) + their neighbors, plus the edges among them.
    pub async fn memory_graph(&self, query: &str, k: usize) -> MemoryGraph {
        let seeds = self.search_inner(None, query, k, 0.0).await;
        if seeds.is_empty() {
            return MemoryGraph::default();
        }
        let seed_ids: Vec<i64> = seeds.iter().map(|s| s.id).collect();
        let nbrs = self.neighbor_ids(&seed_ids, k * 3).await;

        let mut nodes: Vec<MemNode> = seeds
            .iter()
            .map(|s| MemNode {
                id: s.id,
                namespace: s.namespace.clone(),
                text: s.text.clone(),
                score: s.score,
                seed: true,
            })
            .collect();
        let mut have: std::collections::HashSet<i64> = seed_ids.iter().copied().collect();
        for (nid, w) in &nbrs {
            if !have.insert(*nid) {
                continue;
            }
            if let Some((ns, text)) = self.node_info(*nid).await {
                nodes.push(MemNode {
                    id: *nid,
                    namespace: ns,
                    text,
                    score: *w,
                    seed: false,
                });
            }
        }
        let inlist = have
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let eq = format!(
            "SELECT src, dst, weight FROM memory_edges
             WHERE src IN ({inlist}) AND dst IN ({inlist})"
        );
        let edges = sqlx::query(&eq)
            .fetch_all(self.pool())
            .await
            .unwrap_or_default()
            .iter()
            .map(|r| MemEdge {
                src: r.get("src"),
                dst: r.get("dst"),
                weight: r.get::<f64, _>("weight") as f32,
            })
            .collect();
        MemoryGraph { nodes, edges }
    }
}

/// A node in a memory-graph view (a memory; `seed` = matched the query directly).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemNode {
    pub id: i64,
    pub namespace: String,
    pub text: String,
    pub score: f32,
    pub seed: bool,
}

/// A weighted similarity edge between two memories.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemEdge {
    pub src: i64,
    pub dst: i64,
    pub weight: f32,
}

/// A query-centred memory subgraph (graph-augmented recall + the D3 view).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MemoryGraph {
    pub nodes: Vec<MemNode>,
    pub edges: Vec<MemEdge>,
}

#[async_trait]
impl SemanticMemory for SemanticMemoryImpl {
    async fn recall(&self, query: &str, k: usize) -> Vec<RecalledMemory> {
        // Pull a wider cosine pool, then structure-aware re-rank: down-weight
        // over-connected "hub" memories (generic notes that match everything) by
        // their memory-graph degree, so specific, on-point memories surface. A
        // no-op when the graph is empty (degree 0 ⇒ penalty 1.0), so recall never
        // regresses before the first sweep builds the graph.
        let pool = (k * 3).max(12);
        let mut hits = self
            .search_inner(None, query, pool, self.params.recall_floor)
            .await;
        if !hits.is_empty() {
            let ids: Vec<i64> = hits.iter().map(|h| h.id).collect();
            let deg = self.degrees(&ids).await;
            for h in hits.iter_mut() {
                let d = *deg.get(&h.id).unwrap_or(&0) as f32;
                h.score *= 1.0 / (1.0 + (1.0 + d).ln()); // hub suppression
            }
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            hits.truncate(k);
        }
        // Graph-augmented: pull in strongly-connected neighbors of the top hits
        // for richer, coherent context. Bounded + strong-links-only, and a no-op
        // when the memory graph hasn't been built yet (so recall never regresses).
        if !hits.is_empty() {
            let seed_ids: Vec<i64> = hits.iter().map(|h| h.id).collect();
            let mut have: std::collections::HashSet<i64> = seed_ids.iter().copied().collect();
            for (nid, w) in self.neighbor_ids(&seed_ids, k).await {
                if w < 0.6 || hits.len() >= k * 2 {
                    break; // strongest-first → stop at the first weak/over-cap one
                }
                if have.insert(nid) {
                    if let Some((ns, text)) = self.node_info(nid).await {
                        hits.push(RecalledMemory {
                            id: nid,
                            namespace: ns,
                            text,
                            score: w,
                        });
                    }
                }
            }
        }
        hits
    }

    async fn note_used(&self, ids: &[i64]) {
        if ids.is_empty() {
            return;
        }
        let now = now();
        for id in ids {
            let _ = sqlx::query(
                "UPDATE memories SET hits = hits + 1, utility = utility + 0.25, last_used_at = ? WHERE id = ?",
            )
            .bind(&now)
            .bind(id)
            .execute(self.pool())
            .await;
        }
    }

    async fn remember(&self, namespace: &str, kind: &str, text: &str) -> Option<i64> {
        self.add(namespace, kind, text, None, "").await
    }

    async fn remember_pending(&self, namespace: &str, kind: &str, text: &str) -> Option<i64> {
        self.add_pending(namespace, kind, text, None, "").await
    }

    async fn confirm(&self, id: i64, provenance: Option<&str>) {
        self.promote(id, provenance).await;
    }

    async fn reward(&self, ids: &[i64], delta: f64) {
        SemanticMemoryImpl::reward(self, ids, delta).await;
    }

    async fn query(&self, namespace: Option<&str>, q: &str, k: usize) -> Vec<RecalledMemory> {
        self.search_inner(namespace, q, k, 0.0).await
    }
}

// --- vector helpers -------------------------------------------------------

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn blob_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn normalize(v: &mut [f32]) {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}

/// Dot product of two equal-length slices (cosine, since vectors are normalized).
fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use blumi_core::LlmError;

    /// Deterministic 4-dim embedder over a tiny vocabulary, so cosine ranking is
    /// predictable in tests (no model download).
    struct MockEmbedder;
    #[async_trait]
    impl EmbeddingClient for MockEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
            const VOCAB: [&str; 4] = ["rust", "python", "cooking", "music"];
            Ok(texts
                .iter()
                .map(|t| {
                    let lower = t.to_lowercase();
                    let mut v: Vec<f32> = VOCAB
                        .iter()
                        .map(|w| lower.matches(w).count() as f32)
                        .collect();
                    // Avoid all-zero vectors (keeps normalize well-defined).
                    if v.iter().all(|x| *x == 0.0) {
                        v[0] = 0.01;
                    }
                    v
                })
                .collect())
        }
        fn dim(&self) -> usize {
            4
        }
        fn model_id(&self) -> &str {
            "mock-4d"
        }
    }

    async fn store_with(embedder: Option<Arc<dyn EmbeddingClient>>) -> SemanticMemoryImpl {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        SemanticMemoryImpl::new(store, embedder, MemoryParams::default())
    }

    #[tokio::test]
    async fn probation_hides_until_confirmed_then_prunes() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;

        // A pending hypothesis is stored but invisible to recall (the whole store
        // filters status='active', so probation is enforced for free).
        let id = mem
            .remember_pending("agent", "recovery", "rust ownership borrow fix")
            .await
            .unwrap();
        assert!(
            mem.query(None, "rust ownership", 5).await.is_empty(),
            "pending memory must not be recallable"
        );

        // Confirming it (observed-good outcome) promotes it to recallable.
        mem.confirm(id, Some("verified")).await;
        assert!(
            mem.query(None, "rust ownership", 5)
                .await
                .iter()
                .any(|h| h.text.contains("rust")),
            "confirmed memory should be recallable"
        );

        // A never-confirmed hypothesis is reaped; the confirmed one survives.
        mem.remember_pending("agent", "recovery", "python import fix")
            .await
            .unwrap();
        // Negative age ⇒ cutoff slightly in the future, so the fresh pending row
        // is always eligible regardless of clock granularity; active rows are
        // never touched (the DELETE filters status='pending').
        let pruned = mem.prune_pending(-1).await;
        assert_eq!(pruned, 1, "exactly the one pending row is pruned");
        assert!(
            !mem.query(None, "rust ownership", 5).await.is_empty(),
            "the confirmed fix survives the prune"
        );
    }

    #[tokio::test]
    async fn degrees_counts_graph_links_for_hub_suppression() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        // Distinct-but-similar memories (none dedup-merge): the bridge links to
        // both rust and python and is the "hub"; the cake is isolated.
        let a = mem
            .remember("agent", "note", "rust ownership")
            .await
            .unwrap();
        let bridge = mem
            .remember("agent", "note", "rust and python together")
            .await
            .unwrap();
        let p = mem.remember("agent", "note", "python lists").await.unwrap();
        let d = mem
            .remember("agent", "note", "cooking a cake")
            .await
            .unwrap();
        mem.build_memory_graph(0.55, 6).await;

        let deg = mem.degrees(&[a, bridge, p, d]).await;
        let dbridge = deg.get(&bridge).copied().unwrap_or(0);
        assert!(
            dbridge >= 2,
            "bridge is the hub (rust + python), got {dbridge}"
        );
        assert!(
            deg.get(&a).copied().unwrap_or(0) >= 1,
            "rust links to bridge"
        );
        assert_eq!(
            deg.get(&d).copied().unwrap_or(0),
            0,
            "isolated cake: no links"
        );
        // Hub-suppression: the hub takes a stronger penalty than a leaf node.
        let pen = |dg: i64| 1.0_f32 / (1.0 + (1.0 + dg as f32).ln());
        assert!(
            pen(dbridge) < pen(deg.get(&a).copied().unwrap_or(0)),
            "the hub is penalized more than a leaf"
        );
    }

    #[tokio::test]
    async fn value_fitness_rewards_and_evicts_least_valuable() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        let a = mem
            .remember("agent", "note", "rust ownership")
            .await
            .unwrap();
        let b = mem.remember("agent", "note", "python lists").await.unwrap();
        // `a` proves useful over productive turns; `b` proves useless.
        mem.reward(&[a], 1.0).await; // value 1.0 → 2.0
        mem.reward(&[b], -0.9).await; // value 1.0 → 0.1 (clamped ≥ 0)

        // Cap of 1 ⇒ evict the lowest-VALUE row (b), not the least-retrieved.
        assert_eq!(mem.evict("agent", 1).await, 1, "one over cap");
        let active: Vec<i64> = mem
            .list_memories(Some("agent"), Some("active"), 50)
            .await
            .iter()
            .map(|e| e.id)
            .collect();
        assert!(active.contains(&a), "high-value memory survives");
        assert!(!active.contains(&b), "low-value memory evicted");

        // value is surfaced (white-box editor) and reflects the reward.
        let ea = mem.get_memory(a).await.unwrap();
        assert!(ea.value > 1.5, "rewarded value rose, got {}", ea.value);
    }

    #[tokio::test]
    async fn supersede_removes_from_recall_but_keeps_audit() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        let old = mem
            .remember("agent", "note", "rust ownership")
            .await
            .unwrap();
        let new = mem
            .remember("agent", "note", "rust and python")
            .await
            .unwrap();
        mem.supersede(old, new).await;
        // Superseded ⇒ excluded from recall/query (status != active)...
        let hits = mem.query(Some("agent"), "rust ownership", 5).await;
        assert!(hits.iter().all(|h| h.id != old), "superseded not recalled");
        // ...but retained for audit/restore, with provenance to the winner.
        let sup = mem
            .list_memories(Some("agent"), Some("superseded"), 5)
            .await;
        assert_eq!(sup.len(), 1, "superseded row kept");
        assert!(sup[0]
            .source_session
            .as_deref()
            .unwrap_or("")
            .contains("superseded_by"));
    }

    #[tokio::test]
    async fn ingest_consensus_raises_value() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        let id = mem
            .remember("agent", "note", "rust ownership")
            .await
            .unwrap();
        let v0 = mem.get_memory(id).await.unwrap().value;
        // A peer independently holds the same memory ⇒ ingest merges + corroborates.
        assert!(
            mem.ingest_remote("agent", "note", "rust ownership", "peer-1")
                .await
        );
        let v1 = mem.get_memory(id).await.unwrap().value;
        assert!(v1 > v0, "consensus raised value: {v0} -> {v1}");
    }

    #[tokio::test]
    async fn conflict_candidates_finds_band_pairs_only() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        let a = mem
            .remember("agent", "note", "rust ownership")
            .await
            .unwrap();
        let b = mem
            .remember("agent", "note", "rust and python")
            .await
            .unwrap();
        let c = mem
            .remember("agent", "note", "cooking a cake")
            .await
            .unwrap();
        let pairs = mem.conflict_candidates("agent", 0.5, 0.92, 10).await;
        assert!(
            pairs
                .iter()
                .any(|(x, _, y, _)| (*x == a && *y == b) || (*x == b && *y == a)),
            "the similar rust pair is a conflict candidate"
        );
        assert!(
            pairs.iter().all(|(x, _, y, _)| *x != c && *y != c),
            "the orthogonal cake is no one's candidate"
        );
    }

    #[tokio::test]
    async fn cosine_ranks_planted_neighbor_first() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        mem.remember("agent", "note", "rust ownership and borrowing")
            .await
            .unwrap();
        mem.remember("agent", "note", "chocolate cake cooking recipe")
            .await
            .unwrap();
        mem.remember("agent", "note", "python list comprehension")
            .await
            .unwrap();

        let hits = mem.query(None, "rust borrow checker", 3).await;
        assert!(!hits.is_empty());
        assert!(
            hits[0].text.contains("rust"),
            "expected the rust memory first, got {:?}",
            hits[0].text
        );
    }

    #[tokio::test]
    async fn memory_graph_links_similar() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        mem.remember("agent", "note", "rust ownership")
            .await
            .unwrap();
        let bridge = mem
            .remember("agent", "note", "rust and python together")
            .await
            .unwrap();
        mem.remember("agent", "note", "python lists").await.unwrap();
        mem.remember("agent", "note", "cooking a cake")
            .await
            .unwrap();

        // Distinct-but-similar memories link; the isolated cake doesn't.
        let edges = mem.build_memory_graph(0.55, 6).await;
        assert!(edges >= 2, "expected similarity edges, got {edges}");
        let nbrs = mem.neighbor_ids(&[bridge], 10).await;
        assert!(nbrs.len() >= 2, "bridge should connect ≥2 memories");

        // A query subgraph has seed nodes.
        let g = mem.memory_graph("rust", 3).await;
        assert!(!g.nodes.is_empty(), "graph has nodes");
        assert!(g.nodes.iter().any(|n| n.seed), "has a seed node");
    }

    #[tokio::test]
    async fn write_admission_merges_near_duplicate() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        let a = mem
            .remember("agent", "note", "rust rust rust")
            .await
            .unwrap();
        // Identical vector → cosine 1.0 ≥ dedup_threshold → merges (same id).
        let b = mem
            .remember("agent", "note", "rust rust rust")
            .await
            .unwrap();
        assert_eq!(a, b, "near-duplicate should merge into the same memory");
        assert_eq!(mem.count("agent").await, 1);
    }

    #[tokio::test]
    async fn fts_fallback_without_embeddings() {
        let mem = store_with(None).await;
        mem.remember("user", "fact", "the deploy command is make ship")
            .await
            .unwrap();
        mem.remember("user", "fact", "favourite colour is teal")
            .await
            .unwrap();
        let hits = mem.query(None, "deploy", 5).await;
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("make ship"));
    }

    #[tokio::test]
    async fn eviction_caps_namespace_by_utility() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        // Distinct directions so none dedup-merge (pairwise cosine < 0.92).
        let texts = [
            "rust topic",
            "python topic",
            "cooking topic",
            "music topic",
            "rust python topic",
        ];
        let mut ids = Vec::new();
        for t in texts {
            ids.push(mem.remember("project:x", "note", t).await.unwrap());
        }
        assert_eq!(
            mem.count("project:x").await,
            5,
            "distinct vectors must not merge"
        );

        // Boost the first so it has the highest utility and survives eviction.
        let keep = ids[0];
        for _ in 0..10 {
            mem.note_used(&[keep]).await;
        }
        let evicted = mem.evict("project:x", 2).await;
        assert_eq!(evicted, 3);
        assert_eq!(mem.count("project:x").await, 2);
        let still = mem.query(Some("project:x"), "rust", 5).await;
        assert!(still.iter().any(|h| h.id == keep));
    }

    #[tokio::test]
    async fn pinned_entry_survives_eviction() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        let texts = [
            "rust topic",
            "python topic",
            "cooking topic",
            "music topic",
            "rust python topic",
        ];
        let mut ids = Vec::new();
        for t in texts {
            ids.push(mem.remember("project:x", "note", t).await.unwrap());
        }
        // Pin a low-utility entry — it must survive even though it'd be evicted.
        let pinned = ids[1];
        assert!(mem.set_pinned(pinned, true).await);
        mem.evict("project:x", 1).await;
        let remaining = mem
            .list_memories(Some("project:x"), Some("active"), 100)
            .await;
        assert!(
            remaining.iter().any(|e| e.id == pinned && e.pinned),
            "pinned entry must survive eviction"
        );
    }

    #[tokio::test]
    async fn update_text_reembeds_for_vector_search() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        let id = mem
            .remember("agent", "note", "rust ownership")
            .await
            .unwrap();
        assert!(mem.update_memory_text(id, "python lists").await);
        assert_eq!(mem.get_memory(id).await.unwrap().text, "python lists");
        // Re-embedded: the entry now ranks for the new topic.
        let hits = mem.query(None, "python", 5).await;
        assert!(hits.iter().any(|h| h.id == id));
    }

    #[tokio::test]
    async fn update_text_resyncs_fts_without_embeddings() {
        let mem = store_with(None).await;
        let id = mem
            .remember("user", "fact", "deploy with make ship")
            .await
            .unwrap();
        assert!(
            mem.update_memory_text(id, "deploy with cargo release")
                .await
        );
        // FTS index follows: the new term hits, the old one doesn't.
        assert!(mem.query(None, "cargo", 5).await.iter().any(|h| h.id == id));
        assert!(mem.query(None, "make", 5).await.is_empty());
    }

    #[tokio::test]
    async fn delete_and_pin_roundtrip() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        let id = mem
            .remember("agent", "note", "rust ownership")
            .await
            .unwrap();
        assert!(mem.set_pinned(id, true).await);
        assert!(mem.get_memory(id).await.unwrap().pinned);
        assert!(mem.delete_memory(id).await);
        assert!(mem.get_memory(id).await.is_none());
        assert_eq!(mem.count("agent").await, 0);
        // Gone from search too (FTS trigger + vector cascade).
        assert!(mem.query(None, "rust", 5).await.iter().all(|h| h.id != id));
    }

    #[tokio::test]
    async fn diffusion_export_excludes_user_namespace() {
        let mem = store_with(Some(Arc::new(MockEmbedder))).await;
        mem.remember("user", "fact", "private rust preference")
            .await
            .unwrap();
        mem.remember("agent", "note", "shared rust convention")
            .await
            .unwrap();
        let export = mem.high_utility(0.0, 10).await;
        assert!(export.iter().all(|(ns, _, _)| ns != "user"));
        assert!(export.iter().any(|(ns, _, _)| ns == "agent"));
    }
}

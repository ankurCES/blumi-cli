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

    /// Embed + L2-normalize one text, or `None` if embeddings are off/erroring.
    async fn embed_one(&self, text: &str) -> Option<Vec<f32>> {
        let emb = self.embedder.as_ref()?;
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
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(qvec) = self.embed_one(text).await {
            if let Some((id, sim)) = self.best_match(namespace, &qvec).await {
                if sim >= self.params.dedup_threshold {
                    self.merge(id).await;
                    return Some(id);
                }
            }
            let id = self
                .insert(namespace, kind, text, source_session, origin)
                .await?;
            self.insert_vector(id, &qvec).await;
            Some(id)
        } else {
            // No embeddings: dedup on exact text, else insert (no vector).
            if let Some(id) = self.find_exact(namespace, text).await {
                self.merge(id).await;
                return Some(id);
            }
            self.insert(namespace, kind, text, source_session, origin)
                .await
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
        self.add(namespace, kind, text, None, origin)
            .await
            .is_some()
    }

    async fn insert(
        &self,
        namespace: &str,
        kind: &str,
        text: &str,
        source_session: Option<&str>,
        origin: &str,
    ) -> Option<i64> {
        let now = now();
        let res = sqlx::query(
            "INSERT INTO memories
                (namespace, kind, text, origin, source_session, created_at, updated_at,
                 hits, last_used_at, utility, status)
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, NULL, 1.0, 'active')",
        )
        .bind(namespace)
        .bind(kind)
        .bind(text)
        .bind(origin)
        .bind(source_session)
        .bind(&now)
        .bind(&now)
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

    /// Fold near-duplicate clusters in `namespace` into the highest-utility
    /// member (losers → `status='merged'`, utility/hits folded into the keeper).
    /// Returns how many were merged. No-op without vectors.
    pub async fn consolidate(&self, namespace: &str) -> usize {
        let rows = sqlx::query(
            "SELECT m.id AS id, m.utility AS utility, m.hits AS hits, v.vec AS vec
             FROM memories m JOIN memory_vectors v ON v.memory_id = m.id
             WHERE m.status = 'active' AND m.namespace = ?
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
                 WHERE namespace = ? AND status = 'active'
                 ORDER BY utility ASC, updated_at ASC LIMIT ?
             )",
        )
        .bind(namespace)
        .bind(over)
        .execute(self.pool())
        .await
        .map(|r| r.rows_affected() as usize)
        .unwrap_or(0)
    }

    /// One governance sweep over every namespace: consolidate then evict.
    /// Returns `(merged, evicted)` totals.
    pub async fn sweep(&self) -> (usize, usize) {
        let cap = self.params.max_per_namespace;
        let mut merged = 0;
        let mut evicted = 0;
        for ns in self.namespaces().await {
            merged += self.consolidate(&ns).await;
            evicted += self.evict(&ns, cap).await;
        }
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
             ORDER BY utility DESC LIMIT ?",
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
}

#[async_trait]
impl SemanticMemory for SemanticMemoryImpl {
    async fn recall(&self, query: &str, k: usize) -> Vec<RecalledMemory> {
        self.search_inner(None, query, k, self.params.recall_floor)
            .await
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

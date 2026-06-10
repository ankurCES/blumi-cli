//! Semantic long-term memory (LangGraph "Store" analog).
//!
//! A provider-neutral trait the agent loop and the `memory` tool use to recall
//! and persist durable knowledge across sessions — backed by an embeddings
//! vector index with an FTS5 keyword fallback. The implementation lives in
//! `blumi-persist` (it owns the SQLite store); `blumi-core` only calls it, so a
//! missing store degrades to today's behaviour (file-based MEMORY.md + FTS5).

use async_trait::async_trait;

/// One memory surfaced by recall/query, with its relevance score (cosine
/// similarity for vector search, or a normalized rank for the FTS fallback).
#[derive(Debug, Clone)]
pub struct RecalledMemory {
    pub id: i64,
    pub namespace: String,
    pub text: String,
    pub score: f32,
}

/// Learned per-symbol fitness for code search. The agent runner reports turn
/// outcome via [`reward_surfaced`](CodeFitness::reward_surfaced); the store
/// applies it to whatever symbols it surfaced since the last call (mirroring
/// `SemanticMemory`'s value-fitness). Best-effort — must never break a turn.
#[async_trait]
pub trait CodeFitness: Send + Sync {
    /// Reward (positive `delta`) or penalize (negative) the code symbols surfaced
    /// since the last call, then clear the surfaced set.
    async fn reward_surfaced(&self, delta: f64);
}

/// Long-term semantic memory: recall relevant facts, record that they were used
/// (the SEDM utility signal), and persist new ones through a write-admission
/// (dedup) gate. Every method is best-effort — a backing-store error must never
/// break a turn, so failures surface as empty results / `None`.
#[async_trait]
pub trait SemanticMemory: Send + Sync {
    /// Top-`k` relevant memories across recallable namespaces for `query`,
    /// filtered to reasonably-relevant hits (used for background RAG injection).
    async fn recall(&self, query: &str, k: usize) -> Vec<RecalledMemory>;

    /// Record that these memory ids were surfaced/used (bumps hits + utility +
    /// last-used), so consolidation/eviction can favour what actually helps.
    async fn note_used(&self, ids: &[i64]);

    /// Persist a memory under `namespace`/`kind`, applying the write-admission
    /// dedup gate (a near-duplicate merges instead of inserting). Returns the
    /// stored (or merged-into) id, or `None` on failure.
    async fn remember(&self, namespace: &str, kind: &str, text: &str) -> Option<i64>;

    /// Persist a memory as a *pending hypothesis*: stored but excluded from
    /// recall/mining/diffusion until [`SemanticMemory::confirm`]ed by an observed
    /// outcome. Use for guided recoveries whose fix is not yet proven to work.
    /// The default impl falls back to [`SemanticMemory::remember`] (no probation)
    /// for stores that don't model a lifecycle.
    async fn remember_pending(&self, namespace: &str, kind: &str, text: &str) -> Option<i64> {
        self.remember(namespace, kind, text).await
    }

    /// Promote a pending memory to active (recallable) once its outcome is
    /// observed to be good, recording optional `provenance` (the evidence it
    /// worked) and reinforcing it. The default impl reinforces via
    /// [`SemanticMemory::note_used`].
    async fn confirm(&self, id: i64, provenance: Option<&str>) {
        let _ = provenance;
        self.note_used(&[id]).await;
    }

    /// Adjust the learned *value* (fitness) of these memories by `delta` (may be
    /// negative), clamped at zero — the outcome signal eviction ranks by, fed by
    /// turn success/failure and RPL regret. Distinct from [`note_used`], which
    /// only measures retrieval engagement. Default: no-op.
    async fn reward(&self, ids: &[i64], delta: f64) {
        let _ = (ids, delta);
    }

    /// Explicit search (the `memory` tool's `query` action). `namespace = None`
    /// searches all namespaces; no relevance floor is applied (returns best k).
    async fn query(&self, namespace: Option<&str>, q: &str, k: usize) -> Vec<RecalledMemory>;
}

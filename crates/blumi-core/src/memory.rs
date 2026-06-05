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

    /// Explicit search (the `memory` tool's `query` action). `namespace = None`
    /// searches all namespaces; no relevance floor is applied (returns best k).
    async fn query(&self, namespace: Option<&str>, q: &str, k: usize) -> Vec<RecalledMemory>;
}

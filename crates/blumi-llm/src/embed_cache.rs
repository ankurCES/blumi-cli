//! A content-keyed cache wrapping any [`EmbeddingClient`].
//!
//! Embedding the same text twice — a re-run code search, a repeated recall
//! query, the same text re-embedded by a sweep, or duplicate texts arriving from
//! grid peers — returns the stored vector instead of re-running the model under
//! the process-global embedder lock. That cuts both redundant compute and lock
//! contention between memory recall and code search.
//!
//! Caching is sound because embeddings are **deterministic** for a fixed model,
//! and the wrapped client is fixed for the process — identical text always maps
//! to the identical vector, so a cache hit can never change a result. The cache
//! is bounded (FIFO eviction) so it can't grow without limit.

use async_trait::async_trait;
use blumi_core::{EmbeddingClient, LlmError};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Wraps an embedding backend with a bounded text→vector cache.
pub struct CachingEmbeddingClient {
    inner: Arc<dyn EmbeddingClient>,
    cache: Mutex<Cache>,
    cap: usize,
}

struct Cache {
    map: HashMap<String, Vec<f32>>,
    /// Insertion order, for FIFO eviction once `cap` is reached.
    order: VecDeque<String>,
}

impl CachingEmbeddingClient {
    /// Wrap `inner` with a default-capacity (2048-entry) cache.
    pub fn new(inner: Arc<dyn EmbeddingClient>) -> Self {
        Self::with_capacity(inner, 2048)
    }

    pub fn with_capacity(inner: Arc<dyn EmbeddingClient>, cap: usize) -> Self {
        CachingEmbeddingClient {
            inner,
            cache: Mutex::new(Cache {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
            cap: cap.max(1),
        }
    }
}

impl Cache {
    fn insert(&mut self, key: String, val: Vec<f32>, cap: usize) {
        if self.map.contains_key(&key) {
            return;
        }
        while self.map.len() >= cap {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, val);
    }
}

#[async_trait]
impl EmbeddingClient for CachingEmbeddingClient {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        // Pass 1: fill known slots from the cache; record the misses by position.
        let mut out: Vec<Option<Vec<f32>>> = Vec::with_capacity(texts.len());
        let mut miss_idx: Vec<usize> = Vec::new();
        let miss_txt: Vec<String>;
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            let mut misses = Vec::new();
            for (i, t) in texts.iter().enumerate() {
                match cache.map.get(t) {
                    Some(v) => out.push(Some(v.clone())),
                    None => {
                        out.push(None);
                        miss_idx.push(i);
                        misses.push(t.clone());
                    }
                }
            }
            miss_txt = misses;
        }

        // Pass 2: embed only the misses (one batched call), fill + cache them.
        if !miss_txt.is_empty() {
            let fresh = self.inner.embed(&miss_txt).await?;
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            for (k, &idx) in miss_idx.iter().enumerate() {
                if let Some(v) = fresh.get(k) {
                    out[idx] = Some(v.clone());
                    cache.insert(miss_txt[k].clone(), v.clone(), self.cap);
                }
            }
        }

        out.into_iter()
            .map(|o| o.ok_or_else(|| LlmError::Other(anyhow::anyhow!("embed: missing vector"))))
            .collect()
    }

    fn dim(&self) -> usize {
        self.inner.dim()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn ready(&self) -> bool {
        self.inner.ready()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingEmbedder {
        calls: Mutex<usize>,
    }
    #[async_trait]
    impl EmbeddingClient for CountingEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
            *self.calls.lock().unwrap() += texts.len();
            Ok(texts
                .iter()
                .map(|t| vec![t.len() as f32, 0.0, 0.0])
                .collect())
        }
        fn dim(&self) -> usize {
            3
        }
        fn model_id(&self) -> &str {
            "counting"
        }
    }

    #[tokio::test]
    async fn caches_repeats_and_preserves_order() {
        let inner = Arc::new(CountingEmbedder {
            calls: Mutex::new(0),
        });
        let c = CachingEmbeddingClient::new(inner.clone());

        let v1 = c.embed(&["a".into(), "bb".into()]).await.unwrap();
        assert_eq!(v1.len(), 2);
        assert_eq!(v1[0][0], 1.0); // "a".len()
        assert_eq!(v1[1][0], 2.0); // "bb".len()
        assert_eq!(*inner.calls.lock().unwrap(), 2);

        // "a" is cached; only "ccc" is a miss → one more inner embed, order kept.
        let v2 = c.embed(&["a".into(), "ccc".into()]).await.unwrap();
        assert_eq!(v2[0][0], 1.0);
        assert_eq!(v2[1][0], 3.0);
        assert_eq!(*inner.calls.lock().unwrap(), 3, "only the miss is embedded");
    }

    #[tokio::test]
    async fn evicts_past_capacity() {
        let inner = Arc::new(CountingEmbedder {
            calls: Mutex::new(0),
        });
        let c = CachingEmbeddingClient::with_capacity(inner.clone(), 2);
        for t in ["a", "b", "c"] {
            c.embed(&[t.to_string()]).await.unwrap();
        }
        // "a" was evicted (cap 2) → re-embedding it is a fresh call.
        c.embed(&["a".to_string()]).await.unwrap();
        assert_eq!(*inner.calls.lock().unwrap(), 4);
    }
}

//! Native-lite code knowledge base.
//!
//! Ingests a path into a sibling SQLite DB (`knowledge.db`): each file is split
//! into [`extract`]ed symbols, indexed in FTS5 (name + snippet) and — when an
//! embeddings client is available — a brute-force-cosine vector index. Search is
//! hybrid (FTS first for keyword/symbol precision, vector fill for semantic
//! recall). Re-ingest is diff-aware (sha256 per file).
//!
//! Mirrors `blumi-persist`'s [`Store`](blumi_persist) style; degrades to FTS5
//! when embeddings are off, and to chunk-only when a language isn't recognized.

pub mod extract;
#[cfg(feature = "code-graph")]
pub mod extract_ts;

/// How the code reference graph is built. Mirrors `blumi_config::GraphMode`,
/// kept config-dep-free here (like `blumi_persist::MemoryParams`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphMode {
    /// Build no graph.
    Off,
    /// Name-co-occurrence heuristic — the always-available default (Tier 0).
    #[default]
    Lite,
    /// Typed, scope-resolved structural graph (tree-sitter; the `code-graph`
    /// build feature). Tier 1 — parsers land in P1/P2; treated as `Lite` until then.
    Structural,
}

use blumi_core::EmbeddingClient;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// One search/retrieve hit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeHit {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub start_line: i64,
    pub end_line: i64,
    pub snippet: String,
    pub score: f32,
}

/// Per-ingest filtering knobs.
#[derive(Debug, Clone)]
pub struct IngestConfig {
    /// Skip files larger than this (KiB). 0 = no cap.
    pub max_file_kb: u64,
    /// Path substrings to skip (in addition to .gitignore + default noise dirs).
    pub exclude: Vec<String>,
}

impl Default for IngestConfig {
    fn default() -> Self {
        IngestConfig {
            max_file_kb: 256,
            exclude: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IngestStats {
    pub indexed: usize,
    pub skipped: usize,
    pub symbols: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceInfo {
    pub source: String,
    pub files: i64,
    pub symbols: i64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct KnowledgeStatus {
    pub files: i64,
    pub symbols: i64,
    pub vectors: i64,
    pub sources: Vec<SourceInfo>,
}

/// The code knowledge store.
pub struct KnowledgeStore {
    pool: SqlitePool,
    embedder: Option<Arc<dyn EmbeddingClient>>,
    graph_mode: GraphMode,
}

impl KnowledgeStore {
    /// Open (creating if needed) the knowledge DB and run migrations.
    pub async fn open(
        path: &Path,
        embedder: Option<Arc<dyn EmbeddingClient>>,
    ) -> Result<Self, KnowledgeError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(KnowledgeStore {
            pool,
            embedder,
            graph_mode: GraphMode::default(),
        })
    }

    /// In-memory store for tests.
    pub async fn open_in_memory(
        embedder: Option<Arc<dyn EmbeddingClient>>,
    ) -> Result<Self, KnowledgeError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(KnowledgeStore {
            pool,
            embedder,
            graph_mode: GraphMode::default(),
        })
    }

    /// Set how the reference graph is built (default [`GraphMode::Lite`]).
    pub fn with_graph_mode(mut self, mode: GraphMode) -> Self {
        self.graph_mode = mode;
        self
    }

    fn ready(&self) -> bool {
        self.embedder.as_ref().map(|e| e.ready()).unwrap_or(false)
    }

    // --- Ingest ----------------------------------------------------------

    /// Walk `root` (gitignore-aware) and index changed/new files under the
    /// `source` label. Diff-aware: unchanged files (by sha) are skipped.
    pub async fn ingest_path(
        &self,
        root: &Path,
        source: &str,
        cfg: &IngestConfig,
    ) -> Result<IngestStats, KnowledgeError> {
        // Heal any vectors missing from a previous cold-start ingest.
        self.backfill_vectors(512).await;

        let mut stats = IngestStats::default();
        let walker = ignore::WalkBuilder::new(root)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .parents(true)
            .build();

        for dent in walker.flatten() {
            if !dent.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = dent.path();
            let path_str = path.to_string_lossy().to_string();
            if should_skip(&path_str, cfg) {
                continue;
            }
            let meta = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if cfg.max_file_kb > 0 && meta.len() > cfg.max_file_kb * 1024 {
                stats.skipped += 1;
                continue;
            }
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c, // non-UTF8 (binary) read fails → skipped
                Err(_) => {
                    stats.skipped += 1;
                    continue;
                }
            };
            if content.trim().is_empty() {
                continue;
            }
            let sha = sha256(&content);

            // Diff-aware: skip unchanged files.
            let existing: Option<String> =
                sqlx::query_scalar("SELECT sha FROM code_files WHERE path = ?")
                    .bind(&path_str)
                    .fetch_optional(&self.pool)
                    .await?;
            if existing.as_deref() == Some(sha.as_str()) {
                stats.skipped += 1;
                continue;
            }

            let symbols = extract::extract(&path_str, &content);
            let lang = extract::lang_for(&path_str);
            let now = now();

            // Replace the file's prior rows (cascade clears symbols/vec/fts).
            let mut tx = self.pool.begin().await?;
            sqlx::query("DELETE FROM code_files WHERE path = ?")
                .bind(&path_str)
                .execute(&mut *tx)
                .await?;
            let res = sqlx::query(
                "INSERT INTO code_files (source, path, lang, sha, symbols, indexed_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(source)
            .bind(&path_str)
            .bind(lang)
            .bind(&sha)
            .bind(symbols.len() as i64)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            let file_id = res.last_insert_rowid();

            let mut sym_ids = Vec::with_capacity(symbols.len());
            for s in &symbols {
                let r = sqlx::query(
                    "INSERT INTO code_symbols (file_id, name, kind, start_line, end_line, snippet)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(file_id)
                .bind(&s.name)
                .bind(&s.kind)
                .bind(s.start_line as i64)
                .bind(s.end_line as i64)
                .bind(&s.snippet)
                .execute(&mut *tx)
                .await?;
                sym_ids.push(r.last_insert_rowid());
            }
            tx.commit().await?;

            // Embed snippets in one batch per file (best-effort; FTS works without).
            if self.ready() && !symbols.is_empty() {
                let docs: Vec<String> = symbols.iter().map(|s| s.snippet.clone()).collect();
                self.embed_and_store(&sym_ids, &docs).await;
            }

            stats.indexed += 1;
            stats.symbols += symbols.len();
        }
        // Rebuild the symbol reference graph over the (updated) index.
        let _ = self.build_graph().await;
        Ok(stats)
    }

    async fn embed_and_store(&self, sym_ids: &[i64], docs: &[String]) {
        let Some(emb) = &self.embedder else { return };
        let Ok(vecs) = emb.embed(docs).await else {
            return;
        };
        let model = emb.model_id().to_string();
        for (id, mut v) in sym_ids.iter().zip(vecs) {
            normalize(&mut v);
            let _ = sqlx::query(
                "INSERT OR REPLACE INTO code_vec (symbol_id, model, dim, vec) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(&model)
            .bind(v.len() as i64)
            .bind(vec_to_blob(&v))
            .execute(&self.pool)
            .await;
        }
    }

    /// Embed symbols that have no vector yet (e.g. indexed during a cold start).
    /// No-op until the embeddings model is ready. Returns how many were embedded.
    pub async fn backfill_vectors(&self, limit: i64) -> usize {
        if !self.ready() {
            return 0;
        }
        let rows = sqlx::query(
            "SELECT s.id AS id, s.snippet AS snippet
             FROM code_symbols s LEFT JOIN code_vec v ON v.symbol_id = s.id
             WHERE v.symbol_id IS NULL LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        if rows.is_empty() {
            return 0;
        }
        let ids: Vec<i64> = rows.iter().map(|r| r.get::<i64, _>("id")).collect();
        let docs: Vec<String> = rows.iter().map(|r| r.get::<String, _>("snippet")).collect();
        self.embed_and_store(&ids, &docs).await;
        ids.len()
    }

    // --- Search / retrieve ----------------------------------------------

    /// Hybrid search: **FTS first** (keyword + exact-symbol precision — a search
    /// for `PermissionEngine` lands on that symbol, not an unrelated chunk that
    /// merely embeds nearby), then **vector fill** (semantic recall for whatever
    /// the keywords missed), up to `k` hits.
    pub async fn search(&self, query: &str, k: usize) -> Vec<CodeHit> {
        if k == 0 || query.trim().is_empty() {
            return vec![];
        }
        let mut out: Vec<CodeHit> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();

        // FTS first.
        for hit in self.fts_candidates(query, k * 2).await {
            if seen.insert(hit_id(&hit)) {
                out.push(hit);
                if out.len() >= k {
                    break;
                }
            }
        }

        // Vector fill for the remaining slots (semantic recall).
        if out.len() < k {
            if let Some(qvec) = self.embed_one(query).await {
                let mut scored = self.vector_candidates(&qvec).await;
                scored.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                for hit in scored {
                    if seen.insert(hit_id(&hit)) {
                        out.push(hit);
                        if out.len() >= k {
                            break;
                        }
                    }
                }
            }
        }
        out.truncate(k);
        out
    }

    /// All symbols with a vector, scored by cosine against `qvec` (≥ floor).
    async fn vector_candidates(&self, qvec: &[f32]) -> Vec<CodeHit> {
        let rows = sqlx::query(
            "SELECT f.path AS path, s.name AS name, s.kind AS kind,
                    s.start_line AS sl, s.end_line AS el, s.snippet AS snippet, v.vec AS vec
             FROM code_vec v
             JOIN code_symbols s ON s.id = v.symbol_id
             JOIN code_files f ON f.id = s.file_id",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.iter()
            .filter_map(|r| {
                let blob: Vec<u8> = r.get("vec");
                let score = dot(qvec, &blob_to_vec(&blob));
                if score < 0.25 {
                    return None;
                }
                Some(CodeHit {
                    path: r.get("path"),
                    name: r.get("name"),
                    kind: r.get("kind"),
                    start_line: r.get("sl"),
                    end_line: r.get("el"),
                    snippet: r.get("snippet"),
                    score,
                })
            })
            .collect()
    }

    async fn fts_candidates(&self, query: &str, limit: usize) -> Vec<CodeHit> {
        let fts = to_fts_query(query);
        if fts.is_empty() {
            return vec![];
        }
        let rows = sqlx::query(
            "SELECT f.path AS path, s.name AS name, s.kind AS kind,
                    s.start_line AS sl, s.end_line AS el, s.snippet AS snippet
             FROM code_fts x
             JOIN code_symbols s ON s.id = x.rowid
             JOIN code_files f ON f.id = s.file_id
             WHERE code_fts MATCH ? ORDER BY rank LIMIT ?",
        )
        .bind(&fts)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.iter()
            .map(|r| CodeHit {
                path: r.get("path"),
                name: r.get("name"),
                kind: r.get("kind"),
                start_line: r.get("sl"),
                end_line: r.get("el"),
                snippet: r.get("snippet"),
                score: 0.0,
            })
            .collect()
    }

    /// Retrieve symbols by file-path substring, optionally filtered to one name.
    pub async fn retrieve(&self, path_like: &str, symbol: Option<&str>) -> Vec<CodeHit> {
        let like = format!("%{path_like}%");
        let rows = match symbol {
            Some(name) => {
                sqlx::query(
                    "SELECT f.path AS path, s.name AS name, s.kind AS kind,
                            s.start_line AS sl, s.end_line AS el, s.snippet AS snippet
                     FROM code_symbols s JOIN code_files f ON f.id = s.file_id
                     WHERE f.path LIKE ? AND s.name = ? ORDER BY s.start_line LIMIT 50",
                )
                .bind(&like)
                .bind(name)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT f.path AS path, s.name AS name, s.kind AS kind,
                            s.start_line AS sl, s.end_line AS el, s.snippet AS snippet
                     FROM code_symbols s JOIN code_files f ON f.id = s.file_id
                     WHERE f.path LIKE ? ORDER BY s.start_line LIMIT 50",
                )
                .bind(&like)
                .fetch_all(&self.pool)
                .await
            }
        }
        .unwrap_or_default();
        rows.iter()
            .map(|r| CodeHit {
                path: r.get("path"),
                name: r.get("name"),
                kind: r.get("kind"),
                start_line: r.get("sl"),
                end_line: r.get("el"),
                snippet: r.get("snippet"),
                score: 1.0,
            })
            .collect()
    }

    // --- Management ------------------------------------------------------

    pub async fn sources(&self) -> Vec<SourceInfo> {
        sqlx::query(
            "SELECT source, COUNT(*) AS files, COALESCE(SUM(symbols),0) AS syms
             FROM code_files GROUP BY source ORDER BY source",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| SourceInfo {
            source: r.get("source"),
            files: r.get("files"),
            symbols: r.get("syms"),
        })
        .collect()
    }

    /// Remove an ingested source (all its files + cascaded symbols/vectors).
    pub async fn remove(&self, source: &str) -> usize {
        sqlx::query("DELETE FROM code_files WHERE source = ?")
            .bind(source)
            .execute(&self.pool)
            .await
            .map(|r| r.rows_affected() as usize)
            .unwrap_or(0)
    }

    pub async fn status(&self) -> KnowledgeStatus {
        let files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM code_files")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let symbols: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM code_symbols")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let vectors: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM code_vec")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        KnowledgeStatus {
            files,
            symbols,
            vectors,
            sources: self.sources().await,
        }
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

    // --- Reference graph (neighbors / shortest_path / hubs) --------------

    /// Rebuild the symbol reference graph: an edge src→dst means src's body
    /// mentions dst's name. Full rebuild (cheap at native-lite scale).
    pub async fn build_graph(&self) -> Result<usize, KnowledgeError> {
        // Tier-1 structural building lands in P2; for now Off skips, everything
        // else uses the lite name-co-occurrence builder below.
        if self.graph_mode == GraphMode::Off {
            return Ok(0);
        }
        let rows = sqlx::query("SELECT id, name, snippet FROM code_symbols")
            .fetch_all(&self.pool)
            .await?;
        let mut by_name: std::collections::HashMap<String, Vec<i64>> =
            std::collections::HashMap::new();
        let mut syms: Vec<(i64, String, String)> = Vec::with_capacity(rows.len());
        for r in &rows {
            let id: i64 = r.get("id");
            let name: String = r.get("name");
            let snippet: String = r.get("snippet");
            if name.len() >= 3 {
                by_name.entry(name.clone()).or_default().push(id);
            }
            syms.push((id, name, snippet));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM code_edges")
            .execute(&mut *tx)
            .await?;
        let mut count = 0usize;
        for (id, name, snippet) in &syms {
            let mut seen: HashSet<i64> = HashSet::new();
            for tok in identifiers(snippet) {
                if tok == name || tok.len() < 4 || is_stop_ident(tok) {
                    continue;
                }
                if let Some(dsts) = by_name.get(tok) {
                    // Skip over-common names (defined in many places) — they're
                    // "god nodes" that add noise, not signal.
                    if dsts.len() > 8 {
                        continue;
                    }
                    for &dst in dsts {
                        if dst != *id && seen.insert(dst) {
                            sqlx::query(
                                "INSERT OR IGNORE INTO code_edges (src, dst) VALUES (?, ?)",
                            )
                            .bind(id)
                            .bind(dst)
                            .execute(&mut *tx)
                            .await?;
                            count += 1;
                        }
                    }
                }
            }
        }
        tx.commit().await?;
        Ok(count)
    }

    /// Symbols directly connected to any symbol named `name` (both directions).
    pub async fn neighbors(&self, name: &str, limit: usize) -> Vec<CodeHit> {
        let rows = sqlx::query(
            "SELECT DISTINCT f.path AS path, s.name AS name, s.kind AS kind,
                    s.start_line AS sl, s.end_line AS el, s.snippet AS snippet
             FROM code_symbols s JOIN code_files f ON f.id = s.file_id
             WHERE s.id IN (
                 SELECT e.dst FROM code_edges e JOIN code_symbols a ON a.id = e.src WHERE a.name = ?1
                 UNION
                 SELECT e.src FROM code_edges e JOIN code_symbols b ON b.id = e.dst WHERE b.name = ?1
             )
             ORDER BY s.name LIMIT ?2",
        )
        .bind(name)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.iter()
            .map(|r| CodeHit {
                path: r.get("path"),
                name: r.get("name"),
                kind: r.get("kind"),
                start_line: r.get("sl"),
                end_line: r.get("el"),
                snippet: r.get("snippet"),
                score: 0.0,
            })
            .collect()
    }

    /// Most-connected symbols ("god nodes"), by total degree (score = degree).
    pub async fn hubs(&self, limit: usize) -> Vec<CodeHit> {
        let rows = sqlx::query(
            "SELECT f.path AS path, s.name AS name, s.kind AS kind,
                    s.start_line AS sl, s.end_line AS el, s.snippet AS snippet, d.deg AS deg
             FROM (SELECT id, COUNT(*) AS deg FROM
                       (SELECT src AS id FROM code_edges UNION ALL SELECT dst FROM code_edges)
                   GROUP BY id) d
             JOIN code_symbols s ON s.id = d.id
             JOIN code_files f ON f.id = s.file_id
             ORDER BY d.deg DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.iter()
            .map(|r| CodeHit {
                path: r.get("path"),
                name: r.get("name"),
                kind: r.get("kind"),
                start_line: r.get("sl"),
                end_line: r.get("el"),
                snippet: r.get("snippet"),
                score: r.get::<i64, _>("deg") as f32,
            })
            .collect()
    }

    /// Shortest reference path between a symbol named `from` and one named `to`,
    /// as a list of symbol names (empty if unreachable within `max_depth`).
    pub async fn shortest_path(&self, from: &str, to: &str, max_depth: usize) -> Vec<String> {
        use std::collections::{HashMap, VecDeque};
        let names = sqlx::query("SELECT id, name FROM code_symbols")
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        let mut name_of: HashMap<i64, String> = HashMap::new();
        let mut ids_of: HashMap<String, Vec<i64>> = HashMap::new();
        for r in &names {
            let id: i64 = r.get("id");
            let nm: String = r.get("name");
            ids_of.entry(nm.clone()).or_default().push(id);
            name_of.insert(id, nm);
        }
        let goals: HashSet<i64> = ids_of
            .get(to)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        if goals.is_empty() || !ids_of.contains_key(from) {
            return vec![];
        }
        let edges = sqlx::query("SELECT src, dst FROM code_edges")
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        let mut adj: HashMap<i64, Vec<i64>> = HashMap::new();
        for r in &edges {
            let s: i64 = r.get("src");
            let d: i64 = r.get("dst");
            adj.entry(s).or_default().push(d);
            adj.entry(d).or_default().push(s);
        }
        let mut visited: HashSet<i64> = HashSet::new();
        let mut prev: HashMap<i64, i64> = HashMap::new();
        let mut q: VecDeque<(i64, usize)> = VecDeque::new();
        for &id in ids_of.get(from).map(|v| v.as_slice()).unwrap_or(&[]) {
            visited.insert(id);
            q.push_back((id, 0));
        }
        let mut found: Option<i64> = None;
        while let Some((id, depth)) = q.pop_front() {
            if depth > 0 && goals.contains(&id) {
                found = Some(id);
                break;
            }
            if depth >= max_depth {
                continue;
            }
            if let Some(ns) = adj.get(&id) {
                for &n in ns {
                    if visited.insert(n) {
                        prev.insert(n, id);
                        q.push_back((n, depth + 1));
                    }
                }
            }
        }
        let Some(mut cur) = found else {
            return vec![];
        };
        let mut path = vec![name_of.get(&cur).cloned().unwrap_or_default()];
        while let Some(&p) = prev.get(&cur) {
            path.push(name_of.get(&p).cloned().unwrap_or_default());
            cur = p;
        }
        path.reverse();
        path
    }
}

// --- helpers --------------------------------------------------------------

fn hit_id(h: &CodeHit) -> i64 {
    // A stable per-symbol key for dedup across vector/FTS passes.
    let mut hasher = Sha256::new();
    hasher.update(h.path.as_bytes());
    hasher.update(h.start_line.to_le_bytes());
    let d = hasher.finalize();
    i64::from_le_bytes(d[0..8].try_into().unwrap_or_default())
}

/// Default noise directories never worth indexing (in case they aren't ignored).
const NOISE_DIRS: &[&str] = &[
    "/.git/",
    "/node_modules/",
    "/target/",
    "/build/",
    "/dist/",
    "/.dart_tool/",
    "/.venv/",
    "/__pycache__/",
    "/vendor/",
];

/// Asset / binary / generated extensions that are noise in a code KB (many are
/// UTF-8 — e.g. SVGs — so the binary read-check alone doesn't exclude them).
const SKIP_EXT: &[&str] = &[
    "svg", "png", "jpg", "jpeg", "gif", "webp", "ico", "bmp", "pdf", "woff", "woff2", "ttf", "otf",
    "eot", "mp3", "mp4", "mov", "wav", "zip", "gz", "tar", "tgz", "wasm", "lock", "map",
];

fn should_skip(path: &str, cfg: &IngestConfig) -> bool {
    if NOISE_DIRS.iter().any(|d| path.contains(d)) {
        return true;
    }
    if cfg
        .exclude
        .iter()
        .any(|e| !e.is_empty() && path.contains(e))
    {
        return true;
    }
    let lower = path.to_lowercase();
    if lower.ends_with(".min.js") {
        return true;
    }
    // Asset/binary/generated files add noise with little code value.
    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    SKIP_EXT.contains(&ext)
}

fn sha256(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

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

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Free text → safe FTS5 query (each term quoted + AND-ed).
fn to_fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Identifier-like tokens (alphanumeric + `_`, length ≥ 3) for graph edges.
fn identifiers(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|t| t.len() >= 3)
}

/// Ubiquitous lowercase identifiers (trait methods, enum variants, common
/// fields) that would otherwise become noisy graph "god nodes". CamelCase type
/// names are unaffected (the match is exact + lowercase).
fn is_stop_ident(t: &str) -> bool {
    matches!(
        t,
        "name"
            | "new"
            | "from"
            | "into"
            | "user"
            | "self"
            | "kind"
            | "text"
            | "path"
            | "none"
            | "some"
            | "value"
            | "error"
            | "result"
            | "default"
            | "clone"
            | "format"
            | "with"
            | "iter"
            | "into_iter"
            | "unwrap"
            | "async"
            | "await"
            | "send"
            | "recv"
            | "spawn"
            | "string"
            | "options"
            | "config"
            | "data"
            | "item"
            | "items"
            | "args"
            | "input"
            | "output"
            | "label"
            | "title"
            | "status"
            | "message"
            | "content"
            | "model"
            | "query"
            | "create"
            | "build"
            | "open"
            | "save"
            | "load"
            | "search"
            | "parse"
            | "write"
            | "read"
            | "render"
            | "update"
            | "handle"
            | "resolve"
            | "fetch"
            | "store"
            | "list"
            | "apply"
            | "init"
            | "start"
            | "stop"
            | "run"
            | "next"
            | "push"
            | "insert"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> KnowledgeStore {
        KnowledgeStore::open_in_memory(None).await.unwrap()
    }

    #[tokio::test]
    async fn ingest_and_search_fts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("calc.py"),
            "def add(a, b):\n    return a + b\n\nclass Calculator:\n    def multiply(self, a, b):\n        return a * b\n",
        )
        .unwrap();
        let ks = store().await;
        let stats = ks
            .ingest_path(dir.path(), "test", &IngestConfig::default())
            .await
            .unwrap();
        assert_eq!(stats.indexed, 1);
        assert!(stats.symbols >= 3);

        let hits = ks.search("multiply", 5).await;
        assert!(hits.iter().any(|h| h.name == "multiply"));

        // Diff-aware: re-ingest unchanged → skipped, not re-indexed.
        let stats2 = ks
            .ingest_path(dir.path(), "test", &IngestConfig::default())
            .await
            .unwrap();
        assert_eq!(stats2.indexed, 0);
        assert_eq!(stats2.skipped, 1);
    }

    #[tokio::test]
    async fn skips_assets_and_ranks_symbol_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("perm.rs"),
            "pub struct PermissionEngine {\n  yolo: bool,\n}\nimpl PermissionEngine {\n  pub fn new() -> Self { Self { yolo: false } }\n}\n",
        )
        .unwrap();
        // A UTF-8 asset that used to pollute results — must be skipped.
        std::fs::write(
            dir.path().join("logo.svg"),
            "<svg><path d=\"M0 0 L9 9\"/></svg>\n",
        )
        .unwrap();
        let ks = store().await; // None embedder → FTS-first path
        let stats = ks
            .ingest_path(dir.path(), "t", &IngestConfig::default())
            .await
            .unwrap();
        assert_eq!(stats.indexed, 1, "the .svg asset must be skipped");

        let hits = ks.search("PermissionEngine", 5).await;
        assert!(!hits.is_empty());
        assert!(
            hits[0].name.contains("PermissionEngine"),
            "exact symbol must rank first, got {:?}",
            hits[0].name
        );
        assert!(hits.iter().all(|h| !h.path.ends_with(".svg")));
    }

    #[tokio::test]
    async fn graph_neighbors_and_path() {
        let dir = tempfile::tempdir().unwrap();
        // `caller` references `helper`; `helper` references `leaf`.
        std::fs::write(
            dir.path().join("g.rs"),
            "fn leaf() -> u32 {\n    1\n}\n\nfn helper() -> u32 {\n    leaf() + 1\n}\n\nfn caller() -> u32 {\n    helper() + 2\n}\n",
        )
        .unwrap();
        let ks = store().await;
        ks.ingest_path(dir.path(), "g", &IngestConfig::default())
            .await
            .unwrap();
        // caller ↔ helper is a direct edge.
        let nbrs = ks.neighbors("caller", 10).await;
        assert!(nbrs.iter().any(|h| h.name == "helper"), "caller → helper");
        // caller → helper → leaf is a 2-hop path.
        let path = ks.shortest_path("caller", "leaf", 6).await;
        assert_eq!(path, vec!["caller", "helper", "leaf"]);
        // hubs returns something connected.
        assert!(!ks.hubs(5).await.is_empty(), "graph has hubs");
    }

    #[tokio::test]
    async fn retrieve_and_sources_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("lib.rs"),
            "pub fn hello() -> &'static str {\n    \"hi\"\n}\n",
        )
        .unwrap();
        let ks = store().await;
        ks.ingest_path(dir.path(), "proj", &IngestConfig::default())
            .await
            .unwrap();

        let got = ks.retrieve("lib.rs", Some("hello")).await;
        assert_eq!(got.len(), 1);
        assert!(got[0].snippet.contains("\"hi\""));

        let sources = ks.sources().await;
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source, "proj");

        let removed = ks.remove("proj").await;
        assert_eq!(removed, 1);
        assert_eq!(ks.status().await.files, 0);
    }
}

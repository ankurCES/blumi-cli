//! `blumi knowledge` — manage the native-lite code knowledge base.
//!
//! Indexes repos into `knowledge.db` so the `code_search` / `code_retrieve`
//! tools (and you, from the CLI) can find code by meaning or keyword. Shares the
//! process-global embeddings model with the rest of blumi.

use blumi_config::BlumiConfig;
use blumi_knowledge::{IngestConfig, KnowledgeStore};

/// Resolve the configured code-graph mode for this build. `structural` requires
/// the `code-graph` build feature; without it we warn and fall back to `lite`.
pub fn graph_mode(config: &BlumiConfig) -> blumi_knowledge::GraphMode {
    use blumi_config::GraphMode as C;
    use blumi_knowledge::GraphMode as K;
    match config.knowledge.graph.mode {
        C::Off => K::Off,
        C::Lite => K::Lite,
        C::Structural if cfg!(feature = "code-graph") => K::Structural,
        C::Structural => {
            tracing::warn!(
                "knowledge.graph.mode=structural needs the `code-graph` build feature; using lite"
            );
            K::Lite
        }
    }
}

fn ingest_cfg(config: &BlumiConfig) -> IngestConfig {
    IngestConfig {
        max_file_kb: config.knowledge.max_file_kb,
        exclude: config.knowledge.exclude.clone(),
    }
}

async fn open(config: &BlumiConfig) -> anyhow::Result<KnowledgeStore> {
    let embedder = crate::engine::shared_embedder(config);
    KnowledgeStore::open(&config.paths.knowledge_db, embedder)
        .await
        .map_err(|e| anyhow::anyhow!("open knowledge db: {e}"))
}

/// Warm the embeddings model (one-time load/download) so the operation produces
/// vectors / uses vector search. No-op when embeddings are disabled.
async fn warm(config: &BlumiConfig) {
    if let Some(emb) = crate::engine::shared_embedder(config) {
        if !emb.ready() {
            eprintln!("• warming embeddings model (first run downloads ~130 MB)…");
            let _ = emb.embed(&["warmup".to_string()]).await;
        }
    }
}

pub async fn ingest(config: &BlumiConfig, path: String) -> anyhow::Result<()> {
    let root = std::fs::canonicalize(&path).map_err(|e| anyhow::anyhow!("path '{path}': {e}"))?;
    let source = root.to_string_lossy().to_string();
    warm(config).await;
    let ks = open(config).await?;
    eprintln!("• indexing {source} …");
    let stats = ks.ingest_path(&root, &source, &ingest_cfg(config)).await?;
    println!(
        "Indexed {} file(s) ({} symbol(s)); {} unchanged/skipped.",
        stats.indexed, stats.symbols, stats.skipped
    );
    Ok(())
}

pub async fn list(config: &BlumiConfig) -> anyhow::Result<()> {
    let ks = open(config).await?;
    let sources = ks.sources().await;
    if sources.is_empty() {
        println!("No indexed sources. Add one with `blumi knowledge ingest <path>`.");
        return Ok(());
    }
    println!("Indexed sources:");
    for s in &sources {
        println!(
            "  • {}  ({} files, {} symbols)",
            s.source, s.files, s.symbols
        );
    }
    Ok(())
}

pub async fn search(config: &BlumiConfig, query: String) -> anyhow::Result<()> {
    if query.trim().is_empty() {
        anyhow::bail!("empty query");
    }
    let ks = open(config).await?;
    // Use vector search when the index actually has vectors (else FTS is plenty).
    if ks.status().await.vectors > 0 {
        warm(config).await;
    }
    let hits = ks.search(&query, 10).await;
    if hits.is_empty() {
        println!("No code matches '{query}'.");
        return Ok(());
    }
    println!("{} hit(s) for '{query}':", hits.len());
    for h in &hits {
        println!("\n• {}:{} [{}] {}", h.path, h.start_line, h.kind, h.name);
        for line in h.snippet.lines().take(6) {
            println!("    {line}");
        }
    }
    Ok(())
}

pub async fn status(config: &BlumiConfig) -> anyhow::Result<()> {
    let ks = open(config).await?;
    let st = ks.status().await;
    println!(
        "Knowledge base: {} files · {} symbols · {} vectors · {} source(s)",
        st.files,
        st.symbols,
        st.vectors,
        st.sources.len()
    );
    Ok(())
}

pub async fn remove(config: &BlumiConfig, source: String) -> anyhow::Result<()> {
    let ks = open(config).await?;
    let n = ks.remove(&source).await;
    if n == 0 {
        println!("No source matched '{source}'. See `blumi knowledge list`.");
    } else {
        println!("Removed '{source}' ({n} file(s)).");
    }
    Ok(())
}

pub async fn neighbors(config: &BlumiConfig, symbol: String) -> anyhow::Result<()> {
    let ks = open(config).await?;
    let hits = ks.neighbors(&symbol, 30).await;
    if hits.is_empty() {
        println!("No graph neighbors for '{symbol}'. (Ingest a repo first.)");
        return Ok(());
    }
    println!("{} neighbor(s) of '{symbol}':", hits.len());
    for h in &hits {
        println!("  • {}:{} [{}] {}", h.path, h.start_line, h.kind, h.name);
    }
    Ok(())
}

/// Typed code-graph query: `callers` / `callees` / `impact` / `implementers`.
pub async fn relation(config: &BlumiConfig, kind: &str, symbol: String) -> anyhow::Result<()> {
    let ks = open(config).await?;
    let hits = match kind {
        "callers" => ks.callers(&symbol, 40).await,
        "callees" => ks.callees(&symbol, 40).await,
        "implementers" => ks.implementers(&symbol, 40).await,
        _ => ks.impact(&symbol, 5, 100).await, // "impact"
    };
    if hits.is_empty() {
        println!(
            "No {kind} for '{symbol}'. (Ingest a repo; set knowledge.graph.mode=structural \
             + build --features code-graph for precise edges.)"
        );
        return Ok(());
    }
    println!("{} {kind} of '{symbol}':", hits.len());
    for h in &hits {
        println!("  • {}:{} [{}] {}", h.path, h.start_line, h.kind, h.name);
    }
    Ok(())
}

pub async fn path(config: &BlumiConfig, from: String, to: String) -> anyhow::Result<()> {
    let ks = open(config).await?;
    let p = ks.shortest_path(&from, &to, 8).await;
    if p.is_empty() {
        println!("No reference path from '{from}' to '{to}' (within 8 hops).");
    } else {
        println!("{}", p.join(" → "));
    }
    Ok(())
}

pub async fn hubs(config: &BlumiConfig) -> anyhow::Result<()> {
    let ks = open(config).await?;
    let hits = ks.hubs(20).await;
    if hits.is_empty() {
        println!("No graph yet. Ingest a repo with `blumi knowledge ingest <path>`.");
        return Ok(());
    }
    println!("Most-connected symbols:");
    for h in &hits {
        println!(
            "  • {} ({} links)  {}:{}",
            h.name, h.score as i64, h.path, h.start_line
        );
    }
    Ok(())
}

//! `blumi session` — list, search, and show stored sessions.

use blumi_config::BlumiConfig;
use blumi_persist::Store;

pub async fn list(config: BlumiConfig) -> anyhow::Result<()> {
    let store = Store::open(&config.paths.db).await?;
    let sessions = store.list_sessions(50).await?;
    if sessions.is_empty() {
        println!("No sessions yet. Run `blumi run \"...\"` to create one.");
        return Ok(());
    }
    for s in sessions {
        println!(
            "{}  {:>3} msgs  {}  {}",
            &s.id, s.message_count, &s.updated_at, s.title
        );
    }
    Ok(())
}

/// `blumi stats` — aggregate token usage across stored sessions (claudectl-style).
pub async fn stats(config: BlumiConfig) -> anyhow::Result<()> {
    let store = Store::open(&config.paths.db).await?;
    let metas = store.list_sessions(1000).await?;
    if metas.is_empty() {
        println!("no sessions yet — run `blumi run \"...\"` or `blumi loop`.");
        return Ok(());
    }
    use std::collections::BTreeMap;
    let (mut msgs, mut tin, mut tout) = (0u64, 0u64, 0u64);
    let mut by: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    for m in &metas {
        msgs += m.message_count.max(0) as u64;
        tin += m.input_tokens.max(0) as u64;
        tout += m.output_tokens.max(0) as u64;
        let model = if m.model.is_empty() {
            "default".to_string()
        } else {
            m.model.clone()
        };
        let e = by.entry(model).or_default();
        e.0 += 1;
        e.1 += m.input_tokens.max(0) as u64;
        e.2 += m.output_tokens.max(0) as u64;
    }
    let k = |n: u64| {
        if n >= 1000 {
            format!("{:.1}k", n as f64 / 1000.0)
        } else {
            n.to_string()
        }
    };
    println!(
        "usage — {} sessions · {msgs} messages · ↑{} in · ↓{} out",
        metas.len(),
        k(tin),
        k(tout)
    );
    println!("\nby model:");
    for (model, (s, i, o)) in by {
        println!("  {model:<28} {s:>3} sessions · ↑{} ↓{}", k(i), k(o));
    }
    Ok(())
}

pub async fn search(config: BlumiConfig, query: String) -> anyhow::Result<()> {
    if query.trim().is_empty() {
        anyhow::bail!("provide a search query");
    }
    let store = Store::open(&config.paths.db).await?;
    let hits = store.search(&query, 25).await?;
    if hits.is_empty() {
        println!("No matches for {query:?}.");
        return Ok(());
    }
    for h in hits {
        println!("{}  {}", h.session_id, h.title);
        println!("  {}\n", h.snippet);
    }
    Ok(())
}

pub async fn show(config: BlumiConfig, id: String) -> anyhow::Result<()> {
    let store = Store::open(&config.paths.db).await?;
    match store.load_session(&id).await? {
        Some(s) => {
            println!("# {}  ({})\n", s.meta.title, s.meta.id);
            for m in s.messages {
                let text = m.text();
                if !text.is_empty() {
                    println!("[{:?}] {text}\n", m.role);
                }
            }
        }
        None => println!("Session not found: {id}"),
    }
    Ok(())
}

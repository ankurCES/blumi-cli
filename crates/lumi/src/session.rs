//! `lumi session` — list, search, and show stored sessions.

use lumi_config::LumiConfig;
use lumi_persist::Store;

pub async fn list(config: LumiConfig) -> anyhow::Result<()> {
    let store = Store::open(&config.paths.db).await?;
    let sessions = store.list_sessions(50).await?;
    if sessions.is_empty() {
        println!("No sessions yet. Run `lumi run \"...\"` to create one.");
        return Ok(());
    }
    for s in sessions {
        println!(
            "{}  {:>3} msgs  {}  {}",
            &s.id,
            s.message_count,
            &s.updated_at,
            s.title
        );
    }
    Ok(())
}

pub async fn search(config: LumiConfig, query: String) -> anyhow::Result<()> {
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

pub async fn show(config: LumiConfig, id: String) -> anyhow::Result<()> {
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

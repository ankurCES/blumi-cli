//! Retrospection — a daily, differential replay of session transcripts into
//! durable semantic memory.
//!
//! Rather than re-reading the whole history on every turn, blumi consolidates
//! **only what's new since the last run**: a watermark in `~/.blumi/retrospect.json`
//! records the newest message already digested, the pass reads messages after it
//! across all sessions, asks the LLM to distill reusable learnings per session,
//! and stores them (dedup-merged, provenance-tagged) so future sessions recall
//! them cheaply. Driven by the background memory sweep, gated to once per
//! `memory.retrospect_hours`.
//!
//! Two timestamps are tracked, deliberately separate: `watermark` (the newest
//! message consolidated — the differential boundary) and `last_run` (when the
//! pass last executed — the cadence gate). Conflating them would skip work on
//! active sessions whose newest message is recent.

use blumi_core::{LlmClient, LlmOptions};
use blumi_persist::{SemanticMemoryImpl, Store};
use blumi_protocol::{Message, Role, StreamChunk};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

const EPOCH: &str = "1970-01-01T00:00:00Z";
const MAX_PER_MSG_CHARS: usize = 1500;
const MAX_TRANSCRIPT_CHARS: usize = 16000;

/// Persisted retrospection state (`~/.blumi/retrospect.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetrospectState {
    /// RFC3339 timestamp of the newest message consolidated — the differential
    /// boundary for the next pass.
    #[serde(default)]
    pub watermark: Option<String>,
    /// RFC3339 timestamp of when the pass last ran — the cadence gate.
    #[serde(default)]
    pub last_run: Option<String>,
}

impl RetrospectState {
    fn load(path: &Path) -> RetrospectState {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let tmp = path.with_extension("json.tmp");
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

/// Whether a retrospection pass is due (no prior run, or `hours` elapsed since).
pub fn due(path: &Path, hours: u64) -> bool {
    let state = RetrospectState::load(path);
    match state
        .last_run
        .as_deref()
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
    {
        Some(last) => (OffsetDateTime::now_utc() - last) >= time::Duration::hours(hours as i64),
        None => true,
    }
}

/// One retrospection pass: consolidate every session's transcript since the
/// watermark into memory. Returns `(sessions_seen, learnings_stored)`. Advances
/// the watermark to the newest message processed and always stamps `last_run`
/// (so the cadence gate moves even on an empty diff).
pub async fn retrospect_once(
    store: &Store,
    mem: &SemanticMemoryImpl,
    llm: &Arc<dyn LlmClient>,
    model: &str,
    state_path: &Path,
    max_messages: i64,
) -> (usize, usize) {
    let mut state = RetrospectState::load(state_path);
    let since = state.watermark.clone().unwrap_or_else(|| EPOCH.to_string());
    let rows = match store.messages_since(&since, max_messages).await {
        Ok(r) => r,
        Err(_) => return (0, 0),
    };

    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| since.clone());

    if rows.is_empty() {
        state.last_run = Some(now);
        state.save(state_path);
        return (0, 0);
    }

    // Group by session (preserving per-session order) + track the newest ts.
    let mut groups: Vec<(String, Vec<Message>)> = Vec::new();
    let mut newest = since.clone();
    for (sid, m) in rows {
        if let Ok(ts) = m.timestamp.format(&Rfc3339) {
            if ts > newest {
                newest = ts;
            }
        }
        if let Some(g) = groups.iter_mut().find(|(s, _)| *s == sid) {
            g.1.push(m);
        } else {
            groups.push((sid, vec![m]));
        }
    }

    let sessions_seen = groups.len();
    let mut stored = 0;
    for (sid, msgs) in groups {
        let transcript = render_transcript(&msgs);
        if transcript.trim().is_empty() {
            continue;
        }
        for line in extract(llm, model, &transcript).await {
            // origin = "" marks these as locally-authored, so they diffuse across
            // the grid like any other agent-namespace memory. Provenance is kept
            // via source_session + the "retrospection" kind, not via origin (a
            // non-empty origin means "received from a peer" and is excluded from
            // diffusion to prevent bounce-back).
            if mem
                .add("agent", "retrospection", &line, Some(&sid), "")
                .await
                .is_some()
            {
                stored += 1;
            }
        }
    }

    state.watermark = Some(newest);
    state.last_run = Some(now);
    state.save(state_path);
    (sessions_seen, stored)
}

/// Render User/Assistant turns into a compact transcript (tool I/O and system
/// prompts dropped), bounded per-message and overall (keeping the recent tail).
fn render_transcript(msgs: &[Message]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for m in msgs {
        let who = match m.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            _ => continue,
        };
        let text = m.text();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        lines.push(format!("{who}: {}", truncate(text, MAX_PER_MSG_CHARS)));
    }
    let joined = lines.join("\n");
    let chars: Vec<char> = joined.chars().collect();
    if chars.len() > MAX_TRANSCRIPT_CHARS {
        let tail: String = chars[chars.len() - MAX_TRANSCRIPT_CHARS..].iter().collect();
        format!("…\n{tail}")
    } else {
        joined
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head} …")
    }
}

const POLICY: &str = "You consolidate a chat transcript into durable long-term memory. Extract only \
reusable, lasting facts worth recalling in future sessions: user preferences, project decisions and \
conventions, environment/setup facts, and gotchas or fixes. Ignore transient chatter and one-off \
requests. Output a short list, one fact per line, each imperative and self-contained (no pronouns \
referring to 'the chat'). At most 8 lines. If nothing is worth keeping, output exactly NONE.";

async fn extract(llm: &Arc<dyn LlmClient>, model: &str, transcript: &str) -> Vec<String> {
    let opts = LlmOptions {
        model: model.to_string(),
        max_output_tokens: 400,
        temperature: 0.0,
        top_p: 1.0,
        top_k: 0,
        thinking: false,
        prompt_cache: false,
    };
    let prompt = [
        Message::system(POLICY),
        Message::user(format!("Transcript:\n{transcript}")),
    ];
    let mut stream = match llm
        .stream_chat(&prompt, &[], &opts, CancellationToken::new())
        .await
    {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(StreamChunk::Text { text }) => out.push_str(&text),
            Ok(StreamChunk::Done { .. }) => break,
            Err(_) => break,
            _ => {}
        }
    }
    parse_learnings(&out)
}

/// Parse the model's reply into clean learning lines (bullets/numbering stripped,
/// `NONE` and trivially-short lines dropped, capped at 8).
fn parse_learnings(out: &str) -> Vec<String> {
    if out.trim().eq_ignore_ascii_case("none") {
        return vec![];
    }
    out.lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(['-', '*', '•', '·'])
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
                .trim()
                .to_string()
        })
        .filter(|l| l.chars().count() > 8 && !l.eq_ignore_ascii_case("none"))
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_learnings_strips_bullets_and_none() {
        let out =
            "- Prefer tabs over spaces\n* Build with cargo nextest\nNONE\nx\n1. Use the staging DB";
        assert_eq!(
            parse_learnings(out),
            vec![
                "Prefer tabs over spaces".to_string(),
                "Build with cargo nextest".to_string(),
                "Use the staging DB".to_string(),
            ]
        );
        assert!(parse_learnings("NONE").is_empty());
        assert!(parse_learnings("  none  ").is_empty());
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 3), "hel …");
        let _ = truncate("héllo wörld", 4); // multi-byte must not panic
    }

    #[test]
    fn render_drops_non_text_roles() {
        let msgs = vec![Message::system("sys note"), Message::user("hi there")];
        let t = render_transcript(&msgs);
        assert!(t.contains("User: hi there"));
        assert!(!t.contains("sys note"));
    }
}

//! Background conflict resolver (opt-in via `memory.resolve_conflicts`).
//!
//! Same-topic memory pairs that didn't merge on write (cosine just below the
//! dedup threshold) may be genuine *contradictions*. This pass asks the LLM to
//! classify each candidate pair and supersedes the outdated side — the actuator
//! the conflict taxonomy (`conflict_candidates` / `supersede`) was built for.
//! Runs in the memory sweep, bounded per tick, and is best-effort: it never
//! panics, and a flaky or ambiguous judge resolves to "leave both" — never a
//! wrong supersede.

use blumi_core::{LlmClient, LlmOptions};
use blumi_persist::SemanticMemoryImpl;
use blumi_protocol::{Message, StreamChunk};
use futures::StreamExt;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const POLICY: &str = "You compare two stored memories and decide whether they directly \
CONTRADICT — i.e. only one can be true at the same time. Memories that are compatible, \
complementary, more/less specific, or about different things are NOT contradictions. Be \
conservative: when unsure, they do not contradict.";

enum Loser {
    A,
    B,
}

/// One resolver pass: classify same-topic candidate pairs and supersede the
/// outdated side. Returns how many conflicts were resolved (bounded by `cap`).
pub async fn resolve_once(
    mem: &SemanticMemoryImpl,
    llm: &Arc<dyn LlmClient>,
    model: &str,
    dedup_threshold: f32,
    cap: usize,
) -> usize {
    // Candidates sit below the dedup line (at/above it they would have merged on
    // write) but high enough to share a topic.
    let lo = (dedup_threshold - 0.12).max(0.5);
    let hi = dedup_threshold;
    let mut resolved = 0usize;
    for ns in mem.namespaces().await {
        if resolved >= cap {
            break;
        }
        for (id_a, text_a, id_b, text_b) in mem.conflict_candidates(&ns, lo, hi, cap).await {
            if resolved >= cap {
                break;
            }
            match classify(llm, model, &text_a, &text_b).await {
                Some(Loser::A) => {
                    mem.supersede(id_a, id_b).await;
                    resolved += 1;
                }
                Some(Loser::B) => {
                    mem.supersede(id_b, id_a).await;
                    resolved += 1;
                }
                None => {}
            }
        }
    }
    resolved
}

/// Ask the judge which memory is the outdated side of a contradiction, or `None`
/// when they don't contradict (the safe default for anything unclear).
async fn classify(llm: &Arc<dyn LlmClient>, model: &str, a: &str, b: &str) -> Option<Loser> {
    let user = format!(
        "Memory A: {a}\nMemory B: {b}\n\nIf they contradict and A is the outdated/incorrect \
         one, answer A. If they contradict and B is the outdated/incorrect one, answer B. If \
         they do not contradict, answer NONE. Reply with exactly one token: A, B, or NONE."
    );
    let opts = LlmOptions {
        model: model.to_string(),
        max_output_tokens: 8,
        temperature: 0.0,
        top_p: 1.0,
        top_k: 0,
        thinking: false,
        prompt_cache: false,
    };
    let prompt = [Message::system(POLICY), Message::user(user)];
    let mut stream = llm
        .stream_chat(&prompt, &[], &opts, CancellationToken::new())
        .await
        .ok()?;
    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(StreamChunk::Text { text }) => out.push_str(&text),
            Ok(StreamChunk::Done { .. }) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    classify_verdict(&out)
}

/// Parse the judge's reply into a loser. Exact whole-token match (so "Answer: A"
/// still reads as A); any ambiguity or `NONE` leaves both memories untouched.
fn classify_verdict(out: &str) -> Option<Loser> {
    let toks: Vec<String> = out
        .to_ascii_uppercase()
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if toks.iter().any(|t| t == "NONE") {
        return None;
    }
    let a = toks.iter().any(|t| t == "A");
    let b = toks.iter().any(|t| t == "B");
    match (a, b) {
        (true, false) => Some(Loser::A),
        (false, true) => Some(Loser::B),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_parsing_is_conservative() {
        assert!(matches!(classify_verdict("A"), Some(Loser::A)));
        assert!(matches!(classify_verdict("Answer: B"), Some(Loser::B)));
        assert!(classify_verdict("NONE").is_none());
        // Ambiguous / unparseable → leave both untouched.
        assert!(classify_verdict("A or B").is_none());
        assert!(classify_verdict("Both seem fine").is_none());
        assert!(classify_verdict("").is_none());
    }
}

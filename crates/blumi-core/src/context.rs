//! Context-window management: when the conversation grows past a fraction of
//! the model's context, summarize the older messages into one synopsis and keep
//! a recent tail. A pragmatic port of OpenMono's checkpoint/compaction (without
//! a real tokenizer yet — uses a chars/4 estimate, refined later).

use crate::emit::EventEmitter;
use crate::llm::{LlmClient, LlmOptions};
use crate::session::SessionState;
use blumi_protocol::{Event, Message, Role, StreamChunk};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Rough chars-per-token used to estimate window size without a tokenizer.
const CHARS_PER_TOKEN: usize = 4;
/// Per-message overhead (role markers, delimiters) in characters.
const MESSAGE_OVERHEAD_CHARS: usize = 16;

pub struct ContextManager {
    context_size: u32,
    /// Fraction of the context at which compaction triggers.
    compact_threshold: f32,
    /// Number of trailing messages always kept verbatim.
    keep_messages: usize,
}

impl ContextManager {
    pub fn new(context_size: u32) -> Self {
        ContextManager {
            context_size,
            compact_threshold: 0.8,
            keep_messages: 8,
        }
    }

    /// Estimated token count of a set of messages. Counts text, per-message
    /// overhead, AND tool-call arguments — the latter aren't part of `text()`
    /// but are sent to the model and often dominate (e.g. a `FileWrite` carries
    /// the whole file in its arguments). Undercounting here was letting the
    /// window overflow before compaction fired.
    pub fn estimate_tokens(messages: &[Message]) -> usize {
        let chars: usize = messages
            .iter()
            .map(|m| {
                let mut n = m.text().len() + MESSAGE_OVERHEAD_CHARS;
                for tc in &m.tool_calls {
                    n += tc.name.len() + tc.arguments.to_string().len();
                }
                n
            })
            .sum();
        chars / CHARS_PER_TOKEN
    }

    fn budget(&self) -> usize {
        (self.context_size as f32 * self.compact_threshold) as usize
    }

    /// If the conversation exceeds the budget, summarize the older messages
    /// in-place and emit a `Compaction` event. Returns whether it compacted.
    pub async fn maybe_compact(
        &self,
        llm: &Arc<dyn LlmClient>,
        state: &Arc<Mutex<SessionState>>,
        options: &LlmOptions,
        events: &EventEmitter,
        ct: &CancellationToken,
    ) -> bool {
        self.compact(llm, state, options, events, ct, false).await
    }

    /// Force a compaction now regardless of the budget (the manual `/compact`).
    /// Still keeps the recent tail and needs enough history to be worthwhile.
    pub async fn compact_now(
        &self,
        llm: &Arc<dyn LlmClient>,
        state: &Arc<Mutex<SessionState>>,
        options: &LlmOptions,
        events: &EventEmitter,
        ct: &CancellationToken,
    ) -> bool {
        self.compact(llm, state, options, events, ct, true).await
    }

    async fn compact(
        &self,
        llm: &Arc<dyn LlmClient>,
        state: &Arc<Mutex<SessionState>>,
        options: &LlmOptions,
        events: &EventEmitter,
        ct: &CancellationToken,
        force: bool,
    ) -> bool {
        let messages = {
            let st = state.lock().await;
            // Use the larger of our estimate and the provider's last measured
            // prompt size, so a real-but-underestimated context still compacts.
            let used = Self::estimate_tokens(&st.messages).max(st.last_prompt_tokens as usize);
            if !force && used < self.budget() {
                return false;
            }
            if st.messages.len() <= self.keep_messages + 2 {
                return false; // too little to be worth compacting
            }
            st.messages.clone()
        };

        let cutoff = safe_cutoff(&messages, self.keep_messages);
        if cutoff == 0 {
            return false;
        }
        let Some(summary) = self.summarize(llm, &messages[..cutoff], options, ct).await else {
            return false;
        };

        let (compressed, tokens_after) = {
            let mut st = state.lock().await;
            // Re-derive the cutoff against current length (the turn may have
            // appended since we cloned). `safe_cutoff` also never starts the kept
            // tail on a tool_result — otherwise its matching tool_use would be in
            // the summarized (dropped) portion, leaving an orphan that the
            // provider rejects with a 400 on the next request.
            let cutoff = safe_cutoff(&st.messages, self.keep_messages);
            if cutoff == 0 {
                return false;
            }
            let tail = st.messages.split_off(cutoff);
            let summary_msg = Message::user(format!(
                "[Summary of the earlier conversation, condensed to save context]\n\n{summary}"
            ));
            st.messages = std::iter::once(summary_msg).chain(tail).collect();
            // The conversation just shrank: refresh the measured floor so the
            // compaction decision (and the live meter) reflect the new size
            // immediately, rather than staying pinned at the pre-compaction high.
            let after = Self::estimate_tokens(&st.messages) as u32;
            st.last_prompt_tokens = after;
            (cutoff, after)
        };

        events.emit(Event::Compaction {
            messages_compressed: compressed as u32,
            checkpoint: 0,
            tokens_after,
        });
        true
    }

    /// Ask the model to summarize a slice of the conversation.
    async fn summarize(
        &self,
        llm: &Arc<dyn LlmClient>,
        messages: &[Message],
        options: &LlmOptions,
        ct: &CancellationToken,
    ) -> Option<String> {
        summarize_history(llm, messages, options, &ct.child_token()).await
    }
}

/// Summarize a conversation into a concise handoff so a fresh context (an
/// in-place compaction or a rolled-over session) can continue seamlessly.
/// Returns `None` if the provider errors or yields nothing usable.
pub async fn summarize_history(
    llm: &Arc<dyn LlmClient>,
    messages: &[Message],
    options: &LlmOptions,
    ct: &CancellationToken,
) -> Option<String> {
    let transcript = render_transcript(messages);
    let prompt = vec![
        Message::system(
            "You compress conversations. Summarize the following so the assistant can \
             continue seamlessly: preserve decisions made, files touched, important facts, \
             and any unfinished work. Be concise but complete.",
        ),
        Message::user(transcript),
    ];

    let mut stream = match llm.stream_chat(&prompt, &[], options, ct.clone()).await {
        Ok(s) => s,
        Err(_) => return None,
    };

    let mut summary = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(StreamChunk::Text { text }) => summary.push_str(&text),
            Ok(StreamChunk::Done { .. }) => {}
            Err(_) => return None,
            _ => {}
        }
    }
    (!summary.trim().is_empty()).then_some(summary)
}

/// Where to split history for compaction: keep the last `keep` messages, but
/// never let the kept tail *begin* with a tool result — its matching `tool_use`
/// would be in the summarized (dropped) part, leaving an orphan `tool_result`
/// that every provider rejects with a 400. Advance the boundary forward past any
/// leading tool results so the kept tail always starts on a clean message.
fn safe_cutoff(messages: &[Message], keep: usize) -> usize {
    if messages.len() <= keep {
        return 0;
    }
    let mut cutoff = messages.len() - keep;
    while cutoff < messages.len() && messages[cutoff].role == Role::Tool {
        cutoff += 1;
    }
    cutoff
}

fn render_transcript(messages: &[Message]) -> String {
    let mut out = String::new();
    for m in messages {
        let text = m.text();
        if text.is_empty() {
            continue;
        }
        out.push_str(&format!("{:?}: {}\n", m.role, text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ProviderCaps, ToolSpec};
    use crate::LlmError;
    use async_trait::async_trait;
    use blumi_protocol::{FinishReason, SessionId};
    use futures::stream::BoxStream;

    struct SummaryLlm;
    #[async_trait]
    impl LlmClient for SummaryLlm {
        async fn stream_chat(
            &self,
            _m: &[Message],
            _t: &[ToolSpec],
            _o: &LlmOptions,
            _ct: CancellationToken,
        ) -> Result<BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError> {
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(StreamChunk::Text {
                    text: "SUMMARY".into(),
                }),
                Ok(StreamChunk::Done {
                    reason: FinishReason::Stop,
                }),
            ])))
        }
        fn caps(&self) -> ProviderCaps {
            ProviderCaps::default()
        }
    }

    #[test]
    fn estimate_scales_with_text() {
        let small = [Message::user("hi")];
        let big = [Message::user("x".repeat(4000))];
        assert!(ContextManager::estimate_tokens(&big) > ContextManager::estimate_tokens(&small));
    }

    #[test]
    fn safe_cutoff_never_starts_tail_on_a_tool_result() {
        use blumi_protocol::{Role, ToolCall, ToolCallId};
        let asst = |id: &str| {
            Message::assistant_tool_calls(
                None,
                vec![ToolCall {
                    id: ToolCallId::from(id),
                    name: "Bash".into(),
                    arguments: serde_json::json!({}),
                }],
            )
        };
        let tool = |id: &str| Message::tool_result(ToolCallId::from(id), "Bash", "out");
        // u, a(t1), tool(t1), a(t2), tool(t2)
        let msgs = vec![
            Message::user("hi"),
            asst("t1"),
            tool("t1"),
            asst("t2"),
            tool("t2"),
        ];
        // keep=3 → naive cutoff=2 lands on tool(t1) (its tool_use would be dropped).
        let c = safe_cutoff(&msgs, 3);
        assert_eq!(c, 3, "advanced past the orphan tool result");
        assert_ne!(msgs[c].role, Role::Tool);
    }

    #[test]
    fn estimate_counts_tool_call_args() {
        use blumi_protocol::{ToolCall, ToolCallId};
        // Tiny visible text, but a huge tool-call argument (e.g. a file write):
        // the estimate must reflect the args, which `text()` omits.
        let mut m = Message::assistant_tool_calls(
            Some("ok".into()),
            vec![ToolCall {
                id: ToolCallId::from("c1"),
                name: "FileWrite".into(),
                arguments: serde_json::json!({ "content": "x".repeat(8000) }),
            }],
        );
        let with_args = ContextManager::estimate_tokens(std::slice::from_ref(&m));
        m.tool_calls.clear();
        let text_only = ContextManager::estimate_tokens(std::slice::from_ref(&m));
        assert!(
            with_args > text_only + 1000,
            "tool args must be counted: {with_args} vs {text_only}"
        );
    }

    #[tokio::test]
    async fn compacts_on_measured_context_floor() {
        // Short messages (low char estimate) but the provider reported a large
        // prompt — compaction must still fire off the measured floor.
        let cm = ContextManager::new(100);
        let state = Arc::new(Mutex::new(SessionState::new(SessionId::from("s"), "m")));
        {
            let mut st = state.lock().await;
            for i in 0..12 {
                st.messages.push(Message::user(format!("m{i}")));
            }
            st.last_prompt_tokens = 95; // ≥ 80% of 100 → over budget
        }
        let (etx, _erx) = tokio::sync::mpsc::unbounded_channel();
        let events = EventEmitter::new(etx);
        let llm: Arc<dyn LlmClient> = Arc::new(SummaryLlm);
        let compacted = cm
            .maybe_compact(
                &llm,
                &state,
                &LlmOptions::default(),
                &events,
                &CancellationToken::new(),
            )
            .await;
        assert!(compacted, "should compact off the measured context floor");
    }

    #[tokio::test]
    async fn compacts_when_over_budget() {
        // Tiny context so a handful of messages exceed the budget.
        let cm = ContextManager::new(100);
        let state = Arc::new(Mutex::new(SessionState::new(SessionId::from("s"), "m")));
        {
            let mut st = state.lock().await;
            for i in 0..20 {
                st.messages
                    .push(Message::user("word ".repeat(20) + &i.to_string()));
            }
        }
        let (etx, mut erx) = tokio::sync::mpsc::unbounded_channel();
        let events = EventEmitter::new(etx);
        let llm: Arc<dyn LlmClient> = Arc::new(SummaryLlm);

        let compacted = cm
            .maybe_compact(
                &llm,
                &state,
                &LlmOptions::default(),
                &events,
                &CancellationToken::new(),
            )
            .await;
        assert!(compacted);

        let st = state.lock().await;
        // summary + kept tail (8) = 9
        assert_eq!(st.messages.len(), 9);
        assert!(st.messages[0].text().contains("SUMMARY"));
        // The meter-reset signal: a Compaction event carrying the smaller size,
        // and the measured floor refreshed to match (so it isn't pinned high).
        let ev = erx.try_recv().expect("compaction event");
        match ev {
            Event::Compaction { tokens_after, .. } => {
                assert!(tokens_after > 0, "post-compaction size reported");
                assert_eq!(tokens_after, st.last_prompt_tokens, "floor refreshed");
            }
            _ => panic!("expected Compaction event"),
        }
    }

    #[tokio::test]
    async fn no_compaction_under_budget() {
        let cm = ContextManager::new(131_072);
        let state = Arc::new(Mutex::new(SessionState::new(SessionId::from("s"), "m")));
        state.lock().await.messages.push(Message::user("short"));
        let (etx, _erx) = tokio::sync::mpsc::unbounded_channel();
        let events = EventEmitter::new(etx);
        let llm: Arc<dyn LlmClient> = Arc::new(SummaryLlm);
        let compacted = cm
            .maybe_compact(
                &llm,
                &state,
                &LlmOptions::default(),
                &events,
                &CancellationToken::new(),
            )
            .await;
        assert!(!compacted);
    }

    #[tokio::test]
    async fn force_compacts_even_under_budget() {
        // Huge budget: maybe_compact would be a no-op, but compact_now forces it.
        let cm = ContextManager::new(1_000_000);
        let state = Arc::new(Mutex::new(SessionState::new(SessionId::from("s"), "m")));
        {
            let mut st = state.lock().await;
            for i in 0..14 {
                st.messages.push(Message::user(format!("message {i}")));
            }
        }
        let (etx, _erx) = tokio::sync::mpsc::unbounded_channel();
        let events = EventEmitter::new(etx);
        let llm: Arc<dyn LlmClient> = Arc::new(SummaryLlm);

        let opts = LlmOptions::default();
        let ct = CancellationToken::new();
        assert!(!cm.maybe_compact(&llm, &state, &opts, &events, &ct).await);
        assert!(cm.compact_now(&llm, &state, &opts, &events, &ct).await);

        let st = state.lock().await;
        assert_eq!(st.messages.len(), 9); // summary + 8 kept
        assert!(st.messages[0].text().contains("SUMMARY"));
    }
}

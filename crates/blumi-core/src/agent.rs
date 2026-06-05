//! The streaming agent loop: the real [`TurnRunner`].
//!
//! Ported from OpenMono's `ConversationLoop`: stream the model, accumulate
//! text/reasoning/tool-call fragments, run tool calls (read-only +
//! concurrency-safe ones in parallel, the rest serially), append results, and
//! iterate until the model stops calling tools, the iteration budget runs out,
//! a doom loop is detected, or the turn is cancelled.

use crate::context::ContextManager;
use crate::llm::{LlmClient, LlmOptions};
use crate::permissions::PermissionEngine;
use crate::persona::Persona;
use crate::pipeline::execute_tool_call;
use crate::registry::ToolRegistry;
use crate::runner::{TurnContext, TurnRunner};
use crate::session::SessionState;
use crate::tool::{ChangeJournal, SubAgentSpawner, ToolContext};
use crate::Executor;
use async_trait::async_trait;
use blumi_protocol::{
    DoneReason, Event, Message, MessageId, Role, StreamChunk, ToolCall, ToolCallId, Usage,
};
use futures::future::join_all;
use futures::StreamExt;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Identical consecutive tool-call rounds before the runtime intervenes to
/// break the loop (the model re-issued the exact same call with the same args).
const DOOM_NUDGE_AT: usize = 2;
/// How many times we try to redirect a repeating agent before giving up.
const DOOM_MAX_NUDGES: u32 = 2;

/// The production turn runner. Construct one per session.
pub struct AgentTurnRunner {
    llm: Arc<dyn LlmClient>,
    registry: Arc<ToolRegistry>,
    perms: Arc<PermissionEngine>,
    executor: Arc<dyn Executor>,
    options: LlmOptions,
    max_iterations: u32,
    /// Auto-continue step budget surfaced to the actor (see `TurnRunner`).
    /// Atomic so `/autocontinue` can retune it mid-session.
    auto_continue: std::sync::atomic::AtomicU32,
    /// Token ceiling for one self-woken sequence (0 = no cap).
    auto_continue_tokens: u32,
    system_prompt: String,
    working_dir: PathBuf,
    context: ContextManager,
    spawner: Option<Arc<dyn SubAgentSpawner>>,
    journal: Arc<ChangeJournal>,
    /// Available top-level personas (empty = personas disabled).
    personas: Vec<Persona>,
    /// The currently-active persona (layered onto the system prompt).
    active: std::sync::Mutex<Persona>,
}

impl AgentTurnRunner {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmClient>,
        registry: Arc<ToolRegistry>,
        perms: Arc<PermissionEngine>,
        executor: Arc<dyn Executor>,
        options: LlmOptions,
        max_iterations: u32,
        context_size: u32,
        system_prompt: String,
        working_dir: PathBuf,
    ) -> Self {
        AgentTurnRunner {
            llm,
            registry,
            perms,
            executor,
            options,
            max_iterations,
            auto_continue: std::sync::atomic::AtomicU32::new(0),
            auto_continue_tokens: 0,
            system_prompt,
            working_dir,
            context: ContextManager::new(context_size),
            spawner: None,
            journal: Arc::new(ChangeJournal::new()),
            personas: Vec::new(),
            active: std::sync::Mutex::new(Persona::default()),
        }
    }

    /// Enable sub-agent delegation (the `delegate` tool's backend).
    pub fn with_spawner(mut self, spawner: Arc<dyn SubAgentSpawner>) -> Self {
        self.spawner = Some(spawner);
        self
    }

    /// Set how many times the runtime may auto-continue after the per-turn
    /// iteration cap (the actor reads this via `auto_continue_budget`).
    pub fn with_auto_continue(self, n: u32) -> Self {
        self.auto_continue
            .store(n, std::sync::atomic::Ordering::Relaxed);
        self
    }

    /// Set the token ceiling for one self-woken sequence (0 = no cap).
    pub fn with_auto_continue_tokens(mut self, n: u32) -> Self {
        self.auto_continue_tokens = n;
        self
    }

    /// Configure top-level personas and select the active one by name.
    pub fn with_personas(mut self, personas: Vec<Persona>, active: &str) -> Self {
        if let Some(p) = personas.iter().find(|p| p.name == active) {
            self.active = std::sync::Mutex::new(p.clone());
        }
        self.personas = personas;
        self
    }

    fn tool_ctx(&self, ctx: &TurnContext) -> ToolContext {
        ToolContext {
            session_id: ctx.session_id.clone(),
            working_dir: self.working_dir.clone(),
            executor: self.executor.clone(),
            events: ctx.events.clone(),
            interactor: ctx.interactor.clone(),
            spawner: self.spawner.clone(),
            journal: Some(self.journal.clone()),
        }
    }
}

#[async_trait]
impl TurnRunner for AgentTurnRunner {
    async fn run_turn(
        &self,
        state: Arc<Mutex<SessionState>>,
        ctx: TurnContext,
        ct: CancellationToken,
    ) -> DoneReason {
        let tool_specs = self.registry.specs();
        let tool_ctx = self.tool_ctx(&ctx);
        let mut recent_signatures: Vec<String> = Vec::new();
        let mut loop_nudges: u32 = 0;

        // Snapshot the active persona for this turn: it layers extra
        // instructions onto the system prompt and may override the temperature.
        let persona = self.active.lock().expect("persona poisoned").clone();
        let effective_prompt = match (
            self.system_prompt.is_empty(),
            persona.instructions.is_empty(),
        ) {
            (_, true) => self.system_prompt.clone(),
            (true, false) => persona.instructions.clone(),
            (false, false) => format!("{}\n\n{}", self.system_prompt, persona.instructions),
        };

        for _iteration in 0..self.max_iterations {
            if ct.is_cancelled() {
                return DoneReason::Cancelled;
            }

            // Compact the history if it has grown past the context budget.
            self.context
                .maybe_compact(&self.llm, &state, &self.options, &ctx.events, &ct)
                .await;

            // Build the context window: system prompt + conversation so far.
            let (mut window, current_model) = {
                let st = state.lock().await;
                let mut msgs = Vec::with_capacity(st.messages.len() + 1);
                if !effective_prompt.is_empty() {
                    msgs.push(Message::system(effective_prompt.clone()));
                }
                msgs.extend(st.messages.iter().cloned());
                (msgs, st.model.clone())
            };
            // Defensive: never send a tool_result whose tool_use isn't present
            // earlier in the window — that's a guaranteed 400 on every provider.
            strip_orphan_tool_results(&mut window);

            // Honor mid-session model switches (Command::SetModel) within the
            // active provider/client; the persona may override the temperature.
            let mut options = self.options.clone();
            if !current_model.is_empty() {
                options.model = current_model;
            }
            if let Some(t) = persona.temperature {
                options.temperature = t;
            }

            // Stream the model.
            let mut stream = match self
                .llm
                .stream_chat(&window, &tool_specs, &options, ct.child_token())
                .await
            {
                Ok(s) => s,
                Err(crate::LlmError::Cancelled) => return DoneReason::Cancelled,
                Err(e) => {
                    emit_error(&ctx, &e.to_string());
                    return DoneReason::Error;
                }
            };

            let msg_id = MessageId::new();
            ctx.events.emit(Event::AssistantStarted {
                message_id: msg_id.clone(),
            });

            let mut text = String::new();
            let mut accum: BTreeMap<u32, ToolAccum> = BTreeMap::new();
            let mut usage = Usage::default();
            let mut finish = blumi_protocol::FinishReason::Stop;

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(StreamChunk::Thinking { text }) => {
                        ctx.events.emit(Event::Thinking { text });
                    }
                    Ok(StreamChunk::Text { text: t }) => {
                        text.push_str(&t);
                        ctx.events.emit(Event::Token { text: t });
                    }
                    Ok(StreamChunk::ToolCall(delta)) => {
                        let entry = accum.entry(delta.index).or_default();
                        if let Some(id) = delta.id {
                            entry.id = Some(id);
                        }
                        if let Some(name) = delta.name {
                            entry.name = Some(name);
                        }
                        if let Some(frag) = delta.arguments_fragment {
                            entry.args.push_str(&frag);
                        }
                    }
                    Ok(StreamChunk::Usage(u)) => add_usage(&mut usage, &u),
                    Ok(StreamChunk::Done { reason }) => finish = reason,
                    Err(crate::LlmError::Cancelled) => return DoneReason::Cancelled,
                    Err(e) => {
                        emit_error(&ctx, &e.to_string());
                        return DoneReason::Error;
                    }
                }
            }

            ctx.events.emit(Event::AssistantFinished {
                message_id: msg_id,
                finish,
            });
            {
                let mut st = state.lock().await;
                if usage.total() > 0 {
                    st.record_usage(&usage);
                    // Context = the full prompt size: uncached input + cache read +
                    // cache write. With prompt caching, `input_tokens` alone omits
                    // the cached bulk, so the meter would read near-zero.
                    let context =
                        usage.input_tokens + usage.cache_read_tokens + usage.cache_write_tokens;
                    ctx.events.emit(Event::Usage {
                        input: usage.input_tokens,
                        output: usage.output_tokens,
                        total: usage.total(),
                        context,
                        cost_usd: None,
                    });
                } else {
                    // The provider reported no usage (many OpenAI-compatible / local
                    // servers don't). Fall back to a local ~4-chars/token estimate so
                    // the context meter and token counts still work, never stuck at 0.
                    let prompt = ContextManager::estimate_tokens(&window) as u32;
                    let resp_chars: usize = text.len()
                        + accum
                            .values()
                            .map(|a| a.args.len() + a.name.as_deref().map_or(0, str::len))
                            .sum::<usize>();
                    let output = (resp_chars / 4).max(1) as u32;
                    st.record_usage(&Usage {
                        input_tokens: prompt,
                        output_tokens: output,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                    });
                    ctx.events.emit(Event::Usage {
                        input: prompt,
                        output,
                        total: prompt + output,
                        context: prompt,
                        cost_usd: None,
                    });
                }
            }

            let tool_calls = finalize_tool_calls(accum);

            // No tools → record the assistant text and finish the turn.
            if tool_calls.is_empty() {
                if !text.is_empty() {
                    state.lock().await.messages.push(Message::assistant(text));
                }
                return DoneReason::Completed;
            }

            // Doom-loop guard. If the model re-issues the SAME tool call(s) with
            // identical arguments, running them again only reproduces the same
            // result — so instead of executing (or just aborting), we break the
            // loop: drop the pointless repeat, keep any reasoning, and inject an
            // escalating nudge so the agent changes course. Most loops recover
            // here; we only give up if the nudges don't take.
            let signature = signature_of(&tool_calls);
            recent_signatures.push(signature);
            if trailing_repeats(&recent_signatures) >= DOOM_NUDGE_AT {
                loop_nudges += 1;
                if !text.is_empty() {
                    state.lock().await.messages.push(Message::assistant(text));
                }
                if loop_nudges > DOOM_MAX_NUDGES {
                    ctx.events.emit(Event::Notice {
                        message: "broke a repeating tool loop — the agent kept re-issuing \
                                  the same call after being redirected; ending this turn"
                            .into(),
                    });
                    return DoneReason::DoomLoop;
                }
                let nudge = if loop_nudges == 1 {
                    "You just requested the exact same tool call(s) with identical arguments \
                     as the previous step, and that result is already in the conversation \
                     above. Repeating it will not produce anything new. Use the result you \
                     already have, or take a DIFFERENT action — different arguments, a \
                     different tool, or a new approach. Do not repeat the same call."
                } else {
                    "You are still repeating the same tool call. Stop calling tools and reply \
                     now with your best answer using what you already know; if you are \
                     genuinely blocked, state precisely what is blocking you and why."
                };
                state
                    .lock()
                    .await
                    .messages
                    .push(Message::user(nudge.to_string()));
                continue;
            }
            // A productive (non-repeating) step — reset the redirect counter.
            loop_nudges = 0;

            // Record the assistant message (with its tool calls) before results.
            state
                .lock()
                .await
                .messages
                .push(Message::assistant_tool_calls(
                    (!text.is_empty()).then_some(text),
                    tool_calls.clone(),
                ));

            // Execute: read-only + concurrency-safe in parallel, the rest serial.
            let results = self.execute_calls(&tool_calls, &tool_ctx, &ct).await;

            // Append tool results in call order.
            {
                let mut st = state.lock().await;
                for call in &tool_calls {
                    if let Some(result) = results.get(call.id.as_str()) {
                        st.messages.push(Message::tool_result(
                            call.id.clone(),
                            call.name.clone(),
                            result.model_preview.clone(),
                        ));
                    }
                }
            }

            if ct.is_cancelled() {
                return DoneReason::Cancelled;
            }
        }

        // When auto-continue is enabled the actor self-wakes and narrates it, so
        // a turn-level error here would be misleading. Only surface the error
        // when auto-continue is off (then the turn really does stop).
        if self
            .auto_continue
            .load(std::sync::atomic::Ordering::Relaxed)
            == 0
        {
            emit_error(
                &ctx,
                "reached the maximum number of tool iterations for this turn",
            );
        }
        DoneReason::MaxIterations
    }

    fn set_yolo(&self, on: bool) {
        self.perms.set_yolo(on);
    }

    fn yolo(&self) -> bool {
        self.perms.is_yolo()
    }

    fn set_brain_mode(&self, mode: crate::brain::BrainMode) {
        self.perms.set_brain_mode(mode);
    }

    fn brain_mode(&self) -> crate::brain::BrainMode {
        self.perms.brain_mode()
    }

    fn set_plan_mode(&self, on: bool) {
        self.perms.set_plan_mode(on);
    }

    fn plan_mode(&self) -> bool {
        self.perms.is_plan_mode()
    }

    fn auto_continue_budget(&self) -> u32 {
        self.auto_continue
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_auto_continue(&self, n: u32) {
        self.auto_continue
            .store(n, std::sync::atomic::Ordering::Relaxed);
    }

    fn auto_continue_token_budget(&self) -> u32 {
        self.auto_continue_tokens
    }

    async fn compact(
        &self,
        state: Arc<Mutex<SessionState>>,
        events: &crate::emit::EventEmitter,
        ct: CancellationToken,
    ) -> bool {
        self.context
            .compact_now(&self.llm, &state, &self.options, events, &ct)
            .await
    }

    async fn undo(&self) -> Option<String> {
        let change = self.journal.pop()?;
        let display = change.path.display().to_string();
        let outcome = match &change.before {
            Some(bytes) => self.executor.write_file(&change.path, bytes).await,
            None => self.executor.remove_file(&change.path).await,
        };
        Some(match outcome {
            Ok(()) if change.before.is_some() => format!("undid {} to {display}", change.op),
            Ok(()) => format!(
                "undid {} — removed {display} (was newly created)",
                change.op
            ),
            Err(e) => format!("undo failed for {display}: {e}"),
        })
    }

    fn set_persona(&self, name: &str) -> Option<Persona> {
        let found = self.personas.iter().find(|p| p.name == name).cloned()?;
        *self.active.lock().expect("persona poisoned") = found.clone();
        Some(found)
    }
}

impl AgentTurnRunner {
    /// Run all tool calls, parallelising the read-only concurrency-safe ones.
    /// Returns a map of call-id → result.
    async fn execute_calls(
        &self,
        calls: &[ToolCall],
        tool_ctx: &ToolContext,
        ct: &CancellationToken,
    ) -> std::collections::HashMap<String, blumi_protocol::ToolResult> {
        let mut parallel = Vec::new();
        let mut serial = Vec::new();
        for call in calls {
            let safe = self
                .registry
                .get(&call.name)
                .map(|t| t.parallelizable())
                .unwrap_or(false);
            if safe {
                parallel.push(call);
            } else {
                serial.push(call);
            }
        }

        let mut out = std::collections::HashMap::new();

        let parallel_results = join_all(parallel.iter().map(|call| {
            execute_tool_call(
                &self.registry,
                &self.perms,
                tool_ctx,
                call,
                ct.child_token(),
            )
        }))
        .await;
        for (call, result) in parallel.iter().zip(parallel_results) {
            out.insert(call.id.0.clone(), result);
        }

        for call in serial {
            let result = execute_tool_call(
                &self.registry,
                &self.perms,
                tool_ctx,
                call,
                ct.child_token(),
            )
            .await;
            out.insert(call.id.0.clone(), result);
        }

        out
    }
}

#[derive(Default)]
struct ToolAccum {
    id: Option<String>,
    name: Option<String>,
    args: String,
}

fn finalize_tool_calls(accum: BTreeMap<u32, ToolAccum>) -> Vec<ToolCall> {
    accum
        .into_values()
        .filter_map(|a| {
            let name = a.name?;
            let arguments = if a.args.trim().is_empty() {
                serde_json::json!({})
            } else {
                parse_tool_args(&a.args).unwrap_or(serde_json::json!({}))
            };
            let id = a.id.map(ToolCallId::from).unwrap_or_default();
            Some(ToolCall {
                id,
                name,
                arguments,
            })
        })
        .collect()
}

/// Parse model-emitted tool-call arguments into JSON, tolerating the malformed
/// output some models produce for large string values. The classic offender is
/// a multi-line `content` (e.g. a plan written via `FileWrite`): models — and
/// many OpenAI-compatible / local gateways especially — emit *raw*, unescaped
/// control characters (newlines, tabs) inside the JSON string, which strict JSON
/// forbids. A small `Bash` call rarely trips this; a big plan almost always
/// does. Previously a failed parse silently became `{}`, which then surfaced as
/// a misleading "missing field `path`". We first try a strict parse, then retry
/// after escaping stray control characters inside strings. Returns `None` only
/// when the JSON is unrecoverable (e.g. truncated mid-stream).
fn parse_tool_args(raw: &str) -> Option<serde_json::Value> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        return Some(v);
    }
    let repaired = escape_control_chars_in_strings(raw);
    serde_json::from_str::<serde_json::Value>(&repaired).ok()
}

/// Escape raw control characters (U+0000–U+001F) that appear *inside* JSON
/// string literals — `serde_json` rejects them in strict mode. A small state
/// machine tracks string/escape context so whitespace *between* tokens (which
/// JSON permits) is left untouched and only characters within quotes are fixed.
fn escape_control_chars_in_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut in_string = false;
    let mut escaped = false;
    for c in s.chars() {
        if !in_string {
            if c == '"' {
                in_string = true;
            }
            out.push(c);
            continue;
        }
        if escaped {
            // Previous char was a backslash; emit this verbatim (valid escape).
            out.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => {
                out.push(c);
                escaped = true;
            }
            '"' => {
                out.push(c);
                in_string = false;
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

/// Drop any tool-result message whose `tool_use` isn't present earlier in the
/// window. A `tool_result` with no matching `tool_use` is a hard 400 on every
/// provider; aggressive history surgery (compaction, edits) can leave such
/// orphans, so we scrub them right before sending — a last line of defense.
fn strip_orphan_tool_results(window: &mut Vec<Message>) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    window.retain(|m| {
        for tc in &m.tool_calls {
            seen.insert(tc.id.as_str().to_string());
        }
        if m.role == Role::Tool {
            return m
                .tool_call_id
                .as_ref()
                .is_some_and(|id| seen.contains(id.as_str()));
        }
        true
    });
}

fn signature_of(calls: &[ToolCall]) -> String {
    let mut parts: Vec<String> = calls
        .iter()
        .map(|c| format!("{}:{}", c.name, c.arguments))
        .collect();
    parts.sort();
    parts.join("|")
}

/// How many times the most recent signature repeats consecutively at the tail
/// (1 = it just appeared; 2 = the model issued the identical call twice in a row).
fn trailing_repeats(signatures: &[String]) -> usize {
    match signatures.last() {
        Some(last) => signatures.iter().rev().take_while(|s| *s == last).count(),
        None => 0,
    }
}

fn add_usage(total: &mut Usage, u: &Usage) {
    total.input_tokens += u.input_tokens;
    total.output_tokens += u.output_tokens;
    total.cache_read_tokens += u.cache_read_tokens;
    total.cache_write_tokens += u.cache_write_tokens;
}

fn emit_error(ctx: &TurnContext, message: &str) {
    ctx.events.emit(Event::Error {
        kind: "turn_error".into(),
        message: message.to_string(),
        hint: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::spawn_session;
    use crate::exec::{DirEntry, ExecOutput, ExecRequest};
    use crate::llm::{ProviderCaps, ToolSpec};
    use crate::tool::Tool;
    use crate::{ExecError, LlmError, PermissionEngine, ToolRegistry};
    use blumi_config::PermissionConfig;
    use blumi_protocol::{Command, Envelope, FinishReason, SessionId, ToolCallDelta, ToolResult};
    use futures::stream::BoxStream;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::broadcast;

    /// LLM that returns each queued script in order.
    struct ScriptedLlm {
        scripts: std::sync::Mutex<std::collections::VecDeque<Vec<StreamChunk>>>,
    }
    #[async_trait]
    impl LlmClient for ScriptedLlm {
        async fn stream_chat(
            &self,
            _m: &[Message],
            _t: &[ToolSpec],
            _o: &LlmOptions,
            _ct: CancellationToken,
        ) -> Result<BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError> {
            let script = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
            Ok(Box::pin(futures::stream::iter(script.into_iter().map(Ok))))
        }
        fn caps(&self) -> ProviderCaps {
            ProviderCaps::default()
        }
    }

    /// A tool that flips a shared flag when called.
    struct FlagTool(Arc<AtomicBool>);
    #[async_trait]
    impl Tool for FlagTool {
        fn name(&self) -> &str {
            "Flag"
        }
        fn description(&self) -> &str {
            "sets a flag"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object" })
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &ToolContext,
            _ct: CancellationToken,
        ) -> Result<ToolResult, crate::ToolError> {
            self.0.store(true, Ordering::SeqCst);
            Ok(ToolResult::success("flag set"))
        }
    }

    struct NoopExec;
    #[async_trait]
    impl Executor for NoopExec {
        async fn exec(
            &self,
            _r: ExecRequest,
            _ct: CancellationToken,
        ) -> Result<ExecOutput, ExecError> {
            Err(ExecError::Unavailable("noop".into()))
        }
        async fn read_file(&self, _p: &Path) -> Result<Vec<u8>, ExecError> {
            Err(ExecError::Unavailable("noop".into()))
        }
        async fn write_file(&self, _p: &Path, _c: &[u8]) -> Result<(), ExecError> {
            Ok(())
        }
        async fn exists(&self, _p: &Path) -> Result<bool, ExecError> {
            Ok(false)
        }
        async fn list_dir(&self, _p: &Path) -> Result<Vec<DirEntry>, ExecError> {
            Ok(vec![])
        }
        fn working_dir(&self) -> &Path {
            Path::new(".")
        }
    }

    async fn drain_until_done(rx: &mut broadcast::Receiver<Envelope>) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let env = rx.recv().await.unwrap();
            let done = matches!(env.event, Event::TurnDone { .. });
            events.push(env.event);
            if done {
                return events;
            }
        }
    }

    fn tool_call_chunk(id: &str, name: &str, args: &str) -> StreamChunk {
        StreamChunk::ToolCall(ToolCallDelta {
            index: 0,
            id: Some(id.into()),
            name: Some(name.into()),
            arguments_fragment: Some(args.into()),
        })
    }

    #[tokio::test]
    async fn full_turn_calls_tool_then_finishes() {
        let flag = Arc::new(AtomicBool::new(false));

        let llm = Arc::new(ScriptedLlm {
            scripts: std::sync::Mutex::new(
                vec![
                    // iteration 1: call the Flag tool
                    vec![
                        tool_call_chunk("c1", "Flag", "{}"),
                        StreamChunk::Done {
                            reason: FinishReason::ToolCalls,
                        },
                    ],
                    // iteration 2: final answer
                    vec![
                        StreamChunk::Text {
                            text: "all done".into(),
                        },
                        StreamChunk::Done {
                            reason: FinishReason::Stop,
                        },
                    ],
                ]
                .into(),
            ),
        });

        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(FlagTool(flag.clone())));
        let perms = Arc::new(PermissionEngine::new(PermissionConfig {
            yolo: true,
            ..Default::default()
        }));

        let runner = Arc::new(AgentTurnRunner::new(
            llm,
            Arc::new(reg),
            perms,
            Arc::new(NoopExec),
            LlmOptions::default(),
            10,
            131_072,
            "you are blumi".into(),
            PathBuf::from("."),
        ));

        let h = spawn_session(SessionId::from("s"), "m", runner);
        let mut rx = h.subscribe();
        h.send(Command::UserMessage {
            text: "do it".into(),
            attachments: vec![],
            stream_id: None,
        })
        .await
        .unwrap();

        let events = drain_until_done(&mut rx).await;

        assert!(flag.load(Ordering::SeqCst), "tool should have run");
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::ToolStart { name, .. } if name == "Flag")));
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::ToolResult { ok: true, .. })));
        assert!(matches!(
            events.last().unwrap(),
            Event::TurnDone {
                reason: DoneReason::Completed
            }
        ));

        let snap = h.snapshot().await;
        // user, assistant(tool_calls), tool result, assistant("all done")
        assert_eq!(snap.messages.len(), 4);
        assert_eq!(snap.messages.last().unwrap().text(), "all done");
    }

    #[tokio::test]
    async fn doom_loop_is_broken_then_recovers() {
        let flag = Arc::new(AtomicBool::new(false));
        let llm = Arc::new(ScriptedLlm {
            scripts: std::sync::Mutex::new(
                vec![
                    // iter 1: call Flag
                    vec![
                        tool_call_chunk("c1", "Flag", "{}"),
                        StreamChunk::Done {
                            reason: FinishReason::ToolCalls,
                        },
                    ],
                    // iter 2: the SAME call again → runtime should break the loop,
                    // not execute it again, and inject a redirect nudge.
                    vec![
                        tool_call_chunk("c2", "Flag", "{}"),
                        StreamChunk::Done {
                            reason: FinishReason::ToolCalls,
                        },
                    ],
                    // iter 3: the model takes the nudge and finishes.
                    vec![
                        StreamChunk::Text {
                            text: "recovered".into(),
                        },
                        StreamChunk::Done {
                            reason: FinishReason::Stop,
                        },
                    ],
                ]
                .into(),
            ),
        });

        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(FlagTool(flag.clone())));
        let perms = Arc::new(PermissionEngine::new(PermissionConfig {
            yolo: true,
            ..Default::default()
        }));
        let runner = Arc::new(AgentTurnRunner::new(
            llm,
            Arc::new(reg),
            perms,
            Arc::new(NoopExec),
            LlmOptions::default(),
            10,
            131_072,
            "you are blumi".into(),
            PathBuf::from("."),
        ));

        let h = spawn_session(SessionId::from("s"), "m", runner);
        let mut rx = h.subscribe();
        h.send(Command::UserMessage {
            text: "go".into(),
            attachments: vec![],
            stream_id: None,
        })
        .await
        .unwrap();

        let events = drain_until_done(&mut rx).await;

        // The loop recovered: it completed instead of aborting with DoomLoop.
        assert!(matches!(
            events.last().unwrap(),
            Event::TurnDone {
                reason: DoneReason::Completed
            }
        ));
        // The repeated call was NOT executed a second time (Flag ran once).
        let runs = events
            .iter()
            .filter(|e| matches!(e, Event::ToolStart { name, .. } if name == "Flag"))
            .count();
        assert_eq!(runs, 1, "the repeat must be dropped, not re-run");

        let snap = h.snapshot().await;
        assert_eq!(snap.messages.last().unwrap().text(), "recovered");
        // A redirect nudge was injected to break the loop.
        assert!(
            snap.messages
                .iter()
                .any(|m| m.text().contains("same tool call")),
            "a redirect nudge should have been injected"
        );
    }

    #[test]
    fn strips_orphan_tool_results() {
        let orphan = Message::tool_result(ToolCallId::from("gone"), "Bash", "x");
        let asst = Message::assistant_tool_calls(
            None,
            vec![ToolCall {
                id: ToolCallId::from("ok"),
                name: "Bash".into(),
                arguments: serde_json::json!({}),
            }],
        );
        let answered = Message::tool_result(ToolCallId::from("ok"), "Bash", "y");
        let mut w = vec![Message::user("hi"), orphan, asst, answered];
        strip_orphan_tool_results(&mut w);
        // The orphan (no matching tool_use) is dropped; the valid pair survives.
        assert_eq!(w.len(), 3);
        let tool_ids: Vec<_> = w
            .iter()
            .filter(|m| m.role == Role::Tool)
            .map(|m| m.tool_call_id.as_ref().unwrap().as_str().to_string())
            .collect();
        assert_eq!(tool_ids, vec!["ok".to_string()]);
    }

    #[test]
    fn trailing_repeats_counts_consecutive_tail() {
        let a = "Flag:{}".to_string();
        let b = "Other:{}".to_string();
        assert_eq!(trailing_repeats(&[]), 0);
        assert_eq!(trailing_repeats(std::slice::from_ref(&a)), 1);
        assert_eq!(trailing_repeats(&[a.clone(), a.clone()]), 2);
        // A different call in between resets the tail run.
        assert_eq!(trailing_repeats(&[a.clone(), a.clone(), b.clone()]), 1);
        assert_eq!(trailing_repeats(&[b.clone(), a.clone(), a.clone()]), 2);
        // The nudge trigger fires at 2 identical in a row.
        assert!(trailing_repeats(&[a.clone(), a.clone()]) >= DOOM_NUDGE_AT);
    }

    #[test]
    fn finalize_parses_fragmented_args() {
        let mut accum = BTreeMap::new();
        accum.insert(
            0u32,
            ToolAccum {
                id: Some("c1".into()),
                name: Some("Bash".into()),
                args: "{\"command\":\"ls\"}".into(),
            },
        );
        let calls = finalize_tool_calls(accum);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "Bash");
        assert_eq!(calls[0].arguments["command"], "ls");
    }

    #[test]
    fn finalize_repairs_raw_newlines_in_string_args() {
        // A FileWrite for a plan: the model put RAW newlines inside the JSON
        // `content` string (invalid strict JSON). Before the repair this parsed
        // to `{}` and the tool failed with a misleading "missing field `path`".
        let mut accum = BTreeMap::new();
        accum.insert(
            0u32,
            ToolAccum {
                id: Some("w1".into()),
                name: Some("FileWrite".into()),
                args: "{\"path\":\"docs/plan.md\",\"content\":\"# Plan\nline one\n\tindented\"}"
                    .into(),
            },
        );
        let calls = finalize_tool_calls(accum);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments["path"], "docs/plan.md");
        assert_eq!(
            calls[0].arguments["content"], "# Plan\nline one\n\tindented",
            "raw control chars inside the string should be preserved as real newlines/tabs"
        );
    }

    #[test]
    fn parse_tool_args_handles_strict_and_malformed() {
        // Strict-valid JSON is returned as-is.
        let ok = parse_tool_args("{\"path\":\"a\",\"content\":\"x\"}").unwrap();
        assert_eq!(ok["path"], "a");

        // Raw newline inside a string is repaired (not collapsed to {}).
        let fixed = parse_tool_args("{\"content\":\"a\nb\"}").unwrap();
        assert_eq!(fixed["content"], "a\nb");

        // Whitespace BETWEEN tokens (legal JSON) must survive untouched.
        let spaced = parse_tool_args("{\n  \"k\": \"v\"\n}").unwrap();
        assert_eq!(spaced["k"], "v");

        // A backslash-escaped sequence already in the string is left intact.
        let esc = parse_tool_args("{\"content\":\"line\\nkept\"}").unwrap();
        assert_eq!(esc["content"], "line\nkept");

        // Genuinely truncated JSON stays unrecoverable.
        assert!(parse_tool_args("{\"path\":\"a\",\"content\":\"oops").is_none());
    }
}

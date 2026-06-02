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
use crate::pipeline::execute_tool_call;
use crate::registry::ToolRegistry;
use crate::runner::{TurnContext, TurnRunner};
use crate::session::SessionState;
use crate::tool::ToolContext;
use crate::Executor;
use async_trait::async_trait;
use futures::future::join_all;
use futures::StreamExt;
use lumi_protocol::{
    DoneReason, Event, Message, MessageId, StreamChunk, ToolCall, ToolCallId, Usage,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// How many identical consecutive tool-call rounds count as a doom loop.
const DOOM_REPEATS: usize = 3;

/// The production turn runner. Construct one per session.
pub struct AgentTurnRunner {
    llm: Arc<dyn LlmClient>,
    registry: Arc<ToolRegistry>,
    perms: Arc<PermissionEngine>,
    executor: Arc<dyn Executor>,
    options: LlmOptions,
    max_iterations: u32,
    system_prompt: String,
    working_dir: PathBuf,
    context: ContextManager,
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
            system_prompt,
            working_dir,
            context: ContextManager::new(context_size),
        }
    }

    fn tool_ctx(&self, ctx: &TurnContext) -> ToolContext {
        ToolContext {
            session_id: ctx.session_id.clone(),
            working_dir: self.working_dir.clone(),
            executor: self.executor.clone(),
            events: ctx.events.clone(),
            interactor: ctx.interactor.clone(),
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

        for _iteration in 0..self.max_iterations {
            if ct.is_cancelled() {
                return DoneReason::Cancelled;
            }

            // Compact the history if it has grown past the context budget.
            self.context
                .maybe_compact(&self.llm, &state, &self.options, &ctx.events, &ct)
                .await;

            // Build the context window: system prompt + conversation so far.
            let window = {
                let st = state.lock().await;
                let mut msgs = Vec::with_capacity(st.messages.len() + 1);
                if !self.system_prompt.is_empty() {
                    msgs.push(Message::system(self.system_prompt.clone()));
                }
                msgs.extend(st.messages.iter().cloned());
                msgs
            };

            // Stream the model.
            let mut stream = match self
                .llm
                .stream_chat(&window, &tool_specs, &self.options, ct.child_token())
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
            ctx.events.emit(Event::AssistantStarted { message_id: msg_id.clone() });

            let mut text = String::new();
            let mut accum: BTreeMap<u32, ToolAccum> = BTreeMap::new();
            let mut usage = Usage::default();
            let mut finish = lumi_protocol::FinishReason::Stop;

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

            ctx.events.emit(Event::AssistantFinished { message_id: msg_id, finish });
            if usage.total() > 0 {
                let mut st = state.lock().await;
                st.record_usage(&usage);
                ctx.events.emit(Event::Usage {
                    input: usage.input_tokens,
                    output: usage.output_tokens,
                    total: usage.total(),
                    cost_usd: None,
                });
            }

            let tool_calls = finalize_tool_calls(accum);

            // No tools → record the assistant text and finish the turn.
            if tool_calls.is_empty() {
                if !text.is_empty() {
                    state.lock().await.messages.push(Message::assistant(text));
                }
                return DoneReason::Completed;
            }

            // Doom-loop guard.
            let signature = signature_of(&tool_calls);
            recent_signatures.push(signature);
            if is_doom_loop(&recent_signatures) {
                emit_error(&ctx, "doom loop: the agent repeated the same tool calls");
                return DoneReason::DoomLoop;
            }

            // Record the assistant message (with its tool calls) before results.
            state.lock().await.messages.push(Message::assistant_tool_calls(
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

        emit_error(&ctx, "reached the maximum number of tool iterations for this turn");
        DoneReason::MaxIterations
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
    ) -> std::collections::HashMap<String, lumi_protocol::ToolResult> {
        let mut parallel = Vec::new();
        let mut serial = Vec::new();
        for call in calls {
            let safe = self
                .registry
                .get(&call.name)
                .map(|t| t.is_read_only() && t.is_concurrency_safe())
                .unwrap_or(false);
            if safe {
                parallel.push(call);
            } else {
                serial.push(call);
            }
        }

        let mut out = std::collections::HashMap::new();

        let parallel_results = join_all(parallel.iter().map(|call| {
            execute_tool_call(&self.registry, &self.perms, tool_ctx, call, ct.child_token())
        }))
        .await;
        for (call, result) in parallel.iter().zip(parallel_results) {
            out.insert(call.id.0.clone(), result);
        }

        for call in serial {
            let result =
                execute_tool_call(&self.registry, &self.perms, tool_ctx, call, ct.child_token())
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
                serde_json::from_str(&a.args).unwrap_or(serde_json::json!({}))
            };
            let id = a.id.map(ToolCallId::from).unwrap_or_default();
            Some(ToolCall { id, name, arguments })
        })
        .collect()
}

fn signature_of(calls: &[ToolCall]) -> String {
    let mut parts: Vec<String> =
        calls.iter().map(|c| format!("{}:{}", c.name, c.arguments)).collect();
    parts.sort();
    parts.join("|")
}

fn is_doom_loop(signatures: &[String]) -> bool {
    if signatures.len() < DOOM_REPEATS {
        return false;
    }
    let last = &signatures[signatures.len() - 1];
    signatures.iter().rev().take(DOOM_REPEATS).all(|s| s == last)
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
    use futures::stream::BoxStream;
    use lumi_config::PermissionConfig;
    use lumi_protocol::{Command, Envelope, FinishReason, SessionId, ToolCallDelta, ToolResult};
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
        async fn exec(&self, _r: ExecRequest, _ct: CancellationToken) -> Result<ExecOutput, ExecError> {
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
                        StreamChunk::Done { reason: FinishReason::ToolCalls },
                    ],
                    // iteration 2: final answer
                    vec![
                        StreamChunk::Text { text: "all done".into() },
                        StreamChunk::Done { reason: FinishReason::Stop },
                    ],
                ]
                .into(),
            ),
        });

        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(FlagTool(flag.clone())));
        let perms = Arc::new(PermissionEngine::new(PermissionConfig { yolo: true, ..Default::default() }));

        let runner = Arc::new(AgentTurnRunner::new(
            llm,
            Arc::new(reg),
            perms,
            Arc::new(NoopExec),
            LlmOptions::default(),
            10,
            131_072,
            "you are lumi".into(),
            PathBuf::from("."),
        ));

        let h = spawn_session(SessionId::from("s"), "m", runner);
        let mut rx = h.subscribe();
        h.send(Command::UserMessage { text: "do it".into(), attachments: vec![], stream_id: None })
            .await
            .unwrap();

        let events = drain_until_done(&mut rx).await;

        assert!(flag.load(Ordering::SeqCst), "tool should have run");
        assert!(events.iter().any(|e| matches!(e, Event::ToolStart { name, .. } if name == "Flag")));
        assert!(events.iter().any(|e| matches!(e, Event::ToolResult { ok: true, .. })));
        assert!(matches!(events.last().unwrap(), Event::TurnDone { reason: DoneReason::Completed }));

        let snap = h.snapshot().await;
        // user, assistant(tool_calls), tool result, assistant("all done")
        assert_eq!(snap.messages.len(), 4);
        assert_eq!(snap.messages.last().unwrap().text(), "all done");
    }

    #[test]
    fn doom_loop_detects_repeats() {
        let sig = "Flag:{}".to_string();
        assert!(!is_doom_loop(std::slice::from_ref(&sig)));
        assert!(!is_doom_loop(&[sig.clone(), sig.clone()]));
        assert!(is_doom_loop(&[sig.clone(), sig.clone(), sig.clone()]));
    }

    #[test]
    fn finalize_parses_fragmented_args() {
        let mut accum = BTreeMap::new();
        accum.insert(
            0u32,
            ToolAccum { id: Some("c1".into()), name: Some("Bash".into()), args: "{\"command\":\"ls\"}".into() },
        );
        let calls = finalize_tool_calls(accum);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "Bash");
        assert_eq!(calls[0].arguments["command"], "ls");
    }
}

//! Cost-aware model routing: pick a difficulty *tier* per turn and route to a
//! light vs flagship model, so simple work doesn't burn the flagship's price.
//!
//! Mechanism (hybrid by default): a fast, zero-cost **heuristic** classifies the
//! turn from cheap signals (prompt length, tool count, iteration depth, keyword
//! hints); only when the heuristic is *ambiguous* does a small local **judge**
//! model get consulted. The judge fails safe to the light tier, so an unreachable
//! judge never blocks a turn and never silently upgrades to the expensive model.
//!
//! Everything composes with the existing brain/heal hooks and is a no-op unless a
//! [`Router`] is attached and its mode is not `Off`. Tier decisions are recomputed
//! (never replayed) on resume, and stats fold real provider `Usage`, so there is
//! no double counting. Sub-agents are routed by a fixed tier via [`Router::client_for`].

use crate::llm::{LlmClient, LlmOptions};
use blumi_config::{pricing, HeuristicConfig};
use blumi_protocol::{Message, StreamChunk, Usage};
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio_util::sync::CancellationToken;

/// How routing decides a turn's tier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RouterMode {
    /// No routing — the turn runs on the active model (today's behaviour).
    #[default]
    Off,
    /// Heuristic only; ambiguous turns default to the light tier.
    Heuristic,
    /// Heuristic first; consult the judge model only on ambiguous turns.
    Hybrid,
    /// Always consult the judge model.
    Judge,
}

impl RouterMode {
    /// Parse a mode name (config + the `/route` command).
    pub fn parse(s: &str) -> Option<RouterMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" | "" => Some(RouterMode::Off),
            "heuristic" | "rules" | "fast" => Some(RouterMode::Heuristic),
            "hybrid" | "on" | "auto" => Some(RouterMode::Hybrid),
            "judge" | "model" | "always" => Some(RouterMode::Judge),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            RouterMode::Off => "off",
            RouterMode::Heuristic => "heuristic",
            RouterMode::Hybrid => "hybrid",
            RouterMode::Judge => "judge",
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            RouterMode::Off => 0,
            RouterMode::Heuristic => 1,
            RouterMode::Hybrid => 2,
            RouterMode::Judge => 3,
        }
    }

    fn from_u8(v: u8) -> RouterMode {
        match v {
            1 => RouterMode::Heuristic,
            2 => RouterMode::Hybrid,
            3 => RouterMode::Judge,
            _ => RouterMode::Off,
        }
    }
}

/// A difficulty tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Light,
    Heavy,
}

impl Tier {
    pub fn label(&self) -> &'static str {
        match self {
            Tier::Light => "light",
            Tier::Heavy => "heavy",
        }
    }

    /// Parse the sub-agent default tier from config ("light"/"heavy"/"inherit").
    /// `inherit` (or anything unrecognized) returns `None` = no override.
    pub fn parse_subagent(s: &str) -> Option<Tier> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" | "cheap" | "small" => Some(Tier::Light),
            "heavy" | "flagship" | "big" => Some(Tier::Heavy),
            _ => None,
        }
    }
}

/// Cheap signals available before the model call.
pub struct TurnSignals<'a> {
    pub prompt: &'a str,
    pub tool_count: usize,
    pub iteration: u32,
    pub in_subagent: bool,
}

/// The fast heuristic. Returns `Some(tier)` on a confident verdict, `None` when
/// ambiguous, plus a short static reason for the UI/trace.
pub fn classify(sig: &TurnSignals, h: &HeuristicConfig) -> (Option<Tier>, &'static str) {
    let lower = sig.prompt.to_ascii_lowercase();
    let hit = |kws: &[String]| {
        kws.iter()
            .any(|k| !k.is_empty() && lower.contains(&k.to_ascii_lowercase()))
    };
    if hit(&h.heavy_keywords) {
        return (Some(Tier::Heavy), "heavy keyword");
    }
    if hit(&h.light_keywords) {
        return (Some(Tier::Light), "light keyword");
    }
    if h.escalate_iteration > 0 && sig.iteration >= h.escalate_iteration {
        return (Some(Tier::Heavy), "deep iteration");
    }
    if sig.tool_count as u32 >= h.heavy_tool_count {
        return (Some(Tier::Heavy), "rich toolset");
    }
    let len = sig.prompt.chars().count() as u32;
    if len >= h.heavy_chars {
        return (Some(Tier::Heavy), "long prompt");
    }
    if len <= h.light_chars {
        return (Some(Tier::Light), "short prompt");
    }
    (None, "ambiguous")
}

/// The resolved routing decision for one turn (for traces + stats).
#[derive(Clone, Debug)]
pub struct RoutingDecision {
    pub tier: Tier,
    pub provider: String,
    pub model: String,
    pub reason: String,
    /// Whether the judge model was consulted (its tokens are accounted).
    pub judged: bool,
}

/// A tier's resolved client + identity.
#[derive(Clone)]
pub struct TierClient {
    pub client: Arc<dyn LlmClient>,
    pub provider: String,
    pub model: String,
}

/// What the runner needs to execute a turn on the chosen tier.
pub struct Routed {
    pub decision: RoutingDecision,
    pub client: Arc<dyn LlmClient>,
}

const JUDGE_POLICY: &str = "\
You route turns for a coding agent to the right model tier. Judge whether the \
turn is `light` (simple, mechanical, short — renames, formatting, small edits, \
lookups) or `heavy` (reasoning, multi-step, design/debug/refactor/architecture). \
Respond with ONLY a single-line JSON object, no prose, no code fences:\n\
{\"tier\":\"light|heavy\"}";

/// A small local model that classifies ambiguous turns. Mirrors `LocalBrain`:
/// deterministic, tiny output cap, no prompt-cache. Fails safe to `Light`.
pub struct Judge {
    llm: Arc<dyn LlmClient>,
    options: LlmOptions,
}

impl Judge {
    pub fn new(llm: Arc<dyn LlmClient>, model: impl Into<String>) -> Self {
        let options = LlmOptions {
            model: model.into(),
            max_output_tokens: 24,
            temperature: 0.0,
            top_p: 1.0,
            top_k: 0,
            thinking: false,
            prompt_cache: false,
        };
        Judge { llm, options }
    }

    /// Returns `(tier, judge_usage)`. Any error/parse-miss → `(Light, ..)`.
    pub async fn assess(&self, sig: &TurnSignals<'_>, ct: &CancellationToken) -> (Tier, Usage) {
        let user = format!(
            "Prompt:\n{}\n\nTools available: {}. Iteration: {}.\nClassify this turn.",
            truncate(sig.prompt, 600),
            sig.tool_count,
            sig.iteration
        );
        let prompt = [Message::system(JUDGE_POLICY), Message::user(user)];
        let mut stream = match self
            .llm
            .stream_chat(&prompt, &[], &self.options, ct.child_token())
            .await
        {
            Ok(s) => s,
            Err(_) => return (Tier::Light, Usage::default()),
        };
        let mut text = String::new();
        let mut usage = Usage::default();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::Text { text: t }) => text.push_str(&t),
                Ok(StreamChunk::Usage(u)) => {
                    usage.input_tokens += u.input_tokens;
                    usage.output_tokens += u.output_tokens;
                    usage.cache_read_tokens += u.cache_read_tokens;
                    usage.cache_write_tokens += u.cache_write_tokens;
                }
                Ok(StreamChunk::Done { .. }) => break,
                Err(_) => return (Tier::Light, usage),
                _ => {}
            }
        }
        (parse_tier(&text), usage)
    }
}

/// The router the runner/spawner hold. Built once at engine startup.
pub struct Router {
    mode: AtomicU8,
    light: TierClient,
    heavy: TierClient,
    heuristics: HeuristicConfig,
    judge: Option<Judge>,
    stats: Arc<RouterStats>,
    /// Whether the light tier may target a grid peer's local model.
    pub grid_light: bool,
}

impl Router {
    pub fn new(
        mode: RouterMode,
        light: TierClient,
        heavy: TierClient,
        heuristics: HeuristicConfig,
        judge: Option<Judge>,
        grid_light: bool,
    ) -> Self {
        let stats = Arc::new(RouterStats::new(
            &light.model,
            &heavy.model,
            judge
                .as_ref()
                .map(|j| j.options.model.as_str())
                .unwrap_or(""),
        ));
        Router {
            mode: AtomicU8::new(mode.as_u8()),
            light,
            heavy,
            heuristics,
            judge,
            stats,
            grid_light,
        }
    }

    pub fn mode(&self) -> RouterMode {
        RouterMode::from_u8(self.mode.load(Ordering::Relaxed))
    }

    pub fn set_mode(&self, m: RouterMode) {
        self.mode.store(m.as_u8(), Ordering::Relaxed);
    }

    pub fn stats(&self) -> Arc<RouterStats> {
        self.stats.clone()
    }

    /// Iteration index at/above which a Light turn may escalate to Heavy. 0 = off.
    pub fn escalate_at(&self) -> u32 {
        self.heuristics.escalate_iteration
    }

    fn tier_client(&self, t: Tier) -> &TierClient {
        match t {
            Tier::Light => &self.light,
            Tier::Heavy => &self.heavy,
        }
    }

    fn routed(&self, tier: Tier, reason: impl Into<String>, judged: bool) -> Routed {
        let tc = self.tier_client(tier);
        Routed {
            decision: RoutingDecision {
                tier,
                provider: tc.provider.clone(),
                model: tc.model.clone(),
                reason: reason.into(),
                judged,
            },
            client: tc.client.clone(),
        }
    }

    /// Resolve a fixed tier directly (sub-agent demotion — no heuristic/judge).
    pub fn client_for(&self, tier: Tier) -> Routed {
        self.routed(tier, "sub-agent", false)
    }

    /// Decide the tier + resolve the client for one turn.
    pub async fn route(&self, sig: &TurnSignals<'_>, ct: &CancellationToken) -> Routed {
        match self.mode() {
            RouterMode::Off => self.routed(Tier::Heavy, "off", false),
            RouterMode::Judge => self.judge_or(sig, ct, Tier::Light).await,
            RouterMode::Heuristic => {
                let (heur, reason) = classify(sig, &self.heuristics);
                match heur {
                    Some(t) => self.routed(t, reason, false),
                    None => self.routed(Tier::Light, "ambiguous → light", false),
                }
            }
            RouterMode::Hybrid => {
                let (heur, reason) = classify(sig, &self.heuristics);
                match heur {
                    Some(t) => self.routed(t, reason, false),
                    None => self.judge_or(sig, ct, Tier::Light).await,
                }
            }
        }
    }

    async fn judge_or(
        &self,
        sig: &TurnSignals<'_>,
        ct: &CancellationToken,
        fallback: Tier,
    ) -> Routed {
        if let Some(j) = &self.judge {
            let (tier, usage) = j.assess(sig, ct).await;
            self.stats.record_judge(&usage);
            self.routed(tier, "judged", true)
        } else {
            self.routed(fallback, "no judge", false)
        }
    }

    /// A JSON snapshot for the `/route` overlay + `/api/route` (mode + savings).
    pub fn status(&self) -> Value {
        let mut v = self.stats.snapshot();
        if let Value::Object(m) = &mut v {
            m.insert("mode".into(), json!(self.mode().label()));
        }
        v
    }
}

/// The process-wide active router, published by the engine when a session is
/// built, so in-process UIs (the TUI `/route` overlay, the web `/api/route`
/// handler) can read live stats without a query round-trip through the actor.
/// Mirrors the grid-hook globals. Session swaps overwrite it with the new router.
static ACTIVE_ROUTER: StdRwLock<Option<Arc<Router>>> = StdRwLock::new(None);

/// Publish the active router (called by the engine on session build).
pub fn set_active_router(router: Arc<Router>) {
    if let Ok(mut g) = ACTIVE_ROUTER.write() {
        *g = Some(router);
    }
}

/// The active router's status JSON (mode + per-tier counts + `saved_usd`), or
/// `None` when no router is attached.
pub fn active_router_status() -> Option<Value> {
    ACTIVE_ROUTER
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|r| r.status()))
}

/// The active router's mode (for the TUI status line); `Off` when unattached.
pub fn active_router_mode() -> RouterMode {
    ACTIVE_ROUTER
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|r| r.mode()))
        .unwrap_or(RouterMode::Off)
}

#[derive(Default)]
struct TierAccum {
    turns: AtomicU64,
    input: AtomicU64,
    output: AtomicU64,
}

impl TierAccum {
    fn record(&self, input: u64, output: u64) {
        self.turns.fetch_add(1, Ordering::Relaxed);
        self.input.fetch_add(input, Ordering::Relaxed);
        self.output.fetch_add(output, Ordering::Relaxed);
    }

    fn read(&self) -> (u64, u64, u64) {
        (
            self.turns.load(Ordering::Relaxed),
            self.input.load(Ordering::Relaxed),
            self.output.load(Ordering::Relaxed),
        )
    }
}

/// Per-tier token/turn counters + savings math (vs an all-heavy counterfactual).
pub struct RouterStats {
    light: TierAccum,
    heavy: TierAccum,
    judge: TierAccum,
    light_model: String,
    heavy_model: String,
    judge_model: String,
}

impl RouterStats {
    fn new(light_model: &str, heavy_model: &str, judge_model: &str) -> Self {
        RouterStats {
            light: TierAccum::default(),
            heavy: TierAccum::default(),
            judge: TierAccum::default(),
            light_model: light_model.to_string(),
            heavy_model: heavy_model.to_string(),
            judge_model: judge_model.to_string(),
        }
    }

    /// Record one main-turn's usage under its tier.
    pub fn record(&self, tier: Tier, usage: &Usage) {
        let acc = match tier {
            Tier::Light => &self.light,
            Tier::Heavy => &self.heavy,
        };
        acc.record(usage.input_tokens as u64, usage.output_tokens as u64);
    }

    /// Record a judge call's usage (overhead, subtracted from savings).
    pub fn record_judge(&self, usage: &Usage) {
        self.judge
            .record(usage.input_tokens as u64, usage.output_tokens as u64);
    }

    /// JSON for the UIs: per-tier counts + cost, and `saved_usd` vs all-heavy.
    pub fn snapshot(&self) -> Value {
        let (lt, li, lo) = self.light.read();
        let (ht, hi, ho) = self.heavy.read();
        let (jt, ji, jo) = self.judge.read();
        let lc = pricing::estimate(&self.light_model, li, lo);
        let hc = pricing::estimate(&self.heavy_model, hi, ho);
        let jc = pricing::estimate(&self.judge_model, ji, jo);
        let actual = lc + hc + jc;
        // Counterfactual: every turn on the heavy model, with no judge overhead.
        let all_heavy = pricing::estimate(&self.heavy_model, li + hi, lo + ho);
        let saved = all_heavy - actual;
        json!({
            "light": { "model": self.light_model, "turns": lt, "input": li, "output": lo, "cost_usd": lc },
            "heavy": { "model": self.heavy_model, "turns": ht, "input": hi, "output": ho, "cost_usd": hc },
            "judge": { "model": self.judge_model, "turns": jt, "input": ji, "output": jo, "cost_usd": jc },
            "actual_cost_usd": actual,
            "all_heavy_cost_usd": all_heavy,
            "saved_usd": saved,
        })
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

fn parse_tier(text: &str) -> Tier {
    if let Some(obj) = extract_json_object(text) {
        if let Ok(v) = serde_json::from_str::<Value>(&obj) {
            if let Some(t) = v.get("tier").and_then(Value::as_str) {
                return tier_from_str(t);
            }
        }
    }
    // No usable JSON — scan for a keyword; default to the cheap tier.
    if text.to_ascii_lowercase().contains("heavy") {
        Tier::Heavy
    } else {
        Tier::Light
    }
}

fn tier_from_str(s: &str) -> Tier {
    match s.trim().to_ascii_lowercase().as_str() {
        "heavy" | "hard" | "flagship" | "big" => Tier::Heavy,
        _ => Tier::Light,
    }
}

/// Find the first balanced `{...}` JSON object in `s` (tolerates prose/fences).
fn extract_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'"' if !escaped => in_str = !in_str,
            b'\\' if in_str => {
                escaped = !escaped;
                continue;
            }
            b'{' if !in_str => depth += 1,
            b'}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..=i].to_string());
                }
            }
            _ => {}
        }
        escaped = false;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LlmError;
    use crate::llm::ToolSpec;
    use async_trait::async_trait;
    use blumi_protocol::FinishReason;
    use futures::stream::{self, BoxStream};

    fn heur() -> HeuristicConfig {
        HeuristicConfig::default()
    }

    fn sig<'a>(prompt: &'a str, tool_count: usize, iteration: u32) -> TurnSignals<'a> {
        TurnSignals {
            prompt,
            tool_count,
            iteration,
            in_subagent: false,
        }
    }

    #[test]
    fn classify_table() {
        let h = heur();
        // heavy keyword wins first
        assert_eq!(
            classify(&sig("please refactor this", 0, 0), &h).0,
            Some(Tier::Heavy)
        );
        // light keyword
        assert_eq!(
            classify(&sig("rename foo to bar", 0, 0), &h).0,
            Some(Tier::Light)
        );
        // deep iteration escalates
        assert_eq!(classify(&sig("mid", 0, 9), &h).0, Some(Tier::Heavy));
        // rich toolset
        assert_eq!(classify(&sig("mid", 99, 0), &h).0, Some(Tier::Heavy));
        // long prompt
        let long = "x".repeat(2000);
        assert_eq!(classify(&sig(&long, 0, 0), &h).0, Some(Tier::Heavy));
        // short prompt → light
        assert_eq!(classify(&sig("hi", 0, 0), &h).0, Some(Tier::Light));
        // ambiguous (medium length, no signal)
        let mid = "a".repeat(500);
        assert_eq!(classify(&sig(&mid, 0, 0), &h).0, None);
    }

    #[test]
    fn mode_parse_and_label() {
        assert_eq!(RouterMode::parse("hybrid"), Some(RouterMode::Hybrid));
        assert_eq!(RouterMode::parse("Judge"), Some(RouterMode::Judge));
        assert_eq!(RouterMode::parse("off"), Some(RouterMode::Off));
        assert_eq!(RouterMode::parse("nope"), None);
        assert_eq!(
            RouterMode::from_u8(RouterMode::Judge.as_u8()),
            RouterMode::Judge
        );
        assert_eq!(RouterMode::Hybrid.label(), "hybrid");
    }

    #[test]
    fn parse_tier_variants() {
        assert_eq!(parse_tier(r#"{"tier":"heavy"}"#), Tier::Heavy);
        assert_eq!(
            parse_tier("```json\n{\"tier\": \"light\"}\n```"),
            Tier::Light
        );
        assert_eq!(parse_tier("garbage"), Tier::Light); // fail-safe
        assert_eq!(parse_tier("the answer is HEAVY"), Tier::Heavy);
    }

    struct ScriptedLlm {
        reply: String,
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
            let chunks = vec![
                Ok(StreamChunk::Text {
                    text: self.reply.clone(),
                }),
                Ok(StreamChunk::Usage(Usage {
                    input_tokens: 10,
                    output_tokens: 2,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                })),
                Ok(StreamChunk::Done {
                    reason: FinishReason::Stop,
                }),
            ];
            Ok(Box::pin(stream::iter(chunks)))
        }
    }
    struct ErrLlm;
    #[async_trait]
    impl LlmClient for ErrLlm {
        async fn stream_chat(
            &self,
            _m: &[Message],
            _t: &[ToolSpec],
            _o: &LlmOptions,
            _ct: CancellationToken,
        ) -> Result<BoxStream<'static, Result<StreamChunk, LlmError>>, LlmError> {
            Err(LlmError::NoProvider)
        }
    }

    #[tokio::test]
    async fn judge_reads_tier_and_fails_safe() {
        let j = Judge::new(
            Arc::new(ScriptedLlm {
                reply: r#"{"tier":"heavy"}"#.into(),
            }),
            "judge-model",
        );
        let (tier, usage) = j
            .assess(&sig("anything", 0, 0), &CancellationToken::new())
            .await;
        assert_eq!(tier, Tier::Heavy);
        assert_eq!(usage.output_tokens, 2);

        let j2 = Judge::new(Arc::new(ErrLlm), "judge-model");
        let (tier2, _) = j2
            .assess(&sig("anything", 0, 0), &CancellationToken::new())
            .await;
        assert_eq!(tier2, Tier::Light); // unreachable judge → cheap tier
    }

    fn tier_client(model: &str, reply: &str) -> TierClient {
        TierClient {
            client: Arc::new(ScriptedLlm {
                reply: reply.into(),
            }),
            provider: "test".into(),
            model: model.into(),
        }
    }

    #[tokio::test]
    async fn hybrid_routes_light_heavy_and_judges_ambiguous() {
        let router = Router::new(
            RouterMode::Hybrid,
            tier_client("claude-haiku", ""),
            tier_client("claude-opus", ""),
            heur(),
            Some(Judge::new(
                Arc::new(ScriptedLlm {
                    reply: r#"{"tier":"heavy"}"#.into(),
                }),
                "claude-haiku",
            )),
            false,
        );
        let ct = CancellationToken::new();
        // short → light (no judge)
        let r = router.route(&sig("hi", 0, 0), &ct).await;
        assert_eq!(r.decision.tier, Tier::Light);
        assert!(!r.decision.judged);
        // keyword → heavy
        let r = router.route(&sig("refactor the parser", 0, 0), &ct).await;
        assert_eq!(r.decision.tier, Tier::Heavy);
        // ambiguous → judge says heavy
        let mid = "a".repeat(500);
        let r = router.route(&sig(&mid, 0, 0), &ct).await;
        assert_eq!(r.decision.tier, Tier::Heavy);
        assert!(r.decision.judged);
    }

    #[test]
    fn stats_saved_is_positive_when_light_used() {
        let stats = RouterStats::new("claude-haiku", "claude-opus", "claude-haiku");
        // 1M in + 1M out on the light tier.
        stats.record(
            Tier::Light,
            &Usage {
                input_tokens: 1_000_000,
                output_tokens: 1_000_000,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
        );
        let v = stats.snapshot();
        let saved = v["saved_usd"].as_f64().unwrap();
        // haiku (0.80/4.0) vs opus (15/75): big savings, minus zero judge.
        assert!(saved > 50.0, "expected large savings, got {saved}");
    }
}

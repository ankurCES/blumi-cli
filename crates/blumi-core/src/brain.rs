//! A local-LLM "brain" that reviews tool-permission requests (claudectl-style).
//!
//! Rather than asking the user to confirm every write or command, the brain
//! consults a (typically local, cheap) model with the tool, its target, and a
//! safety policy, and returns a verdict: allow, deny, or escalate-to-human.
//! Two modes govern how that verdict is used:
//!
//!   - **Advisory** — the verdict rides along on the approval card as a
//!     recommendation; the user still decides.
//!   - **Auto** — the brain decides: `allow`/`deny` resolve the request without
//!     a prompt, and only `escalate` (or an unreachable brain) falls through to
//!     the user.
//!
//! The brain is bounded by design: it is only consulted where the permission
//! engine would *otherwise prompt*. It never overrides an explicit deny rule,
//! never relaxes destructive-command gating into a silent allow (those are
//! marked `dangerous` and, in auto mode, still escalate), and never fires for
//! reads or yolo (which short-circuit earlier). Fail-safe: any parse failure or
//! provider error yields `Escalate`, so uncertainty always reaches a human.

use crate::llm::{LlmClient, LlmOptions};
use async_trait::async_trait;
use blumi_protocol::{Message, StreamChunk};
use futures::StreamExt;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// How the brain participates in approvals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrainMode {
    /// No brain — the engine prompts the user as usual.
    #[default]
    Off,
    /// Brain recommends; the user still confirms.
    Advisory,
    /// Brain auto-approves/denies; only uncertainty escalates to the user.
    Auto,
}

impl BrainMode {
    /// Parse a mode name (config + the `/brain` command).
    pub fn parse(s: &str) -> Option<BrainMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" | "" => Some(BrainMode::Off),
            "advisory" | "advise" | "suggest" | "on" => Some(BrainMode::Advisory),
            "auto" | "auto-run" | "autorun" | "autopilot" => Some(BrainMode::Auto),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            BrainMode::Off => "off",
            BrainMode::Advisory => "advisory",
            BrainMode::Auto => "auto",
        }
    }

    /// Stable small-int encoding for atomic storage.
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            BrainMode::Off => 0,
            BrainMode::Advisory => 1,
            BrainMode::Auto => 2,
        }
    }

    pub(crate) fn from_u8(v: u8) -> BrainMode {
        match v {
            1 => BrainMode::Advisory,
            2 => BrainMode::Auto,
            _ => BrainMode::Off,
        }
    }
}

/// What the brain decided for one tool call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrainDecision {
    Allow,
    Deny,
    /// Hand the decision to a human.
    Escalate,
}

/// The brain's verdict for one tool call.
#[derive(Clone, Debug)]
pub struct BrainVerdict {
    pub decision: BrainDecision,
    pub reason: String,
}

impl BrainVerdict {
    fn escalate(reason: impl Into<String>) -> Self {
        BrainVerdict {
            decision: BrainDecision::Escalate,
            reason: reason.into(),
        }
    }
}

/// Reviews a single permission-worthy tool call.
#[async_trait]
pub trait Brain: Send + Sync {
    async fn review(
        &self,
        tool: &str,
        subject: &str,
        dangerous: bool,
        input: &Value,
    ) -> BrainVerdict;
}

const POLICY: &str = "\
You are the safety brain for a coding agent. You are shown a single tool call the \
agent wants to run and must judge whether it is safe to allow without a human in \
the loop. Be permissive about ordinary development work (building, testing, \
formatting, editing source files within the project, reading files, listing \
directories) and conservative about anything that could lose work, exfiltrate \
data, or touch the system outside the project (deleting files, force-pushing, \
network calls to unknown hosts, sudo, chmod -R, writing outside the workspace, \
piping curl into a shell). When unsure, escalate.\n\n\
Respond with ONLY a single-line JSON object, no prose, no code fences:\n\
{\"decision\":\"allow|deny|escalate\",\"reason\":\"<=12 words\"}";

/// An LLM-backed brain: prompts a (typically local) model for a JSON verdict.
pub struct LocalBrain {
    llm: Arc<dyn LlmClient>,
    options: LlmOptions,
}

impl LocalBrain {
    /// Build a brain over `llm`, judging with `model`. Deterministic + short:
    /// temperature 0, a tight output cap, no prompt-cache breakpoints.
    pub fn new(llm: Arc<dyn LlmClient>, model: impl Into<String>) -> Self {
        let options = LlmOptions {
            model: model.into(),
            max_output_tokens: 120,
            temperature: 0.0,
            top_p: 1.0,
            top_k: 0,
            thinking: false,
            prompt_cache: false,
        };
        LocalBrain { llm, options }
    }
}

#[async_trait]
impl Brain for LocalBrain {
    async fn review(
        &self,
        tool: &str,
        subject: &str,
        dangerous: bool,
        input: &Value,
    ) -> BrainVerdict {
        let args = serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
        let user = format!(
            "Tool: {tool}\nTarget: {subject}\nFlagged dangerous by heuristics: {dangerous}\n\
             Arguments: {args}\n\nJudge this single call.",
        );
        let prompt = [Message::system(POLICY), Message::user(user)];

        let mut stream = match self
            .llm
            .stream_chat(&prompt, &[], &self.options, CancellationToken::new())
            .await
        {
            Ok(s) => s,
            Err(e) => return BrainVerdict::escalate(format!("brain unavailable: {e}")),
        };

        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::Text { text: t }) => text.push_str(&t),
                Ok(StreamChunk::Done { .. }) => break,
                Err(e) => return BrainVerdict::escalate(format!("brain error: {e}")),
                _ => {}
            }
        }
        parse_verdict(&text)
    }
}

/// Extract a verdict from model output, tolerating prose and code fences around
/// the JSON. Falls back to a keyword scan, then to `Escalate`.
fn parse_verdict(text: &str) -> BrainVerdict {
    if let Some(obj) = extract_json_object(text) {
        if let Ok(v) = serde_json::from_str::<Value>(&obj) {
            let decision = v.get("decision").and_then(Value::as_str).unwrap_or("");
            let reason = v
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if let Some(d) = decision_from_str(decision) {
                return BrainVerdict {
                    decision: d,
                    reason: if reason.is_empty() {
                        "no reason given".into()
                    } else {
                        reason
                    },
                };
            }
        }
    }
    // No usable JSON — scan the raw text for a clear keyword.
    let lower = text.to_ascii_lowercase();
    let reason = text.trim().chars().take(80).collect::<String>();
    if lower.contains("\"deny\"") || lower.contains("deny") {
        BrainVerdict {
            decision: BrainDecision::Deny,
            reason,
        }
    } else if lower.contains("\"allow\"") || lower.contains("allow") {
        BrainVerdict {
            decision: BrainDecision::Allow,
            reason,
        }
    } else {
        BrainVerdict::escalate("brain gave no clear verdict")
    }
}

fn decision_from_str(s: &str) -> Option<BrainDecision> {
    match s.trim().to_ascii_lowercase().as_str() {
        "allow" | "approve" | "yes" => Some(BrainDecision::Allow),
        "deny" | "reject" | "block" | "no" => Some(BrainDecision::Deny),
        "escalate" | "ask" | "human" | "unsure" => Some(BrainDecision::Escalate),
        _ => None,
    }
}

/// Find the first balanced `{...}` JSON object in `s`.
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

    #[test]
    fn mode_parse_and_label() {
        assert_eq!(BrainMode::parse("auto"), Some(BrainMode::Auto));
        assert_eq!(BrainMode::parse("Advisory"), Some(BrainMode::Advisory));
        assert_eq!(BrainMode::parse("off"), Some(BrainMode::Off));
        assert_eq!(BrainMode::parse("nonsense"), None);
        assert_eq!(BrainMode::Auto.label(), "auto");
        assert_eq!(BrainMode::from_u8(BrainMode::Auto.as_u8()), BrainMode::Auto);
    }

    #[test]
    fn parses_clean_json() {
        let v = parse_verdict(r#"{"decision":"allow","reason":"reads a project file"}"#);
        assert_eq!(v.decision, BrainDecision::Allow);
        assert_eq!(v.reason, "reads a project file");
    }

    #[test]
    fn parses_json_in_fences_and_prose() {
        let v =
            parse_verdict("Sure!\n```json\n{\"decision\": \"deny\", \"reason\": \"rm -rf\"}\n```");
        assert_eq!(v.decision, BrainDecision::Deny);
        assert_eq!(v.reason, "rm -rf");
    }

    #[test]
    fn escalates_on_garbage() {
        assert_eq!(
            parse_verdict("hmm I really can't tell").decision,
            BrainDecision::Escalate
        );
    }

    #[test]
    fn keyword_fallback_without_json() {
        assert_eq!(
            parse_verdict("decision: deny").decision,
            BrainDecision::Deny
        );
    }

    #[test]
    fn extracts_balanced_object_ignoring_braces_in_strings() {
        let s = r#"prefix {"reason":"has } brace","decision":"allow"} suffix"#;
        let obj = extract_json_object(s).unwrap();
        let v: Value = serde_json::from_str(&obj).unwrap();
        assert_eq!(v["decision"], "allow");
    }
}

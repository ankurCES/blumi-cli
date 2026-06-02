//! Messaging gateways: run blumi as a bot that maps platform messages to a
//! headless session and streams the reply back.
//!
//! The crate is UI-agnostic like the rest of blumi: a [`GatewayCore`] keeps one
//! headless [`SessionHandle`] per chat (so each conversation has its own
//! context) and turns an inbound message into a reply by driving the same
//! event/command core the TUI and web use. Each platform is just a *transport*
//! that feeds the core inbound text and delivers its reply.

mod discord;
mod telegram;

pub use discord::{run_discord, DiscordOptions};
pub use telegram::{run_telegram, TelegramOptions};

use blumi_core::SessionHandle;
use blumi_protocol::{ApprovalScope, Command, Decision, Event};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Spawns a fresh headless session. The binary implements this over
/// `build_session` — the seam that keeps the gateway free of the engine.
#[async_trait::async_trait]
pub trait SessionSpawner: Send + Sync {
    async fn spawn(&self) -> anyhow::Result<SessionHandle>;
}

/// Routes inbound messages to per-chat sessions and collects each reply.
pub struct GatewayCore {
    spawner: Arc<dyn SessionSpawner>,
    sessions: Mutex<HashMap<String, SessionHandle>>,
    /// Auto-approve tool calls. When false (the safe default for a bot), tools
    /// that need approval are denied — the agent still chats and does read-only
    /// work, but can't write/exec without a human in the loop.
    yolo: bool,
}

impl GatewayCore {
    pub fn new(spawner: Arc<dyn SessionSpawner>, yolo: bool) -> Self {
        GatewayCore {
            spawner,
            sessions: Mutex::new(HashMap::new()),
            yolo,
        }
    }

    /// Handle one inbound message for `chat_id`, returning the agent's reply.
    pub async fn handle(&self, chat_id: &str, text: &str) -> anyhow::Result<String> {
        let session = self.session_for(chat_id).await?;
        run_turn(&session, text, self.yolo).await
    }

    /// Forget a chat's session (e.g. on a `/reset` command).
    pub async fn reset(&self, chat_id: &str) {
        self.sessions.lock().await.remove(chat_id);
    }

    async fn session_for(&self, chat_id: &str) -> anyhow::Result<SessionHandle> {
        let mut map = self.sessions.lock().await;
        if let Some(s) = map.get(chat_id) {
            return Ok(s.clone());
        }
        let session = self.spawner.spawn().await?;
        map.insert(chat_id.to_string(), session.clone());
        Ok(session)
    }
}

/// Drive a single turn: send the user's text, accumulate the assistant's reply,
/// auto-resolve approvals/clarifications (never hang), and return the text.
pub async fn run_turn(session: &SessionHandle, text: &str, yolo: bool) -> anyhow::Result<String> {
    let mut events = session.subscribe();
    session
        .send(Command::UserMessage {
            text: text.to_string(),
            attachments: vec![],
            stream_id: None,
        })
        .await?;

    let mut reply = String::new();
    let mut tools: Vec<String> = Vec::new();
    loop {
        let env = events.recv().await?;
        match env.event {
            Event::Token { text } => reply.push_str(&text),
            Event::ToolStart { name, .. } => tools.push(name),
            Event::ApprovalRequest { request_id, .. } => {
                let decision = if yolo {
                    Decision::Allow
                } else {
                    Decision::Deny
                };
                session
                    .send(Command::ApproveTool {
                        request_id,
                        decision,
                        scope: ApprovalScope::Once,
                    })
                    .await?;
            }
            Event::ClarifyRequest { request_id, .. } => {
                session
                    .send(Command::AnswerClarify {
                        request_id,
                        value: String::new(),
                    })
                    .await?;
            }
            Event::Error { message, .. } => {
                reply.push_str(&format!("\n⚠ {message}"));
            }
            Event::TurnDone { .. } => break,
            _ => {}
        }
    }

    let reply = reply.trim().to_string();
    if reply.is_empty() {
        // A tool-only turn with no prose — acknowledge so the user isn't left
        // staring at silence.
        if tools.is_empty() {
            Ok("(done)".to_string())
        } else {
            Ok(format!("(done — ran {})", tools.join(", ")))
        }
    } else {
        Ok(reply)
    }
}

/// Split a reply into chunks no longer than `limit` bytes, preferring to break
/// on line then word boundaries (for platforms with a per-message cap, e.g.
/// Telegram's 4096). Never splits a multi-byte char.
pub fn split_message(text: &str, limit: usize) -> Vec<String> {
    if text.len() <= limit {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = text;
    while rest.len() > limit {
        // Largest char boundary <= limit.
        let mut cut = limit;
        while cut > 0 && !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        // Prefer a newline, then a space, within the window.
        let window = &rest[..cut];
        let brk = window
            .rfind('\n')
            .or_else(|| window.rfind(' '))
            .map(|i| i + 1)
            .filter(|&i| i > 0)
            .unwrap_or(cut);
        out.push(rest[..brk].trim_end().to_string());
        rest = rest[brk..].trim_start_matches(['\n', ' ']);
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out.retain(|s| !s.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use blumi_core::{spawn_session, SessionState, TurnContext, TurnRunner};
    use blumi_protocol::{DoneReason, Message, Role, SessionId};
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    /// A turn runner that streams back `echo: <the user's message>`.
    struct EchoRunner;

    #[async_trait::async_trait]
    impl TurnRunner for EchoRunner {
        async fn run_turn(
            &self,
            state: Arc<Mutex<SessionState>>,
            ctx: TurnContext,
            _ct: CancellationToken,
        ) -> DoneReason {
            let last = {
                let s = state.lock().await;
                s.messages
                    .iter()
                    .rev()
                    .find(|m| matches!(m.role, Role::User))
                    .map(|m| m.text())
                    .unwrap_or_default()
            };
            ctx.events.emit(Event::Token {
                text: format!("echo: {last}"),
            });
            state
                .lock()
                .await
                .messages
                .push(Message::assistant(format!("echo: {last}")));
            DoneReason::Completed
        }
    }

    struct EchoSpawner;

    #[async_trait::async_trait]
    impl SessionSpawner for EchoSpawner {
        async fn spawn(&self) -> anyhow::Result<SessionHandle> {
            Ok(spawn_session(
                SessionId::new(),
                "test",
                Arc::new(EchoRunner),
            ))
        }
    }

    #[tokio::test]
    async fn collects_reply_and_keeps_a_session_per_chat() {
        let core = GatewayCore::new(Arc::new(EchoSpawner), false);

        let a = core.handle("chatA", "hi").await.unwrap();
        assert_eq!(a, "echo: hi");

        // A different chat gets its own session.
        let b = core.handle("chatB", "yo").await.unwrap();
        assert_eq!(b, "echo: yo");

        // The same chat reuses its session and stays responsive.
        let a2 = core.handle("chatA", "again").await.unwrap();
        assert_eq!(a2, "echo: again");

        // After a reset, the chat still works (new session).
        core.reset("chatA").await;
        let a3 = core.handle("chatA", "fresh").await.unwrap();
        assert_eq!(a3, "echo: fresh");
    }

    #[test]
    fn short_message_is_one_chunk() {
        assert_eq!(split_message("hello", 100), vec!["hello"]);
    }

    #[test]
    fn splits_on_line_boundary() {
        let text = "line one\nline two\nline three";
        let chunks = split_message(text, 12);
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert!(c.len() <= 12, "chunk too long: {c:?}");
        }
        assert!(chunks.join(" ").replace('\n', " ").contains("line three"));
    }

    #[test]
    fn never_splits_multibyte_char() {
        let text = "✿".repeat(50); // each ✿ is 3 bytes
        let chunks = split_message(&text, 10);
        for c in &chunks {
            assert!(c.len() <= 10);
            // Each chunk must be valid UTF-8 made of whole flowers.
            assert!(c.chars().all(|ch| ch == '✿'));
        }
        assert_eq!(chunks.concat().chars().filter(|&c| c == '✿').count(), 50);
    }
}

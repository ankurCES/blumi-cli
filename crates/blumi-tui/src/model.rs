//! TUI state.

use crate::dialog::Picker;
use crate::theme::Theme;
use blumi_protocol::{Envelope, RequestId, Todo, ToolCallId};
use std::path::PathBuf;
use std::time::Instant;
use tui_textarea::TextArea;

const PLACEHOLDER: &str = "Ask blumi to build, fix, or explain… (/ for commands)";

/// What currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Editor,
    Chat,
}

/// One rendered transcript item.
#[derive(Debug, Clone)]
pub enum Entry {
    User(String),
    Assistant(String),
    Tool {
        id: ToolCallId,
        name: String,
        summary: String,
        ok: Option<bool>,
        preview: Option<String>,
        diff_stat: Option<String>,
        diff: Option<String>,
    },
    Notice(String),
}

/// A permission request awaiting the user's decision.
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub request_id: RequestId,
    pub tool: String,
    pub summary: String,
    pub dangerous: bool,
    pub diff: Option<String>,
    /// Optional brain recommendation (advisory mode / auto-mode escalation).
    pub advice: Option<String>,
}

/// Messages that drive `update`.
pub enum Msg {
    Term(crossterm::event::Event),
    Core(Envelope),
    Tick,
}

/// A request to switch the active session, handled by the app loop.
#[derive(Debug, Clone)]
pub enum SessionRequest {
    /// Start a brand-new session.
    New,
    /// Resume a stored session by id.
    Resume(String),
}

pub struct Model {
    pub model_name: String,
    pub working_dir: String,
    /// Memory files (for the `/memory` view).
    pub memory_md: PathBuf,
    pub user_md: PathBuf,
    /// Available skills (name, description) for `/skills`.
    pub skills: Vec<(String, String)>,
    /// Recent sessions (id, title) from history — dashboard + `/sessions`.
    pub recent_sessions: Vec<(String, String)>,
    /// Available personas (name, description) for `/persona`.
    pub personas: Vec<(String, String)>,
    /// The active persona name.
    pub persona: String,
    /// Directory where `/export` writes transcripts.
    pub export_dir: PathBuf,

    pub entries: Vec<Entry>,
    pub streaming: Option<String>,
    pub thinking: Option<String>,
    /// The agent's current todo/task list (the run dashboard).
    pub todos: Vec<Todo>,

    pub busy: bool,
    pub spinner_frame: usize,
    pub turn_count: u32,
    /// Auto-approve everything (yolo). Toggled by `/yolo`; shown in the dashboard.
    pub yolo: bool,
    /// Brain approval mode label ("off"/"advisory"/"auto"). Set by `/brain`.
    pub brain_mode: String,
    /// When the session started (for uptime).
    pub started: Instant,
    /// Milliseconds spent actively working with the bot (busy time).
    pub active_ms: u64,
    /// Tokens in the most recent request ≈ current context usage.
    pub context_tokens: u32,
    /// Model context window (for the usage bar).
    pub context_size: u32,
    /// Optional session title (`/name`).
    pub session_title: String,
    /// Optional steering goal (`/goal`), shown in the dashboard.
    pub goal: String,
    /// Whether to show the reasoning/thinking stream (`/reasoning`).
    pub show_reasoning: bool,
    /// Scheduled cron jobs (name, schedule) for `/cron`.
    pub cron_jobs: Vec<(String, String)>,

    pub focus: Focus,
    /// Lines scrolled up from the bottom; 0 = following the latest output.
    pub scrollback: u16,
    /// Whether the run dashboard sidebar is shown.
    pub show_dashboard: bool,

    pub input: TextArea<'static>,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    pub draft: String,
    /// Selected row in the slash-command popup.
    pub slash_sel: usize,

    pub pending: Option<PendingApproval>,
    pub dialog: Option<Picker>,
    /// Rendered memory text when the `/memory` overlay is open.
    pub memory_view: Option<String>,
    /// Rendered usage analytics when the `/usage` overlay is open.
    pub usage_view: Option<String>,
    /// Rendered task board when the `/board` overlay is open.
    pub board_view: Option<String>,
    /// Path to the persistent task board (`<project>/.blumi/tasks.json`).
    pub tasks_path: PathBuf,
    /// Autonomous loop is running (drives the task board turn by turn).
    pub loop_active: bool,
    /// Loop sends finished tasks to "review" instead of "done".
    pub loop_review: bool,
    /// Loop iteration counter.
    pub loop_iter: u32,
    /// The task currently in flight under the loop (id, title).
    pub loop_current: Option<(String, String)>,
    /// A pending session switch for the app loop to perform.
    pub session_request: Option<SessionRequest>,
    /// Set when the agent asked to reload itself (self-evolution). The app loop
    /// rebuilds the session in place once the turn is idle, keeping the
    /// transcript. Holds the reason for the completion notice.
    pub reload_pending: Option<String>,
    /// Provider/model catalog for the `/provider` + `/model` pickers (live).
    pub model_options: crate::app::ModelOptions,
    /// Pending provider switch (provider, optional key), applied by the app loop.
    pub provider_request: Option<(String, Option<String>)>,
    /// When set, the editor is capturing an API key for this (unready) provider.
    pub provider_key_prompt: Option<String>,

    pub theme: Theme,
    pub theme_idx: usize,

    pub input_tokens: u32,
    pub output_tokens: u32,

    pub should_quit: bool,
    dirty: bool,
}

impl Model {
    pub fn new(model_name: String, working_dir: String) -> Self {
        let mut input = TextArea::default();
        input.set_placeholder_text(PLACEHOLDER);
        Model {
            model_name,
            working_dir,
            memory_md: PathBuf::new(),
            user_md: PathBuf::new(),
            skills: Vec::new(),
            recent_sessions: Vec::new(),
            personas: Vec::new(),
            persona: "default".into(),
            export_dir: PathBuf::new(),
            entries: Vec::new(),
            streaming: None,
            thinking: None,
            todos: Vec::new(),
            busy: false,
            spinner_frame: 0,
            turn_count: 0,
            yolo: false,
            brain_mode: "off".into(),
            started: Instant::now(),
            active_ms: 0,
            context_tokens: 0,
            context_size: 131_072,
            session_title: String::new(),
            goal: String::new(),
            show_reasoning: true,
            cron_jobs: Vec::new(),
            focus: Focus::Editor,
            scrollback: 0,
            show_dashboard: true,
            input,
            history: Vec::new(),
            history_pos: None,
            draft: String::new(),
            slash_sel: 0,
            pending: None,
            dialog: None,
            memory_view: None,
            usage_view: None,
            board_view: None,
            tasks_path: PathBuf::new(),
            loop_active: false,
            loop_review: false,
            loop_iter: 0,
            loop_current: None,
            session_request: None,
            reload_pending: None,
            model_options: crate::app::ModelOptions::default(),
            provider_request: None,
            provider_key_prompt: None,
            theme: Theme::default(),
            theme_idx: 0,
            input_tokens: 0,
            output_tokens: 0,
            should_quit: false,
            dirty: true,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Whether the landing splash should show (nothing has happened yet).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.streaming.is_none() && !self.busy
    }

    /// True while the editor holds a slash command in progress.
    pub fn slash_active(&self) -> bool {
        self.input.lines().len() == 1 && self.input_text().starts_with('/')
    }

    pub fn input_text(&self) -> String {
        self.input.lines().join("\n")
    }

    pub fn clear_input(&mut self) {
        self.input = TextArea::default();
        self.input.set_placeholder_text(PLACEHOLDER);
        self.slash_sel = 0;
    }

    pub fn set_input(&mut self, text: &str) {
        let mut ta = TextArea::from(text.lines().map(|l| l.to_string()).collect::<Vec<_>>());
        ta.set_placeholder_text(PLACEHOLDER);
        self.input = ta;
    }

    pub fn clear_transcript(&mut self) {
        self.entries.clear();
        self.streaming = None;
        self.thinking = None;
        self.scrollback = 0;
    }

    pub fn cycle_theme(&mut self) {
        self.theme_idx = (self.theme_idx + 1) % crate::theme::THEMES.len();
        self.theme = Theme::by_index(self.theme_idx);
        self.entries
            .push(Entry::Notice(format!("theme: {}", self.theme.name)));
    }

    /// Set the theme by name; returns false if unknown.
    pub fn set_theme(&mut self, name: &str) -> bool {
        match (0..crate::theme::THEMES.len()).find(|&i| Theme::by_index(i).name == name) {
            Some(i) => {
                self.theme_idx = i;
                self.theme = Theme::by_index(i);
                true
            }
            None => false,
        }
    }

    /// The most recent user message text, if any.
    pub fn last_user_text(&self) -> Option<String> {
        self.entries.iter().rev().find_map(|e| match e {
            Entry::User(t) => Some(t.clone()),
            _ => None,
        })
    }

    /// Request a brand-new session (handled by the app loop).
    pub fn request_new_session(&mut self) {
        self.session_request = Some(SessionRequest::New);
    }

    /// Request resuming a stored session by id (handled by the app loop).
    pub fn request_resume(&mut self, id: impl Into<String>) {
        self.session_request = Some(SessionRequest::Resume(id.into()));
    }

    pub fn take_session_request(&mut self) -> Option<SessionRequest> {
        self.session_request.take()
    }

    /// Record an agent-requested self-reload (applied by the app loop when idle).
    pub fn request_reload(&mut self, reason: impl Into<String>) {
        self.reload_pending = Some(reason.into());
    }

    /// Request a provider switch (applied by the app loop: persist + reload).
    pub fn request_provider(&mut self, provider: String, key: Option<String>) {
        self.provider_request = Some((provider, key));
    }

    pub fn take_provider_request(&mut self) -> Option<(String, Option<String>)> {
        self.provider_request.take()
    }

    /// Start capturing an API key for an unready provider (masks the editor).
    pub fn start_key_prompt(&mut self, provider: String) {
        self.clear_input();
        self.input.set_mask_char('•');
        self.provider_key_prompt = Some(provider);
    }

    /// Cancel the key prompt (drops the mask via a fresh editor).
    pub fn cancel_key_prompt(&mut self) {
        self.provider_key_prompt = None;
        self.clear_input();
    }

    /// Reset all per-session state (keeps theme, personas, skills, paths).
    pub fn reset_for_session(&mut self) {
        self.entries.clear();
        self.streaming = None;
        self.thinking = None;
        self.todos.clear();
        self.busy = false;
        self.scrollback = 0;
        self.turn_count = 0;
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.context_tokens = 0;
        self.active_ms = 0;
        self.started = Instant::now();
        self.goal.clear();
        self.session_title.clear();
        self.pending = None;
        self.dialog = None;
        self.memory_view = None;
        self.usage_view = None;
        self.board_view = None;
        self.loop_active = false;
        self.loop_current = None;
        self.loop_iter = 0;
        self.clear_input();
    }

    /// Rebuild the transcript view from a resumed session's snapshot.
    pub fn load_snapshot(&mut self, snap: blumi_core::SessionSnapshot) {
        self.reset_for_session();
        self.input_tokens = snap.total_input_tokens;
        self.output_tokens = snap.total_output_tokens;
        self.turn_count = snap.turn_count;
        self.todos = snap.todos;
        if !snap.model.is_empty() {
            self.model_name = snap.model;
        }
        for m in snap.messages {
            match m.role {
                blumi_protocol::Role::User => self.entries.push(Entry::User(m.text())),
                blumi_protocol::Role::Assistant => {
                    let t = m.text();
                    if !t.trim().is_empty() {
                        self.entries.push(Entry::Assistant(t));
                    }
                }
                blumi_protocol::Role::Tool => {
                    let name = m.tool_name.clone().unwrap_or_else(|| "tool".into());
                    let preview = m.text().lines().next().unwrap_or("").to_string();
                    self.entries.push(Entry::Tool {
                        id: ToolCallId::new(),
                        name,
                        summary: String::new(),
                        ok: Some(true),
                        preview: Some(preview),
                        diff_stat: None,
                        diff: None,
                    });
                }
                blumi_protocol::Role::System => {}
            }
        }
    }

    /// Seconds since the session started.
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Number of tool calls in the transcript.
    pub fn tools_run(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e, Entry::Tool { .. }))
            .count()
    }

    /// Fraction of the context window currently used (0.0–1.0).
    pub fn context_frac(&self) -> f64 {
        if self.context_size == 0 {
            0.0
        } else {
            (self.context_tokens as f64 / self.context_size as f64).clamp(0.0, 1.0)
        }
    }

    /// Build the `/board` overlay from the persistent task board (read fresh).
    pub fn open_board(&mut self) {
        let board = blumi_task::TaskBoard::load(&self.tasks_path);
        let c = board.counts();
        let mut s = format!(
            "running {} · queued {} · review {} · done {}",
            c.doing, c.todo, c.review, c.done
        );
        if board.is_empty() {
            s.push_str("\n\nno tasks yet — add with `blumi task add`, then `blumi loop`");
        } else {
            for (i, t) in board.tasks().iter().enumerate() {
                s.push_str(&format!(
                    "\n{:>2}. {} P{}  {}",
                    i + 1,
                    t.state.icon(),
                    t.priority,
                    t.title
                ));
            }
        }
        self.board_view = Some(s);
    }

    /// Build the `/usage` analytics overlay.
    pub fn open_usage(&mut self) {
        let total = self.input_tokens + self.output_tokens;
        let pct = (self.context_frac() * 100.0).round() as u32;
        let model = if self.model_name.is_empty() {
            "default"
        } else {
            &self.model_name
        };
        let s = format!(
            "usage analytics\n\n\
             model:    {model}\n\
             persona:  {}\n\
             uptime:   {}\n\
             active:   {} with the bot\n\
             turns:    {}\n\
             tools:    {} run\n\
             tokens:   ↑{} in · ↓{} out · {} total\n\
             context:  {} / {} ({pct}%)",
            self.persona,
            fmt_dur(self.uptime_secs()),
            fmt_dur(self.active_ms / 1000),
            self.turn_count,
            self.tools_run(),
            self.input_tokens,
            self.output_tokens,
            total,
            self.context_tokens,
            self.context_size,
        );
        self.usage_view = Some(s);
    }

    /// Load the memory files into the `/memory` overlay.
    pub fn open_memory(&mut self) {
        let read = |p: &PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        let agent = read(&self.memory_md);
        let user = read(&self.user_md);
        let mut s = String::new();
        s.push_str("MEMORY.md (agent notes)\n");
        s.push_str(if agent.trim().is_empty() {
            "  (empty)\n"
        } else {
            &agent
        });
        s.push_str("\nUSER.md (about you)\n");
        s.push_str(if user.trim().is_empty() {
            "  (empty)\n"
        } else {
            &user
        });
        self.memory_view = Some(s);
    }

    /// Write the current transcript to a markdown file under `export_dir`.
    /// Returns the path written.
    pub fn export_transcript(&self) -> std::io::Result<PathBuf> {
        let mut md = String::from("# blumi session transcript\n\n");
        for entry in &self.entries {
            match entry {
                Entry::User(t) => md.push_str(&format!("## You\n\n{t}\n\n")),
                Entry::Assistant(t) => md.push_str(&format!("## blumi\n\n{t}\n\n")),
                Entry::Tool {
                    name, summary, ok, ..
                } => {
                    let mark = match ok {
                        Some(true) => "ok",
                        Some(false) => "failed",
                        None => "running",
                    };
                    md.push_str(&format!("- `{name}` ({mark}) — {summary}\n"));
                }
                Entry::Notice(t) => md.push_str(&format!("> {t}\n\n")),
            }
        }
        std::fs::create_dir_all(&self.export_dir)?;
        let path = self
            .export_dir
            .join(format!("transcript-turn{}.md", self.turn_count));
        std::fs::write(&path, md)?;
        Ok(path)
    }
}

/// Compact human duration: `2d 3h`, `4h 5m`, `6m 7s`, or `8s`.
pub fn fmt_dur(secs: u64) -> String {
    let (d, h, m, s) = (
        secs / 86_400,
        (secs % 86_400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_prompt_masks_then_clears() {
        let mut m = Model::new("m".into(), "/".into());
        m.start_key_prompt("openai".into());
        assert_eq!(m.provider_key_prompt.as_deref(), Some("openai"));
        assert_eq!(m.input.mask_char(), Some('•'));
        m.cancel_key_prompt();
        assert!(m.provider_key_prompt.is_none());
        assert_eq!(m.input.mask_char(), None);
    }

    #[test]
    fn provider_request_roundtrip() {
        let mut m = Model::new("m".into(), "/".into());
        m.request_provider("openai".into(), Some("sk".into()));
        assert_eq!(
            m.take_provider_request(),
            Some(("openai".to_string(), Some("sk".to_string())))
        );
        assert!(m.take_provider_request().is_none());
    }
}

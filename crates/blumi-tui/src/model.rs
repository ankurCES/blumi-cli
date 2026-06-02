//! TUI state.

use crate::dialog::Picker;
use crate::theme::Theme;
use blumi_protocol::{Envelope, RequestId, Todo, ToolCallId};
use std::path::PathBuf;
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
}

/// Messages that drive `update`.
pub enum Msg {
    Term(crossterm::event::Event),
    Core(Envelope),
    Tick,
}

pub struct Model {
    pub model_name: String,
    pub working_dir: String,
    /// Memory files (for the `/memory` view).
    pub memory_md: PathBuf,
    pub user_md: PathBuf,
    /// Available skills (name, description) for `/skills`.
    pub skills: Vec<(String, String)>,

    pub entries: Vec<Entry>,
    pub streaming: Option<String>,
    pub thinking: Option<String>,
    /// The agent's current todo/task list (the run dashboard).
    pub todos: Vec<Todo>,

    pub busy: bool,
    pub spinner_frame: usize,
    pub turn_count: u32,

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
            entries: Vec::new(),
            streaming: None,
            thinking: None,
            todos: Vec::new(),
            busy: false,
            spinner_frame: 0,
            turn_count: 0,
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
}

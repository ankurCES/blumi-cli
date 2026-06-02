//! TUI state.

use blumi_protocol::{Envelope, RequestId, ToolCallId};
use tui_textarea::TextArea;

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

    pub entries: Vec<Entry>,
    pub streaming: Option<String>,
    pub thinking: Option<String>,

    pub busy: bool,
    pub spinner_frame: usize,

    pub focus: Focus,
    /// Lines scrolled up from the bottom; 0 = following the latest output.
    pub scrollback: u16,

    pub input: TextArea<'static>,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    pub draft: String,

    pub pending: Option<PendingApproval>,

    pub input_tokens: u32,
    pub output_tokens: u32,

    pub should_quit: bool,
    dirty: bool,
}

impl Model {
    pub fn new(model_name: String, working_dir: String) -> Self {
        let mut input = TextArea::default();
        input.set_placeholder_text("Ask blumi to build, fix, or explain something…");
        Model {
            model_name,
            working_dir,
            entries: Vec::new(),
            streaming: None,
            thinking: None,
            busy: false,
            spinner_frame: 0,
            focus: Focus::Editor,
            scrollback: 0,
            input,
            history: Vec::new(),
            history_pos: None,
            draft: String::new(),
            pending: None,
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

    pub fn input_text(&self) -> String {
        self.input.lines().join("\n")
    }

    pub fn clear_input(&mut self) {
        self.input = TextArea::default();
        self.input
            .set_placeholder_text("Ask blumi to build, fix, or explain something…");
    }

    pub fn set_input(&mut self, text: &str) {
        let mut ta = TextArea::from(text.lines().map(|l| l.to_string()).collect::<Vec<_>>());
        ta.set_placeholder_text("Ask blumi to build, fix, or explain something…");
        self.input = ta;
    }
}

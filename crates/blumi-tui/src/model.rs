//! TUI state.

use crate::dialog::Picker;
use crate::motion::Motion;
use crate::theme::{Theme, ThemeRegistry};
use blumi_protocol::{Envelope, RequestId, Todo, ToolCallId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use tui_textarea::TextArea;

const PLACEHOLDER: &str = "Ask blumi to build, fix, or explain… (/ for commands)";

/// What currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Editor,
    Chat,
    /// The left explorer sidebar (tabbed: workspaces / sessions).
    Sidebar,
    /// The right agent dashboard pane (scrollable).
    Dashboard,
}

/// Editor input mode (vim-flavored, but optional). `Insert` is the default and
/// the only mode a chat-first user ever needs; `Nav` lets power users drive the
/// transcript with j/k/g/G and vim chords without the editor eating keys. Only
/// meaningful while `Focus::Editor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Insert,
    Nav,
}

/// Which tab the left explorer sidebar is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Workspaces,
    Sessions,
    Skills,
}

impl SidebarTab {
    /// Cycle to the next tab (Workspaces → Sessions → Skills → …).
    pub fn toggled(self) -> SidebarTab {
        match self {
            SidebarTab::Workspaces => SidebarTab::Sessions,
            SidebarTab::Sessions => SidebarTab::Skills,
            SidebarTab::Skills => SidebarTab::Workspaces,
        }
    }
}

/// A selectable project workspace shown in the left sidebar.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub name: String,
    pub path: String,
    pub pinned: bool,
}

/// File-browser popup state for `/open-workspace`: navigate directories and open
/// one (or several) as workspaces. Lists only sub-directories of `cwd`.
#[derive(Debug, Clone)]
pub struct FsBrowser {
    /// Directory currently being listed.
    pub cwd: PathBuf,
    /// Sub-directory names of `cwd` (full path = `cwd.join(name)`).
    pub entries: Vec<String>,
    /// Highlighted entry index.
    pub sel: usize,
}

impl FsBrowser {
    /// Open the browser at `start` (falls back to `/` if unreadable).
    pub fn open(start: PathBuf) -> Self {
        let mut b = FsBrowser {
            cwd: start,
            entries: Vec::new(),
            sel: 0,
        };
        b.reload();
        b
    }

    /// Refresh `entries` = sorted, non-hidden sub-directories of `cwd`.
    pub fn reload(&mut self) {
        let mut dirs: Vec<String> = std::fs::read_dir(&self.cwd)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| !n.starts_with('.')) // hide dotdirs (.git, .cache, …)
                    .collect()
            })
            .unwrap_or_default();
        dirs.sort_by_key(|s| s.to_lowercase());
        self.entries = dirs;
        self.sel = 0;
    }

    /// Full path of the highlighted entry, if any.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.entries.get(self.sel).map(|n| self.cwd.join(n))
    }

    /// Move the cursor by `delta`, wrapping.
    pub fn move_sel(&mut self, delta: isize) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        self.sel = (self.sel as isize + delta).rem_euclid(n as isize) as usize;
    }

    /// Descend into the highlighted directory.
    pub fn enter_dir(&mut self) {
        if let Some(p) = self.selected_path() {
            if p.is_dir() {
                self.cwd = p;
                self.reload();
            }
        }
    }

    /// Go up to the parent directory, re-highlighting the folder we came from.
    pub fn up_dir(&mut self) {
        let prev = self.cwd.clone();
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
            self.reload();
            if let Some(name) = prev.file_name().and_then(|s| s.to_str()) {
                if let Some(i) = self.entries.iter().position(|e| e == name) {
                    self.sel = i;
                }
            }
        }
    }
}

/// Lifecycle status of a delegated team member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Working,
    Done,
    Failed,
}

/// One delegated sub-agent shown in the right "active agents" pane.
#[derive(Debug, Clone)]
pub struct AgentCard {
    pub id: String,
    pub role: String,
    pub task: String,
    pub status: AgentStatus,
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

/// A plan awaiting the user's approval (the `ExitPlanMode` tool) — shown as a
/// scrollable modal.
pub struct PlanReview {
    pub request_id: RequestId,
    pub plan: String,
    /// Vertical scroll offset (rows) within the plan body.
    pub scroll: u16,
}

/// A plan's resolution, for the `/plans` browser dots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStatus {
    /// Rejected (red dot).
    Rejected,
    /// Approved, but superseded by a later plan (steady green dot).
    Approved,
    /// The current approved plan being worked (blinking green dot).
    Live,
}

/// One resolved plan in the `/plans` history browser.
pub struct PlanRecord {
    pub title: String,
    pub content: String,
    pub status: PlanStatus,
}

/// Messages that drive `update`.
pub enum Msg {
    Term(crossterm::event::Event),
    Core(Envelope),
    Tick,
    /// A background job (`/bg`) finished — its result, posted from a detached task.
    Bg(BgUpdate),
}

/// A finished background job, sent from its detached task back to the UI loop.
#[derive(Debug, Clone)]
pub struct BgUpdate {
    pub id: String,
    pub text: String,
    pub ok: bool,
}

/// A tool that's currently executing — tracked so the UI can post "still
/// working" charms for long-running ones (hermes-style).
pub struct ToolRun {
    pub started: Instant,
    pub name: String,
    /// How many long-run charms we've already posted for this tool (max 2).
    pub charms: u8,
}

/// An independently-scrollable panel: tracks its own scroll offset, on-screen
/// rect, and content length so the wheel/keys can pan it in isolation.
#[derive(Default)]
pub struct ScrollPane {
    pub scroll: usize,
    pub lines: usize,
    pub area: Option<(u16, u16, u16, u16)>,
}

impl ScrollPane {
    fn view_h(&self) -> usize {
        self.area.map_or(0, |(_, _, _, h)| h as usize)
    }

    /// Largest valid scroll offset (so the last line sits at the bottom).
    pub fn max_scroll(&self) -> usize {
        self.lines.saturating_sub(self.view_h())
    }

    /// Scroll by `delta` lines, clamped. `isize::MIN`/`MAX` jump to top/bottom.
    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.max_scroll() as isize;
        self.scroll = (self.scroll as isize).saturating_add(delta).clamp(0, max) as usize;
    }

    /// Record geometry + content length at render time, re-clamping the offset.
    pub fn record(&mut self, x: u16, y: u16, w: u16, h: u16, lines: usize) {
        self.area = Some((x, y, w, h));
        self.lines = lines;
        let max = self.max_scroll();
        if self.scroll > max {
            self.scroll = max;
        }
    }

    /// Whether (col,row) falls inside this panel's recorded rect.
    pub fn hit(&self, col: u16, row: u16) -> bool {
        self.area
            .is_some_and(|(x, y, w, h)| col >= x && col < x + w && row >= y && row < y + h)
    }
}

/// Which dashboard sub-panel the keyboard scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashPanel {
    Agents,
    Tasks,
}

/// A request to switch the active session, handled by the app loop.
#[derive(Debug, Clone)]
pub enum SessionRequest {
    /// Start a brand-new session.
    New,
    /// Resume a stored session by id.
    Resume(String),
    /// Switch to (or open) a remote-instance tab by name.
    Remote(String),
    /// Open a project workspace (by path) as a new tab.
    OpenWorkspace(String),
    /// Switch to an already-open tab by index (0 = local).
    SwitchTab(usize),
}

/// A row in the Sessions explorer. Configured remote instances (gateways you can
/// attach to live — e.g. the local `blumi serve` your phone drives) are listed
/// first, then recent stored sessions you can resume.
pub enum SessionEntry {
    /// A configured remote instance, by name — selecting it attaches live.
    Remote(String),
    /// A stored session `(id, title)` — selecting it resumes the transcript.
    Stored(String, String),
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
    /// Project workspaces for the left sidebar (recent + pinned + scanned).
    pub workspaces: Vec<Workspace>,
    /// Delegated team members for the right "active agents" pane.
    pub agents: Vec<AgentCard>,
    /// Which explorer tab is showing (workspaces / sessions).
    pub sidebar_tab: SidebarTab,
    /// Selection index in the workspaces list.
    pub ws_sel: usize,
    /// Selection index in the sessions list.
    pub sess_sel: usize,
    /// Selection index in the skills list.
    pub skill_sel: usize,
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
    /// When the current turn started working (for the live "working · Ns"
    /// indicator + long-running reassurance). `None` when idle.
    pub busy_since: Option<Instant>,
    /// Currently-executing tools, keyed by tool-call id, for long-run charms.
    pub running_tools: HashMap<String, ToolRun>,
    /// Number of background jobs (`/bg`) currently running.
    pub bg_count: usize,
    /// Monotonic counter for background-job ids.
    pub bg_seq: usize,
    /// A pending `/bg` prompt the app loop should spawn as a background job.
    pub bg_request: Option<String>,
    pub spinner_frame: usize,
    pub turn_count: u32,
    /// Auto-approve everything (yolo). Toggled by `/yolo`; shown in the dashboard.
    pub yolo: bool,
    /// Brain approval mode label ("off"/"advisory"/"auto"). Set by `/brain`.
    pub brain_mode: String,
    /// Pre-rendered accelerator + embeddings line for `/accel` (set by the host).
    pub accel: String,
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
    /// Editor input mode (Insert default / Nav for power-user transcript driving).
    pub mode: Mode,
    /// A pending vim chord (e.g. the first `g` of `gg`) + when it was pressed.
    pub chord: Option<(char, Instant)>,
    /// Lines scrolled up from the bottom; 0 = following the latest output.
    pub scrollback: u16,
    /// Whether the run dashboard sidebar is shown.
    pub show_dashboard: bool,
    /// Whether the left explorer rail is shown (toggled with Ctrl+B; also gated
    /// by terminal width).
    pub explorer_open: bool,

    pub input: TextArea<'static>,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    pub draft: String,
    /// Selected row in the slash-command popup.
    pub slash_sel: usize,

    pub pending: Option<PendingApproval>,
    /// A plan awaiting approval (the `ExitPlanMode` tool), shown as a scrollable
    /// modal that captures keys until approved/rejected.
    pub plan_review: Option<PlanReview>,
    /// Whether planning mode is on (mutating tools blocked). Mirrors the core
    /// flag for the header/dashboard indicator.
    pub plan_mode: bool,
    /// Auto-continue step budget (self-wake on the per-turn cap). Mirrors the
    /// core value for the dashboard; retuned live by `/autocontinue`.
    pub auto_continue: u32,
    pub dialog: Option<Picker>,
    /// Screen rect (x, y, w, h) of the open dialog's row list, recorded at
    /// render time so mouse clicks can be mapped to a row (click-to-select).
    pub dialog_list_area: Option<(u16, u16, u16, u16)>,
    /// Screen rect (x, y, w, h) of the sidebar's active list + its tab bar row,
    /// recorded at render time for click-to-select / click-to-switch-tab.
    pub sidebar_list_area: Option<(u16, u16, u16, u16)>,
    pub sidebar_tab_area: Option<(u16, u16, u16, u16)>,
    /// Title-row rects of the rails (click to collapse) + the editor box (click
    /// to focus + return to Insert mode), recorded at render time.
    pub explorer_title_area: Option<(u16, u16, u16, u16)>,
    pub agent_title_area: Option<(u16, u16, u16, u16)>,
    pub editor_area: Option<(u16, u16, u16, u16)>,
    /// Per-chip rects of the header tab strip (x, y, w, tab_index), recorded at
    /// render time so a click selects that specific tab.
    pub header_tab_areas: Vec<(u16, u16, u16, usize)>,
    /// Independently-scrollable dashboard sub-panels (active agents, tasks).
    pub agents_pane: ScrollPane,
    pub tasks_pane: ScrollPane,
    /// Which sub-panel the keyboard scrolls (when the dashboard is focused).
    pub dash_panel: DashPanel,
    /// The `/dashboard` full-screen modal: open flag + its own scroll pane.
    pub dash_modal: bool,
    /// The `/help` command-reference modal (shares `modal_pane` for scrolling).
    pub help_modal: bool,
    pub modal_pane: ScrollPane,
    /// `/plans`: proposed-plan history browser (two-pane: list + content).
    pub plans: Vec<PlanRecord>,
    pub plans_view: bool,
    pub plans_sel: usize,
    /// Scroll state for the selected plan's content (right pane).
    pub plans_pane: ScrollPane,
    /// Left-pane list rect, recorded at render for mouse-click selection.
    pub plans_list_area: Option<(u16, u16, u16, u16)>,
    /// Shared plan store (blumi.db); resolved plans persist + reload on startup.
    pub plans_store: Option<std::sync::Arc<blumi_persist::Store>>,
    /// Rendered memory text when the `/memory` overlay is open.
    pub memory_view: Option<String>,
    /// Rendered usage analytics when the `/usage` overlay is open.
    pub usage_view: Option<String>,
    /// Rendered task board when the `/board` overlay is open.
    pub board_view: Option<String>,
    /// Rendered grid view (task distribution by runtime) when `/grid` is open.
    pub grid_view: Option<String>,
    /// Rendered self-healing summary when `/heal` is open (recoveries/evolutions).
    pub heal_view: Option<String>,
    /// Rendered cost-aware routing summary when `/route` is open (tiers + savings).
    pub route_view: Option<String>,
    /// Rendered always-on discovery summary when `/discoveries` is open.
    pub discoveries_view: Option<String>,
    /// Rendered semantic-memory summary when `/memories` is open (entries).
    pub memories_view: Option<String>,
    /// Rendered knowledge-base summary when `/knowledge` is open (counts + sources).
    pub knowledge_view: Option<String>,
    /// Semantic memory store for the `/memories` overlay; None when memory is off.
    pub mem_store: Option<std::sync::Arc<blumi_persist::SemanticMemoryImpl>>,
    /// Code knowledge base for the `/knowledge` overlay; None when knowledge is off.
    pub knowledge_store: Option<std::sync::Arc<blumi_knowledge::KnowledgeStore>>,
    /// File-browser popup state when `/open-workspace` is open.
    pub fs_browser: Option<FsBrowser>,
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
    /// Configured remote instances (names) available to attach via `/remote`.
    pub remotes: Vec<String>,
    /// Open tabs (name, is_remote); index 0 is the local session.
    pub tabs: Vec<(String, bool)>,
    /// Index of the active tab.
    pub active_tab: usize,
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
    /// All selectable themes (built-ins + user themes from ~/.blumi/themes).
    pub themes: ThemeRegistry,
    /// Cinematic motion effects (tachyonfx). Applied last in `view::render`.
    pub motion: Motion,

    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Estimated session spend in USD (from billed tokens × list price).
    pub cost_usd: f64,

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
            workspaces: Vec::new(),
            agents: Vec::new(),
            sidebar_tab: SidebarTab::Workspaces,
            skill_sel: 0,
            ws_sel: 0,
            sess_sel: 0,
            personas: Vec::new(),
            persona: "default".into(),
            export_dir: PathBuf::new(),
            entries: Vec::new(),
            streaming: None,
            thinking: None,
            todos: Vec::new(),
            busy: false,
            busy_since: None,
            running_tools: HashMap::new(),
            bg_count: 0,
            bg_seq: 0,
            bg_request: None,
            spinner_frame: 0,
            turn_count: 0,
            yolo: false,
            brain_mode: "off".into(),
            accel: String::new(),
            started: Instant::now(),
            active_ms: 0,
            context_tokens: 0,
            context_size: 131_072,
            session_title: String::new(),
            goal: String::new(),
            show_reasoning: true,
            cron_jobs: Vec::new(),
            focus: Focus::Editor,
            mode: Mode::Insert,
            chord: None,
            scrollback: 0,
            show_dashboard: true,
            explorer_open: true,
            input,
            history: Vec::new(),
            history_pos: None,
            draft: String::new(),
            slash_sel: 0,
            pending: None,
            plan_review: None,
            plan_mode: false,
            auto_continue: 12,
            dialog: None,
            dialog_list_area: None,
            sidebar_list_area: None,
            sidebar_tab_area: None,
            explorer_title_area: None,
            agent_title_area: None,
            editor_area: None,
            header_tab_areas: Vec::new(),
            agents_pane: ScrollPane::default(),
            tasks_pane: ScrollPane::default(),
            dash_panel: DashPanel::Agents,
            dash_modal: false,
            help_modal: false,
            modal_pane: ScrollPane::default(),
            plans: Vec::new(),
            plans_view: false,
            plans_sel: 0,
            plans_pane: ScrollPane::default(),
            plans_list_area: None,
            plans_store: None,
            memory_view: None,
            usage_view: None,
            board_view: None,
            grid_view: None,
            heal_view: None,
            route_view: None,
            discoveries_view: None,
            memories_view: None,
            knowledge_view: None,
            mem_store: None,
            knowledge_store: None,
            fs_browser: None,
            remotes: Vec::new(),
            tabs: vec![("local".to_string(), false)],
            active_tab: 0,
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
            themes: ThemeRegistry::default(),
            motion: Motion::default(),
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
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
        self.theme_idx = self.themes.next_index(self.theme_idx);
        self.theme = self.themes.get(self.theme_idx);
        self.motion.scene_in();
        self.entries
            .push(Entry::Notice(format!("theme: {}", self.theme.name)));
    }

    /// Set the theme by name (case-insensitive); returns false if unknown.
    pub fn set_theme(&mut self, name: &str) -> bool {
        match self.themes.index_of(name) {
            Some(i) => {
                self.theme_idx = i;
                self.theme = self.themes.get(i);
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

    /// Request attaching to (or switching to) a remote-instance tab by name.
    pub fn request_remote(&mut self, name: impl Into<String>) {
        self.session_request = Some(SessionRequest::Remote(name.into()));
    }

    /// A team member started — add it to the active-agents pane (capped).
    pub fn agent_started(&mut self, id: String, role: String, task: String) {
        self.agents.push(AgentCard {
            id,
            role,
            task,
            status: AgentStatus::Working,
        });
        if self.agents.len() > 12 {
            let drop = self.agents.len() - 12;
            self.agents.drain(0..drop);
        }
        self.mark_dirty();
    }

    /// A team member finished — mark it done/failed and update its line.
    pub fn agent_finished(&mut self, id: &str, ok: bool, summary: String) {
        if let Some(a) = self.agents.iter_mut().find(|a| a.id == id) {
            a.status = if ok {
                AgentStatus::Done
            } else {
                AgentStatus::Failed
            };
            if !summary.trim().is_empty() {
                a.task = summary;
            }
        }
        self.mark_dirty();
    }

    /// Cycle the explorer tab (workspaces → sessions → skills → …).
    pub fn toggle_sidebar_tab(&mut self) {
        self.sidebar_tab = self.sidebar_tab.toggled();
        self.mark_dirty();
    }

    /// Toggle the left explorer rail (Ctrl+B).
    pub fn toggle_explorer(&mut self) {
        self.explorer_open = !self.explorer_open;
        self.mark_dirty();
    }
    /// Toggle the right agent rail (Ctrl+J / `/tasks`).
    pub fn toggle_dashboard(&mut self) {
        self.show_dashboard = !self.show_dashboard;
        self.mark_dirty();
    }
    /// Switch editor mode (Insert ↔ Nav), redrawing on change.
    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.mark_dirty();
        }
    }
    /// Record/resolve a vim chord on `c`. Returns true when `c` completes a
    /// same-key chord pressed within the 600 ms window (e.g. the 2nd `g` of `gg`).
    pub fn chord(&mut self, c: char) -> bool {
        let now = Instant::now();
        if let Some((prev, at)) = self.chord {
            if prev == c && now.duration_since(at).as_millis() <= 600 {
                self.chord = None;
                return true;
            }
        }
        self.chord = Some((c, now));
        false
    }
    /// Drop a stale chord (called on tick) so a lone `g` doesn't linger.
    pub fn clear_stale_chord(&mut self) {
        if let Some((_, at)) = self.chord {
            if Instant::now().duration_since(at).as_millis() > 600 {
                self.chord = None;
            }
        }
    }

    /// The Sessions explorer rows: configured remotes (live attach) first, then
    /// recent stored sessions (resume). One source of truth for nav/select/render
    /// so the prepended remotes index consistently everywhere.
    pub fn session_entries(&self) -> Vec<SessionEntry> {
        let mut v: Vec<SessionEntry> = self
            .remotes
            .iter()
            .map(|n| SessionEntry::Remote(n.clone()))
            .collect();
        v.extend(
            self.recent_sessions
                .iter()
                .map(|(id, title)| SessionEntry::Stored(id.clone(), title.clone())),
        );
        v
    }

    /// Move the active explorer list's selection (clamped).
    pub fn sidebar_move(&mut self, delta: isize) {
        match self.sidebar_tab {
            SidebarTab::Workspaces => {
                self.ws_sel = step_index(self.ws_sel, delta, self.workspaces.len())
            }
            SidebarTab::Sessions => {
                self.sess_sel = step_index(self.sess_sel, delta, self.session_entries().len())
            }
            SidebarTab::Skills => {
                self.skill_sel = step_index(self.skill_sel, delta, self.skills.len())
            }
        }
        self.mark_dirty();
    }

    /// Activate the active explorer selection (open workspace / resume session).
    pub fn sidebar_activate(&mut self) {
        match self.sidebar_tab {
            SidebarTab::Workspaces => {
                if let Some(ws) = self.workspaces.get(self.ws_sel) {
                    self.session_request = Some(SessionRequest::OpenWorkspace(ws.path.clone()));
                }
            }
            SidebarTab::Sessions => match self.session_entries().get(self.sess_sel) {
                // A configured remote/gateway → attach live (same as `/remote`),
                // so you watch its running turn + active agents in this TUI.
                Some(SessionEntry::Remote(name)) => {
                    self.session_request = Some(SessionRequest::Remote(name.clone()));
                }
                // A stored session → resume its transcript.
                Some(SessionEntry::Stored(id, _)) => {
                    self.session_request = Some(SessionRequest::Resume(id.clone()));
                }
                None => {}
            },
            // Skills are browse-only in the rail; the agent loads them via the
            // `skill` tool. Selecting one is a no-op (no session change).
            SidebarTab::Skills => {}
        }
    }

    /// Request switching to an already-open tab by index.
    pub fn request_tab(&mut self, index: usize) {
        self.session_request = Some(SessionRequest::SwitchTab(index));
    }

    /// Open the `/open-workspace` file browser, starting at the parent of the
    /// current working directory (so sibling projects show up first).
    pub fn open_fs_browser(&mut self) {
        let here = PathBuf::from(&self.working_dir);
        let start = here.parent().map(|p| p.to_path_buf()).unwrap_or(here);
        self.fs_browser = Some(FsBrowser::open(start));
    }

    /// Add a directory to the workspace pane (deduped) so it shows immediately.
    pub fn add_workspace(&mut self, path: &str) {
        let path = path.trim_end_matches('/').to_string();
        if path.is_empty() || self.workspaces.iter().any(|w| w.path == path) {
            return;
        }
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        self.workspaces.push(Workspace {
            name,
            path,
            pinned: false,
        });
    }

    /// Open `path` as a workspace: show it in the pane now + ask the app loop to
    /// open/switch to it (which also persists it to recents).
    pub fn open_workspace_path(&mut self, path: &str) {
        self.add_workspace(path);
        self.session_request = Some(SessionRequest::OpenWorkspace(path.to_string()));
    }

    /// Switch to the next open tab (wraps). No-op if only the local tab is open.
    pub fn cycle_tab(&mut self) {
        if self.tabs.len() > 1 {
            let next = (self.active_tab + 1) % self.tabs.len();
            self.request_tab(next);
        }
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
        self.busy_since = None;
        self.running_tools.clear();
        self.scrollback = 0;
        self.turn_count = 0;
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.cost_usd = 0.0;
        self.context_tokens = 0;
        self.active_ms = 0;
        self.started = Instant::now();
        self.goal.clear();
        self.session_title.clear();
        self.pending = None;
        self.plan_review = None;
        self.plan_mode = false;
        self.dialog = None;
        self.memory_view = None;
        self.usage_view = None;
        self.board_view = None;
        self.grid_view = None;
        self.heal_view = None;
        self.route_view = None;
        self.discoveries_view = None;
        self.memories_view = None;
        self.knowledge_view = None;
        self.help_modal = false;
        self.plans_view = false;
        self.agents.clear();
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

    /// Seconds the current turn has been working (0 when idle).
    pub fn busy_secs(&self) -> u64 {
        self.busy_since.map(|t| t.elapsed().as_secs()).unwrap_or(0)
    }

    /// Take a pending `/bg` prompt for the app loop to spawn as a background job.
    pub fn take_bg_request(&mut self) -> Option<String> {
        self.bg_request.take()
    }

    /// Scroll the right dashboard pane by `delta` lines (clamped to its content).
    /// `isize::MIN`/`MAX` jump to top/bottom.
    /// Scroll the keyboard-selected dashboard sub-panel by `delta` lines.
    pub fn scroll_dashboard(&mut self, delta: isize) {
        match self.dash_panel {
            DashPanel::Agents => self.agents_pane.scroll_by(delta),
            DashPanel::Tasks => self.tasks_pane.scroll_by(delta),
        }
    }

    /// Switch which dashboard sub-panel the keyboard scrolls.
    pub fn cycle_dash_panel(&mut self) {
        self.dash_panel = match self.dash_panel {
            DashPanel::Agents => DashPanel::Tasks,
            DashPanel::Tasks => DashPanel::Agents,
        };
    }

    /// Open/close the `/dashboard` full-screen modal.
    pub fn toggle_dash_modal(&mut self) {
        self.dash_modal = !self.dash_modal;
        if self.dash_modal {
            self.help_modal = false;
            self.modal_pane.scroll = 0;
        }
    }

    /// Open the `/help` command-reference modal (scrollable; shares modal_pane).
    pub fn open_help_modal(&mut self) {
        self.help_modal = true;
        self.dash_modal = false;
        self.modal_pane.scroll = 0;
    }

    /// Record a resolved plan in the `/plans` history. The newest approved plan
    /// is "live" (blinking); approving a new one demotes the previous live.
    pub fn record_plan(&mut self, content: String, approved: bool) {
        let title = content
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("(untitled plan)")
            .trim_start_matches('#')
            .trim()
            .to_string();
        let status = if approved {
            for p in self.plans.iter_mut() {
                if p.status == PlanStatus::Live {
                    p.status = PlanStatus::Approved;
                }
            }
            PlanStatus::Live
        } else {
            PlanStatus::Rejected
        };
        self.plans.push(PlanRecord {
            title,
            content,
            status,
        });
    }

    /// Open the `/plans` browser, selecting the live plan (else the newest).
    pub fn open_plans_view(&mut self) {
        self.plans_view = true;
        self.plans_sel = self
            .plans
            .iter()
            .rposition(|p| p.status == PlanStatus::Live)
            .or_else(|| self.plans.len().checked_sub(1))
            .unwrap_or(0);
        self.plans_pane.scroll = 0;
    }

    /// Select plan `idx` (if valid), resetting the content scroll.
    pub fn plans_select(&mut self, idx: usize) {
        if idx < self.plans.len() && idx != self.plans_sel {
            self.plans_sel = idx;
            self.plans_pane.scroll = 0;
        }
    }

    /// Move the plan selection by `delta` (isize::MIN/MAX jump to the ends).
    pub fn plans_move(&mut self, delta: isize) {
        if self.plans.is_empty() {
            return;
        }
        let max = self.plans.len() as isize - 1;
        let next = (self.plans_sel as isize)
            .saturating_add(delta)
            .clamp(0, max) as usize;
        self.plans_select(next);
    }

    /// Replace the in-memory plan list from persisted records (on startup).
    pub fn load_plans(&mut self, stored: Vec<blumi_persist::StoredPlan>) {
        self.plans = stored
            .into_iter()
            .map(|s| PlanRecord {
                title: s.title,
                content: s.content,
                status: match s.status.as_str() {
                    "live" => PlanStatus::Live,
                    "rejected" => PlanStatus::Rejected,
                    _ => PlanStatus::Approved,
                },
            })
            .collect();
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
        // Tasks executing on a remote grid runtime carry an `owner` (peer name).
        let remote_n = board
            .tasks()
            .iter()
            .filter(|t| t.owner.as_deref().is_some_and(|o| !o.is_empty()))
            .count();
        if remote_n > 0 {
            s.push_str(&format!(" · {} remote {remote_n}", crate::icons::remote()));
        }
        let total_cost: f64 = board.tasks().iter().filter_map(|t| t.cost_usd).sum();
        if total_cost > 0.0 {
            s.push_str(&format!(" · ${total_cost:.2}"));
        }
        if board.is_empty() {
            s.push_str("\n\nno tasks yet — add with `blumi task add`, then `blumi loop`");
        } else {
            for (i, t) in board.tasks().iter().enumerate() {
                // Show the remote runtime (icon + peer) for handed-off tasks.
                let owner = match t.owner.as_deref().filter(|o| !o.is_empty()) {
                    Some(host) => format!("  {} {host}", crate::icons::remote()),
                    None => String::new(),
                };
                // Per-task cost: $ when priced, else raw tokens once it has run.
                let cost = match t.cost_usd {
                    Some(c) if c > 0.0 => format!("  ${c:.3}"),
                    _ if t.input_tokens + t.output_tokens > 0 => {
                        format!("  ↑{} ↓{}", t.input_tokens, t.output_tokens)
                    }
                    _ => String::new(),
                };
                s.push_str(&format!(
                    "\n{:>2}. {} P{}  {}{}{}",
                    i + 1,
                    t.state.icon(),
                    t.priority,
                    t.title,
                    cost,
                    owner
                ));
            }
        }
        self.board_view = Some(s);
    }

    /// Build the `/discoveries` overlay: tasks the always-on pass proposed
    /// (titles marked `Discovered:`), plus a pointer to the full reports.
    pub fn open_discoveries(&mut self) {
        let board = blumi_task::TaskBoard::load(&self.tasks_path);
        let found: Vec<_> = board
            .tasks()
            .iter()
            .filter(|t| t.title.starts_with("Discovered:"))
            .collect();
        let mut s = format!("proactively discovered tasks: {}\n", found.len());
        if found.is_empty() {
            s.push_str(
                "\nnone yet — enable in settings.json:\n  \"always_on\": { \"enabled\": true, \"autonomy\": \"propose\" }\n\nreports land in ~/.blumi/reports/",
            );
        } else {
            for (i, t) in found.iter().enumerate() {
                let title = t.title.trim_start_matches("Discovered:").trim();
                s.push_str(&format!("\n{:>2}. {} {}", i + 1, t.state.icon(), title));
            }
            s.push_str("\n\nfull reports in ~/.blumi/reports/");
        }
        self.discoveries_view = Some(s);
    }

    /// Build the `/grid` overlay: task distribution across runtimes, derived from
    /// the board's `owner` field (which peer each task ran on). For live peer
    /// online/offline health + token usage, see the mobile app's Grid section or
    /// ask in chat (the gateway holds the live registry).
    pub fn open_grid(&mut self) {
        use blumi_task::TaskState;
        use std::collections::BTreeMap;
        let board = blumi_task::TaskBoard::load(&self.tasks_path);
        // owner ("local" = unowned) -> the states of its tasks
        let mut groups: BTreeMap<String, Vec<TaskState>> = BTreeMap::new();
        for t in board.tasks() {
            let key = match &t.owner {
                Some(o) if !o.is_empty() => o.clone(),
                _ => "local".to_string(),
            };
            groups.entry(key).or_default().push(t.state);
        }
        let mut s = String::from("grid — task distribution by runtime");
        if groups.is_empty() {
            s.push_str(
                "\n\nno tasks yet. enable the grid (settings.json), add tasks, \
                 then run the loop in grid mode.",
            );
        }
        let (mut local_n, mut remote_n) = (0usize, 0usize);
        for (owner, states) in &groups {
            let doing = states
                .iter()
                .filter(|x| matches!(x, TaskState::Doing))
                .count();
            let done = states
                .iter()
                .filter(|x| matches!(x, TaskState::Done))
                .count();
            let is_local = owner == "local";
            let icon = if is_local {
                crate::icons::local()
            } else {
                crate::icons::remote()
            };
            s.push_str(&format!(
                "\n{} {}  —  {} tasks (running {}, done {})",
                icon,
                owner,
                states.len(),
                doing,
                done
            ));
            if is_local {
                local_n += states.len();
            } else {
                remote_n += states.len();
            }
        }
        let peers = groups.keys().filter(|k| *k != "local").count();
        s.push_str(&format!(
            "\n\ntotals: {local_n} local · {remote_n} remote · {peers} peer(s) with work"
        ));
        s.push_str("\n\nlive peer health + token usage: the app's Grid section, or ask in chat.");
        self.grid_view = Some(s);
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
/// Step a list index by `delta`, clamped to `[0, len-1]` (0 if empty).
pub(crate) fn step_index(cur: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (cur as isize + delta).clamp(0, len as isize - 1) as usize
}

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

    #[test]
    fn fs_browser_lists_descends_and_ascends() {
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("beta")).unwrap();
        fs::create_dir(root.join("alpha")).unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();
        fs::create_dir(root.join("alpha").join("sub")).unwrap();

        let mut b = FsBrowser::open(root.to_path_buf());
        // sorted, dotdirs hidden
        assert_eq!(b.entries, vec!["alpha".to_string(), "beta".to_string()]);
        assert_eq!(b.selected_path(), Some(root.join("alpha")));

        // wrap-around movement
        b.move_sel(-1);
        assert_eq!(b.selected_path(), Some(root.join("beta")));

        // descend into alpha
        b.sel = 0;
        b.enter_dir();
        assert_eq!(b.cwd, root.join("alpha"));
        assert_eq!(b.entries, vec!["sub".to_string()]);

        // ascend → back to root, re-highlighting the folder we came from
        b.up_dir();
        assert_eq!(b.cwd, root.to_path_buf());
        assert_eq!(b.selected_path(), Some(root.join("alpha")));
    }

    #[test]
    fn add_workspace_dedups_and_appends_to_pane() {
        let mut m = Model::new("m".into(), "/tmp/proj".into());
        let before = m.workspaces.len();
        m.add_workspace("/a/b/foo");
        m.add_workspace("/a/b/foo/"); // trailing slash → same path
        m.add_workspace("/a/b/bar");
        assert_eq!(m.workspaces.len(), before + 2);
        let foo = m.workspaces.iter().find(|w| w.path == "/a/b/foo").unwrap();
        assert_eq!(foo.name, "foo");
    }
}

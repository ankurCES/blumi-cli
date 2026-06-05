//! The TUI event loop: terminal setup/teardown and the select loop bridging
//! crossterm input, the core's event stream, and an animation tick.

use crate::dialog::Picker;
use crate::model::{BgUpdate, Entry, Model, Msg, SessionRequest};
use crate::{update, view};
use blumi_core::SessionHandle;
use blumi_protocol::{Command, Envelope, Event, Role};
use blumi_task::{TaskBoard, TaskState};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture, EventStream,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

pub(crate) type Term = Terminal<CrosstermBackend<Stdout>>;

/// Context fill at which we roll over to a fresh session (after in-place
/// compaction has already done what it can lower down). Headroom for the handoff.
const ROLLOVER_THRESHOLD: f64 = 0.92;

/// A selectable provider for the TUI provider picker.
#[derive(Clone)]
pub struct ProviderOpt {
    pub name: String,
    pub label: String,
    /// Has a usable key (or needs none); unready ones prompt for a key.
    pub ready: bool,
}

/// Active provider/model + suggestions for the TUI pickers (read live).
#[derive(Clone, Default)]
pub struct ModelOptions {
    pub provider: String,
    pub model: String,
    pub models: Vec<String>,
    pub providers: Vec<ProviderOpt>,
}

/// Creates, resumes, lists, and saves sessions on the TUI's behalf. The binary
/// implements this over `build_session` + the persistence store — the seam that
/// lets the TUI switch live sessions without knowing how they're wired.
#[async_trait::async_trait]
pub trait SessionFactory: Send + Sync {
    /// Spawn a fresh session.
    async fn create(&self) -> anyhow::Result<SessionHandle>;

    /// Spawn a session for an autonomous background job (`/bg`). It runs
    /// unattended with no UI, so it must auto-approve tools (yolo) rather than
    /// block on prompts. Default: fall back to a normal session.
    async fn create_background(&self) -> anyhow::Result<SessionHandle> {
        self.create().await
    }
    /// Resume a stored session by id, seeded with its prior messages.
    async fn resume(&self, id: &str) -> anyhow::Result<SessionHandle>;
    /// Rebuild the agent in place (self-evolution): re-read config + re-scan
    /// skills, seeded with the live snapshot so the conversation is preserved.
    async fn reload(&self, snapshot: blumi_core::SessionSnapshot) -> anyhow::Result<SessionHandle>;

    /// Roll over to a *fresh* session when the context window is full: the new
    /// session is seeded with a condensed handoff (a summary of the old session
    /// plus its last few turns) so the agent keeps context with a near-empty
    /// window. Default: fall back to an in-place reload (no reduction).
    async fn rollover(
        &self,
        snapshot: blumi_core::SessionSnapshot,
    ) -> anyhow::Result<SessionHandle> {
        self.reload(snapshot).await
    }
    /// Recent sessions as (id, title), newest first.
    async fn list(&self) -> Vec<(String, String)>;
    /// Persist the given session (best-effort).
    async fn save(&self, handle: &SessionHandle);
    /// Active provider/model + suggestions + selectable providers (read live).
    fn model_options(&self) -> ModelOptions;
    /// Persist a provider switch (+ an optional API key) to settings.json. The
    /// app loop then reloads the session to apply it.
    async fn set_provider(&self, provider: &str, api_key: Option<String>) -> anyhow::Result<()>;

    /// Configured remote-instance names (for the `/remote` picker + tab bar).
    fn remotes(&self) -> Vec<String> {
        Vec::new()
    }

    /// Attach to a remote instance by name, returning a proxying session handle.
    async fn connect_remote(&self, _name: &str) -> anyhow::Result<SessionHandle> {
        anyhow::bail!("remote instances are not supported by this host")
    }

    /// Open a project workspace rooted at `path` as a fresh session (its own
    /// working dir + that project's config/skills).
    async fn open_workspace(&self, _path: &str) -> anyhow::Result<SessionHandle> {
        anyhow::bail!("workspace switching is not supported by this host")
    }

    /// Project workspaces for the left sidebar (recent + pinned + scanned).
    fn workspaces(&self) -> Vec<crate::Workspace> {
        Vec::new()
    }
}

/// Everything the TUI needs besides the session handle.
pub struct TuiConfig {
    pub model_name: String,
    pub working_dir: String,
    pub memory_md: PathBuf,
    pub user_md: PathBuf,
    /// Available skills (name, description) for `/skills`.
    pub skills: Vec<(String, String)>,
    /// Recent sessions (id, title) for the dashboard + `/sessions`.
    pub recent_sessions: Vec<(String, String)>,
    /// Available personas (name, description) for `/persona`.
    pub personas: Vec<(String, String)>,
    /// The active persona name.
    pub persona: String,
    /// Directory `/export` writes transcripts into.
    pub export_dir: PathBuf,
    /// Model context window size (for the usage bar).
    pub context_size: u32,
    /// Scheduled cron jobs (name, schedule) for `/cron`.
    pub cron_jobs: Vec<(String, String)>,
    /// Path to the persistent task board (for the `/board` overlay).
    pub tasks_path: PathBuf,
    /// Initial brain approval mode label ("off"/"advisory"/"auto").
    pub brain_mode: String,
    /// Auto-continue step budget (dashboard display + `/autocontinue` default).
    pub auto_continue: u32,
    /// User themes loaded from ~/.blumi/themes/*.toml (appended to the built-ins).
    pub themes: Vec<crate::theme::Theme>,
}

/// Run the interactive TUI, sourcing sessions from `factory`. Restores the
/// terminal on exit (including on error).
pub async fn run(factory: Arc<dyn SessionFactory>, cfg: TuiConfig) -> anyhow::Result<()> {
    crate::theme::init_fill_from_env();
    crate::icons::init_from_env();
    let mut terminal = setup_terminal()?;
    let result = run_loop(&mut terminal, factory, cfg).await;
    let _ = teardown_terminal(&mut terminal);
    result
}

pub(crate) fn setup_terminal() -> anyhow::Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableFocusChange,
        EnableMouseCapture
    )?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

pub(crate) fn teardown_terminal(terminal: &mut Term) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run_loop(
    terminal: &mut Term,
    factory: Arc<dyn SessionFactory>,
    cfg: TuiConfig,
) -> anyhow::Result<()> {
    let mut model = Model::new(cfg.model_name, cfg.working_dir);
    model.memory_md = cfg.memory_md;
    model.user_md = cfg.user_md;
    model.skills = cfg.skills;
    model.recent_sessions = cfg.recent_sessions;
    model.personas = cfg.personas;
    model.persona = cfg.persona;
    model.export_dir = cfg.export_dir;
    model.context_size = cfg.context_size;
    model.cron_jobs = cfg.cron_jobs;
    model.tasks_path = cfg.tasks_path;
    model.brain_mode = cfg.brain_mode;
    model.auto_continue = cfg.auto_continue;
    // Built-in palettes + any user themes from ~/.blumi/themes; re-sync the active
    // theme in case a user theme overrides the default (rose) at index 0.
    model.themes = crate::theme::ThemeRegistry::builtin().with_user(cfg.themes);
    model.theme = model.themes.get(model.theme_idx);

    // Cinematic motion: honor env switches, then play a launch "scene-in".
    model.motion = crate::motion::Motion::from_env();
    model.motion.scene_in();
    model.mark_dirty();

    let mut session = factory.create().await?;
    let mut events = session.subscribe();
    model.model_options = factory.model_options();
    model.remotes = factory.remotes();
    model.workspaces = factory.workspaces();
    // Open tabs: one live handle + the saved transcript per tab, parallel to
    // `model.tabs`. Index 0 is the local session; the active tab's transcript
    // lives in `model.entries`, inactive tabs' in `tab_views`.
    let mut handles: Vec<SessionHandle> = vec![session.clone()];
    let mut tab_views: Vec<Vec<Entry>> = vec![Vec::new()];
    let mut input = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50)); // ~20fps

    // On launch, if there's history, offer the session picker (resume where you
    // left off after a crash/kill, or esc for a fresh one).
    if !model.recent_sessions.is_empty() || !model.remotes.is_empty() {
        model.dialog = Some(Picker::session_picker(
            &model.recent_sessions,
            &model.remotes,
        ));
    }

    // Tracks the busy→idle edge so we persist (crash recovery) and consider a
    // context rollover exactly once per finished turn.
    let mut was_busy = false;

    // Background jobs (`/bg`) run on detached tasks and report completion here.
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel::<BgUpdate>();

    terminal.draw(|f| view::render(&mut model, f))?;

    loop {
        if model.should_quit {
            break;
        }

        let msg = tokio::select! {
            ev = input.next() => match ev {
                Some(Ok(ev)) => Some(Msg::Term(ev)),
                Some(Err(_)) => None,
                None => { model.should_quit = true; None }
            },
            env = events.recv() => match env {
                Ok(env) => Some(Msg::Core(env)),
                Err(_) => None, // lagged or closed; keep going
            },
            Some(u) = bg_rx.recv() => Some(Msg::Bg(u)),
            _ = tick.tick() => Some(Msg::Tick),
        };

        let Some(msg) = msg else { continue };
        update::update(&mut model, msg, &session).await;

        // A command may have requested a session switch (new/resume local, or a
        // remote tab, or switching between open tabs). The loop owns the handles
        // + subscription, so it performs the swap here.
        if let Some(req) = model.take_session_request() {
            match req {
                // New / Resume always operate on the local tab (index 0).
                SessionRequest::New | SessionRequest::Resume(_) => {
                    factory.save(&handles[0]).await;
                    let next = match &req {
                        SessionRequest::Resume(id) => factory.resume(id).await,
                        _ => factory.create().await,
                    };
                    match next {
                        Ok(handle) => {
                            // Drop any non-local tabs' stashed views back to local focus.
                            if model.active_tab != 0 {
                                tab_views[model.active_tab] = std::mem::take(&mut model.entries);
                            }
                            handles[0] = handle.clone();
                            tab_views[0].clear();
                            model.active_tab = 0;
                            model.busy = false;
                            model.pending = None;
                            if let SessionRequest::Resume(_) = req {
                                model.load_snapshot(handle.snapshot().await);
                            } else {
                                model.reset_for_session();
                            }
                            session = handle;
                            events = session.subscribe();
                            model.recent_sessions = factory.list().await;
                        }
                        Err(e) => model
                            .entries
                            .push(Entry::Notice(format!("session switch failed: {e}"))),
                    }
                }
                // Attach to an existing remote tab, or open a new one.
                SessionRequest::Remote(name) => {
                    if let Some(i) = model.tabs.iter().position(|(n, _)| *n == name) {
                        switch_tab(
                            &mut model,
                            &mut session,
                            &mut events,
                            &handles,
                            &mut tab_views,
                            i,
                        );
                    } else {
                        match factory.connect_remote(&name).await {
                            Ok(handle) => {
                                tab_views[model.active_tab] = std::mem::take(&mut model.entries);
                                handles.push(handle.clone());
                                tab_views.push(Vec::new());
                                model.tabs.push((name.clone(), true));
                                model.active_tab = handles.len() - 1;
                                model.busy = false;
                                model.pending = None;
                                model.scrollback = 0;
                                model.entries.push(Entry::Notice(format!(
                                    "⇆ attached to remote '{name}' — /remote local to return"
                                )));
                                session = handle;
                                events = session.subscribe();
                            }
                            Err(e) => model
                                .entries
                                .push(Entry::Notice(format!("remote attach failed: {e}"))),
                        }
                    }
                }
                // Open a project workspace as a new local tab (or switch if open).
                SessionRequest::OpenWorkspace(path) => {
                    let label = workspace_label(&path);
                    if let Some(i) = model.tabs.iter().position(|(n, _)| n == &label) {
                        switch_tab(
                            &mut model,
                            &mut session,
                            &mut events,
                            &handles,
                            &mut tab_views,
                            i,
                        );
                    } else {
                        match factory.open_workspace(&path).await {
                            Ok(handle) => {
                                tab_views[model.active_tab] = std::mem::take(&mut model.entries);
                                handles.push(handle.clone());
                                tab_views.push(Vec::new());
                                model.tabs.push((label.clone(), false));
                                model.active_tab = handles.len() - 1;
                                model.reset_for_session();
                                model.working_dir = path.clone();
                                model.busy = false;
                                model.pending = None;
                                model.scrollback = 0;
                                model
                                    .entries
                                    .push(Entry::Notice(format!("◳ workspace '{label}' — {path}")));
                                session = handle;
                                events = session.subscribe();
                            }
                            Err(e) => model
                                .entries
                                .push(Entry::Notice(format!("open workspace failed: {e}"))),
                        }
                    }
                }
                SessionRequest::SwitchTab(i) => {
                    switch_tab(
                        &mut model,
                        &mut session,
                        &mut events,
                        &handles,
                        &mut tab_views,
                        i,
                    );
                }
            }
            model.mark_dirty();
        }

        // Background job: spawn a detached, unattended (yolo) session that runs
        // concurrently; its result is posted back over `bg_tx` and dropped into
        // the transcript when done. The foreground stays fully usable.
        if let Some(prompt) = model.take_bg_request() {
            match factory.create_background().await {
                Ok(handle) => {
                    model.bg_seq += 1;
                    model.bg_count += 1;
                    let id = format!("bg{}", model.bg_seq);
                    model.entries.push(Entry::Notice(format!(
                        "⬢ {id} started in the background: {prompt}"
                    )));
                    let tx = bg_tx.clone();
                    let fac = factory.clone();
                    tokio::spawn(async move {
                        let mut rx = handle.subscribe();
                        let _ = handle
                            .send(Command::UserMessage {
                                text: prompt,
                                attachments: vec![],
                                stream_id: None,
                            })
                            .await;
                        let mut ok = true;
                        loop {
                            match rx.recv().await {
                                Ok(env) => match env.event {
                                    Event::TurnDone { .. } => break,
                                    Event::Error { .. } => {
                                        ok = false;
                                        break;
                                    }
                                    _ => {}
                                },
                                Err(_) => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        let snap = handle.snapshot().await;
                        let text = snap
                            .messages
                            .iter()
                            .rev()
                            .find(|m| m.role == Role::Assistant && !m.text().trim().is_empty())
                            .map(|m| m.text())
                            .unwrap_or_default();
                        fac.save(&handle).await;
                        let _ = tx.send(BgUpdate { id, text, ok });
                    });
                    model.mark_dirty();
                }
                Err(e) => model
                    .entries
                    .push(Entry::Notice(format!("background job failed: {e}"))),
            }
        }

        // Self-evolution: the agent asked to reload. We wait until the turn is
        // idle, then rebuild the session (fresh config + skills) seeded with the
        // live snapshot so the conversation is preserved — the transcript stays.
        if !model.busy {
            if let Some(reason) = model.reload_pending.take() {
                let snapshot = session.snapshot().await;
                factory.save(&session).await;
                match factory.reload(snapshot).await {
                    Ok(handle) => {
                        session = handle;
                        events = session.subscribe();
                        model.model_options = factory.model_options();
                        model.recent_sessions = factory.list().await;
                        model.entries.push(Entry::Notice(format!(
                            "✿ reloaded — skills + config refreshed ({reason})"
                        )));
                    }
                    Err(e) => model
                        .entries
                        .push(Entry::Notice(format!("reload failed: {e}"))),
                }
                model.mark_dirty();
            }
        }

        // Provider switch: persist the choice (+ key), then rebuild the session
        // with the new provider's client, keeping the conversation.
        if !model.busy {
            if let Some((provider, key)) = model.take_provider_request() {
                match factory.set_provider(&provider, key).await {
                    Ok(()) => {
                        let snapshot = session.snapshot().await;
                        factory.save(&session).await;
                        match factory.reload(snapshot).await {
                            Ok(handle) => {
                                session = handle;
                                events = session.subscribe();
                                model.model_options = factory.model_options();
                                model.model_name = model.model_options.model.clone();
                                model
                                    .entries
                                    .push(Entry::Notice(format!("✿ provider → {provider}")));
                            }
                            Err(e) => model
                                .entries
                                .push(Entry::Notice(format!("provider switch failed: {e}"))),
                        }
                    }
                    Err(e) => model
                        .entries
                        .push(Entry::Notice(format!("provider switch failed: {e}"))),
                }
                model.mark_dirty();
            }
        }

        // Turn just finished (busy→idle): persist for crash recovery, and roll
        // over to a fresh session if the context window is (near) full.
        if was_busy && !model.busy {
            let is_remote = model
                .tabs
                .get(model.active_tab)
                .map(|(_, r)| *r)
                .unwrap_or(false);
            if !is_remote {
                factory.save(&session).await;
            }
            // Only the local tab rolls over (workspace/remote tabs are left as-is).
            if model.active_tab == 0 && !is_remote && model.context_frac() >= ROLLOVER_THRESHOLD {
                let snapshot = session.snapshot().await;
                match factory.rollover(snapshot).await {
                    Ok(handle) => {
                        handles[0] = handle.clone();
                        session = handle;
                        events = session.subscribe();
                        model.context_tokens = 0; // fresh window
                        model.recent_sessions = factory.list().await;
                        model.entries.push(Entry::Notice(
                            "↻ context window full — rolled over to a fresh session \
                             (history carried forward)"
                                .into(),
                        ));
                        model.mark_dirty();
                    }
                    Err(e) => model
                        .entries
                        .push(Entry::Notice(format!("rollover failed: {e}"))),
                }
            }
        }
        was_busy = model.busy;

        // Autonomous loop: when idle, advance the task board (ralph-style).
        advance_loop(&mut model, &session).await;

        if model.take_dirty() {
            terminal.draw(|f| view::render(&mut model, f))?;
        }
    }

    factory.save(&session).await; // persist on exit
    Ok(())
}

/// Switch the active tab to `i`, preserving each tab's transcript: the leaving
/// tab's `entries` are stashed and the entering tab's restored, then the live
/// A short tab label for a workspace path (its final path component).
fn workspace_label(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// handle + subscription are repointed.
fn switch_tab(
    model: &mut Model,
    session: &mut SessionHandle,
    events: &mut broadcast::Receiver<Envelope>,
    handles: &[SessionHandle],
    tab_views: &mut [Vec<Entry>],
    i: usize,
) {
    if i >= handles.len() || i == model.active_tab {
        return;
    }
    tab_views[model.active_tab] = std::mem::take(&mut model.entries);
    model.entries = std::mem::take(&mut tab_views[i]);
    model.active_tab = i;
    model.busy = false;
    model.pending = None;
    model.scrollback = 0;
    *session = handles[i].clone();
    *events = session.subscribe();
    let label = model.tabs[i].0.clone();
    model
        .entries
        .push(Entry::Notice(format!("⇆ switched to {label}")));
}

/// Drive the in-TUI autonomous loop: when a turn finishes, advance the current
/// task and dispatch the next highest-priority todo as a fresh turn.
async fn advance_loop(model: &mut Model, session: &SessionHandle) {
    if !model.loop_active
        || model.busy
        || model.pending.is_some()
        || model.provider_key_prompt.is_some()
        || model.dialog.is_some()
    {
        return;
    }
    let mut board = TaskBoard::load(&model.tasks_path);

    // The previous loop task just finished — advance it.
    if let Some((id, title)) = model.loop_current.take() {
        let next = if model.loop_review {
            TaskState::Review
        } else {
            TaskState::Done
        };
        board.set_state_now(&id, next);
        board.save().ok();
        model
            .entries
            .push(Entry::Notice(format!("{} {title}", next.icon())));
    }

    // Pick the next todo, or finish.
    let Some(task) = board.next_todo().cloned() else {
        model.loop_active = false;
        model.entries.push(Entry::Notice(format!(
            "✿ loop complete — {} iterations",
            model.loop_iter
        )));
        model.mark_dirty();
        return;
    };
    board.set_state_now(&task.id, TaskState::Doing);
    board.save().ok();
    model.loop_iter += 1;
    model.loop_current = Some((task.id.clone(), task.title.clone()));

    let prompt = if task.detail.trim().is_empty() {
        task.title.clone()
    } else {
        format!("{}\n\n{}", task.title, task.detail)
    };
    model.entries.push(Entry::User(format!(
        "▶ [loop {}] {}",
        model.loop_iter, task.title
    )));
    model.busy = true;
    model.busy_since = Some(std::time::Instant::now());
    model.scrollback = 0;
    let _ = session
        .send(Command::UserMessage {
            text: prompt,
            attachments: vec![],
            stream_id: None,
        })
        .await;
    model.mark_dirty();
}

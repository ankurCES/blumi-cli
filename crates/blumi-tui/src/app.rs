//! The TUI event loop: terminal setup/teardown and the select loop bridging
//! crossterm input, the core's event stream, and an animation tick.

use crate::model::{Entry, Model, Msg, SessionRequest};
use crate::{update, view};
use blumi_core::SessionHandle;
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

pub(crate) type Term = Terminal<CrosstermBackend<Stdout>>;

/// Creates, resumes, lists, and saves sessions on the TUI's behalf. The binary
/// implements this over `build_session` + the persistence store — the seam that
/// lets the TUI switch live sessions without knowing how they're wired.
#[async_trait::async_trait]
pub trait SessionFactory: Send + Sync {
    /// Spawn a fresh session.
    async fn create(&self) -> anyhow::Result<SessionHandle>;
    /// Resume a stored session by id, seeded with its prior messages.
    async fn resume(&self, id: &str) -> anyhow::Result<SessionHandle>;
    /// Rebuild the agent in place (self-evolution): re-read config + re-scan
    /// skills, seeded with the live snapshot so the conversation is preserved.
    async fn reload(&self, snapshot: blumi_core::SessionSnapshot) -> anyhow::Result<SessionHandle>;
    /// Recent sessions as (id, title), newest first.
    async fn list(&self) -> Vec<(String, String)>;
    /// Persist the given session (best-effort).
    async fn save(&self, handle: &SessionHandle);
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
}

/// Run the interactive TUI, sourcing sessions from `factory`. Restores the
/// terminal on exit (including on error).
pub async fn run(factory: Arc<dyn SessionFactory>, cfg: TuiConfig) -> anyhow::Result<()> {
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

    let mut session = factory.create().await?;
    let mut events = session.subscribe();
    let mut input = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50)); // ~20fps

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
            _ = tick.tick() => Some(Msg::Tick),
        };

        let Some(msg) = msg else { continue };
        update::update(&mut model, msg, &session).await;

        // A command may have requested a new/resumed session — the loop owns the
        // handle + subscription, so we swap them here (saving the current one).
        if let Some(req) = model.take_session_request() {
            factory.save(&session).await;
            let next = match &req {
                SessionRequest::New => factory.create().await,
                SessionRequest::Resume(id) => factory.resume(id).await,
            };
            match next {
                Ok(handle) => {
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
            model.mark_dirty();
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

        if model.take_dirty() {
            terminal.draw(|f| view::render(&mut model, f))?;
        }
    }

    factory.save(&session).await; // persist on exit
    Ok(())
}

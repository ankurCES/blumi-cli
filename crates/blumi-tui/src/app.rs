//! The TUI event loop: terminal setup/teardown and the select loop bridging
//! crossterm input, the core's event stream, and an animation tick.

use crate::model::{Entry, Model, Msg, SessionRequest};
use crate::{update, view};
use blumi_core::SessionHandle;
use blumi_protocol::Command;
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

pub(crate) type Term = Terminal<CrosstermBackend<Stdout>>;

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
    /// Resume a stored session by id, seeded with its prior messages.
    async fn resume(&self, id: &str) -> anyhow::Result<SessionHandle>;
    /// Rebuild the agent in place (self-evolution): re-read config + re-scan
    /// skills, seeded with the live snapshot so the conversation is preserved.
    async fn reload(&self, snapshot: blumi_core::SessionSnapshot) -> anyhow::Result<SessionHandle>;
    /// Recent sessions as (id, title), newest first.
    async fn list(&self) -> Vec<(String, String)>;
    /// Persist the given session (best-effort).
    async fn save(&self, handle: &SessionHandle);
    /// Active provider/model + suggestions + selectable providers (read live).
    fn model_options(&self) -> ModelOptions;
    /// Persist a provider switch (+ an optional API key) to settings.json. The
    /// app loop then reloads the session to apply it.
    async fn set_provider(&self, provider: &str, api_key: Option<String>) -> anyhow::Result<()>;
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
    model.tasks_path = cfg.tasks_path;

    let mut session = factory.create().await?;
    let mut events = session.subscribe();
    model.model_options = factory.model_options();
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

        // Autonomous loop: when idle, advance the task board (ralph-style).
        advance_loop(&mut model, &session).await;

        if model.take_dirty() {
            terminal.draw(|f| view::render(&mut model, f))?;
        }
    }

    factory.save(&session).await; // persist on exit
    Ok(())
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

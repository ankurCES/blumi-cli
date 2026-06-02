//! The TUI event loop: terminal setup/teardown and the select loop bridging
//! crossterm input, the core's event stream, and an animation tick.

use crate::model::{Model, Msg};
use crate::{update, view};
use blumi_core::SessionHandle;
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, EnableFocusChange, EventStream,
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
use std::time::Duration;

pub(crate) type Term = Terminal<CrosstermBackend<Stdout>>;

/// Everything the TUI needs besides the session handle.
pub struct TuiConfig {
    pub model_name: String,
    pub working_dir: String,
    pub memory_md: PathBuf,
    pub user_md: PathBuf,
    /// Available skills (name, description) for `/skills`.
    pub skills: Vec<(String, String)>,
}

/// Run the interactive TUI against an already-spawned session. Restores the
/// terminal on exit (including on error).
pub async fn run(session: SessionHandle, cfg: TuiConfig) -> anyhow::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run_loop(&mut terminal, session, cfg).await;
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
        EnableFocusChange
    )?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

pub(crate) fn teardown_terminal(terminal: &mut Term) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run_loop(
    terminal: &mut Term,
    session: SessionHandle,
    cfg: TuiConfig,
) -> anyhow::Result<()> {
    let mut model = Model::new(cfg.model_name, cfg.working_dir);
    model.memory_md = cfg.memory_md;
    model.user_md = cfg.user_md;
    model.skills = cfg.skills;
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

        if model.take_dirty() {
            terminal.draw(|f| view::render(&mut model, f))?;
        }
    }

    Ok(())
}

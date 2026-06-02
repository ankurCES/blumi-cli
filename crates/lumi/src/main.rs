//! lumi — the CLI entry point.

mod prompt;
mod run;
mod session;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "lumi",
    version,
    about = "Local-first, provider-agnostic agentic coding assistant"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Override the provider (e.g. anthropic, openai, local, mock).
    #[arg(long, global = true)]
    provider: Option<String>,

    /// Override the model id.
    #[arg(long, global = true)]
    model: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a single prompt headlessly, streaming the result to stdout.
    Run {
        /// The prompt (read from stdin if omitted).
        prompt: Vec<String>,
        /// Auto-approve tool calls (otherwise denied in headless mode).
        #[arg(long)]
        yolo: bool,
    },
    /// Interactive terminal UI (Phase 2).
    Tui,
    /// Web UI + server (Phase 4).
    Web,
    /// List, search, and show stored sessions.
    Session {
        #[command(subcommand)]
        action: SessionCmd,
    },
}

#[derive(Subcommand)]
enum SessionCmd {
    /// List recent sessions.
    List,
    /// Full-text search across sessions.
    Search { query: Vec<String> },
    /// Print a session's transcript.
    Show { id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let working_dir = std::env::current_dir()?;
    let home_override = std::env::var("LUMI_HOME").ok().map(PathBuf::from);
    let mut config = lumi_config::LumiConfig::load(&working_dir, home_override)?;
    if let Some(p) = cli.provider {
        config.llm.provider = p;
    }
    if let Some(m) = cli.model {
        config.llm.model = m;
    }

    match cli.command {
        Some(Commands::Run { prompt, yolo }) => run::run(config, prompt.join(" "), yolo).await,
        Some(Commands::Tui) | None => {
            eprintln!("The interactive TUI lands in Phase 2. For now: lumi run \"<prompt>\"");
            Ok(())
        }
        Some(Commands::Web) => {
            eprintln!("The web UI lands in Phase 4. For now: lumi run \"<prompt>\"");
            Ok(())
        }
        Some(Commands::Session { action }) => match action {
            SessionCmd::List => session::list(config).await,
            SessionCmd::Search { query } => session::search(config, query.join(" ")).await,
            SessionCmd::Show { id } => session::show(config, id).await,
        },
    }
}

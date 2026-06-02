//! blumi — the CLI entry point.

mod branding;
mod cron;
mod engine;
mod onboarding;
mod prompt;
mod run;
mod session;
mod tui;
mod web;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "blumi",
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

    /// Start with a named agent persona (e.g. architect, pair, reviewer).
    #[arg(long, global = true)]
    persona: Option<String>,

    /// Execution sandbox: "local" (host) or "docker" (container).
    #[arg(long, global = true)]
    sandbox: Option<String>,
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
    /// Interactive terminal UI.
    Tui,
    /// Run the setup wizard (pick provider, enter key/endpoint, choose model).
    Login,
    /// Launch the embedded web UI + HTTP/SSE server.
    Web,
    /// List, search, and show stored sessions.
    Session {
        #[command(subcommand)]
        action: SessionCmd,
    },
    /// Schedule prompts to run on a timer (cron automations).
    Cron {
        #[command(subcommand)]
        action: CronCmd,
    },
}

#[derive(Subcommand)]
enum CronCmd {
    /// Add a scheduled job.
    Add {
        /// A short name for the job.
        #[arg(long)]
        name: String,
        /// Schedule: "every 1h", "hourly:15", "daily 09:00", or an RFC3339 time.
        #[arg(long)]
        schedule: String,
        /// The prompt to run.
        #[arg(long)]
        prompt: String,
        /// Where to deliver output: "log" (default) or "file:<path>".
        #[arg(long)]
        deliver: Option<String>,
    },
    /// List scheduled jobs.
    List,
    /// Remove a job by id or name.
    Rm { id: String },
    /// Run due jobs now; pass --watch to keep running.
    Run {
        #[arg(long)]
        watch: bool,
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
    let provider_flag = cli.provider.is_some();
    let is_bare = cli.command.is_none();

    let working_dir = std::env::current_dir()?;
    let home_override = std::env::var("BLUMI_HOME").ok().map(PathBuf::from);
    let mut config = blumi_config::BlumiConfig::load(&working_dir, home_override)?;
    if let Some(p) = cli.provider {
        config.llm.provider = p;
    }
    if let Some(m) = cli.model {
        config.llm.model = m;
    }
    if let Some(p) = cli.persona {
        config.persona = p;
    }
    if let Some(s) = cli.sandbox {
        config.executor.backend = s;
    }

    match cli.command {
        Some(Commands::Run { prompt, yolo }) => run::run(config, prompt.join(" "), yolo).await,
        Some(Commands::Login) => {
            match onboarding::ensure_configured(config, true).await? {
                Some(_) => {
                    branding::greeting();
                    eprintln!("  ✓ saved to ~/.blumi/settings.json — run `blumi` to start.");
                }
                None => eprintln!("onboarding cancelled."),
            }
            Ok(())
        }
        Some(Commands::Tui) | None => {
            use std::io::IsTerminal;
            if !std::io::stdout().is_terminal() {
                // Non-interactive (piped): show the static banner instead of a TUI.
                branding::banner();
                return Ok(());
            }
            // Bare `blumi` shows the animated rose splash; `blumi tui` skips it.
            if is_bare {
                branding::greeting();
            }
            // First-run onboarding, unless a provider was passed explicitly.
            let config = if config.is_first_run() && !provider_flag {
                match onboarding::ensure_configured(config, false).await? {
                    Some(c) => c,
                    None => return Ok(()), // cancelled
                }
            } else {
                config
            };
            tui::run(config).await
        }
        Some(Commands::Web) => web::run(config).await,
        Some(Commands::Session { action }) => match action {
            SessionCmd::List => session::list(config).await,
            SessionCmd::Search { query } => session::search(config, query.join(" ")).await,
            SessionCmd::Show { id } => session::show(config, id).await,
        },
        Some(Commands::Cron { action }) => match action {
            CronCmd::Add {
                name,
                schedule,
                prompt,
                deliver,
            } => cron::add(config, name, schedule, prompt, deliver),
            CronCmd::List => cron::list(config),
            CronCmd::Rm { id } => cron::remove(config, id),
            CronCmd::Run { watch } => cron::run(config, watch).await,
        },
    }
}

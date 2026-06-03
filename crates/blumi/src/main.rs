//! blumi — the CLI entry point.

mod branding;
mod cron;
mod engine;
mod gateway;
mod loop_run;
mod mcp;
mod onboarding;
mod playbook;
mod prompt;
mod providers;
mod remote;
mod run;
mod session;
mod task;
mod tui;
mod web;
mod workspace;

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
        /// Local-LLM approval brain mode: off | advisory | auto.
        #[arg(long)]
        brain: Option<String>,
    },
    /// Interactive terminal UI.
    Tui,
    /// Run the setup wizard (pick provider, enter key/endpoint, choose model).
    Login,
    /// Launch the embedded web UI + HTTP/SSE server.
    Web {
        /// Bind address (default 127.0.0.1). A non-loopback host requires a password.
        #[arg(long)]
        host: Option<String>,
        /// Set/replace the login password (hashed + saved; enables auth).
        #[arg(long)]
        password: Option<String>,
    },
    /// List, search, and show stored sessions.
    Session {
        #[command(subcommand)]
        action: SessionCmd,
    },
    /// Aggregate token usage across stored sessions.
    Stats,
    /// Schedule prompts to run on a timer (cron automations).
    Cron {
        #[command(subcommand)]
        action: CronCmd,
    },
    /// Run a YAML playbook (multi-step workflow with gates + resume).
    Playbook {
        #[command(subcommand)]
        action: PlaybookCmd,
    },
    /// Run blumi as a messaging bot (Telegram/Discord/Slack/WhatsApp).
    Gateway {
        #[command(subcommand)]
        action: GatewayCmd,
    },
    /// Manage the task board (the work queue for `blumi loop`).
    Task {
        #[command(subcommand)]
        action: TaskCmd,
    },
    /// Manage skills (the bundled SKILL.md library + your own).
    Skills {
        #[command(subcommand)]
        action: SkillsCmd,
    },
    /// Manage MCP servers (defaults + a catalog of configurable ones).
    Mcp {
        #[command(subcommand)]
        action: McpCmd,
    },
    /// Autonomously work the task board: select → run → advance, repeat.
    Loop {
        /// Stop after at most N iterations.
        #[arg(long)]
        max: Option<u32>,
        /// Stop once reported cost (USD) reaches this (provider-dependent).
        #[arg(long)]
        budget: Option<f64>,
        /// Auto-approve tool calls (otherwise approval-requiring tools are denied).
        #[arg(long)]
        yolo: bool,
        /// Send finished tasks to "review" instead of "done".
        #[arg(long)]
        review: bool,
        /// Desktop notification when the loop finishes.
        #[arg(long)]
        notify: bool,
        /// Local-LLM approval brain mode: off | advisory | auto.
        #[arg(long)]
        brain: Option<String>,
    },
}

#[derive(Subcommand)]
enum TaskCmd {
    /// Add a task to the board.
    Add {
        /// The task title.
        title: Vec<String>,
        /// Priority 1 (highest) .. 4 (lowest).
        #[arg(long, short, default_value_t = 3)]
        priority: u8,
        /// Optional longer detail for the agent.
        #[arg(long)]
        detail: Option<String>,
    },
    /// List the board with status + counts.
    List,
    /// Mark a task in-progress (todo → doing).
    Start { id: String },
    /// Mark a task for review (doing → review).
    Review { id: String },
    /// Mark a task done.
    Done { id: String },
    /// Cancel a task.
    Cancel { id: String },
    /// Remove a task from the board.
    Rm { id: String },
}

#[derive(Subcommand)]
enum SkillsCmd {
    /// List discovered skills (bundled + your own).
    List,
    /// Re-materialize the bundled skills into ~/.blumi/skills (restore/refresh).
    Sync,
}

#[derive(Subcommand)]
enum McpCmd {
    /// List configured MCP servers (and whether each is enabled).
    List,
    /// Show the catalog of configurable (keyed) servers you can add.
    Catalog,
    /// Seed the default no-config servers into settings.json.
    Defaults,
    /// Add a server from the catalog by name.
    Add { name: String },
    /// Enable a configured server.
    Enable { name: String },
    /// Disable a configured server (keeps its config).
    Disable { name: String },
    /// Remove a configured server.
    Remove { name: String },
}

#[derive(Subcommand)]
enum GatewayCmd {
    /// Telegram bot (long-poll; needs a @BotFather token).
    Telegram {
        /// Bot token (overrides gateway.telegram.token in settings.json).
        #[arg(long)]
        token: Option<String>,
    },
    /// Discord bot (Gateway WebSocket; needs the MESSAGE CONTENT intent on).
    Discord {
        /// Bot token (overrides gateway.discord.token in settings.json).
        #[arg(long)]
        token: Option<String>,
    },
    /// Slack bot (Socket Mode; needs a bot token + an app-level token).
    Slack {
        /// Bot token `xoxb-…` (overrides gateway.slack.bot_token).
        #[arg(long)]
        bot_token: Option<String>,
        /// App-level token `xapp-…` (overrides gateway.slack.app_token).
        #[arg(long)]
        app_token: Option<String>,
    },
    /// WhatsApp Cloud API bot (runs an inbound webhook server).
    Whatsapp {
        /// Webhook port (overrides gateway.whatsapp.webhook_port; default 8080).
        #[arg(long)]
        port: Option<u16>,
    },
}

#[derive(Subcommand)]
enum PlaybookCmd {
    /// Run a playbook file (resumes after the last completed step).
    Run {
        /// Path to the playbook .yaml.
        file: PathBuf,
        /// Ignore saved progress and run every step.
        #[arg(long)]
        restart: bool,
    },
    /// List playbooks found under ~/.blumi/playbooks and .blumi/playbooks.
    List,
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

    // Pre-bundled skills: materialize the binary's bundled SKILL.md collections
    // into ~/.blumi/skills on first run (idempotent; never clobbers user skills).
    config.paths.ensure_dirs().ok();
    if let Err(e) = blumi_skills::sync_bundled_skills(&config.paths.skills, false) {
        tracing::warn!("could not sync bundled skills: {e}");
    }

    match cli.command {
        Some(Commands::Run {
            prompt,
            yolo,
            brain,
        }) => {
            let mut config = config;
            if let Some(b) = brain {
                config.brain.mode = b;
            }
            run::run(config, prompt.join(" "), yolo).await
        }
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
        Some(Commands::Web { host, password }) => web::run(config, host, password).await,
        Some(Commands::Session { action }) => match action {
            SessionCmd::List => session::list(config).await,
            SessionCmd::Search { query } => session::search(config, query.join(" ")).await,
            SessionCmd::Show { id } => session::show(config, id).await,
        },
        Some(Commands::Stats) => session::stats(config).await,
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
        Some(Commands::Playbook { action }) => match action {
            PlaybookCmd::Run { file, restart } => playbook::run(config, file, restart).await,
            PlaybookCmd::List => playbook::list(config),
        },
        Some(Commands::Gateway { action }) => match action {
            GatewayCmd::Telegram { token } => gateway::run_telegram(config, token).await,
            GatewayCmd::Discord { token } => gateway::run_discord(config, token).await,
            GatewayCmd::Slack {
                bot_token,
                app_token,
            } => gateway::run_slack(config, bot_token, app_token).await,
            GatewayCmd::Whatsapp { port } => gateway::run_whatsapp(config, port).await,
        },
        Some(Commands::Task { action }) => match action {
            TaskCmd::Add {
                title,
                priority,
                detail,
            } => task::add(config, title.join(" "), priority, detail),
            TaskCmd::List => task::list(config),
            TaskCmd::Start { id } => task::transition(config, id, blumi_task::TaskState::Doing),
            TaskCmd::Review { id } => task::transition(config, id, blumi_task::TaskState::Review),
            TaskCmd::Done { id } => task::transition(config, id, blumi_task::TaskState::Done),
            TaskCmd::Cancel { id } => {
                task::transition(config, id, blumi_task::TaskState::Cancelled)
            }
            TaskCmd::Rm { id } => task::remove(config, id),
        },
        Some(Commands::Skills { action }) => match action {
            SkillsCmd::Sync => {
                let n = blumi_skills::sync_bundled_skills(&config.paths.skills, true)?;
                println!(
                    "✿ synced {n} bundled skills → {}",
                    config.paths.skills.display()
                );
                Ok(())
            }
            SkillsCmd::List => {
                let dirs = [
                    config.paths.skills.clone(),
                    working_dir.join(".blumi").join("skills"),
                ];
                for m in blumi_skills::SkillCatalog::load(&dirs).list() {
                    let desc = m.description.lines().next().unwrap_or("");
                    println!("{:<30} {desc}", m.name);
                }
                Ok(())
            }
        },
        Some(Commands::Mcp { action }) => mcp::run(action, &config),
        Some(Commands::Loop {
            max,
            budget,
            yolo,
            review,
            notify,
            brain,
        }) => {
            let mut config = config;
            if let Some(b) = brain {
                config.brain.mode = b;
            }
            loop_run::run(
                config,
                loop_run::LoopOptions {
                    max,
                    budget,
                    yolo,
                    review,
                    notify,
                },
            )
            .await
        }
    }
}

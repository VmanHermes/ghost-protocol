use clap::{Parser, Subcommand};

mod commands;
mod configure;
mod detect;
mod setup;

#[derive(Parser)]
#[command(
    name = "ghost",
    about = "Ghost Protocol CLI — multi-machine AI agent control plane",
    after_help = "\x1b[2mQuick start:\n\
  ghost status          Mesh overview (machines, sessions)\n\
  ghost agents          Available agents across the mesh\n\
  ghost chat <agent>    Start a chat with an agent\n\
  ghost configure       Customize project settings (optional)\n\
  ghost setup claude    Configure Claude API key for this machine\n\
\n\
  Documentation: ghost help <command>\x1b[0m"
)]
struct Cli {
    #[arg(
        long,
        env = "GHOST_DAEMON_URL",
        default_value = "http://127.0.0.1:8787"
    )]
    daemon_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Customize project settings for the current directory
    Configure,
    /// Configure machine-level Ghost integrations
    Setup {
        #[command(subcommand)]
        target: SetupCommands,
    },
    /// Show mesh status
    Status,
    /// List available agents
    Agents,
    /// List registered projects
    Projects,
    /// Start a chat with an agent
    Chat {
        /// Agent ID (e.g., hermes, ollama:gemma4)
        agent: String,
    },
}

#[derive(Subcommand)]
enum SetupCommands {
    /// Configure Ghost-managed Claude Code auth for this machine
    Claude,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Configure => configure::run(&cli.daemon_url).await,
        Commands::Setup { target } => match target {
            SetupCommands::Claude => setup::run_claude().await,
        },
        Commands::Status => commands::status(&cli.daemon_url).await,
        Commands::Agents => commands::agents(&cli.daemon_url).await,
        Commands::Projects => commands::projects(&cli.daemon_url).await,
        Commands::Chat { agent } => commands::chat(&cli.daemon_url, &agent).await,
    };
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

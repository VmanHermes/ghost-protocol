use clap::{Parser, Subcommand};

mod commands;
mod detect;
mod init;

#[derive(Parser)]
#[command(name = "ghost", about = "Ghost Protocol CLI")]
struct Cli {
    #[arg(long, env = "GHOST_DAEMON_URL", default_value = "http://127.0.0.1:8787")]
    daemon_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a Ghost Protocol project in the current directory
    Init,
    /// Show mesh status
    Status,
    /// List available agents
    Agents,
    /// List registered projects
    Projects,
    /// Start a chat with an agent
    Chat {
        /// Agent ID (e.g., claude-code, ollama:llama3)
        agent: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Init => init::run(&cli.daemon_url).await,
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

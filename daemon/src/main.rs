mod cli;
mod config;
mod hardware;
mod host;
mod middleware;
mod server;
mod store;
mod terminal;
mod transport;

use clap::Parser;
use config::{Cli, CliCommand, Settings};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // CLI subcommands talk to a running daemon via HTTP — no tracing needed
    match &cli.command {
        Some(CliCommand::Serve) | None => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "ghost_protocol_daemon=info".into()),
                )
                .init();

            let settings = Settings::from_cli(cli).expect("invalid configuration");

            tracing::info!(
                bind = ?settings.bind_hosts,
                port = settings.bind_port,
                "starting ghost-protocol-daemon"
            );

            if let Err(e) = server::run(settings).await {
                tracing::error!(error = %e, "daemon exited with error");
                std::process::exit(1);
            }
        }
        Some(cmd) => {
            let port = cli.bind_port;
            if let Err(e) = cli::run(cmd.clone(), port).await {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
}

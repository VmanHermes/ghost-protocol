mod chat;
mod cli;
mod config;
mod mcp;
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
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;

            let log_buffer = host::logs::LogBuffer::new();
            let log_layer = host::logs::LogBufferLayer { buffer: log_buffer.clone() };

            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "ghost_protocol_daemon=info".into()),
                )
                .finish()
                .with(log_layer)
                .init();

            let settings = Settings::from_cli(cli).expect("invalid configuration");

            tracing::info!(
                bind = ?settings.bind_hosts,
                port = settings.bind_port,
                "starting ghost-protocol-daemon"
            );

            if let Err(e) = server::run(settings, log_buffer).await {
                tracing::error!(error = %e, "daemon exited with error");
                std::process::exit(1);
            }
        }
        Some(CliCommand::Mcp) => {
            if let Err(e) = mcp::transport::run_stdio(cli.bind_port).await {
                eprintln!("MCP server error: {e}");
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

mod config;
mod host;
mod middleware;
mod server;
mod store;
mod terminal;
mod transport;

use clap::Parser;
use config::{Cli, Settings};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ghost_protocol_daemon=info".into()),
        )
        .init();

    let cli = Cli::parse();
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

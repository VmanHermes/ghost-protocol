// daemon/src/main.rs
mod config;

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

    // Server startup will be added in Task 10
    tracing::info!("daemon ready");
}

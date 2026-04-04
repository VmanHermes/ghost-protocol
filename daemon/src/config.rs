// daemon/src/config.rs
use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "ghost-protocol-daemon", about = "Ghost Protocol terminal daemon")]
pub struct Cli {
    /// Bind address (comma-separated for multiple interfaces)
    #[arg(long, env = "GHOST_PROTOCOL_BIND_HOST", default_value = "127.0.0.1", global = true)]
    pub bind_host: String,

    /// Bind port
    #[arg(long, env = "GHOST_PROTOCOL_BIND_PORT", default_value_t = 8787, global = true)]
    pub bind_port: u16,

    /// Allowed CIDRs (comma-separated)
    #[arg(
        long,
        env = "GHOST_PROTOCOL_ALLOWED_CIDRS",
        default_value = "100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32",
        global = true
    )]
    pub allowed_cidrs: String,

    /// Database path
    #[arg(long, env = "GHOST_PROTOCOL_DB", default_value = "./data/ghost_protocol.db", global = true)]
    pub db_path: PathBuf,

    /// Log directory
    #[arg(long, env = "GHOST_PROTOCOL_LOG_DIR", global = true)]
    pub log_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum CliCommand {
    /// Show one-line machine status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List terminal sessions
    Sessions {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List known network hosts
    Hosts {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show full machine info as JSON
    Info,
    /// Start the daemon server (default)
    Serve,
    /// Start MCP resource server over stdio (for Claude Code integration)
    Mcp,
}

#[derive(Clone)]
pub struct Settings {
    pub bind_hosts: Vec<String>,
    pub bind_port: u16,
    pub allowed_cidrs: Vec<ipnet::IpNet>,
    pub db_path: PathBuf,
    pub log_dir: PathBuf,
}

impl Settings {
    pub fn from_cli(cli: Cli) -> Result<Self, String> {
        let bind_hosts: Vec<String> = cli
            .bind_host
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let allowed_cidrs: Vec<ipnet::IpNet> = cli
            .allowed_cidrs
            .split(',')
            .map(|s| {
                s.trim()
                    .parse::<ipnet::IpNet>()
                    .map_err(|e| format!("invalid CIDR '{}': {}", s.trim(), e))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let log_dir = cli
            .log_dir
            .unwrap_or_else(|| cli.db_path.parent().unwrap_or(&PathBuf::from(".")).join("logs"));

        Ok(Settings {
            bind_hosts,
            bind_port: cli.bind_port,
            allowed_cidrs,
            db_path: cli.db_path,
            log_dir,
        })
    }

    pub fn is_ip_allowed(&self, ip: IpAddr) -> bool {
        self.allowed_cidrs.iter().any(|net| net.contains(&ip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_is_ip_allowed_tailscale() {
        let settings = Settings::from_cli(Cli {
            bind_host: "127.0.0.1".into(),
            bind_port: 8787,
            allowed_cidrs: "100.64.0.0/10,127.0.0.1/32".into(),
            db_path: PathBuf::from("./data/test.db"),
            log_dir: None,
            command: None,
        })
        .unwrap();

        assert!(settings.is_ip_allowed(IpAddr::V4(Ipv4Addr::new(100, 100, 1, 1))));
        assert!(settings.is_ip_allowed(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!settings.is_ip_allowed(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }

    #[test]
    fn test_multiple_bind_hosts() {
        let settings = Settings::from_cli(Cli {
            bind_host: "100.64.1.1,127.0.0.1".into(),
            bind_port: 8787,
            allowed_cidrs: "127.0.0.1/32".into(),
            db_path: PathBuf::from("./data/test.db"),
            log_dir: None,
            command: None,
        })
        .unwrap();

        assert_eq!(settings.bind_hosts, vec!["100.64.1.1", "127.0.0.1"]);
    }
}

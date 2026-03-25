use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aiclient-api", about = "Unified AI gateway service")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the daemon
    Start {
        /// API port
        #[arg(long, default_value = "9090")]
        port: u16,
        /// Bind address
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Run in foreground (don't daemonize)
        #[arg(long)]
        foreground: bool,
        /// Protect API with bearer token
        #[arg(long)]
        api_key: Option<String>,
        /// Log file path
        #[arg(long)]
        log_file: Option<String>,
    },
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Authentication management
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Show daemon status
    Status,
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// List available models
    Models,
    /// Provider management
    Provider {
        #[command(subcommand)]
        action: ProviderAction,
    },
    /// Tail daemon logs
    Logs {
        /// Show last N lines
        #[arg(long, default_value = "50")]
        lines: usize,
        /// Filter level
        #[arg(long, default_value = "info")]
        level: String,
    },
    /// Self-update from GitHub Releases
    Update,
    /// Stop daemon + remove config + remove binary
    Uninstall,
}

#[derive(Subcommand)]
pub enum AuthAction {
    /// Authenticate with GitHub Copilot
    Copilot {
        #[arg(long, default_value = "individual")]
        account_type: String,
    },
    /// Authenticate with Kiro
    Kiro,
    /// List authenticated providers
    List,
    /// Revoke a provider's tokens
    Revoke { provider: String },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current config
    Show,
    /// Set a config value
    Set { key: String, value: String },
    /// Reload config from disk
    Reload,
}

#[derive(Subcommand)]
pub enum ProviderAction {
    /// Enable a provider
    Enable { name: String },
    /// Disable a provider
    Disable { name: String },
}

mod cli;

use clap::Parser;
use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Start { port, host, foreground: _, api_key: _, log_file: _ } => {
            println!("Starting daemon on {}:{}", host, port);
        }
        Command::Stop => println!("Stopping daemon..."),
        Command::Restart => println!("Restarting daemon..."),
        Command::Auth { action: _ } => println!("Auth action"),
        Command::Status => println!("Querying status..."),
        Command::Config { action: _ } => println!("Config action"),
        Command::Models => println!("Listing models..."),
        Command::Provider { action: _ } => println!("Provider action"),
        Command::Logs { lines: _, level: _ } => println!("Tailing logs..."),
        Command::Update => println!("Updating..."),
        Command::Uninstall => println!("Uninstalling..."),
    }
}

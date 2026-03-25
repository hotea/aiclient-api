use clap::Parser;

mod cli;

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Command::Start { port, host, foreground, api_key, log_file } => {
            cli::start::run(host, port, foreground, api_key, log_file).await
        }
        cli::Command::Stop => cli::stop::run(),
        cli::Command::Restart => {
            cli::restart::run("127.0.0.1".into(), 9090, false, None, None).await
        }
        cli::Command::Auth { action } => cli::auth::run(action).await,
        _ => {
            eprintln!("Command not yet implemented");
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

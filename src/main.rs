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
        cli::Command::Status => cli::status::run().await,
        cli::Command::Config { action } => cli::config_cmd::run(action).await,
        cli::Command::Models => cli::models::run().await,
        cli::Command::Provider { action } => cli::provider_cmd::run(action).await,
        cli::Command::Logs { lines, level } => cli::logs::run(lines, &level).await,
        cli::Command::Update => {
            eprintln!("Update not yet implemented");
            Ok(())
        }
        cli::Command::Uninstall => {
            eprintln!("Uninstall not yet implemented");
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

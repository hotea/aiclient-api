use clap::Parser;

mod cli;

fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Command::Start {
            port,
            host,
            foreground,
            api_key,
            log_file,
        } => match aiclient_api::daemon::read_pid() {
            Ok(Some(pid)) => Err(anyhow::anyhow!("Daemon already running (pid {})", pid)),
            Ok(None) => {
                let daemonize_result =
                    cli::start::daemonize_if_needed(foreground, log_file.as_deref());
                if let Err(e) = daemonize_result {
                    Err(e)
                } else {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build();

                    match runtime {
                        Ok(rt) => {
                            rt.block_on(cli::start::run(host, port, foreground, api_key, log_file))
                        }
                        Err(e) => Err(e.into()),
                    }
                }
            }
            Err(e) => Err(e),
        },
        cli::Command::Stop => cli::stop::run(),
        cli::Command::Restart => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();

            match runtime {
                Ok(rt) => rt.block_on(cli::restart::run(
                    "127.0.0.1".into(),
                    9090,
                    false,
                    None,
                    None,
                )),
                Err(e) => Err(e.into()),
            }
        }
        cli::Command::Auth { action } => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();

            match runtime {
                Ok(rt) => rt.block_on(cli::auth::run(action)),
                Err(e) => Err(e.into()),
            }
        }
        cli::Command::Status => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();

            match runtime {
                Ok(rt) => rt.block_on(cli::status::run()),
                Err(e) => Err(e.into()),
            }
        }
        cli::Command::Config { action } => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();

            match runtime {
                Ok(rt) => rt.block_on(cli::config_cmd::run(action)),
                Err(e) => Err(e.into()),
            }
        }
        cli::Command::Models => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();

            match runtime {
                Ok(rt) => rt.block_on(cli::models::run()),
                Err(e) => Err(e.into()),
            }
        }
        cli::Command::Provider { action } => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();

            match runtime {
                Ok(rt) => rt.block_on(cli::provider_cmd::run(action)),
                Err(e) => Err(e.into()),
            }
        }
        cli::Command::Logs { lines, level } => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();

            match runtime {
                Ok(rt) => rt.block_on(cli::logs::run(lines, &level)),
                Err(e) => Err(e.into()),
            }
        }
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

use anyhow::Result;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

pub async fn run(
    host: String,
    port: u16,
    foreground: bool,
    api_key: Option<String>,
    log_file: Option<String>,
) -> Result<()> {
    if let Some(pid) = aiclient_api::daemon::read_pid()? {
        anyhow::bail!("Daemon already running (pid {})", pid);
    }

    let mut config = aiclient_api::config::load_default_config()?;
    config.server.host = host;
    config.server.port = port;
    if let Some(key) = api_key {
        config.api_key = key;
    }

    let log_path = log_file
        .map(PathBuf::from)
        .unwrap_or_else(|| aiclient_api::util::xdg::log_path());

    if !foreground {
        aiclient_api::daemon::daemonize(&log_path)?;
    }

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));
    if foreground {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    } else {
        let file_appender = tracing_appender::rolling::never(
            log_path.parent().unwrap_or(&PathBuf::from(".")),
            log_path.file_name().unwrap_or_default(),
        );
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(file_appender)
            .with_ansi(false)
            .init();
    }

    tracing::info!(
        "aiclient-api starting on {}:{}",
        config.server.host,
        config.server.port
    );

    if foreground {
        aiclient_api::daemon::write_pid(std::process::id())?;
    }

    let state = aiclient_api::server::state::AppState::new(config.clone());
    let app = aiclient_api::server::build_router(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);

    let shutdown = async {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("failed to install SIGTERM handler");
        let sigint = tokio::signal::ctrl_c();
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("Received SIGTERM"),
            _ = sigint => tracing::info!("Received SIGINT"),
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    tracing::info!("Shutting down...");
    aiclient_api::daemon::remove_pid()?;
    Ok(())
}

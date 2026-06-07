use anyhow::Result;
use std::path::PathBuf;
use tokio::signal::unix::SignalKind;
use tracing_subscriber::EnvFilter;
pub fn daemonize_if_needed(foreground: bool, log_file: Option<&str>) -> Result<()> {
    if foreground {
        return Ok(());
    }

    let log_path = log_file
        .map(PathBuf::from)
        .unwrap_or_else(aiclient_api::util::xdg::log_path);
    aiclient_api::daemon::daemonize(&log_path)
}

pub async fn run(
    host: String,
    port: u16,
    foreground: bool,
    api_key: Option<String>,
    log_file: Option<String>,
) -> Result<()> {
    let mut config = aiclient_api::config::load_default_config()?;
    config.server.host = host;
    config.server.port = port;
    if let Some(key) = api_key {
        config.api_key = key;
        config.auth_enabled = true;
    }

    let log_path = log_file
        .map(PathBuf::from)
        .unwrap_or_else(aiclient_api::util::xdg::log_path);

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.logging.level));
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

    let provider_load = aiclient_api::providers::load_configured_providers(&config).await;
    for provider in &provider_load.loaded {
        tracing::info!("Initialized provider '{}'", provider);
    }
    for skipped in &provider_load.skipped {
        tracing::warn!("Skipped provider: {}", skipped);
    }
    {
        let mut providers = state.providers.write().await;
        *providers = provider_load.providers;
    }

    let app = aiclient_api::server::build_router(state.clone());

    // Spawn the Unix socket control server
    let control_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = aiclient_api::daemon::control::start_control_server(control_state).await {
            tracing::error!("Control server error: {:#}", e);
        }
    });

    // Spawn SIGHUP handler for config hot-reload
    let reload_state = state.clone();
    tokio::spawn(async move {
        let mut sighup = tokio::signal::unix::signal(SignalKind::hangup())
            .expect("failed to install SIGHUP handler");
        loop {
            sighup.recv().await;
            tracing::info!("Received SIGHUP, reloading config...");
            match aiclient_api::config::load_default_config() {
                Ok(new_config) => {
                    let load =
                        aiclient_api::providers::load_configured_providers(&new_config).await;
                    {
                        let mut providers = reload_state.providers.write().await;
                        *providers = load.providers;
                    }
                    reload_state.config.store(std::sync::Arc::new(new_config));
                    tracing::info!(
                        loaded = ?load.loaded,
                        skipped = ?load.skipped,
                        "Config and providers reloaded successfully"
                    );
                }
                Err(e) => tracing::error!("Config reload failed: {:#}", e),
            }
        }
    });

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);

    let shutdown = async {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        let sigint = tokio::signal::ctrl_c();
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("Received SIGTERM"),
            _ = sigint => tracing::info!("Received SIGINT"),
        }
    };

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await?;
    tracing::info!("Shutting down...");
    aiclient_api::daemon::remove_pid()?;
    Ok(())
}

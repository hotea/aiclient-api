use anyhow::Result;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use aiclient_api::auth::TokenStore;

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

    // Initialize providers from config
    {
        let store = aiclient_api::auth::token_store::XdgTokenStore::default();
        let vscode_version = config.vscode_version.clone();
        let mut providers = state.providers.write().await;

        for (_name, provider_config) in &config.providers {
            match provider_config {
                aiclient_api::config::types::ProviderConfig::Copilot {
                    enabled: true,
                    account_type,
                    ..
                } => {
                    match store.load("copilot").await {
                        Ok(aiclient_api::auth::TokenData::Copilot { github_token, .. }) => {
                            let provider = aiclient_api::providers::copilot::CopilotProvider::new(
                                github_token,
                                account_type.clone(),
                                &vscode_version,
                            );
                            provider.start();
                            providers.insert("copilot".to_string(), provider);
                            tracing::info!("Initialized Copilot provider");
                        }
                        Ok(_) => {
                            tracing::warn!("Unexpected token type for copilot provider, skipping");
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to load Copilot token, skipping provider: {:#}",
                                e
                            );
                        }
                    }
                }
                aiclient_api::config::types::ProviderConfig::Copilot {
                    enabled: false, ..
                } => {
                    // Provider disabled, skip
                }
                aiclient_api::config::types::ProviderConfig::Kiro {
                    enabled: true,
                    region,
                    ..
                } => {
                    match store.load("kiro").await {
                        Ok(token_data) => {
                            match aiclient_api::providers::kiro::KiroProvider::new(&token_data, region) {
                                Ok(provider) => {
                                    provider.start();
                                    providers.insert(
                                        "kiro".to_string(),
                                        provider as std::sync::Arc<dyn aiclient_api::providers::Provider>,
                                    );
                                    tracing::info!("Kiro provider initialized");
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to create Kiro provider: {:#}", e);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Kiro auth not configured: {:#}", e);
                        }
                    }
                }
                aiclient_api::config::types::ProviderConfig::Kiro {
                    enabled: false, ..
                } => {
                    // Provider disabled, skip
                }
            }
        }
    }

    let app = aiclient_api::server::build_router(state.clone());

    // Spawn the Unix socket control server
    let control_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = aiclient_api::daemon::control::start_control_server(control_state).await {
            tracing::error!("Control server error: {:#}", e);
        }
    });

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

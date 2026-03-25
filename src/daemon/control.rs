use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

use crate::server::state::AppState;

pub async fn start_control_server(state: AppState) -> Result<()> {
    let socket_path = crate::util::xdg::socket_path();

    // Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Remove existing socket if present
    match std::fs::remove_file(&socket_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(anyhow::anyhow!(e).context("Failed to remove existing socket file")),
    }

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("Failed to bind Unix socket at {}", socket_path.display()))?;

    tracing::info!("Control server listening on {}", socket_path.display());

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state).await {
                        tracing::warn!("Control connection error: {:#}", e);
                    }
                });
            }
            Err(e) => {
                tracing::error!("Failed to accept control connection: {:#}", e);
            }
        }
    }
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    state: AppState,
) -> Result<()> {
    // Read length-prefixed JSON request
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.context("Failed to read request length")?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 1024 * 1024 {
        anyhow::bail!("Request too large: {} bytes", len);
    }

    let mut req_buf = vec![0u8; len];
    stream.read_exact(&mut req_buf).await.context("Failed to read request body")?;

    let request: serde_json::Value = serde_json::from_slice(&req_buf)
        .context("Failed to parse request JSON")?;

    let response = dispatch_request(request, &state).await;

    // Write length-prefixed JSON response
    let resp_bytes = serde_json::to_vec(&response)?;
    stream.write_all(&(resp_bytes.len() as u32).to_be_bytes()).await?;
    stream.write_all(&resp_bytes).await?;
    stream.flush().await?;

    Ok(())
}

async fn dispatch_request(
    request: serde_json::Value,
    state: &AppState,
) -> serde_json::Value {
    let method = match request.get("method").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => {
            return serde_json::json!({
                "ok": false,
                "error": "Missing 'method' field in request"
            });
        }
    };

    match method.as_str() {
        "status" => handle_status(state).await,
        "config.show" => handle_config_show(state).await,
        "config.reload" => handle_config_reload(state).await,
        "models" => handle_models(state).await,
        "provider.enable" => {
            let name = request
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            handle_provider_enable(name).await
        }
        "provider.disable" => {
            let name = request
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            handle_provider_disable(name).await
        }
        "config.set" => {
            let params = request.get("params");
            let key = params
                .and_then(|p| p.get("key"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let value = params
                .and_then(|p| p.get("value"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            handle_config_set(key, value).await
        }
        "logs.tail" => {
            let n = request
                .get("params")
                .and_then(|p| p.get("lines"))
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;
            handle_logs_tail(n).await
        }
        unknown => {
            serde_json::json!({
                "ok": false,
                "error": format!("Unknown method: {}", unknown)
            })
        }
    }
}

async fn handle_status(state: &AppState) -> serde_json::Value {
    let uptime_secs = state.start_time.elapsed().as_secs();
    let providers = state.providers.read().await;
    let provider_count = providers.len();

    let provider_health: serde_json::Value = providers
        .iter()
        .map(|(name, provider)| {
            (name.clone(), serde_json::json!({
                "healthy": provider.is_healthy()
            }))
        })
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();

    serde_json::json!({
        "ok": true,
        "data": {
            "uptime_seconds": uptime_secs,
            "provider_count": provider_count,
            "connections": 0,
            "providers": provider_health
        }
    })
}

async fn handle_config_show(state: &AppState) -> serde_json::Value {
    let config = state.config.load();
    match serde_json::to_value(config.as_ref()) {
        Ok(v) => serde_json::json!({ "ok": true, "data": v }),
        Err(e) => serde_json::json!({ "ok": false, "error": format!("Serialization error: {}", e) }),
    }
}

async fn handle_config_reload(state: &AppState) -> serde_json::Value {
    match crate::config::load_default_config() {
        Ok(new_config) => {
            state.config.store(std::sync::Arc::new(new_config));
            serde_json::json!({ "ok": true, "data": { "message": "Config reloaded" } })
        }
        Err(e) => {
            serde_json::json!({ "ok": false, "error": format!("Failed to reload config: {:#}", e) })
        }
    }
}

async fn handle_models(state: &AppState) -> serde_json::Value {
    let providers = state.providers.read().await;
    let mut all_models = Vec::new();

    for (_name, provider) in providers.iter() {
        match provider.list_models().await {
            Ok(models) => {
                all_models.extend(models);
            }
            Err(e) => {
                tracing::warn!("Failed to list models for provider: {:#}", e);
            }
        }
    }

    match serde_json::to_value(&all_models) {
        Ok(v) => serde_json::json!({ "ok": true, "data": { "models": v } }),
        Err(e) => serde_json::json!({ "ok": false, "error": format!("Serialization error: {}", e) }),
    }
}

async fn handle_provider_enable(name: String) -> serde_json::Value {
    if name.is_empty() {
        return serde_json::json!({ "ok": false, "error": "Missing provider name" });
    }
    // Not fully implemented yet — would require config hot-reload
    serde_json::json!({
        "ok": true,
        "data": { "message": format!("Provider '{}' enable requested (requires restart to take effect)", name) }
    })
}

async fn handle_provider_disable(name: String) -> serde_json::Value {
    if name.is_empty() {
        return serde_json::json!({ "ok": false, "error": "Missing provider name" });
    }
    // Not fully implemented yet — would require config hot-reload
    serde_json::json!({
        "ok": true,
        "data": { "message": format!("Provider '{}' disable requested (requires restart to take effect)", name) }
    })
}

async fn handle_config_set(key: String, value: serde_json::Value) -> serde_json::Value {
    if key.is_empty() {
        return serde_json::json!({ "ok": false, "error": "Missing config key" });
    }
    // Not yet implemented — would require config hot-reload and persistence
    serde_json::json!({
        "ok": true,
        "data": { "message": format!("config.set for '{}' not yet implemented", key), "key": key, "value": value }
    })
}

async fn handle_logs_tail(n: usize) -> serde_json::Value {
    let log_path = crate::util::xdg::log_path();
    match tokio::fs::read_to_string(&log_path).await {
        Ok(contents) => {
            let lines: Vec<&str> = contents.lines().collect();
            let start = lines.len().saturating_sub(n);
            let tail: Vec<&str> = lines[start..].to_vec();
            serde_json::json!({
                "ok": true,
                "data": { "lines": tail, "count": tail.len(), "path": log_path.display().to_string() }
            })
        }
        Err(e) => {
            serde_json::json!({
                "ok": false,
                "error": format!("Failed to read log file at {}: {}", log_path.display(), e)
            })
        }
    }
}

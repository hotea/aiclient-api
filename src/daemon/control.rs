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

async fn handle_connection(mut stream: tokio::net::UnixStream, state: AppState) -> Result<()> {
    // Read length-prefixed JSON request
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .context("Failed to read request length")?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 1024 * 1024 {
        anyhow::bail!("Request too large: {} bytes", len);
    }

    let mut req_buf = vec![0u8; len];
    stream
        .read_exact(&mut req_buf)
        .await
        .context("Failed to read request body")?;

    let request: serde_json::Value =
        serde_json::from_slice(&req_buf).context("Failed to parse request JSON")?;

    let response = dispatch_request(request, &state).await;

    // Write length-prefixed JSON response
    let resp_bytes = serde_json::to_vec(&response)?;
    stream
        .write_all(&(resp_bytes.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(&resp_bytes).await?;
    stream.flush().await?;

    Ok(())
}

async fn dispatch_request(request: serde_json::Value, state: &AppState) -> serde_json::Value {
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
        "provider.list" => handle_provider_list(state).await,
        "provider.enable" => {
            let name = request
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            handle_provider_set_enabled(state, name, true).await
        }
        "provider.disable" => {
            let name = request
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            handle_provider_set_enabled(state, name, false).await
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

async fn handle_provider_list(state: &AppState) -> serde_json::Value {
    let config = state.config.load();
    let providers = state.providers.read().await;
    let mut names: Vec<&String> = config.providers.keys().collect();
    names.sort();

    let provider_list: Vec<serde_json::Value> = names
        .into_iter()
        .filter_map(|name| {
            let config = config.providers.get(name)?;
            let runtime_provider = providers.get(name.as_str());
            Some(serde_json::json!({
                "name": name,
                "type": provider_type(config),
                "configured_enabled": config.is_enabled(),
                "running": runtime_provider.is_some(),
                "healthy": runtime_provider.map(|provider| provider.is_healthy()).unwrap_or(false),
            }))
        })
        .collect();
    let count = provider_list.len();
    let running_count = providers.len();

    serde_json::json!({
        "ok": true,
        "data": {
            "providers": provider_list,
            "count": count,
            "running_count": running_count,
        }
    })
}

fn provider_type(config: &crate::config::types::ProviderConfig) -> &'static str {
    match config {
        crate::config::types::ProviderConfig::Copilot { .. } => "copilot",
        crate::config::types::ProviderConfig::Kiro { .. } => "kiro",
        crate::config::types::ProviderConfig::Common { .. } => "common",
    }
}

async fn handle_status(state: &AppState) -> serde_json::Value {
    let uptime_secs = state.start_time.elapsed().as_secs();
    let providers = state.providers.read().await;
    let provider_count = providers.len();

    let provider_health: serde_json::Value = providers
        .iter()
        .map(|(name, provider)| {
            (
                name.clone(),
                serde_json::json!({
                    "healthy": provider.is_healthy()
                }),
            )
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
        Err(e) => {
            serde_json::json!({ "ok": false, "error": format!("Serialization error: {}", e) })
        }
    }
}

async fn handle_config_reload(state: &AppState) -> serde_json::Value {
    match crate::config::load_default_config() {
        Ok(new_config) => {
            let load = apply_config(state, new_config).await;
            serde_json::json!({
                "ok": true,
                "data": {
                    "message": "Config and providers reloaded",
                    "loaded": load.loaded,
                    "skipped": load.skipped,
                }
            })
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
        Err(e) => {
            serde_json::json!({ "ok": false, "error": format!("Serialization error: {}", e) })
        }
    }
}

async fn handle_provider_set_enabled(
    state: &AppState,
    name: String,
    enabled: bool,
) -> serde_json::Value {
    if name.is_empty() {
        return serde_json::json!({ "ok": false, "error": "Missing provider name" });
    }

    let mut config = state.config.load().as_ref().clone();
    let mut matched_providers = Vec::new();

    if let Some(provider_config) = config.providers.get_mut(&name) {
        provider_config.set_enabled(enabled);
        matched_providers.push(name.clone());
    } else {
        let matching_type_names: Vec<String> = config
            .providers
            .iter()
            .filter_map(|(provider_name, provider_config)| {
                if provider_type(provider_config) == name {
                    Some(provider_name.clone())
                } else {
                    None
                }
            })
            .collect();

        for provider_name in matching_type_names {
            if let Some(provider_config) = config.providers.get_mut(&provider_name) {
                provider_config.set_enabled(enabled);
                matched_providers.push(provider_name);
            }
        }
    }

    if matched_providers.is_empty() {
        let error = if is_provider_type_name(&name) {
            format!(
                "No configured providers of type '{}'. Add provider instances such as [providers.aihubmix] with type = \"{}\" to config.toml, then run config reload.",
                name,
                name
            )
        } else {
            format!(
                "Provider '{}' not found in current config. Use a configured provider name such as 'aihubmix', or add a [providers.{}] entry to config.toml.",
                name,
                name
            )
        };
        return serde_json::json!({
            "ok": false,
            "error": error
        });
    }

    let load = apply_config(state, config).await;
    let action = if enabled { "enabled" } else { "disabled" };

    serde_json::json!({
        "ok": true,
        "data": {
            "message": format!("Provider selection '{}' {} without restart", name, action),
            "provider": name,
            "matched_providers": matched_providers,
            "enabled": enabled,
            "loaded": load.loaded,
            "skipped": load.skipped,
            "persistent": false,
        }
    })
}

fn is_provider_type_name(name: &str) -> bool {
    matches!(name, "common" | "copilot" | "kiro")
}

async fn apply_config(
    state: &AppState,
    config: crate::config::types::Config,
) -> crate::providers::ProviderLoadResult {
    let load = crate::providers::load_configured_providers(&config).await;
    {
        let mut providers = state.providers.write().await;
        *providers = load.providers.clone();
    }
    state.config.store(std::sync::Arc::new(config));
    load
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{Config, ProviderConfig};
    use crate::server::state::AppState;

    fn config_with_disabled_common_provider() -> Config {
        let mut config = Config {
            default_provider: "mock".to_string(),
            ..Config::default()
        };
        config.providers.clear();
        config.providers.insert(
            "mock".to_string(),
            ProviderConfig::Common {
                enabled: false,
                base_url: "http://127.0.0.1:9/v1".to_string(),
                api_key: "test-key".to_string(),
                api_keys: Vec::new(),
                api_key_env: String::new(),
                api_key_envs: Vec::new(),
                auth_scheme: "Bearer".to_string(),
                chat_completions_path: "/chat/completions".to_string(),
                models_path: "/models".to_string(),
                models: vec!["test-model".to_string()],
                vendor: "mock".to_string(),
                supports_streaming: true,
                supports_tools: false,
                supports_vision: false,
                supports_thinking: false,
                headers: Default::default(),
            },
        );
        config
    }

    #[tokio::test]
    async fn provider_enable_disable_updates_runtime_provider_map() {
        let state = AppState::new(config_with_disabled_common_provider());
        assert!(state.providers.read().await.is_empty());

        let enabled = handle_provider_set_enabled(&state, "mock".to_string(), true).await;
        assert_eq!(enabled["ok"], true);
        assert!(state.providers.read().await.contains_key("mock"));
        assert_eq!(state.config.load().providers["mock"].is_enabled(), true);

        let disabled = handle_provider_set_enabled(&state, "mock".to_string(), false).await;
        assert_eq!(disabled["ok"], true);
        assert!(!state.providers.read().await.contains_key("mock"));
        assert_eq!(state.config.load().providers["mock"].is_enabled(), false);
    }

    #[tokio::test]
    async fn provider_list_reports_configured_and_runtime_status() {
        let state = AppState::new(config_with_disabled_common_provider());

        let initial = handle_provider_list(&state).await;
        assert_eq!(initial["ok"], true);
        assert_eq!(initial["data"]["count"], 1);
        assert_eq!(initial["data"]["running_count"], 0);
        assert_eq!(initial["data"]["providers"][0]["name"], "mock");
        assert_eq!(initial["data"]["providers"][0]["type"], "common");
        assert_eq!(initial["data"]["providers"][0]["configured_enabled"], false);
        assert_eq!(initial["data"]["providers"][0]["running"], false);

        let _ = handle_provider_set_enabled(&state, "mock".to_string(), true).await;
        let enabled = handle_provider_list(&state).await;
        assert_eq!(enabled["data"]["running_count"], 1);
        assert_eq!(enabled["data"]["providers"][0]["configured_enabled"], true);
        assert_eq!(enabled["data"]["providers"][0]["running"], true);
        assert_eq!(enabled["data"]["providers"][0]["healthy"], true);
    }

    #[tokio::test]
    async fn provider_enable_by_type_updates_matching_providers() {
        let state = AppState::new(config_with_disabled_common_provider());

        let enabled = handle_provider_set_enabled(&state, "common".to_string(), true).await;

        assert_eq!(enabled["ok"], true);
        assert_eq!(enabled["data"]["matched_providers"][0], "mock");
        assert!(state.providers.read().await.contains_key("mock"));
        assert_eq!(state.config.load().providers["mock"].is_enabled(), true);
    }

    #[tokio::test]
    async fn provider_enable_unknown_name_returns_actionable_error() {
        let mut config = Config::default();
        config.providers.clear();
        let state = AppState::new(config);

        let response = handle_provider_set_enabled(&state, "common".to_string(), true).await;

        assert_eq!(response["ok"], false);
        assert!(response["error"]
            .as_str()
            .unwrap()
            .contains("No configured providers of type 'common'"));
    }
}

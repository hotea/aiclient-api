use anyhow::{bail, Context, Result};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

use super::commands::ApiKeyAction;
use super::status::send_control_request;
use aiclient_api::config::types::Config;
use aiclient_api::util::xdg::config_dir;

pub async fn run(action: ApiKeyAction) -> Result<()> {
    match action {
        ApiKeyAction::Generate { print_only } => {
            let api_key = generate_api_key();
            if print_only {
                println!("{}", api_key);
                return Ok(());
            }

            let config_path = get_config_path();
            let mut config = load_or_default_config(&config_path)?;
            config.api_key = api_key.clone();
            config.auth_enabled = true;
            save_config(&config_path, &config)?;
            let reload = reload_daemon_if_running().await;

            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "ok": true,
                    "data": {
                        "api_key": api_key,
                        "auth_enabled": true,
                        "config_path": config_path.display().to_string(),
                        "reload": reload,
                    }
                }))?
            );
        }
        ApiKeyAction::Enable => {
            let config_path = get_config_path();
            let mut config = load_or_default_config(&config_path)?;
            if config.api_key.trim().is_empty() {
                bail!("No API key configured. Run `aiclient-api api-key generate` first.");
            }
            config.auth_enabled = true;
            save_config(&config_path, &config)?;
            let reload = reload_daemon_if_running().await;
            print_status(true, config_path, reload)?;
        }
        ApiKeyAction::Disable => {
            let config_path = get_config_path();
            let mut config = load_or_default_config(&config_path)?;
            config.auth_enabled = false;
            save_config(&config_path, &config)?;
            let reload = reload_daemon_if_running().await;
            print_status(false, config_path, reload)?;
        }
        ApiKeyAction::Show => {
            let config_path = get_config_path();
            let config = load_or_default_config(&config_path)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "ok": true,
                    "data": {
                        "auth_enabled": config.auth_enabled,
                        "api_key_configured": !config.api_key.trim().is_empty(),
                        "config_path": config_path.display().to_string(),
                    }
                }))?
            );
        }
    }

    Ok(())
}

fn generate_api_key() -> String {
    format!(
        "ak-{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn get_config_path() -> PathBuf {
    config_dir().join("config.toml")
}

fn load_or_default_config(path: &Path) -> Result<Config> {
    if path.exists() {
        let content = fs::read_to_string(path).context("Failed to read config file")?;
        let config: Config = toml::from_str(&content).context("Failed to parse config file")?;
        Ok(config.with_default_providers())
    } else {
        Ok(Config::default())
    }
}

fn save_config(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create config directory")?;
    }

    let toml_string = toml::to_string_pretty(config).context("Failed to serialize config")?;
    fs::write(path, toml_string).context("Failed to write config file")?;
    Ok(())
}

async fn reload_daemon_if_running() -> serde_json::Value {
    match send_control_request(json!({"method": "config.reload"})).await {
        Ok(resp) => json!({
            "attempted": true,
            "ok": resp.get("ok").and_then(|ok| ok.as_bool()).unwrap_or(false),
            "response": resp,
        }),
        Err(e) => json!({
            "attempted": true,
            "ok": false,
            "error": e.to_string(),
        }),
    }
}

fn print_status(auth_enabled: bool, config_path: PathBuf, reload: serde_json::Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "data": {
                "auth_enabled": auth_enabled,
                "config_path": config_path.display().to_string(),
                "reload": reload,
            }
        }))?
    );
    Ok(())
}

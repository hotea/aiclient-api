use anyhow::{Context, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::fs;
use std::path::PathBuf;

use super::commands::ConfigAction;
use super::status::send_control_request;
use aiclient_api::config::types::{Config, Format};
use aiclient_api::util::xdg::config_dir;

pub async fn run(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Init => {
            run_interactive_config().await?;
        }
        ConfigAction::Show => {
            let resp = send_control_request(serde_json::json!({"method": "config.show"})).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        ConfigAction::Reload => {
            let resp = send_control_request(serde_json::json!({"method": "config.reload"})).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        ConfigAction::Set { key, value } => {
            eprintln!("Config set not yet implemented (key={:?}, value={:?})", key, value);
        }
    }
    Ok(())
}

async fn run_interactive_config() -> Result<()> {
    let theme = ColorfulTheme::default();
    
    println!("\n🔧 Interactive Configuration Wizard\n");
    println!("This will guide you through setting up aiclient-api configuration.\n");

    // Load existing config or create default
    let config_path = get_config_path();
    let mut config = load_or_default_config(&config_path)?;

    // Step 1: Choose default output format
    println!("📋 Step 1: Choose default output format");
    let formats = vec!["OpenAI (Chat Completions)", "Anthropic (Messages API)"];
    let format_selection = Select::with_theme(&theme)
        .with_prompt("Which format do you want as default?")
        .items(&formats)
        .default(if matches!(config.default_format, Format::OpenAI) { 0 } else { 1 })
        .interact()?;
    
    config.default_format = if format_selection == 0 {
        Format::OpenAI
    } else {
        Format::Anthropic
    };

    // Step 2: Choose default provider
    println!("\n🤖 Step 2: Choose default provider");
    
    // Check which providers are authenticated
    let available_providers = check_authenticated_providers().await;
    
    if available_providers.is_empty() {
        println!("⚠️  No providers are authenticated yet!");
        println!("Please run authentication first:");
        println!("  - For GitHub Copilot: aiclient-api auth copilot");
        println!("  - For Kiro (AWS): aiclient-api auth kiro");
        
        let continue_anyway = Confirm::with_theme(&theme)
            .with_prompt("Continue configuration anyway?")
            .default(true)
            .interact()?;
        
        if !continue_anyway {
            println!("Configuration cancelled.");
            return Ok(());
        }
        
        // Allow choosing even if not authenticated
        let all_providers = vec!["copilot", "kiro"];
        let provider_idx = Select::with_theme(&theme)
            .with_prompt("Which provider do you want to use by default?")
            .items(&all_providers)
            .default(0)
            .interact()?;
        config.default_provider = all_providers[provider_idx].to_string();
    } else {
        println!("✅ Available providers: {}", available_providers.join(", "));
        
        let provider_idx = Select::with_theme(&theme)
            .with_prompt("Which provider do you want to use by default?")
            .items(&available_providers)
            .default(0)
            .interact()?;
        config.default_provider = available_providers[provider_idx].clone();
    }

    // Step 3: Fetch and display available models (if service is running)
    println!("\n🎯 Step 3: Available models");
    
    match fetch_available_models(&config.default_provider).await {
        Ok(models) => {
            if models.is_empty() {
                println!("⚠️  No models found for provider: {}", config.default_provider);
            } else {
                println!("✅ Found {} models for {}:", models.len(), config.default_provider);
                for (i, model) in models.iter().take(10).enumerate() {
                    println!("   {}. {}", i + 1, model);
                }
                if models.len() > 10 {
                    println!("   ... and {} more", models.len() - 10);
                }
            }
        }
        Err(e) => {
            println!("⚠️  Could not fetch models: {}", e);
            println!("   (This is normal if the service is not running)");
        }
    }

    // Step 4: Server configuration
    println!("\n⚙️  Step 4: Server configuration");
    
    let configure_server = Confirm::with_theme(&theme)
        .with_prompt("Configure server settings (host/port)?")
        .default(false)
        .interact()?;
    
    if configure_server {
        config.server.host = Input::with_theme(&theme)
            .with_prompt("Server host")
            .default(config.server.host)
            .interact_text()?;
        
        config.server.port = Input::with_theme(&theme)
            .with_prompt("Server port")
            .default(config.server.port)
            .interact_text()?;
    }

    // Step 5: API key
    println!("\n🔐 Step 5: API security");
    
    let use_api_key = Confirm::with_theme(&theme)
        .with_prompt("Require API key for requests?")
        .default(!config.api_key.is_empty())
        .interact()?;
    
    if use_api_key {
        config.api_key = Input::with_theme(&theme)
            .with_prompt("API key")
            .default(if config.api_key.is_empty() {
                "your-secret-key".to_string()
            } else {
                config.api_key.clone()
            })
            .interact_text()?;
    } else {
        config.api_key = String::new();
    }

    // Step 6: Save configuration
    println!("\n💾 Step 6: Save configuration");
    println!("\nConfiguration summary:");
    println!("  • Default format: {:?}", config.default_format);
    println!("  • Default provider: {}", config.default_provider);
    println!("  • Server: {}:{}", config.server.host, config.server.port);
    println!("  • API key: {}", if config.api_key.is_empty() { "disabled" } else { "enabled" });
    
    let save = Confirm::with_theme(&theme)
        .with_prompt("Save configuration?")
        .default(true)
        .interact()?;
    
    if save {
        save_config(&config_path, &config)?;
        println!("\n✅ Configuration saved to: {}", config_path.display());
        println!("\n🚀 Next steps:");
        println!("  1. Start the service: aiclient-api start");
        println!("  2. Check status: aiclient-api status");
        println!("  3. List models: aiclient-api models");
    } else {
        println!("\n❌ Configuration not saved.");
    }

    Ok(())
}

fn get_config_path() -> PathBuf {
    config_dir().join("config.toml")
}

fn load_or_default_config(path: &PathBuf) -> Result<Config> {
    if path.exists() {
        let content = fs::read_to_string(path)
            .context("Failed to read config file")?;
        let config: Config = toml::from_str(&content)
            .context("Failed to parse config file")?;
        Ok(config)
    } else {
        Ok(Config::default())
    }
}

fn save_config(path: &PathBuf, config: &Config) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .context("Failed to create config directory")?;
    }
    
    let toml_string = toml::to_string_pretty(config)
        .context("Failed to serialize config")?;
    
    fs::write(path, toml_string)
        .context("Failed to write config file")?;
    
    Ok(())
}

async fn check_authenticated_providers() -> Vec<String> {
    let mut providers = Vec::new();
    
    // Check copilot token
    let cfg_dir = config_dir();
    let copilot_token = cfg_dir.join("copilot").join("token.json");
    if copilot_token.exists() {
        providers.push("copilot".to_string());
    }
        
    
    // Check kiro token
    let kiro_token = cfg_dir.join("kiro").join("token.json");
    if kiro_token.exists() {
        providers.push("kiro".to_string());
    }
    
    providers
}

async fn fetch_available_models(provider: &str) -> Result<Vec<String>> {
    // Try to fetch from running service
    let client = reqwest::Client::new();
    let resp = client
        .get("http://localhost:9090/v1/models")
        .send()
        .await?;
    
    if !resp.status().is_success() {
        anyhow::bail!("Service not running or not accessible");
    }
    
    let json: serde_json::Value = resp.json().await?;
    
    let models: Vec<String> = json
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let owned_by = item.get("owned_by")?.as_str()?;
                    let id = item.get("id")?.as_str()?;
                    if owned_by == provider {
                        Some(id.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    
    Ok(models)
}

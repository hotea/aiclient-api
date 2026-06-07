use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

pub mod common;
pub mod copilot;
pub mod kiro;
pub mod router;

use crate::auth::TokenStore;
use crate::config::types::{Config, ProviderConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub provider: String,
    pub vendor: String,
    pub display_name: String,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_thinking: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub system: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub tools: Option<Vec<serde_json::Value>>,
    pub tool_choice: Option<serde_json::Value>,
    pub extra: serde_json::Value,
}

pub enum ProviderResponse {
    Complete(serde_json::Value),
    Stream(Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    OpenAI,
    Anthropic,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn is_healthy(&self) -> bool;
    async fn list_models(&self) -> Result<Vec<Model>>;
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse>;
    fn prefers_native_anthropic_streaming(&self) -> bool {
        false
    }
    fn supports_passthrough(&self, _format: OutputFormat) -> bool {
        false
    }
    async fn passthrough(
        &self,
        _model: &str,
        _body: serde_json::Value,
        _format: OutputFormat,
        _stream: bool,
    ) -> Result<ProviderResponse> {
        anyhow::bail!("passthrough not supported")
    }
}

pub struct ProviderLoadResult {
    pub providers: HashMap<String, Arc<dyn Provider>>,
    pub loaded: Vec<String>,
    pub skipped: Vec<String>,
}

pub async fn load_configured_providers(config: &Config) -> ProviderLoadResult {
    let mut providers = HashMap::new();
    let mut loaded = Vec::new();
    let mut skipped = Vec::new();

    for (provider_name, provider_config) in config.providers.iter() {
        match load_provider(provider_name, provider_config, config).await {
            Ok(Some((name, provider))) => {
                loaded.push(name.clone());
                providers.insert(name, provider);
            }
            Ok(None) => {}
            Err(e) => {
                skipped.push(format!("{}: {:#}", provider_name, e));
            }
        }
    }

    ProviderLoadResult {
        providers,
        loaded,
        skipped,
    }
}

async fn load_provider(
    provider_name: &str,
    provider_config: &ProviderConfig,
    config: &Config,
) -> Result<Option<(String, Arc<dyn Provider>)>> {
    if !provider_config.is_enabled() {
        return Ok(None);
    }

    match provider_config {
        ProviderConfig::Copilot { account_type, .. } => {
            let store = crate::auth::token_store::XdgTokenStore::default();
            match store.load("copilot").await? {
                crate::auth::TokenData::Copilot { github_token, .. } => {
                    let provider = copilot::CopilotProvider::new(
                        github_token,
                        account_type.clone(),
                        &config.vscode_version,
                    );
                    provider.start();
                    Ok(Some(("copilot".to_string(), provider)))
                }
                _ => anyhow::bail!("Unexpected token type for copilot provider"),
            }
        }
        ProviderConfig::Kiro { region, .. } => {
            let store = crate::auth::token_store::XdgTokenStore::default();
            let token_data = store.load("kiro").await?;
            let provider = kiro::KiroProvider::new(&token_data, region)?;
            provider.start();
            Ok(Some(("kiro".to_string(), provider as Arc<dyn Provider>)))
        }
        ProviderConfig::Common {
            base_url,
            api_key,
            api_keys,
            api_key_env,
            api_key_envs,
            auth_scheme,
            chat_completions_path,
            models_path,
            models,
            vendor,
            supports_streaming,
            supports_tools,
            supports_vision,
            supports_thinking,
            headers,
            ..
        } => {
            let provider = common::CommonProvider::new(common::CommonProviderConfig {
                name: provider_name.to_string(),
                base_url: base_url.clone(),
                api_key: api_key.clone(),
                api_keys: api_keys.clone(),
                api_key_env: api_key_env.clone(),
                api_key_envs: api_key_envs.clone(),
                auth_scheme: auth_scheme.clone(),
                chat_completions_path: chat_completions_path.clone(),
                models_path: models_path.clone(),
                models: models.clone(),
                vendor: vendor.clone(),
                supports_streaming: *supports_streaming,
                supports_tools: *supports_tools,
                supports_vision: *supports_vision,
                supports_thinking: *supports_thinking,
                headers: headers.clone(),
            })?;
            Ok(Some((provider_name.to_string(), provider)))
        }
    }
}

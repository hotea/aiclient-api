use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::providers::{Model, OutputFormat, Provider, ProviderRequest, ProviderResponse};

pub struct CommonProvider {
    name: String,
    base_url: String,
    api_keys: Vec<String>,
    next_key: AtomicUsize,
    auth_scheme: String,
    chat_completions_path: String,
    models_path: String,
    configured_models: Vec<String>,
    vendor: String,
    supports_streaming: bool,
    supports_tools: bool,
    supports_vision: bool,
    supports_thinking: bool,
    extra_headers: HashMap<String, String>,
    client: reqwest::Client,
}

pub struct CommonProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub api_keys: Vec<String>,
    pub api_key_env: String,
    pub api_key_envs: Vec<String>,
    pub auth_scheme: String,
    pub chat_completions_path: String,
    pub models_path: String,
    pub models: Vec<String>,
    pub vendor: String,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_thinking: bool,
    pub headers: HashMap<String, String>,
}

impl CommonProvider {
    pub fn new(config: CommonProviderConfig) -> Result<Arc<Self>> {
        let api_keys = resolve_api_keys(
            &config.api_key,
            &config.api_keys,
            &config.api_key_env,
            &config.api_key_envs,
        )?;

        Ok(Arc::new(Self {
            name: config.name,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            api_keys,
            next_key: AtomicUsize::new(0),
            auth_scheme: config.auth_scheme,
            chat_completions_path: config.chat_completions_path,
            models_path: config.models_path,
            configured_models: config.models,
            vendor: config.vendor,
            supports_streaming: config.supports_streaming,
            supports_tools: config.supports_tools,
            supports_vision: config.supports_vision,
            supports_thinking: config.supports_thinking,
            extra_headers: config.headers,
            client: reqwest::Client::new(),
        }))
    }

    fn model_without_prefix(&self, model: &str) -> String {
        model
            .strip_prefix(&format!("{}/", self.name))
            .unwrap_or(model)
            .to_string()
    }

    fn chat_completions_url(&self) -> String {
        if self.base_url.ends_with("/chat/completions") {
            self.base_url.clone()
        } else {
            join_url(&self.base_url, &self.chat_completions_path)
        }
    }

    fn models_url(&self) -> String {
        if self.base_url.ends_with("/chat/completions") {
            let base = self
                .base_url
                .trim_end_matches("/chat/completions")
                .trim_end_matches('/');
            join_url(base, &self.models_path)
        } else {
            join_url(&self.base_url, &self.models_path)
        }
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(api_key) = self.next_api_key() {
            let value = if self.auth_scheme.trim().is_empty()
                || has_auth_scheme(&api_key, self.auth_scheme.trim())
            {
                api_key
            } else {
                format!("{} {}", self.auth_scheme.trim(), api_key)
            };
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&value).context("Invalid common provider auth header")?,
            );
        }

        for (name, value) in &self.extra_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("Invalid common provider header name: {}", name))?;
            let header_value = HeaderValue::from_str(value)
                .with_context(|| format!("Invalid common provider header value for {}", name))?;
            headers.insert(header_name, header_value);
        }

        Ok(headers)
    }

    fn next_api_key(&self) -> Option<String> {
        if self.api_keys.is_empty() {
            return None;
        }

        let index = self.next_key.fetch_add(1, Ordering::Relaxed) % self.api_keys.len();
        Some(self.api_keys[index].clone())
    }

    fn model_entry(&self, model: &str) -> Model {
        Model {
            id: format!("{}/{}", self.name, self.model_without_prefix(model)),
            provider: self.name.clone(),
            vendor: self.vendor.clone(),
            display_name: format!("{} ({})", self.model_without_prefix(model), self.name),
            max_input_tokens: None,
            max_output_tokens: None,
            supports_streaming: self.supports_streaming,
            supports_tools: self.supports_tools,
            supports_vision: self.supports_vision,
            supports_thinking: self.supports_thinking,
        }
    }

    fn build_chat_body(&self, request: ProviderRequest) -> serde_json::Value {
        let mut body = match request.extra {
            serde_json::Value::Object(map) => serde_json::Value::Object(map),
            _ => serde_json::json!({}),
        };

        body["model"] = serde_json::Value::String(self.model_without_prefix(&request.model));
        body["messages"] = serde_json::Value::Array(request.messages);
        body["stream"] = serde_json::Value::Bool(request.stream);

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(tools) = request.tools {
            body["tools"] = serde_json::json!(tools);
        }
        if let Some(tool_choice) = request.tool_choice {
            body["tool_choice"] = tool_choice;
        }
        if let Some(system) = request.system {
            if let Some(messages) = body["messages"].as_array_mut() {
                messages.insert(0, serde_json::json!({"role": "system", "content": system}));
            }
        }

        body
    }

    async fn send_chat(&self, body: serde_json::Value) -> Result<reqwest::Response> {
        let resp = self
            .client
            .post(self.chat_completions_url())
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .context("Failed to send common provider chat completions request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            bail!(
                "Common provider '{}' chat completions request failed: HTTP {} - {}",
                self.name,
                status,
                err_body
            );
        }

        Ok(resp)
    }
}

#[async_trait]
impl Provider for CommonProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_healthy(&self) -> bool {
        true
    }

    fn default_model(&self) -> Option<String> {
        self.configured_models
            .first()
            .map(|model| self.model_without_prefix(model))
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        if !self.configured_models.is_empty() {
            return Ok(self
                .configured_models
                .iter()
                .map(|model| self.model_entry(model))
                .collect());
        }

        let resp = self
            .client
            .get(self.models_url())
            .headers(self.headers()?)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Failed to fetch models from common provider '{}'",
                    self.name
                )
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!(
                "Failed to fetch models from common provider '{}': HTTP {} - {}",
                self.name,
                status,
                body
            );
        }

        let value: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse models response")?;
        let models = value
            .get("data")
            .and_then(|data| data.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(|id| id.as_str()))
                    .map(|model| self.model_entry(model))
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse> {
        let stream = request.stream;
        let body = self.build_chat_body(request);
        let resp = self.send_chat(body).await?;

        if stream {
            let byte_stream = resp
                .bytes_stream()
                .map(|r| r.map_err(|e| anyhow::anyhow!(e)));
            Ok(ProviderResponse::Stream(Box::pin(byte_stream)))
        } else {
            let json: serde_json::Value = resp
                .json()
                .await
                .context("Failed to parse common provider chat response")?;
            Ok(ProviderResponse::Complete(json))
        }
    }

    fn supports_passthrough(&self, format: OutputFormat) -> bool {
        format == OutputFormat::OpenAI
    }

    async fn passthrough(
        &self,
        model: &str,
        mut body: serde_json::Value,
        format: OutputFormat,
        stream: bool,
    ) -> Result<ProviderResponse> {
        if format != OutputFormat::OpenAI {
            bail!("common provider only supports OpenAI-compatible passthrough");
        }

        body["model"] = serde_json::Value::String(self.model_without_prefix(model));
        let resp = self.send_chat(body).await?;

        if stream {
            let byte_stream = resp
                .bytes_stream()
                .map(|r| r.map_err(|e| anyhow::anyhow!(e)));
            Ok(ProviderResponse::Stream(Box::pin(byte_stream)))
        } else {
            let json: serde_json::Value = resp
                .json()
                .await
                .context("Failed to parse common provider passthrough response")?;
            Ok(ProviderResponse::Complete(json))
        }
    }
}

fn resolve_api_keys(
    api_key: &str,
    api_keys: &[String],
    api_key_env: &str,
    api_key_envs: &[String],
) -> Result<Vec<String>> {
    let mut resolved = Vec::new();
    push_key_values(&mut resolved, api_key);
    for key in api_keys {
        push_key_values(&mut resolved, key);
    }

    if !api_key_env.trim().is_empty() {
        let value = crate::config::resolve_env_var(api_key_env)?;
        push_key_values(&mut resolved, &value);
    }
    for env_name in api_key_envs {
        if !env_name.trim().is_empty() {
            let value = crate::config::resolve_env_var(env_name)?;
            push_key_values(&mut resolved, &value);
        }
    }

    Ok(resolved)
}

fn push_key_values(keys: &mut Vec<String>, value: &str) {
    for key in value
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        keys.push(key.to_string());
    }
}

fn has_auth_scheme(api_key: &str, auth_scheme: &str) -> bool {
    api_key
        .get(..auth_scheme.len())
        .map(|prefix| prefix.eq_ignore_ascii_case(auth_scheme))
        .unwrap_or(false)
        && api_key[auth_scheme.len()..].starts_with(' ')
}

fn join_url(base: &str, path: &str) -> String {
    if path.is_empty() {
        base.to_string()
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

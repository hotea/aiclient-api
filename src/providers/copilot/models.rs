use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::providers::Model;
use super::headers::CopilotHeaders;

#[derive(Debug, Deserialize)]
struct ModelCapabilities {
    #[serde(default)]
    supports_streaming: Option<bool>,
    #[serde(rename = "type")]
    model_type: Option<String>,
    #[serde(default)]
    tokenizer: Option<String>,
    limits: Option<ModelLimits>,
    supports: Option<ModelSupports>,
}

#[derive(Debug, Deserialize)]
struct ModelLimits {
    max_context_window_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
    max_prompt_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ModelSupports {
    streaming: Option<bool>,
    tool_calls: Option<bool>,
    parallel_tool_calls: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    id: String,
    name: Option<String>,
    vendor: Option<String>,
    capabilities: Option<ModelCapabilities>,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<RawModel>,
}

pub async fn fetch_models(
    headers: &CopilotHeaders,
    copilot_token: &str,
) -> Result<Vec<Model>> {
    let client = reqwest::Client::new();
    let hdrs = headers.build(copilot_token);

    let resp = client
        .get("https://api.githubcopilot.com/models")
        .headers(hdrs)
        .send()
        .await
        .context("Failed to fetch models from Copilot")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to fetch models: HTTP {} - {}", status, body);
    }

    let models_resp: ModelsResponse = resp
        .json()
        .await
        .context("Failed to parse models response")?;

    let models = models_resp
        .data
        .into_iter()
        .map(|m| {
            let caps = m.capabilities.as_ref();
            let limits = caps.and_then(|c| c.limits.as_ref());
            let supports = caps.and_then(|c| c.supports.as_ref());

            let max_input_tokens = limits
                .and_then(|l| l.max_prompt_tokens.or(l.max_context_window_tokens));
            let max_output_tokens = limits.and_then(|l| l.max_output_tokens);

            let supports_streaming = supports
                .and_then(|s| s.streaming)
                .or_else(|| caps.and_then(|c| c.supports_streaming))
                .unwrap_or(true);

            let supports_tools = supports
                .and_then(|s| s.tool_calls)
                .unwrap_or(false);

            Model {
                id: format!("copilot/{}", m.id),
                provider: "copilot".to_string(),
                vendor: m.vendor.unwrap_or_else(|| "github".to_string()),
                display_name: m.name.unwrap_or_else(|| m.id.clone()),
                max_input_tokens,
                max_output_tokens,
                supports_streaming,
                supports_tools,
                supports_vision: false,
                supports_thinking: false,
            }
        })
        .collect();

    Ok(models)
}

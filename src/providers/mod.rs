use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub mod copilot;
pub mod router;

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

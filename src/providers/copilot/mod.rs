pub mod client;
pub mod headers;
pub mod models;

use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

use crate::auth::copilot::fetch_copilot_token;
use crate::config::types::AccountType;
use crate::providers::{Model, OutputFormat, Provider, ProviderRequest, ProviderResponse};
use client::CopilotClient;
use headers::CopilotHeaders;

pub struct CopilotToken {
    pub copilot_token: String,
    pub expires_at: i64,
    pub refresh_in: u64,
}

pub struct CopilotProvider {
    client: CopilotClient,
    headers: Arc<headers::CopilotHeaders>,
    token: Arc<RwLock<Option<CopilotToken>>>,
    github_token: String,
    account_type: AccountType,
    healthy: AtomicBool,
}

impl CopilotProvider {
    pub fn new(
        github_token: String,
        account_type: AccountType,
        vscode_version: &str,
    ) -> Arc<Self> {
        let client = CopilotClient::new(&account_type);
        let headers = Arc::new(CopilotHeaders::new(vscode_version));

        Arc::new(Self {
            client,
            headers,
            token: Arc::new(RwLock::new(None)),
            github_token,
            account_type,
            healthy: AtomicBool::new(false),
        })
    }

    pub fn start(self: &Arc<Self>) {
        self.headers.start_session_rotation();
        self.start_token_refresh();
    }

    fn start_token_refresh(self: &Arc<Self>) {
        let provider = self.clone();
        tokio::spawn(async move {
            loop {
                match fetch_copilot_token(&provider.github_token).await {
                    Ok(resp) => {
                        let refresh_in = resp.refresh_in;
                        {
                            let mut token = provider.token.write().await;
                            *token = Some(CopilotToken {
                                copilot_token: resp.token,
                                expires_at: resp.expires_at,
                                refresh_in: resp.refresh_in,
                            });
                        }
                        provider.healthy.store(true, Ordering::Relaxed);
                        tracing::info!("Copilot token refreshed successfully");

                        let sleep_secs = if refresh_in > 60 {
                            refresh_in - 60
                        } else {
                            1
                        };
                        sleep(Duration::from_secs(sleep_secs)).await;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to fetch Copilot token: {:#}", e);
                        sleep(Duration::from_secs(15)).await;
                    }
                }
            }
        });
    }

    async fn get_copilot_token(&self) -> Result<String> {
        let token = self.token.read().await;
        token
            .as_ref()
            .map(|t| t.copilot_token.clone())
            .context("Copilot token not yet available")
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn name(&self) -> &str {
        "copilot"
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        let copilot_token = self.get_copilot_token().await?;
        models::fetch_models(&self.headers, &copilot_token).await
    }

    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse> {
        let copilot_token = self.get_copilot_token().await?;
        let headers = self.headers.build(&copilot_token);

        // Strip provider prefix from model id if present
        let model_id = if let Some(stripped) = request.model.strip_prefix("copilot/") {
            stripped.to_string()
        } else {
            request.model.clone()
        };

        let mut body = serde_json::json!({
            "model": model_id,
            "messages": request.messages,
            "stream": request.stream,
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(max_tok) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tok);
        }
        if let Some(tools) = request.tools {
            body["tools"] = serde_json::json!(tools);
        }
        if let Some(tc) = request.tool_choice {
            body["tool_choice"] = tc;
        }
        if let Some(system) = request.system {
            // Prepend system as a system message
            if let Some(messages) = body["messages"].as_array_mut() {
                messages.insert(0, serde_json::json!({"role": "system", "content": system}));
            }
        }

        if request.stream {
            let resp = self
                .client
                .chat_completions(headers, body, true)
                .await?;

            let byte_stream = resp
                .bytes_stream()
                .map(|r| r.map(|b| b.into()).map_err(|e| anyhow::anyhow!(e)));

            Ok(ProviderResponse::Stream(Box::pin(byte_stream)))
        } else {
            let resp = self
                .client
                .chat_completions(headers, body, false)
                .await?;

            let json: serde_json::Value = resp.json().await.context("Failed to parse chat response")?;
            Ok(ProviderResponse::Complete(json))
        }
    }

    fn supports_passthrough(&self, _format: OutputFormat) -> bool {
        true
    }

    async fn passthrough(
        &self,
        _model: &str,
        body: serde_json::Value,
        format: OutputFormat,
        stream: bool,
    ) -> Result<ProviderResponse> {
        let copilot_token = self.get_copilot_token().await?;
        let headers = self.headers.build(&copilot_token);

        let resp = match format {
            OutputFormat::OpenAI => {
                self.client.chat_completions(headers, body, stream).await?
            }
            OutputFormat::Anthropic => {
                self.client.messages(headers, body, stream).await?
            }
        };

        if stream {
            let byte_stream = resp
                .bytes_stream()
                .map(|r| r.map(|b: Bytes| b).map_err(|e| anyhow::anyhow!(e)));

            Ok(ProviderResponse::Stream(Box::pin(byte_stream)))
        } else {
            let json: serde_json::Value =
                resp.json().await.context("Failed to parse passthrough response")?;
            Ok(ProviderResponse::Complete(json))
        }
    }
}

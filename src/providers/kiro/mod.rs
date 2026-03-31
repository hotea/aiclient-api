pub mod client;
pub mod cw_types;
pub mod eventstream;
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

use crate::auth::{kiro as kiro_auth, TokenData};
use crate::providers::{Model, OutputFormat, Provider, ProviderRequest, ProviderResponse};
use client::KiroClient;
use cw_types::{
    CWAssistantMessage, CWConversationState, CWCurrentMessage, CWGenerateRequest, CWHistoryItem,
    CWHistoryUserMessage, CWUserInputMessage,
};
use models::{kiro_models, to_cw_model_id};

pub struct KiroToken {
    pub access_token: String,
    pub expires_at: i64,
}

pub struct KiroProvider {
    client: KiroClient,
    token: Arc<RwLock<Option<KiroToken>>>,
    region: String,
    idc_region: Option<String>,
    auth_method: String,
    profile_arn: Option<String>,
    healthy: AtomicBool,
    // For Builder ID / IDC refresh
    client_id: Option<String>,
    client_secret: Option<String>,
    refresh_token: Arc<RwLock<String>>,
    http_client: reqwest::Client,
}

impl KiroProvider {
    pub fn new(token_data: &TokenData, region: &str) -> Result<Arc<Self>> {
        match token_data {
            TokenData::Kiro {
                access_token,
                refresh_token,
                client_id,
                client_secret,
                auth_method,
                idc_region,
                profile_arn,
                expires_at,
                ..
            } => {
                let kiro_client = KiroClient::new(region);
                let http_client = reqwest::Client::new();

                Ok(Arc::new(Self {
                    client: kiro_client,
                    token: Arc::new(RwLock::new(Some(KiroToken {
                        access_token: access_token.clone(),
                        expires_at: *expires_at,
                    }))),
                    region: region.to_string(),
                    idc_region: idc_region.clone(),
                    auth_method: auth_method.clone(),
                    profile_arn: profile_arn.clone(),
                    healthy: AtomicBool::new(false),
                    client_id: client_id.clone(),
                    client_secret: client_secret.clone(),
                    refresh_token: Arc::new(RwLock::new(refresh_token.clone())),
                    http_client,
                }))
            }
            _ => {
                anyhow::bail!("Expected Kiro TokenData, got a different variant");
            }
        }
    }

    pub fn start(self: &Arc<Self>) {
        self.start_token_refresh();
    }

    fn start_token_refresh(self: &Arc<Self>) {
        let provider = self.clone();
        tokio::spawn(async move {
            let mut consecutive_failures: u32 = 0;
            loop {
                // Check if we need to refresh (within 5 minutes of expiry)
                let needs_refresh = {
                    let token = provider.token.read().await;
                    match token.as_ref() {
                        Some(t) => {
                            let now = chrono::Utc::now().timestamp();
                            t.expires_at - now < 300 // 5 minutes
                        }
                        None => true,
                    }
                };

                if needs_refresh {
                    let refresh_token_val = provider.refresh_token.read().await.clone();
                    let result = if provider.auth_method == "builder_id"
                        || provider.auth_method == "idc"
                    {
                        if let (Some(client_id), Some(client_secret)) = (
                            &provider.client_id,
                            &provider.client_secret,
                        ) {
                            // Use idc_region for OIDC endpoint if set, otherwise fall back to region
                            let refresh_region = provider
                                .idc_region
                                .as_deref()
                                .unwrap_or(&provider.region);
                            kiro_auth::refresh_builder_id(
                                &provider.http_client,
                                refresh_region,
                                &refresh_token_val,
                                client_id,
                                client_secret,
                            )
                            .await
                        } else {
                            Err(anyhow::anyhow!("Missing client_id or client_secret for Builder ID refresh"))
                        }
                    } else {
                        kiro_auth::refresh_social(
                            &provider.http_client,
                            &provider.region,
                            &refresh_token_val,
                        )
                        .await
                    };

                    match result {
                        Ok(resp) => {
                            consecutive_failures = 0;
                            let expires_at = chrono::Utc::now().timestamp() + resp.expires_in as i64;
                            {
                                let mut token = provider.token.write().await;
                                *token = Some(KiroToken {
                                    access_token: resp.access_token,
                                    expires_at,
                                });
                            }
                            {
                                let mut rt = provider.refresh_token.write().await;
                                *rt = resp.refresh_token;
                            }
                            provider.healthy.store(true, Ordering::Relaxed);
                            tracing::info!("Kiro token refreshed successfully");
                        }
                        Err(e) => {
                            consecutive_failures += 1;
                            tracing::warn!(
                                "Failed to refresh Kiro token ({} consecutive): {:#}",
                                consecutive_failures,
                                e
                            );
                            if consecutive_failures >= 3 {
                                provider.healthy.store(false, Ordering::Relaxed);
                            }
                            sleep(Duration::from_secs(15)).await;
                            continue;
                        }
                    }
                } else {
                    // Token is valid, mark healthy on first run
                    if !provider.healthy.load(Ordering::Relaxed) {
                        provider.healthy.store(true, Ordering::Relaxed);
                        tracing::info!("Kiro token is still valid, provider healthy");
                    }
                }

                // Sleep until 5 minutes before expiry
                let sleep_secs = {
                    let token = provider.token.read().await;
                    match token.as_ref() {
                        Some(t) => {
                            let now = chrono::Utc::now().timestamp();
                            let remaining = t.expires_at - now;
                            if remaining > 300 {
                                (remaining - 300) as u64
                            } else {
                                1
                            }
                        }
                        None => 60,
                    }
                };
                sleep(Duration::from_secs(sleep_secs)).await;
            }
        });
    }

    async fn get_access_token(&self) -> Result<String> {
        let token = self.token.read().await;
        token
            .as_ref()
            .map(|t| t.access_token.clone())
            .context("Kiro access token not yet available")
    }

    fn build_cw_request(&self, request: &ProviderRequest) -> Result<CWGenerateRequest> {
        // Strip provider prefix from model id if present
        let model_internal = if let Some(stripped) = request.model.strip_prefix("kiro/") {
            stripped.to_string()
        } else {
            request.model.clone()
        };
        let cw_model_id = to_cw_model_id(&model_internal);

        // Separate messages into history and the last user message
        let messages = &request.messages;
        let (last_user_content, history_messages) = extract_last_user_and_history(messages)?;

        // Build history items
        let mut history: Vec<CWHistoryItem> = Vec::new();

        // Add system prompt as first user message if present
        if let Some(system) = &request.system {
            history.push(CWHistoryItem {
                user_input_message: Some(CWHistoryUserMessage {
                    content: system.clone(),
                }),
                assistant_response_message: Some(CWAssistantMessage {
                    content: "Understood.".to_string(),
                }),
            });
        }

        for item in history_messages {
            history.push(item);
        }

        let cw_request = CWGenerateRequest {
            conversation_state: CWConversationState {
                chat_trigger_type: "MANUAL".to_string(),
                current_message: CWCurrentMessage {
                    user_input_message: CWUserInputMessage {
                        content: last_user_content,
                        model_id: cw_model_id,
                        origin: "AI_EDITOR".to_string(),
                    },
                },
                history,
            },
            profile_arn: self.profile_arn.clone(),
        };

        Ok(cw_request)
    }
}

fn extract_last_user_and_history(
    messages: &[serde_json::Value],
) -> Result<(String, Vec<CWHistoryItem>)> {
    // Find the last user message
    let last_user_idx = messages
        .iter()
        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .context("No user message found in request")?;

    let last_user_content = extract_message_content(&messages[last_user_idx]);

    // Build history pairs from preceding messages
    let preceding = &messages[..last_user_idx];
    let mut history = Vec::new();
    let mut i = 0;

    // Skip initial system messages (handled separately)
    while i < preceding.len() {
        let role = preceding[i]
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("");

        if role == "system" {
            i += 1;
            continue;
        }

        if role == "user" {
            let user_content = extract_message_content(&preceding[i]);
            i += 1;

            // Look for following assistant message
            let assistant_content = if i < preceding.len()
                && preceding[i]
                    .get("role")
                    .and_then(|r| r.as_str())
                    == Some("assistant")
            {
                let content = extract_message_content(&preceding[i]);
                i += 1;
                content
            } else {
                String::new()
            };

            history.push(CWHistoryItem {
                user_input_message: Some(CWHistoryUserMessage {
                    content: user_content,
                }),
                assistant_response_message: if assistant_content.is_empty() {
                    None
                } else {
                    Some(CWAssistantMessage {
                        content: assistant_content,
                    })
                },
            });
        } else if role == "assistant" {
            history.push(CWHistoryItem {
                user_input_message: None,
                assistant_response_message: Some(CWAssistantMessage {
                    content: extract_message_content(&preceding[i]),
                }),
            });
            i += 1;
        } else {
            i += 1;
        }
    }

    Ok((last_user_content, history))
}

fn extract_message_content(msg: &serde_json::Value) -> String {
    match msg.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => {
            // Handle array content (e.g. Anthropic format)
            arr.iter()
                .filter_map(|part| {
                    if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                        part.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => String::new(),
    }
}

#[async_trait]
impl Provider for KiroProvider {
    fn name(&self) -> &str {
        "kiro"
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    fn supports_passthrough(&self, _format: OutputFormat) -> bool {
        false
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        Ok(kiro_models())
    }

    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse> {
        let access_token = self.get_access_token().await?;
        let cw_request = self.build_cw_request(&request)?;

        let resp = self
            .client
            .generate_assistant_response(&access_token, cw_request, self.profile_arn.as_deref())
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("CodeWhisperer API error: HTTP {} - {}", status, body);
        }

        if request.stream {
            // For streaming, return the raw byte stream
            // The consumer will need to parse the event stream format
            let byte_stream = resp
                .bytes_stream()
                .map(|r| r.map(|b: Bytes| b).map_err(|e| anyhow::anyhow!(e)));
            Ok(ProviderResponse::Stream(Box::pin(byte_stream)))
        } else {
            // For non-streaming, AWS still returns event-stream format
            // We need to parse it and collect all content
            let body_bytes = resp.bytes().await.context("Failed to read response body")?;
            
            tracing::debug!("Raw response size: {} bytes", body_bytes.len());
            
            // Parse the event stream
            let events = eventstream::parse_event_stream(&body_bytes)
                .context("Failed to parse AWS event stream")?;
            
            tracing::info!("Parsed {} events from event stream", events.len());
            
            // Collect all content
            let content = eventstream::collect_content(&events);
            
            if content.is_empty() {
                anyhow::bail!("No content found in response events");
            }
            
            tracing::info!("Collected content: {}", content);
            
            // Extract metering information for usage stats
            let mut input_tokens = 0u32;
            let mut output_tokens = 0u32;
            
            for event in &events {
                if let eventstream::KiroEvent::Metering(metering) = event {
                    if let Some(usage) = metering.usage {
                        // AWS CodeWhisperer returns usage in tokens
                        // Heuristic: assume roughly 50/50 split or use prompt length estimate
                        // For more accuracy, we'd need to count tokens in the content
                        let total = usage as u32;
                        // Estimate: count words in content for output, rest is input
                        let content_word_count = content.split_whitespace().count() as u32;
                        output_tokens = content_word_count.max(total / 4); // rough estimate
                        input_tokens = total.saturating_sub(output_tokens);
                    }
                }
            }
            
            // Build a response in Anthropic's format
            let json = serde_json::json!({
                "id": format!("msg_{}", uuid::Uuid::new_v4()),
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": content
                    }
                ],
                "model": request.model,
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens
                }
            });
            
            Ok(ProviderResponse::Complete(json))
        }
    }
}

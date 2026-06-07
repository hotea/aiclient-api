pub mod client;
pub mod cw_types;
pub mod eventstream;
pub mod models;

use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{sleep, timeout};

use crate::auth::{kiro as kiro_auth, TokenData};
use crate::providers::{Model, OutputFormat, Provider, ProviderRequest, ProviderResponse};
use client::KiroClient;
use cw_types::{
    CWAssistantMessage, CWConversationState, CWCurrentMessage, CWGenerateRequest, CWHistoryItem,
    CWHistoryToolUse, CWHistoryUserMessage, CWInferenceConfig, CWTool, CWToolInputSchema,
    CWToolResult, CWToolResultContentBlock, CWToolSpecification, CWUserInputMessage,
    CWUserInputMessageContext,
};
use models::{kiro_models, to_cw_model_id};

const KIRO_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const KIRO_STREAM_MAX_OUTPUT_CHARS: usize = 512 * 1024;

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
                    let result =
                        if provider.auth_method == "builder_id" || provider.auth_method == "idc" {
                            if let (Some(client_id), Some(client_secret)) =
                                (&provider.client_id, &provider.client_secret)
                            {
                                // Use idc_region for OIDC endpoint if set, otherwise fall back to region
                                let refresh_region =
                                    provider.idc_region.as_deref().unwrap_or(&provider.region);
                                kiro_auth::refresh_builder_id(
                                    &provider.http_client,
                                    refresh_region,
                                    &refresh_token_val,
                                    client_id,
                                    client_secret,
                                )
                                .await
                            } else {
                                Err(anyhow::anyhow!(
                                    "Missing client_id or client_secret for Builder ID refresh"
                                ))
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
                            let expires_at =
                                chrono::Utc::now().timestamp() + resp.expires_in as i64;
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
        let history_tools = build_cw_tools(request.tools.as_ref());
        let (last_user_message, history_messages) =
            extract_last_user_and_history(messages, &cw_model_id, &history_tools)?;

        // Build history items
        let mut history: Vec<CWHistoryItem> = Vec::new();

        // Add system prompt as first user message if present
        if let Some(system) = &request.system {
            history.push(CWHistoryItem {
                user_input_message: Some(CWHistoryUserMessage {
                    content: system.clone(),
                    model_id: Some(cw_model_id.clone()),
                    origin: Some("AI_EDITOR".to_string()),
                    user_input_message_context: None,
                }),
                assistant_response_message: Some(CWAssistantMessage {
                    content: "Understood.".to_string(),
                    tool_uses: None,
                }),
            });
        }

        for item in history_messages {
            history.push(item);
        }

        history = sanitize_history(history, &cw_model_id);

        let current_tools = history_tools.clone();
        let current_tool_results = extract_tool_results_from_message(&last_user_message);
        let mut current_content = extract_message_content(&last_user_message);
        let conversation_id = extract_conversation_id(&request.extra)
            .or_else(|| extract_conversation_id_from_messages(&request.messages))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let additional_model_request_fields =
            extract_additional_model_request_fields(&request.extra);
        if current_content.is_empty() && !current_tool_results.is_empty() {
            current_content = "Continue".to_string();
        }

        let cw_request = CWGenerateRequest {
            conversation_state: CWConversationState {
                conversation_id,
                agent_continuation_id: uuid::Uuid::new_v4().to_string(),
                agent_task_type: "vibe".to_string(),
                chat_trigger_type: "MANUAL".to_string(),
                current_message: CWCurrentMessage {
                    user_input_message: CWUserInputMessage {
                        content: current_content,
                        model_id: cw_model_id,
                        origin: "AI_EDITOR".to_string(),
                        user_input_message_context: build_user_input_context(
                            current_tools,
                            current_tool_results,
                        ),
                    },
                },
                history,
            },
            profile_arn: self.profile_arn.clone(),
            inference_config: Some(CWInferenceConfig {
                max_tokens: request.max_tokens,
                temperature: request.temperature,
            })
            .filter(|config| config.max_tokens.is_some() || config.temperature.is_some()),
            additional_model_request_fields,
        };

        Ok(cw_request)
    }
}

fn sanitize_history(mut history: Vec<CWHistoryItem>, model_id: &str) -> Vec<CWHistoryItem> {
    if history.is_empty() {
        return history;
    }

    history = relocate_tool_result_messages(history);
    history = remove_invalid_tool_result_messages(history);
    history = ensure_tool_uses_have_results(history);

    if history[0].user_input_message.is_none() {
        history.insert(0, synthetic_user_message("Hello", model_id));
    }

    let mut sanitized = Vec::with_capacity(history.len() + 1);
    let mut expect_user = true;

    for item in history {
        if item.user_input_message.is_some() {
            if !expect_user {
                sanitized.push(synthetic_assistant_message("Understood."));
            }
            sanitized.push(item);
            expect_user = false;
        } else if item.assistant_response_message.is_some() {
            if expect_user {
                sanitized.push(synthetic_user_message("Continue", model_id));
            }
            sanitized.push(item);
            expect_user = true;
        }
    }

    sanitized
}

fn has_tool_uses(item: &CWHistoryItem) -> bool {
    item.assistant_response_message
        .as_ref()
        .and_then(|message| message.tool_uses.as_ref())
        .map(|tool_uses| !tool_uses.is_empty())
        .unwrap_or(false)
}

fn has_tool_results(item: &CWHistoryItem) -> bool {
    item.user_input_message
        .as_ref()
        .and_then(|message| message.user_input_message_context.as_ref())
        .map(|context| !context.tool_results.is_empty())
        .unwrap_or(false)
}

fn tool_use_ids(item: &CWHistoryItem) -> Vec<String> {
    item.assistant_response_message
        .as_ref()
        .and_then(|message| message.tool_uses.as_ref())
        .into_iter()
        .flatten()
        .map(|tool_use| tool_use.tool_use_id.clone())
        .collect()
}

fn matching_tool_results(
    item: &CWHistoryItem,
    valid_tool_use_ids: &std::collections::HashSet<String>,
) -> Vec<CWToolResult> {
    let Some(context) = item
        .user_input_message
        .as_ref()
        .and_then(|message| message.user_input_message_context.as_ref())
    else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    context
        .tool_results
        .iter()
        .filter(|result| valid_tool_use_ids.contains(&result.tool_use_id))
        .filter(|result| seen.insert(result.tool_use_id.clone()))
        .cloned()
        .collect()
}

fn relocate_tool_result_messages(history: Vec<CWHistoryItem>) -> Vec<CWHistoryItem> {
    let mut result = Vec::with_capacity(history.len());
    let mut consumed = std::collections::HashSet::new();

    for (index, item) in history.iter().enumerate() {
        if consumed.contains(&index) {
            continue;
        }

        result.push(item.clone());

        if !has_tool_uses(item) {
            continue;
        }

        let valid_tool_use_ids = tool_use_ids(item)
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        for (candidate_index, candidate) in history.iter().enumerate().skip(index + 1) {
            if consumed.contains(&candidate_index) || !has_tool_results(candidate) {
                continue;
            }

            let matches = matching_tool_results(candidate, &valid_tool_use_ids);
            if matches.is_empty() {
                continue;
            }

            result.push(tool_result_history_item(matches));
            consumed.insert(candidate_index);
            break;
        }
    }

    result
}

fn remove_invalid_tool_result_messages(history: Vec<CWHistoryItem>) -> Vec<CWHistoryItem> {
    let mut result = Vec::with_capacity(history.len());

    for item in history {
        if !has_tool_results(&item) {
            result.push(item);
            continue;
        }

        let valid_tool_use_ids = result
            .last()
            .filter(|previous| has_tool_uses(previous))
            .map(|previous| {
                tool_use_ids(previous)
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();

        let matches = matching_tool_results(&item, &valid_tool_use_ids);
        if matches.is_empty() {
            if let Some(stripped) = strip_tool_results(item) {
                result.push(stripped);
            }
        } else {
            result.push(tool_result_history_item(matches));
        }
    }

    result
}

fn ensure_tool_uses_have_results(history: Vec<CWHistoryItem>) -> Vec<CWHistoryItem> {
    let mut result = Vec::with_capacity(history.len() + 2);
    let mut index = 0;

    while index < history.len() {
        let item = &history[index];
        result.push(item.clone());

        if !has_tool_uses(item) {
            index += 1;
            continue;
        }

        let valid_tool_use_ids = tool_use_ids(item);
        let id_set = valid_tool_use_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let next_matches = history
            .get(index + 1)
            .map(|next| matching_tool_results(next, &id_set))
            .unwrap_or_default();

        if next_matches.len() != id_set.len() {
            let mut completed = next_matches;
            let existing_ids = completed
                .iter()
                .map(|result| result.tool_use_id.clone())
                .collect::<std::collections::HashSet<_>>();
            completed.extend(
                valid_tool_use_ids
                    .into_iter()
                    .filter(|tool_use_id| !existing_ids.contains(tool_use_id))
                    .map(failed_tool_result),
            );
            result.push(tool_result_history_item(completed));
            if matches!(history.get(index + 1), Some(next) if has_tool_results(next)) {
                index += 1;
            }
        }

        index += 1;
    }

    result
}

fn strip_tool_results(mut item: CWHistoryItem) -> Option<CWHistoryItem> {
    let user = item.user_input_message.as_mut()?;
    if user.content.trim().is_empty() {
        return None;
    }
    user.user_input_message_context = None;
    Some(item)
}

fn tool_result_history_item(tool_results: Vec<CWToolResult>) -> CWHistoryItem {
    CWHistoryItem {
        user_input_message: Some(CWHistoryUserMessage {
            content: "Tool results provided.".to_string(),
            model_id: None,
            origin: Some("AI_EDITOR".to_string()),
            user_input_message_context: Some(CWUserInputMessageContext {
                tools: Vec::new(),
                tool_results,
            }),
        }),
        assistant_response_message: None,
    }
}

fn failed_tool_result(tool_use_id: String) -> CWToolResult {
    CWToolResult {
        tool_use_id,
        content: vec![CWToolResultContentBlock {
            text: Some("Tool execution failed".to_string()),
            json: None,
        }],
        status: Some("error".to_string()),
    }
}

fn synthetic_user_message(content: &str, model_id: &str) -> CWHistoryItem {
    CWHistoryItem {
        user_input_message: Some(CWHistoryUserMessage {
            content: content.to_string(),
            model_id: Some(model_id.to_string()),
            origin: Some("AI_EDITOR".to_string()),
            user_input_message_context: None,
        }),
        assistant_response_message: None,
    }
}

fn synthetic_assistant_message(content: &str) -> CWHistoryItem {
    CWHistoryItem {
        user_input_message: None,
        assistant_response_message: Some(CWAssistantMessage {
            content: content.to_string(),
            tool_uses: None,
        }),
    }
}

fn extract_additional_model_request_fields(extra: &Value) -> Option<Value> {
    let Value::Object(map) = extra else {
        return None;
    };

    if let Some(value) = map.get("thinking") {
        return Some(json!({ "thinking": value }));
    }

    map.get("additionalModelRequestFields").cloned()
}

fn extract_last_user_and_history(
    messages: &[serde_json::Value],
    model_id: &str,
    _tools: &[CWTool],
) -> Result<(Value, Vec<CWHistoryItem>)> {
    // Find the last user message
    let last_user_idx = messages
        .iter()
        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .context("No user message found in request")?;

    let last_user_message = messages[last_user_idx].clone();

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
            let user_message = preceding[i].clone();
            let user_content = extract_message_content(&user_message);
            i += 1;

            // Look for following assistant message
            let assistant_message = if i < preceding.len()
                && preceding[i].get("role").and_then(|r| r.as_str()) == Some("assistant")
            {
                let message = preceding[i].clone();
                i += 1;
                Some(message)
            } else {
                None
            };

            let assistant_content = assistant_message
                .as_ref()
                .map(extract_message_content)
                .unwrap_or_default();
            let assistant_tool_uses = assistant_message.as_ref().and_then(build_history_tool_uses);

            history.push(CWHistoryItem {
                user_input_message: Some(CWHistoryUserMessage {
                    content: if user_content.is_empty() {
                        "(empty)".to_string()
                    } else {
                        user_content
                    },
                    model_id: Some(model_id.to_string()),
                    origin: Some("AI_EDITOR".to_string()),
                    user_input_message_context: build_history_user_context(&user_message),
                }),
                assistant_response_message: None,
            });

            if !assistant_content.is_empty() || assistant_tool_uses.is_some() {
                history.push(CWHistoryItem {
                    user_input_message: None,
                    assistant_response_message: Some(CWAssistantMessage {
                        content: if assistant_content.is_empty() {
                            "(empty)".to_string()
                        } else {
                            assistant_content
                        },
                        tool_uses: assistant_tool_uses,
                    }),
                });
            }
        } else if role == "assistant" {
            history.push(CWHistoryItem {
                user_input_message: None,
                assistant_response_message: Some(CWAssistantMessage {
                    content: {
                        let content = extract_message_content(&preceding[i]);
                        if content.is_empty() {
                            "(empty)".to_string()
                        } else {
                            content
                        }
                    },
                    tool_uses: build_history_tool_uses(&preceding[i]),
                }),
            });
            i += 1;
        } else {
            i += 1;
        }
    }

    Ok((last_user_message, history))
}

fn extract_message_content(msg: &serde_json::Value) -> String {
    match msg.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => {
            let text = arr
                .iter()
                .filter_map(|part| {
                    if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                        part.get("text")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            if !text.is_empty() {
                return text;
            }

            String::new()
        }
        _ => String::new(),
    }
}

fn build_cw_tools(tools: Option<&Vec<Value>>) -> Vec<CWTool> {
    tools
        .into_iter()
        .flat_map(|tools| tools.iter())
        .filter_map(|tool| {
            let function = match tool.get("type").and_then(|v| v.as_str()) {
                Some("function") => tool.get("function").unwrap_or(tool),
                _ => tool.get("function").unwrap_or(tool),
            };

            let name = function.get("name").and_then(|v| v.as_str())?.to_string();
            let description = function
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let schema = function
                .get("parameters")
                .cloned()
                .or_else(|| function.get("input_schema").cloned())
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));

            Some(CWTool {
                tool_specification: CWToolSpecification {
                    name,
                    description,
                    input_schema: CWToolInputSchema { json: schema },
                },
            })
        })
        .collect()
}

fn extract_tool_results_from_message(msg: &Value) -> Vec<CWToolResult> {
    let Some(content) = msg.get("content").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    content
        .iter()
        .filter(|block| block.get("type").and_then(|v| v.as_str()) == Some("tool_result"))
        .filter_map(|block| {
            let tool_use_id = block
                .get("tool_use_id")
                .and_then(|v| v.as_str())?
                .to_string();
            let status = if block
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                Some("error".to_string())
            } else {
                Some("success".to_string())
            };

            let content_blocks = match block.get("content") {
                Some(Value::String(text)) => vec![CWToolResultContentBlock {
                    text: Some(text.clone()),
                    json: None,
                }],
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(|item| match item.get("type").and_then(|v| v.as_str()) {
                        Some("text") => Some(CWToolResultContentBlock {
                            text: item
                                .get("text")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            json: None,
                        }),
                        Some("json") => Some(CWToolResultContentBlock {
                            text: Some(
                                item.get("json").cloned().unwrap_or(Value::Null).to_string(),
                            ),
                            json: None,
                        }),
                        _ if item.is_object() || item.is_array() => {
                            Some(CWToolResultContentBlock {
                                text: Some(item.to_string()),
                                json: None,
                            })
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                Some(value) if value.is_object() || value.is_array() => {
                    vec![CWToolResultContentBlock {
                        text: Some(value.to_string()),
                        json: None,
                    }]
                }
                _ => vec![CWToolResultContentBlock {
                    text: Some("(empty result)".to_string()),
                    json: None,
                }],
            };

            Some(CWToolResult {
                tool_use_id,
                content: if content_blocks.is_empty() {
                    vec![CWToolResultContentBlock {
                        text: Some("(empty result)".to_string()),
                        json: None,
                    }]
                } else {
                    content_blocks
                },
                status,
            })
        })
        .collect()
}

fn build_history_tool_uses(msg: &Value) -> Option<Vec<CWHistoryToolUse>> {
    let mut tool_uses = Vec::new();

    if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
        for tool_call in tool_calls {
            let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let function = tool_call.get("function").unwrap_or(tool_call);
            let Some(name) = function.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let input = function
                .get("arguments")
                .map(parse_json_or_string)
                .unwrap_or_else(|| json!({}));

            tool_uses.push(CWHistoryToolUse {
                tool_use_id: id.to_string(),
                name: name.to_string(),
                input,
            });
        }
    }

    if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
        for block in content {
            if block.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
                continue;
            }
            let Some(id) = block.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(name) = block.get("name").and_then(|v| v.as_str()) else {
                continue;
            };

            tool_uses.push(CWHistoryToolUse {
                tool_use_id: id.to_string(),
                name: name.to_string(),
                input: block.get("input").cloned().unwrap_or_else(|| json!({})),
            });
        }
    }

    if tool_uses.is_empty() {
        None
    } else {
        Some(tool_uses)
    }
}

fn build_history_user_context(msg: &Value) -> Option<CWUserInputMessageContext> {
    let tool_results = extract_tool_results_from_message(msg);
    build_user_input_context(Vec::new(), tool_results)
}

fn build_user_input_context(
    tools: Vec<CWTool>,
    tool_results: Vec<CWToolResult>,
) -> Option<CWUserInputMessageContext> {
    if tools.is_empty() && tool_results.is_empty() {
        None
    } else {
        Some(CWUserInputMessageContext {
            tools,
            tool_results,
        })
    }
}

fn parse_json_or_string(value: &Value) -> Value {
    match value {
        Value::String(text) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        other => other.clone(),
    }
}

fn extract_conversation_id(extra: &Value) -> Option<String> {
    let Value::Object(map) = extra else {
        return None;
    };

    map.get("conversation_id")
        .and_then(|v| v.as_str())
        .or_else(|| map.get("conversationId").and_then(|v| v.as_str()))
        .or_else(|| {
            map.get("metadata")
                .and_then(|v| v.get("conversation_id"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            map.get("metadata")
                .and_then(|v| v.get("conversationId"))
                .and_then(|v| v.as_str())
        })
        .map(str::to_string)
}

fn extract_conversation_id_from_messages(messages: &[Value]) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        let Value::Object(map) = message else {
            return None;
        };

        map.get("conversation_id")
            .and_then(|v| v.as_str())
            .or_else(|| map.get("conversationId").and_then(|v| v.as_str()))
            .map(str::to_string)
    })
}

fn tool_use_to_anthropic_block(tool_use: &eventstream::ToolUseCompleteEvent) -> Value {
    json!({
        "type": "tool_use",
        "id": tool_use.tool_use_id,
        "name": tool_use.name,
        "input": serde_json::from_str::<Value>(&tool_use.input).unwrap_or_else(|_| json!({})),
    })
}

fn tool_result_to_anthropic_block(tool_result: &eventstream::ToolResultEvent) -> Option<Value> {
    let tool_result = tool_result.tool_result.as_ref()?;
    let tool_use_id = tool_result.tool_use_id.as_ref()?;

    let mut content = Vec::new();
    for block in tool_result.content.as_ref().into_iter().flatten() {
        if let Some(text) = &block.text {
            content.push(json!({ "type": "text", "text": text }));
        } else if let Some(json_value) = &block.json {
            content.push(json_value.clone());
        }
    }

    Some(json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": if content.is_empty() { Value::String("(empty result)".to_string()) } else { Value::Array(content) },
        "is_error": tool_result.status.as_deref() == Some("error"),
    }))
}

#[async_trait]
impl Provider for KiroProvider {
    fn name(&self) -> &str {
        "kiro"
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    fn default_model(&self) -> Option<String> {
        Some("claude-sonnet-4-6".to_string())
    }

    fn prefers_native_anthropic_streaming(&self) -> bool {
        true
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

        if tracing::enabled!(tracing::Level::DEBUG) {
            let tool_result_count = cw_request
                .conversation_state
                .current_message
                .user_input_message
                .user_input_message_context
                .as_ref()
                .map(|ctx| ctx.tool_results.len())
                .unwrap_or(0);
            if tool_result_count > 0 {
                tracing::debug!(
                    tool_result_count,
                    payload = %serde_json::to_string_pretty(&cw_request).unwrap_or_default(),
                    "Sending Kiro continuation request with tool results"
                );
            }
        }

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
            let thinking = eventstream::collect_thinking(&events);
            let tool_uses = eventstream::collect_tool_uses(&events);
            let invalid_state = events.iter().find_map(|event| match event {
                eventstream::KiroEvent::InvalidState(invalid) => Some(invalid),
                _ => None,
            });
            let conversation_id = events.iter().find_map(|event| match event {
                eventstream::KiroEvent::MessageMetadata(metadata) => {
                    metadata.conversation_id.clone()
                }
                _ => None,
            });
            let utterance_id = events.iter().find_map(|event| match event {
                eventstream::KiroEvent::MessageMetadata(metadata) => metadata.utterance_id.clone(),
                _ => None,
            });

            if let Some(invalid) = invalid_state {
                tracing::warn!(
                    reason = invalid.reason.as_deref().unwrap_or("unknown"),
                    message = invalid.message.as_deref().unwrap_or(""),
                    "kiro returned invalid state event"
                );
            }

            if content.is_empty() && thinking.is_empty() && tool_uses.is_empty() {
                if let Some(invalid) = invalid_state {
                    anyhow::bail!(
                        "Kiro invalid state: {}{}",
                        invalid.reason.as_deref().unwrap_or("unknown"),
                        invalid
                            .message
                            .as_deref()
                            .map(|message| format!(" - {message}"))
                            .unwrap_or_default()
                    );
                }
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
            let mut content_blocks = Vec::new();

            if !thinking.is_empty() {
                content_blocks.push(json!({
                    "type": "thinking",
                    "thinking": thinking,
                }));
            }

            if !content.is_empty() {
                content_blocks.push(json!({
                    "type": "text",
                    "text": content,
                }));
            }

            for tool_use in &tool_uses {
                content_blocks.push(tool_use_to_anthropic_block(tool_use));
            }

            for event in &events {
                if let eventstream::KiroEvent::ToolResult(tool_result) = event {
                    if let Some(block) = tool_result_to_anthropic_block(tool_result) {
                        content_blocks.push(block);
                    }
                }
            }

            let stop_reason = if !tool_uses.is_empty() {
                "tool_use"
            } else {
                "end_turn"
            };

            let json = serde_json::json!({
                "id": format!("msg_{}", uuid::Uuid::new_v4()),
                "type": "message",
                "role": "assistant",
                "content": content_blocks,
                "model": request.model,
                "stop_reason": stop_reason,
                "conversation_id": conversation_id,
                "utterance_id": utterance_id,
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens
                }
            });

            Ok(ProviderResponse::Complete(json))
        }
    }
}

pub fn anthropic_stream_from_kiro(
    stream: std::pin::Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>,
    model: String,
    conversation_id: Option<String>,
) -> impl Stream<Item = anyhow::Result<Bytes>> {
    stream::unfold(
        (
            stream,
            Vec::<u8>::new(),
            AnthropicStreamState::new(model, conversation_id),
        ),
        |(mut stream, mut buffer, mut state)| async move {
            loop {
                if let Some(event) = state.pending.pop_front() {
                    return Some((Ok(Bytes::from(event)), (stream, buffer, state)));
                }

                if state.finished {
                    return None;
                }

                match timeout(KIRO_STREAM_IDLE_TIMEOUT, stream.as_mut().next()).await {
                    Err(_) => {
                        tracing::warn!(
                            idle_timeout_secs = KIRO_STREAM_IDLE_TIMEOUT.as_secs(),
                            output_chars = state.output_chars,
                            "kiro stream idle timeout; ending downstream stream"
                        );
                        state.finish_with_stop_reason("max_tokens");
                        if let Some(event) = state.pending.pop_front() {
                            return Some((Ok(Bytes::from(event)), (stream, buffer, state)));
                        }
                        return None;
                    }
                    Ok(Some(Ok(bytes))) => {
                        buffer.extend_from_slice(&bytes);
                        if let Ok(events) = eventstream::parse_event_stream(&buffer) {
                            if !events.is_empty() {
                                if !state.sent_message_start {
                                    for event in &events {
                                        if let eventstream::KiroEvent::MessageMetadata(metadata) =
                                            event
                                        {
                                            if let Some(conversation_id) = &metadata.conversation_id
                                            {
                                                state.conversation_id =
                                                    Some(conversation_id.clone());
                                            }
                                            if let Some(utterance_id) = &metadata.utterance_id {
                                                state.utterance_id = Some(utterance_id.clone());
                                            }
                                        }
                                    }
                                }
                                buffer.clear();
                                state.ingest(&events);
                                if state.output_chars >= KIRO_STREAM_MAX_OUTPUT_CHARS {
                                    tracing::warn!(
                                        max_output_chars = KIRO_STREAM_MAX_OUTPUT_CHARS,
                                        output_chars = state.output_chars,
                                        "kiro stream output guard triggered; ending downstream stream"
                                    );
                                    state.finish_with_stop_reason("max_tokens");
                                }
                                continue;
                            }
                        }
                    }
                    Ok(Some(Err(err))) => return Some((Err(err), (stream, buffer, state))),
                    Ok(None) => {
                        if state.finished {
                            return None;
                        }

                        if !buffer.is_empty() {
                            if let Ok(events) = eventstream::parse_event_stream(&buffer) {
                                if !events.is_empty() {
                                    state.ingest(&events);
                                    buffer.clear();
                                    continue;
                                }
                            }
                        }

                        state.finish();
                        if let Some(event) = state.pending.pop_front() {
                            return Some((Ok(Bytes::from(event)), (stream, buffer, state)));
                        }
                        return None;
                    }
                }
            }
        },
    )
}

struct AnthropicStreamState {
    model: String,
    pending: std::collections::VecDeque<Vec<u8>>,
    sent_message_start: bool,
    finished: bool,
    partial_tool_uses: std::collections::HashMap<String, (Option<String>, String)>,
    conversation_id: Option<String>,
    utterance_id: Option<String>,
    text_block_open: bool,
    text_block_index: u32,
    thinking_block_open: bool,
    thinking_block_index: u32,
    next_block_index: u32,
    stop_reason: Option<String>,
    output_tokens: u32,
    output_chars: usize,
}

impl AnthropicStreamState {
    fn new(model: String, conversation_id: Option<String>) -> Self {
        Self {
            model,
            pending: std::collections::VecDeque::new(),
            sent_message_start: false,
            finished: false,
            partial_tool_uses: std::collections::HashMap::new(),
            conversation_id,
            utterance_id: None,
            text_block_open: false,
            text_block_index: 0,
            thinking_block_open: false,
            thinking_block_index: 0,
            next_block_index: 0,
            stop_reason: None,
            output_tokens: 0,
            output_chars: 0,
        }
    }

    fn ingest(&mut self, events: &[eventstream::KiroEvent]) {
        self.ensure_message_start();

        for event in events {
            match event {
                eventstream::KiroEvent::Reasoning(reasoning) => {
                    if let Some(text) = &reasoning.text {
                        self.output_chars = self.output_chars.saturating_add(text.len());
                        if !self.thinking_block_open {
                            self.thinking_block_index = self.next_block_index;
                            self.next_block_index += 1;
                            self.pending.push_back(sse_event(
                                "content_block_start",
                                json!({
                                    "type": "content_block_start",
                                    "index": self.thinking_block_index,
                                    "content_block": {
                                        "type": "thinking",
                                        "thinking": "",
                                    }
                                }),
                            ));
                            self.thinking_block_open = true;
                        }
                        self.pending.push_back(sse_event(
                            "content_block_delta",
                            json!({
                                "type": "content_block_delta",
                                "index": self.thinking_block_index,
                                "delta": {
                                    "type": "thinking_delta",
                                    "thinking": text,
                                }
                            }),
                        ));
                    }
                }
                eventstream::KiroEvent::Content(content) => {
                    if self.thinking_block_open {
                        self.pending.push_back(sse_event(
                            "content_block_stop",
                            json!({
                                "type": "content_block_stop",
                                "index": self.thinking_block_index,
                            }),
                        ));
                        self.thinking_block_open = false;
                    }

                    let text = content.content.as_deref().unwrap_or("");
                    if !text.is_empty() {
                        if !self.text_block_open {
                            self.text_block_index = self.next_block_index;
                            self.next_block_index += 1;
                            self.pending.push_back(sse_event(
                                "content_block_start",
                                json!({
                                    "type": "content_block_start",
                                    "index": self.text_block_index,
                                    "content_block": {
                                        "type": "text",
                                        "text": "",
                                    }
                                }),
                            ));
                            self.text_block_open = true;
                        }
                        self.output_tokens = self
                            .output_tokens
                            .saturating_add(text.split_whitespace().count() as u32);
                        self.output_chars = self.output_chars.saturating_add(text.len());
                        self.pending.push_back(sse_event(
                            "content_block_delta",
                            json!({
                                "type": "content_block_delta",
                                "index": self.text_block_index,
                                "delta": {
                                    "type": "text_delta",
                                    "text": text,
                                }
                            }),
                        ));
                    }
                }
                eventstream::KiroEvent::ToolUse(tool_use) => {
                    let Some(tool_use_id) = tool_use.tool_use_id.as_ref() else {
                        continue;
                    };
                    let partial = self
                        .partial_tool_uses
                        .entry(tool_use_id.clone())
                        .or_insert_with(|| (None, String::new()));

                    if let Some(name) = &tool_use.name {
                        partial.0 = Some(name.clone());
                    }
                    if let Some(input) = &tool_use.input {
                        self.output_chars = self.output_chars.saturating_add(input.len());
                        partial.1.push_str(input);
                    }

                    if tool_use.stop.unwrap_or(false) {
                        let (name, input) = self
                            .partial_tool_uses
                            .remove(tool_use_id)
                            .unwrap_or_default();
                        self.emit_tool_use(
                            tool_use_id.clone(),
                            name.or_else(|| tool_use.name.clone()).unwrap_or_default(),
                            input,
                        );
                    }
                }
                eventstream::KiroEvent::ToolResult(tool_result) => {
                    let Some(block) = tool_result_to_anthropic_block(tool_result) else {
                        continue;
                    };
                    let index = self.next_block_index;
                    self.next_block_index += 1;
                    self.pending.push_back(sse_event(
                        "content_block_start",
                        json!({
                            "type": "content_block_start",
                            "index": index,
                            "content_block": block,
                        }),
                    ));
                    self.pending.push_back(sse_event(
                        "content_block_stop",
                        json!({
                            "type": "content_block_stop",
                            "index": index,
                        }),
                    ));
                }
                eventstream::KiroEvent::MessageMetadata(metadata) => {
                    if let Some(conversation_id) = &metadata.conversation_id {
                        self.conversation_id = Some(conversation_id.clone());
                    }
                    if let Some(utterance_id) = &metadata.utterance_id {
                        self.utterance_id = Some(utterance_id.clone());
                    }

                    if self.sent_message_start {
                        self.pending.push_back(sse_event(
                            "message_delta",
                            json!({
                                "type": "message_delta",
                                "delta": {
                                    "conversation_id": self.conversation_id,
                                    "utterance_id": self.utterance_id,
                                }
                            }),
                        ));
                    }
                }
                eventstream::KiroEvent::ContextUsage(_) => {
                    self.stop_reason
                        .get_or_insert_with(|| "end_turn".to_string());
                }
                _ => {}
            }
        }
    }

    fn finish(&mut self) {
        self.finish_with_stop_reason(
            self.stop_reason
                .clone()
                .unwrap_or_else(|| "end_turn".to_string()),
        );
    }

    fn finish_with_stop_reason(&mut self, stop_reason: impl Into<String>) {
        if self.finished {
            return;
        }

        let stop_reason = stop_reason.into();
        self.ensure_message_start();

        if self.thinking_block_open {
            self.pending.push_back(sse_event(
                "content_block_stop",
                json!({
                    "type": "content_block_stop",
                    "index": self.thinking_block_index,
                }),
            ));
            self.thinking_block_open = false;
        }

        if self.text_block_open {
            self.pending.push_back(sse_event(
                "content_block_stop",
                json!({
                    "type": "content_block_stop",
                    "index": self.text_block_index,
                }),
            ));
            self.text_block_open = false;
        }

        self.pending.push_back(sse_event(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": stop_reason,
                },
                "usage": {
                    "output_tokens": self.output_tokens,
                }
            }),
        ));
        self.pending.push_back(sse_event(
            "message_stop",
            json!({
                "type": "message_stop",
            }),
        ));
        self.finished = true;
    }

    fn ensure_message_start(&mut self) {
        if self.sent_message_start {
            return;
        }

        self.pending.push_back(sse_event(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.model,
                    "conversation_id": self.conversation_id,
                    "utterance_id": self.utterance_id,
                    "stop_reason": Value::Null,
                    "usage": {
                        "input_tokens": 0,
                        "output_tokens": 0,
                    }
                }
            }),
        ));
        self.sent_message_start = true;
    }

    fn emit_tool_use(&mut self, tool_use_id: String, name: String, input: String) {
        if self.text_block_open {
            self.pending.push_back(sse_event(
                "content_block_stop",
                json!({
                    "type": "content_block_stop",
                    "index": self.text_block_index,
                }),
            ));
            self.text_block_open = false;
        }

        if self.thinking_block_open {
            self.pending.push_back(sse_event(
                "content_block_stop",
                json!({
                    "type": "content_block_stop",
                    "index": self.thinking_block_index,
                }),
            ));
            self.thinking_block_open = false;
        }

        let index = self.next_block_index;
        self.next_block_index += 1;
        self.stop_reason = Some("tool_use".to_string());
        self.pending.push_back(sse_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "tool_use",
                    "id": tool_use_id,
                    "name": name,
                    "input": {},
                }
            }),
        ));
        self.pending.push_back(sse_event(
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": input,
                }
            }),
        ));
        self.pending.push_back(sse_event(
            "content_block_stop",
            json!({
                "type": "content_block_stop",
                "index": index,
            }),
        ));
    }
}

fn sse_event(event: &str, data: Value) -> Vec<u8> {
    format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(&data).unwrap_or_default()
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::{
        extract_conversation_id_from_messages, extract_last_user_and_history, KiroProvider,
    };
    use crate::providers::ProviderRequest;
    use serde_json::{json, Value};

    #[test]
    fn preserves_tool_only_assistant_history() {
        let messages = vec![
            json!({"role": "user", "content": "Use the tool"}),
            json!({
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }),
            json!({
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "Sunny"}
                ]
            }),
        ];

        let (_, history) =
            extract_last_user_and_history(&messages, "claude-sonnet-4-6", &[]).unwrap();

        let assistant = history[1].assistant_response_message.as_ref().unwrap();
        assert_eq!(assistant.content, "(empty)");
        let tool_uses = assistant.tool_uses.as_ref().unwrap();
        assert_eq!(tool_uses[0].tool_use_id, "toolu_1");
        assert_eq!(tool_uses[0].name, "get_weather");
        assert_eq!(tool_uses[0].input, json!({"location": "Tokyo"}));
    }

    #[test]
    fn keeps_tool_results_on_user_history_context() {
        let messages = vec![
            json!({
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "Sunny"}
                ]
            }),
            json!({"role": "assistant", "content": "It is sunny."}),
            json!({"role": "user", "content": "Thanks"}),
        ];

        let (_, history) =
            extract_last_user_and_history(&messages, "claude-sonnet-4-6", &[]).unwrap();

        let user_context = history[0]
            .user_input_message
            .as_ref()
            .and_then(|msg| msg.user_input_message_context.as_ref())
            .unwrap();
        assert_eq!(user_context.tool_results.len(), 1);
        assert_eq!(user_context.tool_results[0].tool_use_id, "toolu_1");
        assert_eq!(user_context.tool_results[0].content.len(), 1);
        assert_eq!(
            user_context.tool_results[0].content[0].text.as_deref(),
            Some("Sunny")
        );
        assert!(user_context.tool_results[0].content[0].json.is_none());
    }

    #[test]
    fn uses_empty_placeholder_for_tool_only_history_messages() {
        let messages = vec![
            json!({"role": "user", "content": "Use the tool"}),
            json!({
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }),
            json!({
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "Sunny"}
                ]
            }),
        ];

        let (_, history) =
            extract_last_user_and_history(&messages, "claude-sonnet-4-6", &[]).unwrap();

        assert_eq!(
            history[1]
                .assistant_response_message
                .as_ref()
                .unwrap()
                .content,
            "(empty)"
        );
    }

    #[test]
    fn uses_continue_for_tool_result_only_current_message() {
        let provider = KiroProvider {
            client: super::client::KiroClient::new("us-east-1"),
            token: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            region: "us-east-1".to_string(),
            idc_region: None,
            auth_method: "builder_id".to_string(),
            profile_arn: None,
            healthy: std::sync::atomic::AtomicBool::new(true),
            client_id: None,
            client_secret: None,
            refresh_token: std::sync::Arc::new(tokio::sync::RwLock::new(String::new())),
            http_client: reqwest::Client::new(),
        };

        let request = ProviderRequest {
            model: "kiro/claude-sonnet-4-6".to_string(),
            messages: vec![
                json!({"role": "user", "content": "Use the tool"}),
                json!({
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"location": "Tokyo"}}
                    ]
                }),
                json!({
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "toolu_1", "content": "{\"forecast\":\"Sunny\"}"}
                    ]
                }),
            ],
            system: None,
            temperature: None,
            max_tokens: Some(256),
            stream: false,
            tools: Some(vec![json!({
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}, "required": ["location"]}
            })]),
            tool_choice: None,
            extra: Value::Null,
        };

        let cw_request = provider.build_cw_request(&request).unwrap();
        assert_eq!(
            cw_request
                .conversation_state
                .current_message
                .user_input_message
                .content,
            "Continue"
        );
        assert_eq!(
            cw_request
                .conversation_state
                .current_message
                .user_input_message
                .user_input_message_context
                .as_ref()
                .unwrap()
                .tool_results[0]
                .content[0]
                .text
                .as_deref(),
            Some("{\"forecast\":\"Sunny\"}")
        );
    }

    #[test]
    fn extracts_conversation_id_from_prior_messages() {
        let messages = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "Hi"}], "conversation_id": "conv_123"}),
        ];

        assert_eq!(
            extract_conversation_id_from_messages(&messages).as_deref(),
            Some("conv_123")
        );
    }

    #[test]
    fn sanitize_history_does_not_append_user_after_final_assistant() {
        let messages = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "assistant", "content": "Hi"}),
            json!({"role": "user", "content": "Next"}),
        ];

        let (_, raw_history) =
            extract_last_user_and_history(&messages, "claude-sonnet-4-6", &[]).unwrap();
        let history = super::sanitize_history(raw_history, "claude-sonnet-4-6");

        assert!(history.last().unwrap().assistant_response_message.is_some());
    }

    #[test]
    fn sanitizes_history_to_start_with_user_and_alternate() {
        let provider = KiroProvider {
            client: super::client::KiroClient::new("us-east-1"),
            token: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            region: "us-east-1".to_string(),
            idc_region: None,
            auth_method: "builder_id".to_string(),
            profile_arn: None,
            healthy: std::sync::atomic::AtomicBool::new(true),
            client_id: None,
            client_secret: None,
            refresh_token: std::sync::Arc::new(tokio::sync::RwLock::new(String::new())),
            http_client: reqwest::Client::new(),
        };

        let request = ProviderRequest {
            model: "kiro/claude-sonnet-4-6".to_string(),
            messages: vec![
                json!({"role": "assistant", "content": "Prior assistant"}),
                json!({"role": "assistant", "content": "Another assistant"}),
                json!({"role": "user", "content": "Actual user"}),
            ],
            system: None,
            temperature: Some(0.2),
            max_tokens: Some(128),
            stream: false,
            tools: None,
            tool_choice: None,
            extra: json!({"thinking": {"type": "enabled", "budget_tokens": 1024}}),
        };

        let cw_request = provider.build_cw_request(&request).unwrap();
        assert_eq!(
            cw_request.conversation_state.history[0]
                .user_input_message
                .as_ref()
                .unwrap()
                .content,
            "Hello"
        );
        assert!(cw_request.conversation_state.history[1]
            .assistant_response_message
            .is_some());
        assert_eq!(cw_request.conversation_state.agent_task_type, "vibe");
        assert!(cw_request.conversation_state.agent_continuation_id.len() > 10);
        assert_eq!(
            cw_request.inference_config.as_ref().unwrap().max_tokens,
            Some(128)
        );
        assert_eq!(
            cw_request.inference_config.as_ref().unwrap().temperature,
            Some(0.2)
        );
        assert_eq!(
            cw_request.additional_model_request_fields.as_ref().unwrap()["thinking"]["type"],
            "enabled"
        );
    }

    #[test]
    fn relocates_tool_result_after_matching_tool_use() {
        let messages = vec![
            json!({"role": "user", "content": "Use a tool"}),
            json!({
                "role": "assistant",
                "content": [{"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Tokyo"}}]
            }),
            json!({"role": "assistant", "content": "Waiting"}),
            json!({
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "Sunny"}]
            }),
            json!({"role": "user", "content": "Continue"}),
        ];

        let (_, raw_history) =
            extract_last_user_and_history(&messages, "claude-sonnet-4-6", &[]).unwrap();
        let history = super::sanitize_history(raw_history, "claude-sonnet-4-6");

        let tool_use_index = history
            .iter()
            .position(|item| {
                item.assistant_response_message
                    .as_ref()
                    .and_then(|message| message.tool_uses.as_ref())
                    .is_some()
            })
            .unwrap();
        let result_context = history[tool_use_index + 1]
            .user_input_message
            .as_ref()
            .and_then(|message| message.user_input_message_context.as_ref())
            .unwrap();
        assert_eq!(result_context.tool_results[0].tool_use_id, "toolu_1");
        assert_eq!(
            result_context.tool_results[0].content[0].text.as_deref(),
            Some("Sunny")
        );
    }

    #[test]
    fn fills_missing_tool_result_with_error() {
        let messages = vec![
            json!({"role": "user", "content": "Use a tool"}),
            json!({
                "role": "assistant",
                "content": [{"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Tokyo"}}]
            }),
            json!({"role": "user", "content": "Continue"}),
        ];

        let (_, raw_history) =
            extract_last_user_and_history(&messages, "claude-sonnet-4-6", &[]).unwrap();
        let history = super::sanitize_history(raw_history, "claude-sonnet-4-6");

        let tool_use_index = history
            .iter()
            .position(|item| {
                item.assistant_response_message
                    .as_ref()
                    .and_then(|message| message.tool_uses.as_ref())
                    .is_some()
            })
            .unwrap();
        let result = &history[tool_use_index + 1]
            .user_input_message
            .as_ref()
            .and_then(|message| message.user_input_message_context.as_ref())
            .unwrap()
            .tool_results[0];
        assert_eq!(result.tool_use_id, "toolu_1");
        assert_eq!(result.status.as_deref(), Some("error"));
    }

    #[test]
    fn drops_orphan_tool_result_without_content() {
        let messages = vec![
            json!({
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "orphan", "content": "Unused"}]
            }),
            json!({"role": "assistant", "content": "Ignored"}),
            json!({"role": "user", "content": "Next"}),
        ];

        let (_, raw_history) =
            extract_last_user_and_history(&messages, "claude-sonnet-4-6", &[]).unwrap();
        let history = super::sanitize_history(raw_history, "claude-sonnet-4-6");

        assert!(history.iter().all(|item| {
            item.user_input_message
                .as_ref()
                .and_then(|message| message.user_input_message_context.as_ref())
                .map(|context| context.tool_results.is_empty())
                .unwrap_or(true)
        }));
    }

    #[test]
    fn guarded_finish_closes_open_text_block() {
        let mut state =
            super::AnthropicStreamState::new("kiro/test".to_string(), Some("conv_1".to_string()));
        state.ingest(&[super::eventstream::KiroEvent::Content(
            super::eventstream::ContentEvent {
                content: Some("hello".to_string()),
                model_id: None,
            },
        )]);

        state.finish_with_stop_reason("max_tokens");

        let output = state
            .pending
            .iter()
            .map(|event| String::from_utf8_lossy(event).to_string())
            .collect::<String>();

        assert!(output.contains("content_block_stop"));
        assert!(output.contains("message_stop"));
        assert!(output.contains("max_tokens"));
        assert!(state.finished);
    }

    #[test]
    fn guarded_finish_without_prior_events_starts_message_first() {
        let mut state =
            super::AnthropicStreamState::new("kiro/test".to_string(), Some("conv_1".to_string()));

        state.finish_with_stop_reason("max_tokens");

        let events = state
            .pending
            .iter()
            .map(|event| String::from_utf8_lossy(event).to_string())
            .collect::<Vec<_>>();

        assert!(events.first().unwrap().contains("message_start"));
        assert!(events.iter().any(|event| event.contains("message_stop")));
    }

    #[test]
    fn output_guard_counts_thinking_and_tool_input() {
        let mut state =
            super::AnthropicStreamState::new("kiro/test".to_string(), Some("conv_1".to_string()));

        state.ingest(&[
            super::eventstream::KiroEvent::Reasoning(super::eventstream::ReasoningEvent {
                text: Some("thinking".to_string()),
                signature: None,
                redacted_content: None,
            }),
            super::eventstream::KiroEvent::ToolUse(super::eventstream::ToolUseEvent {
                name: Some("tool".to_string()),
                tool_use_id: Some("toolu_1".to_string()),
                input: Some("{\"x\":1}".to_string()),
                stop: Some(false),
            }),
        ]);

        assert_eq!(state.output_chars, "thinking".len() + "{\"x\":1}".len());
    }
}

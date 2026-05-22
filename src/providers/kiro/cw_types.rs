use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct CWGenerateRequest {
    #[serde(rename = "conversationState")]
    pub conversation_state: CWConversationState,
    #[serde(rename = "profileArn", skip_serializing_if = "Option::is_none")]
    pub profile_arn: Option<String>,
    #[serde(rename = "inferenceConfig", skip_serializing_if = "Option::is_none")]
    pub inference_config: Option<CWInferenceConfig>,
    #[serde(
        rename = "additionalModelRequestFields",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_model_request_fields: Option<serde_json::Value>,
}

#[derive(Serialize, Clone)]
pub struct CWConversationState {
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
    #[serde(rename = "agentContinuationId")]
    pub agent_continuation_id: String,
    #[serde(rename = "agentTaskType")]
    pub agent_task_type: String,
    #[serde(rename = "chatTriggerType")]
    pub chat_trigger_type: String,
    #[serde(rename = "currentMessage")]
    pub current_message: CWCurrentMessage,
    pub history: Vec<CWHistoryItem>,
}

#[derive(Serialize, Clone)]
pub struct CWCurrentMessage {
    #[serde(rename = "userInputMessage")]
    pub user_input_message: CWUserInputMessage,
}

#[derive(Serialize, Clone)]
pub struct CWUserInputMessage {
    pub content: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub origin: String,
    #[serde(
        rename = "userInputMessageContext",
        skip_serializing_if = "Option::is_none"
    )]
    pub user_input_message_context: Option<CWUserInputMessageContext>,
}

#[derive(Serialize, Clone)]
pub struct CWHistoryItem {
    #[serde(rename = "userInputMessage", skip_serializing_if = "Option::is_none")]
    pub user_input_message: Option<CWHistoryUserMessage>,
    #[serde(
        rename = "assistantResponseMessage",
        skip_serializing_if = "Option::is_none"
    )]
    pub assistant_response_message: Option<CWAssistantMessage>,
}

#[derive(Serialize, Clone)]
pub struct CWHistoryUserMessage {
    pub content: String,
    #[serde(rename = "modelId", skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(
        rename = "userInputMessageContext",
        skip_serializing_if = "Option::is_none"
    )]
    pub user_input_message_context: Option<CWUserInputMessageContext>,
}

#[derive(Serialize, Clone)]
pub struct CWAssistantMessage {
    pub content: String,
    #[serde(rename = "toolUses", skip_serializing_if = "Option::is_none")]
    pub tool_uses: Option<Vec<CWHistoryToolUse>>,
}

#[derive(Serialize, Clone)]
pub struct CWUserInputMessageContext {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<CWTool>,
    #[serde(rename = "toolResults", skip_serializing_if = "Vec::is_empty", default)]
    pub tool_results: Vec<CWToolResult>,
}

#[derive(Serialize, Clone)]
pub struct CWTool {
    #[serde(rename = "toolSpecification")]
    pub tool_specification: CWToolSpecification,
}

#[derive(Serialize, Clone)]
pub struct CWToolSpecification {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: CWToolInputSchema,
}

#[derive(Serialize, Clone)]
pub struct CWToolInputSchema {
    pub json: serde_json::Value,
}

#[derive(Serialize, Clone)]
pub struct CWInferenceConfig {
    #[serde(rename = "maxTokens", skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

#[derive(Serialize, Clone)]
pub struct CWToolResult {
    #[serde(rename = "toolUseId")]
    pub tool_use_id: String,
    pub content: Vec<CWToolResultContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct CWToolResultContentBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<serde_json::Value>,
}

#[derive(Serialize, Clone)]
pub struct CWHistoryToolUse {
    #[serde(rename = "toolUseId")]
    pub tool_use_id: String,
    pub name: String,
    pub input: serde_json::Value,
}

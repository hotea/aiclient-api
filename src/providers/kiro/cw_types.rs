use serde::Serialize;

#[derive(Serialize)]
pub struct CWGenerateRequest {
    #[serde(rename = "conversationState")]
    pub conversation_state: CWConversationState,
    #[serde(rename = "profileArn", skip_serializing_if = "Option::is_none")]
    pub profile_arn: Option<String>,
}

#[derive(Serialize)]
pub struct CWConversationState {
    #[serde(rename = "chatTriggerType")]
    pub chat_trigger_type: String,
    #[serde(rename = "currentMessage")]
    pub current_message: CWCurrentMessage,
    pub history: Vec<CWHistoryItem>,
}

#[derive(Serialize)]
pub struct CWCurrentMessage {
    #[serde(rename = "userInputMessage")]
    pub user_input_message: CWUserInputMessage,
}

#[derive(Serialize)]
pub struct CWUserInputMessage {
    pub content: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub origin: String,
}

#[derive(Serialize)]
pub struct CWHistoryItem {
    #[serde(rename = "userInputMessage", skip_serializing_if = "Option::is_none")]
    pub user_input_message: Option<CWHistoryUserMessage>,
    #[serde(rename = "assistantResponseMessage", skip_serializing_if = "Option::is_none")]
    pub assistant_response_message: Option<CWAssistantMessage>,
}

#[derive(Serialize)]
pub struct CWHistoryUserMessage {
    pub content: String,
}

#[derive(Serialize)]
pub struct CWAssistantMessage {
    pub content: String,
}

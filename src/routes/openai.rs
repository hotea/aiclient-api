use axum::extract::State;
use axum::Json;
use serde_json::Value;
use crate::server::state::AppState;
use crate::util::error::AppError;

pub async fn chat_completions(
    State(_state): State<AppState>,
    Json(_body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    Err(AppError::Unavailable("Not yet implemented".into()))
}

pub async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let providers = state.providers.read().await;
    let mut models = Vec::new();
    for provider in providers.values() {
        if let Ok(provider_models) = provider.list_models().await {
            for m in provider_models {
                models.push(serde_json::json!({
                    "id": m.id,
                    "object": "model",
                    "created": 0,
                    "owned_by": m.provider,
                }));
            }
        }
    }
    Ok(Json(serde_json::json!({
        "object": "list",
        "data": models,
    })))
}

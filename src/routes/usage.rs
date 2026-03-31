use axum::{extract::State, http::StatusCode, Json};
use serde_json::json;

use crate::server::state::AppState;
use crate::usage::UsageStats;
use crate::util::error::AppError;

/// GET /v1/usage - Get current usage statistics
pub async fn get_usage(State(state): State<AppState>) -> Result<Json<UsageStats>, AppError> {
    let stats = state.usage_tracker.get_stats().await;
    Ok(Json(stats))
}

/// DELETE /v1/usage - Reset usage statistics
pub async fn reset_usage(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    state.usage_tracker.reset().await;
    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "Usage statistics reset successfully"
        })),
    ))
}

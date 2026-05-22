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

/// GET /v1/usage/summary - Compact usage view for integrations like cc-switch
pub async fn get_usage_summary(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let stats = state.usage_tracker.get_stats().await;

    let providers = stats
        .providers
        .iter()
        .map(|(name, usage)| {
            json!({
                "provider": name,
                "request_count": usage.request_count,
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "total_tokens": usage.total_tokens,
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "is_valid": true,
        "remaining": stats.total.total_tokens,
        "unit": "tokens",
        "request_count": stats.total.request_count,
        "input_tokens": stats.total.input_tokens,
        "output_tokens": stats.total.output_tokens,
        "total_tokens": stats.total.total_tokens,
        "providers": providers,
    })))
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

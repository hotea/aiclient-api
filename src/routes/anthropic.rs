use axum::extract::State;
use axum::Json;
use serde_json::Value;
use crate::server::state::AppState;
use crate::util::error::AppError;

pub async fn messages(
    State(_state): State<AppState>,
    Json(_body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    Err(AppError::Unavailable("Not yet implemented".into()))
}

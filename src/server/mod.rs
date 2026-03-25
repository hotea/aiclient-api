pub mod middleware;
pub mod state;

use axum::Router;
use state::AppState;
use tower_http::catch_panic::CatchPanicLayer;

pub fn build_router(state: AppState) -> Router {
    use axum::routing::{get, post};
    Router::new()
        .route("/healthz", get(crate::routes::health::healthz))
        .route("/v1/chat/completions", post(crate::routes::openai::chat_completions))
        .route("/v1/models", get(crate::routes::openai::list_models))
        .route("/v1/messages", post(crate::routes::anthropic::messages))
        .layer(CatchPanicLayer::new())
        .layer(middleware::cors_layer())
        .with_state(state)
}

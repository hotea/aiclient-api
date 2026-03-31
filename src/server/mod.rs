pub mod middleware;
pub mod state;

use axum::middleware as axum_mw;
use axum::Router;
use state::AppState;
use tower_http::catch_panic::CatchPanicLayer;

pub fn build_router(state: AppState) -> Router {
    use axum::routing::{delete, get, post};

    // V1 API routes with auth & rate limiting middlewares
    let v1_routes = Router::new()
        .route("/chat/completions", post(crate::routes::openai::chat_completions))
        .route("/models", get(crate::routes::openai::list_models))
        .route("/messages", post(crate::routes::anthropic::messages))
        .route("/usage", get(crate::routes::usage::get_usage))
        .route("/usage", delete(crate::routes::usage::reset_usage))
        .layer(axum_mw::from_fn_with_state(state.clone(), middleware::auth))
        .layer(axum_mw::from_fn_with_state(state.clone(), middleware::rate_limit));

    Router::new()
        .route("/healthz", get(crate::routes::health::healthz))
        .nest("/v1", v1_routes)
        .layer(CatchPanicLayer::new())
        .layer(middleware::cors_layer())
        .with_state(state)
}

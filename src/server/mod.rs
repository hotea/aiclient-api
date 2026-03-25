pub mod middleware;
pub mod state;

use axum::Router;
use state::AppState;
use tower_http::catch_panic::CatchPanicLayer;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .layer(CatchPanicLayer::new())
        .layer(middleware::cors_layer())
        .with_state(state)
}

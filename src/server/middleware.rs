use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

pub async fn request_id(mut req: Request, next: Next) -> Response {
    let id = Uuid::new_v4().to_string();
    req.headers_mut().insert("x-request-id", id.parse().unwrap());
    next.run(req).await
}

pub fn cors_layer() -> CorsLayer {
    CorsLayer::very_permissive()
}

use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Instant;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use crate::server::state::AppState;
use crate::util::error::AppError;

pub type RateLimitMap = Arc<RwLock<HashMap<IpAddr, Instant>>>;

pub fn new_rate_limit_map() -> RateLimitMap {
    Arc::new(RwLock::new(HashMap::new()))
}

pub async fn request_id(mut req: Request, next: Next) -> Response {
    let id = Uuid::new_v4().to_string();
    req.headers_mut().insert("x-request-id", id.parse().unwrap());
    next.run(req).await
}

pub fn cors_layer() -> CorsLayer {
    CorsLayer::very_permissive()
}

/// Bearer token auth middleware.
/// If `config.api_key` is non-empty, validates `Authorization: Bearer <key>`.
/// If `api_key` is empty, all requests are allowed through.
pub async fn auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let config = state.config.load();
    let api_key = &config.api_key;

    if api_key.is_empty() {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..];
            if token == api_key {
                Ok(next.run(req).await)
            } else {
                Err(AppError::Unauthorized("Invalid API key".to_string()))
            }
        }
        Some(_) => Err(AppError::Unauthorized(
            "Invalid authorization format, expected Bearer token".to_string(),
        )),
        None => Err(AppError::Unauthorized(
            "Missing Authorization header".to_string(),
        )),
    }
}

/// Per-IP rate limiting middleware.
/// If `config.server.rate_limit_seconds > 0`, rejects requests that come
/// faster than the configured interval with 429 Too Many Requests.
pub async fn rate_limit(
    State(state): State<AppState>,
    State(limiter): State<RateLimitMap>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let config = state.config.load();
    let limit_secs = config.server.rate_limit_seconds;

    if limit_secs == 0 {
        return Ok(next.run(req).await);
    }

    // Extract client IP from ConnectInfo or fall back to a default
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    let now = Instant::now();
    let interval = std::time::Duration::from_secs(limit_secs);

    {
        let map = limiter.read().await;
        if let Some(last) = map.get(&ip) {
            if now.duration_since(*last) < interval {
                return Err(AppError::RateLimited);
            }
        }
    }

    {
        let mut map = limiter.write().await;
        map.insert(ip, now);
    }

    Ok(next.run(req).await)
}

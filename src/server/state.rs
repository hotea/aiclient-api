use arc_swap::ArcSwap;
use axum::extract::FromRef;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Instant;

use crate::config::types::Config;
use crate::providers::Provider;
use crate::server::middleware::RateLimitMap;
use crate::usage::UsageTracker;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ArcSwap<Config>>,
    pub providers: Arc<RwLock<HashMap<String, Arc<dyn Provider>>>>,
    pub start_time: Instant,
    pub rate_limiter: RateLimitMap,
    pub usage_tracker: UsageTracker,
    pub routing_counter: Arc<AtomicUsize>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(ArcSwap::from_pointee(config)),
            providers: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
            rate_limiter: super::middleware::new_rate_limit_map(),
            usage_tracker: UsageTracker::new(),
            routing_counter: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl FromRef<AppState> for RateLimitMap {
    fn from_ref(state: &AppState) -> Self {
        state.rate_limiter.clone()
    }
}

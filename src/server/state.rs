use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Instant;

use crate::config::types::Config;
use crate::providers::Provider;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ArcSwap<Config>>,
    pub providers: Arc<RwLock<HashMap<String, Arc<dyn Provider>>>>,
    pub start_time: Instant,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(ArcSwap::from_pointee(config)),
            providers: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
        }
    }
}

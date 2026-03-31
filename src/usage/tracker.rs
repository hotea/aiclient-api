use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Usage statistics for a single provider
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderUsage {
    /// Total number of requests
    pub request_count: u64,
    /// Total input/prompt tokens consumed
    pub input_tokens: u64,
    /// Total output/completion tokens consumed
    pub output_tokens: u64,
    /// Total tokens consumed
    pub total_tokens: u64,
    /// Per-model usage breakdown
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub models: HashMap<String, ModelUsage>,
}

/// Usage statistics for a specific model
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelUsage {
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Aggregated usage statistics across all providers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageStats {
    /// Per-provider usage
    pub providers: HashMap<String, ProviderUsage>,
    /// Total aggregated usage
    pub total: ProviderUsage,
}

/// Thread-safe usage tracker
pub struct UsageTracker {
    stats: Arc<RwLock<UsageStats>>,
}

impl UsageTracker {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RwLock::new(UsageStats::default())),
        }
    }

    /// Record usage for a request
    pub async fn record(
        &self,
        provider: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) {
        let mut stats = self.stats.write().await;
        let total_tokens = input_tokens + output_tokens;

        // Update provider stats
        let provider_stats = stats.providers.entry(provider.to_string()).or_default();
        provider_stats.request_count += 1;
        provider_stats.input_tokens += input_tokens;
        provider_stats.output_tokens += output_tokens;
        provider_stats.total_tokens += total_tokens;

        // Update per-model stats
        let model_stats = provider_stats.models.entry(model.to_string()).or_default();
        model_stats.request_count += 1;
        model_stats.input_tokens += input_tokens;
        model_stats.output_tokens += output_tokens;
        model_stats.total_tokens += total_tokens;

        // Update total stats
        stats.total.request_count += 1;
        stats.total.input_tokens += input_tokens;
        stats.total.output_tokens += output_tokens;
        stats.total.total_tokens += total_tokens;
    }

    /// Get current usage statistics
    pub async fn get_stats(&self) -> UsageStats {
        self.stats.read().await.clone()
    }

    /// Reset all statistics
    pub async fn reset(&self) {
        let mut stats = self.stats.write().await;
        *stats = UsageStats::default();
    }

    /// Get usage for a specific provider
    pub async fn get_provider_usage(&self, provider: &str) -> Option<ProviderUsage> {
        let stats = self.stats.read().await;
        stats.providers.get(provider).cloned()
    }
}

impl Default for UsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for UsageTracker {
    fn clone(&self) -> Self {
        Self {
            stats: Arc::clone(&self.stats),
        }
    }
}

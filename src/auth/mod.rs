pub mod copilot;
pub mod token_store;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TokenData {
    #[serde(rename = "copilot")]
    Copilot {
        github_token: String,
        copilot_token: Option<String>,
        expires_at: Option<i64>,
    },
    #[serde(rename = "kiro")]
    Kiro {
        access_token: String,
        refresh_token: String,
        client_id: Option<String>,
        client_secret: Option<String>,
        auth_method: String,
        region: String,
        idc_region: Option<String>,
        profile_arn: Option<String>,
        expires_at: i64,
    },
}

#[async_trait]
pub trait TokenStore: Send + Sync {
    async fn load(&self, provider: &str) -> Result<TokenData>;
    async fn save(&self, provider: &str, data: &TokenData) -> Result<()>;
    async fn delete(&self, provider: &str) -> Result<()>;
}

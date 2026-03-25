use super::{TokenData, TokenStore};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;

pub struct XdgTokenStore {
    base_dir: PathBuf,
}

impl XdgTokenStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn default() -> Self {
        Self::new(crate::util::xdg::config_dir())
    }

    fn token_path(&self, provider: &str) -> PathBuf {
        self.base_dir.join(provider).join("token.json")
    }
}

#[async_trait]
impl TokenStore for XdgTokenStore {
    async fn load(&self, provider: &str) -> Result<TokenData> {
        let path = self.token_path(provider);
        let content = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("No token found for provider: {}", provider))?;
        let data: TokenData = serde_json::from_str(&content)
            .with_context(|| format!("Invalid token file for: {}", provider))?;
        Ok(data)
    }

    async fn save(&self, provider: &str, data: &TokenData) -> Result<()> {
        let path = self.token_path(provider);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(data)?;
        tokio::fs::write(&path, &json).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&path, perms).await?;
        }

        Ok(())
    }

    async fn delete(&self, provider: &str) -> Result<()> {
        let path = self.token_path(provider);
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
        }
        let dir = self.base_dir.join(provider);
        if dir.exists() {
            let _ = tokio::fs::remove_dir(&dir).await;
        }
        Ok(())
    }

    fn is_expired(&self, data: &TokenData) -> bool {
        let now = chrono::Utc::now().timestamp();
        match data {
            TokenData::Copilot { expires_at, .. } => {
                expires_at.is_some_and(|exp| now >= exp)
            }
            TokenData::Kiro { expires_at, .. } => now >= *expires_at,
        }
    }
}

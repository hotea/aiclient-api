use anyhow::{bail, Context, Result};
use reqwest::header::HeaderMap;

use crate::config::types::AccountType;

pub struct CopilotClient {
    client: reqwest::Client,
    base_url: String,
}

impl CopilotClient {
    pub fn new(account_type: &AccountType) -> Self {
        let base_url = match account_type {
            AccountType::Individual => "https://api.githubcopilot.com",
            AccountType::Business => "https://api.business.githubcopilot.com",
            AccountType::Enterprise => "https://api.enterprise.githubcopilot.com",
        }
        .to_string();

        Self {
            client: reqwest::Client::new(),
            base_url,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    pub async fn chat_completions(
        &self,
        headers: HeaderMap,
        body: serde_json::Value,
        _stream: bool,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Failed to send chat completions request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            bail!("Chat completions request failed: HTTP {} - {}", status, err_body);
        }

        Ok(resp)
    }

    pub async fn messages(
        &self,
        headers: HeaderMap,
        body: serde_json::Value,
        _stream: bool,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Failed to send messages request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            bail!("Messages request failed: HTTP {} - {}", status, err_body);
        }

        Ok(resp)
    }
}

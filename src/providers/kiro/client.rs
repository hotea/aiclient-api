use anyhow::{Context, Result};
use reqwest::header::HeaderMap;
use uuid::Uuid;

use super::cw_types::CWGenerateRequest;

pub struct KiroClient {
    client: reqwest::Client,
    region: String,
}

impl KiroClient {
    pub fn new(region: &str) -> Self {
        let client = reqwest::Client::builder()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            region: region.to_string(),
        }
    }

    pub fn base_url(&self) -> String {
        format!("https://q.{}.amazonaws.com", self.region)
    }

    pub async fn generate_assistant_response(
        &self,
        access_token: &str,
        request: CWGenerateRequest,
        _profile_arn: Option<&str>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/generateAssistantResponse", self.base_url());
        let mut headers = HeaderMap::new();

        headers.insert(
            "Authorization",
            format!("Bearer {}", access_token)
                .parse()
                .context("Invalid authorization header value")?,
        );
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert("Accept", "application/json".parse().unwrap());
        headers.insert(
            "amz-sdk-invocation-id",
            Uuid::new_v4().to_string().parse().unwrap(),
        );
        headers.insert(
            "amz-sdk-request",
            "attempt=1; max=3".parse().unwrap(),
        );
        headers.insert("x-amzn-codewhisperer-optout", "true".parse().unwrap());
        headers.insert("x-amzn-kiro-agent-mode", "vibe".parse().unwrap());
        headers.insert(
            "x-amz-user-agent",
            "aws-sdk-js/1.0.34 KiroIDE-0.11.63".parse().unwrap(),
        );
        headers.insert("Connection", "close".parse().unwrap());

        self.client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .context("Failed to call CodeWhisperer API")
    }
}

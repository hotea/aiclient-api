use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

use super::{TokenData, TokenStore};

const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const COPILOT_TOKEN_URL: &str =
    "https://api.github.com/copilot_internal/v2/token";

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CopilotTokenResponse {
    pub token: String,
    pub expires_at: i64,
    pub refresh_in: u64,
}

pub async fn authenticate(store: &dyn TokenStore) -> Result<()> {
    let client = reqwest::Client::new();

    // Step 1: Request device code
    let body = serde_json::json!({
        "client_id": CLIENT_ID,
        "scope": "read:user"
    });

    let device_resp: DeviceCodeResponse = client
        .post(DEVICE_CODE_URL)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .json(&body)
        .send()
        .await
        .context("Failed to request device code")?
        .json()
        .await
        .context("Failed to parse device code response")?;

    // Step 2: Show user code and attempt to open browser
    println!(
        "\nPlease visit: {}",
        device_resp.verification_uri
    );
    println!("And enter code: {}", device_resp.user_code);
    println!("\nAttempting to open browser...");

    let _ = open::that(&device_resp.verification_uri);

    // Step 3: Poll for access token
    let mut poll_interval = Duration::from_secs(device_resp.interval + 1);
    let poll_body = serde_json::json!({
        "client_id": CLIENT_ID,
        "device_code": device_resp.device_code,
        "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
    });

    println!("\nWaiting for authorization...");

    loop {
        sleep(poll_interval).await;

        let token_resp: AccessTokenResponse = client
            .post(ACCESS_TOKEN_URL)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .json(&poll_body)
            .send()
            .await
            .context("Failed to poll for access token")?
            .json()
            .await
            .context("Failed to parse access token response")?;

        if let Some(access_token) = token_resp.access_token {
            // Save token to store
            let token_data = TokenData::Copilot {
                github_token: access_token,
                copilot_token: None,
                expires_at: None,
            };
            store
                .save("copilot", &token_data)
                .await
                .context("Failed to save token")?;
            return Ok(());
        }

        match token_resp.error.as_deref() {
            Some("authorization_pending") => {
                // Continue polling
            }
            Some("slow_down") => {
                poll_interval += Duration::from_secs(5);
            }
            Some("expired_token") => {
                bail!("Device code expired. Please try again.");
            }
            Some("access_denied") => {
                bail!("Authorization denied by user.");
            }
            Some(err) => {
                bail!("Unexpected error: {}", err);
            }
            None => {
                bail!("Unexpected empty response from OAuth server");
            }
        }
    }
}

pub async fn fetch_copilot_token(
    client: &reqwest::Client,
    github_token: &str,
) -> Result<CopilotTokenResponse> {
    let resp = client
        .get(COPILOT_TOKEN_URL)
        .header("authorization", format!("token {}", github_token))
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("editor-version", "vscode/1.110.1")
        .header("editor-plugin-version", "copilot-chat/0.38.2")
        .header("user-agent", "GitHubCopilotChat/0.38.2")
        .header("x-github-api-version", "2025-10-01")
        .header("x-vscode-user-agent-library-version", "electron-fetch")
        .send()
        .await
        .context("Failed to fetch Copilot token")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to fetch Copilot token: HTTP {} - {}", status, body);
    }

    let token_resp: CopilotTokenResponse = resp
        .json()
        .await
        .context("Failed to parse Copilot token response")?;

    Ok(token_resp)
}

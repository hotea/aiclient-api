use anyhow::{bail, Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::sleep;

use super::{TokenData, TokenStore};

// ---- Response types ----

#[derive(Debug, Deserialize)]
struct ClientRegisterResponse {
    #[serde(rename = "clientId")]
    client_id: String,
    #[serde(rename = "clientSecret")]
    client_secret: String,
}

#[derive(Debug, Deserialize)]
struct DeviceAuthResponse {
    #[serde(rename = "deviceCode")]
    device_code: String,
    #[serde(rename = "userCode")]
    user_code: String,
    #[serde(rename = "verificationUri")]
    verification_uri: String,
    #[serde(rename = "verificationUriComplete")]
    verification_uri_complete: Option<String>,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresIn")]
    expires_in: Option<u64>,
    error: Option<String>,
    message: Option<String>,
}

/// Public type for token refresh callers
#[derive(Debug, Clone)]
pub struct KiroTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub profile_arn: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SocialTokenResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresIn")]
    expires_in: u64,
    #[serde(rename = "profileArn")]
    profile_arn: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresIn")]
    expires_in: u64,
    #[serde(rename = "profileArn")]
    profile_arn: Option<String>,
}

// ---- Builder ID Device Flow ----

pub async fn authenticate_builder_id(store: &dyn TokenStore, region: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let oidc_base = format!("https://oidc.{}.amazonaws.com", region);

    // Step 1: Register client
    let reg_body = serde_json::json!({
        "clientName": "aiclient-api",
        "clientType": "public",
        "scopes": [
            "codewhisperer:completions",
            "codewhisperer:analysis",
            "codewhisperer:conversations"
        ]
    });

    let reg_resp: ClientRegisterResponse = client
        .post(format!("{}/client/register", oidc_base))
        .header("Content-Type", "application/json")
        .json(&reg_body)
        .send()
        .await
        .context("Failed to register OIDC client")?
        .json()
        .await
        .context("Failed to parse client registration response")?;

    // Step 2: Request device authorization
    let device_body = serde_json::json!({
        "clientId": reg_resp.client_id,
        "clientSecret": reg_resp.client_secret,
        "startUrl": "https://view.awsapps.com/start"
    });

    let device_resp: DeviceAuthResponse = client
        .post(format!("{}/device_authorization", oidc_base))
        .header("Content-Type", "application/json")
        .json(&device_body)
        .send()
        .await
        .context("Failed to request device authorization")?
        .json()
        .await
        .context("Failed to parse device authorization response")?;

    // Step 3: Show user code and open browser
    println!("\nPlease enter this code: {}", device_resp.user_code);
    let uri = device_resp
        .verification_uri_complete
        .as_deref()
        .unwrap_or(&device_resp.verification_uri);
    println!("Opening: {}", uri);
    let _ = open::that(uri);
    println!("\nWaiting for authorization...");

    // Step 4: Poll for token
    let poll_interval = device_resp.interval.unwrap_or(5) + 1;
    let mut current_interval = Duration::from_secs(poll_interval);

    let poll_body = serde_json::json!({
        "clientId": reg_resp.client_id,
        "clientSecret": reg_resp.client_secret,
        "deviceCode": device_resp.device_code,
        "grantType": "urn:ietf:params:oauth:grant-type:device_code"
    });

    loop {
        sleep(current_interval).await;

        let token_resp: DeviceTokenResponse = client
            .post(format!("{}/token", oidc_base))
            .header("Content-Type", "application/json")
            .json(&poll_body)
            .send()
            .await
            .context("Failed to poll for token")?
            .json()
            .await
            .context("Failed to parse token response")?;

        if let (Some(access_token), Some(refresh_token), Some(expires_in)) = (
            token_resp.access_token,
            token_resp.refresh_token,
            token_resp.expires_in,
        ) {
            let expires_at = chrono::Utc::now().timestamp() + expires_in as i64;
            let token_data = TokenData::Kiro {
                access_token,
                refresh_token,
                client_id: Some(reg_resp.client_id),
                client_secret: Some(reg_resp.client_secret),
                auth_method: "builder_id".to_string(),
                region: region.to_string(),
                idc_region: None,
                profile_arn: None,
                expires_at,
            };
            store
                .save("kiro", &token_data)
                .await
                .context("Failed to save Kiro token")?;
            return Ok(());
        }

        match token_resp.error.as_deref() {
            Some("authorization_pending") | Some("AuthorizationPendingException") => {
                // Continue polling
            }
            Some("slow_down") | Some("SlowDownException") => {
                current_interval += Duration::from_secs(5);
            }
            Some("expired_token") | Some("ExpiredTokenException") => {
                bail!("Device code expired. Please try again.");
            }
            Some("access_denied") | Some("AccessDeniedException") => {
                bail!("Authorization denied.");
            }
            Some(err) => {
                let msg = token_resp.message.unwrap_or_default();
                bail!("Unexpected error: {} - {}", err, msg);
            }
            None => {
                bail!("Unexpected empty response from token endpoint");
            }
        }
    }
}

// ---- Social Auth (Google / GitHub) ----

pub async fn authenticate_social(store: &dyn TokenStore, region: &str, provider: &str) -> Result<()> {
    // Step 1: Generate PKCE
    let code_verifier_bytes: [u8; 32] = rand::rng().random();
    let code_verifier = URL_SAFE_NO_PAD.encode(code_verifier_bytes);

    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let hash = hasher.finalize();
    let code_challenge = URL_SAFE_NO_PAD.encode(hash);

    // Generate random state
    let state_bytes: [u8; 16] = rand::rng().random();
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    // Step 2: Start local HTTP server
    let (port, listener) = bind_local_port(&[19876, 19877, 19878, 19879, 19880]).await?;

    let redirect_uri = format!("http://127.0.0.1:{}/oauth/callback", port);

    // Step 3: Build auth URL and open browser
    let auth_url = format!(
        "https://prod.{region}.auth.desktop.kiro.dev/login?\
        idp={provider}&\
        redirect_uri={redirect_uri}&\
        code_challenge={code_challenge}&\
        code_challenge_method=S256&\
        state={state}&\
        prompt=select_account"
    );

    println!("\nOpening browser for {} authentication...", provider);
    println!("URL: {}", auth_url);
    let _ = open::that(&auth_url);
    println!("\nWaiting for callback...");

    // Step 4: Accept one connection and extract the auth code
    let code = receive_oauth_callback(listener, &state).await?;

    // Step 5: Exchange code for tokens
    let client = reqwest::Client::new();
    let token_body = serde_json::json!({
        "code": code,
        "codeVerifier": code_verifier,
        "redirectUri": redirect_uri
    });

    let resp = client
        .post(format!(
            "https://prod.{}.auth.desktop.kiro.dev/oauth/token",
            region
        ))
        .header("Content-Type", "application/json")
        .json(&token_body)
        .send()
        .await
        .context("Failed to exchange authorization code for tokens")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Token exchange failed: HTTP {} - {}", status, body);
    }

    let token_resp: SocialTokenResponse = resp
        .json()
        .await
        .context("Failed to parse token exchange response")?;

    let expires_at = chrono::Utc::now().timestamp() + token_resp.expires_in as i64;

    let token_data = TokenData::Kiro {
        access_token: token_resp.access_token,
        refresh_token: token_resp.refresh_token,
        client_id: None,
        client_secret: None,
        auth_method: provider.to_lowercase(),
        region: region.to_string(),
        idc_region: None,
        profile_arn: token_resp.profile_arn,
        expires_at,
    };

    store
        .save("kiro", &token_data)
        .await
        .context("Failed to save Kiro token")?;

    Ok(())
}

async fn bind_local_port(ports: &[u16]) -> Result<(u16, tokio::net::TcpListener)> {
    for &port in ports {
        if let Ok(listener) = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await {
            return Ok((port, listener));
        }
    }
    bail!("Could not bind to any of the local ports: {:?}", ports)
}

async fn receive_oauth_callback(
    listener: tokio::net::TcpListener,
    expected_state: &str,
) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut stream, _) = listener
        .accept()
        .await
        .context("Failed to accept OAuth callback connection")?;

    let mut buf = vec![0u8; 16 * 1024];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Extract query string from GET request line
    let first_line = request.lines().next().unwrap_or("");
    // e.g. "GET /oauth/callback?code=xxx&state=yyy HTTP/1.1"
    let query_string = first_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .splitn(2, '?')
        .nth(1)
        .unwrap_or("");

    let mut code = None;
    let mut state = None;
    let mut error = None;

    for part in query_string.split('&') {
        let mut kv = part.splitn(2, '=');
        let key = kv.next().unwrap_or("");
        let val = kv.next().unwrap_or("");
        match key {
            "code" => code = Some(url_decode(val)),
            "state" => state = Some(url_decode(val)),
            "error" => error = Some(url_decode(val)),
            _ => {}
        }
    }

    // Send HTTP response back
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n\
        <html><body><h1>Authentication successful!</h1><p>You can close this tab.</p></body></html>";
    let _ = stream.write_all(response.as_bytes()).await;

    if let Some(err) = error {
        bail!("OAuth callback returned error: {}", err);
    }

    let s = state.context("Missing state parameter in OAuth callback")?;
    if s != expected_state {
        bail!("OAuth state mismatch");
    }

    code.context("No authorization code in callback")
}

fn url_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

// ---- Token Refresh Functions ----

pub async fn refresh_builder_id(
    client: &reqwest::Client,
    region: &str,
    refresh_token: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<KiroTokenResponse> {
    let oidc_base = format!("https://oidc.{}.amazonaws.com", region);
    let body = serde_json::json!({
        "grantType": "refresh_token",
        "refreshToken": refresh_token,
        "clientId": client_id,
        "clientSecret": client_secret
    });

    let resp = client
        .post(format!("{}/token", oidc_base))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Failed to refresh Builder ID token")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        bail!("Token refresh failed: HTTP {} - {}", status, body_text);
    }

    let r: RefreshTokenResponse = resp
        .json()
        .await
        .context("Failed to parse refresh token response")?;

    Ok(KiroTokenResponse {
        access_token: r.access_token,
        refresh_token: r.refresh_token,
        expires_in: r.expires_in,
        profile_arn: r.profile_arn,
    })
}

pub async fn refresh_social(
    client: &reqwest::Client,
    region: &str,
    refresh_token: &str,
) -> Result<KiroTokenResponse> {
    let body = serde_json::json!({
        "refreshToken": refresh_token
    });

    let resp = client
        .post(format!(
            "https://prod.{}.auth.desktop.kiro.dev/refreshToken",
            region
        ))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Failed to refresh social token")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        bail!("Social token refresh failed: HTTP {} - {}", status, body_text);
    }

    let r: RefreshTokenResponse = resp
        .json()
        .await
        .context("Failed to parse refresh token response")?;

    Ok(KiroTokenResponse {
        access_token: r.access_token,
        refresh_token: r.refresh_token,
        expires_in: r.expires_in,
        profile_arn: r.profile_arn,
    })
}

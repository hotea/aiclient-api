use anyhow::{Context, Result};
use reqwest::header::HeaderMap;
use sha2::{Digest, Sha256};
use std::env;
use uuid::Uuid;

use super::cw_types::CWGenerateRequest;

pub struct KiroClient {
    client: reqwest::Client,
    region: String,
    machine_id: String,
}

impl KiroClient {
    pub fn new(region: &str) -> Self {
        let client = reqwest::Client::builder()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        
        // Generate machine ID from hostname or fallback
        let machine_id = Self::generate_machine_id();
        
        Self {
            client,
            region: region.to_string(),
            machine_id,
        }
    }

    fn generate_machine_id() -> String {
        // Try to get a unique identifier (hostname, or fallback to default)
        let unique_key = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "KIRO_DEFAULT_MACHINE".to_string());
        
        // Hash it to create a machine ID like the Node.js version does
        let mut hasher = Sha256::new();
        hasher.update(unique_key.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    fn get_os_info() -> String {
        let os_name = env::consts::OS;
        let os_version = match os_name {
            "macos" => {
                // On macOS, try to get the version
                std::process::Command::new("sw_vers")
                    .arg("-productVersion")
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            }
            "windows" => {
                // On Windows, try to get version
                env::var("OS").unwrap_or_else(|_| "unknown".to_string())
            }
            _ => "unknown".to_string(),
        };
        
        match os_name {
            "macos" => format!("macos#{}", os_version),
            "windows" => format!("windows#{}", os_version),
            _ => format!("{}#{}", os_name, os_version),
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

        // Version info
        let kiro_version = "0.11.63";
        let sdk_version = "1.0.34";
        let os_info = Self::get_os_info();
        let rust_version = env!("CARGO_PKG_RUST_VERSION", "1.70.0");

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
        
        // x-amz-user-agent with machine ID
        headers.insert(
            "x-amz-user-agent",
            format!("aws-sdk-js/{} KiroIDE-{}-{}", sdk_version, kiro_version, &self.machine_id)
                .parse()
                .unwrap(),
        );
        
        // Full user-agent with detailed system info (matching Node.js implementation)
        headers.insert(
            "user-agent",
            format!(
                "aws-sdk-js/{} ua/2.1 os/{} lang/rust md/rust#{} api/codewhispererstreaming#{} m/E KiroIDE-{}-{}",
                sdk_version, os_info, rust_version, sdk_version, kiro_version, &self.machine_id
            )
            .parse()
            .unwrap(),
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

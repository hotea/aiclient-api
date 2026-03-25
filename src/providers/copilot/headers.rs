use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::sync::Arc;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

pub struct CopilotHeaders {
    machine_id: String,
    session_id: Arc<RwLock<String>>,
    vscode_version: String,
}

fn generate_session_id() -> String {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{}{}", Uuid::new_v4(), timestamp_ms)
}

impl CopilotHeaders {
    pub fn new(vscode_version: &str) -> Self {
        let machine_id = crate::util::machine_id::get_machine_id();
        let session_id = generate_session_id();
        Self {
            machine_id,
            session_id: Arc::new(RwLock::new(session_id)),
            vscode_version: vscode_version.to_string(),
        }
    }

    pub fn build(&self, copilot_token: &str) -> HeaderMap {
        let request_id = Uuid::new_v4().to_string();
        // We need a synchronous read; for build() we use blocking read
        // Since this is called from async contexts, use try_read or block_in_place
        let session_id = self
            .session_id
            .try_read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| generate_session_id());

        let mut headers = HeaderMap::new();

        let insert = |map: &mut HeaderMap, key: &str, val: &str| {
            if let (Ok(k), Ok(v)) = (
                HeaderName::from_str(key),
                HeaderValue::from_str(val),
            ) {
                map.insert(k, v);
            }
        };

        insert(&mut headers, "authorization", &format!("Bearer {}", copilot_token));
        insert(&mut headers, "content-type", "application/json");
        insert(&mut headers, "copilot-integration-id", "vscode-chat");
        insert(
            &mut headers,
            "editor-version",
            &format!("vscode/{}", self.vscode_version),
        );
        insert(&mut headers, "editor-plugin-version", "copilot-chat/0.38.2");
        insert(&mut headers, "user-agent", "GitHubCopilotChat/0.38.2");
        insert(&mut headers, "openai-intent", "conversation-agent");
        insert(&mut headers, "x-github-api-version", "2025-10-01");
        insert(&mut headers, "x-request-id", &request_id);
        insert(&mut headers, "x-vscode-user-agent-library-version", "electron-fetch");
        insert(&mut headers, "x-agent-task-id", &request_id);
        insert(&mut headers, "x-interaction-type", "conversation-agent");
        insert(&mut headers, "vscode-machineid", &self.machine_id);
        insert(&mut headers, "vscode-sessionid", &session_id);

        headers
    }

    pub fn start_session_rotation(self: &Arc<Self>) {
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(3600)).await;
                let new_id = generate_session_id();
                *session_id.write().await = new_id;
                tracing::debug!("Rotated Copilot session ID");
            }
        });
    }
}

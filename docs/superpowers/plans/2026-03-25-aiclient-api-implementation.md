# aiclient-api Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust-based unified AI gateway daemon that authenticates against GitHub Copilot and Kiro, exposing their models through OpenAI-compatible and Anthropic-compatible API endpoints with full CLI control.

**Architecture:** Layered monolith — single binary with clap CLI dispatching to either interactive auth commands or a background daemon. The daemon runs an axum HTTP server (TCP) for API routes and an axum Unix socket server for control commands. Providers implement a `Provider` trait; format conversion is handled by stateless converter functions.

**Tech Stack:** Rust, tokio, axum 0.8, reqwest, clap 4, serde/serde_json, toml, tracing, arc-swap, daemonize

**Spec:** `docs/superpowers/specs/2026-03-25-aiclient-api-design.md`

---

## File Structure

```
aiclient-api/
├── Cargo.toml
├── config.example.toml
├── src/
│   ├── main.rs                    # clap CLI definition + dispatch
│   ├── cli/
│   │   ├── mod.rs                 # CLI subcommand enum re-exports
│   │   ├── auth.rs                # auth copilot | kiro | list | revoke
│   │   ├── start.rs               # start [--foreground] — launch daemon
│   │   ├── stop.rs                # stop — SIGTERM via PID file
│   │   ├── restart.rs             # stop + start
│   │   ├── status.rs              # query daemon via Unix socket
│   │   ├── config_cmd.rs          # config show | set | reload
│   │   ├── models.rs              # list models via Unix socket
│   │   ├── provider_cmd.rs        # provider enable | disable
│   │   ├── logs.rs                # tail daemon logs
│   │   ├── update.rs              # self-update from GitHub Releases
│   │   └── uninstall.rs           # stop + cleanup
│   ├── config/
│   │   ├── mod.rs                 # load(), default(), merge logic
│   │   └── types.rs               # Config, ServerConfig, ProviderConfig, etc.
│   ├── auth/
│   │   ├── mod.rs                 # TokenStore trait + TokenData enum
│   │   ├── copilot.rs             # GitHub device flow OAuth
│   │   ├── kiro.rs                # AWS Builder ID device flow
│   │   └── token_store.rs         # XDG file persistence (JSON, 0600)
│   ├── daemon/
│   │   ├── mod.rs                 # daemonize fork, PID file, signal handling
│   │   └── control.rs             # Unix socket JSON-RPC server
│   ├── server/
│   │   ├── mod.rs                 # axum Router assembly + middleware
│   │   ├── middleware.rs          # auth, CORS, request-id, logging, rate-limit
│   │   └── state.rs               # AppState struct
│   ├── routes/
│   │   ├── mod.rs                 # route registration helper
│   │   ├── openai.rs              # POST /v1/chat/completions, GET /v1/models
│   │   ├── anthropic.rs           # POST /v1/messages
│   │   └── health.rs              # GET /healthz
│   ├── providers/
│   │   ├── mod.rs                 # Provider trait + Model struct + ProviderRequest/Response
│   │   ├── router.rs              # model-to-provider resolution logic
│   │   ├── copilot/
│   │   │   ├── mod.rs             # CopilotProvider impl Provider
│   │   │   ├── client.rs          # reqwest calls to api.githubcopilot.com
│   │   │   ├── models.rs          # model listing & capability detection
│   │   │   └── headers.rs         # VSCode header spoofing (machine-id, session-id)
│   │   └── kiro/
│   │       ├── mod.rs             # KiroProvider impl Provider
│   │       ├── client.rs          # CodeWhisperer generateAssistantResponse calls
│   │       ├── models.rs          # Kiro model listing
│   │       └── cw_types.rs        # CodeWhisperer request/response structs
│   ├── convert/
│   │   ├── mod.rs                 # re-exports, OutputFormat enum
│   │   ├── openai_types.rs        # OpenAI request/response structs
│   │   ├── anthropic_types.rs     # Anthropic request/response structs
│   │   ├── to_openai.rs           # ProviderResponse → OpenAI format
│   │   ├── to_anthropic.rs        # ProviderResponse → Anthropic format
│   │   ├── from_openai.rs         # OpenAI request → ProviderRequest
│   │   ├── from_anthropic.rs      # Anthropic request → ProviderRequest
│   │   └── stream.rs              # SSE chunk converters (both directions)
│   └── util/
│       ├── mod.rs
│       ├── error.rs               # AppError enum, IntoResponse impl
│       ├── stream.rs              # SSE stream helpers
│       ├── machine_id.rs          # SHA256(MAC address) generation
│       └── xdg.rs                 # XDG path resolution helpers
└── tests/
    ├── config_test.rs
    ├── convert_test.rs
    ├── auth_test.rs
    └── routes_test.rs
```

---

## Phase 1: Project Skeleton & Config

### Task 1: Initialize Cargo Project & Dependencies

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs` (minimal)
- Create: `config.example.toml`

- [ ] **Step 1: Initialize Cargo project**

Run: `cargo init --name aiclient-api`

- [ ] **Step 2: Write Cargo.toml with all dependencies**

```toml
[package]
name = "aiclient-api"
version = "0.1.0"
edition = "2024"

[dependencies]
# CLI
clap = { version = "4", features = ["derive"] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# HTTP server
axum = { version = "0.8", features = ["macros"] }
tower = { version = "0.5" }
tower-http = { version = "0.6", features = ["cors", "catch-panic", "request-id", "trace", "util"] }

# HTTP client
reqwest = { version = "0.12", features = ["json", "stream"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"

# Streaming
tokio-stream = { version = "0.1", features = ["sync"] }
futures = "0.3"
bytes = "1"

# Config hot-reload
arc-swap = "1"

# Auth & crypto
sha2 = "0.10"
mac_address = "1"
uuid = { version = "1", features = ["v4"] }
open = "5"
base64 = "0.22"

# XDG dirs
dirs = "6"

# Daemon
daemonize = "0.5"

# Unix socket
hyper = { version = "1", features = ["server"] }
hyper-util = { version = "0.1", features = ["tokio"] }
tokio-util = { version = "0.7" }

# Error handling
anyhow = "1"
thiserror = "2"

# Async trait
async-trait = "0.1"

# Time
chrono = { version = "0.4", features = ["serde"] }

# Self-update
self_update = "0.41"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Create minimal main.rs that compiles**

```rust
fn main() {
    println!("aiclient-api");
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully (downloads dependencies)

- [ ] **Step 5: Create config.example.toml**

Copy the config example from the spec (Section 8.1).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs config.example.toml
git commit -m "feat: initialize Cargo project with dependencies"
```

---

### Task 2: Utility Modules (XDG, Machine ID, Errors)

**Files:**
- Create: `src/util/mod.rs`
- Create: `src/util/xdg.rs`
- Create: `src/util/machine_id.rs`
- Create: `src/util/error.rs`
- Create: `src/util/stream.rs`

- [ ] **Step 1: Write tests for XDG path resolution**

Create `tests/util_test.rs`:
```rust
#[test]
fn test_config_dir_returns_path() {
    let path = aiclient_api::util::xdg::config_dir();
    assert!(path.ends_with("aiclient-api"));
}

#[test]
fn test_runtime_dir_returns_path() {
    let path = aiclient_api::util::xdg::runtime_dir();
    assert!(path.to_str().unwrap().contains("aiclient-api"));
}

#[test]
fn test_state_dir_returns_path() {
    let path = aiclient_api::util::xdg::state_dir();
    assert!(path.to_str().unwrap().contains("aiclient-api"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test util_test`
Expected: FAIL — module not found

- [ ] **Step 3: Implement `src/util/xdg.rs`**

```rust
use std::path::PathBuf;

const APP_NAME: &str = "aiclient-api";

/// ~/.config/aiclient-api/
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join(APP_NAME)
}

/// $XDG_RUNTIME_DIR/aiclient-api/ or /tmp/aiclient-api-{uid}/
pub fn runtime_dir() -> PathBuf {
    if let Some(dir) = dirs::runtime_dir() {
        return dir.join(APP_NAME);
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/{}-{}", APP_NAME, uid))
}

/// ~/.local/state/aiclient-api/
pub fn state_dir() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".local/state")
        })
        .join(APP_NAME)
}

/// Socket path: $XDG_RUNTIME_DIR/aiclient-api/ctl.sock
pub fn socket_path() -> PathBuf {
    runtime_dir().join("ctl.sock")
}

/// PID file path
pub fn pid_path() -> PathBuf {
    runtime_dir().join("daemon.pid")
}

/// Default log file path
pub fn log_path() -> PathBuf {
    state_dir().join("daemon.log")
}
```

Note: Add `libc = "0.2"` to Cargo.toml dependencies.

- [ ] **Step 4: Implement `src/util/machine_id.rs`**

```rust
use sha2::{Sha256, Digest};

/// Generate a machine ID from the primary MAC address, SHA256 hashed.
/// Falls back to a random UUID persisted in config dir if no MAC found.
pub fn get_machine_id() -> String {
    if let Ok(Some(addr)) = mac_address::get_mac_address() {
        let mut hasher = Sha256::new();
        hasher.update(addr.to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    } else {
        // Fallback: load or generate a persistent random ID
        let path = super::xdg::config_dir().join("machine_id");
        if let Ok(id) = std::fs::read_to_string(&path) {
            return id.trim().to_string();
        }
        let id = uuid::Uuid::new_v4().to_string();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, &id);
        id
    }
}
```

- [ ] **Step 5: Implement `src/util/error.rs`**

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Provider error: {0}")]
    Provider(#[from] anyhow::Error),

    #[error("Authentication required: {0}")]
    Unauthorized(String),

    #[error("Provider unavailable: {0}")]
    Unavailable(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Rate limited")]
    RateLimited,

    #[error("Upstream error: {status} {body}")]
    Upstream { status: u16, body: String },
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Unavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded".into()),
            AppError::Upstream { status, body } => {
                let code = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
                return (code, body.clone()).into_response();
            }
            AppError::Provider(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
        let body = json!({ "error": { "message": message, "type": "error" } });
        (status, axum::Json(body)).into_response()
    }
}
```

- [ ] **Step 6: Implement `src/util/stream.rs`** (placeholder for SSE helpers)

```rust
// SSE stream helpers — will be filled in during streaming task
```

- [ ] **Step 7: Wire up `src/util/mod.rs`**

```rust
pub mod error;
pub mod machine_id;
pub mod stream;
pub mod xdg;
```

- [ ] **Step 8: Update `src/main.rs` to expose util as lib**

Create `src/lib.rs`:
```rust
pub mod util;
```

- [ ] **Step 9: Run tests**

Run: `cargo test --test util_test`
Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add src/util/ src/lib.rs tests/util_test.rs Cargo.toml Cargo.lock
git commit -m "feat: add utility modules (xdg, machine_id, error)"
```

---

### Task 3: Config Types & Loading

**Files:**
- Create: `src/config/types.rs`
- Create: `src/config/mod.rs`

- [ ] **Step 1: Write test for config deserialization**

Create `tests/config_test.rs`:
```rust
use aiclient_api::config::types::*;

#[test]
fn test_deserialize_full_config() {
    let toml_str = r#"
default_format = "openai"
default_provider = "copilot"
api_key = ""
vscode_version = "1.110.1"

[server]
host = "127.0.0.1"
port = 9090
rate_limit_seconds = 0

[providers.copilot]
type = "copilot"
enabled = true
account_type = "individual"

[providers.kiro]
type = "kiro"
enabled = true
region = "us-east-1"

[logging]
level = "info"
file = ""
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.default_provider, "copilot");
    assert_eq!(config.server.port, 9090);
    assert!(config.providers.contains_key("copilot"));
    assert!(config.providers.contains_key("kiro"));
}

#[test]
fn test_default_config_is_valid() {
    let config = Config::default();
    assert_eq!(config.server.port, 9090);
    assert_eq!(config.default_format, Format::OpenAI);
}

#[test]
fn test_provider_config_copilot_discriminant() {
    let toml_str = r#"
type = "copilot"
enabled = true
account_type = "individual"
"#;
    let pc: ProviderConfig = toml::from_str(toml_str).unwrap();
    match pc {
        ProviderConfig::Copilot { account_type, .. } => {
            assert_eq!(account_type, AccountType::Individual);
        }
        _ => panic!("Expected Copilot variant"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test config_test`
Expected: FAIL

- [ ] **Step 3: Implement `src/config/types.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    OpenAI,
    Anthropic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Individual,
    Business,
    Enterprise,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_format")]
    pub default_format: Format,
    #[serde(default = "default_provider")]
    pub default_provider: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_vscode_version")]
    pub vscode_version: String,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub logging: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub rate_limit_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProviderConfig {
    #[serde(rename = "copilot")]
    Copilot {
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        account_type: AccountType,
        enterprise_url: Option<String>,
    },
    #[serde(rename = "kiro")]
    Kiro {
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default = "default_region")]
        region: String,
        idc_region: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub file: String,
}

// Default value functions
fn default_format() -> Format { Format::OpenAI }
fn default_provider() -> String { "copilot".to_string() }
fn default_vscode_version() -> String { "1.110.1".to_string() }
fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 9090 }
fn default_true() -> bool { true }
fn default_region() -> String { "us-east-1".to_string() }
fn default_log_level() -> String { "info".to_string() }

impl Default for Config {
    fn default() -> Self {
        Config {
            default_format: default_format(),
            default_provider: default_provider(),
            api_key: String::new(),
            vscode_version: default_vscode_version(),
            server: ServerConfig::default(),
            providers: HashMap::new(),
            logging: LogConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            host: default_host(),
            port: default_port(),
            rate_limit_seconds: 0,
        }
    }
}

impl Default for AccountType {
    fn default() -> Self {
        AccountType::Individual
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        LogConfig {
            level: default_log_level(),
            file: String::new(),
        }
    }
}
```

- [ ] **Step 4: Implement `src/config/mod.rs`**

```rust
pub mod types;

use anyhow::{Context, Result};
use std::path::Path;
use types::Config;

/// Load config from file, falling back to defaults for missing fields
pub fn load_config(path: &Path) -> Result<Config> {
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {}", path.display()))?;
        Ok(config)
    } else {
        Ok(Config::default())
    }
}

/// Load config from the default XDG path
pub fn load_default_config() -> Result<Config> {
    let path = crate::util::xdg::config_dir().join("config.toml");
    load_config(&path)
}
```

- [ ] **Step 5: Wire up in `src/lib.rs`**

```rust
pub mod config;
pub mod util;
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test config_test`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/config/ tests/config_test.rs src/lib.rs
git commit -m "feat: add config types and loading"
```

---

## Phase 2: CLI Framework & Process Management

### Task 4: CLI Argument Parsing with clap

**Files:**
- Create: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement `src/cli/mod.rs` with clap derive**

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aiclient-api", about = "Unified AI gateway service")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the daemon
    Start {
        /// API port
        #[arg(long, default_value = "9090")]
        port: u16,
        /// Bind address
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Run in foreground (don't daemonize)
        #[arg(long)]
        foreground: bool,
        /// Protect API with bearer token
        #[arg(long)]
        api_key: Option<String>,
        /// Log file path
        #[arg(long)]
        log_file: Option<String>,
    },
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Authentication management
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Show daemon status
    Status,
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// List available models
    Models,
    /// Provider management
    Provider {
        #[command(subcommand)]
        action: ProviderAction,
    },
    /// Tail daemon logs
    Logs {
        /// Show last N lines
        #[arg(long, default_value = "50")]
        lines: usize,
        /// Filter level
        #[arg(long, default_value = "info")]
        level: String,
    },
    /// Self-update from GitHub Releases
    Update,
    /// Stop daemon + remove config + remove binary
    Uninstall,
}

#[derive(Subcommand)]
pub enum AuthAction {
    /// Authenticate with GitHub Copilot
    Copilot {
        #[arg(long, default_value = "individual")]
        account_type: String,
    },
    /// Authenticate with Kiro
    Kiro,
    /// List authenticated providers
    List,
    /// Revoke a provider's tokens
    Revoke {
        /// Provider name
        provider: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current config
    Show,
    /// Set a config value
    Set {
        key: String,
        value: String,
    },
    /// Reload config from disk
    Reload,
}

#[derive(Subcommand)]
pub enum ProviderAction {
    /// Enable a provider
    Enable { name: String },
    /// Disable a provider
    Disable { name: String },
}
```

- [ ] **Step 2: Update `src/main.rs` to parse CLI args**

```rust
mod cli;

use clap::Parser;
use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Start { port, host, foreground, api_key, log_file } => {
            println!("Starting daemon on {}:{}", host, port);
            // TODO: implement
        }
        Command::Stop => println!("Stopping daemon..."),
        Command::Restart => println!("Restarting daemon..."),
        Command::Auth { action } => println!("Auth: {:?}", "todo"),
        Command::Status => println!("Querying status..."),
        Command::Config { action } => println!("Config: todo"),
        Command::Models => println!("Listing models..."),
        Command::Provider { action } => println!("Provider: todo"),
        Command::Logs { lines, level } => println!("Tailing logs..."),
    }
}
```

- [ ] **Step 3: Verify CLI parses correctly**

Run: `cargo run -- --help`
Expected: Shows help with all subcommands

Run: `cargo run -- start --help`
Expected: Shows start options

- [ ] **Step 4: Commit**

```bash
git add src/cli/ src/main.rs
git commit -m "feat: add CLI argument parsing with clap"
```

---

### Task 5: Token Store (File Persistence)

**Files:**
- Create: `src/auth/mod.rs`
- Create: `src/auth/token_store.rs`

- [ ] **Step 1: Write tests for token store**

Create `tests/auth_test.rs`:
```rust
use aiclient_api::auth::token_store::XdgTokenStore;
use aiclient_api::auth::{TokenData, TokenStore};
use tempfile::TempDir;

#[tokio::test]
async fn test_save_and_load_copilot_token() {
    let tmp = TempDir::new().unwrap();
    let store = XdgTokenStore::new(tmp.path().to_path_buf());

    let data = TokenData::Copilot {
        github_token: "gho_test123".to_string(),
        copilot_token: None,
        expires_at: None,
    };
    store.save("copilot", &data).await.unwrap();
    let loaded = store.load("copilot").await.unwrap();

    match loaded {
        TokenData::Copilot { github_token, .. } => {
            assert_eq!(github_token, "gho_test123");
        }
        _ => panic!("Expected Copilot token"),
    }
}

#[tokio::test]
async fn test_delete_token() {
    let tmp = TempDir::new().unwrap();
    let store = XdgTokenStore::new(tmp.path().to_path_buf());

    let data = TokenData::Copilot {
        github_token: "gho_test".to_string(),
        copilot_token: None,
        expires_at: None,
    };
    store.save("copilot", &data).await.unwrap();
    store.delete("copilot").await.unwrap();

    assert!(store.load("copilot").await.is_err());
}

#[tokio::test]
async fn test_load_nonexistent_returns_error() {
    let tmp = TempDir::new().unwrap();
    let store = XdgTokenStore::new(tmp.path().to_path_buf());
    assert!(store.load("nonexistent").await.is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test auth_test`
Expected: FAIL

- [ ] **Step 3: Implement `src/auth/mod.rs`**

```rust
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
```

- [ ] **Step 4: Implement `src/auth/token_store.rs`**

```rust
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

        // Set permissions to 0600
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
        // Also remove the provider directory if empty
        let dir = self.base_dir.join(provider);
        if dir.exists() {
            let _ = tokio::fs::remove_dir(&dir).await;
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Wire up in `src/lib.rs`**

```rust
pub mod auth;
pub mod config;
pub mod util;
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test auth_test`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/auth/ tests/auth_test.rs src/lib.rs
git commit -m "feat: add token store with XDG file persistence"
```

---

### Task 6: Daemon Process Management

**Files:**
- Create: `src/daemon/mod.rs`

- [ ] **Step 1: Implement `src/daemon/mod.rs`**

```rust
use anyhow::{bail, Context, Result};
use std::path::Path;

/// Read PID from file and check if process is alive
pub fn read_pid() -> Result<Option<u32>> {
    let pid_path = crate::util::xdg::pid_path();
    if !pid_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&pid_path)?;
    let pid: u32 = content.trim().parse()?;

    // Check if process is alive
    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
    if alive {
        Ok(Some(pid))
    } else {
        // Stale PID file — clean up
        let _ = std::fs::remove_file(&pid_path);
        Ok(None)
    }
}

/// Write PID to file
pub fn write_pid(pid: u32) -> Result<()> {
    let pid_path = crate::util::xdg::pid_path();
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_path, pid.to_string())?;
    Ok(())
}

/// Remove PID file
pub fn remove_pid() -> Result<()> {
    let pid_path = crate::util::xdg::pid_path();
    if pid_path.exists() {
        std::fs::remove_file(&pid_path)?;
    }
    Ok(())
}

/// Stop the daemon: send SIGTERM, wait up to 10s, then SIGKILL
pub fn stop_daemon() -> Result<()> {
    match read_pid()? {
        Some(pid) => {
            eprintln!("Stopping daemon (pid {})...", pid);
            unsafe { libc::kill(pid as i32, libc::SIGTERM); }

            // Poll for exit (10 second timeout)
            for _ in 0..100 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                if !alive {
                    remove_pid()?;
                    eprintln!("Daemon stopped.");
                    return Ok(());
                }
            }

            // Force kill
            eprintln!("Daemon didn't stop gracefully, sending SIGKILL...");
            unsafe { libc::kill(pid as i32, libc::SIGKILL); }
            std::thread::sleep(std::time::Duration::from_millis(500));
            remove_pid()?;
            eprintln!("Daemon killed.");
            Ok(())
        }
        None => {
            eprintln!("Daemon is not running.");
            Ok(())
        }
    }
}

/// Daemonize the current process (fork to background)
pub fn daemonize(log_file: &Path) -> Result<()> {
    use daemonize::Daemonize;

    let pid_path = crate::util::xdg::pid_path();
    let runtime_dir = crate::util::xdg::runtime_dir();
    std::fs::create_dir_all(&runtime_dir)?;

    // Ensure parent dir for log file exists
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let stdout = std::fs::OpenOptions::new()
        .create(true).append(true).open(log_file)?;
    let stderr = stdout.try_clone()?;

    let daemon = Daemonize::new()
        .pid_file(&pid_path)
        .working_directory(".")
        .stdout(stdout)
        .stderr(stderr);

    daemon.start()
        .context("Failed to daemonize")?;

    Ok(())
}
```

- [ ] **Step 2: Wire up in `src/lib.rs`**

Add `pub mod daemon;`

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/daemon/ src/lib.rs
git commit -m "feat: add daemon process management (fork, PID, signals)"
```

---

## Phase 3: Server & Routing Foundation

### Task 7: AppState & Server Setup

**Files:**
- Create: `src/server/mod.rs`
- Create: `src/server/state.rs`
- Create: `src/server/middleware.rs`

- [ ] **Step 1: Implement `src/server/state.rs`**

```rust
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
```

- [ ] **Step 2: Create the Provider trait stub `src/providers/mod.rs`**

```rust
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub mod router;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub provider: String,
    pub vendor: String,
    pub display_name: String,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_thinking: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub system: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub tools: Option<Vec<serde_json::Value>>,
    pub tool_choice: Option<serde_json::Value>,
    pub extra: serde_json::Value,
}

pub enum ProviderResponse {
    Complete(serde_json::Value),
    Stream(Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    OpenAI,
    Anthropic,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn is_healthy(&self) -> bool;
    async fn list_models(&self) -> Result<Vec<Model>>;
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse>;

    fn supports_passthrough(&self, _format: OutputFormat) -> bool { false }

    async fn passthrough(
        &self,
        _model: &str,
        _body: serde_json::Value,
        _format: OutputFormat,
        _stream: bool,
    ) -> Result<ProviderResponse> {
        anyhow::bail!("passthrough not supported")
    }
}
```

- [ ] **Step 3: Create provider router stub `src/providers/router.rs`**

```rust
use super::Provider;
use crate::server::state::AppState;
use anyhow::{bail, Result};
use std::sync::Arc;

/// Resolve which provider handles a request.
/// Priority: model prefix ("copilot/gpt-5") > X-Provider header > config default
pub async fn resolve_provider(
    state: &AppState,
    model: &str,
    header_provider: Option<&str>,
) -> Result<(Arc<dyn Provider>, String)> {
    let providers = state.providers.read().await;
    let config = state.config.load();

    // Check model prefix: "copilot/gpt-5" → provider="copilot", model="gpt-5"
    if let Some((prefix, actual_model)) = model.split_once('/') {
        if let Some(provider) = providers.get(prefix) {
            return Ok((provider.clone(), actual_model.to_string()));
        }
        bail!("Provider '{}' not found", prefix);
    }

    // Check X-Provider header
    if let Some(name) = header_provider {
        if let Some(provider) = providers.get(name) {
            return Ok((provider.clone(), model.to_string()));
        }
        bail!("Provider '{}' not found", name);
    }

    // Fallback to default
    let default_name = &config.default_provider;
    if let Some(provider) = providers.get(default_name.as_str()) {
        return Ok((provider.clone(), model.to_string()));
    }

    bail!("No provider available for model '{}'", model)
}
```

- [ ] **Step 4: Implement `src/server/middleware.rs`**

```rust
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

/// Add x-request-id header to every request (axum 0.8 signature)
pub async fn request_id(
    mut req: Request,
    next: Next,
) -> Response {
    let id = Uuid::new_v4().to_string();
    req.headers_mut().insert(
        "x-request-id",
        id.parse().unwrap(),
    );
    next.run(req).await
}

/// CORS layer — allow all origins (local gateway)
pub fn cors_layer() -> CorsLayer {
    CorsLayer::very_permissive()
}
```

- [ ] **Step 5: Implement `src/server/mod.rs`**

```rust
pub mod middleware;
pub mod state;

use axum::Router;
use state::AppState;
use tower_http::catch_panic::CatchPanicLayer;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Routes will be added in later tasks
        .layer(CatchPanicLayer::new())
        .layer(middleware::cors_layer())
        .with_state(state)
}
```

- [ ] **Step 6: Wire up in `src/lib.rs`**

```rust
pub mod auth;
pub mod config;
pub mod daemon;
pub mod providers;
pub mod server;
pub mod util;
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo build`
Expected: Compiles (may need minor fixes for generic params in middleware)

- [ ] **Step 8: Commit**

```bash
git add src/server/ src/providers/ src/lib.rs
git commit -m "feat: add server skeleton, AppState, Provider trait, router"
```

---

### Task 8: Route Handlers (Health, Models, OpenAI, Anthropic)

**Files:**
- Create: `src/routes/mod.rs`
- Create: `src/routes/health.rs`
- Create: `src/routes/openai.rs`
- Create: `src/routes/anthropic.rs`

- [ ] **Step 1: Write integration test for health endpoint**

Create `tests/routes_test.rs`:
```rust
use axum::http::StatusCode;
use axum_test::TestServer;

#[tokio::test]
async fn test_healthz_returns_ok() {
    let config = aiclient_api::config::types::Config::default();
    let state = aiclient_api::server::state::AppState::new(config);
    let app = aiclient_api::server::build_router(state);

    let server = TestServer::new(app).unwrap();
    let response = server.get("/healthz").await;
    response.assert_status_ok();
    response.assert_json(&serde_json::json!({ "status": "ok" }));
}
```

Note: Add `axum-test = "16"` to `[dev-dependencies]` in Cargo.toml.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test routes_test`
Expected: FAIL (no route for /healthz)

- [ ] **Step 3: Implement `src/routes/health.rs`**

```rust
use axum::Json;
use serde_json::{json, Value};

pub async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}
```

- [ ] **Step 4: Implement `src/routes/openai.rs`** (stub with TODO)

```rust
use axum::extract::State;
use axum::Json;
use serde_json::Value;

use crate::server::state::AppState;
use crate::util::error::AppError;

/// POST /v1/chat/completions
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    // TODO: implement in format conversion task
    Err(AppError::Unavailable("Not yet implemented".into()))
}

/// GET /v1/models
pub async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let providers = state.providers.read().await;
    let mut models = Vec::new();
    for provider in providers.values() {
        if let Ok(provider_models) = provider.list_models().await {
            for m in provider_models {
                models.push(serde_json::json!({
                    "id": m.id,
                    "object": "model",
                    "created": 0,
                    "owned_by": m.provider,
                }));
            }
        }
    }
    Ok(Json(serde_json::json!({
        "object": "list",
        "data": models,
    })))
}
```

- [ ] **Step 5: Implement `src/routes/anthropic.rs`** (stub)

```rust
use axum::extract::State;
use axum::Json;
use serde_json::Value;

use crate::server::state::AppState;
use crate::util::error::AppError;

/// POST /v1/messages
pub async fn messages(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    // TODO: implement in format conversion task
    Err(AppError::Unavailable("Not yet implemented".into()))
}
```

- [ ] **Step 6: Implement `src/routes/mod.rs`**

```rust
pub mod anthropic;
pub mod health;
pub mod openai;
```

- [ ] **Step 7: Wire routes into `src/server/mod.rs`**

```rust
pub fn build_router(state: AppState) -> Router {
    use axum::routing::{get, post};

    Router::new()
        .route("/healthz", get(crate::routes::health::healthz))
        .route("/v1/chat/completions", post(crate::routes::openai::chat_completions))
        .route("/v1/models", get(crate::routes::openai::list_models))
        .route("/v1/messages", post(crate::routes::anthropic::messages))
        .layer(CatchPanicLayer::new())
        .layer(middleware::cors_layer())
        .with_state(state)
}
```

- [ ] **Step 8: Wire up routes module in `src/lib.rs`**

Add `pub mod routes;`

- [ ] **Step 9: Run tests**

Run: `cargo test --test routes_test`
Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add src/routes/ src/server/mod.rs src/lib.rs Cargo.toml tests/routes_test.rs
git commit -m "feat: add route handlers (health, openai, anthropic stubs)"
```

---

### Task 9: Daemon Start Command (Wire CLI → Server)

**Files:**
- Create: `src/cli/start.rs`
- Create: `src/cli/stop.rs`
- Create: `src/cli/restart.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement `src/cli/start.rs`**

```rust
use anyhow::Result;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

pub async fn run(
    host: String,
    port: u16,
    foreground: bool,
    api_key: Option<String>,
    log_file: Option<String>,
) -> Result<()> {
    // Check if already running
    if let Some(pid) = crate::daemon::read_pid()? {
        anyhow::bail!("Daemon already running (pid {})", pid);
    }

    // Load config
    let mut config = crate::config::load_default_config()?;

    // Apply CLI overrides
    config.server.host = host;
    config.server.port = port;
    if let Some(key) = api_key {
        config.api_key = key;
    }

    let log_path = log_file
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::util::xdg::log_path());

    if !foreground {
        crate::daemon::daemonize(&log_path)?;
    }

    // Setup tracing
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    if foreground {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .init();
    } else {
        let file_appender = tracing_appender::rolling::never(
            log_path.parent().unwrap_or(&PathBuf::from(".")),
            log_path.file_name().unwrap_or_default(),
        );
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(file_appender)
            .with_ansi(false)
            .init();
    }

    tracing::info!("aiclient-api starting on {}:{}", config.server.host, config.server.port);

    // Write PID (daemonize does this too, but foreground mode needs it)
    if foreground {
        crate::daemon::write_pid(std::process::id())?;
    }

    // Build state & router
    let state = crate::server::state::AppState::new(config.clone());
    let app = crate::server::build_router(state);

    // Start listener
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);

    // Graceful shutdown on SIGTERM/SIGINT
    let shutdown = async {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate()
        ).expect("failed to install SIGTERM handler");
        let sigint = tokio::signal::ctrl_c();

        tokio::select! {
            _ = sigterm.recv() => tracing::info!("Received SIGTERM"),
            _ = sigint => tracing::info!("Received SIGINT"),
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    tracing::info!("Shutting down...");
    crate::daemon::remove_pid()?;
    Ok(())
}
```

- [ ] **Step 2: Implement `src/cli/stop.rs`**

```rust
use anyhow::Result;

pub fn run() -> Result<()> {
    crate::daemon::stop_daemon()
}
```

- [ ] **Step 3: Implement `src/cli/restart.rs`**

```rust
use anyhow::Result;

pub async fn run(
    host: String,
    port: u16,
    foreground: bool,
    api_key: Option<String>,
    log_file: Option<String>,
) -> Result<()> {
    let _ = crate::daemon::stop_daemon();
    // Small delay to ensure socket is freed
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    crate::cli::start::run(host, port, foreground, api_key, log_file).await
}
```

- [ ] **Step 4: Restructure CLI module files**

Create `src/cli/commands.rs` by moving the clap derive structs (`Cli`, `Command`, `AuthAction`, `ConfigAction`, `ProviderAction`) from Task 4's `cli/mod.rs` into this new file.

Then update `src/cli/mod.rs` to:
```rust
mod commands;
pub use commands::*;

pub mod auth;
pub mod start;
pub mod stop;
pub mod restart;
pub mod status;
pub mod config_cmd;
pub mod models;
pub mod provider_cmd;
pub mod logs;
pub mod update;
pub mod uninstall;
```

- [ ] **Step 5: Wire main.rs to call start/stop/restart**

Update `src/main.rs`:
```rust
use clap::Parser;

mod cli;

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Command::Start { port, host, foreground, api_key, log_file } => {
            cli::start::run(host, port, foreground, api_key, log_file).await
        }
        cli::Command::Stop => cli::stop::run(),
        cli::Command::Restart => {
            // Use defaults for restart — TODO: accept same args as start
            cli::restart::run(
                "127.0.0.1".into(), 9090, false, None, None
            ).await
        }
        _ => {
            eprintln!("Command not yet implemented");
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
```

- [ ] **Step 6: Create placeholder files for remaining CLI modules**

Create empty stub files for `src/cli/auth.rs`, `src/cli/status.rs`, `src/cli/config_cmd.rs`, `src/cli/models.rs`, `src/cli/provider_cmd.rs`, `src/cli/logs.rs`, `src/cli/update.rs`, `src/cli/uninstall.rs` — each containing a single `// TODO: implement` comment.

- [ ] **Step 7: Verify foreground start works**

Run: `cargo run -- start --foreground`
Expected: Server starts, shows "Listening on 127.0.0.1:9090"

Run in another terminal: `curl http://127.0.0.1:9090/healthz`
Expected: `{"status":"ok"}`

Stop with Ctrl+C.

- [ ] **Step 8: Commit**

```bash
git add src/cli/ src/main.rs
git commit -m "feat: wire start/stop/restart CLI to daemon and server"
```

---

## Phase 4: GitHub Copilot Auth & Provider

### Task 10: Copilot OAuth Device Flow

**Files:**
- Modify: `src/cli/auth.rs`
- Create: `src/auth/copilot.rs`

- [ ] **Step 1: Implement `src/auth/copilot.rs`**

Implement the GitHub device flow OAuth:
1. POST to `https://github.com/login/device/code` with `client_id=Iv1.b507a08c87ecfe98`
2. Display user_code and verification_uri, open browser
3. Poll `https://github.com/login/oauth/access_token` until approved
4. Save token via TokenStore

Key function signatures:
```rust
pub async fn authenticate(store: &dyn TokenStore) -> Result<()>
pub async fn fetch_copilot_token(github_token: &str) -> Result<(String, i64, u64)>
```

The `fetch_copilot_token` function calls `GET https://api.github.com/copilot_internal/v2/token` and returns `(token, expires_at, refresh_in)`.

- [ ] **Step 2: Implement `src/cli/auth.rs`** (auth subcommand handler)

Wire auth copilot/kiro/list/revoke commands to call the appropriate auth module.

- [ ] **Step 3: Verify auth copilot works interactively**

Run: `cargo run -- auth copilot`
Expected: Shows device code and URL, opens browser, waits for approval

- [ ] **Step 4: Verify token persistence**

Run: `cat ~/.config/aiclient-api/copilot/token.json`
Expected: Contains `github_token` field

- [ ] **Step 5: Commit**

```bash
git add src/auth/copilot.rs src/cli/auth.rs
git commit -m "feat: implement Copilot GitHub device flow OAuth"
```

---

### Task 11: Copilot Provider (Client, Headers, Models)

**Files:**
- Create: `src/providers/copilot/mod.rs`
- Create: `src/providers/copilot/client.rs`
- Create: `src/providers/copilot/headers.rs`
- Create: `src/providers/copilot/models.rs`

- [ ] **Step 1: Implement `src/providers/copilot/headers.rs`**

Build VSCode-spoofed headers per spec Section 4.4:
- `vscode-machineid` from `machine_id::get_machine_id()`
- `vscode-sessionid` as `{UUIDv4}{timestamp_ms}`, rotated hourly
- All required headers: editor-version, user-agent, x-github-api-version, etc.

```rust
pub struct CopilotHeaders {
    machine_id: String,
    session_id: Arc<RwLock<String>>,
    vscode_version: String,
}

impl CopilotHeaders {
    pub fn new(vscode_version: &str) -> Self { ... }
    pub fn build(&self, copilot_token: &str) -> HeaderMap { ... }
    pub fn start_session_rotation(&self) { ... } // tokio::spawn hourly rotation
}
```

- [ ] **Step 2: Implement `src/providers/copilot/models.rs`**

Fetch model list from `GET https://api.githubcopilot.com/models` (with Copilot headers), parse into `Vec<Model>`. Map upstream model capabilities to the `Model` struct.

- [ ] **Step 3: Implement `src/providers/copilot/client.rs`**

The core HTTP client that sends requests to `api.githubcopilot.com`:
- `chat_completions()` → POST `/chat/completions` (OpenAI format passthrough)
- `messages()` → POST `/v1/messages` (Anthropic format passthrough)
- Both support streaming (return `reqwest::Response` for stream cases)

- [ ] **Step 4: Implement `src/providers/copilot/mod.rs`** (Provider trait impl)

```rust
pub struct CopilotProvider {
    client: CopilotClient,
    headers: CopilotHeaders,
    token: Arc<RwLock<Option<CopilotToken>>>,
    account_type: AccountType,
    healthy: AtomicBool,
}

#[async_trait]
impl Provider for CopilotProvider {
    fn name(&self) -> &str { "copilot" }
    fn is_healthy(&self) -> bool { self.healthy.load(Ordering::Relaxed) }
    async fn list_models(&self) -> Result<Vec<Model>> { ... }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse> { ... }
    fn supports_passthrough(&self, format: OutputFormat) -> bool { true } // Copilot supports both
    async fn passthrough(...) -> Result<ProviderResponse> { ... } // Forward raw body
}
```

Include a token refresh background loop:
```rust
pub fn start_token_refresh(self: &Arc<Self>) {
    let this = Arc::clone(self);
    tokio::spawn(async move {
        loop {
            // Refresh 60 seconds before expiry
            // On failure, retry every 15 seconds
        }
    });
}
```

- [ ] **Step 5: Register Copilot provider in daemon startup**

Modify `src/cli/start.rs` to initialize providers based on config, inserting them into `AppState.providers`.

- [ ] **Step 6: Test models endpoint with real auth**

Run: `cargo run -- start --foreground`
Then: `curl http://127.0.0.1:9090/v1/models`
Expected: Returns JSON with model list from Copilot

- [ ] **Step 7: Commit**

```bash
git add src/providers/copilot/ src/cli/start.rs
git commit -m "feat: implement Copilot provider (client, headers, models, token refresh)"
```

---

## Phase 5: Format Conversion & Request Flow

### Task 12: OpenAI & Anthropic Type Definitions

**Files:**
- Create: `src/convert/mod.rs`
- Create: `src/convert/openai_types.rs`
- Create: `src/convert/anthropic_types.rs`

- [ ] **Step 1: Define OpenAI request/response types in `src/convert/openai_types.rs`**

Serde-derivable structs for:
- `OpenAIChatRequest` (model, messages, stream, temperature, max_tokens, tools, tool_choice)
- `OpenAIChatResponse` (id, object, created, model, choices, usage)
- `OpenAIStreamChunk` (id, object, created, model, choices with delta)
- Supporting types: `OpenAIMessage`, `OpenAIDelta`, `OpenAIChoice`, `OpenAIUsage`

- [ ] **Step 2: Define Anthropic request/response types in `src/convert/anthropic_types.rs`**

Serde-derivable structs for:
- `AnthropicMessagesRequest` (model, messages, system, max_tokens, stream, temperature, tools)
- `AnthropicMessagesResponse` (id, type, role, content, model, stop_reason, usage)
- `AnthropicStreamEvent` (event types: message_start, content_block_start, content_block_delta, content_block_stop, message_delta, message_stop)
- Supporting types: `AnthropicMessage`, `AnthropicContent`, `AnthropicUsage`

- [ ] **Step 3: Write test for round-trip serialization**

```rust
#[test]
fn test_openai_request_deserialize() {
    let json = r#"{"model":"gpt-4","messages":[{"role":"user","content":"hello"}],"stream":false}"#;
    let req: OpenAIChatRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.model, "gpt-4");
}

#[test]
fn test_anthropic_request_deserialize() {
    let json = r#"{"model":"claude-3","messages":[{"role":"user","content":"hello"}],"max_tokens":1024}"#;
    let req: AnthropicMessagesRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.model, "claude-3");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test convert_test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/convert/
git commit -m "feat: add OpenAI and Anthropic API type definitions"
```

---

### Task 13: Format Conversion Functions

**Files:**
- Create: `src/convert/from_openai.rs`
- Create: `src/convert/from_anthropic.rs`
- Create: `src/convert/to_openai.rs`
- Create: `src/convert/to_anthropic.rs`

- [ ] **Step 1: Write conversion tests**

Test the key mappings from spec Section 7.3:
- System message extraction (OpenAI inline → Anthropic top-level)
- Tool call format conversion
- Content type normalization (string vs array)

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement `from_openai.rs`**

`pub fn from_openai(req: OpenAIChatRequest) -> Result<ProviderRequest>` — converts OpenAI chat request to internal ProviderRequest format.

- [ ] **Step 4: Implement `from_anthropic.rs`**

`pub fn from_anthropic(req: AnthropicMessagesRequest) -> Result<ProviderRequest>` — converts Anthropic messages request to ProviderRequest.

- [ ] **Step 5: Implement `to_openai.rs`**

`pub fn to_openai(resp: &serde_json::Value, model: &str) -> OpenAIChatResponse` — wraps provider response in OpenAI format.

- [ ] **Step 6: Implement `to_anthropic.rs`**

`pub fn to_anthropic(resp: &serde_json::Value, model: &str) -> AnthropicMessagesResponse` — wraps provider response in Anthropic format.

- [ ] **Step 7: Run tests**

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/convert/
git commit -m "feat: implement format conversion functions (OpenAI <-> Anthropic)"
```

---

### Task 14: SSE Stream Conversion

**Files:**
- Create: `src/convert/stream.rs`
- Modify: `src/util/stream.rs`

- [ ] **Step 1: Implement `src/convert/stream.rs`**

Two chunk converter functions:
- `pub fn chunk_to_openai(chunk: &[u8], model: &str) -> Vec<u8>` — parse provider SSE chunk, emit OpenAI SSE format
- `pub fn chunk_to_anthropic(chunk: &[u8], model: &str) -> Vec<u8>` — parse provider SSE chunk, emit Anthropic SSE format

Handle the SSE `data: ...` prefix and `\n\n` delimiter.

- [ ] **Step 2: Implement `src/util/stream.rs`** (SSE helpers)

```rust
use axum::response::sse::{Event, KeepAlive, Sse};
use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

use crate::convert::OutputFormat;
use crate::providers::ProviderResponse;

/// Convert a provider stream response into an SSE response
pub fn into_sse_response(
    stream: Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>,
    format: OutputFormat,
    model: &str,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let model = model.to_string();
    let mapped = futures::stream::unfold(
        (stream, model, format),
        |(mut stream, model, format)| async move {
            use futures::StreamExt;
            match stream.next().await {
                Some(Ok(bytes)) => {
                    let converted = match format {
                        OutputFormat::OpenAI => crate::convert::stream::chunk_to_openai(&bytes, &model),
                        OutputFormat::Anthropic => crate::convert::stream::chunk_to_anthropic(&bytes, &model),
                    };
                    let event = Event::default().data(String::from_utf8_lossy(&converted).to_string());
                    Some((Ok(event), (stream, model, format)))
                }
                _ => None,
            }
        },
    );
    Sse::new(mapped).keep_alive(KeepAlive::default())
}
```

- [ ] **Step 3: Commit**

```bash
git add src/convert/stream.rs src/util/stream.rs
git commit -m "feat: add SSE stream conversion helpers"
```

---

### Task 15: Wire Full Request Flow in Route Handlers

**Files:**
- Modify: `src/routes/openai.rs`
- Modify: `src/routes/anthropic.rs`

- [ ] **Step 1: Implement full `/v1/chat/completions` handler**

Logic:
1. Parse `OpenAIChatRequest` from body
2. Extract model, resolve provider via `router::resolve_provider`
3. If `provider.supports_passthrough(OpenAI)` → call `provider.passthrough()`
4. Else → `from_openai()` → `provider.chat()` → `to_openai()`
5. If streaming → return SSE response via `util::stream::into_sse_response`
6. If not streaming → return JSON

- [ ] **Step 2: Implement full `/v1/messages` handler**

Same flow but for Anthropic format:
1. Parse `AnthropicMessagesRequest`
2. Resolve provider
3. Passthrough check or convert
4. Streaming or JSON response

- [ ] **Step 3: Manual test with Copilot provider**

Run: `cargo run -- start --foreground`

Test OpenAI format:
```bash
curl http://127.0.0.1:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Say hello"}],"stream":false}'
```

Test Anthropic format:
```bash
curl http://127.0.0.1:9090/v1/messages \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","messages":[{"role":"user","content":"Say hello"}],"max_tokens":100,"stream":false}'
```

Expected: Both return valid responses from Copilot upstream

- [ ] **Step 4: Test streaming**

```bash
curl http://127.0.0.1:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Say hello"}],"stream":true}'
```

Expected: SSE stream of chunks

- [ ] **Step 5: Commit**

```bash
git add src/routes/
git commit -m "feat: wire full request flow through route handlers"
```

---

## Phase 6: Control Socket & Runtime Commands

### Task 16: Unix Socket Control Server

**Files:**
- Create: `src/daemon/control.rs`
- Modify: `src/cli/status.rs`
- Modify: `src/cli/config_cmd.rs`
- Modify: `src/cli/models.rs`
- Modify: `src/cli/provider_cmd.rs`

- [ ] **Step 1: Implement `src/daemon/control.rs`**

JSON-RPC server over Unix socket using axum + hyper-util:
- Method `status` → return uptime, provider health, connections
- Method `config.show` → return current config
- Method `config.set` → hot-update config via ArcSwap
- Method `config.reload` → re-read config.toml
- Method `models` → list models from all providers
- Method `provider.enable` / `provider.disable` → toggle provider
- Method `logs.stream` → stream recent log lines

Start the control server alongside the HTTP server in the daemon startup.

- [ ] **Step 2: Implement CLI client side** (`src/cli/status.rs`, etc.)

Each CLI command connects to the Unix socket, sends a JSON-RPC request, prints the response:
```rust
pub async fn run() -> Result<()> {
    let socket_path = crate::util::xdg::socket_path();
    let stream = tokio::net::UnixStream::connect(&socket_path).await?;
    // Send JSON-RPC request, read response, print
    ...
}
```

- [ ] **Step 3: Wire remaining CLI commands in main.rs**

Update the match arms for Status, Config, Models, Provider, Logs.

- [ ] **Step 4: Test control socket**

Run daemon: `cargo run -- start --foreground`
In another terminal:
```bash
cargo run -- status
cargo run -- models
cargo run -- config show
```
Expected: Returns daemon info

- [ ] **Step 5: Commit**

```bash
git add src/daemon/control.rs src/cli/
git commit -m "feat: add Unix socket control server and CLI commands"
```

---

## Phase 7: Kiro Auth & Provider

### Task 17: Kiro Authentication (All Three Flows)

**Files:**
- Create: `src/auth/kiro.rs`

- [ ] **Step 1: Implement `src/auth/kiro.rs` — Builder ID device flow (Option A)**

Implement AWS Builder ID device code flow (spec Section 4.2, Option A — recommended, no browser callback):
1. Register OIDC client at `https://oidc.{region}.amazonaws.com/client/register`
2. Start device authorization at `https://oidc.{region}.amazonaws.com/device_authorization`
3. Display user_code, open browser to verificationUriComplete
4. Poll `https://oidc.{region}.amazonaws.com/token` with device_code grant type
5. Save tokens (accessToken, refreshToken, clientId, clientSecret) via TokenStore

```rust
pub async fn authenticate_builder_id(store: &dyn TokenStore, region: &str) -> Result<()>
```

- [ ] **Step 2: Implement Google social auth (Option B)**

PKCE + localhost callback flow (spec Section 4.2, Option B):
1. Generate PKCE code_verifier + code_challenge
2. Open browser to `https://prod.{region}.auth.desktop.kiro.dev/socialAuth` with `provider=Google`
3. Start a local HTTP listener on a random port for the OAuth callback
4. Exchange authorization code for tokens at `https://prod.{region}.auth.desktop.kiro.dev/exchangeToken`
5. Save with `auth_method: "google"`

```rust
pub async fn authenticate_social(store: &dyn TokenStore, region: &str, provider: &str) -> Result<()>
```

- [ ] **Step 3: Implement GitHub social auth (Option C)**

Same PKCE + localhost callback flow as Option B but with `provider=GitHub`. Reuse the `authenticate_social` function with different provider parameter.

- [ ] **Step 4: Wire auth kiro in `src/cli/auth.rs`**

Present a menu to the user:
```
Select authentication method:
  1. AWS Builder ID (recommended — no browser callback)
  2. Google account
  3. GitHub account
```

Call the corresponding auth function based on selection.

- [ ] **Step 5: Test interactively**

Run: `cargo run -- auth kiro`
Expected: Shows auth method menu, proceeds with selected flow

- [ ] **Step 6: Commit**

```bash
git add src/auth/kiro.rs src/cli/auth.rs
git commit -m "feat: implement Kiro auth (Builder ID, Google, GitHub)"
```

---

### Task 18: Kiro Provider (CodeWhisperer Client)

**Files:**
- Create: `src/providers/kiro/mod.rs`
- Create: `src/providers/kiro/client.rs`
- Create: `src/providers/kiro/models.rs`
- Create: `src/providers/kiro/cw_types.rs`

- [ ] **Step 1: Implement `src/providers/kiro/cw_types.rs`**

CodeWhisperer request/response structs per spec Section 4.2.1:
- `CWGenerateRequest` with `conversationState` containing `currentMessage`, `history`, `chatTriggerType`
- Streaming response event types

- [ ] **Step 2: Implement `src/providers/kiro/client.rs`**

POST to `https://q.{region}.amazonaws.com/generateAssistantResponse` with required headers (x-amzn-kiro-agent-mode, amz-sdk-invocation-id, etc.).

- [ ] **Step 3: Implement `src/providers/kiro/models.rs`**

Kiro model listing — may be hardcoded or fetched from an API endpoint.

- [ ] **Step 4: Implement `src/providers/kiro/mod.rs`**

```rust
pub struct KiroProvider { ... }

#[async_trait]
impl Provider for KiroProvider {
    fn name(&self) -> &str { "kiro" }
    fn supports_passthrough(&self, _: OutputFormat) -> bool { false } // Always convert
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse> {
        // Convert ProviderRequest → CW conversationState format
        // Send to CodeWhisperer API
        // Parse streaming response → ProviderChunk events
    }
    ...
}
```

Include token refresh loop similar to Copilot.

- [ ] **Step 5: Register Kiro provider in daemon startup**

Update `src/cli/start.rs` to also initialize Kiro provider if configured and tokens exist.

- [ ] **Step 6: Test with real Kiro auth**

Run: `cargo run -- start --foreground`
```bash
curl http://127.0.0.1:9090/v1/chat/completions \
  -d '{"model":"kiro/claude-sonnet-4","messages":[{"role":"user","content":"hello"}]}'
```

- [ ] **Step 7: Commit**

```bash
git add src/providers/kiro/
git commit -m "feat: implement Kiro provider (CodeWhisperer client)"
```

---

## Phase 8: Middleware & Polish

### Task 19: Auth Middleware & Rate Limiting

**Files:**
- Modify: `src/server/middleware.rs`
- Modify: `src/server/mod.rs`

- [ ] **Step 1: Implement bearer token auth middleware**

If `config.api_key` is non-empty, validate `Authorization: Bearer <key>` header on all `/v1/*` routes. Return 401 if missing/invalid.

- [ ] **Step 2: Implement rate limiting middleware**

Simple token-bucket per IP using `Arc<RwLock<HashMap<IpAddr, Instant>>>`. If `rate_limit_seconds > 0`, reject requests faster than the limit with 429.

- [ ] **Step 3: Wire middlewares into router**

Add auth and rate-limit layers to the `/v1/*` routes in `build_router()`.

- [ ] **Step 4: Test auth middleware**

Set `api_key = "test123"` in config, verify:
- Request without key → 401
- Request with wrong key → 401
- Request with correct key → 200

- [ ] **Step 5: Commit**

```bash
git add src/server/
git commit -m "feat: add auth and rate-limit middleware"
```

---

### Task 20: Error Format Matching & Logs Command

**Files:**
- Modify: `src/util/error.rs`
- Modify: `src/cli/logs.rs`

- [ ] **Step 1: Make errors match endpoint format**

OpenAI errors: `{ "error": { "message": "...", "type": "...", "code": "..." } }`
Anthropic errors: `{ "type": "error", "error": { "type": "...", "message": "..." } }`

Add a `format` field to AppError or use the request path to determine format.

- [ ] **Step 2: Implement `src/cli/logs.rs`**

Tail the daemon log file, with `--lines` and `--level` filtering. Can read directly from the log file (no socket needed for basic implementation).

- [ ] **Step 3: Commit**

```bash
git add src/util/error.rs src/cli/logs.rs
git commit -m "feat: format-aware error responses and logs command"
```

---

### Task 21: SIGHUP Config Reload & Hot Update

**Files:**
- Modify: `src/cli/start.rs`

- [ ] **Step 1: Add SIGHUP handler for config reload**

In the daemon startup, install a SIGHUP handler that re-reads `config.toml` and updates the `ArcSwap<Config>`:

```rust
let config_arc = state.config.clone();
tokio::spawn(async move {
    let mut sighup = tokio::signal::unix::signal(SignalKind::hangup()).unwrap();
    loop {
        sighup.recv().await;
        tracing::info!("Received SIGHUP, reloading config...");
        match crate::config::load_default_config() {
            Ok(new_config) => {
                config_arc.store(Arc::new(new_config));
                tracing::info!("Config reloaded");
            }
            Err(e) => tracing::error!("Config reload failed: {}", e),
        }
    }
});
```

- [ ] **Step 2: Verify hot reload**

1. Start daemon
2. Change `config.toml` (e.g., change `default_provider`)
3. Send `kill -HUP $(cat ~/.config/aiclient-api/daemon.pid)` (or use `config reload` CLI)
4. Verify new config is active via `config show`

- [ ] **Step 3: Commit**

```bash
git add src/cli/start.rs
git commit -m "feat: add SIGHUP config hot-reload"
```

---

## Phase 9: Integration Testing & Final Polish

### Task 22: End-to-End Integration Test

**Files:**
- Create: `tests/integration_test.rs`

- [ ] **Step 1: Write integration test**

Start a test server, register a mock provider, send requests through both endpoints:

```rust
#[tokio::test]
async fn test_openai_endpoint_with_mock_provider() {
    // Setup AppState with a mock provider
    // Send POST /v1/chat/completions
    // Assert response is valid OpenAI format
}

#[tokio::test]
async fn test_anthropic_endpoint_with_mock_provider() {
    // Setup AppState with a mock provider
    // Send POST /v1/messages
    // Assert response is valid Anthropic format
}

#[tokio::test]
async fn test_model_routing_with_prefix() {
    // Register two mock providers
    // Send request with "provider_a/model" prefix
    // Assert it was routed to provider_a
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test integration_test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add end-to-end integration tests"
```

---

### Task 23: README & Final Cleanup

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Write README.md**

Include: what it is, quick start (auth + start), configuration, supported providers, API endpoints.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`
Fix any warnings.

- [ ] **Step 4: Final commit**

```bash
git add README.md
git commit -m "docs: add README with quick start guide"
```

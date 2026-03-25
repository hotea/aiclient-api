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
        ServerConfig { host: default_host(), port: default_port(), rate_limit_seconds: 0 }
    }
}

impl Default for AccountType {
    fn default() -> Self { AccountType::Individual }
}

impl Default for LogConfig {
    fn default() -> Self { LogConfig { level: default_log_level(), file: String::new() } }
}

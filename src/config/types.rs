use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    OpenAI,
    Anthropic,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    #[default]
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
    #[serde(default = "default_provider_configs")]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub routing: RoutingConfig,
    #[serde(default)]
    pub logging: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderRoutingMode {
    Auto,
    Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutingConfig {
    #[serde(default = "default_provider_routing_mode")]
    pub mode: ProviderRoutingMode,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub weights: HashMap<String, u32>,
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
    #[serde(rename = "common")]
    Common {
        #[serde(default = "default_true")]
        enabled: bool,
        /// Provider base URL, for example `https://integrate.api.nvidia.com/v1`.
        /// A full `/chat/completions` URL is also accepted.
        base_url: String,
        #[serde(default)]
        api_key: String,
        #[serde(default)]
        api_keys: Vec<String>,
        #[serde(default)]
        api_key_env: String,
        #[serde(default)]
        api_key_envs: Vec<String>,
        #[serde(default = "default_auth_scheme")]
        auth_scheme: String,
        #[serde(default = "default_chat_completions_path")]
        chat_completions_path: String,
        #[serde(default = "default_models_path")]
        models_path: String,
        #[serde(default)]
        models: Vec<String>,
        #[serde(default = "default_common_vendor")]
        vendor: String,
        #[serde(default = "default_true")]
        supports_streaming: bool,
        #[serde(default)]
        supports_tools: bool,
        #[serde(default)]
        supports_vision: bool,
        #[serde(default)]
        supports_thinking: bool,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

impl ProviderConfig {
    pub fn is_enabled(&self) -> bool {
        match self {
            ProviderConfig::Copilot { enabled, .. }
            | ProviderConfig::Kiro { enabled, .. }
            | ProviderConfig::Common { enabled, .. } => *enabled,
        }
    }

    pub fn set_enabled(&mut self, value: bool) {
        match self {
            ProviderConfig::Copilot { enabled, .. }
            | ProviderConfig::Kiro { enabled, .. }
            | ProviderConfig::Common { enabled, .. } => *enabled = value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub file: String,
}

fn default_format() -> Format {
    Format::OpenAI
}
fn default_provider() -> String {
    "copilot".to_string()
}
fn default_vscode_version() -> String {
    "1.110.1".to_string()
}
fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    9090
}
fn default_true() -> bool {
    true
}
fn default_region() -> String {
    "us-east-1".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_auth_scheme() -> String {
    "Bearer".to_string()
}
fn default_chat_completions_path() -> String {
    "/chat/completions".to_string()
}
fn default_models_path() -> String {
    "/models".to_string()
}
fn default_common_vendor() -> String {
    "openai-compatible".to_string()
}
fn default_provider_routing_mode() -> ProviderRoutingMode {
    ProviderRoutingMode::Auto
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_format: default_format(),
            default_provider: default_provider(),
            api_key: String::new(),
            vscode_version: default_vscode_version(),
            server: ServerConfig::default(),
            providers: default_provider_configs(),
            routing: RoutingConfig::default(),
            logging: LogConfig::default(),
        }
    }
}

impl Config {
    pub fn with_default_providers(mut self) -> Self {
        for (name, provider) in default_provider_configs() {
            self.providers.entry(name).or_insert(provider);
        }
        self
    }
}

pub fn default_provider_configs() -> HashMap<String, ProviderConfig> {
    let mut providers = HashMap::new();
    providers.insert(
        "copilot".to_string(),
        ProviderConfig::Copilot {
            enabled: true,
            account_type: AccountType::Individual,
            enterprise_url: None,
        },
    );
    providers.insert(
        "kiro".to_string(),
        ProviderConfig::Kiro {
            enabled: true,
            region: default_region(),
            idc_region: None,
        },
    );
    providers.insert(
        "opencode".to_string(),
        common_provider_template(
            "https://opencode.ai/zen/v1",
            "OPENCODE_API_KEY",
            "opencode",
            vec!["nemotron-3-ultra-free".to_string()],
        ),
    );
    providers.insert(
        "nvidia".to_string(),
        common_provider_template(
            "https://integrate.api.nvidia.com/v1",
            "NVIDIA_API_KEY",
            "nvidia",
            vec!["nemotron-3-ultra-free".to_string()],
        ),
    );
    providers.insert(
        "aihubmix".to_string(),
        common_provider_template(
            "https://aihubmix.com/v1/chat/completions",
            "AIHUBMIX_API_KEY",
            "aihubmix",
            vec!["gpt-4.1-free".to_string()],
        ),
    );
    providers.insert(
        "openrouter".to_string(),
        common_provider_template(
            "https://openrouter.ai/api/v1",
            "OPENROUTER_API_KEY",
            "openrouter",
            vec!["nvidia/nemotron-3-ultra-550b-a55b:free".to_string()],
        ),
    );
    providers
}

fn common_provider_template(
    base_url: &str,
    api_key_env: &str,
    vendor: &str,
    models: Vec<String>,
) -> ProviderConfig {
    ProviderConfig::Common {
        enabled: false,
        base_url: base_url.to_string(),
        api_key: String::new(),
        api_keys: Vec::new(),
        api_key_env: api_key_env.to_string(),
        api_key_envs: Vec::new(),
        auth_scheme: default_auth_scheme(),
        chat_completions_path: default_chat_completions_path(),
        models_path: default_models_path(),
        models,
        vendor: vendor.to_string(),
        supports_streaming: true,
        supports_tools: false,
        supports_vision: false,
        supports_thinking: false,
        headers: HashMap::new(),
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

impl Default for RoutingConfig {
    fn default() -> Self {
        RoutingConfig {
            mode: default_provider_routing_mode(),
            provider: String::new(),
            weights: HashMap::new(),
        }
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

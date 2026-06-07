use aiclient_api::config::types::*;

#[test]
fn test_deserialize_full_config() {
    let toml_str = r#"
default_format = "openai"
default_provider = "copilot"
api_key = ""
auth_enabled = false
vscode_version = "1.110.1"

[routing]
mode = "auto"
provider = "copilot"
models = ["auto", "chat"]

[routing.weights]
opencode = 2
aihubmix = 1

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

[providers.opencode]
type = "common"
enabled = true
base_url = "https://opencode.ai/zen/v1"
api_key_env = "OPENCODE_API_KEY"
models = ["nemotron-3-ultra-free"]
vendor = "opencode"

[providers.aihubmix]
type = "common"
enabled = true
base_url = "https://aihubmix.com/v1/chat/completions"
api_key_env = "AIHUBMIX_API_KEY"
models = ["gpt-4.1-free"]
vendor = "aihubmix"

[logging]
level = "info"
file = ""
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.default_provider, "copilot");
    assert!(!config.auth_enabled);
    assert_eq!(config.routing.mode, ProviderRoutingMode::Auto);
    assert_eq!(config.routing.provider, "copilot");
    assert_eq!(config.routing.models, vec!["auto", "chat"]);
    assert_eq!(config.routing.weights["opencode"], 2);
    assert_eq!(config.routing.weights["aihubmix"], 1);
    assert_eq!(config.server.port, 9090);
    assert!(config.providers.contains_key("copilot"));
    assert!(config.providers.contains_key("kiro"));
    assert!(config.providers.contains_key("opencode"));
    assert!(config.providers.contains_key("aihubmix"));
}

#[test]
fn test_default_config_is_valid() {
    let config = Config::default();
    assert_eq!(config.server.port, 9090);
    assert_eq!(config.default_format, Format::OpenAI);
    assert!(!config.auth_enabled);
    assert_eq!(config.routing.mode, ProviderRoutingMode::Auto);
    assert!(config.routing.provider.is_empty());
    assert_eq!(config.routing.models, vec!["auto"]);
    assert!(config.routing.weights.is_empty());
    assert!(config.providers.contains_key("copilot"));
    assert!(config.providers.contains_key("kiro"));
    assert!(config.providers.contains_key("opencode"));
    assert!(config.providers.contains_key("nvidia"));
    assert!(config.providers.contains_key("aihubmix"));
    assert!(config.providers.contains_key("openrouter"));
    match &config.providers["aihubmix"] {
        ProviderConfig::Common {
            enabled,
            base_url,
            api_key_env,
            models,
            ..
        } => {
            assert!(!enabled);
            assert_eq!(base_url, "https://aihubmix.com/v1/chat/completions");
            assert_eq!(api_key_env, "AIHUBMIX_API_KEY");
            assert_eq!(models, &vec!["gpt-4.1-free".to_string()]);
        }
        _ => panic!("Expected default aihubmix common provider"),
    }
    match &config.providers["openrouter"] {
        ProviderConfig::Common {
            enabled,
            base_url,
            api_key_env,
            models,
            ..
        } => {
            assert!(!enabled);
            assert_eq!(base_url, "https://openrouter.ai/api/v1");
            assert_eq!(api_key_env, "OPENROUTER_API_KEY");
            assert_eq!(
                models,
                &vec!["nvidia/nemotron-3-ultra-550b-a55b:free".to_string()]
            );
        }
        _ => panic!("Expected default openrouter common provider"),
    }
}

#[test]
fn test_provider_config_common_discriminant() {
    let toml_str = r#"
type = "common"
enabled = true
base_url = "https://integrate.api.nvidia.com/v1"
api_key_env = "NVIDIA_API_KEY"
api_key_envs = ["NVIDIA_API_KEY_1", "NVIDIA_API_KEY_2"]
api_keys = ["direct-key-1", "direct-key-2"]
models = ["nemotron-3-ultra-free"]
vendor = "nvidia"
supports_tools = true

[headers]
"X-Custom-Header" = "custom-value"
"#;
    let pc: ProviderConfig = toml::from_str(toml_str).unwrap();
    match pc {
        ProviderConfig::Common {
            base_url,
            api_key_env,
            api_key_envs,
            api_keys,
            models,
            vendor,
            supports_streaming,
            supports_tools,
            headers,
            ..
        } => {
            assert_eq!(base_url, "https://integrate.api.nvidia.com/v1");
            assert_eq!(api_key_env, "NVIDIA_API_KEY");
            assert_eq!(api_key_envs, vec!["NVIDIA_API_KEY_1", "NVIDIA_API_KEY_2"]);
            assert_eq!(api_keys, vec!["direct-key-1", "direct-key-2"]);
            assert_eq!(models, vec!["nemotron-3-ultra-free"]);
            assert_eq!(vendor, "nvidia");
            assert!(supports_streaming);
            assert!(supports_tools);
            assert_eq!(headers["X-Custom-Header"], "custom-value");
        }
        _ => panic!("Expected Common variant"),
    }
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

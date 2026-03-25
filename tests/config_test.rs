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

pub mod types;

use anyhow::{Context, Result};
use std::path::Path;
use types::Config;

pub fn load_config(path: &Path) -> Result<Config> {
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {}", path.display()))?;
        Ok(config.with_default_providers())
    } else {
        Ok(Config::default())
    }
}

pub fn load_default_config() -> Result<Config> {
    let path = crate::util::xdg::config_dir().join("config.toml");
    load_config(&path)
}

pub fn resolve_env_var(name: &str) -> Result<String> {
    if let Ok(value) = std::env::var(name) {
        return Ok(value);
    }

    for path in dotenv_paths() {
        if let Some(value) = read_dotenv_var(&path, name)? {
            return Ok(value);
        }
    }

    anyhow::bail!(
        "Environment variable '{}' is not set and was not found in .env files",
        name
    )
}

fn dotenv_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".env"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(std::path::PathBuf::from(home).join(".aiclient-api/.env"));
    }
    paths.push(crate::util::xdg::config_dir().join(".env"));
    paths
}

fn read_dotenv_var(path: &Path, name: &str) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read .env file: {}", path.display()))?;
    Ok(parse_dotenv_var(&content, name))
}

fn parse_dotenv_var(content: &str, name: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != name {
            continue;
        }

        return Some(parse_dotenv_value(value));
    }

    None
}

fn parse_dotenv_value(value: &str) -> String {
    let value = strip_inline_comment(value.trim()).trim();
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn strip_inline_comment(value: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in value.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => {
                if idx == 0 || value[..idx].ends_with(char::is_whitespace) {
                    return &value[..idx];
                }
            }
            _ => {}
        }
    }

    value
}

#[cfg(test)]
mod tests {
    use super::parse_dotenv_var;

    #[test]
    fn parses_dotenv_values() {
        let content = r#"
# comment
PLAIN=value
QUOTED="quoted value"
SINGLE='single value'
export EXPORTED=from-export
INLINE=value # comment
HASH=sk-test#not-comment
"#;

        assert_eq!(parse_dotenv_var(content, "PLAIN").as_deref(), Some("value"));
        assert_eq!(
            parse_dotenv_var(content, "QUOTED").as_deref(),
            Some("quoted value")
        );
        assert_eq!(
            parse_dotenv_var(content, "SINGLE").as_deref(),
            Some("single value")
        );
        assert_eq!(
            parse_dotenv_var(content, "EXPORTED").as_deref(),
            Some("from-export")
        );
        assert_eq!(
            parse_dotenv_var(content, "INLINE").as_deref(),
            Some("value")
        );
        assert_eq!(
            parse_dotenv_var(content, "HASH").as_deref(),
            Some("sk-test#not-comment")
        );
    }

    #[test]
    fn dotenv_paths_prefer_project_then_home() {
        let paths = super::dotenv_paths();
        assert!(paths
            .first()
            .and_then(|path| path.file_name())
            .is_some_and(|name| name == ".env"));
        assert!(paths
            .iter()
            .any(|path| path.ends_with(".aiclient-api/.env")));
    }
}

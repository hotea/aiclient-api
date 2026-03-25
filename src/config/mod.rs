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
        Ok(config)
    } else {
        Ok(Config::default())
    }
}

pub fn load_default_config() -> Result<Config> {
    let path = crate::util::xdg::config_dir().join("config.toml");
    load_config(&path)
}

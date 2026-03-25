use super::Provider;
use crate::server::state::AppState;
use anyhow::{bail, Result};
use std::sync::Arc;

pub async fn resolve_provider(
    state: &AppState,
    model: &str,
    header_provider: Option<&str>,
) -> Result<(Arc<dyn Provider>, String)> {
    let providers = state.providers.read().await;
    let config = state.config.load();

    if let Some((prefix, actual_model)) = model.split_once('/') {
        if let Some(provider) = providers.get(prefix) {
            return Ok((provider.clone(), actual_model.to_string()));
        }
        bail!("Provider '{}' not found", prefix);
    }

    if let Some(name) = header_provider {
        if let Some(provider) = providers.get(name) {
            return Ok((provider.clone(), model.to_string()));
        }
        bail!("Provider '{}' not found", name);
    }

    let default_name = &config.default_provider;
    if let Some(provider) = providers.get(default_name.as_str()) {
        return Ok((provider.clone(), model.to_string()));
    }

    bail!("No provider available for model '{}'", model)
}

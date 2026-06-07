use super::Provider;
use crate::config::types::{Config, ProviderRoutingMode};
use crate::server::state::AppState;
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub async fn resolve_provider(
    state: &AppState,
    model: &str,
    header_provider: Option<&str>,
) -> Result<(Arc<dyn Provider>, String)> {
    let providers = state.providers.read().await;
    let config = state.config.load();

    match &config.routing.mode {
        ProviderRoutingMode::Fixed => {
            let fixed_name = if config.routing.provider.trim().is_empty() {
                config.default_provider.as_str()
            } else {
                config.routing.provider.as_str()
            };
            let (model_provider, actual_model) = split_model_provider(model);
            if let Some(prefix) = model_provider {
                ensure_fixed_provider(prefix, fixed_name)?;
            }
            if let Some(name) = header_provider {
                ensure_fixed_provider(name, fixed_name)?;
            }
            if let Some(provider) = providers.get(fixed_name) {
                let resolved_model =
                    resolve_generic_model(provider, actual_model, config.as_ref())?;
                return Ok((provider.clone(), resolved_model));
            }

            bail!("Fixed provider '{}' not found", fixed_name);
        }
        ProviderRoutingMode::Auto => {
            if let Some((prefix, actual_model)) = model.split_once('/') {
                if let Some(provider) = providers.get(prefix) {
                    return Ok((provider.clone(), actual_model.to_string()));
                }
                bail!("Provider '{}' not found", prefix);
            }

            if let Some(name) = header_provider {
                if let Some(provider) = providers.get(name) {
                    let resolved_model = resolve_generic_model(provider, model, config.as_ref())?;
                    return Ok((provider.clone(), resolved_model));
                }
                bail!("Provider '{}' not found", name);
            }

            ensure_auto_model(model, config.as_ref())?;
            let provider = resolve_auto_provider(state, &providers, config.as_ref())?;
            let resolved_model = resolve_generic_model(&provider, model, config.as_ref())?;
            Ok((provider, resolved_model))
        }
    }
}

fn split_model_provider(model: &str) -> (Option<&str>, &str) {
    match model.split_once('/') {
        Some((prefix, actual_model)) => (Some(prefix), actual_model),
        None => (None, model),
    }
}

fn ensure_fixed_provider(requested: &str, fixed_name: &str) -> Result<()> {
    if requested == fixed_name {
        Ok(())
    } else {
        bail!(
            "Provider '{}' is not allowed in fixed routing mode; configured provider is '{}'",
            requested,
            fixed_name
        )
    }
}

fn is_generic_model(model: &str, config: &Config) -> bool {
    config
        .routing
        .models
        .iter()
        .any(|generic_model| generic_model == model)
}

fn ensure_auto_model(model: &str, config: &Config) -> Result<()> {
    if is_generic_model(model, config) {
        return Ok(());
    }

    bail!(
        "Automatic routing requires a generic model name. Use one of: {}",
        config.routing.models.join(", ")
    )
}

fn resolve_generic_model(
    provider: &Arc<dyn Provider>,
    model: &str,
    config: &Config,
) -> Result<String> {
    if !is_generic_model(model, config) {
        return Ok(model.to_string());
    }

    provider.default_model().ok_or_else(|| {
        anyhow::anyhow!(
            "Provider '{}' does not have a default model for generic model '{}'",
            provider.name(),
            model
        )
    })
}

fn resolve_auto_provider(
    state: &AppState,
    providers: &HashMap<String, Arc<dyn Provider>>,
    config: &Config,
) -> Result<Arc<dyn Provider>> {
    let mut candidates: Vec<(&str, u32)> = providers
        .iter()
        .filter_map(|(name, provider)| {
            if !provider.is_healthy() {
                return None;
            }
            let weight = config.routing.weights.get(name).copied().unwrap_or(1);
            (weight > 0).then_some((name.as_str(), weight))
        })
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(b.0));

    let total_weight: usize = candidates.iter().map(|(_, weight)| *weight as usize).sum();
    if total_weight == 0 {
        bail!("No healthy provider available for automatic routing");
    }

    let mut slot = state.routing_counter.fetch_add(1, Ordering::Relaxed) % total_weight;
    for (name, weight) in candidates {
        let weight = weight as usize;
        if slot < weight {
            return providers
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Provider '{}' disappeared during routing", name));
        }
        slot -= weight;
    }

    bail!("No healthy provider available for automatic routing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Model, OutputFormat, ProviderRequest, ProviderResponse};
    use async_trait::async_trait;

    struct MockProvider {
        name: String,
        healthy: bool,
    }

    impl MockProvider {
        fn new(name: &str) -> Arc<Self> {
            Arc::new(Self {
                name: name.to_string(),
                healthy: true,
            })
        }

        fn unhealthy(name: &str) -> Arc<Self> {
            Arc::new(Self {
                name: name.to_string(),
                healthy: false,
            })
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        fn is_healthy(&self) -> bool {
            self.healthy
        }

        fn default_model(&self) -> Option<String> {
            Some(format!("{}-default", self.name))
        }

        async fn list_models(&self) -> Result<Vec<Model>> {
            Ok(Vec::new())
        }

        async fn chat(&self, _request: ProviderRequest) -> Result<ProviderResponse> {
            anyhow::bail!("not used")
        }

        fn supports_passthrough(&self, _format: OutputFormat) -> bool {
            false
        }
    }

    async fn state_with_providers(config: Config, names: &[&str]) -> AppState {
        let state = AppState::new(config);
        {
            let mut providers = state.providers.write().await;
            for name in names {
                providers.insert(name.to_string(), MockProvider::new(name));
            }
        }
        state
    }

    #[tokio::test]
    async fn auto_routing_rotates_loaded_providers_by_default() {
        let state = state_with_providers(Config::default(), &["beta", "alpha"]).await;

        let (first, first_model) = resolve_provider(&state, "auto", None).await.unwrap();
        let (second, second_model) = resolve_provider(&state, "auto", None).await.unwrap();
        let (third, third_model) = resolve_provider(&state, "auto", None).await.unwrap();

        assert_eq!(first.name(), "alpha");
        assert_eq!(first_model, "alpha-default");
        assert_eq!(second.name(), "beta");
        assert_eq!(second_model, "beta-default");
        assert_eq!(third.name(), "alpha");
        assert_eq!(third_model, "alpha-default");
    }

    #[tokio::test]
    async fn auto_routing_honors_provider_weights() {
        let mut config = Config::default();
        config.routing.weights.insert("alpha".to_string(), 2);
        config.routing.weights.insert("beta".to_string(), 1);
        let state = state_with_providers(config, &["alpha", "beta"]).await;

        let mut selected = Vec::new();
        for _ in 0..4 {
            let (provider, _) = resolve_provider(&state, "auto", None).await.unwrap();
            selected.push(provider.name().to_string());
        }

        assert_eq!(selected, ["alpha", "alpha", "beta", "alpha"]);
    }

    #[tokio::test]
    async fn fixed_routing_uses_configured_provider() {
        let mut config = Config::default();
        config.routing.mode = ProviderRoutingMode::Fixed;
        config.routing.provider = "beta".to_string();
        let state = state_with_providers(config, &["alpha", "beta"]).await;

        let (provider, model) = resolve_provider(&state, "auto", None).await.unwrap();

        assert_eq!(provider.name(), "beta");
        assert_eq!(model, "beta-default");
    }

    #[tokio::test]
    async fn fixed_routing_accepts_matching_provider_prefix() {
        let mut config = Config::default();
        config.routing.mode = ProviderRoutingMode::Fixed;
        config.routing.provider = "beta".to_string();
        let state = state_with_providers(config, &["alpha", "beta"]).await;

        let (provider, model) = resolve_provider(&state, "beta/remote-model", None)
            .await
            .unwrap();

        assert_eq!(provider.name(), "beta");
        assert_eq!(model, "remote-model");
    }

    #[tokio::test]
    async fn fixed_routing_rejects_other_provider_prefix() {
        let mut config = Config::default();
        config.routing.mode = ProviderRoutingMode::Fixed;
        config.routing.provider = "beta".to_string();
        let state = state_with_providers(config, &["alpha", "beta"]).await;

        let err = match resolve_provider(&state, "alpha/remote-model", None).await {
            Ok((provider, _)) => panic!("unexpected provider: {}", provider.name()),
            Err(err) => err,
        };

        assert!(err.to_string().contains("fixed routing mode"));
    }

    #[tokio::test]
    async fn explicit_provider_prefix_overrides_auto_routing_mode() {
        let mut config = Config::default();
        config.routing.weights.insert("alpha".to_string(), 100);
        let state = state_with_providers(config, &["alpha", "beta"]).await;

        let (provider, model) = resolve_provider(&state, "beta/remote-model", None)
            .await
            .unwrap();

        assert_eq!(provider.name(), "beta");
        assert_eq!(model, "remote-model");
    }

    #[tokio::test]
    async fn auto_routing_skips_unhealthy_providers() {
        let state = AppState::new(Config::default());
        {
            let mut providers = state.providers.write().await;
            providers.insert("alpha".to_string(), MockProvider::unhealthy("alpha"));
            providers.insert("beta".to_string(), MockProvider::new("beta"));
        }

        let (provider, model) = resolve_provider(&state, "auto", None).await.unwrap();

        assert_eq!(provider.name(), "beta");
        assert_eq!(model, "beta-default");
    }

    #[tokio::test]
    async fn auto_routing_rejects_provider_specific_unprefixed_models() {
        let state = state_with_providers(Config::default(), &["alpha", "beta"]).await;

        let err = match resolve_provider(&state, "provider-specific-model", None).await {
            Ok((provider, _)) => panic!("unexpected provider: {}", provider.name()),
            Err(err) => err,
        };

        assert!(err.to_string().contains("generic model name"));
    }
}

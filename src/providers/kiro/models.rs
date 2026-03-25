use crate::providers::Model;

pub fn kiro_models() -> Vec<Model> {
    let models = vec![
        ("claude-sonnet-4-6", "claude-sonnet-4.6", "Anthropic"),
        ("claude-opus-4-6", "claude-opus-4.6", "Anthropic"),
        ("claude-sonnet-4-5", "claude-sonnet-4.5", "Anthropic"),
        ("claude-opus-4-5", "claude-opus-4.5", "Anthropic"),
        ("claude-haiku-4-5", "claude-haiku-4.5", "Anthropic"),
        ("claude-sonnet-4-20250514", "claude-sonnet-4-20250514", "Anthropic"),
    ];

    models
        .into_iter()
        .map(|(internal_name, _display_name, vendor)| Model {
            id: format!("kiro/{}", internal_name),
            provider: "kiro".to_string(),
            vendor: vendor.to_string(),
            display_name: format!("{} (Kiro)", internal_name),
            max_input_tokens: None,
            max_output_tokens: None,
            supports_streaming: true,
            supports_tools: false,
            supports_vision: false,
            supports_thinking: false,
        })
        .collect()
}

/// Map model IDs from internal/display format to CodeWhisperer model format.
/// CodeWhisperer uses dot notation for versions (e.g. "claude-sonnet-4.6").
pub fn to_cw_model_id(model: &str) -> String {
    // Strip kiro/ prefix if present
    let model = model.strip_prefix("kiro/").unwrap_or(model);

    // Map dash-version notation to dot-version notation
    // e.g. "claude-sonnet-4-6" -> "claude-sonnet-4.6"
    // e.g. "claude-opus-4-5" -> "claude-opus-4.5"
    // e.g. "claude-haiku-4-5" -> "claude-haiku-4.5"
    // e.g. "claude-sonnet-4-20250514" -> stays as is (already uses dashes)
    let mapped = match model {
        "claude-sonnet-4-6" => "claude-sonnet-4.6",
        "claude-opus-4-6" => "claude-opus-4.6",
        "claude-sonnet-4-5" => "claude-sonnet-4.5",
        "claude-opus-4-5" => "claude-opus-4.5",
        "claude-haiku-4-5" => "claude-haiku-4.5",
        other => other,
    };

    mapped.to_string()
}

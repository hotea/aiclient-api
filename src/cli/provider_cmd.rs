use anyhow::Result;

use super::commands::ProviderAction;
use super::status::send_control_request;

pub async fn run(action: ProviderAction) -> Result<()> {
    match action {
        ProviderAction::Enable { name } => {
            let resp = send_control_request(serde_json::json!({
                "method": "provider.enable",
                "params": { "name": name }
            }))
            .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        ProviderAction::Disable { name } => {
            let resp = send_control_request(serde_json::json!({
                "method": "provider.disable",
                "params": { "name": name }
            }))
            .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
    }
    Ok(())
}

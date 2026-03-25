use anyhow::Result;

use super::commands::ConfigAction;
use super::status::send_control_request;

pub async fn run(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let resp = send_control_request(serde_json::json!({"method": "config.show"})).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        ConfigAction::Reload => {
            let resp = send_control_request(serde_json::json!({"method": "config.reload"})).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        ConfigAction::Set { key, value } => {
            eprintln!("Config set not yet implemented (key={:?}, value={:?})", key, value);
        }
    }
    Ok(())
}

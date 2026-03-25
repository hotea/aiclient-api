use anyhow::Result;

use super::status::send_control_request;

pub async fn run() -> Result<()> {
    let resp = send_control_request(serde_json::json!({"method": "models"})).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

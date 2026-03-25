use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn send_control_request(request: serde_json::Value) -> Result<serde_json::Value> {
    let socket_path = aiclient_api::util::xdg::socket_path();
    let mut stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .context("Failed to connect to daemon. Is it running?")?;

    let req_bytes = serde_json::to_vec(&request)?;
    // Write length prefix (4 bytes big-endian) then JSON
    stream.write_all(&(req_bytes.len() as u32).to_be_bytes()).await?;
    stream.write_all(&req_bytes).await?;
    stream.flush().await?;

    // Read response: length prefix then JSON
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut resp_buf = vec![0u8; len];
    stream.read_exact(&mut resp_buf).await?;

    Ok(serde_json::from_slice(&resp_buf)?)
}

pub async fn run() -> Result<()> {
    let resp = send_control_request(serde_json::json!({"method": "status"})).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

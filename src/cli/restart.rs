use anyhow::Result;

pub async fn run(
    host: String,
    port: u16,
    foreground: bool,
    api_key: Option<String>,
    log_file: Option<String>,
) -> Result<()> {
    let _ = aiclient_api::daemon::stop_daemon();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    super::start::run(host, port, foreground, api_key, log_file).await
}

use anyhow::Result;

pub async fn run(
    host: String,
    port: u16,
    foreground: bool,
    api_key: Option<String>,
    log_file: Option<String>,
) -> Result<()> {
    super::start::run(host, port, foreground, api_key, log_file).await
}

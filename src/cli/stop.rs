use anyhow::Result;

pub fn run() -> Result<()> {
    aiclient_api::daemon::stop_daemon()
}

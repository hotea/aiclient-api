use anyhow::Result;

pub async fn run(lines: usize, level: &str) -> Result<()> {
    let log_path = aiclient_api::util::xdg::log_path();

    if !log_path.exists() {
        eprintln!("Log file not found: {}", log_path.display());
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&log_path).await?;
    let all_lines: Vec<&str> = content.lines().collect();

    // Filter by log level
    let filtered: Vec<&str> = all_lines
        .iter()
        .filter(|line| {
            if level == "warn" || level == "warning" {
                line.contains("WARN") || line.contains("ERROR")
            } else if level == "error" {
                line.contains("ERROR")
            } else {
                // info, debug, and any other level: show all
                true
            }
        })
        .copied()
        .collect();

    let start = if filtered.len() > lines {
        filtered.len() - lines
    } else {
        0
    };

    for line in &filtered[start..] {
        println!("{}", line);
    }

    Ok(())
}

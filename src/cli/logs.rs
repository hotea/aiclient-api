use anyhow::Result;

pub async fn run(lines: usize, level: &str) -> Result<()> {
    let log_path = aiclient_api::util::xdg::log_path();

    if !log_path.exists() {
        eprintln!("Log file not found: {}", log_path.display());
        return Ok(());
    }

    let content = std::fs::read_to_string(&log_path)?;
    let all_lines: Vec<&str> = content.lines().collect();

    // Filter by log level
    let filtered: Vec<&str> = all_lines
        .iter()
        .filter(|line| {
            if level == "info" {
                // Show all levels (debug, info, warn, error)
                true
            } else if level == "warn" || level == "warning" {
                line.contains("WARN") || line.contains("ERROR")
            } else if level == "error" {
                line.contains("ERROR")
            } else if level == "debug" {
                true
            } else {
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

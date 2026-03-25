use anyhow::{Context, Result};
use std::path::Path;

/// Read PID from file and check if process is alive
pub fn read_pid() -> Result<Option<u32>> {
    let pid_path = crate::util::xdg::pid_path();
    if !pid_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&pid_path)?;
    let pid: u32 = content.trim().parse()?;

    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
    if alive {
        Ok(Some(pid))
    } else {
        let _ = std::fs::remove_file(&pid_path);
        Ok(None)
    }
}

/// Write PID to file
pub fn write_pid(pid: u32) -> Result<()> {
    let pid_path = crate::util::xdg::pid_path();
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_path, pid.to_string())?;
    Ok(())
}

/// Remove PID file
pub fn remove_pid() -> Result<()> {
    let pid_path = crate::util::xdg::pid_path();
    if pid_path.exists() {
        std::fs::remove_file(&pid_path)?;
    }
    Ok(())
}

/// Stop the daemon: send SIGTERM, wait up to 10s, then SIGKILL
pub fn stop_daemon() -> Result<()> {
    match read_pid()? {
        Some(pid) => {
            eprintln!("Stopping daemon (pid {})...", pid);
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }

            for _ in 0..100 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                if !alive {
                    remove_pid()?;
                    eprintln!("Daemon stopped.");
                    return Ok(());
                }
            }

            eprintln!("Daemon didn't stop gracefully, sending SIGKILL...");
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            remove_pid()?;
            eprintln!("Daemon killed.");
            Ok(())
        }
        None => {
            eprintln!("Daemon is not running.");
            Ok(())
        }
    }
}

/// Daemonize the current process (fork to background)
pub fn daemonize(log_file: &Path) -> Result<()> {
    use daemonize::Daemonize;

    let pid_path = crate::util::xdg::pid_path();
    let runtime_dir = crate::util::xdg::runtime_dir();
    std::fs::create_dir_all(&runtime_dir)?;

    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;
    let stderr = stdout.try_clone()?;

    let daemon = Daemonize::new()
        .pid_file(&pid_path)
        .working_directory(".")
        .stdout(stdout)
        .stderr(stderr);

    daemon.start().context("Failed to daemonize")?;

    Ok(())
}

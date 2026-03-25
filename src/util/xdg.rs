use std::path::PathBuf;

const APP_NAME: &str = "aiclient-api";

/// ~/.config/aiclient-api/
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join(APP_NAME)
}

/// $XDG_RUNTIME_DIR/aiclient-api/ or /tmp/aiclient-api-{uid}/
pub fn runtime_dir() -> PathBuf {
    if let Some(dir) = dirs::runtime_dir() {
        return dir.join(APP_NAME);
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/{}-{}", APP_NAME, uid))
}

/// ~/.local/state/aiclient-api/
pub fn state_dir() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".local/state")
        })
        .join(APP_NAME)
}

/// Socket path: $XDG_RUNTIME_DIR/aiclient-api/ctl.sock
pub fn socket_path() -> PathBuf {
    runtime_dir().join("ctl.sock")
}

/// PID file path
pub fn pid_path() -> PathBuf {
    runtime_dir().join("daemon.pid")
}

/// Default log file path
pub fn log_path() -> PathBuf {
    state_dir().join("daemon.log")
}

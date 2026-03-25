#[test]
fn test_config_dir_returns_path() {
    let path = aiclient_api::util::xdg::config_dir();
    assert!(path.ends_with("aiclient-api"));
}

#[test]
fn test_runtime_dir_returns_path() {
    let path = aiclient_api::util::xdg::runtime_dir();
    assert!(path.to_str().unwrap().contains("aiclient-api"));
}

#[test]
fn test_state_dir_returns_path() {
    let path = aiclient_api::util::xdg::state_dir();
    assert!(path.to_str().unwrap().contains("aiclient-api"));
}

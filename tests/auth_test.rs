use aiclient_api::auth::token_store::XdgTokenStore;
use aiclient_api::auth::{TokenData, TokenStore};
use tempfile::TempDir;

#[tokio::test]
async fn test_save_and_load_copilot_token() {
    let tmp = TempDir::new().unwrap();
    let store = XdgTokenStore::new(tmp.path().to_path_buf());

    let data = TokenData::Copilot {
        github_token: "gho_test123".to_string(),
        copilot_token: None,
        expires_at: None,
    };
    store.save("copilot", &data).await.unwrap();
    let loaded = store.load("copilot").await.unwrap();

    match loaded {
        TokenData::Copilot { github_token, .. } => {
            assert_eq!(github_token, "gho_test123");
        }
        _ => panic!("Expected Copilot token"),
    }
}

#[tokio::test]
async fn test_delete_token() {
    let tmp = TempDir::new().unwrap();
    let store = XdgTokenStore::new(tmp.path().to_path_buf());

    let data = TokenData::Copilot {
        github_token: "gho_test".to_string(),
        copilot_token: None,
        expires_at: None,
    };
    store.save("copilot", &data).await.unwrap();
    store.delete("copilot").await.unwrap();

    assert!(store.load("copilot").await.is_err());
}

#[tokio::test]
async fn test_load_nonexistent_returns_error() {
    let tmp = TempDir::new().unwrap();
    let store = XdgTokenStore::new(tmp.path().to_path_buf());
    assert!(store.load("nonexistent").await.is_err());
}

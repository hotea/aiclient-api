#[tokio::test]
async fn test_healthz_returns_ok() {
    let config = aiclient_api::config::types::Config::default();
    let state = aiclient_api::server::state::AppState::new(config);
    let app = aiclient_api::server::build_router(state);

    let server = axum_test::TestServer::new(app);
    let response = server.get("/healthz").await;
    response.assert_status_ok();
    response.assert_json(&serde_json::json!({ "status": "ok" }));
}

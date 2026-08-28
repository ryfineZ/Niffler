use std::sync::Arc;

use aether_data::repository::auth::InMemoryAuthApiKeySnapshotRepository;
use aether_data::repository::content_moderation_evidence::InsertContentModerationEvidenceRecord;
use aether_data::repository::provider_catalog::InMemoryProviderCatalogReadRepository;
use aether_data::repository::users::{InMemoryUserReadRepository, StoredUserAuthRecord};
use serde_json::json;

use super::super::{
    build_router_with_state, sample_currently_usable_auth_snapshot, sample_endpoint, sample_key,
    sample_provider, start_server,
};
use crate::constants::{
    GATEWAY_HEADER, TRUSTED_ADMIN_SESSION_ID_HEADER, TRUSTED_ADMIN_USER_ID_HEADER,
    TRUSTED_ADMIN_USER_ROLE_HEADER,
};

#[tokio::test]
async fn gateway_returns_readable_names_for_content_moderation_evidence_when_url_is_set() {
    let Some(state) = crate::data::tests::postgres_app_state_when_url_is_set(
        "gateway_returns_readable_names_for_content_moderation_evidence",
    )
    .await
    else {
        return;
    };

    let user = sample_content_moderation_user("user-content-moderation", "alice-review");
    let user_repository = Arc::new(InMemoryUserReadRepository::seed_auth_users([user]));
    let mut auth_snapshot = sample_currently_usable_auth_snapshot(
        "api-key-content-moderation",
        "user-content-moderation",
    );
    auth_snapshot.api_key_name = Some("Alice production key".to_string());
    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed([(
        None,
        auth_snapshot,
    )]));

    let mut upstream_key = sample_key(
        "upstream-account-content-moderation",
        "provider-content-moderation",
        "openai:responses",
        "upstream-secret",
    );
    upstream_key.name = "OpenAI primary account".to_string();
    let provider_catalog_repository = Arc::new(InMemoryProviderCatalogReadRepository::seed(
        vec![sample_provider(
            "provider-content-moderation",
            "OpenAI protected",
            10,
        )],
        vec![sample_endpoint(
            "upstream-service-content-moderation",
            "provider-content-moderation",
            "openai:responses",
            "https://api.openai.example",
        )],
        vec![upstream_key],
    ));

    let data_state = (*state.data)
        .clone()
        .with_user_reader(user_repository)
        .with_auth_api_key_reader(auth_repository)
        .attach_provider_catalog_repository_for_tests(provider_catalog_repository);
    let state = state.with_data_state_for_tests(data_state);
    state
        .data
        .insert_content_moderation_evidence(InsertContentModerationEvidenceRecord {
            id: "evidence-content-moderation".to_string(),
            request_id: "request-content-moderation".to_string(),
            user_id: Some("user-content-moderation".to_string()),
            api_key_id: Some("api-key-content-moderation".to_string()),
            provider_id: Some("provider-content-moderation".to_string()),
            upstream_service_id: Some("upstream-service-content-moderation".to_string()),
            upstream_account_id: Some("upstream-account-content-moderation".to_string()),
            moderation_model: "omni-moderation-latest".to_string(),
            input_sha256: "a".repeat(64),
            input_text: Some("blocked user text".to_string()),
            categories: json!({"violence": true}),
            category_scores: json!({"violence": 0.91}),
            flagged: true,
            created_at_unix_secs: 1_800_000_000,
            expires_at_unix_secs: 1_802_592_000,
        })
        .await
        .expect("evidence insert should succeed")
        .expect("evidence writer should be available");

    let gateway = build_router_with_state(state);
    let (gateway_url, gateway_handle) = start_server(gateway).await;
    let response = reqwest::Client::new()
        .get(format!(
            "{gateway_url}/api/admin/content-moderation/evidence/evidence-content-moderation"
        ))
        .header(GATEWAY_HEADER, "rust-phase3b")
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), http::StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["username"], "alice-review");
    assert_eq!(payload["api_key_name"], "Alice production key");
    assert_eq!(payload["provider_name"], "OpenAI protected");
    assert_eq!(payload["upstream_service_name"], "openai:responses");
    assert_eq!(payload["upstream_account_name"], "OpenAI primary account");
    assert_eq!(payload["input_text"], "blocked user text");

    gateway_handle.abort();
}

fn sample_content_moderation_user(user_id: &str, username: &str) -> StoredUserAuthRecord {
    StoredUserAuthRecord::new(
        user_id.to_string(),
        Some(format!("{username}@example.com")),
        true,
        username.to_string(),
        None,
        "user".to_string(),
        "local".to_string(),
        None,
        None,
        None,
        true,
        false,
        None,
        None,
    )
    .expect("user should build")
}

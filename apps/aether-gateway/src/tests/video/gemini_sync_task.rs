use aether_crypto::{encrypt_python_fernet_plaintext, DEVELOPMENT_ENCRYPTION_KEY};
use aether_data::repository::candidates::InMemoryRequestCandidateRepository;
use aether_data::repository::provider_catalog::InMemoryProviderCatalogReadRepository;
use aether_data::repository::video_tasks::InMemoryVideoTaskRepository;
use aether_data_contracts::repository::candidates::{
    RequestCandidateReadRepository, RequestCandidateStatus,
};
use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use aether_data_contracts::repository::video_tasks::{
    UpsertVideoTask, VideoTaskStatus, VideoTaskWriteRepository,
};
use axum::body::{to_bytes, Body};
use axum::response::Response;
use axum::routing::any;
use axum::{extract::Request, Json, Router};
use http::header::{HeaderName, HeaderValue};
use http::StatusCode;
use serde_json::json;
use std::sync::{Arc, Mutex};

use crate::constants::{CONTROL_EXECUTED_HEADER, CONTROL_EXECUTE_FALLBACK_HEADER, TRACE_ID_HEADER};

use super::{
    build_router_with_state, build_state_with_execution_runtime_override, response_request_id,
    start_server,
    VideoTaskTruthSourceMode,
};

#[tokio::test]
async fn gateway_executes_gemini_video_cancel_via_data_backed_local_follow_up_with_local_planning_only(
) {
    #[derive(Debug, Clone)]
    struct SeenExecutionRuntimeSyncRequest {
        method: String,
        url: String,
        auth_header_value: String,
    }

    let decision_hits = Arc::new(Mutex::new(0usize));
    let decision_hits_clone = Arc::clone(&decision_hits);
    let plan_hits = Arc::new(Mutex::new(0usize));
    let plan_hits_clone = Arc::clone(&plan_hits);
    let seen_execution_runtime = Arc::new(Mutex::new(None::<SeenExecutionRuntimeSyncRequest>));
    let seen_execution_runtime_clone = Arc::clone(&seen_execution_runtime);
    let report_hits = Arc::new(Mutex::new(0usize));
    let report_hits_clone = Arc::clone(&report_hits);
    let public_hits = Arc::new(Mutex::new(0usize));
    let public_hits_clone = Arc::clone(&public_hits);

    let upstream = Router::new()
        .route(
            "/api/internal/gateway/resolve",
            any(|_request: Request| async move {
                Json(json!({
                    "action": "proxy_public",
                    "route_class": "ai_public",
                    "route_family": "gemini",
                    "route_kind": "video",
                    "auth_endpoint_signature": "gemini:video",
                    "execution_runtime_candidate": true,
                    "auth_context": {
                        "user_id": "user-gemini-video-cancel-local-123",
                        "api_key_id": "key-gemini-video-cancel-local-123",
                        "access_allowed": true
                    },
                    "public_path": "/v1beta/models/veo-3/operations/localshort123:cancel"
                }))
            }),
        )
        .route(
            "/api/internal/gateway/decision-sync",
            any(move |_request: Request| {
                let decision_hits_inner = Arc::clone(&decision_hits_clone);
                async move {
                    *decision_hits_inner.lock().expect("mutex should lock") += 1;
                    Json(json!({"action": "proxy_public"}))
                }
            }),
        )
        .route(
            "/api/internal/gateway/plan-sync",
            any(move |_request: Request| {
                let plan_hits_inner = Arc::clone(&plan_hits_clone);
                async move {
                    *plan_hits_inner.lock().expect("mutex should lock") += 1;
                    Json(json!({"action": "proxy_public"}))
                }
            }),
        )
        .route(
            "/api/internal/gateway/report-sync",
            any(move |_request: Request| {
                let report_hits_inner = Arc::clone(&report_hits_clone);
                async move {
                    *report_hits_inner.lock().expect("mutex should lock") += 1;
                    Json(json!({"ok": true}))
                }
            }),
        )
        .route(
            "/v1beta/models/veo-3/operations/localshort123:cancel",
            any(move |_request: Request| {
                let public_hits_inner = Arc::clone(&public_hits_clone);
                async move {
                    *public_hits_inner.lock().expect("mutex should lock") += 1;
                    (StatusCode::IM_A_TEAPOT, Body::from("public-route-hit"))
                }
            }),
        );

    let execution_runtime = Router::new().route(
        "/v1/execute/sync",
        any(move |request: Request| {
            let seen_execution_runtime_inner = Arc::clone(&seen_execution_runtime_clone);
            async move {
                let (_parts, body) = request.into_parts();
                let raw_body = to_bytes(body, usize::MAX).await.expect("body should read");
                let payload: serde_json::Value = serde_json::from_slice(&raw_body)
                    .expect("execution runtime payload should parse");
                *seen_execution_runtime_inner
                    .lock()
                    .expect("mutex should lock") = Some(SeenExecutionRuntimeSyncRequest {
                    method: payload
                        .get("method")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    url: payload
                        .get("url")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    auth_header_value: payload
                        .get("headers")
                        .and_then(|value| value.get("x-goog-api-key"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                });
                Json(json!({
                    "request_id": "trace-gemini-video-cancel-local-123",
                    "status_code": 200,
                    "headers": {
                        "content-type": "application/json"
                    },
                    "body": {
                        "json_body": {}
                    },
                    "telemetry": {
                        "elapsed_ms": 17
                    }
                }))
            }
        }),
    );

    let repository = Arc::new(InMemoryVideoTaskRepository::default());
    let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
    repository
        .upsert(UpsertVideoTask {
            id: "task-gemini-cancel-local-123".to_string(),
            short_id: Some("localshort123".to_string()),
            request_id: "request-gemini-video-cancel-local-123".to_string(),
            user_id: Some("user-gemini-video-cancel-local-123".to_string()),
            api_key_id: Some("key-gemini-video-cancel-local-123".to_string()),
            username: Some("video-user".to_string()),
            api_key_name: Some("video-key".to_string()),
            external_task_id: Some("operations/ext-123".to_string()),
            provider_id: Some("provider-gemini-video-local-1".to_string()),
            endpoint_id: Some("endpoint-gemini-video-local-1".to_string()),
            key_id: Some("key-gemini-video-local-1".to_string()),
            client_api_format: Some("gemini:video".to_string()),
            provider_api_format: Some("gemini:video".to_string()),
            format_converted: false,
            model: Some("veo-3".to_string()),
            prompt: Some("gemini prompt".to_string()),
            original_request_body: Some(json!({"prompt": "gemini prompt"})),
            duration_seconds: Some(8),
            resolution: Some("720p".to_string()),
            aspect_ratio: Some("16:9".to_string()),
            size: Some("720p".to_string()),
            status: VideoTaskStatus::Submitted,
            progress_percent: 0,
            progress_message: None,
            retry_count: 0,
            poll_interval_seconds: 10,
            next_poll_at_unix_secs: Some(124),
            poll_count: 0,
            max_poll_count: 360,
            created_at_unix_ms: 123,
            submitted_at_unix_secs: Some(123),
            completed_at_unix_secs: None,
            updated_at_unix_secs: 123,
            error_code: None,
            error_message: None,
            video_url: None,
            request_metadata: Some(json!({
                "rust_local_snapshot": {
                    "Gemini": {
                        "local_short_id": "localshort123",
                        "upstream_operation_name": "operations/ext-123",
                        "user_id": "user-gemini-video-cancel-local-123",
                        "api_key_id": "key-gemini-video-cancel-local-123",
                        "model": "veo-3",
                        "status": "Submitted",
                        "progress_percent": 0,
                        "error_code": null,
                        "error_message": null,
                        "metadata": {},
                        "persistence": {
                            "request_id": "request-gemini-video-cancel-local-123",
                            "username": "video-user",
                            "api_key_name": "video-key",
                            "client_api_format": "gemini:video",
                            "provider_api_format": "gemini:video",
                            "original_request_body": {
                                "prompt": "gemini prompt"
                            },
                            "format_converted": false
                        },
                        "transport": {
                            "upstream_base_url": "https://generativelanguage.googleapis.com",
                            "provider_name": "gemini-video",
                            "provider_id": "provider-gemini-video-local-1",
                            "endpoint_id": "endpoint-gemini-video-local-1",
                            "key_id": "key-gemini-video-local-1",
                            "headers": {
                                "x-goog-api-key": "sk-upstream-gemini-video",
                                "content-type": "application/json"
                            },
                            "content_type": "application/json",
                            "model_name": "veo-3-upstream",
                            "proxy": null,
                            "transport_profile": null,
                            "timeouts": null
                        }
                    }
                }
            })),
        })
        .await
        .expect("upsert should succeed");

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let (execution_runtime_url, execution_runtime_handle) = start_server(execution_runtime).await;
    let gateway_state = build_state_with_execution_runtime_override(execution_runtime_url)
        .with_data_state_for_tests(
        crate::data::GatewayDataState::with_video_task_and_request_candidate_repository_for_tests(
            repository,
            Arc::clone(&request_candidate_repository),
        ),
    );
    let gateway = build_router_with_state(gateway_state.clone());
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .post(format!(
            "{gateway_url}/v1beta/models/veo-3/operations/localshort123:cancel"
        ))
        .header("x-goog-api-key", "client-key")
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(TRACE_ID_HEADER, "trace-gemini-video-cancel-local-123")
        .body("{}")
        .send()
        .await
        .expect("request should succeed");

    let request_id = response_request_id(&response);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("body should parse"),
        json!({})
    );

    let seen_execution_runtime_request = seen_execution_runtime
        .lock()
        .expect("mutex should lock")
        .clone()
        .expect("execution runtime sync should be captured");
    assert_eq!(seen_execution_runtime_request.method, "POST");
    assert_eq!(
        seen_execution_runtime_request.url,
        "https://generativelanguage.googleapis.com/v1beta/models/veo-3/operations/ext-123:cancel"
    );
    assert_eq!(
        seen_execution_runtime_request.auth_header_value,
        "sk-upstream-gemini-video"
    );

    let stored_candidates = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            crate::request_candidate_runtime::flush_request_candidate_status_writes(&gateway_state)
                .await;
            let stored_candidates = request_candidate_repository
                .list_by_request_id(&request_id)
                .await
                .expect("request candidate trace should read");
            if stored_candidates
                .iter()
                .any(|candidate| candidate.status == RequestCandidateStatus::Success)
            {
                break stored_candidates;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("request candidate status should settle");
    assert_eq!(stored_candidates.len(), 1);
    assert_eq!(stored_candidates[0].status, RequestCandidateStatus::Success);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(*report_hits.lock().expect("mutex should lock"), 0);
    assert_eq!(*decision_hits.lock().expect("mutex should lock"), 0);
    assert_eq!(*plan_hits.lock().expect("mutex should lock"), 0);
    assert_eq!(*public_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    execution_runtime_handle.abort();
    upstream_handle.abort();
}

#[tokio::test]
async fn gateway_executes_gemini_video_cancel_via_reconstructed_data_backed_local_follow_up_with_local_follow_up_routing(
) {
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SeenExecutionRuntimeSyncRequest {
        method: String,
        url: String,
        api_key: String,
    }

    fn sample_provider() -> StoredProviderCatalogProvider {
        StoredProviderCatalogProvider::new(
            "provider-gemini-video-followup-1".to_string(),
            "gemini".to_string(),
            Some("https://example.com".to_string()),
            "custom".to_string(),
        )
        .expect("provider should build")
        .with_transport_fields(
            true,
            false,
            false,
            None,
            Some(2),
            None,
            Some(20.0),
            None,
            None,
        )
    }

    fn sample_endpoint() -> StoredProviderCatalogEndpoint {
        StoredProviderCatalogEndpoint::new(
            "endpoint-gemini-video-followup-1".to_string(),
            "provider-gemini-video-followup-1".to_string(),
            "gemini:video".to_string(),
            Some("gemini".to_string()),
            Some("video".to_string()),
            true,
        )
        .expect("endpoint should build")
        .with_transport_fields(
            "https://generativelanguage.googleapis.com".to_string(),
            None,
            None,
            Some(2),
            None,
            None,
            None,
            None,
        )
        .expect("endpoint transport should build")
    }

    fn sample_key() -> StoredProviderCatalogKey {
        StoredProviderCatalogKey::new(
            "key-gemini-video-followup-1".to_string(),
            "provider-gemini-video-followup-1".to_string(),
            "prod".to_string(),
            "api_key".to_string(),
            None,
            true,
        )
        .expect("key should build")
        .with_transport_fields(
            Some(json!(["gemini:video"])),
            encrypt_python_fernet_plaintext(DEVELOPMENT_ENCRYPTION_KEY, "sk-upstream-gemini-video")
                .expect("api key should encrypt"),
            None,
            None,
            Some(json!({"gemini:video": 1})),
            None,
            None,
            None,
            None,
        )
        .expect("key transport should build")
    }

    let decision_hits = Arc::new(Mutex::new(0usize));
    let decision_hits_clone = Arc::clone(&decision_hits);
    let execute_hits = Arc::new(Mutex::new(0usize));
    let execute_hits_clone = Arc::clone(&execute_hits);
    let public_hits = Arc::new(Mutex::new(0usize));
    let public_hits_clone = Arc::clone(&public_hits);
    let report_hits = Arc::new(Mutex::new(0usize));
    let report_hits_clone = Arc::clone(&report_hits);
    let seen_execution_runtime = Arc::new(Mutex::new(None::<SeenExecutionRuntimeSyncRequest>));
    let seen_execution_runtime_clone = Arc::clone(&seen_execution_runtime);

    let upstream = Router::new()
        .route(
            "/api/internal/gateway/resolve",
            any(|_request: Request| async move {
                Json(json!({
                    "action": "proxy_public",
                    "route_class": "ai_public",
                    "route_family": "gemini",
                    "route_kind": "video",
                    "auth_endpoint_signature": "gemini:video",
                    "execution_runtime_candidate": true,
                    "auth_context": {
                        "user_id": "user-gemini-video-cancel-op-123",
                        "api_key_id": "key-gemini-video-cancel-op-123",
                        "access_allowed": true
                    },
                    "public_path": "/v1beta/models/veo-3/operations/opshort123:cancel"
                }))
            }),
        )
        .route(
            "/api/internal/gateway/decision-sync",
            any(move |_request: Request| {
                let decision_hits_inner = Arc::clone(&decision_hits_clone);
                async move {
                    *decision_hits_inner.lock().expect("mutex should lock") += 1;
                    Json(json!({
                        "action": "execution_runtime_sync_decision",
                        "decision_kind": "gemini_video_cancel_sync",
                        "request_id": "unexpected-decision-hit"
                    }))
                }
            }),
        )
        .route(
            "/api/internal/gateway/report-sync",
            any(move |_request: Request| {
                let report_hits_inner = Arc::clone(&report_hits_clone);
                async move {
                    *report_hits_inner.lock().expect("mutex should lock") += 1;
                    Json(json!({"ok": true}))
                }
            }),
        )
        .route(
            "/api/internal/gateway/execute-sync",
            any(move |_request: Request| {
                let execute_hits_inner = Arc::clone(&execute_hits_clone);
                async move {
                    *execute_hits_inner.lock().expect("mutex should lock") += 1;
                    let mut response = Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from("{\"fallback\":true}"))
                        .expect("response should build");
                    response.headers_mut().insert(
                        HeaderName::from_static(CONTROL_EXECUTED_HEADER),
                        HeaderValue::from_static("true"),
                    );
                    response
                }
            }),
        )
        .route(
            "/v1beta/models/veo-3/operations/opshort123:cancel",
            any(move |_request: Request| {
                let public_hits_inner = Arc::clone(&public_hits_clone);
                async move {
                    *public_hits_inner.lock().expect("mutex should lock") += 1;
                    (StatusCode::IM_A_TEAPOT, Body::from("public-route-hit"))
                }
            }),
        );

    let execution_runtime = Router::new().route(
        "/v1/execute/sync",
        any(move |request: Request| {
            let seen_execution_runtime_inner = Arc::clone(&seen_execution_runtime_clone);
            async move {
                let (_parts, body) = request.into_parts();
                let raw_body = to_bytes(body, usize::MAX).await.expect("body should read");
                let payload: serde_json::Value = serde_json::from_slice(&raw_body)
                    .expect("execution runtime payload should parse");
                *seen_execution_runtime_inner
                    .lock()
                    .expect("mutex should lock") = Some(SeenExecutionRuntimeSyncRequest {
                    method: payload
                        .get("method")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    url: payload
                        .get("url")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    api_key: payload
                        .get("headers")
                        .and_then(|value| value.get("x-goog-api-key"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                });
                Json(json!({
                    "request_id": "trace-gemini-video-cancel-op-123",
                    "status_code": 200,
                    "headers": {
                        "content-type": "application/json"
                    },
                    "body": {
                        "json_body": {}
                    }
                }))
            }
        }),
    );

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let (execution_runtime_url, execution_runtime_handle) = start_server(execution_runtime).await;

    let repository = Arc::new(InMemoryVideoTaskRepository::default());
    repository
        .upsert(UpsertVideoTask {
            id: "task-gemini-cancel-op-123".to_string(),
            short_id: Some("opshort123".to_string()),
            request_id: "request-gemini-video-cancel-op-123".to_string(),
            user_id: Some("user-gemini-video-cancel-op-123".to_string()),
            api_key_id: Some("key-gemini-video-cancel-op-123".to_string()),
            username: Some("video-user".to_string()),
            api_key_name: Some("video-key".to_string()),
            external_task_id: Some("operations/ext-op-123".to_string()),
            provider_id: Some("provider-gemini-video-followup-1".to_string()),
            endpoint_id: Some("endpoint-gemini-video-followup-1".to_string()),
            key_id: Some("key-gemini-video-followup-1".to_string()),
            client_api_format: Some("gemini:video".to_string()),
            provider_api_format: Some("gemini:video".to_string()),
            format_converted: false,
            model: Some("veo-3".to_string()),
            prompt: Some("operation cancel".to_string()),
            original_request_body: Some(json!({
                "prompt": "operation cancel"
            })),
            duration_seconds: Some(4),
            resolution: Some("720p".to_string()),
            aspect_ratio: Some("16:9".to_string()),
            size: None,
            status: VideoTaskStatus::Processing,
            progress_percent: 50,
            progress_message: None,
            retry_count: 0,
            poll_interval_seconds: 10,
            next_poll_at_unix_secs: Some(123),
            poll_count: 0,
            max_poll_count: 360,
            created_at_unix_ms: 123,
            submitted_at_unix_secs: Some(123),
            completed_at_unix_secs: None,
            updated_at_unix_secs: 123,
            error_code: None,
            error_message: None,
            video_url: None,
            request_metadata: None,
        })
        .await
        .expect("upsert should succeed");
    let provider_catalog_repository = Arc::new(InMemoryProviderCatalogReadRepository::seed(
        vec![sample_provider()],
        vec![sample_endpoint()],
        vec![sample_key()],
    ));
    let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());

    let gateway_state = build_state_with_execution_runtime_override(execution_runtime_url)
        .with_video_task_truth_source_mode(VideoTaskTruthSourceMode::RustAuthoritative)
        .with_data_state_for_tests(
            crate::data::GatewayDataState::with_video_task_provider_transport_and_request_candidate_repository_for_tests(
                repository,
                provider_catalog_repository,
                Arc::clone(&request_candidate_repository),
                DEVELOPMENT_ENCRYPTION_KEY,
            ),
        );
    let gateway = build_router_with_state(gateway_state.clone());
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .post(format!(
            "{gateway_url}/v1beta/models/veo-3/operations/opshort123:cancel"
        ))
        .header(CONTROL_EXECUTE_FALLBACK_HEADER, "true")
        .header(TRACE_ID_HEADER, "trace-gemini-video-cancel-op-123")
        .send()
        .await
        .expect("request should succeed");

    let request_id = response_request_id(&response);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("body should parse"),
        json!({})
    );

    let seen_execution_runtime_request = seen_execution_runtime
        .lock()
        .expect("mutex should lock")
        .clone()
        .expect("execution runtime sync should be captured");
    assert_eq!(seen_execution_runtime_request.method, "POST");
    assert_eq!(
        seen_execution_runtime_request.url,
        "https://generativelanguage.googleapis.com/v1beta/models/veo-3/operations/ext-op-123:cancel"
    );
    assert_eq!(
        seen_execution_runtime_request.api_key,
        "sk-upstream-gemini-video"
    );

    let stored_candidates = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            crate::request_candidate_runtime::flush_request_candidate_status_writes(&gateway_state)
                .await;
            let stored_candidates = request_candidate_repository
                .list_by_request_id(&request_id)
                .await
                .expect("request candidate trace should read");
            if stored_candidates
                .iter()
                .any(|candidate| candidate.status == RequestCandidateStatus::Success)
            {
                break stored_candidates;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("request candidate status should settle");
    assert_eq!(stored_candidates.len(), 1);
    assert_eq!(stored_candidates[0].status, RequestCandidateStatus::Success);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(*decision_hits.lock().expect("mutex should lock"), 0);
    assert_eq!(*execute_hits.lock().expect("mutex should lock"), 0);
    assert_eq!(*report_hits.lock().expect("mutex should lock"), 0);
    assert_eq!(*public_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    execution_runtime_handle.abort();
    upstream_handle.abort();
}

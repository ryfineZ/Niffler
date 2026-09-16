use super::*;
use aether_data_contracts::repository::candidates::RequestCandidateReadRepository;
use aether_data_contracts::repository::usage::{UsageBodyCaptureState, UsageReadRepository};
use serde_json::Value;

struct ReplayServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for ReplayServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn replay_server(
    response_status: u16,
    content_type: &'static str,
    response_body: String,
) -> (ReplayServer, Arc<Mutex<Vec<(String, HeaderMap, Value)>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = Router::new().fallback(any(move |request: Request| {
        let captured = captured.clone();
        let response_body = response_body.clone();
        async move {
            let (parts, body) = request.into_parts();
            let body = axum::body::to_bytes(body, 4 * 1024 * 1024).await.unwrap();
            captured.lock().unwrap().push((
                parts.uri.to_string(),
                parts.headers,
                serde_json::from_slice(&body).unwrap(),
            ));
            if response_body == "SLOW" {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            (
                StatusCode::from_u16(response_status).unwrap(),
                [("content-type", content_type)],
                response_body,
            )
        }
    }));
    // Five-digit ports; each test owns an abort-on-drop server even on assertion failure.
    let mut bound = None;
    for port in 18100..19100 {
        if let Ok(listener) =
            tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
        {
            bound = Some(listener);
            break;
        }
    }
    let listener = bound.expect("test port available");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (ReplayServer { url, task }, requests)
}

fn usage(format: &str) -> StoredRequestUsageAudit {
    let mut item = sample_usage_row(
        "usage-replay",
        "original-request",
        Some("user-1"),
        Some("user-key"),
        None,
        "Original",
        "gpt-test",
        "success",
        10,
        5,
        0.2,
        0.2,
        DAY_1_UNIX_SECS,
    );
    item.api_format = Some(format.to_string());
    item.endpoint_api_format = Some(format.to_string());
    item.request_headers = Some(
        json!({"authorization":"Bearer old-user-secret", "x-api-key":"old-secret", "cookie":"old-cookie", "x-aether-execution-http1-only":"true"}),
    );
    item.request_body = Some(if format == "openai:responses" {
        json!({"model":"gpt-test","input":[{"role":"user","content":"replay full input"}],"stream":true})
    } else {
        json!({"model":"gpt-test","messages":[{"role":"user","content":"replay full input"}],"stream":false})
    });
    item
}

fn state(
    item: StoredRequestUsageAudit,
    base_url: &str,
    detached: bool,
    codex: bool,
) -> (
    AppState,
    Arc<InMemoryRequestCandidateRepository>,
    Arc<InMemoryUsageReadRepository>,
) {
    let format = item.endpoint_api_format.clone().unwrap();
    let usage_repo = Arc::new(if detached {
        InMemoryUsageReadRepository::seed_with_detached_bodies(vec![item])
    } else {
        InMemoryUsageReadRepository::seed(vec![item])
    });
    let candidates = Arc::new(InMemoryRequestCandidateRepository::seed(Vec::new()));
    let mut provider = sample_provider("provider-1", "Original", 10);
    let mut key = sample_key("provider-key-1", "provider-1", &format, "sk-replay-current");
    provider.request_timeout_secs = Some(1.0);
    if codex {
        provider.provider_type = "codex".into();
        provider.config = Some(json!({"openai_responses_image_generation_tool_enabled":false}));
        key.auth_type = "oauth".into();
        key.encrypted_auth_config = Some(encrypt_python_fernet_plaintext(DEVELOPMENT_ENCRYPTION_KEY,
            &json!({"provider_type":"codex","expires_at":4102444800u64,"account_id":"account-test"}).to_string()).unwrap());
    }
    let catalog = Arc::new(InMemoryProviderCatalogReadRepository::seed(
        vec![provider, sample_provider("provider-2", "Alternate", 10)],
        vec![
            sample_endpoint("endpoint-1", "provider-1", &format, base_url),
            sample_endpoint("endpoint-2", "provider-2", &format, base_url),
        ],
        vec![
            key,
            sample_key("provider-key-2", "provider-2", &format, "sk-alternate"),
        ],
    ));
    let state = AppState::new().unwrap().with_data_state_for_tests(
        GatewayDataState::with_request_candidate_and_usage_repository_for_tests(
            candidates.clone(),
            usage_repo.clone(),
        )
        .with_provider_catalog_reader(catalog)
        .with_encryption_key_for_tests(DEVELOPMENT_ENCRYPTION_KEY),
    );
    (state, candidates, usage_repo)
}

async fn replay(state: &AppState, params: Value) -> (StatusCode, Value, Option<AdminAuditEvent>) {
    let response = local_admin_usage_response(
        state,
        http::Method::POST,
        "/api/admin/usage/usage-replay/replay",
        Some(params),
    )
    .await;
    let status = response.status();
    let audit = response.extensions().get::<AdminAuditEvent>().cloned();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap(), audit)
}

#[tokio::test]
async fn admin_usage_replay_dispatches_ref_body_and_preserves_original_billing() {
    let (server, requests) = replay_server(
        200,
        "application/json",
        json!({"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]})
            .to_string(),
    )
    .await;
    let original = usage("openai:chat");
    let (state, candidates, usage_repo) =
        state(original.clone(), &format!("{}/v1", server.url), true, false);
    let (status, payload, audit) = replay(&state, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(payload["status"], "success", "{payload}");
    assert_eq!(payload["status_code"], 200);
    assert_eq!(
        payload["response_body"]["choices"][0]["message"]["content"],
        "ok"
    );
    assert_eq!(payload["original_request_id"], original.request_id);
    {
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "/v1/chat/completions");
        assert_eq!(captured[0].1["authorization"], "Bearer sk-replay-current");
        assert!(!captured[0].1.contains_key("cookie"));
        assert!(!captured[0].1.contains_key("x-api-key"));
        assert_eq!(captured[0].2["messages"][0]["content"], "replay full input");
        assert_eq!(captured[0].2["model"], "gpt-test-target");
    }
    let records = candidates
        .list_by_request_id(payload["replay_id"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, RequestCandidateStatus::Success);
    assert!(records[0].user_id.is_none());
    assert!(records[0].api_key_id.is_none());
    assert_eq!(
        records[0].extra_data.as_ref().unwrap()["admin_replay"]["original_request_id"],
        "original-request"
    );
    let after = usage_repo
        .find_by_id("usage-replay")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.actual_total_cost_usd, original.actual_total_cost_usd);
    assert_eq!(after.status, original.status);
    assert_eq!(audit.unwrap().action, "execute_usage_replay");
}

#[tokio::test]
async fn admin_usage_replay_responses_stream_uses_responses_path_and_keeps_tools() {
    let body = "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"name\":\"read_file\",\"call_id\":\"call-1\",\"arguments\":\"{}\"}]}}\n\n";
    let (server, requests) = replay_server(200, "text/event-stream", body.to_string()).await;
    let (state, _, _) = state(
        usage("openai:responses"),
        &format!("{}/v1", server.url),
        false,
        false,
    );
    let (_, payload, _) = replay(&state, json!({})).await;
    assert_eq!(payload["status"], "success", "{payload}");
    assert_eq!(payload["response_body"]["output"][0]["name"], "read_file");
    let captured = requests.lock().unwrap();
    assert_eq!(captured[0].0, "/v1/responses");
    assert_eq!(captured[0].2["stream"], true);
    assert_eq!(
        captured[0].2["input"][0]["content"][0]["text"],
        "replay full input"
    );
}

#[tokio::test]
async fn admin_usage_replay_surfaces_http_and_stream_errors() {
    for (code, kind, body, expected) in [
        (502, "text/html", "<html>Bad Gateway</html>", "502"),
        (200, "text/event-stream", "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"quota exceeded\"}}}\n\n", "quota exceeded"),
        (200, "text/event-stream", "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n", "提前结束"),
    ] {
        let (server, requests) = replay_server(code, kind, body.into()).await;
        let (state, candidates, _) = state(usage("openai:responses"), &format!("{}/v1", server.url), false, false);
        let (status, payload, _) = replay(&state, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["status"], "failed");
        assert!(payload["error_message"].as_str().unwrap().contains(expected), "{payload}");
        assert!(!payload["response_body"].is_null());
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert_eq!(candidates.list_by_request_id(payload["replay_id"].as_str().unwrap()).await.unwrap()[0].status, RequestCandidateStatus::Failed);
    }
}

#[tokio::test]
async fn admin_usage_replay_rejects_missing_truncated_body_and_foreign_key_without_dispatch() {
    let (server, requests) = replay_server(200, "application/json", "{}".into()).await;
    for mode in ["missing", "truncated", "foreign-key"] {
        let mut item = usage("openai:chat");
        if mode == "missing" {
            item.request_body = None;
        }
        if mode == "truncated" {
            item.request_body_state = Some(UsageBodyCaptureState::Truncated);
        }
        let (state, _, _) = state(item, &server.url, false, false);
        let params = if mode == "foreign-key" {
            json!({"api_key_id":"provider-key-2"})
        } else {
            json!({})
        };
        let (status, payload, _) = replay(&state, params).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{payload}");
        assert!(payload["detail"].as_str().is_some());
    }
    assert!(requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn admin_usage_replay_cross_provider_selects_target_key_and_drops_original_mapping() {
    let (server, requests) = replay_server(
        200,
        "application/json",
        "{\"choices\":[{\"message\":{\"content\":\"ok\"}}]}".into(),
    )
    .await;
    let (state, _, _) = state(
        usage("openai:chat"),
        &format!("{}/v1", server.url),
        false,
        false,
    );
    let (_, payload, _) = replay(&state, json!({"provider_id":"provider-2"})).await;
    assert_eq!(payload["status"], "success", "{payload}");
    assert_eq!(payload["mapping"]["target_api_key_id"], "provider-key-2");
    let captured = requests.lock().unwrap();
    assert_eq!(captured[0].1["authorization"], "Bearer sk-alternate");
    assert_eq!(captured[0].2["model"], "gpt-test");
}

#[tokio::test]
async fn admin_usage_replay_codex_oauth_rebuilds_auth_and_forces_streaming() {
    let body = "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}]}}\n\n";
    let (server, requests) = replay_server(200, "text/event-stream", body.into()).await;
    let mut item = usage("openai:responses");
    item.request_body.as_mut().unwrap()["stream"] = json!(false);
    let (state, _, _) = state(
        item,
        &format!("{}/backend-api/codex", server.url),
        false,
        true,
    );
    let (_, payload, _) = replay(&state, json!({})).await;
    assert_eq!(payload["status"], "success", "{payload}");
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].0, "/backend-api/codex/responses");
    assert_eq!(captured[0].1["authorization"], "Bearer sk-replay-current");
    assert_eq!(captured[0].1["chatgpt-account-id"], "account-test");
    assert!(
        !captured[0].2["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool["type"] == "image_generation")),
        "replay must respect the disabled hosted-image setting"
    );
    assert_eq!(captured[0].2["stream"], true);
}

#[tokio::test]
async fn admin_usage_replay_timeout_is_recorded_without_retry() {
    let (server, requests) = replay_server(200, "application/json", "SLOW".into()).await;
    let (state, candidates, _) = state(
        usage("openai:chat"),
        &format!("{}/v1", server.url),
        false,
        false,
    );
    let (_, payload, _) = replay(&state, json!({})).await;
    assert_eq!(payload["status"], "failed", "{payload}");
    assert!(payload["error_message"].as_str().unwrap().contains("超时"));
    assert!(payload["status_code"].is_null());
    assert_eq!(requests.lock().unwrap().len(), 1);
    let records = candidates
        .list_by_request_id(payload["replay_id"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(records[0].status, RequestCandidateStatus::Failed);
    assert!(records[0].finished_at_unix_ms.is_some());
}

#[tokio::test]
async fn admin_usage_replay_converts_saved_chat_input_to_claude_endpoint() {
    let response = json!({"type":"message","role":"assistant","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"});
    let (server, requests) = replay_server(200, "application/json", response.to_string()).await;
    let mut item = usage("openai:chat");
    item.endpoint_api_format = Some("claude:messages".into());
    item.has_format_conversion = true;
    let (state, _, _) = state(item, &server.url, false, false);
    let (_, payload, _) = replay(&state, json!({})).await;
    assert_eq!(payload["status"], "success", "{payload}");
    let captured = requests.lock().unwrap();
    assert_eq!(captured[0].0, "/v1/messages");
    assert_eq!(captured[0].1["x-api-key"], "sk-replay-current");
    assert_eq!(captured[0].2["messages"][0]["content"], "replay full input");
    assert_eq!(payload["response_body"]["content"][0]["text"], "ok");
}

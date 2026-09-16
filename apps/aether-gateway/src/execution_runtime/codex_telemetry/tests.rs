use super::*;

fn plan() -> ExecutionPlan {
    serde_json::from_value(json!({
        "request_id": "req", "provider_id": "provider", "endpoint_id": "endpoint", "key_id": "key",
        "method": "POST", "url": "https://chatgpt.com/backend-api/codex/responses",
        "headers": {"Authorization": "Bearer test-secret", "ChatGPT-Account-ID": "acct", "user-agent": "codex_cli_rs/0.153.4", "originator": "codex_cli_rs", "cookie": "must-not-leak", "x-aether-execution-follow-redirects": "true"},
        "body": {"json_body": {"model": "model-a", "input": "private prompt"}},
        "client_api_format": "openai:responses", "provider_api_format": "openai:responses",
        "proxy": {"enabled": true, "url": "http://127.0.0.1:18090"}
    })).unwrap()
}

fn observation() -> Observation {
    Observation {
        state: AppState::new().expect("test state"),
        template: telemetry_plan(&plan(), false, json!({})),
        params: json!({"model": "model-a"}),
        started: Instant::now(),
        started_ns: unix_ns().to_string(),
        first_event_ms: None,
        first_token_ms: None,
        pending: Vec::new(),
        event: Vec::new(),
        dropping: false,
        line_nonempty: false,
        live_timestamps: true,
        finished: false,
        status: "interrupted",
        usage: Value::Null,
        response_id: "resp_test".into(),
    }
}

#[test]
fn codex_telemetry_requires_exact_official_responses_and_credentials() {
    let mut input = plan();
    assert!(eligible(&input));
    for url in [
        "http://chatgpt.com/backend-api/codex/responses",
        "https://chatgpt.com.evil.test/backend-api/codex/responses",
        "https://chatgpt.com:18443/backend-api/codex/responses",
        "https://chatgpt.com/backend-api/codex/responses/compact",
        ANALYTICS_URL,
    ] {
        input.url = url.into();
        assert!(!eligible(&input), "{url}");
    }
    input = plan();
    input.headers.remove("Authorization");
    assert!(!eligible(&input));
    input = plan();
    input.client_api_format = "openai:image".into();
    assert!(!eligible(&input));
}

#[test]
fn codex_telemetry_separates_credentials_and_preserves_proxy() {
    let source = plan();
    let analytics = telemetry_plan(&source, false, json!({"events": []}));
    let metrics = telemetry_plan(&source, true, json!({"resourceMetrics": []}));
    assert_eq!(analytics.proxy, source.proxy);
    assert_eq!(metrics.proxy, source.proxy);
    assert_eq!(header(&analytics, "authorization"), "Bearer test-secret");
    assert_eq!(header(&metrics, "authorization"), "");
    assert_eq!(header(&metrics, "chatgpt-account-id"), "");
    assert_eq!(header(&analytics, "cookie"), "");
    assert_eq!(
        header(
            &analytics,
            aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER
        ),
        "false"
    );
    assert!(!serde_json::to_string(&metrics.body)
        .unwrap()
        .contains("private prompt"));
}

#[tokio::test]
async fn codex_telemetry_defaults_off() {
    let state = AppState::new().unwrap();
    assert!(Observation::begin(&state, &plan()).await.is_none());
}

#[test]
fn codex_telemetry_handles_fragmented_terminal_without_text_leakage() {
    let mut observed = observation();
    let event = b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"text\":\"private answer\"}],\"usage\":{\"input_tokens\":12,\"output_tokens\":5,\"output_tokens_details\":{\"reasoning_tokens\":3}}}}\r\n\r\n";
    for chunk in event.chunks(3) {
        observed.observe(chunk);
    }
    assert!(observed.finished);
    assert_eq!(observed.params["status"], "completed");
    assert_eq!(observed.params["reasoning_output_tokens"], 3);
    assert!(!observed.params.to_string().contains("private answer"));
    observed.status = "failed";
    observed.finish();
    assert_eq!(observed.params["status"], "completed");
}

#[test]
fn codex_telemetry_does_not_treat_eof_or_http_success_as_completion() {
    let mut observed = observation();
    observed.http_status(200);
    observed.eof();
    assert_eq!(observed.params["status"], "interrupted");
    let mut observed = observation();
    observed.http_status(429);
    assert_eq!(observed.params["status"], "failed");
}

#[test]
fn codex_telemetry_bounds_oversized_events_and_recovers() {
    let mut observed = observation();
    observed.observe(&vec![b'x'; MAX_EVENT_BYTES + 1]);
    assert!(observed.pending.is_empty());
    assert!(observed.event.is_empty());
    observed.observe(b"\n\ndata: {\"type\":\"response.failed\"}\n\n");
    assert_eq!(observed.params["status"], "failed");
}

#[test]
fn codex_telemetry_handles_terminal_without_final_blank_line() {
    let mut observed = observation();
    observed.observe(b"data: {\"type\":\"response.completed\"}");
    observed.eof();
    assert_eq!(observed.params["status"], "completed");
}

#[test]
fn codex_telemetry_keeps_models_and_histogram_observations_separate() {
    let mut a = observation();
    a.params["duration_ms"] = json!(10000);
    let first = metrics::payload(&a, false);
    a.params["model"] = json!("model-b");
    a.params["duration_ms"] = json!(20000);
    let second = metrics::payload(&a, false);
    let point = "/resourceMetrics/0/scopeMetrics/0/metrics/0/histogram/dataPoints/0";
    assert_eq!(first.pointer(point).unwrap()["count"], "1");
    assert_eq!(first.pointer(point).unwrap()["sum"], 10000);
    assert_eq!(second.pointer(point).unwrap()["sum"], 20000);
    assert!(first.pointer(point).unwrap()["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["key"] == "model" && a["value"]["stringValue"] == "model-a"));
    assert!(second.pointer(point).unwrap()["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["key"] == "model" && a["value"]["stringValue"] == "model-b"));
}

#[test]
fn codex_telemetry_simulation_matches_event_counts_and_catalog() {
    let mut observed = observation();
    observed.params["is_first_turn"] = json!(true);
    let initialization = simulation::initialize(&mut observed);
    assert_eq!(initialization.len(), 4);
    observed.params["shell_command_count"] = json!(1);
    observed.params["dynamic_tool_call_count"] = json!(1);
    observed.params["file_change_count"] = json!(1);
    let events = simulation::terminal(&observed);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event_type"] == "codex_hook_run")
            .count(),
        4
    );
    assert_eq!(events.len(), 9);
    let accepted = events
        .iter()
        .find(|event| event["event_type"] == "codex_accepted_line_fingerprints")
        .unwrap();
    assert!(accepted["event_params"]["repo_hash"].is_null());
    assert!(!events
        .iter()
        .any(|event| event["event_type"] == "codex_turn_steer_event"));
    let startup = metrics::payload(&observed, true);
    let incremental = metrics::payload(&observed, false);
    let pointer = "/resourceMetrics/0/scopeMetrics/0/metrics";
    let names: std::collections::HashSet<&str> = startup
        .pointer(pointer)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .chain(incremental.pointer(pointer).unwrap().as_array().unwrap())
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        startup.pointer(pointer).unwrap().as_array().unwrap().len(),
        62
    );
    assert_eq!(names.len(), 66);
}

#[tokio::test]
async fn codex_telemetry_rejects_invalid_config_without_blocking_model_requests() {
    use crate::data::GatewayDataState;
    let state = AppState::new().unwrap().with_data_state_for_tests(
        GatewayDataState::disabled()
            .with_system_config_values_for_tests([(CONFIG_KEY.to_string(), json!("true"))]),
    );
    assert!(!enabled(&state).await);
}

#[tokio::test]
async fn codex_telemetry_sender_blocks_redirects_on_wire() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut listener = None;
    for port in 19090..19120 {
        if let Ok(bound) =
            tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
        {
            listener = Some(bound);
            break;
        }
    }
    let listener = listener.expect("available five-digit test port");
    let address = listener.local_addr().unwrap();
    let mut request = telemetry_plan(&plan(), true, json!({}));
    request.url = format!("http://{address}/metrics");
    request.proxy = None;
    let receive = async {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0u8; 1024];
            let count = socket.read(&mut chunk).await.unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
        }
        socket.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{address}/redirect\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        String::from_utf8(bytes).unwrap().to_ascii_lowercase()
    };
    let (response, headers) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(send_request(&request, b"{}".to_vec()), receive)
    })
    .await
    .unwrap();
    assert_eq!(response.unwrap().status_code(), 307);
    assert!(headers.contains("statsig-api-key:"));
    assert!(!headers.contains("authorization:"));
    assert!(!headers.contains("chatgpt-account-id:"));
    assert!(!headers.contains("cookie:"));
    assert!(!headers.contains("x-aether-execution-follow-redirects:"));
}

#[test]
fn codex_telemetry_discards_whole_oversized_sse_frame() {
    let mut observed = observation();
    observed.observe(&vec![b'x'; MAX_EVENT_BYTES + 1]);
    observed.observe(b"\ndata: {\"type\":\"response.completed\"}\n\n");
    assert!(!observed.finished);
    observed.observe(b"data: {\"type\":\"response.failed\"}\n\n");
    assert_eq!(observed.params["status"], "failed");
}

#[test]
fn codex_telemetry_batches_keep_resources_separate_and_cleanup_on_drop() {
    let mut first = telemetry_plan(&plan(), true, json!({"resourceMetrics":[{"model":"a"}]}));
    first.key_id = uuid::Uuid::new_v4().to_string();
    let mut guard = batch::enqueue(&first).unwrap();
    let mut second = first.clone();
    second.body = RequestBody::from_json(json!({"resourceMetrics":[{"model":"b"}]}));
    assert!(batch::enqueue(&second).is_none());
    let combined = guard.take().unwrap();
    assert_eq!(
        combined.body.json_body.unwrap()["resourceMetrics"],
        json!([{"model":"a"},{"model":"b"}])
    );
    let guard = batch::enqueue(&first).unwrap();
    drop(guard);
    assert!(batch::enqueue(&first).is_some());
}

#[test]
fn codex_telemetry_accepts_bridged_text_but_excludes_explicit_images() {
    let mut input = plan();
    aether_ai_formats::api::apply_openai_responses_image_generation_bridge_body_edits(
        input.body.json_body.as_mut().unwrap(),
        "openai:responses",
        None,
        true,
    );
    assert!(input.body.json_body.as_ref().unwrap()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["type"] == "image_generation"));
    assert!(eligible(&input));
    for choice in [
        json!("image_generation"),
        json!({"type": "image_generation"}),
    ] {
        input.body.json_body.as_mut().unwrap()["tool_choice"] = choice;
        assert!(!eligible(&input));
    }
    input.body.json_body.as_mut().unwrap()["tool_choice"] = json!("auto");
    input.body.json_body.as_mut().unwrap()["model"] = json!("gpt-image-2");
    assert!(!eligible(&input));
}

#[test]
fn codex_telemetry_reads_complete_json_with_whitespace() {
    let response = json!({"id": "resp_json", "status": "completed",
        "output": [{"text": "private answer"}],
        "usage": {"input_tokens": 12, "output_tokens": 5}});
    for raw in [
        serde_json::to_string(&response).unwrap() + "\n",
        serde_json::to_string_pretty(&response).unwrap(),
    ] {
        let mut observed = observation();
        observed.observe_buffered(raw.as_bytes());
        assert_eq!(observed.params["status"], "completed");
        assert_eq!(observed.params["input_tokens"], 12);
        assert_eq!(observed.response_id, "resp_json");
        assert!(!observed.params.to_string().contains("private answer"));
        assert!(observed.params["explicit_client_interrupt_requested_at_ms"].is_null());
    }
}

#[test]
fn codex_telemetry_read_failure_stays_failed_after_eof() {
    let mut observed = observation();
    observed.observe(b"data: {\"type\":\"response.created\"}\n\n");
    observed.fail();
    observed.eof();
    assert_eq!(observed.params["status"], "failed");
    assert!(observed.params["explicit_client_interrupt_requested_at_ms"].is_null());
}

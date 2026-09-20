use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn candidate(state: &AppState, p: &ExecutionPlan) -> Box<passive::Candidate> {
    let prepared = prepare(state, p).await.ok().flatten().unwrap();
    let mut headers = success().unwrap().headers;
    headers.insert("content-type".into(), "text/event-stream".into());
    prepared
        .observe(state, 200, &mut headers)
        .await
        .ok()
        .flatten()
        .unwrap()
}

async fn cached(state: &AppState, p: &ExecutionPlan) -> Option<String> {
    state
        .runtime_state
        .kv_get(&format!("codex-state:active:{}", state_scope(state, p)))
        .await
        .unwrap()
}

#[tokio::test]
async fn formal_stream_requires_complete_consumption_and_reports_source() {
    for cancel in [true, false] {
        let state = configured_state("codex", "oauth");
        let p = plan();
        let candidate = candidate(&state, &p).await;
        let mut execution = StateError::unavailable().stream(&p);
        execution.status_code = 200;
        execution.headers = BTreeMap::from([("content-type".into(), "text/event-stream".into())]);
        execution.response = DirectUpstreamResponse::Buffered(Bytes::from(success().unwrap().body));
        execution.codex_state_candidate = Some(candidate);
        let mut stream =
            Box::pin(crate::execution_runtime::build_direct_execution_frame_stream(execution));
        if cancel {
            stream.next().await.unwrap().unwrap();
            stream.next().await.unwrap().unwrap();
            drop(stream);
            assert!(cached(&state, &p).await.is_none());
        } else {
            while let Some(frame) = stream.next().await {
                frame.unwrap();
            }
            assert!(cached(&state, &p).await.is_some());
            let snapshot = diagnostics::read(&state, "provider", "account-a")
                .await
                .ok()
                .unwrap();
            assert_eq!(snapshot["items"][0]["source"], "response");
            assert_eq!(snapshot["items"][0]["last_use"]["mode"], "passthrough");
            assert_eq!(snapshot["items"][0]["last_use"]["observed_blocks"], 10);
            assert!(!snapshot
                .to_string()
                .contains(&success().unwrap().headers[HEADER]));
        }
    }
}

#[tokio::test]
async fn formal_capacity_incomplete_malformed_and_read_failure_never_publish() {
    let completed = String::from_utf8(success().unwrap().body).unwrap();
    for (body, read_failure) in [
        ("data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"model_at_capacity\"}}}\n\n".to_string(), false),
        ("data: {\"type\":\"response.incomplete\"}\n\n".to_string(), false),
        ("data: {\"type\":\"response.created\"}\n\n".to_string(), false),
        (format!("{completed}data: {{\"type\":\"error\"}}\n\n"), false),
        (format!("{completed}data: {{"), false),
        (completed.clone(), true),
    ] {
        let state = configured_state("codex", "oauth");
        let p = plan();
        let mut c = candidate(&state, &p).await;
        for chunk in body.as_bytes().chunks(3) { c.observe(chunk); }
        if read_failure { c.fail(); }
        c.finish().await;
        assert!(cached(&state, &p).await.is_none(), "{body}, read_failure={read_failure}");
    }
}

#[tokio::test]
async fn passive_preserves_active_cache_and_shares_probe_lock() {
    for busy in [false, true] {
        let state = configured_state("codex", "oauth");
        let p = plan();
        let mut c = candidate(&state, &p).await;
        let lease = if busy {
            let account = digest(json!(["provider", "account-a", "workspace"]).to_string());
            Some(
                state
                    .runtime_state
                    .lock_try_acquire(
                        &format!("codex-state:collect:{account}"),
                        "probe",
                        LEASE_TTL,
                    )
                    .await
                    .unwrap()
                    .unwrap(),
            )
        } else {
            prepare_with_probe(&state, &p, |_| async { success() })
                .await
                .ok()
                .unwrap();
            None
        };
        let before = cached(&state, &p).await;
        c.observe(&success().unwrap().body);
        c.finish().await;
        assert_eq!(cached(&state, &p).await, before);
        if let Some(lease) = lease {
            state.runtime_state.lock_release(&lease).await.unwrap();
        }
    }
}

#[tokio::test]
async fn passive_revalidates_configuration_switch_and_guards_before_publishing() {
    for change in ["configuration", "disabled", "auth", "rate"] {
        let state = configured_state("codex", "oauth");
        let p = plan();
        let mut c = candidate(&state, &p).await;
        match change {
            "configuration" => {
                state
                    .upsert_system_config_json_value(
                        "system_proxy_node_id",
                        &json!("changed"),
                        None,
                    )
                    .await
                    .unwrap();
            }
            "disabled" => {
                state
                    .upsert_system_config_json_value(CONFIG_KEY, &json!(false), None)
                    .await
                    .unwrap();
            }
            _ => {
                let guard = format!(
                    "codex-state:guard:{}",
                    digest(json!(["provider", "account-a", "workspace"]).to_string())
                );
                reject(
                    &state,
                    &guard,
                    &digest("Bearer secret"),
                    if change == "auth" { 401 } else { 429 },
                    &BTreeMap::new(),
                )
                .await
                .ok()
                .unwrap();
            }
        }
        c.observe(&success().unwrap().body);
        c.finish().await;
        assert!(cached(&state, &p).await.is_none(), "{change}");
    }
}

#[tokio::test]
async fn successful_formal_sync_response_populates_cache_for_next_request() {
    let state = configured_state("codex", "oauth");
    let p = plan();
    let prepared = prepare(&state, &p).await.ok().flatten().unwrap();
    assert!(!prepared.injected);
    let listener = crate::test_support::bind_loopback_listener().await.unwrap();
    let mut dispatch = prepared.plan.clone();
    dispatch.url = format!("http://{}", listener.local_addr().unwrap());
    let returned = success().unwrap().headers[HEADER].clone();
    let serve = async {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let text = String::from_utf8_lossy(&bytes[..end]);
                let length: usize = text
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let body =
            r#"{"object":"response","status":"completed","output":[],"usage":{"output_tokens":1}}"#;
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nx-codex-turn-state: {returned}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    };
    let execute = async {
        let result = DirectSyncExecutionRuntime::new()
            .execute_sync_observed(&dispatch, Some((&state, &prepared)))
            .await
            .unwrap();
        assert_eq!(result.status_code, 200);
        assert!(!result.headers.contains_key(HEADER));
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(serve, execute);
    })
    .await
    .unwrap();
    let next = prepare_with_probe(&state, &p, unexpected_probe)
        .await
        .ok()
        .flatten()
        .expect("normal successful response should seed the cache");
    assert!(next.injected);
    assert_eq!(next.plan.headers[HEADER], returned);
}

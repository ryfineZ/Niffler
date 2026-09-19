use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Copy)]
enum BodyFailure {
    Truncated,
    Pending,
    InvalidJson,
}

// 不派生服务任务：测试 future 完成、失败或超时都会直接释放监听器和连接。
async fn bad_state_response(
    listener: tokio::net::TcpListener,
    failure: BodyFailure,
    done: tokio::sync::oneshot::Receiver<()>,
) {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        let n = socket.read(&mut chunk).await.unwrap();
        assert_ne!(n, 0);
        request.extend_from_slice(&chunk[..n]);
        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            let header_text = String::from_utf8_lossy(&request[..end]);
            let length: usize = header_text
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            if request.len() >= end + 4 + length {
                break;
            }
        }
    }
    let length = if matches!(failure, BodyFailure::InvalidJson) {
        1
    } else {
        100
    };
    socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-codex-turn-state: invalid-state\r\ncontent-length: {length}\r\n\r\n{{").as_bytes()).await.unwrap();
    if matches!(failure, BodyFailure::Truncated | BodyFailure::InvalidJson) {
        socket.shutdown().await.unwrap();
    }
    let _ = done.await;
}

async fn assert_sync_stops_before_body_failure(failure: BodyFailure, main_entry: bool) {
    let state = configured_state("codex", "oauth");
    let listener = crate::test_support::bind_loopback_listener().await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(addr.port() >= 10000);
    let mut p = plan();
    p.proxy = Some(
        serde_json::from_value(
            json!({"enabled":true,"mode":"tunnel","node_id":"remote-node",
        "extra":{"tunnel_base_url":format!("http://{addr}")}}),
        )
        .unwrap(),
    );
    p.timeouts = Some(ExecutionTimeouts {
        total_ms: Some(5000),
        ..Default::default()
    });
    let prepared = prepare_with_probe(&state, &p, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let execute = async {
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            if main_entry {
                let decision = crate::control::GatewayControlDecision::synthetic(
                    "/v1/responses",
                    Some("ai_public".into()),
                    Some("openai".into()),
                    Some("responses".into()),
                    Some("openai:responses".into()),
                )
                .with_execution_runtime_candidate(true);
                let response = crate::execution_runtime::sync::execute_execution_runtime_sync(
                    &state,
                    "/v1/responses",
                    p.clone(),
                    "state-regression",
                    &decision,
                    "openai_responses_sync",
                    None,
                    None,
                )
                .await
                .expect("sync entry should return a terminal response")
                .expect("invalid state must not return None and dispatch the next account");
                assert_eq!(response.status(), 503);
                let bytes = axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap();
                serde_json::from_slice::<Value>(&bytes).unwrap()
            } else {
                let result =
                    crate::execution_runtime::transport::execute_sync_plan(&state, None, &p)
                        .await
                        .expect("state error must win over body failure");
                assert_eq!(result.status_code, 503);
                result.body.unwrap().json_body.unwrap()
            }
        })
        .await;
        let _ = done_tx.send(());
        result
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(4), async {
        tokio::join!(execute, bad_state_response(listener, failure, done_rx))
    })
    .await
    .expect("test connection must finish and be released");
    let body = result.expect("invalid state headers must be handled without waiting for body");
    assert_eq!(body["error"]["code"], "codex_turn_state_shape_changed");
    assert_eq!(body["error"]["upstream_request_sent"], true);
    assert!(terminal_error(Some(&body.to_string())));
    assert!(state
        .runtime_state
        .kv_get(&prepared.cache_key)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sync_main_stops_on_bad_state_before_truncated_body() {
    assert_sync_stops_before_body_failure(BodyFailure::Truncated, true).await;
}

#[tokio::test]
async fn sync_main_stops_on_bad_state_without_waiting_for_body() {
    assert_sync_stops_before_body_failure(BodyFailure::Pending, true).await;
}

#[tokio::test]
async fn sync_retry_entry_stops_on_bad_state_before_json_decode() {
    assert_sync_stops_before_body_failure(BodyFailure::InvalidJson, false).await;
}

#[tokio::test]
async fn local_tunnel_sync_stops_on_bad_state_without_waiting_for_body() {
    use crate::tunnel::{tunnel_protocol, TunnelProxyConn};
    use axum::extract::ws::Message;
    let state = configured_state("codex", "oauth");
    let tunnel_app = state.tunnel.app_state();
    let (proxy_tx, mut proxy_rx) = aether_runtime::bounded_queue(8);
    let (close_tx, _) = tokio::sync::watch::channel(false);
    tunnel_app
        .hub
        .register_proxy(std::sync::Arc::new(TunnelProxyConn::new(
            701,
            "local-node".into(),
            "Local node".into(),
            proxy_tx,
            close_tx,
            16,
            2,
        )));
    let mut p = plan();
    p.proxy = Some(
        serde_json::from_value(json!({"enabled":true,"mode":"tunnel","node_id":"local-node"}))
            .unwrap(),
    );
    let prepared = prepare_with_probe(&state, &p, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let execute = async {
        let result = crate::execution_runtime::transport::execute_sync_plan(&state, None, &p).await;
        let _ = done_tx.send(());
        result
    };
    let respond = async {
        let Message::Binary(data) = proxy_rx.recv().await.unwrap() else {
            panic!("expected headers");
        };
        let request = tunnel_protocol::FrameHeader::parse(&data).unwrap();
        assert_eq!(request.msg_type, tunnel_protocol::REQUEST_HEADERS);
        let meta = tunnel_protocol::ResponseMeta {
            status: 200,
            headers: vec![
                (HEADER.into(), "invalid-state".into()),
                ("content-type".into(), "application/json".into()),
            ],
        };
        let mut frame = tunnel_protocol::encode_frame(
            request.stream_id,
            tunnel_protocol::RESPONSE_HEADERS,
            0,
            &serde_json::to_vec(&meta).unwrap(),
        );
        tunnel_app.hub.handle_proxy_frame(701, &mut frame).await;
        // 不发送正文或 STREAM_END；只有响应头也必须完成拦截。
        let _ = done_rx.await;
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(execute, respond)
    })
    .await
    .expect("local tunnel must not wait for body");
    let result = result.unwrap();
    assert_eq!(result.status_code, 503);
    let body = result.body.unwrap().json_body.unwrap();
    assert_eq!(body["error"]["code"], "codex_turn_state_shape_changed");
    assert_eq!(body["error"]["upstream_request_sent"], true);
    assert!(state
        .runtime_state
        .kv_get(&prepared.cache_key)
        .await
        .unwrap()
        .is_none());
}

async fn assert_direct_transport_observes_headers(backend: &str) {
    let state = AppState::new().unwrap();
    let listener = crate::test_support::bind_loopback_listener().await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(addr.port() >= 10000);
    // 直接构造已准备快照，只把网络目标指向本地故障服务；不做真实账号采集。
    let mut p = plan();
    p.url = format!("http://{addr}/responses");
    p.transport_profile = Some(serde_json::from_value(json!({
        "profile_id":"chrome136", "backend":backend, "http_mode":"http1_only", "pool_scope":"key",
        "extra":{"browser_profile":"chrome136"}
    })).unwrap());
    p.timeouts = Some(ExecutionTimeouts {
        total_ms: Some(5000),
        ..Default::default()
    });
    let prepared = Prepared {
        plan: p,
        cache_key: "state".into(),
        cached_value: "version".into(),
        guard_key: "guard".into(),
        credential: "credential".into(),
        blocks: 10,
    };
    state
        .runtime_state
        .kv_set("state", "version", Some(Duration::from_secs(60)))
        .await
        .unwrap();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let execute = async {
        let result = DirectSyncExecutionRuntime::new()
            .execute_sync_observed(&prepared.plan, Some((&state, &prepared)))
            .await;
        let _ = done_tx.send(());
        result
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            execute,
            bad_state_response(listener, BodyFailure::Pending, done_rx)
        )
    })
    .await
    .expect("transport must inspect headers before reading body");
    let result = result.unwrap();
    assert_eq!(result.status_code, 503);
    assert_eq!(
        result.body.unwrap().json_body.unwrap()["error"]["code"],
        "codex_turn_state_shape_changed"
    );
    assert!(state.runtime_state.kv_get("state").await.unwrap().is_none());
}

#[tokio::test]
async fn reqwest_sync_observes_bad_state_before_reading_body() {
    assert_direct_transport_observes_headers(aether_contracts::TRANSPORT_BACKEND_REQWEST_RUSTLS)
        .await;
}

#[tokio::test]
async fn browser_sync_observes_bad_state_before_reading_body() {
    assert_direct_transport_observes_headers(aether_contracts::TRANSPORT_BACKEND_BROWSER_WREQ)
        .await;
}

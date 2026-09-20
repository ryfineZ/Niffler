use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn plan() -> ExecutionPlan {
    serde_json::from_value(json!({"request_id":"compact-request","provider_id":"provider","endpoint_id":"endpoint","key_id":"account",
        "method":"POST","url":"https://chatgpt.com/backend-api/codex/responses/compact",
        "headers":{"authorization":"Bearer secret","chatgpt-account-id":"workspace","accept":"application/json"},
        "body":{"json_body":{"model":"gpt-5.6-sol","input":[{"role":"user","content":"history"}]}},
        "client_api_format":"openai:responses:compact","provider_api_format":"openai:responses:compact"})).unwrap()
}

fn state(provider_type: &str, auth_type: &str) -> AppState {
    use aether_data::repository::provider_catalog::InMemoryProviderCatalogReadRepository;
    use aether_data_contracts::repository::provider_catalog::{
        StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
    };
    let catalog = InMemoryProviderCatalogReadRepository::seed(
        vec![StoredProviderCatalogProvider::new(
            "provider".into(),
            "Provider".into(),
            None,
            provider_type.into(),
        )
        .unwrap()],
        vec![StoredProviderCatalogEndpoint::new(
            "endpoint".into(),
            "provider".into(),
            "openai:responses:compact".into(),
            Some("openai".into()),
            Some("compact".into()),
            true,
        )
        .unwrap()],
        vec![StoredProviderCatalogKey::new(
            "account".into(),
            "provider".into(),
            "Account".into(),
            auth_type.into(),
            None,
            true,
        )
        .unwrap()],
    );
    AppState::new().unwrap().with_data_state_for_tests(
        crate::data::GatewayDataState::with_provider_transport_reader_for_tests(
            std::sync::Arc::new(catalog),
            "test-key",
        ),
    )
}

fn completed() -> Vec<u8> {
    format!("data: {}\n\n", json!({"type":"response.completed","response":{"status":"completed","id":"cmp1","usage":{"input_tokens":12,"output_tokens":4},"output":[{"type":"message","role":"user","content":[]},{"type":"compaction","encrypted_content":"real-opaque-result"}]}})).into_bytes()
}

#[test]
fn legacy_bridge_preserves_history_and_fields_without_duplicate_trigger() {
    let mut p = plan();
    let body = p.body.json_body.as_mut().unwrap();
    body["instructions"] = json!("preserve");
    body["reasoning"] = json!({"effort":"high"});
    body["tools"] = json!([{ "type":"function", "name":"work" }]);
    body["prompt_cache_key"] = json!("thread");
    body["service_tier"] = json!("priority");
    body["tool_choice"] = json!("none");
    let original = body.clone();
    p.url.push_str("?tenant=1");
    protocol::bridge_request(&mut p).unwrap();
    assert_eq!(
        p.url,
        "https://chatgpt.com/backend-api/codex/responses?tenant=1"
    );
    let body = p.body.json_body.as_ref().unwrap();
    for key in [
        "instructions",
        "reasoning",
        "tools",
        "prompt_cache_key",
        "service_tier",
        "tool_choice",
    ] {
        assert_eq!(body[key], original[key]);
    }
    assert_eq!(body["input"][0], original["input"][0]);
    assert!(is_v2(body));
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    p.url = "https://chatgpt.com/backend-api/codex/v1/responses/compact".into();
    protocol::bridge_request(&mut p).unwrap();
    assert_eq!(
        p.body.json_body.unwrap()["input"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn bridge_requires_real_completed_compaction_and_preserves_usage() {
    let result = protocol::bridge_response(&completed()).unwrap();
    assert_eq!(result["object"], "response.compaction");
    assert_eq!(
        result["output"][1]["encrypted_content"],
        "real-opaque-result"
    );
    assert_eq!(result["usage"]["input_tokens"], 12);
    for response in [
        json!({"type":"response.created"}),
        json!({"type":"response.completed","response":{"output":[]}}),
        json!({"type":"response.completed","response":{"output":[{"type":"compaction"}]}}),
        json!({"type":"response.completed","response":{"status":"incomplete","output":[{"type":"compaction","encrypted_content":"x"}]}}),
        json!({"type":"response.completed","response":{"output":[{"type":"compaction","encrypted_content":"x"},{"type":"compaction","encrypted_content":"y"}]}}),
        json!({"type":"response.failed","response":{"error":{"code":"rate_limit_exceeded"}}}),
    ] {
        let error =
            protocol::bridge_response(format!("data: {response}\n\n").as_bytes()).unwrap_err();
        assert!(error.sent);
    }
    let mut failed_after_success = completed();
    failed_after_success.extend_from_slice(b"data: {\"type\":\"error\"}\n\n");
    assert!(protocol::bridge_response(&failed_after_success).is_err());
}

#[test]
fn streamed_compaction_output_requires_completion_and_terminal_errors_stop_failover() {
    let sse = b"data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"x\"}}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n";
    assert_eq!(
        protocol::bridge_response(sse).unwrap()["output"][0]["encrypted_content"],
        "x"
    );
    let failed = CompactError::incomplete().response(&plan());
    let DirectUpstreamResponse::Buffered(body) = failed.response else {
        panic!()
    };
    assert!(terminal_error(std::str::from_utf8(&body).ok()));
    assert!(!terminal_error(Some(
        r#"{"error":{"code":"server_error"}}"#
    )));
}

// listener/连接均由本次 future 持有；无后台服务，失败或取消时自动释放。
async fn relay_once(
    listener: tokio::net::TcpListener,
    status: u16,
    body: Vec<u8>,
    partial: bool,
) -> Vec<u8> {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let len = socket.read(&mut buffer).await.unwrap();
        assert_ne!(len, 0);
        request.extend_from_slice(&buffer[..len]);
        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            let text = String::from_utf8_lossy(&request[..end]);
            let content_length = text
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if request.len() >= end + 4 + content_length {
                break;
            }
        }
    }
    let length = body.len() + if partial { 100 } else { 0 };
    socket.write_all(format!("HTTP/1.1 {status} Test\r\ncontent-type: text/event-stream\r\nx-codex-turn-state: invalid-for-generation\r\ncontent-length: {length}\r\nconnection: close\r\n\r\n").as_bytes()).await.unwrap();
    socket.write_all(&body).await.unwrap();
    socket.shutdown().await.unwrap();
    request
}

async fn relay_plan() -> (tokio::net::TcpListener, ExecutionPlan) {
    let listener = crate::test_support::bind_loopback_listener().await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(addr.port() >= 10000);
    let mut p = plan();
    p.proxy = Some(serde_json::from_value(json!({"enabled":true,"mode":"tunnel","node_id":"remote","extra":{"tunnel_base_url":format!("http://{addr}")}})).unwrap());
    p.timeouts = Some(aether_contracts::ExecutionTimeouts {
        total_ms: Some(3000),
        ..Default::default()
    });
    (listener, p)
}

#[tokio::test]
async fn legacy_sync_bridge_dispatches_once_with_state_off_and_ignores_state_shape() {
    let state = state("codex", "oauth");
    let (listener, p) = relay_plan().await;
    let (result, request) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            transport::execute_sync_plan(&state, None, &p),
            relay_once(listener, 200, completed(), false)
        )
    })
    .await
    .expect("mock connection must finish");
    let result = result.unwrap();
    assert_eq!(result.status_code, 200);
    assert_eq!(result.headers["x-niffler-compaction"], "v1-to-v2");
    assert!(!result.headers.contains_key("x-codex-turn-state"));
    assert_eq!(result.headers["x-niffler-turn-state"], "not_applicable");
    assert_eq!(
        result.body.unwrap().json_body.unwrap()["object"],
        "response.compaction"
    );
    let request = String::from_utf8_lossy(&request);
    assert!(request.contains("compaction_trigger"));
    assert!(!request.contains("Reply with OK."));
    assert!(!request.contains("x-codex-turn-state"));
    assert!(!request.contains("/responses/compact"));
}

#[tokio::test]
async fn legacy_stream_bridge_is_buffered_and_partial_response_is_terminal() {
    let state = state("codex", "oauth");
    for partial in [false, true] {
        let (listener, mut p) = relay_plan().await;
        p.stream = true;
        let (result, _) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                maybe_execute_stream(&state, &p),
                relay_once(listener, 200, completed(), partial)
            )
        })
        .await
        .expect("mock connection must finish");
        let result = result.unwrap();
        assert_eq!(result.status_code, if partial { 502 } else { 200 });
        let DirectUpstreamResponse::Buffered(bytes) = result.response else {
            panic!("legacy output must be complete")
        };
        if partial {
            assert!(terminal_error(std::str::from_utf8(&bytes).ok()));
        } else {
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()["object"],
                "response.compaction"
            );
        }
    }
}

#[tokio::test]
async fn native_compact_with_state_enabled_never_probes_or_applies_shape_rules() {
    let state = state("codex", "oauth");
    state
        .upsert_system_config_json_value(codex_turn_state::CONFIG_KEY, &json!(true), None)
        .await
        .unwrap();
    let (listener, mut p) = relay_plan().await;
    protocol::bridge_request(&mut p).unwrap();
    p.provider_api_format = "openai:responses".into();
    p.client_api_format = "openai:responses".into();
    let (result, request) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            maybe_execute_sync(&state, &p),
            relay_once(listener, 200, completed(), false)
        )
    })
    .await
    .expect("mock connection must finish");
    let result = result.unwrap();
    assert_eq!(result.status_code, 200);
    assert_eq!(result.headers["x-niffler-compaction"], "v2");
    assert!(!result.headers.contains_key("x-codex-turn-state"));
    assert!(!String::from_utf8_lossy(&request).contains("Reply with OK."));
}

#[tokio::test]
async fn api_keys_and_other_providers_remain_on_original_compact_protocol() {
    for (provider, auth) in [("custom", "oauth"), ("codex", "api_key")] {
        assert!(maybe_execute_sync(&state(provider, auth), &plan())
            .await
            .is_none());
    }
    let mut p = plan();
    p.url = "https://relay.example/responses/compact".into();
    assert!(maybe_execute_sync(&state("codex", "oauth"), &p)
        .await
        .is_none());
}

#[tokio::test]
async fn public_sync_and_stream_entries_keep_compaction_output_and_stop_failed_requests() {
    for streaming in [false, true] {
        for status in [200, 401, 429] {
            let state = state("codex", "oauth");
            let (listener, mut p) = relay_plan().await;
            p.stream = streaming;
            let decision = crate::control::GatewayControlDecision::synthetic(
                "/v1/responses/compact",
                Some("ai_public".into()),
                Some("openai".into()),
                Some("compact".into()),
                Some("openai:responses:compact".into()),
            )
            .with_execution_runtime_candidate(true);
            let execute = async {
                let response = if streaming {
                    crate::execution_runtime::execute_execution_runtime_stream(
                        &state,
                        p,
                        "compact-regression",
                        &decision,
                        "openai_responses_compact_stream",
                        None,
                        None,
                    )
                    .await
                } else {
                    crate::execution_runtime::execute_execution_runtime_sync(
                        &state,
                        "/v1/responses/compact",
                        p,
                        "compact-regression",
                        &decision,
                        "openai_responses_compact_sync",
                        None,
                        None,
                    )
                    .await
                }
                .unwrap()
                .expect("compact must finish without another candidate");
                assert_eq!(response.status().as_u16(), status);
                assert_eq!(
                    response.headers()["x-niffler-compaction"],
                    if status == 200 { "v1-to-v2" } else { "failed" }
                );
                if status == 200 {
                    assert_eq!(
                        response.headers()["x-niffler-compaction-egress"],
                        "configured"
                    );
                }
                assert!(!response.headers().contains_key("x-codex-turn-state"));
                let bytes = axum::body::to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap();
                let text = String::from_utf8_lossy(&bytes);
                if status == 200 {
                    assert!(text.contains("real-opaque-result"), "{text}");
                    if streaming {
                        assert!(text.contains("response.completed"), "{text}");
                    } else {
                        assert_eq!(
                            serde_json::from_slice::<Value>(&bytes).unwrap()["object"],
                            "response.compaction"
                        );
                    }
                } else {
                    assert!(terminal_error(Some(&text)), "{text}");
                }
            };
            tokio::time::timeout(Duration::from_secs(6), async {
                tokio::join!(execute, relay_once(listener, status, completed(), false))
            })
            .await
            .expect("both mock and client must release connections");
        }
    }
}

#[tokio::test]
async fn decoded_response_limit_and_compressed_response_are_checked() {
    use std::io::Write;
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&completed()).unwrap();
    let headers = BTreeMap::from([("content-encoding".into(), "gzip".into())]);
    let bytes = read_body(
        DirectUpstreamResponse::Buffered(Bytes::from(gzip.finish().unwrap())),
        &headers,
    )
    .await
    .unwrap();
    assert!(protocol::bridge_response(&bytes).is_ok());
    assert_eq!(
        decode(std::io::repeat(0).take(protocol::MAX_BYTES as u64 + 1))
            .unwrap_err()
            .code,
        "codex_compact_response_too_large"
    );
    assert!(read_body(
        DirectUpstreamResponse::Buffered(Bytes::from_static(b"invalid gzip")),
        &headers
    )
    .await
    .is_err());
}

#[tokio::test]
async fn deflate_accepts_zlib_and_raw_compressed_responses() {
    use std::io::Write;
    let expected = completed();
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    zlib.write_all(&expected).unwrap();
    let mut raw = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    raw.write_all(&expected).unwrap();
    let headers = BTreeMap::from([("content-encoding".into(), "deflate".into())]);
    for compressed in [zlib.finish().unwrap(), raw.finish().unwrap()] {
        let decoded = read_body(
            DirectUpstreamResponse::Buffered(Bytes::from(compressed)),
            &headers,
        )
        .await
        .unwrap();
        assert_eq!(decoded, expected);
    }
}

#[tokio::test]
async fn legacy_bridge_runs_on_reqwest_and_browser_transports() {
    for backend in [
        aether_contracts::TRANSPORT_BACKEND_REQWEST_RUSTLS,
        aether_contracts::TRANSPORT_BACKEND_BROWSER_WREQ,
    ] {
        let state = state("codex", "oauth");
        let original = plan();
        let route = compact_route::prepare(&state, &original)
            .await
            .ok()
            .unwrap();
        let mut dispatch = original.clone();
        protocol::bridge_request(&mut dispatch).unwrap();
        let listener = crate::test_support::bind_loopback_listener().await.unwrap();
        dispatch.url = format!("http://{}/responses", listener.local_addr().unwrap());
        dispatch.transport_profile = Some(serde_json::from_value(json!({"profile_id":"chrome136","backend":backend,"http_mode":"http1_only","pool_scope":"key","extra":{"browser_profile":"chrome136"}})).unwrap());
        let (result, _) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                execute(&state, &original, &dispatch, &route, true),
                relay_once(listener, 200, completed(), false)
            )
        })
        .await
        .unwrap();
        let result = result.unwrap();
        assert_eq!(result.status_code, 200);
        let DirectUpstreamResponse::Buffered(bytes) = result.response else {
            panic!()
        };
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["object"],
            "response.compaction"
        );
    }
}

#[tokio::test]
async fn legacy_bridge_runs_on_local_tunnel_without_shape_checks() {
    use crate::tunnel::{tunnel_protocol, TunnelProxyConn};
    use axum::extract::ws::Message;
    let state = state("codex", "oauth");
    let app = state.tunnel.app_state();
    let (tx, mut rx) = aether_runtime::bounded_queue(8);
    let (close, _) = tokio::sync::watch::channel(false);
    app.hub
        .register_proxy(std::sync::Arc::new(TunnelProxyConn::new(
            703,
            "local-node".into(),
            "Local".into(),
            tx,
            close,
            16,
            2,
        )));
    let mut p = plan();
    p.proxy = Some(
        serde_json::from_value(json!({"mode":"tunnel","node_id":"local-node","enabled":true}))
            .unwrap(),
    );
    let respond = async {
        let Message::Binary(data) = rx.recv().await.unwrap() else {
            panic!()
        };
        let request = tunnel_protocol::FrameHeader::parse(&data).unwrap();
        let meta = tunnel_protocol::ResponseMeta {
            status: 200,
            headers: vec![
                ("content-type".into(), "text/event-stream".into()),
                ("x-codex-turn-state".into(), "invalid".into()),
            ],
        };
        for (kind, payload) in [
            (
                tunnel_protocol::RESPONSE_HEADERS,
                serde_json::to_vec(&meta).unwrap(),
            ),
            (tunnel_protocol::RESPONSE_BODY, completed()),
            (tunnel_protocol::STREAM_END, Vec::new()),
        ] {
            let mut frame = tunnel_protocol::encode_frame(request.stream_id, kind, 0, &payload);
            app.hub.handle_proxy_frame(703, &mut frame).await;
        }
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(maybe_execute_sync(&state, &p), respond)
    })
    .await
    .unwrap();
    let result = result.unwrap();
    assert_eq!(result.status_code, 200);
    assert_eq!(
        result.body.unwrap().json_body.unwrap()["output"][1]["encrypted_content"],
        "real-opaque-result"
    );
}

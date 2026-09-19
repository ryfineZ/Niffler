use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn public_sync_and_stream_forward_without_state_during_collection_cooldown() {
    for streaming in [false, true] {
        let state = configured_state("codex", "oauth");
        let account = digest(json!(["provider", "account-a", "workspace"]).to_string());
        state
            .runtime_state
            .kv_set(
                &format!("codex-state:cooldown:{account}"),
                "cooling-down",
                Some(Duration::from_secs(60)),
            )
            .await
            .unwrap();
        // 不启动后台服务；超时或测试退出时直接释放监听器和连接。
        let listener = crate::test_support::bind_loopback_listener().await.unwrap();
        let mut p = plan();
        p.stream = streaming;
        p.proxy = Some(
            serde_json::from_value(
                json!({"enabled":true,"mode":"tunnel","node_id":"remote-node",
            "extra":{"tunnel_base_url":format!("http://{}", listener.local_addr().unwrap())}}),
            )
            .unwrap(),
        );
        let decision = crate::control::GatewayControlDecision::synthetic(
            "/v1/responses",
            Some("ai_public".into()),
            Some("openai".into()),
            Some("responses".into()),
            Some("openai:responses".into()),
        )
        .with_execution_runtime_candidate(true);
        let response = json!({"id":"resp_passthrough","object":"response","status":"completed","output":[
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"forwarded answer"}]}
        ],"usage":{"input_tokens":1,"output_tokens":2}});
        let body = if streaming {
            format!(
                "data: {}\n\n",
                json!({"type":"response.completed","response":response})
            )
        } else {
            response.to_string()
        };
        let execute = async {
            let response = if streaming {
                crate::execution_runtime::execute_execution_runtime_stream(
                    &state,
                    p,
                    "passthrough-regression",
                    &decision,
                    "openai_responses_stream",
                    None,
                    None,
                )
                .await
            } else {
                crate::execution_runtime::execute_execution_runtime_sync(
                    &state,
                    "/v1/responses",
                    p,
                    "passthrough-regression",
                    &decision,
                    "openai_responses_sync",
                    None,
                    None,
                )
                .await
            }
            .unwrap()
            .expect("passthrough must finish the selected request");
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["x-niffler-turn-state"], "passthrough");
            assert!(!response.headers().contains_key(HEADER));
            let bytes = axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&bytes).contains("forwarded answer"));
        };
        let serve = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0u8; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                assert_ne!(n, 0);
                request.extend_from_slice(&chunk[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length: usize = headers
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
            let request = String::from_utf8_lossy(&request);
            assert!(request.contains("private prompt"));
            assert!(!request.contains("Reply with OK."));
            assert!(!request.contains("client-state"));
            let content_type = if streaming {
                "text/event-stream"
            } else {
                "application/json"
            };
            // 正常转发不能因上游返回不合格 state 而丢掉有效正文。
            socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\nx-codex-turn-state: invalid-state\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(execute, serve)
        })
        .await
        .expect("mock and client must finish and release connections");
    }
}

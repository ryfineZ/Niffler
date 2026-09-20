//! 有界读取维护探测；只返回固定类别与时间，不保存错误文本或 State 原文到诊断。
use super::*;

#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct ProbeObservation {
    pub phase: &'static str,
    pub dispatched: Option<bool>,
    pub http_status: u16,
    pub elapsed_ms: u64,
    pub headers_ms: Option<u64>,
    pub first_byte_ms: Option<u64>,
    pub completed: bool,
    pub returned_state: &'static str,
}

pub(super) struct ProbeResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub observation: ProbeObservation,
}
impl ProbeResponse {
    #[cfg(test)]
    pub fn new(status: u16, headers: BTreeMap<String, String>, body: Vec<u8>) -> Self {
        Self {
            status,
            headers,
            body,
            observation: ProbeObservation {
                dispatched: Some(true),
                http_status: status,
                ..Default::default()
            },
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ProbeFailure {
    pub reason: &'static str,
    pub observation: ProbeObservation,
}
impl ProbeFailure {
    pub fn new(reason: &'static str, phase: &'static str) -> Self {
        Self {
            reason,
            observation: ProbeObservation {
                phase,
                dispatched: Some(false),
                ..Default::default()
            },
        }
    }
    #[cfg(test)]
    pub fn transport() -> Self {
        Self::new("transport_error", "dispatch")
    }
}
pub(super) type ProbeResult = Result<ProbeResponse, ProbeFailure>;

#[derive(Default)]
pub(super) struct ProbeStream {
    pub data: Vec<u8>,
    event_start: usize,
    line_has_content: bool,
    pub terminal: bool,
}
impl ProbeStream {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        for &byte in bytes {
            if self.terminal {
                break;
            }
            if self.data.len() >= MAX_PROBE_BYTES {
                return Err("body_too_large");
            }
            self.data.push(byte);
            if byte == b'\n' {
                if !self.line_has_content {
                    let event = std::str::from_utf8(&self.data[self.event_start..])
                        .map_err(|_| "invalid_response")?;
                    let data = event
                        .lines()
                        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !data.is_empty() && data != "[DONE]" {
                        let value: Value =
                            serde_json::from_str(&data).map_err(|_| "invalid_response")?;
                        self.terminal = matches!(
                            value.get("type").and_then(Value::as_str).or_else(|| event
                                .lines()
                                .find_map(|line| line.strip_prefix("event:").map(str::trim))),
                            Some(
                                "response.completed"
                                    | "response.failed"
                                    | "response.incomplete"
                                    | "error"
                            )
                        );
                    }
                    self.event_start = self.data.len();
                }
                self.line_has_content = false;
            } else if byte != b'\r' {
                self.line_has_content = true;
            }
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<(), &'static str> {
        if !self.terminal {
            self.push(b"\n\n")?;
        }
        if self.terminal {
            Ok(())
        } else {
            Err("missing_completion")
        }
    }
}

fn dispatch_reason(error: super::super::transport::ExecutionRuntimeTransportError) -> &'static str {
    use super::super::transport::ExecutionRuntimeTransportError;
    if let ExecutionRuntimeTransportError::UpstreamRequest(message) = error {
        if message
            .rsplit_once("[kind=")
            .and_then(|(_, tail)| tail.strip_suffix(']'))
            .is_some_and(|kinds| kinds.split(',').any(|kind| kind == "timeout"))
        {
            return "timeout";
        }
    }
    "dispatch_error"
}

pub(super) async fn probe_once(state: &AppState, plan: &ExecutionPlan) -> ProbeResult {
    let started = Instant::now();
    let mut observation = ProbeObservation {
        phase: "dispatch",
        dispatched: None,
        ..Default::default()
    };
    let result = tokio::time::timeout(PROBE_TIMEOUT, async {
        let execution = match execute_stream_plan_via_local_tunnel(state, plan)
            .await
            .map_err(dispatch_reason)?
        {
            Some(value) => value,
            None => DirectSyncExecutionRuntime::new()
                .execute_stream(plan)
                .await
                .map_err(dispatch_reason)?,
        };
        let status = execution.status_code;
        let headers = execution.headers;
        observation.http_status = status;
        observation.dispatched = Some(true);
        observation.headers_ms = Some(started.elapsed().as_millis() as u64);
        observation.phase = "first_byte";
        observation.returned_state = policy::state_reason(
            header(&headers, HEADER),
            policy::expected_blocks(
                header(&plan.headers, "authorization"),
                header(&plan.headers, "chatgpt-account-id"),
            ),
            now(),
        );
        if status != 200 {
            return Ok((status, headers, Vec::new()));
        }
        let mut reader = ProbeStream::default();
        macro_rules! receive {
            ($chunk:expr) => {{
                let bytes = $chunk?;
                if !bytes.is_empty() {
                    observation
                        .first_byte_ms
                        .get_or_insert(started.elapsed().as_millis() as u64);
                    observation.phase = "completion";
                }
                reader.push(&bytes)?;
            }};
        }
        match execution.response {
            DirectUpstreamResponse::Reqwest(response) => {
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    receive!(chunk.map_err(|e| if e.is_timeout() {
                        "timeout"
                    } else {
                        "read_error"
                    }));
                    if reader.terminal {
                        break;
                    }
                }
            }
            DirectUpstreamResponse::BrowserWreq(response) => {
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    receive!(chunk.map_err(|e| if e.is_timeout() {
                        "timeout"
                    } else {
                        "read_error"
                    }));
                    if reader.terminal {
                        break;
                    }
                }
            }
            DirectUpstreamResponse::LocalTunnel(mut response) => {
                while let Some(chunk) = response.next_chunk().await.map_err(|_| "read_error")? {
                    receive!(Ok::<_, &'static str>(chunk));
                    if reader.terminal {
                        break;
                    }
                }
            }
            DirectUpstreamResponse::Buffered(bytes) => {
                receive!(Ok::<_, &'static str>(bytes));
            }
        }
        reader.finish()?;
        observation.completed = policy::probe_outcome(&reader.data).is_ok();
        observation.phase = "validation";
        Ok((status, headers, reader.data))
    })
    .await;
    observation.elapsed_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(Ok((status, headers, body))) => Ok(ProbeResponse {
            status,
            headers,
            body,
            observation,
        }),
        result => Err(ProbeFailure {
            reason: match result {
                Ok(Err(reason)) => reason,
                _ => "timeout",
            },
            observation,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DONE: &[u8] =
        b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\r\n\r\n";
    #[test]
    fn completed_event_is_terminal_at_every_chunk_boundary() {
        for split in 0..=DONE.len() {
            let mut stream = ProbeStream::default();
            stream.push(&DONE[..split]).unwrap();
            stream.push(&DONE[split..]).unwrap();
            assert!(stream.terminal);
            stream.push(b"connection can remain open").unwrap();
            assert!(policy::probe_outcome(&stream.data).is_ok());
        }
    }
    #[test]
    fn rejects_incomplete_and_oversized_streams() {
        let mut stream = ProbeStream::default();
        stream
            .push(b"data: {\"type\":\"response.created\"}\n\n")
            .unwrap();
        assert_eq!(stream.finish(), Err("missing_completion"));
        let mut stream = ProbeStream::default();
        assert_eq!(
            stream.push(&vec![b'x'; MAX_PROBE_BYTES + 1]),
            Err("body_too_large")
        );
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn reads_completed_before_eof_and_retains_headers_on_body_timeout() {
        for completed in [true, false] {
            let state = crate::execution_runtime::codex_turn_state::tests::configured_state(
                "codex", "oauth",
            );
            let listener = crate::test_support::bind_loopback_listener().await.unwrap();
            let mut plan = crate::execution_runtime::codex_turn_state::tests::plan();
            plan.url = format!("http://{}/responses", listener.local_addr().unwrap());
            plan.stream = true;
            plan.timeouts = Some(ExecutionTimeouts {
                total_ms: Some(if completed { 2000 } else { 100 }),
                ..Default::default()
            });
            let (done, stopped) = tokio::sync::oneshot::channel();
            let server = async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                assert!(socket.read(&mut request).await.unwrap() > 0);
                let event = if completed {
                    "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
                } else {
                    "data: {\"type\":\"response.created\"}\n\n"
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nx-codex-turn-state: invalid\r\ncontent-length: 4096\r\n\r\n{event}").as_bytes()).await.unwrap();
                // 保持连接，不发送 EOF。整个 server future 与测试一起退出。
                let _ = stopped.await;
            };
            let client = async {
                let result = probe_once(&state, &plan).await;
                let _ = done.send(());
                if completed {
                    let response = result.unwrap_or_else(|failure| {
                        panic!("completed must not wait for EOF: {failure:?}")
                    });
                    assert!(response.observation.completed);
                    assert_eq!(response.status, 200);
                } else {
                    let failure = result.err().expect("pending body must time out");
                    assert_eq!(failure.reason, "timeout");
                    assert_eq!(failure.observation.http_status, 200);
                    assert_eq!(failure.observation.phase, "completion");
                    assert_eq!(failure.observation.returned_state, "invalid_structure");
                    assert!(failure.observation.first_byte_ms.is_some());
                }
            };
            tokio::time::timeout(Duration::from_secs(3), async {
                tokio::join!(server, client);
            })
            .await
            .unwrap();
        }
    }
}

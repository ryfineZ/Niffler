//! 可选的 Codex 客户端遥测模拟。仅保存白名单元数据，不保存提示词或响应正文。
mod batch;
mod catalog;
mod metrics;
mod simulation;
use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aether_contracts::{ExecutionPlan, ExecutionResult, ExecutionTimeouts, RequestBody};
use base64::Engine as _;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use super::transport::{
    execute_sync_plan_via_local_tunnel, resolve_local_tunnel_node_id, send_request,
    ExecutionRuntimeTransportError,
};
use crate::AppState;

pub(crate) const CONFIG_KEY: &str = "codex_telemetry_enabled";
const ANALYTICS_URL: &str = "https://chatgpt.com/backend-api/codex/analytics-events/events";
const METRICS_URL: &str = "https://ab.chatgpt.com/otlp/v1/metrics";
const STATSIG_KEY: &str = "client-MkRuleRQBd6qakfnDYqJVR9JuXcY57Ljly3vi5JVUIO";
const MAX_EVENT_BYTES: usize = 1024 * 1024;
static SEND_SLOTS: Semaphore = Semaphore::const_new(256);
static ACTIVE_SENDS: Semaphore = Semaphore::const_new(8);
static ACCOUNTS: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static THREADS: LazyLock<Mutex<HashMap<(String, String), Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn header<'a>(plan: &'a ExecutionPlan, name: &str) -> &'a str {
    plan.headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .unwrap_or("")
}

fn deployment_disabled() -> bool {
    matches!(
        std::env::var("CODEX_TELEMETRY_ENABLED")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "false" | "0" | "no" | "off"
    )
}

pub(crate) fn eligible(plan: &ExecutionPlan) -> bool {
    let Ok(url) = reqwest::Url::parse(&plan.url) else {
        return false;
    };
    let Some(body) = plan.body.json_body.as_ref() else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("chatgpt.com")
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.path(),
            "/backend-api/codex/responses" | "/backend-api/codex/v1/responses"
        )
        && plan.method.eq_ignore_ascii_case("POST")
        && plan.client_api_format != "openai:image"
        && !header(plan, "chatgpt-account-id").is_empty()
        && header(plan, "authorization").starts_with("Bearer ")
        && header(plan, "authorization").len() > 7
        // 声明生图工具也适用于普通文本请求，仅排除明确选择生图的请求。
        && !body.get("model").and_then(Value::as_str)
            .is_some_and(|model| model.trim().to_ascii_lowercase().starts_with("gpt-image-"))
        && !body.get("tool_choice")
            .and_then(|choice| choice.as_str().or_else(|| choice.get("type").and_then(Value::as_str)))
            .is_some_and(|choice| choice.trim().eq_ignore_ascii_case("image_generation"))
}

async fn enabled(state: &AppState) -> bool {
    if deployment_disabled() {
        return false;
    }
    match tokio::time::timeout(
        Duration::from_millis(250),
        state.read_system_config_json_value(CONFIG_KEY),
    )
    .await
    {
        Ok(Ok(Some(Value::Bool(value)))) => value,
        Ok(Ok(None | Some(Value::Null))) => false,
        _ => {
            tracing::warn!(
                event_name = "codex_telemetry_config_unavailable",
                "Codex 遥测配置不可用，本次跳过"
            );
            false
        }
    }
}

// 不派生 Debug：模板中包含发往官方分析端点的 OAuth Authorization。
pub(crate) struct Observation {
    state: AppState,
    template: ExecutionPlan,
    params: Value,
    started: Instant,
    started_ns: String,
    first_event_ms: Option<u64>,
    first_token_ms: Option<u64>,
    pending: Vec<u8>,
    event: Vec<u8>,
    dropping: bool,
    line_nonempty: bool,
    live_timestamps: bool,
    finished: bool,
    status: &'static str,
    usage: Value,
    response_id: String,
}

impl Observation {
    pub(crate) async fn begin(state: &AppState, plan: &ExecutionPlan) -> Option<Self> {
        if !eligible(plan) || !enabled(state).await {
            return None;
        }
        let body = plan.body.json_body.as_ref()?;
        let metadata = body
            .pointer("/client_metadata/x-codex-turn-metadata")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .or_else(|| serde_json::from_str::<Value>(header(plan, "x-codex-turn-metadata")).ok())
            .unwrap_or(Value::Null);
        let session = nonempty(header(plan, "session-id"))
            .or_else(|| nonempty(header(plan, "session_id")))
            .or_else(|| {
                body.pointer("/client_metadata/session_id")
                    .and_then(Value::as_str)
            })
            .or_else(|| metadata.get("session_id").and_then(Value::as_str))
            .unwrap_or(&plan.request_id)
            .to_string();
        let thread = nonempty(header(plan, "thread-id"))
            .or_else(|| {
                body.pointer("/client_metadata/thread_id")
                    .and_then(Value::as_str)
            })
            .or_else(|| metadata.get("thread_id").and_then(Value::as_str))
            .unwrap_or(&session)
            .to_string();
        let turn = metadata
            .get("turn_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let mut observation = Self {
            state: state.clone(),
            template: telemetry_plan(plan, false, json!({})),
            params: json!({
                "session_id": session, "thread_id": thread, "turn_id": turn,
                "root_turn_id": metadata.get("root_turn_id").and_then(Value::as_str).unwrap_or(&turn),
                "model": body.get("model").and_then(Value::as_str).or(plan.model_name.as_deref()),
                "model_provider": "openai", "reasoning_effort": body.pointer("/reasoning/effort").and_then(Value::as_str).unwrap_or("medium"),
                "service_tier": body.get("service_tier").and_then(Value::as_str).unwrap_or("default"), "started_at": unix_ns() / 1_000_000_000,
                "sampling_request_count": 1, "steer_count": 0,
            }),
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
            response_id: format!("resp_{}", uuid::Uuid::new_v4().simple()),
        };
        let first = mark_thread(&plan.key_id, &thread);
        observation.params["is_first_turn"] = json!(first);
        let events = simulation::initialize(&mut observation);
        observation.submit(false, json!({"events": events}));
        if mark_account(&plan.key_id) {
            observation.submit(true, metrics::payload(&observation, true));
        }
        Some(observation)
    }

    pub(crate) fn fail(&mut self) {
        self.status = "failed";
        self.finish();
    }

    pub(crate) fn http_status(&mut self, status: u16) {
        if !(200..300).contains(&status) {
            self.params["codex_error_http_status_code"] = json!(status);
            self.status = "failed";
            self.finish();
        }
    }

    pub(crate) fn observe(&mut self, data: &[u8]) {
        if self.finished {
            return;
        }
        if self.first_event_ms.is_none() {
            self.first_event_ms = Some(self.elapsed());
        }
        // 逐行解析，限制单事件内存；只提取状态和数字用量，绝不上传 data 原文。
        for part in data.split_inclusive(|byte| *byte == b'\n') {
            let newline = part.last() == Some(&b'\n');
            self.line_nonempty |= part.iter().any(|byte| !matches!(byte, b'\n' | b'\r'));
            if !self.dropping && self.pending.len() + part.len() <= MAX_EVENT_BYTES {
                self.pending.extend_from_slice(part);
            } else {
                self.pending.clear();
                self.event.clear();
                self.dropping = true;
            }
            if newline {
                let blank = !self.line_nonempty;
                self.line_nonempty = false;
                if self.dropping {
                    self.pending.clear();
                    if blank {
                        self.event.clear();
                        self.dropping = false;
                    }
                    continue;
                }
                let line = std::mem::take(&mut self.pending);
                let line = line.strip_suffix(b"\n").unwrap_or(&line);
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                if line.is_empty() {
                    if !self.dropping {
                        let event = std::mem::take(&mut self.event);
                        self.parse_event(&event);
                        if self.finished {
                            return;
                        }
                    }
                    self.event.clear();
                    self.dropping = false;
                } else if !self.dropping {
                    if let Some(payload) = line.strip_prefix(b"data:") {
                        if self.event.len() + payload.len() < MAX_EVENT_BYTES {
                            self.event.extend_from_slice(payload);
                            self.event.push(b'\n');
                        } else {
                            self.event.clear();
                            self.dropping = true;
                        }
                    }
                }
            }
        }
    }

    fn parse_event(&mut self, data: &[u8]) {
        if self.finished {
            return;
        }
        let Ok(event) = serde_json::from_slice::<Value>(data) else {
            return;
        };
        self.parse_value(&event);
    }

    fn parse_value(&mut self, event: &Value) {
        if self.finished {
            return;
        }
        let typ = event.get("type").and_then(Value::as_str).unwrap_or("");
        if self.live_timestamps && typ.ends_with(".delta") && self.first_token_ms.is_none() {
            self.first_token_ms = Some(self.elapsed());
        }
        let response = event.get("response").unwrap_or(event);
        let status = match typ {
            "response.completed" => Some("completed"),
            "response.failed" | "error" => Some("failed"),
            "response.incomplete" => Some("interrupted"),
            _ if typ.is_empty() => match response.get("status").and_then(Value::as_str) {
                Some("completed") => Some("completed"),
                Some("failed") => Some("failed"),
                Some("incomplete") => Some("interrupted"),
                _ => None,
            },
            _ => None,
        };
        if let Some(status) = status {
            self.status = status;
            if let Some(id) = response.get("id").and_then(Value::as_str) {
                self.response_id = id.to_string();
            }
            if let Some(tier) = response.get("service_tier").and_then(Value::as_str) {
                self.params["service_tier"] = json!(tier);
            }
            self.usage = numeric_usage(response.get("usage").unwrap_or(&Value::Null));
            self.finish();
        }
    }

    pub(crate) fn eof(&mut self) {
        if !self.dropping {
            if !self.pending.is_empty() {
                let pending = std::mem::take(&mut self.pending);
                if let Some(payload) = pending.strip_prefix(b"data:") {
                    if self.event.len() + payload.len() <= MAX_EVENT_BYTES {
                        self.event.extend_from_slice(payload);
                    }
                } else {
                    self.parse_event(&pending);
                }
            }
            let event = std::mem::take(&mut self.event);
            self.parse_event(&event);
        }
        self.finish();
    }

    pub(crate) fn observe_buffered(&mut self, data: &[u8]) {
        if self.finished {
            return;
        }
        self.live_timestamps = false;
        if self.first_event_ms.is_none() {
            self.first_event_ms = Some(self.elapsed());
        }
        // 完整响应先按 JSON 解析，避免 SSE 按行处理丢弃 JSON 的换行。
        if let Ok(value) = serde_json::from_slice::<Value>(data) {
            self.parse_value(&value);
        } else {
            self.observe(data);
        }
        self.eof();
    }

    pub(crate) fn sync_result(&mut self, result: &ExecutionResult) {
        self.live_timestamps = false;
        self.first_event_ms = result.telemetry.as_ref().and_then(|value| value.ttfb_ms);
        self.http_status(result.status_code);
        if let Some(body) = &result.body {
            if let Some(value) = &body.json_body {
                self.parse_value(value);
            } else if let Some(raw) = &body.body_bytes_b64 {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(raw) {
                    let decoded =
                        super::transport::decode_response_body_bytes(&result.headers, &bytes);
                    self.observe_buffered(decoded.as_deref().unwrap_or(&bytes));
                }
            }
        }
        self.eof();
    }

    fn elapsed(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        let elapsed = self.elapsed();
        self.params["status"] = json!(self.status);
        self.params["duration_ms"] = json!(elapsed);
        self.params["completed_at"] = json!(unix_ns() / 1_000_000_000);
        if let Some(ms) = self.first_event_ms {
            self.params["before_first_sampling_ms"] = json!(ms);
        }
        if let Some(usage) = self.usage.as_object() {
            for (key, value) in usage {
                self.params[key] = value.clone();
            }
        }
        self.params["sampling_ms"] = json!(elapsed.saturating_sub(
            self.first_token_ms
                .or(self.first_event_ms)
                .unwrap_or(elapsed)
        ));
        if self.status == "interrupted" {
            self.params["explicit_client_interrupt_requested_at_ms"] = json!(unix_ns() / 1_000_000);
        }
        self.submit(false, json!({"events": simulation::terminal(self)}));
        self.submit(true, metrics::payload(self, false));
        self.pending.clear();
        self.event.clear();
    }

    fn submit(&self, metrics: bool, body: Value) {
        let Ok(permit) = SEND_SLOTS.try_acquire() else {
            tracing::warn!(
                event_name = "codex_telemetry_dropped",
                "Codex 遥测并发已满，跳过上报"
            );
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let state = self.state.clone();
        let incremental = metrics
            && body
                .pointer("/resourceMetrics/0/scopeMetrics/0/metrics")
                .and_then(Value::as_array)
                .is_some_and(|items| items.len() < 62);
        let mut plan = telemetry_plan(&self.template, metrics, body);
        let mut batch = if incremental {
            let Some(guard) = batch::enqueue(&plan) else {
                return;
            };
            Some(guard)
        } else {
            None
        };
        runtime.spawn(async move {
            let _permit = permit;
            if incremental {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let Some(queued) = batch.as_mut().and_then(batch::BatchGuard::take) else {
                    return;
                };
                plan = queued;
            }
            let Ok(Ok(_active)) =
                tokio::time::timeout(Duration::from_secs(10), ACTIVE_SENDS.acquire()).await
            else {
                tracing::warn!(event_name = "codex_telemetry_dropped", "Codex 遥测发送已满");
                return;
            };
            if !enabled(&state).await {
                return;
            }
            let send = async {
                if resolve_local_tunnel_node_id(&state, plan.proxy.as_ref()).is_some() {
                    execute_sync_plan_via_local_tunnel(&state, &plan)
                        .await
                        .map(|result| result.status_code)
                } else {
                    let body =
                        serde_json::to_vec(plan.body.json_body.as_ref().expect("telemetry body"))
                            .map_err(ExecutionRuntimeTransportError::BodyEncode)?;
                    send_request(&plan, body)
                        .await
                        .map(|response| response.status_code())
                }
            };
            match tokio::time::timeout(Duration::from_secs(10), send).await {
                Ok(Ok(status_code)) => {
                    tracing::info!(
                        event_name = "codex_telemetry_delivery",
                        metrics,
                        status_code,
                        accepted = (200..300).contains(&status_code),
                        "Codex 遥测上报结果"
                    );
                }
                _ => tracing::warn!(
                    event_name = "codex_telemetry_delivery_failed",
                    metrics,
                    "Codex 遥测发送失败或超时"
                ),
            }
        });
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.finish();
    }
}

fn nonempty(value: &str) -> Option<&str> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
fn unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn mark_thread(account: &str, thread: &str) -> bool {
    let Ok(mut threads) = THREADS.lock() else {
        return false;
    };
    threads.retain(|_, seen| seen.elapsed() < Duration::from_secs(300));
    if threads.len() >= 4096 {
        return false;
    }
    threads
        .insert((account.to_owned(), thread.to_owned()), Instant::now())
        .is_none()
}

fn numeric_usage(usage: &Value) -> Value {
    let mut values = serde_json::Map::new();
    for (key, pointer) in [
        ("input_tokens", "/input_tokens"),
        ("output_tokens", "/output_tokens"),
        ("total_tokens", "/total_tokens"),
        ("cached_input_tokens", "/input_tokens_details/cached_tokens"),
        (
            "reasoning_output_tokens",
            "/output_tokens_details/reasoning_tokens",
        ),
    ] {
        if let Some(value) = usage.pointer(pointer).and_then(Value::as_u64) {
            values.insert(key.into(), json!(value));
        }
    }
    Value::Object(values)
}

fn telemetry_plan(source: &ExecutionPlan, metrics: bool, body: Value) -> ExecutionPlan {
    let mut headers = BTreeMap::from([
        ("content-type".into(), "application/json".into()),
        ("accept".into(), "*/*".into()),
        (
            aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER.into(),
            "false".into(),
        ),
    ]);
    if metrics {
        headers.insert("user-agent".into(), "OTel-OTLP-Exporter-Rust/0.31.0".into());
        headers.insert(
            "statsig-api-key".into(),
            std::env::var("CODEX_STATSIG_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| STATSIG_KEY.into()),
        );
    } else {
        for name in [
            "authorization",
            "chatgpt-account-id",
            "user-agent",
            "originator",
        ] {
            let value = header(source, name);
            if !value.is_empty() {
                headers.insert(name.into(), value.into());
            }
        }
    }
    ExecutionPlan {
        request_id: source.request_id.clone(),
        candidate_id: None,
        provider_name: source.provider_name.clone(),
        provider_id: source.provider_id.clone(),
        endpoint_id: source.endpoint_id.clone(),
        key_id: source.key_id.clone(),
        method: "POST".into(),
        url: if metrics { METRICS_URL } else { ANALYTICS_URL }.into(),
        headers,
        content_type: Some("application/json".into()),
        content_encoding: None,
        body: RequestBody::from_json(body),
        stream: false,
        client_api_format: "provider_ops:telemetry".into(),
        provider_api_format: "provider_ops:telemetry".into(),
        model_name: source.model_name.clone(),
        proxy: source.proxy.clone(),
        transport_profile: source.transport_profile.clone(),
        timeouts: Some(ExecutionTimeouts {
            connect_ms: Some(3000),
            read_ms: Some(10000),
            total_ms: Some(10000),
            ..Default::default()
        }),
    }
}

fn mark_account(account: &str) -> bool {
    let Ok(mut accounts) = ACCOUNTS.lock() else {
        return false;
    };
    accounts.retain(|_, seen| seen.elapsed() < Duration::from_secs(300));
    if accounts.len() >= 4096 {
        return false;
    }
    accounts
        .insert(account.to_owned(), Instant::now())
        .is_none()
}

#[cfg(test)]
#[path = "codex_telemetry/tests.rs"]
mod tests;

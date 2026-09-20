//! 官方 Codex 压缩：V2 原样流转，V1 单次转换；不执行生成用 state 探测。
mod protocol;
pub(crate) use protocol::is_v2;

use std::collections::BTreeMap;
use std::io::Read;
use std::time::{Duration, Instant};

use aether_contracts::{ExecutionPlan, ExecutionResult, ExecutionTelemetry, ResponseBody};
use axum::body::Bytes;
use base64::Engine as _;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::codex_turn_state::{self, compact_route};
use super::transport::{
    self, DirectSyncExecutionRuntime, DirectUpstreamResponse, DirectUpstreamStreamExecution,
};
use crate::AppState;

#[derive(Clone, Copy, Debug)]
struct CompactError {
    code: &'static str,
    message: &'static str,
    status: u16,
    sent: bool,
}

impl CompactError {
    fn request() -> Self {
        Self {
            code: "codex_compact_invalid_request",
            message: "压缩请求格式无效或超过大小限制。",
            status: 400,
            sent: false,
        }
    }
    fn incomplete() -> Self {
        Self {
            code: "codex_compact_incomplete",
            message: "上游压缩响应未完整结束；请求可能已产生用量，不会自动重发。",
            status: 502,
            sent: true,
        }
    }
    fn invalid() -> Self {
        Self {
            code: "codex_compact_invalid_output",
            message: "上游未返回完整有效的压缩结果；不会自动重发。",
            status: 502,
            sent: true,
        }
    }
    fn upstream(status: u16) -> Self {
        Self {
            code: "codex_compact_upstream_rejected",
            message: "上游压缩请求失败；不会自动重发，请检查账号或稍后重试。",
            status,
            sent: true,
        }
    }
    fn runtime() -> Self {
        Self {
            code: "codex_compact_runtime_unavailable",
            message: "无法读取压缩请求的账号或出口状态。",
            status: 503,
            sent: false,
        }
    }
    fn limit() -> Self {
        Self {
            code: "codex_compact_response_too_large",
            message: "压缩响应超过大小限制；不会自动重发。",
            status: 502,
            sent: true,
        }
    }
    fn timeout(sent: bool) -> Self {
        Self {
            code: "codex_compact_timeout",
            message: "压缩请求超时；不会自动重发。",
            status: 504,
            sent,
        }
    }
    fn state(error: codex_turn_state::StateError) -> Self {
        if error.egress_unavailable() {
            return Self { code: "codex_compact_egress_unavailable", message: "有效 state 绑定的出口已不可用，或属于另一台机器的直连/本机代理；压缩请求未发出，请恢复该出口或让请求回到采集机器。", status:503, sent:false };
        }
        Self {
            code: "codex_compact_state_unavailable",
            message: "压缩请求的账号被保护或共享状态不可用；请求未发出。",
            status: error.status(),
            sent: false,
        }
    }
    fn response(self, plan: &ExecutionPlan) -> DirectUpstreamStreamExecution {
        let body = json!({"error":{"code":self.code,"type":"codex_compact_error","message":self.message,
            "upstream_request_sent":self.sent,"upstream_usage_unknown":self.sent}});
        buffered(
            plan,
            self.status,
            BTreeMap::from([("x-niffler-compaction".into(), "failed".into())]),
            body,
            Instant::now(),
        )
    }
}

pub(crate) fn terminal_error(text: Option<&str>) -> bool {
    text.and_then(|text| serde_json::from_str::<Value>(text).ok())
        .is_some_and(|body| {
            body.pointer("/error/type").and_then(Value::as_str) == Some("codex_compact_error")
        })
}

pub(crate) fn handled(headers: &BTreeMap<String, String>) -> bool {
    headers
        .get("x-niffler-compaction")
        .is_some_and(|value| matches!(value.as_str(), "v1-to-v2" | "v2" | "failed"))
}

pub(crate) fn handled_context(context: Option<&Value>) -> bool {
    context
        .and_then(|value| value.pointer("/provider_response_headers/x-niffler-compaction"))
        .and_then(Value::as_str)
        .is_some_and(|value| matches!(value, "v1-to-v2" | "v2" | "failed"))
}

pub(crate) fn is_candidate(plan: &ExecutionPlan) -> bool {
    kind(plan).is_some()
}

fn kind(plan: &ExecutionPlan) -> Option<bool> {
    let url = reqwest::Url::parse(&plan.url).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("chatgpt.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || !plan.method.eq_ignore_ascii_case("POST")
        || plan.client_api_format == "openai:image"
    {
        return None;
    }
    match (url.path(), plan.provider_api_format.as_str()) {
        (
            "/backend-api/codex/responses/compact" | "/backend-api/codex/v1/responses/compact",
            "openai:responses:compact",
        ) => Some(true),
        (
            "/backend-api/codex/responses" | "/backend-api/codex/v1/responses",
            "openai:responses",
        ) if plan.body.json_body.as_ref().is_some_and(is_v2) => Some(false),
        _ => None,
    }
}

fn timeout(plan: &ExecutionPlan) -> Duration {
    Duration::from_millis(
        plan.timeouts
            .as_ref()
            .and_then(|v| v.total_ms)
            .filter(|v| *v > 0)
            .unwrap_or(120_000),
    )
}

/// 所有正式入口都在 state 普通生成准备之前调用，OAuth 重试入口也使用同一函数。
pub(crate) async fn maybe_execute_stream(
    state: &AppState,
    plan: &ExecutionPlan,
) -> Option<DirectUpstreamStreamExecution> {
    let legacy = kind(plan)?;
    let transport = match state
        .read_provider_transport_snapshot(&plan.provider_id, &plan.endpoint_id, &plan.key_id)
        .await
    {
        Ok(Some(value)) => value,
        _ => return Some(CompactError::runtime().response(plan)),
    };
    if !transport
        .provider
        .provider_type
        .eq_ignore_ascii_case("codex")
        || !transport.key.auth_type.eq_ignore_ascii_case("oauth")
    {
        return None;
    }
    let route =
        match tokio::time::timeout(Duration::from_secs(5), compact_route::prepare(state, plan))
            .await
        {
            Ok(Ok(route)) => route,
            Ok(Err(error)) => return Some(CompactError::state(error).response(plan)),
            Err(_) => return Some(CompactError::timeout(false).response(plan)),
        };
    let mut dispatch = route.plan.clone();
    if legacy {
        if let Err(error) = protocol::bridge_request(&mut dispatch) {
            return Some(error.response(plan));
        }
    }
    // 沿用客户端自己的协议字段，但绝不注入账号池缓存的 state。
    dispatch.stream = true;
    dispatch.headers.retain(|name, _| {
        !name.eq_ignore_ascii_case(aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER)
    });
    dispatch.headers.insert(
        aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER.into(),
        "false".into(),
    );
    let result = tokio::time::timeout(
        timeout(plan),
        execute(state, plan, &dispatch, &route, legacy),
    )
    .await;
    Some(match result {
        Ok(Ok(execution)) => execution,
        Ok(Err(error)) => error.response(plan),
        Err(_) => CompactError::timeout(true).response(plan),
    })
}

async fn execute(
    state: &AppState,
    original: &ExecutionPlan,
    dispatch: &ExecutionPlan,
    route: &compact_route::CompactRoute,
    legacy: bool,
) -> Result<DirectUpstreamStreamExecution, CompactError> {
    let started = Instant::now();
    let result = match transport::execute_stream_plan_via_local_tunnel(state, dispatch).await {
        Ok(Some(execution)) => Ok(execution),
        Ok(None) => {
            DirectSyncExecutionRuntime::new()
                .execute_stream(dispatch)
                .await
        }
        Err(error) => Err(error),
    };
    let mut execution = match result {
        Ok(execution) => execution,
        Err(_) => {
            transport::record_manual_proxy_request_failure(state, dispatch).await;
            return Err(CompactError::incomplete());
        }
    };
    route
        .observe(state, execution.status_code, &execution.headers)
        .await
        .map_err(|_| CompactError {
            sent: true,
            ..CompactError::runtime()
        })?;
    codex_turn_state::strip(&mut execution.headers);
    execution.headers.insert(
        "x-niffler-compaction".into(),
        if legacy { "v1-to-v2" } else { "v2" }.into(),
    );
    execution.headers.insert(
        "x-niffler-compaction-egress".into(),
        if route.bound { "state" } else { "configured" }.into(),
    );
    if !(200..300).contains(&execution.status_code) {
        transport::record_manual_proxy_request_outcome(state, dispatch, execution.status_code)
            .await;
        return Ok(error_response(
            CompactError::upstream(execution.status_code),
            original,
            &execution.headers,
        ));
    }
    if !legacy {
        transport::record_manual_proxy_request_success(state, dispatch).await;
        return Ok(execution);
    }
    let bytes = read_body(execution.response, &execution.headers).await?;
    let body = match protocol::bridge_response(&bytes) {
        Ok(body) => body,
        Err(error) => {
            route
                .observe(state, error.status, &execution.headers)
                .await
                .map_err(|_| CompactError {
                    sent: true,
                    ..CompactError::runtime()
                })?;
            transport::record_manual_proxy_request_outcome(state, dispatch, error.status).await;
            return Ok(error_response(error, original, &execution.headers));
        }
    };
    transport::record_manual_proxy_request_success(state, dispatch).await;
    Ok(buffered(
        original,
        execution.status_code,
        execution.headers,
        body,
        started,
    ))
}

fn error_response(
    error: CompactError,
    plan: &ExecutionPlan,
    headers: &BTreeMap<String, String>,
) -> DirectUpstreamStreamExecution {
    let mut response = error.response(plan);
    if let Some(value) = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("retry-after"))
        .map(|(_, value)| value)
    {
        response.headers.insert("retry-after".into(), value.clone());
    }
    response
}

fn buffered(
    plan: &ExecutionPlan,
    status_code: u16,
    mut headers: BTreeMap<String, String>,
    body: Value,
    started_at: Instant,
) -> DirectUpstreamStreamExecution {
    headers.retain(|name, _| {
        !matches!(
            name.to_ascii_lowercase().as_str(),
            "content-encoding" | "content-length" | "transfer-encoding" | "trailer"
        )
    });
    headers.insert("content-type".into(), "application/json".into());
    DirectUpstreamStreamExecution {
        request_id: plan.request_id.clone(),
        candidate_id: plan.candidate_id.clone(),
        status_code,
        headers,
        provider_api_format: plan.provider_api_format.clone(),
        stream_summary_report_context: json!({"provider_api_format":plan.provider_api_format,"client_api_format":plan.client_api_format,"model":plan.model_name,"upstream_is_stream":false}),
        response: DirectUpstreamResponse::Buffered(Bytes::from(body.to_string())),
        started_at,
        codex_telemetry: None,
        codex_state_candidate: None,
    }
}

pub(crate) async fn maybe_execute_sync(
    state: &AppState,
    plan: &ExecutionPlan,
) -> Option<ExecutionResult> {
    let execution = maybe_execute_stream(state, plan).await?;
    let started = execution.started_at;
    let status = execution.status_code;
    let mut headers = execution.headers;
    let bytes = match tokio::time::timeout(
        timeout(plan).saturating_sub(started.elapsed()),
        read_body(execution.response, &headers),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        outcome => {
            let error = match outcome {
                Ok(Err(error)) => error,
                _ => CompactError::timeout(true),
            };
            let failed = error.response(plan);
            headers = failed.headers;
            let DirectUpstreamResponse::Buffered(bytes) = failed.response else {
                unreachable!()
            };
            return Some(sync_result(
                plan,
                error.status,
                headers,
                bytes.to_vec(),
                started,
            ));
        }
    };
    headers.retain(|name, _| {
        !matches!(
            name.to_ascii_lowercase().as_str(),
            "content-encoding" | "content-length"
        )
    });
    Some(sync_result(plan, status, headers, bytes, started))
}

fn sync_result(
    plan: &ExecutionPlan,
    status_code: u16,
    headers: BTreeMap<String, String>,
    bytes: Vec<u8>,
    started: Instant,
) -> ExecutionResult {
    let body = match serde_json::from_slice(&bytes) {
        Ok(body) => ResponseBody {
            json_body: Some(body),
            body_bytes_b64: None,
        },
        Err(_) => ResponseBody {
            json_body: None,
            body_bytes_b64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        },
    };
    ExecutionResult {
        request_id: plan.request_id.clone(),
        candidate_id: plan.candidate_id.clone(),
        status_code,
        headers,
        body: Some(body),
        telemetry: Some(ExecutionTelemetry {
            ttfb_ms: None,
            elapsed_ms: Some(started.elapsed().as_millis() as u64),
            upstream_bytes: Some(bytes.len() as u64),
        }),
        error: None,
    }
}

async fn read_body(
    response: DirectUpstreamResponse,
    headers: &BTreeMap<String, String>,
) -> Result<Vec<u8>, CompactError> {
    let mut bytes = Vec::new();
    match response {
        DirectUpstreamResponse::Buffered(chunk) => append(&mut bytes, &chunk)?,
        DirectUpstreamResponse::Reqwest(response) => {
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                append(&mut bytes, &chunk.map_err(|_| CompactError::incomplete())?)?;
            }
        }
        DirectUpstreamResponse::BrowserWreq(response) => {
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                append(&mut bytes, &chunk.map_err(|_| CompactError::incomplete())?)?;
            }
        }
        DirectUpstreamResponse::LocalTunnel(mut response) => {
            while let Some(chunk) = response
                .next_chunk()
                .await
                .map_err(|_| CompactError::incomplete())?
            {
                append(&mut bytes, &chunk)?;
            }
        }
    }
    let encoding = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-encoding"))
        .map(|(_, value)| value.trim().to_ascii_lowercase());
    match encoding.as_deref() {
        None | Some("" | "identity") => Ok(bytes),
        Some("gzip") => decode(flate2::read::GzDecoder::new(bytes.as_slice())),
        Some("deflate") => match decode(flate2::read::ZlibDecoder::new(bytes.as_slice())) {
            Err(error) if error.code == "codex_compact_invalid_output" => {
                decode(flate2::read::DeflateDecoder::new(bytes.as_slice()))
            }
            result => result,
        },
        Some("zstd") => decode(
            zstd::stream::read::Decoder::new(bytes.as_slice())
                .map_err(|_| CompactError::invalid())?,
        ),
        _ => Err(CompactError::invalid()),
    }
}

fn append(bytes: &mut Vec<u8>, chunk: &[u8]) -> Result<(), CompactError> {
    if chunk.len() > protocol::MAX_BYTES.saturating_sub(bytes.len()) {
        return Err(CompactError::limit());
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

fn decode(reader: impl Read) -> Result<Vec<u8>, CompactError> {
    let mut bytes = Vec::new();
    reader
        .take(protocol::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CompactError::invalid())?;
    if bytes.len() > protocol::MAX_BYTES {
        return Err(CompactError::limit());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;

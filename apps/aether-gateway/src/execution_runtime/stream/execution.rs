use std::collections::{BTreeMap, VecDeque};
use std::io::Error as IoError;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use aether_contracts::{
    ExecutionPlan, ExecutionStreamTerminalSummary, ExecutionTelemetry, StandardizedUsage,
    StreamFrame, StreamFramePayload,
};
use aether_data::UsageObjectStore;
use aether_data_contracts::repository::candidates::RequestCandidateStatus;
use aether_data_contracts::repository::usage::{
    usage_body_ref, UsageBodyCaptureState, UsageBodyField,
};
use aether_scheduler_core::{
    parse_request_candidate_report_context, SchedulerRequestCandidateStatusUpdate,
};
use aether_usage_runtime::{
    build_lifecycle_usage_seed, build_stream_terminal_usage_payload_seed,
    build_sync_terminal_usage_payload_seed, build_terminal_usage_context_seed,
    build_usage_event_data_seed, UsageBodyCapturePolicy, UsageEvent, UsageEventType,
    UsageRequestRecordLevel, UsageRuntimeAccess, DEFAULT_USAGE_RESPONSE_BODY_CAPTURE_LIMIT_BYTES,
};
use async_stream::stream;
use axum::body::{Body, Bytes};
use axum::http::Response;
use base64::Engine as _;
use futures_util::stream::BoxStream;
use futures_util::{StreamExt, TryStreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_util::io::StreamReader;
use tracing::{debug, info, warn};

use super::error::{
    build_synthetic_non_success_stream_error_body, collect_error_body, decode_stream_error_body,
    inspect_prefetched_stream_body, read_next_frame,
    should_synthesize_non_success_stream_error_body,
    stream_client_error_status_code_for_upstream_status, synthetic_error_response_headers,
    StreamPrefetchInspection,
};
#[path = "execution_failures.rs"]
mod execution_failures;
use self::execution_failures::{
    build_stream_failure_from_execution_error, build_stream_failure_report,
    handle_prefetch_stream_failure, record_stream_sync_failure, submit_midstream_stream_failure,
    StreamFailureReport,
};
use crate::ai_serving::api::{
    maybe_bridge_standard_sync_json_to_stream, maybe_build_provider_private_stream_normalizer,
    maybe_build_stream_response_rewriter, normalize_provider_private_report_context,
    StreamingStandardTerminalObserver,
};
use crate::api::response::{
    attach_control_metadata_headers, build_client_response, build_client_response_from_parts,
};
use crate::clock::current_unix_ms as current_request_candidate_unix_ms;
use crate::constants::{CONTROL_CANDIDATE_ID_HEADER, CONTROL_REQUEST_ID_HEADER};
use crate::control::GatewayControlDecision;
use crate::execution_runtime::build_direct_execution_frame_stream;
use crate::execution_runtime::chatgpt_web_image::maybe_execute_chatgpt_web_image_stream;
use crate::execution_runtime::grok::maybe_execute_grok_stream;
use crate::execution_runtime::kiro_cache::{
    billed_input_tokens as kiro_billed_input_tokens, build_kiro_prompt_cache_profile,
    estimate_kiro_prompt_input_tokens, kiro_prompt_cache_tracker,
    kiro_simulated_cache_enabled_from_provider_config,
    kiro_simulated_cache_enabled_from_report_context, KiroPromptCacheUsage,
    KIRO_SIMULATED_CACHE_ENABLED_CONTEXT_FIELD,
};
use crate::execution_runtime::kiro_web_search::maybe_execute_kiro_web_search_stream;
use crate::execution_runtime::oauth_retry::refresh_oauth_plan_auth_for_retry;
#[cfg(test)]
use crate::execution_runtime::remote_compat::post_stream_plan_to_remote_execution_runtime;
use crate::execution_runtime::submission::{
    resolve_core_error_background_report_kind, resolve_local_sync_error_status_code,
    strip_utf8_bom_and_ws, submit_local_core_error_or_sync_finalize,
};
use crate::execution_runtime::transport::{
    execute_stream_plan_via_local_tunnel, record_manual_proxy_request_failure,
    record_manual_proxy_request_success, record_manual_proxy_stream_error,
    DirectSyncExecutionRuntime, DirectUpstreamStreamExecution, ExecutionRuntimeTransportError,
};
use crate::execution_runtime::{
    apply_endpoint_response_header_rules, attach_provider_response_headers_to_report_context,
    local_failover_response_text, resolve_core_stream_direct_finalize_report_kind,
    resolve_core_stream_error_finalize_report_kind,
    resolve_local_candidate_failover_analysis_stream, should_fallback_to_control_stream,
    should_retry_next_local_candidate_stream, LocalFailoverDecision,
};
use crate::execution_runtime::{MAX_STREAM_PREFETCH_BYTES, MAX_STREAM_PREFETCH_FRAMES};
use crate::handlers::shared::provider_pool::record_admin_provider_pool_model_cooldown;
use crate::log_ids::short_request_id;
use crate::niffler_error_return::{
    build_niffler_upstream_error_context, rewrite_niffler_upstream_error_response,
};
use crate::orchestration::{
    apply_local_execution_effect, build_local_error_flow_metadata,
    is_retryable_openai_transient_processing_error, resolve_local_stream_failover_policy,
    trace_upstream_response_body, with_error_flow_report_context,
    with_upstream_response_report_context, LocalAdaptiveRateLimitEffect,
    LocalAdaptiveSuccessEffect, LocalAttemptFailureEffect, LocalExecutionEffect,
    LocalExecutionEffectContext, LocalHealthFailureEffect, LocalHealthSuccessEffect,
    LocalOAuthInvalidationEffect, LocalPoolErrorEffect,
};
use crate::provider_pool_demand::{
    acquire_provider_pool_in_flight_guard, ProviderPoolInFlightGuard,
};
use crate::request_candidate_runtime::{
    bind_execution_plan_to_request_id, ensure_execution_request_candidate_slot,
    snapshot_local_request_candidate_status, submit_local_request_candidate_status,
    submit_local_request_candidate_status_snapshot,
};
use crate::usage::submit_stream_report;
use crate::usage::{GatewayStreamReportRequest, GatewaySyncReportRequest};
use crate::{
    AppState, GatewayError, GEMINI_FILES_DOWNLOAD_PLAN_KIND, OPENAI_VIDEO_CONTENT_PLAN_KIND,
};

fn merge_stream_usage_report_context(context: Option<Value>, plan: &ExecutionPlan) -> Value {
    let mut context = context.unwrap_or_else(|| json!({}));
    if !context.is_object() {
        context = json!({});
    }
    let object = context
        .as_object_mut()
        .expect("stream usage report context must be an object");
    for (key, value) in [
        ("provider_api_format", plan.provider_api_format.as_str()),
        ("client_api_format", plan.client_api_format.as_str()),
    ] {
        let missing = object
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty());
        if missing && !value.trim().is_empty() {
            object.insert(key.to_string(), Value::String(value.to_string()));
        }
    }
    context
}

const OPENAI_IMAGE_STREAM_PLAN_KIND: &str = "openai_image_stream";
const SSE_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const SSE_KEEPALIVE_BYTES: &[u8] = b": aether-keepalive\n\n";
const STREAM_IDLE_LOG_INTERVAL: Duration = Duration::from_secs(60);
const STREAM_IDLE_LOG_INTERVAL_MS: u64 = 60_000;
const REWRITTEN_STREAM_PREFETCH_TIMEOUT: Duration = Duration::from_millis(750);
const OPENAI_IMAGE_STREAM_DEFAULT_TOTAL_TIMEOUT_MS: u64 = 900_000;
const STREAM_FAILOVER_EXECUTION_FRAME_MAX_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_EXECUTION_FRAME_MAX_BYTES: usize = 64 * 1024 * 1024;

async fn record_stream_candidate_preparation_failure(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    started_at_unix_ms: u64,
    error: &GatewayError,
) {
    let finished_at_unix_ms = current_request_candidate_unix_ms();
    let latency_ms = finished_at_unix_ms.saturating_sub(started_at_unix_ms);
    let error_message = format!("Local stream candidate preparation failed: {error:?}");
    submit_local_request_candidate_status(
        state,
        plan,
        report_context,
        SchedulerRequestCandidateStatusUpdate {
            status: RequestCandidateStatus::Failed,
            status_code: Some(http::StatusCode::INTERNAL_SERVER_ERROR.as_u16()),
            error_type: Some("execution_candidate_preparation_failed".to_string()),
            error_message: Some(error_message.clone()),
            latency_ms: Some(latency_ms),
            started_at_unix_ms: Some(started_at_unix_ms),
            finished_at_unix_ms: Some(finished_at_unix_ms),
        },
    )
    .await;

    if state.usage_runtime.is_enabled() {
        let mut data = build_usage_event_data_seed(plan, report_context);
        data.status_code = Some(http::StatusCode::INTERNAL_SERVER_ERROR.as_u16());
        data.error_message = Some(error_message.clone());
        data.error_category = Some("server_error".to_string());
        data.response_time_ms = Some(latency_ms);
        let error_body = json!({
            "error": {
                "type": "execution_candidate_preparation_failed",
                "message": error_message,
                "code": http::StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            }
        });
        data.response_headers = Some(json!({"content-type": "application/json"}));
        data.response_body = Some(error_body.clone());
        data.client_response_headers = Some(json!({"content-type": "application/json"}));
        data.client_response_body = Some(error_body);
        state.usage_runtime.submit_terminal_event(
            state.data.as_ref(),
            UsageEvent::new(UsageEventType::Failed, plan.request_id.clone(), data),
        );
    }
}

fn execution_frame_max_bytes(stream_failover_enabled: bool, provider_api_format: &str) -> usize {
    if stream_failover_enabled && crate::ai_serving::is_openai_responses_format(provider_api_format)
    {
        STREAM_FAILOVER_EXECUTION_FRAME_MAX_BYTES
    } else {
        DEFAULT_EXECUTION_FRAME_MAX_BYTES
    }
}

fn extend_bounded(buffer: &mut Vec<u8>, bytes: &[u8], max_bytes: usize) {
    let remaining = max_bytes.saturating_sub(buffer.len());
    buffer.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
}

fn record_sync_terminal_usage(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewaySyncReportRequest,
) {
    let context_seed = build_terminal_usage_context_seed(plan, report_context);
    let payload_seed = build_sync_terminal_usage_payload_seed(payload);
    state
        .usage_runtime
        .record_sync_terminal(state.data.as_ref(), context_seed, payload_seed);
}

fn build_stream_sync_payload(
    trace_id: &str,
    report_kind: String,
    report_context: Option<Value>,
    status_code: u16,
    headers: BTreeMap<String, String>,
    body_json: Option<Value>,
    body_base64: Option<String>,
    telemetry: Option<ExecutionTelemetry>,
) -> GatewaySyncReportRequest {
    GatewaySyncReportRequest {
        trace_id: trace_id.to_string(),
        report_kind,
        report_context,
        status_code,
        headers,
        body_json,
        client_body_json: None,
        body_base64,
        telemetry,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_stream_error_sync_payload(
    trace_id: &str,
    report_kind: String,
    report_context: Option<Value>,
    upstream_status_code: u16,
    provider_headers: BTreeMap<String, String>,
    provider_body_json: Option<Value>,
    provider_body_base64: Option<String>,
    client_headers: BTreeMap<String, String>,
    client_body_json: Option<Value>,
    telemetry: Option<ExecutionTelemetry>,
) -> GatewaySyncReportRequest {
    let client_status_code =
        stream_client_error_status_code_for_upstream_status(upstream_status_code);
    let mut report_context = report_context;
    if client_status_code != upstream_status_code || client_headers != provider_headers {
        let mut object = match report_context {
            Some(Value::Object(object)) => object,
            Some(other) => serde_json::Map::from_iter([("seed".to_string(), other)]),
            None => serde_json::Map::new(),
        };
        object.insert(
            "client_response_status_code".to_string(),
            Value::from(client_status_code),
        );
        object.insert(
            "client_response_headers".to_string(),
            serde_json::to_value(client_headers).unwrap_or(Value::Null),
        );
        report_context = Some(Value::Object(object));
    }

    GatewaySyncReportRequest {
        trace_id: trace_id.to_string(),
        report_kind,
        report_context,
        status_code: upstream_status_code,
        headers: provider_headers,
        body_json: provider_body_json,
        client_body_json,
        body_base64: provider_body_base64,
        telemetry,
    }
}

fn record_stream_terminal_usage(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewayStreamReportRequest,
    cancelled: bool,
) {
    let context_seed = build_terminal_usage_context_seed(plan, report_context);
    let payload_seed = build_stream_terminal_usage_payload_seed(payload);
    state.usage_runtime.record_stream_terminal(
        state.data.as_ref(),
        context_seed,
        payload_seed,
        cancelled,
    );
}

struct StreamObjectCapture {
    store: Option<Arc<UsageObjectStore>>,
    request_id: String,
    field: UsageBodyField,
    object_key: Option<String>,
    content_type: String,
    enabled: bool,
    seen_bytes: u64,
    writer: Option<aether_data::UsageObjectStoreWriter>,
    error_message: Option<String>,
}

impl StreamObjectCapture {
    fn new(
        store: Option<Arc<UsageObjectStore>>,
        request_id: &str,
        field: UsageBodyField,
        content_type: Option<&str>,
        enabled: bool,
    ) -> Self {
        let object_key = store
            .as_ref()
            .map(|store| store.object_key(request_id, field.as_storage_field(), "raw"));
        Self {
            store,
            request_id: request_id.to_string(),
            field,
            object_key,
            content_type: content_type
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("application/octet-stream")
                .to_string(),
            enabled,
            seen_bytes: 0,
            writer: None,
            error_message: None,
        }
    }

    async fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.seen_bytes = self.seen_bytes.saturating_add(bytes.len() as u64);
        if !self.enabled || self.error_message.is_some() {
            return;
        }

        if self.writer.is_none() {
            let Some(store) = self.store.as_ref() else {
                self.error_message = Some("usage object store is not configured".to_string());
                return;
            };
            let Some(object_key) = self.object_key.as_deref() else {
                self.error_message = Some("usage object key could not be built".to_string());
                return;
            };
            match store.start_multipart(object_key).await {
                Ok(writer) => self.writer = Some(writer),
                Err(err) => {
                    self.error_message = Some(err.to_string());
                    return;
                }
            }
        }

        let write_result = self
            .writer
            .as_mut()
            .expect("stream object writer should exist after initialization")
            .write(bytes)
            .await;
        if let Err(err) = write_result {
            self.error_message = Some(err.to_string());
            if let Some(writer) = self.writer.take() {
                let _ = writer.abort().await;
            }
        }
    }

    async fn finish(self) -> (Option<Value>, UsageBodyCaptureState) {
        if !self.enabled || self.seen_bytes == 0 {
            return (None, UsageBodyCaptureState::None);
        }

        let body_ref = usage_body_ref(&self.request_id, self.field);
        if let Some(error_message) = self.error_message {
            return (
                Some(json!({
                    "body_ref": body_ref,
                    "object_key": self.object_key.clone(),
                    "payload_format": "raw",
                    "content_type": self.content_type,
                    "storage_status": "unavailable",
                    "size_bytes": self.seen_bytes,
                    "error_message": error_message,
                    "owned_by_current_capture": self.object_key.is_some(),
                })),
                UsageBodyCaptureState::Unavailable,
            );
        }

        let Some(mut writer) = self.writer else {
            return (
                Some(json!({
                    "body_ref": body_ref,
                    "object_key": self.object_key.clone(),
                    "payload_format": "raw",
                    "content_type": self.content_type,
                    "storage_status": "unavailable",
                    "size_bytes": self.seen_bytes,
                    "error_message": "usage object upload did not start",
                    "owned_by_current_capture": self.object_key.is_some(),
                })),
                UsageBodyCaptureState::Unavailable,
            );
        };
        match writer.complete(&self.content_type, "raw").await {
            Ok(stored) => (
                Some(json!({
                    "body_ref": body_ref,
                    "object_key": stored.object_key,
                    "payload_format": stored.payload_format,
                    "content_type": stored.content_type,
                    "storage_status": "available",
                    "size_bytes": stored.size_bytes,
                    "sha256": stored.sha256,
                    "owned_by_current_capture": true,
                })),
                UsageBodyCaptureState::Reference,
            ),
            Err(err) => (
                Some(json!({
                    "body_ref": body_ref,
                    "object_key": self.object_key.clone(),
                    "payload_format": "raw",
                    "content_type": self.content_type,
                    "storage_status": "unavailable",
                    "size_bytes": self.seen_bytes,
                    "error_message": err.to_string(),
                    "owned_by_current_capture": self.object_key.is_some(),
                })),
                UsageBodyCaptureState::Unavailable,
            ),
        }
    }
}

fn attach_stream_body_object_descriptors(
    report_context: &mut Option<Value>,
    provider_descriptor: Option<Value>,
    client_descriptor: Option<Value>,
) {
    if provider_descriptor.is_none() && client_descriptor.is_none() {
        return;
    }

    let context = report_context
        .get_or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut();
    let Some(context) = context else {
        return;
    };
    let mut objects = serde_json::Map::new();
    if let Some(descriptor) = provider_descriptor {
        if descriptor.get("storage_status").and_then(Value::as_str) == Some("available") {
            if let Some(body_ref) = descriptor.get("body_ref").and_then(Value::as_str) {
                context.insert(
                    "response_body_ref".to_string(),
                    Value::String(body_ref.to_string()),
                );
            }
        } else {
            context.remove("response_body_ref");
        }
        objects.insert("response_body".to_string(), descriptor);
    }
    if let Some(descriptor) = client_descriptor {
        if descriptor.get("storage_status").and_then(Value::as_str) == Some("available") {
            if let Some(body_ref) = descriptor.get("body_ref").and_then(Value::as_str) {
                context.insert(
                    "client_response_body_ref".to_string(),
                    Value::String(body_ref.to_string()),
                );
            }
        } else {
            context.remove("client_response_body_ref");
        }
        objects.insert("client_response_body".to_string(), descriptor);
    }
    context.insert("usage_body_objects".to_string(), Value::Object(objects));
}

fn build_stream_body_capture(
    body: &[u8],
    state: UsageBodyCaptureState,
    body_truncated: bool,
) -> (Option<String>, Option<UsageBodyCaptureState>) {
    let state = if !body.is_empty() && state == UsageBodyCaptureState::Unavailable {
        if body_truncated {
            UsageBodyCaptureState::Truncated
        } else {
            UsageBodyCaptureState::Inline
        }
    } else {
        state
    };
    let body_base64 = (!body.is_empty()
        && matches!(
            state,
            UsageBodyCaptureState::Inline | UsageBodyCaptureState::Truncated
        ))
    .then(|| base64::engine::general_purpose::STANDARD.encode(body));
    (body_base64, Some(state))
}

fn wrap_non_json_binary_stream_error_for_client(
    plan_kind: &str,
    headers: &BTreeMap<String, String>,
    error_body: &[u8],
) -> Result<Option<Value>, GatewayError> {
    let content_type = headers
        .get("content-type")
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if content_type.starts_with("application/json") {
        return Ok(None);
    }

    let body = match plan_kind {
        GEMINI_FILES_DOWNLOAD_PLAN_KIND => json!({
            "error": String::from_utf8_lossy(error_body).to_string(),
        }),
        OPENAI_VIDEO_CONTENT_PLAN_KIND => json!({
            "error": {
                "type": "upstream_error",
                "message": "Video not available",
            }
        }),
        _ => return Ok(None),
    };
    Ok(Some(body))
}

fn with_stream_error_trace_context(
    report_context: Option<&Value>,
    status_code: u16,
    headers: &BTreeMap<String, String>,
    body_json: Option<&Value>,
    body_bytes: &[u8],
    response_text: Option<&str>,
    local_failover_analysis: crate::orchestration::LocalFailoverAnalysis,
) -> Option<Value> {
    let body = trace_upstream_response_body(body_json, body_bytes);
    let upstream_context = with_upstream_response_report_context(
        report_context,
        status_code,
        Some(headers),
        body.as_ref(),
        None,
        None,
    );
    with_error_flow_report_context(
        upstream_context.as_ref().or(report_context),
        build_local_error_flow_metadata(status_code, response_text, local_failover_analysis),
    )
}

#[allow(clippy::too_many_arguments)] // stream report payload assembly mirrors runtime state
fn build_stream_usage_payload(
    trace_id: String,
    report_kind: String,
    report_context: Option<Value>,
    status_code: u16,
    headers: BTreeMap<String, String>,
    provider_body: &[u8],
    provider_body_state: UsageBodyCaptureState,
    provider_body_truncated: bool,
    client_body: &[u8],
    client_body_state: UsageBodyCaptureState,
    client_body_truncated: bool,
    terminal_summary: Option<ExecutionStreamTerminalSummary>,
    telemetry: Option<ExecutionTelemetry>,
) -> GatewayStreamReportRequest {
    let (provider_body_base64, provider_body_state) =
        build_stream_body_capture(provider_body, provider_body_state, provider_body_truncated);
    let (client_body_base64, client_body_state) =
        build_stream_body_capture(client_body, client_body_state, client_body_truncated);
    GatewayStreamReportRequest {
        trace_id,
        report_kind,
        report_context,
        status_code,
        headers,
        provider_body_base64,
        provider_body_state,
        client_body_base64,
        client_body_state,
        terminal_summary,
        telemetry,
    }
}

fn seed_kiro_report_context_input_tokens(plan: &ExecutionPlan, report_context: &mut Option<Value>) {
    if !plan
        .provider_name
        .as_deref()
        .is_some_and(|provider_name| provider_name.eq_ignore_ascii_case("Kiro"))
    {
        return;
    }

    let Some(context) = report_context.as_mut().and_then(Value::as_object_mut) else {
        return;
    };
    if context
        .get("input_tokens")
        .and_then(Value::as_u64)
        .is_some_and(|input_tokens| input_tokens > 0)
    {
        return;
    }

    let Some(original_request_body) = context.get("original_request_body").cloned() else {
        return;
    };
    let estimated_input_tokens = estimate_kiro_prompt_input_tokens(&original_request_body);
    context.insert(
        "input_tokens".to_string(),
        Value::from(estimated_input_tokens),
    );
}

async fn seed_kiro_simulated_cache_enabled(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: &mut Option<Value>,
) {
    if !plan
        .provider_name
        .as_deref()
        .is_some_and(|provider_name| provider_name.eq_ignore_ascii_case("Kiro"))
    {
        return;
    }

    let enabled = match state
        .read_provider_catalog_providers_by_ids(std::slice::from_ref(&plan.provider_id))
        .await
    {
        Ok(providers) => providers
            .iter()
            .find(|provider| provider.id == plan.provider_id)
            .filter(|provider| provider.provider_type.eq_ignore_ascii_case("kiro"))
            .is_some_and(|provider| {
                kiro_simulated_cache_enabled_from_provider_config(provider.config.as_ref())
            }),
        Err(err) => {
            warn!(
                event_name = "kiro_simulated_cache_config_read_failed",
                log_type = "event",
                request_id = %plan.request_id,
                provider_id = %plan.provider_id,
                error = ?err,
                "failed to read Kiro simulated cache provider config; defaulting disabled"
            );
            false
        }
    };

    let Some(context) = report_context.as_mut().and_then(Value::as_object_mut) else {
        return;
    };
    if enabled {
        context.insert(
            KIRO_SIMULATED_CACHE_ENABLED_CONTEXT_FIELD.to_string(),
            Value::Bool(true),
        );
    } else {
        context.remove(KIRO_SIMULATED_CACHE_ENABLED_CONTEXT_FIELD);
    }
}

fn seed_kiro_report_context_prompt_cache_usage(
    plan: &ExecutionPlan,
    report_context: &mut Option<Value>,
) {
    if !plan
        .provider_name
        .as_deref()
        .is_some_and(|provider_name| provider_name.eq_ignore_ascii_case("Kiro"))
    {
        return;
    }

    let simulated_cache_enabled =
        kiro_simulated_cache_enabled_from_report_context(report_context.as_ref());
    let Some(context) = report_context.as_mut().and_then(Value::as_object_mut) else {
        return;
    };
    if context
        .get("kiro_web_search_mcp")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return;
    }
    if !simulated_cache_enabled {
        return;
    }
    if kiro_cache_usage_from_context_object(context).is_some() {
        return;
    }

    let Some(original_request_body) = context.get("original_request_body").cloned() else {
        return;
    };
    let input_tokens = context
        .get("input_tokens")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            let estimated = estimate_kiro_prompt_input_tokens(&original_request_body);
            context.insert("input_tokens".to_string(), Value::from(estimated));
            estimated
        });
    let Some(profile) = build_kiro_prompt_cache_profile(&original_request_body, input_tokens)
    else {
        return;
    };

    let cache_usage = kiro_prompt_cache_tracker()
        .compute_and_update(kiro_stream_cache_credential_id(plan), &profile);
    if cache_usage.cache_creation_input_tokens == 0 && cache_usage.cache_read_input_tokens == 0 {
        return;
    }
    context.insert(
        "cache_creation_input_tokens".to_string(),
        Value::from(cache_usage.cache_creation_input_tokens),
    );
    context.insert(
        "cache_read_input_tokens".to_string(),
        Value::from(cache_usage.cache_read_input_tokens),
    );
}

fn kiro_stream_cache_credential_id(plan: &ExecutionPlan) -> String {
    format!("{}:{}:{}", plan.provider_id, plan.endpoint_id, plan.key_id)
}

fn kiro_cache_usage_from_context_object(
    context: &serde_json::Map<String, Value>,
) -> Option<KiroPromptCacheUsage> {
    let cache_creation_input_tokens = context
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read_input_tokens = context
        .get("cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    (cache_creation_input_tokens > 0 || cache_read_input_tokens > 0).then_some(
        KiroPromptCacheUsage {
            cache_creation_input_tokens,
            cache_read_input_tokens,
        },
    )
}

fn kiro_cache_usage_from_report_context(report_context: &Value) -> Option<KiroPromptCacheUsage> {
    report_context
        .as_object()
        .and_then(kiro_cache_usage_from_context_object)
}

fn maybe_apply_kiro_prompt_cache_usage_to_stream_summary(
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    summary: &mut Option<ExecutionStreamTerminalSummary>,
) {
    if !plan
        .provider_name
        .as_deref()
        .is_some_and(|provider_name| provider_name.eq_ignore_ascii_case("Kiro"))
    {
        return;
    }

    let Some(report_context) = report_context else {
        return;
    };
    let Some(original_request_body) = report_context.get("original_request_body") else {
        return;
    };
    let simulated_cache_enabled =
        kiro_simulated_cache_enabled_from_report_context(Some(report_context));

    let summary = summary.get_or_insert_with(ExecutionStreamTerminalSummary::default);
    let usage = summary
        .standardized_usage
        .get_or_insert_with(StandardizedUsage::new);
    let estimated_input_tokens = report_context
        .get("input_tokens")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            let estimated_input_tokens = estimate_kiro_prompt_input_tokens(original_request_body);
            if estimated_input_tokens > 0 {
                estimated_input_tokens
            } else {
                usage.input_tokens.max(0) as u64
            }
        });

    if !simulated_cache_enabled {
        usage.cache_creation_tokens = 0;
        usage.cache_read_tokens = 0;
        if usage.input_tokens <= 0 {
            usage.input_tokens = estimated_input_tokens as i64;
        }
        return;
    }

    if let Some(cache_usage) = kiro_cache_usage_from_report_context(report_context) {
        usage.input_tokens = kiro_billed_input_tokens(estimated_input_tokens, cache_usage) as i64;
        usage.cache_creation_tokens = cache_usage.cache_creation_input_tokens as i64;
        usage.cache_read_tokens = cache_usage.cache_read_input_tokens as i64;
        return;
    }

    if usage.cache_creation_tokens > 0 || usage.cache_read_tokens > 0 {
        if usage.input_tokens <= 0 {
            usage.input_tokens = kiro_billed_input_tokens(
                estimated_input_tokens,
                KiroPromptCacheUsage {
                    cache_creation_input_tokens: usage.cache_creation_tokens.max(0) as u64,
                    cache_read_input_tokens: usage.cache_read_tokens.max(0) as u64,
                },
            ) as i64;
        }
        return;
    }

    if usage.input_tokens <= 0 {
        usage.input_tokens = estimated_input_tokens as i64;
    }

    let Some(profile) =
        build_kiro_prompt_cache_profile(original_request_body, estimated_input_tokens)
    else {
        return;
    };

    let cache_usage = kiro_prompt_cache_tracker()
        .compute_and_update(kiro_stream_cache_credential_id(plan), &profile);
    if cache_usage.cache_creation_input_tokens == 0 && cache_usage.cache_read_input_tokens == 0 {
        return;
    }

    let billed_input_tokens = kiro_billed_input_tokens(estimated_input_tokens, cache_usage);
    usage.input_tokens = billed_input_tokens as i64;
    usage.cache_creation_tokens = cache_usage.cache_creation_input_tokens as i64;
    usage.cache_read_tokens = cache_usage.cache_read_input_tokens as i64;
}

fn append_stream_capture_bytes(
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    max_bytes: usize,
    truncated: &mut bool,
) {
    if chunk.is_empty() || max_bytes == 0 {
        return;
    }
    if buffer.len() >= max_bytes {
        *truncated = true;
        return;
    }
    let remaining = max_bytes - buffer.len();
    let keep_len = remaining.min(chunk.len());
    buffer.extend_from_slice(&chunk[..keep_len]);
    if keep_len < chunk.len() {
        *truncated = true;
    }
}

fn observe_stream_usage_bytes(
    observer: &mut StreamingStandardTerminalObserver,
    report_context: &Value,
    buffered: &mut Vec<u8>,
    chunk: &[u8],
) {
    if chunk.is_empty() {
        return;
    }

    buffered.extend_from_slice(chunk);
    while let Some(line_end) = buffered.iter().position(|byte| *byte == b'\n') {
        let line = buffered.drain(..=line_end).collect::<Vec<_>>();
        if let Err(err) = observer.push_line(report_context, line) {
            observer.disable_with_error(err.to_string());
            buffered.clear();
            break;
        }
    }
}

fn finalize_stream_usage_observer(
    observer: &mut Option<StreamingStandardTerminalObserver>,
    report_context: Option<&Value>,
    buffered: &mut Vec<u8>,
) -> Option<ExecutionStreamTerminalSummary> {
    let (Some(observer), Some(report_context)) = (observer.as_mut(), report_context) else {
        return None;
    };

    if !buffered.is_empty() {
        let line = std::mem::take(buffered);
        if let Err(err) = observer.push_line(report_context, line) {
            observer.disable_with_error(err.to_string());
        }
    }

    match observer.finish(report_context) {
        Ok(summary) => summary,
        Err(err) => {
            observer.disable_with_error(err.to_string());
            observer.latest_summary().cloned()
        }
    }
}

fn merge_stream_terminal_summary(
    mut current: Option<ExecutionStreamTerminalSummary>,
    observed: Option<ExecutionStreamTerminalSummary>,
) -> Option<ExecutionStreamTerminalSummary> {
    let Some(observed) = observed else {
        return current;
    };

    let Some(current_summary) = current.as_mut() else {
        return Some(observed);
    };

    if should_replace_stream_usage(
        current_summary.standardized_usage.as_ref(),
        observed.standardized_usage.as_ref(),
    ) {
        current_summary.standardized_usage = observed.standardized_usage;
    }
    if current_summary.finish_reason.is_none() {
        current_summary.finish_reason = observed.finish_reason;
    }
    if current_summary.response_id.is_none() {
        current_summary.response_id = observed.response_id;
    }
    if current_summary.model.is_none() {
        current_summary.model = observed.model;
    }
    current_summary.observed_finish |= observed.observed_finish;
    current_summary.unknown_event_count = current_summary
        .unknown_event_count
        .saturating_add(observed.unknown_event_count);
    if current_summary.parser_error.is_none() {
        current_summary.parser_error = observed.parser_error;
    }

    current
}

fn should_replace_stream_usage(
    current: Option<&aether_contracts::StandardizedUsage>,
    observed: Option<&aether_contracts::StandardizedUsage>,
) -> bool {
    let Some(observed) = observed else {
        return false;
    };
    let Some(current) = current else {
        return true;
    };

    observed.is_more_complete_than(current)
}

async fn execute_in_process_stream(
    state: &AppState,
    plan: &ExecutionPlan,
) -> Result<DirectUpstreamStreamExecution, ExecutionRuntimeTransportError> {
    if let Some(execution) = execute_stream_plan_via_local_tunnel(state, plan).await? {
        return Ok(execution);
    }

    match DirectSyncExecutionRuntime::new().execute_stream(plan).await {
        Ok(execution) => {
            record_manual_proxy_request_success(state, plan).await;
            Ok(execution)
        }
        Err(error) => {
            record_manual_proxy_request_failure(state, plan).await;
            Err(error)
        }
    }
}

async fn execute_in_process_stream_with_oauth_retry(
    state: &AppState,
    plan: &mut ExecutionPlan,
    trace_id: &str,
    report_context: Option<&Value>,
) -> Result<DirectUpstreamStreamExecution, ExecutionRuntimeTransportError> {
    let mut execution = execute_in_process_stream(state, plan).await?;
    apply_stream_summary_report_context(&mut execution, report_context);
    if execution.status_code >= 400
        && refresh_oauth_plan_auth_for_retry(state, plan, execution.status_code, None, trace_id)
            .await
    {
        execution = execute_in_process_stream(state, plan).await?;
        apply_stream_summary_report_context(&mut execution, report_context);
    }
    Ok(execution)
}

#[allow(clippy::too_many_arguments)] // internal function, grouping would add unnecessary indirection
pub(crate) async fn execute_execution_runtime_stream(
    state: &AppState,
    mut plan: ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    report_kind: Option<String>,
    mut report_context: Option<serde_json::Value>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let stream_started_at = Instant::now();
    bind_execution_plan_to_request_id(&mut plan, &mut report_context, trace_id);
    let candidate_started_unix_secs = current_request_candidate_unix_ms();
    let lifecycle_seed = build_lifecycle_usage_seed(&plan, report_context.as_ref());
    state
        .usage_runtime
        .record_pending(state.data.as_ref(), lifecycle_seed);
    if let Err(error) =
        ensure_execution_request_candidate_slot(state, &mut plan, &mut report_context).await
    {
        record_stream_candidate_preparation_failure(
            state,
            &plan,
            report_context.as_ref(),
            candidate_started_unix_secs,
            &error,
        )
        .await;
        return Err(error);
    }
    seed_kiro_report_context_input_tokens(&plan, &mut report_context);
    let request_candidate_status_snapshot =
        snapshot_local_request_candidate_status(&plan, report_context.as_ref());
    if let Some(snapshot) = request_candidate_status_snapshot.clone() {
        submit_local_request_candidate_status_snapshot(
            state,
            &snapshot,
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Pending,
                status_code: None,
                error_type: None,
                error_message: None,
                latency_ms: None,
                started_at_unix_ms: Some(candidate_started_unix_secs),
                finished_at_unix_ms: None,
            },
        )
        .await;
    }
    let plan_request_id_for_log = short_request_id(plan.request_id.as_str());
    let provider_name = plan
        .provider_name
        .clone()
        .unwrap_or_else(|| "-".to_string());
    let endpoint_id = plan.endpoint_id.clone();
    let key_id = plan.key_id.clone();
    let model_name = plan.model_name.clone().unwrap_or_else(|| "-".to_string());
    let candidate_index = parse_request_candidate_report_context(report_context.as_ref())
        .and_then(|context| context.candidate_index)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let mut provider_pool_in_flight_guard = acquire_provider_pool_in_flight_guard(
        state.runtime_state.clone(),
        &plan.provider_id,
        plan.request_id.as_str(),
        plan.candidate_id.as_deref(),
        key_id.as_str(),
    )
    .await;
    match maybe_execute_grok_stream(&plan, report_context.as_ref()).await {
        Ok(Some(grok_stream)) => {
            return execute_stream_from_frame_stream(
                state,
                plan,
                trace_id,
                decision,
                plan_kind,
                report_kind,
                grok_stream.report_context.or(report_context),
                candidate_started_unix_secs,
                stream_started_at,
                grok_stream.frame_stream,
                provider_pool_in_flight_guard.take(),
            )
            .await;
        }
        Ok(None) => {}
        Err(err) => {
            info!(
                event_name = "grok_execution_unavailable",
                log_type = "ops",
                trace_id = %trace_id,
                request_id = %plan_request_id_for_log,
                candidate_id = ?plan.candidate_id,
                provider_name = provider_name.as_str(),
                endpoint_id = %endpoint_id,
                key_id = %key_id,
                model_name = model_name.as_str(),
                candidate_index = candidate_index.as_str(),
                error = %err,
                "gateway Grok stream execution unavailable"
            );
            let terminal_unix_secs = current_request_candidate_unix_ms();
            submit_local_request_candidate_status(
                state,
                &plan,
                report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: None,
                    error_type: Some("grok_execution_unavailable".to_string()),
                    error_message: Some(format!("{err:?}")),
                    latency_ms: None,
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            return Ok(None);
        }
    }
    match maybe_execute_kiro_web_search_stream(state, &plan, report_context.as_ref()).await {
        Ok(Some(kiro_web_search)) => {
            return execute_stream_from_frame_stream(
                state,
                plan,
                trace_id,
                decision,
                plan_kind,
                report_kind,
                kiro_web_search.report_context.or(report_context),
                candidate_started_unix_secs,
                stream_started_at,
                kiro_web_search.frame_stream,
                provider_pool_in_flight_guard.take(),
            )
            .await;
        }
        Ok(None) => {}
        Err(err) => {
            info!(
                event_name = "kiro_web_search_mcp_unavailable",
                log_type = "ops",
                trace_id = %trace_id,
                request_id = %plan_request_id_for_log,
                candidate_id = ?plan.candidate_id,
                provider_name = provider_name.as_str(),
                endpoint_id = %endpoint_id,
                key_id = %key_id,
                model_name = model_name.as_str(),
                candidate_index = candidate_index.as_str(),
                error = %err,
                "gateway Kiro web_search MCP execution unavailable"
            );
            let terminal_unix_secs = current_request_candidate_unix_ms();
            submit_local_request_candidate_status(
                state,
                &plan,
                report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: None,
                    error_type: Some("kiro_web_search_mcp_unavailable".to_string()),
                    error_message: Some(format!("{err:?}")),
                    latency_ms: None,
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            return Ok(None);
        }
    }
    match maybe_execute_chatgpt_web_image_stream(state, &plan, report_context.as_ref()).await {
        Ok(Some(chatgpt_web_image)) => {
            return execute_stream_from_frame_stream(
                state,
                plan,
                trace_id,
                decision,
                plan_kind,
                report_kind,
                chatgpt_web_image.report_context.or(report_context),
                candidate_started_unix_secs,
                stream_started_at,
                chatgpt_web_image.frame_stream,
                provider_pool_in_flight_guard.take(),
            )
            .await;
        }
        Ok(None) => {}
        Err(err) => {
            info!(
                event_name = "chatgpt_web_image_execution_unavailable",
                log_type = "ops",
                trace_id = %trace_id,
                request_id = %plan_request_id_for_log,
                candidate_id = ?plan.candidate_id,
                provider_name = provider_name.as_str(),
                endpoint_id = %endpoint_id,
                key_id = %key_id,
                model_name = model_name.as_str(),
                candidate_index = candidate_index.as_str(),
                error = %err,
                "gateway ChatGPT-Web image stream execution unavailable"
            );
            let terminal_unix_secs = current_request_candidate_unix_ms();
            submit_local_request_candidate_status(
                state,
                &plan,
                report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: None,
                    error_type: Some("chatgpt_web_image_execution_unavailable".to_string()),
                    error_message: Some(format!("{err:?}")),
                    latency_ms: None,
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            return Ok(None);
        }
    }
    #[cfg(not(test))]
    {
        let execution = match execute_in_process_stream_with_oauth_retry(
            state,
            &mut plan,
            trace_id,
            report_context.as_ref(),
        )
        .await
        {
            Ok(execution) => execution,
            Err(err) => {
                info!(
                    event_name = "stream_execution_runtime_unavailable",
                    log_type = "ops",
                    trace_id = %trace_id,
                    request_id = %plan_request_id_for_log,
                    candidate_id = ?plan.candidate_id,
                    provider_name,
                    endpoint_id,
                    key_id,
                    model_name,
                    candidate_index = candidate_index.as_str(),
                    error = %err,
                    "gateway in-process stream execution unavailable"
                );
                let terminal_unix_secs = current_request_candidate_unix_ms();
                submit_local_request_candidate_status(
                    state,
                    &plan,
                    report_context.as_ref(),
                    SchedulerRequestCandidateStatusUpdate {
                        status: RequestCandidateStatus::Failed,
                        status_code: None,
                        error_type: Some("execution_runtime_unavailable".to_string()),
                        error_message: Some(format!("{err:?}")),
                        latency_ms: None,
                        started_at_unix_ms: Some(candidate_started_unix_secs),
                        finished_at_unix_ms: Some(terminal_unix_secs),
                    },
                )
                .await;
                return Ok(None);
            }
        };
        let frame_stream = build_direct_execution_frame_stream(execution).boxed();
        return execute_stream_from_frame_stream(
            state,
            plan,
            trace_id,
            decision,
            plan_kind,
            report_kind,
            report_context,
            candidate_started_unix_secs,
            stream_started_at,
            frame_stream,
            provider_pool_in_flight_guard.take(),
        )
        .await;
    }
    #[cfg(test)]
    {
        let remote_execution_runtime_base_url = state
            .execution_runtime_override_base_url()
            .unwrap_or_default();
        if remote_execution_runtime_base_url.trim().is_empty() {
            let execution = match execute_in_process_stream_with_oauth_retry(
                state,
                &mut plan,
                trace_id,
                report_context.as_ref(),
            )
            .await
            {
                Ok(execution) => execution,
                Err(err) => {
                    info!(
                        event_name = "stream_execution_runtime_unavailable",
                        log_type = "ops",
                        trace_id = %trace_id,
                        request_id = %plan_request_id_for_log,
                        candidate_id = ?plan.candidate_id,
                        provider_name,
                        endpoint_id,
                        key_id,
                        model_name,
                        candidate_index = candidate_index.as_str(),
                        error = %err,
                        "gateway in-process stream execution unavailable"
                    );
                    let terminal_unix_secs = current_request_candidate_unix_ms();
                    submit_local_request_candidate_status(
                        state,
                        &plan,
                        report_context.as_ref(),
                        SchedulerRequestCandidateStatusUpdate {
                            status: RequestCandidateStatus::Failed,
                            status_code: None,
                            error_type: Some("execution_runtime_unavailable".to_string()),
                            error_message: Some(err.to_string()),
                            latency_ms: None,
                            started_at_unix_ms: Some(candidate_started_unix_secs),
                            finished_at_unix_ms: Some(terminal_unix_secs),
                        },
                    )
                    .await;
                    return Ok(None);
                }
            };
            let frame_stream = build_direct_execution_frame_stream(execution).boxed();
            return execute_stream_from_frame_stream(
                state,
                plan,
                trace_id,
                decision,
                plan_kind,
                report_kind,
                report_context,
                candidate_started_unix_secs,
                stream_started_at,
                frame_stream,
                provider_pool_in_flight_guard.take(),
            )
            .await;
        }

        let execution_trace_id = report_context
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|context| context.get("trace_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(trace_id);
        let response = match post_stream_plan_to_remote_execution_runtime(
            state,
            remote_execution_runtime_base_url,
            Some(execution_trace_id),
            &plan,
        )
        .await
        {
            Ok(response) => response,
            Err(err) => {
                warn!(
                    event_name = "stream_execution_runtime_remote_unavailable",
                    log_type = "ops",
                    trace_id = %trace_id,
                    request_id = %plan_request_id_for_log,
                    candidate_id = ?plan.candidate_id,
                    error = ?err,
                    "gateway remote execution runtime stream unavailable"
                );
                let terminal_unix_secs = current_request_candidate_unix_ms();
                submit_local_request_candidate_status(
                    state,
                    &plan,
                    report_context.as_ref(),
                    SchedulerRequestCandidateStatusUpdate {
                        status: RequestCandidateStatus::Failed,
                        status_code: None,
                        error_type: Some("execution_runtime_unavailable".to_string()),
                        error_message: Some(format!("{err:?}")),
                        latency_ms: None,
                        started_at_unix_ms: Some(candidate_started_unix_secs),
                        finished_at_unix_ms: Some(terminal_unix_secs),
                    },
                )
                .await;
                return Ok(None);
            }
        };

        if response.status() != http::StatusCode::OK {
            let terminal_unix_secs = current_request_candidate_unix_ms();
            submit_local_request_candidate_status(
                state,
                &plan,
                report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: Some(response.status().as_u16()),
                    error_type: Some("execution_runtime_http_error".to_string()),
                    error_message: Some(format!(
                        "execution runtime returned HTTP {}",
                        response.status()
                    )),
                    latency_ms: None,
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            return Ok(Some(attach_control_metadata_headers(
                build_client_response(response, trace_id, Some(decision))?,
                Some(plan.request_id.as_str()),
                plan.candidate_id.as_deref(),
            )?));
        }

        let frame_stream = response
            .bytes_stream()
            .map_err(|err| IoError::other(err.to_string()))
            .boxed();
        return execute_stream_from_frame_stream(
            state,
            plan,
            trace_id,
            decision,
            plan_kind,
            report_kind,
            report_context,
            candidate_started_unix_secs,
            stream_started_at,
            frame_stream,
            provider_pool_in_flight_guard.take(),
        )
        .await;
    }
}

fn decode_stream_data_chunk(
    chunk_b64: Option<&str>,
    text: Option<&str>,
) -> Result<Vec<u8>, GatewayError> {
    if let Some(chunk_b64) = chunk_b64 {
        return base64::engine::general_purpose::STANDARD
            .decode(chunk_b64)
            .map_err(|err| GatewayError::Internal(err.to_string()));
    }
    Ok(text.unwrap_or_default().as_bytes().to_vec())
}

fn response_headers_indicate_sse(headers: &BTreeMap<String, String>) -> bool {
    headers
        .get("content-type")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

fn parse_prefetched_sync_json_body(body: &[u8]) -> Option<Value> {
    let stripped = strip_utf8_bom_and_ws(body);
    serde_json::from_slice::<Value>(stripped).ok()
}

fn encode_terminal_sse_error_event(failure: &StreamFailureReport) -> Result<Bytes, std::io::Error> {
    let payload = failure
        .to_json_string()
        .map_err(|err| IoError::other(err.to_string()))?;
    let mut event = String::from("event: aether.error\n");
    for line in payload.lines() {
        event.push_str("data: ");
        event.push_str(line);
        event.push('\n');
    }
    event.push('\n');
    Ok(Bytes::from(event))
}

fn encode_openai_responses_failed_event(
    failure: &StreamFailureReport,
) -> Result<Bytes, std::io::Error> {
    let failure_body = failure
        .to_json_string()
        .map_err(|err| IoError::other(err.to_string()))?;
    let failure_json: Value =
        serde_json::from_str(&failure_body).map_err(|err| IoError::other(err.to_string()))?;
    let error = failure_json.get("error").cloned().unwrap_or_else(|| {
        json!({
            "type": failure.error_type.as_str(),
            "message": failure.error_message.as_str(),
            "code": failure.status_code,
        })
    });
    let payload = serde_json::to_string(&json!({
        "type": "response.failed",
        "response": {
            "status": "failed",
            "error": error,
        }
    }))
    .map_err(|err| IoError::other(err.to_string()))?;
    Ok(Bytes::from(format!(
        "event: response.failed\ndata: {payload}\n\n"
    )))
}

fn image_stream_failed_event_name(report_context: Option<&Value>) -> &'static str {
    let operation = report_context
        .and_then(|value| value.get("image_request"))
        .and_then(|value| value.get("operation"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if operation == "edit" {
        "image_edit.failed"
    } else {
        "image_generation.failed"
    }
}

fn encode_openai_image_failed_event(
    report_context: Option<&Value>,
    failure: &StreamFailureReport,
) -> Result<Bytes, std::io::Error> {
    let event_name = image_stream_failed_event_name(report_context);
    let failure_body = failure
        .to_json_string()
        .map_err(|err| IoError::other(err.to_string()))?;
    let failure_json: Value =
        serde_json::from_str(&failure_body).map_err(|err| IoError::other(err.to_string()))?;
    let error = failure_json.get("error").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "type": failure.error_type.as_str(),
            "message": failure.error_message.as_str(),
            "code": failure.status_code,
        })
    });
    let payload = serde_json::json!({
        "type": event_name,
        "error": error,
    });
    let payload = serde_json::to_string(&payload).map_err(|err| IoError::other(err.to_string()))?;
    let mut event = format!("event: {event_name}\n");
    for line in payload.lines() {
        event.push_str("data: ");
        event.push_str(line);
        event.push('\n');
    }
    event.push('\n');
    Ok(Bytes::from(event))
}

fn resolve_openai_image_stream_total_timeout_ms(
    plan_kind: &str,
    plan: &ExecutionPlan,
) -> Option<u64> {
    if plan_kind != OPENAI_IMAGE_STREAM_PLAN_KIND {
        return None;
    }
    Some(
        plan.timeouts
            .as_ref()
            .and_then(|timeouts| timeouts.total_ms)
            .unwrap_or(OPENAI_IMAGE_STREAM_DEFAULT_TOTAL_TIMEOUT_MS)
            .max(1),
    )
}

fn should_limit_direct_finalize_prefetch(plan_kind: &str, has_local_stream_rewriter: bool) -> bool {
    plan_kind == OPENAI_IMAGE_STREAM_PLAN_KIND || has_local_stream_rewriter
}

fn build_sse_body_stream(
    prefetched_chunks_for_body: Vec<Bytes>,
    mut rx: mpsc::Receiver<Result<Bytes, IoError>>,
    emit_keepalive: bool,
    keepalive_interval: Duration,
) -> impl futures_util::Stream<Item = Result<Bytes, IoError>> + Send + 'static {
    stream! {
        let mut sent_prefetched_chunk = false;
        for chunk in prefetched_chunks_for_body {
            sent_prefetched_chunk = true;
            yield Ok(chunk);
        }

        if emit_keepalive {
            if !sent_prefetched_chunk {
                yield Ok(Bytes::from_static(SSE_KEEPALIVE_BYTES));
            }
            let mut keepalive = tokio::time::interval(keepalive_interval);
            keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
            keepalive.tick().await;
            loop {
                tokio::select! {
                    biased;
                    item = rx.recv() => {
                        let Some(item) = item else {
                            break;
                        };
                        yield item;
                    }
                    _ = keepalive.tick() => {
                        yield Ok(Bytes::from_static(SSE_KEEPALIVE_BYTES));
                    }
                }
            }
        } else {
            while let Some(item) = rx.recv().await {
                yield item;
            }
        }
    }
}

fn stream_chunk_contains_sse_done(chunk: &[u8]) -> bool {
    std::str::from_utf8(chunk)
        .ok()
        .is_some_and(|text| text.lines().any(|line| line.trim() == "data: [DONE]"))
}

#[derive(Default)]
struct OpenAiResponsesImageTerminalTracker {
    buffered: Vec<u8>,
    current_event: Option<String>,
    image_call_count: usize,
    image_result_count: usize,
    saw_response_completed: bool,
}

impl OpenAiResponsesImageTerminalTracker {
    const MAX_LINE_PREFIX_BYTES: usize = 64 * 1024;

    fn push(&mut self, chunk: &[u8]) {
        let mut remaining = chunk;
        while let Some(line_end) = remaining.iter().position(|byte| *byte == b'\n') {
            self.append_line_prefix(&remaining[..line_end]);
            let line = std::mem::take(&mut self.buffered);
            self.observe_line(&line);
            remaining = &remaining[line_end + 1..];
        }
        self.append_line_prefix(remaining);
    }

    fn append_line_prefix(&mut self, bytes: &[u8]) {
        let remaining_capacity = Self::MAX_LINE_PREFIX_BYTES.saturating_sub(self.buffered.len());
        self.buffered
            .extend_from_slice(&bytes[..bytes.len().min(remaining_capacity)]);
    }

    fn observe_line(&mut self, line: &[u8]) {
        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        if let Some(event_name) = line.trim().strip_prefix("event:").map(str::trim) {
            self.current_event = Some(event_name.to_string());
            if event_name.starts_with("response.image_generation_call.") {
                self.image_call_count = self.image_call_count.max(1);
            }
            return;
        }
        let Some(data) = line.trim().strip_prefix("data:") else {
            return;
        };
        let parsed = serde_json::from_str::<Value>(data.trim()).ok();
        let event_type = parsed
            .as_ref()
            .and_then(|event| event.get("type"))
            .and_then(Value::as_str)
            .or(self.current_event.as_deref())
            .unwrap_or_default();
        let contains_image_call = data.contains("\"type\":\"image_generation_call\"")
            || data.contains("\"type\": \"image_generation_call\"");
        if event_type.starts_with("response.image_generation_call.") {
            self.image_call_count = self.image_call_count.max(1);
        } else if event_type == "response.output_item.added" && contains_image_call {
            self.image_call_count = self.image_call_count.saturating_add(1);
        }
        if event_type == "response.output_item.done"
            && contains_image_call
            && contains_non_empty_json_string(data, "result")
        {
            self.image_result_count = self.image_result_count.saturating_add(1);
            self.image_call_count = self.image_call_count.max(self.image_result_count);
        }
        if event_type == "response.completed" {
            self.saw_response_completed = true;
            if self.image_result_count == 0
                && contains_image_call
                && contains_non_empty_json_string(data, "result")
            {
                self.image_call_count = 1;
                self.image_result_count = 1;
            }
        }
    }

    fn incomplete_at_eof(&self) -> bool {
        self.image_call_count > 0
            && (self.image_result_count < self.image_call_count || !self.saw_response_completed)
    }
}

fn contains_non_empty_json_string(data: &str, field: &str) -> bool {
    let compact_marker = format!("\"{field}\":\"");
    let spaced_marker = format!("\"{field}\": \"");
    [compact_marker.as_str(), spaced_marker.as_str()]
        .iter()
        .find_map(|marker| data.find(marker).map(|index| (index, marker.len())))
        .and_then(|(index, marker_len)| data.as_bytes().get(index + marker_len))
        .is_some_and(|next| *next != b'"')
}

async fn next_stream_frame<R>(
    buffered_frames: &mut VecDeque<StreamFrame>,
    lines: &mut FramedRead<R, LinesCodec>,
) -> Result<Option<StreamFrame>, GatewayError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    if let Some(frame) = buffered_frames.pop_front() {
        return Ok(Some(frame));
    }
    read_next_frame(lines).await
}

async fn next_stream_frame_until_downstream_closed<R>(
    buffered_frames: &mut VecDeque<StreamFrame>,
    lines: &mut FramedRead<R, LinesCodec>,
    tx: &mpsc::Sender<Result<Bytes, IoError>>,
) -> Result<Option<StreamFrame>, GatewayError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    if let Some(frame) = buffered_frames.pop_front() {
        return Ok(Some(frame));
    }

    tokio::select! {
        frame = read_next_frame(lines) => frame,
        () = tx.closed() => Ok(None),
    }
}

fn should_refresh_stream_usage_telemetry(
    previous: Option<&ExecutionTelemetry>,
    next: &ExecutionTelemetry,
) -> bool {
    let previous_ttfb = previous.and_then(|telemetry| telemetry.ttfb_ms);
    let previous_elapsed = previous.and_then(|telemetry| telemetry.elapsed_ms);
    let next_ttfb = next.ttfb_ms;
    let next_elapsed = next.elapsed_ms;

    (next_ttfb.is_some() && next_ttfb != previous_ttfb)
        || (next_elapsed.is_some() && next_elapsed != previous_elapsed)
}

fn build_terminal_stream_telemetry(
    stream_started_at: Instant,
    telemetry: Option<&ExecutionTelemetry>,
    usage_stream_telemetry: Option<&ExecutionTelemetry>,
    upstream_bytes: u64,
) -> ExecutionTelemetry {
    let current_elapsed_ms = stream_started_at
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let ttfb_ms = telemetry
        .and_then(|telemetry| telemetry.ttfb_ms)
        .or_else(|| usage_stream_telemetry.and_then(|telemetry| telemetry.ttfb_ms));
    let prior_elapsed_ms = telemetry
        .and_then(|telemetry| telemetry.elapsed_ms)
        .or_else(|| usage_stream_telemetry.and_then(|telemetry| telemetry.elapsed_ms))
        .unwrap_or(0);
    let elapsed_ms = current_elapsed_ms
        .max(prior_elapsed_ms)
        .max(ttfb_ms.unwrap_or(0));
    ExecutionTelemetry {
        ttfb_ms,
        elapsed_ms: Some(elapsed_ms),
        upstream_bytes: Some(upstream_bytes),
    }
}

fn should_skip_direct_finalize_prefetch(
    direct_stream_finalize_kind: Option<&str>,
    content_type: Option<&str>,
    provider_api_format: &str,
    client_api_format: &str,
    has_private_stream_normalizer: bool,
    has_local_stream_rewriter: bool,
) -> bool {
    if direct_stream_finalize_kind.is_none() {
        return false;
    }

    let content_type = content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("text/event-stream") {
        return !provider_api_format
            .trim()
            .to_ascii_lowercase()
            .starts_with("openai:responses");
    }

    if has_private_stream_normalizer || has_local_stream_rewriter {
        return false;
    }

    if !provider_api_format.eq_ignore_ascii_case(client_api_format) {
        return false;
    }

    if content_type.is_empty() {
        return true;
    }

    !(content_type.contains("json") || content_type.ends_with("+json"))
}

fn should_probe_success_failover_before_stream(headers: &BTreeMap<String, String>) -> bool {
    let content_type = headers
        .get("content-type")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase();

    content_type.contains("json") || content_type.ends_with("+json")
}

async fn probe_local_stream_success_failover_text<R>(
    buffered_frames: &mut VecDeque<StreamFrame>,
    lines: &mut FramedRead<R, LinesCodec>,
) -> Result<Option<String>, GatewayError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    while let Some(frame) = read_next_frame(lines).await? {
        let probe_text = match &frame.payload {
            StreamFramePayload::Data { chunk_b64, text } => {
                match decode_stream_data_chunk(chunk_b64.as_deref(), text.as_deref()) {
                    Ok(chunk) if !chunk.is_empty() => {
                        Some(String::from_utf8_lossy(&chunk).into_owned())
                    }
                    Ok(_) | Err(_) => None,
                }
            }
            StreamFramePayload::Error { .. } | StreamFramePayload::Eof { .. } => None,
            StreamFramePayload::Headers { .. } | StreamFramePayload::Telemetry { .. } => None,
        };
        buffered_frames.push_back(frame);
        if probe_text.is_some() {
            return Ok(probe_text);
        }
    }

    Ok(None)
}

async fn execute_stream_from_frame_stream(
    state: &AppState,
    plan: ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    report_kind: Option<String>,
    report_context: Option<serde_json::Value>,
    candidate_started_unix_secs: u64,
    stream_started_at: Instant,
    frame_stream: BoxStream<'static, Result<Bytes, IoError>>,
    in_flight_guard: Option<ProviderPoolInFlightGuard>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let request_id = plan.request_id.as_str();
    let request_id_for_log = short_request_id(request_id);
    let candidate_id = plan.candidate_id.as_deref();
    let provider_name = plan.provider_name.as_deref().unwrap_or("-");
    let model_name = plan.model_name.as_deref().unwrap_or("-");
    let lifecycle_seed = build_lifecycle_usage_seed(&plan, report_context.as_ref());
    let request_candidate_status_snapshot =
        snapshot_local_request_candidate_status(&plan, report_context.as_ref());
    let candidate_index = parse_request_candidate_report_context(report_context.as_ref())
        .and_then(|context| context.candidate_index)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let stream_failover_policy =
        resolve_local_stream_failover_policy(state, &plan, report_context.as_ref()).await;
    let frame_max_bytes = execution_frame_max_bytes(
        stream_failover_policy.enabled,
        plan.provider_api_format.as_str(),
    );
    let reader = StreamReader::new(frame_stream);
    let mut lines = FramedRead::new(reader, LinesCodec::new_with_max_length(frame_max_bytes));

    let first_frame = read_next_frame(&mut lines).await?.ok_or_else(|| {
        GatewayError::Internal("execution runtime stream ended before headers frame".to_string())
    })?;
    let StreamFramePayload::Headers {
        status_code,
        mut headers,
    } = first_frame.payload
    else {
        return Err(GatewayError::Internal(
            "execution runtime stream must start with headers frame".to_string(),
        ));
    };
    let mut report_context =
        attach_provider_response_headers_to_report_context(report_context, &headers);
    seed_kiro_report_context_input_tokens(&plan, &mut report_context);
    if status_code == 200 {
        seed_kiro_simulated_cache_enabled(state, &plan, &mut report_context).await;
        seed_kiro_report_context_prompt_cache_usage(&plan, &mut report_context);
    }
    let mut buffered_frames = VecDeque::new();
    let mut stream_terminal_summary: Option<ExecutionStreamTerminalSummary> = None;
    if status_code == 200 && should_probe_success_failover_before_stream(&headers) {
        let success_probe_text =
            probe_local_stream_success_failover_text(&mut buffered_frames, &mut lines).await?;
        if should_retry_next_local_candidate_stream(
            state,
            &plan,
            plan_kind,
            report_context.as_ref(),
            status_code,
            success_probe_text.as_deref(),
        )
        .await
        {
            let terminal_unix_secs = current_request_candidate_unix_ms();
            submit_local_request_candidate_status(
                state,
                &plan,
                report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: Some(status_code),
                    error_type: Some("success_failover_pattern".to_string()),
                    error_message: Some(
                        "execution runtime stream matched provider success failover rule"
                            .to_string(),
                    ),
                    latency_ms: None,
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            warn!(
                event_name = "local_stream_candidate_retry_scheduled",
                log_type = "event",
                trace_id = %trace_id,
                request_id = %request_id_for_log,
                status_code,
                provider_name = provider_name,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                model_name,
                candidate_index = candidate_index.as_str(),
                "gateway local stream decision retrying next candidate after success failover rule match"
            );
            return Ok(None);
        }
    }

    let stream_error_finalize_kind =
        resolve_core_stream_error_finalize_report_kind(plan_kind, status_code);

    if !(200..300).contains(&status_code) {
        let provider_error_body = collect_error_body(&mut lines).await?;
        let synthetic_body_json =
            should_synthesize_non_success_stream_error_body(status_code, &provider_error_body)
                .then(|| build_synthetic_non_success_stream_error_body(status_code, &headers));
        let (provider_body_json, provider_body_base64) =
            decode_stream_error_body(&headers, &provider_error_body);
        let client_status_code = stream_client_error_status_code_for_upstream_status(status_code);
        let wrapped_binary_body_json = wrap_non_json_binary_stream_error_for_client(
            plan_kind,
            &headers,
            &provider_error_body,
        )?;
        let (mut client_body_json, mut client_error_body) =
            if let Some(body_json) = synthetic_body_json.or(wrapped_binary_body_json) {
                let body_bytes = serde_json::to_vec(&body_json)
                    .map_err(|err| GatewayError::Internal(err.to_string()))?;
                (Some(body_json), body_bytes)
            } else {
                (provider_body_json.clone(), provider_error_body.clone())
            };
        let error_response_text =
            local_failover_response_text(client_body_json.as_ref(), &client_error_body, None);
        let failover_analysis = resolve_local_candidate_failover_analysis_stream(
            state,
            &plan,
            report_context.as_ref(),
            status_code,
            error_response_text.as_deref(),
        )
        .await;
        apply_local_execution_effect(
            state,
            LocalExecutionEffectContext {
                plan: &plan,
                report_context: report_context.as_ref(),
            },
            LocalExecutionEffect::AttemptFailure(LocalAttemptFailureEffect {
                status_code,
                classification: failover_analysis.classification,
            }),
        )
        .await;
        apply_local_execution_effect(
            state,
            LocalExecutionEffectContext {
                plan: &plan,
                report_context: report_context.as_ref(),
            },
            LocalExecutionEffect::AdaptiveRateLimit(LocalAdaptiveRateLimitEffect {
                status_code,
                classification: failover_analysis.classification,
                headers: Some(&headers),
                error_body: error_response_text.as_deref(),
            }),
        )
        .await;
        apply_local_execution_effect(
            state,
            LocalExecutionEffectContext {
                plan: &plan,
                report_context: report_context.as_ref(),
            },
            LocalExecutionEffect::HealthFailure(LocalHealthFailureEffect {
                status_code,
                classification: failover_analysis.classification,
                error_body: error_response_text.as_deref(),
            }),
        )
        .await;
        apply_local_execution_effect(
            state,
            LocalExecutionEffectContext {
                plan: &plan,
                report_context: report_context.as_ref(),
            },
            LocalExecutionEffect::OauthInvalidation(LocalOAuthInvalidationEffect {
                status_code,
                response_text: error_response_text.as_deref(),
            }),
        )
        .await;
        apply_local_execution_effect(
            state,
            LocalExecutionEffectContext {
                plan: &plan,
                report_context: report_context.as_ref(),
            },
            LocalExecutionEffect::PoolError(LocalPoolErrorEffect {
                status_code,
                classification: failover_analysis.classification,
                headers: &headers,
                error_body: error_response_text.as_deref(),
            }),
        )
        .await;
        let failover_decision = failover_analysis.decision;
        debug!(
            event_name = "execution_runtime_stream_failover_decided",
            log_type = "debug",
            trace_id = %trace_id,
            request_id = %request_id_for_log,
            candidate_id = ?candidate_id,
            plan_kind,
            status_code,
            provider_name,
            endpoint_id = %plan.endpoint_id,
            key_id = %plan.key_id,
            model_name,
            candidate_index = candidate_index.as_str(),
            failover_decision = failover_decision.as_str(),
            "gateway resolved execution runtime stream failover decision"
        );
        if matches!(failover_decision, LocalFailoverDecision::RetryNextCandidate) {
            let terminal_unix_secs = current_request_candidate_unix_ms();
            let error_trace_report_context = with_stream_error_trace_context(
                report_context.as_ref(),
                status_code,
                &headers,
                provider_body_json.as_ref(),
                &provider_error_body,
                error_response_text.as_deref(),
                failover_analysis,
            );
            submit_local_request_candidate_status(
                state,
                &plan,
                error_trace_report_context
                    .as_ref()
                    .or(report_context.as_ref()),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: Some(status_code),
                    error_type: Some("retryable_upstream_status".to_string()),
                    error_message: Some(format!(
                        "execution runtime stream returned retryable status {status_code}"
                    )),
                    latency_ms: None,
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            warn!(
                event_name = "local_stream_candidate_retry_scheduled",
                log_type = "event",
                trace_id = %trace_id,
                request_id = %request_id_for_log,
                status_code,
                provider_name = provider_name,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                model_name,
                candidate_index = candidate_index.as_str(),
                "gateway local stream decision retrying next candidate after retryable execution runtime status"
            );
            return Ok(None);
        }

        if !matches!(failover_decision, LocalFailoverDecision::StopLocalFailover)
            && should_fallback_to_control_stream(
                plan_kind,
                status_code,
                stream_error_finalize_kind.is_some(),
            )
        {
            let terminal_unix_secs = current_request_candidate_unix_ms();
            let error_trace_report_context = with_stream_error_trace_context(
                report_context.as_ref(),
                status_code,
                &headers,
                provider_body_json.as_ref(),
                &provider_error_body,
                error_response_text.as_deref(),
                failover_analysis,
            );
            submit_local_request_candidate_status(
                state,
                &plan,
                error_trace_report_context
                    .as_ref()
                    .or(report_context.as_ref()),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: Some(status_code),
                    error_type: Some("control_fallback".to_string()),
                    error_message: Some(format!(
                        "stream decision fell back to control after status {status_code}"
                    )),
                    latency_ms: None,
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            return Ok(None);
        }

        let mut client_headers = if (300..400).contains(&status_code) {
            let mut headers = synthetic_error_response_headers(headers.clone());
            headers.insert(
                "x-aether-upstream-status".to_string(),
                status_code.to_string(),
            );
            headers
        } else {
            headers.clone()
        };
        apply_endpoint_response_header_rules(
            state,
            &plan,
            &mut client_headers,
            client_body_json.as_ref(),
        )
        .await?;
        let context = build_niffler_upstream_error_context(&plan, report_context.as_ref());
        if let Some(rewrite) = rewrite_niffler_upstream_error_response(
            state,
            context,
            status_code,
            provider_body_json.as_ref(),
            &provider_error_body,
        )
        .await
        {
            client_headers.remove("content-encoding");
            client_headers.remove("content-length");
            client_headers.insert("content-type".to_string(), "application/json".to_string());
            client_body_json = Some(rewrite.body_json);
            client_error_body = rewrite.body_bytes;
        }

        let client_response_headers = client_headers.clone();
        let error_trace_report_context = with_stream_error_trace_context(
            report_context.as_ref(),
            status_code,
            &headers,
            provider_body_json.as_ref(),
            &provider_error_body,
            error_response_text.as_deref(),
            failover_analysis,
        );
        let payload = build_stream_error_sync_payload(
            trace_id,
            stream_error_finalize_kind
                .as_deref()
                .or(report_kind.as_deref())
                .unwrap_or_default()
                .to_string(),
            error_trace_report_context.or(report_context),
            status_code,
            headers.clone(),
            provider_body_json,
            provider_body_base64,
            client_headers,
            client_body_json,
            None,
        );
        record_sync_terminal_usage(state, &plan, payload.report_context.as_ref(), &payload);
        let terminal_unix_secs = current_request_candidate_unix_ms();
        submit_local_request_candidate_status(
            state,
            &plan,
            payload.report_context.as_ref(),
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Failed,
                status_code: Some(status_code),
                error_type: Some("execution_runtime_stream_non_success_status".to_string()),
                error_message: Some(format!(
                    "execution runtime stream returned non-success status {status_code}"
                )),
                latency_ms: None,
                started_at_unix_ms: Some(candidate_started_unix_secs),
                finished_at_unix_ms: Some(terminal_unix_secs),
            },
        )
        .await;
        if stream_error_finalize_kind.is_some() {
            let response =
                submit_local_core_error_or_sync_finalize(state, trace_id, decision, payload)
                    .await?;
            return Ok(Some(attach_control_metadata_headers(
                response,
                Some(request_id),
                candidate_id,
            )?));
        }
        return Ok(Some(attach_control_metadata_headers(
            build_client_response_from_parts(
                client_status_code,
                &client_response_headers,
                Body::from(client_error_body),
                trace_id,
                Some(decision),
            )?,
            Some(request_id),
            candidate_id,
        )?));
    }

    let direct_stream_finalize_kind = resolve_core_stream_direct_finalize_report_kind(plan_kind);
    let normalized_stream_report_context =
        normalize_provider_private_report_context(report_context.as_ref());
    let upstream_headers = headers.clone();
    let mut private_stream_normalizer =
        maybe_build_provider_private_stream_normalizer(report_context.as_ref());
    let mut local_stream_rewriter =
        maybe_build_stream_response_rewriter(normalized_stream_report_context.as_ref());
    if private_stream_normalizer.is_some() || local_stream_rewriter.is_some() {
        headers.remove("content-encoding");
        headers.remove("content-length");
        headers.insert("content-type".to_string(), "text/event-stream".to_string());
    }
    let upstream_content_type = upstream_headers.get("content-type").map(String::as_str);
    let prefetch_openai_responses_sse = stream_failover_policy.enabled
        && upstream_content_type.is_some_and(|content_type| {
            content_type
                .to_ascii_lowercase()
                .contains("text/event-stream")
        })
        && crate::ai_serving::is_openai_responses_format(plan.provider_api_format.as_str());
    if prefetch_openai_responses_sse {
        debug!(
            event_name = "stream_failover_prefetch_started",
            log_type = "event",
            trace_id = %trace_id,
            request_id = %request_id_for_log,
            provider_name,
            endpoint_id = %plan.endpoint_id,
            key_id = %plan.key_id,
            model_name,
            max_wait_ms = stream_failover_policy.max_wait_ms,
            max_buffer_bytes = stream_failover_policy.max_buffer_bytes,
            cooldown_seconds = stream_failover_policy.cooldown_seconds,
            "gateway started OpenAI Responses pre-output failover"
        );
    }
    let skip_direct_finalize_prefetch = should_skip_direct_finalize_prefetch(
        direct_stream_finalize_kind.as_deref(),
        upstream_content_type,
        plan.provider_api_format.as_str(),
        plan.client_api_format.as_str(),
        private_stream_normalizer.is_some(),
        local_stream_rewriter.is_some(),
    ) && !prefetch_openai_responses_sse;
    let limit_direct_finalize_prefetch =
        should_limit_direct_finalize_prefetch(plan_kind, local_stream_rewriter.is_some())
            || prefetch_openai_responses_sse;
    let mut prefetched_chunks: Vec<Bytes> = Vec::new();
    let mut provider_prefetched_body = Vec::new();
    let mut prefetched_body = Vec::new();
    let mut prefetched_inspection_body = Vec::new();
    let mut prefetched_telemetry: Option<ExecutionTelemetry> = None;
    let mut reached_eof = false;
    let mut sync_json_stream_bridge_active = false;
    let mut prefetch_committed_output = false;
    let stream_failover_deadline = prefetch_openai_responses_sse
        .then(|| Instant::now() + Duration::from_millis(stream_failover_policy.max_wait_ms));
    if skip_direct_finalize_prefetch {
        debug!(
            event_name = "execution_runtime_stream_prefetch_skipped",
            log_type = "debug",
            trace_id = %trace_id,
            request_id = %request_id_for_log,
            candidate_id = ?candidate_id,
            plan_kind,
            provider_name,
            endpoint_id = %plan.endpoint_id,
            key_id = %plan.key_id,
            model_name,
            candidate_index = candidate_index.as_str(),
            content_type = upstream_content_type.unwrap_or("-"),
            provider_api_format = plan.provider_api_format.as_str(),
            client_api_format = plan.client_api_format.as_str(),
            "gateway skipped direct finalize prefetch for same-format passthrough stream"
        );
    }
    if let Some(report_kind) = direct_stream_finalize_kind
        .as_ref()
        .filter(|_| !skip_direct_finalize_prefetch)
    {
        while if prefetch_openai_responses_sse {
            prefetched_inspection_body.len() < stream_failover_policy.max_buffer_bytes
        } else {
            prefetched_chunks.len() < MAX_STREAM_PREFETCH_FRAMES
                && prefetched_inspection_body.len() < MAX_STREAM_PREFETCH_BYTES
        } {
            let next_frame_result = if let Some(deadline) = stream_failover_deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    debug!(
                        event_name = "stream_failover_prefetch_committed",
                        log_type = "event",
                        trace_id = %trace_id,
                        request_id = %request_id_for_log,
                        provider_name,
                        endpoint_id = %plan.endpoint_id,
                        key_id = %plan.key_id,
                        model_name,
                        reason = "max_wait_reached",
                        max_wait_ms = stream_failover_policy.max_wait_ms,
                        buffered_bytes = prefetched_inspection_body.len(),
                        "gateway committed the OpenAI Responses stream after the configured wait"
                    );
                    break;
                }
                match tokio::time::timeout(
                    remaining,
                    next_stream_frame(&mut buffered_frames, &mut lines),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => {
                        debug!(
                            event_name = "stream_failover_prefetch_committed",
                            log_type = "event",
                            trace_id = %trace_id,
                            request_id = %request_id_for_log,
                            provider_name,
                            endpoint_id = %plan.endpoint_id,
                            key_id = %plan.key_id,
                            model_name,
                            reason = "max_wait_reached",
                            max_wait_ms = stream_failover_policy.max_wait_ms,
                            buffered_bytes = prefetched_inspection_body.len(),
                            "gateway committed the OpenAI Responses stream after the configured wait"
                        );
                        break;
                    }
                }
            } else if limit_direct_finalize_prefetch {
                match tokio::time::timeout(
                    REWRITTEN_STREAM_PREFETCH_TIMEOUT,
                    next_stream_frame(&mut buffered_frames, &mut lines),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => {
                        debug!(
                            event_name = "execution_runtime_stream_prefetch_limited",
                            log_type = "debug",
                            trace_id = %trace_id,
                            request_id = %request_id_for_log,
                            candidate_id = ?candidate_id,
                            plan_kind,
                            report_kind,
                            provider_name,
                            endpoint_id = %plan.endpoint_id,
                            key_id = %plan.key_id,
                            model_name,
                            candidate_index = candidate_index.as_str(),
                            timeout_ms = REWRITTEN_STREAM_PREFETCH_TIMEOUT.as_millis() as u64,
                            "gateway stopped rewritten stream prefetch before client-visible body"
                        );
                        break;
                    }
                }
            } else {
                next_stream_frame(&mut buffered_frames, &mut lines).await
            };
            let Some(frame) = (match next_frame_result {
                Ok(frame) => frame,
                Err(err) => {
                    let failure = build_stream_failure_report(
                        "execution_runtime_stream_frame_decode_error",
                        format!("failed to decode execution runtime stream frame: {err:?}"),
                        502,
                    );
                    return handle_prefetch_stream_failure(
                        state,
                        trace_id,
                        decision,
                        &plan,
                        report_context,
                        request_id,
                        candidate_id,
                        report_kind,
                        headers,
                        prefetched_telemetry,
                        &provider_prefetched_body,
                        failure,
                    )
                    .await;
                }
            }) else {
                reached_eof = true;
                break;
            };
            match frame.payload {
                StreamFramePayload::Data { chunk_b64, text } => {
                    let chunk =
                        match decode_stream_data_chunk(chunk_b64.as_deref(), text.as_deref()) {
                            Ok(chunk) => chunk,
                            Err(err) => {
                                let failure = build_stream_failure_report(
                                    "execution_runtime_stream_chunk_decode_error",
                                    format!(
                                        "failed to decode execution runtime stream chunk: {err:?}"
                                    ),
                                    502,
                                );
                                return handle_prefetch_stream_failure(
                                    state,
                                    trace_id,
                                    decision,
                                    &plan,
                                    report_context,
                                    request_id,
                                    candidate_id,
                                    report_kind,
                                    headers,
                                    prefetched_telemetry,
                                    &prefetched_body,
                                    failure,
                                )
                                .await;
                            }
                        };

                    if chunk.is_empty() {
                        continue;
                    }

                    if prefetch_openai_responses_sse {
                        extend_bounded(
                            &mut provider_prefetched_body,
                            &chunk,
                            stream_failover_policy.max_buffer_bytes,
                        );
                        extend_bounded(
                            &mut prefetched_inspection_body,
                            &chunk,
                            stream_failover_policy.max_buffer_bytes,
                        );
                    } else {
                        provider_prefetched_body.extend_from_slice(&chunk);
                        prefetched_inspection_body.extend_from_slice(&chunk);
                    }

                    let inspection = inspect_prefetched_stream_body(
                        &upstream_headers,
                        &prefetched_inspection_body,
                    );
                    match inspection {
                        StreamPrefetchInspection::EmbeddedError(body_json) => {
                            debug!(
                                event_name = "execution_runtime_stream_prefetch_embedded_error_detected",
                                log_type = "debug",
                                trace_id = %trace_id,
                                request_id = %request_id_for_log,
                                candidate_id = ?candidate_id,
                                plan_kind,
                                report_kind,
                                provider_name,
                                endpoint_id = %plan.endpoint_id,
                                key_id = %plan.key_id,
                                model_name,
                                candidate_index = candidate_index.as_str(),
                                provider_prefetched_body_bytes = provider_prefetched_body.len(),
                                "gateway detected embedded error while prefetching execution runtime stream"
                            );
                            if prefetch_openai_responses_sse {
                                let effective_status_code =
                                    resolve_local_sync_error_status_code(status_code, &body_json);
                                let error_body = serde_json::to_string(&body_json).ok();
                                let retryable_stream_error =
                                    error_body.as_deref().is_some_and(|body| {
                                        is_retryable_openai_transient_processing_error(
                                            effective_status_code,
                                            body,
                                        )
                                    });
                                let should_pause_model =
                                    effective_status_code == 503 && retryable_stream_error;
                                let payload = build_stream_sync_payload(
                                    trace_id,
                                    report_kind.clone(),
                                    report_context,
                                    effective_status_code,
                                    headers,
                                    Some(body_json),
                                    None,
                                    prefetched_telemetry,
                                );
                                let failure_analysis = record_stream_sync_failure(
                                    state,
                                    &plan,
                                    payload.report_context.as_ref(),
                                    &payload,
                                    Some(candidate_started_unix_secs),
                                )
                                .await;
                                if should_pause_model {
                                    if let Some(provider_model_name) = plan.model_name.as_deref() {
                                        record_admin_provider_pool_model_cooldown(
                                            state.runtime_state.as_ref(),
                                            &plan.provider_id,
                                            &plan.key_id,
                                            provider_model_name,
                                            "openai_transient_model_capacity",
                                            stream_failover_policy.cooldown_seconds,
                                        )
                                        .await;
                                    }
                                }
                                if retryable_stream_error
                                    && matches!(
                                        failure_analysis.decision,
                                        LocalFailoverDecision::RetryNextCandidate
                                    )
                                {
                                    warn!(
                                        event_name = "stream_failover_retry_scheduled",
                                        log_type = "event",
                                        trace_id = %trace_id,
                                        request_id = %request_id_for_log,
                                        upstream_status_code = status_code,
                                        effective_status_code,
                                        provider_name,
                                        endpoint_id = %plan.endpoint_id,
                                        key_id = %plan.key_id,
                                        model_name,
                                        candidate_index = candidate_index.as_str(),
                                        buffered_bytes = prefetched_inspection_body.len(),
                                        "gateway scheduled another account before client output"
                                    );
                                    return Ok(None);
                                }
                                let response = submit_local_core_error_or_sync_finalize(
                                    state, trace_id, decision, payload,
                                )
                                .await?;
                                return Ok(Some(attach_control_metadata_headers(
                                    response,
                                    Some(request_id),
                                    candidate_id,
                                )?));
                            }
                            let effective_status_code =
                                resolve_local_sync_error_status_code(status_code, &body_json);
                            let payload = build_stream_sync_payload(
                                trace_id,
                                report_kind.clone(),
                                report_context,
                                effective_status_code,
                                headers,
                                Some(body_json),
                                None,
                                prefetched_telemetry,
                            );
                            let failure_analysis = record_stream_sync_failure(
                                state,
                                &plan,
                                payload.report_context.as_ref(),
                                &payload,
                                Some(candidate_started_unix_secs),
                            )
                            .await;
                            if matches!(
                                failure_analysis.decision,
                                LocalFailoverDecision::RetryNextCandidate
                            ) {
                                warn!(
                                    event_name = "local_stream_candidate_retry_scheduled",
                                    log_type = "event",
                                    trace_id = %trace_id,
                                    request_id = %request_id_for_log,
                                    upstream_status_code = status_code,
                                    effective_status_code,
                                    provider_name,
                                    endpoint_id = %plan.endpoint_id,
                                    key_id = %plan.key_id,
                                    model_name,
                                    candidate_index = candidate_index.as_str(),
                                    "gateway retrying next candidate after embedded upstream error"
                                );
                                return Ok(None);
                            }
                            let response = submit_local_core_error_or_sync_finalize(
                                state, trace_id, decision, payload,
                            )
                            .await?;
                            return Ok(Some(attach_control_metadata_headers(
                                response,
                                Some(request_id),
                                candidate_id,
                            )?));
                        }
                        StreamPrefetchInspection::NeedMore => {}
                        StreamPrefetchInspection::NonError => {}
                    }

                    if !response_headers_indicate_sse(&upstream_headers)
                        && (200..300).contains(&status_code)
                    {
                        if let Some(body_json) =
                            parse_prefetched_sync_json_body(&prefetched_inspection_body)
                        {
                            match maybe_bridge_standard_sync_json_to_stream(
                                &body_json,
                                plan.provider_api_format.as_str(),
                                plan.client_api_format.as_str(),
                                report_context.as_ref(),
                            ) {
                                Ok(Some(outcome)) => {
                                    headers.remove("content-encoding");
                                    headers.remove("content-length");
                                    headers.insert(
                                        "content-type".to_string(),
                                        "text/event-stream".to_string(),
                                    );
                                    stream_terminal_summary = outcome.terminal_summary;
                                    prefetched_body.extend_from_slice(&outcome.sse_body);
                                    prefetched_chunks.push(Bytes::from(outcome.sse_body));
                                    sync_json_stream_bridge_active = true;
                                    break;
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    let failure = build_stream_failure_report(
                                        "execution_runtime_sync_json_stream_bridge_error",
                                        format!(
                                            "failed to bridge execution runtime sync json to stream: {err:?}"
                                        ),
                                        502,
                                    );
                                    return handle_prefetch_stream_failure(
                                        state,
                                        trace_id,
                                        decision,
                                        &plan,
                                        report_context,
                                        request_id,
                                        candidate_id,
                                        report_kind,
                                        headers,
                                        prefetched_telemetry,
                                        &provider_prefetched_body,
                                        failure,
                                    )
                                    .await;
                                }
                            }
                        }
                    }

                    let normalized_chunk = if let Some(normalizer) =
                        private_stream_normalizer.as_mut()
                    {
                        match normalizer.push_chunk(&chunk) {
                            Ok(normalized_chunk) => normalized_chunk,
                            Err(err) => {
                                let failure = build_stream_failure_report(
                                    "execution_runtime_stream_rewrite_error",
                                    format!(
                                        "failed to normalize execution runtime stream chunk: {err:?}"
                                    ),
                                    502,
                                );
                                return handle_prefetch_stream_failure(
                                    state,
                                    trace_id,
                                    decision,
                                    &plan,
                                    report_context,
                                    request_id,
                                    candidate_id,
                                    report_kind,
                                    headers,
                                    prefetched_telemetry,
                                    &provider_prefetched_body,
                                    failure,
                                )
                                .await;
                            }
                        }
                    } else {
                        chunk
                    };
                    let rewritten_chunk = if let Some(rewriter) = local_stream_rewriter.as_mut() {
                        match rewriter.push_chunk(&normalized_chunk) {
                            Ok(rewritten_chunk) => rewritten_chunk,
                            Err(err) => {
                                let failure = build_stream_failure_report(
                                    "execution_runtime_stream_rewrite_error",
                                    format!(
                                        "failed to rewrite execution runtime stream chunk: {err:?}"
                                    ),
                                    502,
                                );
                                return handle_prefetch_stream_failure(
                                    state,
                                    trace_id,
                                    decision,
                                    &plan,
                                    report_context,
                                    request_id,
                                    candidate_id,
                                    report_kind,
                                    headers,
                                    prefetched_telemetry,
                                    &provider_prefetched_body,
                                    failure,
                                )
                                .await;
                            }
                        }
                    } else {
                        normalized_chunk
                    };
                    if !rewritten_chunk.is_empty() {
                        prefetched_body.extend_from_slice(&rewritten_chunk);
                        prefetched_chunks.push(Bytes::from(rewritten_chunk));
                    }

                    if matches!(inspection, StreamPrefetchInspection::NonError) {
                        prefetch_committed_output = true;
                        if prefetch_openai_responses_sse {
                            debug!(
                                event_name = "stream_failover_prefetch_committed",
                                log_type = "event",
                                trace_id = %trace_id,
                                request_id = %request_id_for_log,
                                provider_name,
                                endpoint_id = %plan.endpoint_id,
                                key_id = %plan.key_id,
                                model_name,
                                reason = "protocol_commit_event",
                                buffered_bytes = prefetched_inspection_body.len(),
                                "gateway committed the OpenAI Responses stream at a protocol event boundary"
                            );
                        }
                        break;
                    }
                }
                StreamFramePayload::Telemetry {
                    telemetry: frame_telemetry,
                } => {
                    prefetched_telemetry = Some(frame_telemetry);
                }
                StreamFramePayload::Eof { summary } => {
                    if summary.is_some() {
                        stream_terminal_summary = summary;
                    }
                    reached_eof = true;
                    break;
                }
                StreamFramePayload::Error { error } => {
                    warn!(
                        event_name = "stream_execution_prefetch_error_frame",
                        log_type = "ops",
                        trace_id = %trace_id,
                        request_id,
                        candidate_id = ?candidate_id,
                        error = %error.message,
                        "execution runtime stream emitted error frame during prefetch"
                    );
                    return handle_prefetch_stream_failure(
                        state,
                        trace_id,
                        decision,
                        &plan,
                        report_context,
                        request_id,
                        candidate_id,
                        report_kind,
                        headers,
                        prefetched_telemetry,
                        &provider_prefetched_body,
                        build_stream_failure_from_execution_error(&error),
                    )
                    .await;
                }
                StreamFramePayload::Headers { .. } => {}
            }
        }
        if prefetch_openai_responses_sse
            && !prefetch_committed_output
            && !reached_eof
            && prefetched_inspection_body.len() >= stream_failover_policy.max_buffer_bytes
        {
            debug!(
                event_name = "stream_failover_prefetch_committed",
                log_type = "event",
                trace_id = %trace_id,
                request_id = %request_id_for_log,
                provider_name,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                model_name,
                reason = "max_buffer_reached",
                max_buffer_bytes = stream_failover_policy.max_buffer_bytes,
                buffered_bytes = prefetched_inspection_body.len(),
                "gateway committed the OpenAI Responses stream after the buffer limit"
            );
        }
        if prefetch_openai_responses_sse && reached_eof && !prefetch_committed_output {
            debug!(
                event_name = "stream_failover_prefetch_committed",
                log_type = "event",
                trace_id = %trace_id,
                request_id = %request_id_for_log,
                provider_name,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                model_name,
                reason = "upstream_eof",
                buffered_bytes = prefetched_inspection_body.len(),
                "gateway committed the OpenAI Responses stream after upstream EOF"
            );
        }
    }
    drop(private_stream_normalizer);
    drop(local_stream_rewriter);

    let mut initial_stream_telemetry = prefetched_telemetry;
    if !prefetched_body.is_empty()
        && initial_stream_telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.ttfb_ms)
            .is_none()
    {
        let first_byte_elapsed_ms = stream_started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let provider_prefetched_bytes =
            u64::try_from(provider_prefetched_body.len()).unwrap_or(u64::MAX);
        if let Some(telemetry) = initial_stream_telemetry.as_mut() {
            telemetry.ttfb_ms = Some(first_byte_elapsed_ms);
            telemetry.elapsed_ms = telemetry.elapsed_ms.or(Some(first_byte_elapsed_ms));
            telemetry.upstream_bytes = telemetry.upstream_bytes.or(Some(provider_prefetched_bytes));
        } else {
            initial_stream_telemetry = Some(ExecutionTelemetry {
                ttfb_ms: Some(first_byte_elapsed_ms),
                elapsed_ms: Some(first_byte_elapsed_ms),
                upstream_bytes: Some(provider_prefetched_bytes),
            });
        }
    }

    state.usage_runtime.record_stream_started(
        state.data.as_ref(),
        &lifecycle_seed,
        status_code,
        initial_stream_telemetry.as_ref(),
    );
    if let Some(snapshot) = request_candidate_status_snapshot {
        let latency_ms = initial_stream_telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.elapsed_ms);
        submit_local_request_candidate_status_snapshot(
            state,
            &snapshot,
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Streaming,
                status_code: Some(status_code),
                error_type: None,
                error_message: None,
                latency_ms,
                started_at_unix_ms: Some(candidate_started_unix_secs),
                finished_at_unix_ms: None,
            },
        )
        .await;
    }

    apply_endpoint_response_header_rules(state, &plan, &mut headers, None).await?;

    let request_id = request_id.to_string();
    let candidate_id = candidate_id.map(ToOwned::to_owned);
    let (tx, mut rx) = mpsc::channel::<Result<Bytes, IoError>>(16);
    let state_for_report = state.clone();
    let trace_id_owned = trace_id.to_string();
    let headers_for_report = headers.clone();
    let report_kind_owned = report_kind;
    let report_context_owned = report_context;
    let normalized_stream_report_context_owned = normalized_stream_report_context;
    let lifecycle_seed_for_report = lifecycle_seed;
    let provider_prefetched_body_for_report = provider_prefetched_body;
    let prefetched_body_for_report = prefetched_body;
    let prefetched_chunks_for_body = prefetched_chunks;
    let sync_json_stream_bridge_active_for_report = sync_json_stream_bridge_active;
    let initial_telemetry = initial_stream_telemetry;
    let initial_reached_eof = reached_eof;
    let direct_stream_finalize_kind_owned = direct_stream_finalize_kind;
    let candidate_started_unix_secs_for_report = candidate_started_unix_secs;
    let request_id_for_report = request_id.clone();
    let request_id_for_report_log = short_request_id(&request_id);
    let candidate_id_for_report = candidate_id.clone();
    let candidate_index_for_report = candidate_index.clone();
    let is_openai_image_stream_for_report = plan_kind == OPENAI_IMAGE_STREAM_PLAN_KIND;
    let openai_image_stream_total_timeout_ms =
        resolve_openai_image_stream_total_timeout_ms(plan_kind, &plan);
    let plan_for_report = plan;
    let emit_passthrough_sse_terminal_error = (skip_direct_finalize_prefetch
        || prefetch_openai_responses_sse)
        && response_headers_indicate_sse(&upstream_headers)
        && !is_openai_image_stream_for_report;
    let body_capture_policy = match UsageRuntimeAccess::body_capture_policy(state.data.as_ref())
        .await
    {
        Ok(policy) => policy,
        Err(err) => {
            warn!(
                event_name = "stream_execution_body_capture_policy_read_failed",
                log_type = "ops",
                trace_id = %trace_id,
                request_id = %request_id_for_report_log,
                candidate_id = ?candidate_id_for_report.as_deref(),
                error = %err,
                fallback_request_body_bytes = DEFAULT_USAGE_RESPONSE_BODY_CAPTURE_LIMIT_BYTES,
                "gateway failed to read body capture policy; falling back to default stream capture limits"
            );
            UsageBodyCapturePolicy::default()
        }
    };
    let max_stream_body_buffer_bytes = if matches!(
        body_capture_policy.record_level,
        UsageRequestRecordLevel::Basic
    ) {
        0
    } else {
        body_capture_policy
            .max_response_body_bytes
            .unwrap_or(DEFAULT_USAGE_RESPONSE_BODY_CAPTURE_LIMIT_BYTES)
    };
    let full_stream_body_capture = matches!(
        body_capture_policy.record_level,
        UsageRequestRecordLevel::Full
    );
    let usage_object_store_for_report = full_stream_body_capture
        .then(|| state.data.usage_object_store())
        .flatten();
    let provider_stream_content_type = upstream_headers.get("content-type").cloned();
    let client_stream_content_type = headers.get("content-type").cloned();
    let plan_kind_for_report = plan_kind.to_string();
    let stream_started_at_for_report = stream_started_at;
    let provider_pool_in_flight_guard_for_report = in_flight_guard;
    tokio::spawn(async move {
        let _provider_pool_in_flight_guard = provider_pool_in_flight_guard_for_report;
        let mut report_context_owned = report_context_owned;
        let mut provider_buffered_body = Vec::new();
        let mut buffered_body = Vec::new();
        let mut provider_body_truncated = false;
        let mut client_body_truncated = false;
        let mut private_stream_normalizer = if sync_json_stream_bridge_active_for_report {
            None
        } else {
            maybe_build_provider_private_stream_normalizer(report_context_owned.as_ref())
        };
        let stream_usage_report_context = Some(merge_stream_usage_report_context(
            normalized_stream_report_context_owned,
            &plan_for_report,
        ));
        let mut local_stream_rewriter = if sync_json_stream_bridge_active_for_report {
            None
        } else {
            maybe_build_stream_response_rewriter(stream_usage_report_context.as_ref())
        };
        let mut stream_usage_observer = stream_usage_report_context
            .as_ref()
            .filter(|_| !sync_json_stream_bridge_active_for_report)
            .map(|_| StreamingStandardTerminalObserver::default());
        let mut stream_usage_observer_buffered = Vec::new();
        let mut provider_object_capture = StreamObjectCapture::new(
            usage_object_store_for_report.clone(),
            &request_id_for_report,
            UsageBodyField::ResponseBody,
            provider_stream_content_type.as_deref(),
            full_stream_body_capture,
        );
        let mut client_object_capture = StreamObjectCapture::new(
            usage_object_store_for_report,
            &request_id_for_report,
            UsageBodyField::ClientResponseBody,
            client_stream_content_type.as_deref(),
            full_stream_body_capture,
        );
        provider_object_capture
            .write(&provider_prefetched_body_for_report)
            .await;
        client_object_capture
            .write(&prefetched_body_for_report)
            .await;
        append_stream_capture_bytes(
            &mut provider_buffered_body,
            &provider_prefetched_body_for_report,
            max_stream_body_buffer_bytes,
            &mut provider_body_truncated,
        );
        append_stream_capture_bytes(
            &mut buffered_body,
            &prefetched_body_for_report,
            max_stream_body_buffer_bytes,
            &mut client_body_truncated,
        );
        let mut client_visible_stream_completed =
            stream_chunk_contains_sse_done(&prefetched_body_for_report);
        let mut image_terminal_tracker = OpenAiResponsesImageTerminalTracker::default();
        image_terminal_tracker.push(&prefetched_body_for_report);
        let mut usage_stream_telemetry: Option<ExecutionTelemetry> = initial_telemetry.clone();
        let mut telemetry: Option<ExecutionTelemetry> = initial_telemetry;
        let reached_eof = initial_reached_eof;
        let mut downstream_dropped = false;
        let mut terminal_failure: Option<StreamFailureReport> = None;
        let initial_elapsed_ms = stream_started_at_for_report
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let last_upstream_frame_elapsed_ms = Arc::new(AtomicU64::new(initial_elapsed_ms));
        let last_client_chunk_elapsed_ms =
            Arc::new(AtomicU64::new(if prefetched_body_for_report.is_empty() {
                0
            } else {
                initial_elapsed_ms
            }));
        let provider_stream_bytes = Arc::new(AtomicU64::new(
            u64::try_from(provider_prefetched_body_for_report.len()).unwrap_or(u64::MAX),
        ));
        let client_stream_bytes = Arc::new(AtomicU64::new(
            u64::try_from(prefetched_body_for_report.len()).unwrap_or(u64::MAX),
        ));
        let idle_monitor_done = Arc::new(AtomicBool::new(false));
        let idle_monitor_handle = {
            let done = Arc::clone(&idle_monitor_done);
            let last_upstream = Arc::clone(&last_upstream_frame_elapsed_ms);
            let last_client = Arc::clone(&last_client_chunk_elapsed_ms);
            let provider_bytes = Arc::clone(&provider_stream_bytes);
            let client_bytes = Arc::clone(&client_stream_bytes);
            let trace_id_for_idle = trace_id_owned.clone();
            let request_id_for_idle = request_id_for_report_log.clone();
            let candidate_id_for_idle = candidate_id_for_report.clone();
            let candidate_index_for_idle = candidate_index_for_report.clone();
            let plan_kind_for_idle = plan_kind_for_report.clone();
            let provider_name_for_idle = plan_for_report
                .provider_name
                .clone()
                .unwrap_or_else(|| "-".to_string());
            let endpoint_id_for_idle = plan_for_report.endpoint_id.clone();
            let key_id_for_idle = plan_for_report.key_id.clone();
            let model_name_for_idle = plan_for_report
                .model_name
                .clone()
                .unwrap_or_else(|| "-".to_string());
            let has_local_stream_rewriter_for_idle = local_stream_rewriter.is_some();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(STREAM_IDLE_LOG_INTERVAL);
                interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                interval.tick().await;
                loop {
                    interval.tick().await;
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                    let elapsed_ms = stream_started_at_for_report
                        .elapsed()
                        .as_millis()
                        .min(u128::from(u64::MAX)) as u64;
                    let last_upstream_frame_elapsed_ms = last_upstream.load(Ordering::Relaxed);
                    let last_client_chunk_elapsed_ms = last_client.load(Ordering::Relaxed);
                    let upstream_idle_ms =
                        elapsed_ms.saturating_sub(last_upstream_frame_elapsed_ms);
                    let client_idle_ms = if last_client_chunk_elapsed_ms == 0 {
                        elapsed_ms
                    } else {
                        elapsed_ms.saturating_sub(last_client_chunk_elapsed_ms)
                    };
                    if upstream_idle_ms >= STREAM_IDLE_LOG_INTERVAL_MS {
                        warn!(
                            event_name = "stream_execution_upstream_idle",
                            log_type = "ops",
                            trace_id = %trace_id_for_idle,
                            request_id = %request_id_for_idle,
                            candidate_id = ?candidate_id_for_idle.as_deref(),
                            candidate_index = candidate_index_for_idle.as_str(),
                            plan_kind = plan_kind_for_idle.as_str(),
                            provider_name = provider_name_for_idle.as_str(),
                            endpoint_id = %endpoint_id_for_idle,
                            key_id = %key_id_for_idle,
                            model_name = model_name_for_idle.as_str(),
                            elapsed_ms,
                            provider_bytes = provider_bytes.load(Ordering::Relaxed),
                            client_bytes = client_bytes.load(Ordering::Relaxed),
                            last_upstream_frame_elapsed_ms,
                            last_client_chunk_elapsed_ms,
                            "gateway stream has not received an upstream frame within the idle window"
                        );
                    } else if client_idle_ms >= STREAM_IDLE_LOG_INTERVAL_MS
                        && last_upstream_frame_elapsed_ms >= last_client_chunk_elapsed_ms
                    {
                        warn!(
                            event_name = "stream_execution_client_visible_idle",
                            log_type = "ops",
                            trace_id = %trace_id_for_idle,
                            request_id = %request_id_for_idle,
                            candidate_id = ?candidate_id_for_idle.as_deref(),
                            candidate_index = candidate_index_for_idle.as_str(),
                            plan_kind = plan_kind_for_idle.as_str(),
                            provider_name = provider_name_for_idle.as_str(),
                            endpoint_id = %endpoint_id_for_idle,
                            key_id = %key_id_for_idle,
                            model_name = model_name_for_idle.as_str(),
                            elapsed_ms,
                            provider_bytes = provider_bytes.load(Ordering::Relaxed),
                            client_bytes = client_bytes.load(Ordering::Relaxed),
                            last_upstream_frame_elapsed_ms,
                            last_client_chunk_elapsed_ms,
                            local_stream_rewriter = has_local_stream_rewriter_for_idle,
                            "gateway stream received upstream frames but has no recent client-visible chunk"
                        );
                    }
                }
            })
        };
        if !provider_prefetched_body_for_report.is_empty() {
            let normalized_prefetched_chunk = if let Some(normalizer) =
                private_stream_normalizer.as_mut()
            {
                match normalizer.push_chunk(&provider_prefetched_body_for_report) {
                    Ok(normalized_chunk) => Some(normalized_chunk),
                    Err(err) => {
                        warn!(
                            event_name = "stream_execution_prefetch_normalize_restore_failed",
                            log_type = "ops",
                            trace_id = %trace_id_owned,
                            request_id = %request_id_for_report_log,
                            candidate_id = ?candidate_id_for_report.as_deref(),
                            error = ?err,
                            "gateway failed to restore private stream normalization state after prefetch"
                        );
                        terminal_failure = Some(build_stream_failure_report(
                            "execution_runtime_stream_rewrite_error",
                            format!(
                                "failed to restore private stream normalization state after prefetch: {err:?}"
                            ),
                            502,
                        ));
                        None
                    }
                }
            } else {
                None
            };
            let replay_chunk = normalized_prefetched_chunk
                .as_deref()
                .unwrap_or(provider_prefetched_body_for_report.as_slice());
            if let (Some(observer), Some(report_context)) = (
                stream_usage_observer.as_mut(),
                stream_usage_report_context.as_ref(),
            ) {
                observe_stream_usage_bytes(
                    observer,
                    report_context,
                    &mut stream_usage_observer_buffered,
                    replay_chunk,
                );
            }
            if terminal_failure.is_none() {
                if let Some(rewriter) = local_stream_rewriter.as_mut() {
                    if let Err(err) = rewriter.push_chunk(replay_chunk) {
                        warn!(
                            event_name = "stream_execution_prefetch_rewrite_restore_failed",
                            log_type = "ops",
                            trace_id = %trace_id_owned,
                            request_id = %request_id_for_report_log,
                            candidate_id = ?candidate_id_for_report.as_deref(),
                            error = ?err,
                            "gateway failed to restore local stream rewrite state after prefetch"
                        );
                        terminal_failure = Some(build_stream_failure_report(
                            "execution_runtime_stream_rewrite_error",
                            format!(
                                "failed to restore local stream rewrite state after prefetch: {err:?}"
                            ),
                            502,
                        ));
                    }
                }
            }
        }

        if terminal_failure.is_none() && !reached_eof {
            let mut image_stream_total_timeout = openai_image_stream_total_timeout_ms
                .map(|timeout_ms| Box::pin(tokio::time::sleep(Duration::from_millis(timeout_ms))));
            loop {
                let next_frame_result = if let Some(timeout_sleep) =
                    image_stream_total_timeout.as_mut()
                {
                    tokio::select! {
                        result = next_stream_frame_until_downstream_closed(
                            &mut buffered_frames,
                            &mut lines,
                            &tx,
                        ) => result,
                        () = tx.closed() => {
                            downstream_dropped = true;
                            break;
                        }
                        _ = timeout_sleep.as_mut() => {
                            let timeout_ms = openai_image_stream_total_timeout_ms
                                .unwrap_or(OPENAI_IMAGE_STREAM_DEFAULT_TOTAL_TIMEOUT_MS);
                            let elapsed_ms = stream_started_at_for_report
                                .elapsed()
                                .as_millis()
                                .min(u128::from(u64::MAX)) as u64;
                            warn!(
                                event_name = "openai_image_stream_total_timeout",
                                log_type = "ops",
                                trace_id = %trace_id_owned,
                                request_id = %request_id_for_report_log,
                                candidate_id = ?candidate_id_for_report.as_deref(),
                                candidate_index = candidate_index_for_report.as_str(),
                                plan_kind = plan_kind_for_report.as_str(),
                                provider_name = plan_for_report.provider_name.as_deref().unwrap_or("-"),
                                endpoint_id = %plan_for_report.endpoint_id,
                                key_id = %plan_for_report.key_id,
                                model_name = plan_for_report.model_name.as_deref().unwrap_or("-"),
                                elapsed_ms,
                                timeout_ms,
                                provider_bytes = provider_stream_bytes.load(Ordering::Relaxed),
                                client_bytes = client_stream_bytes.load(Ordering::Relaxed),
                                last_upstream_frame_elapsed_ms = last_upstream_frame_elapsed_ms.load(Ordering::Relaxed),
                                last_client_chunk_elapsed_ms = last_client_chunk_elapsed_ms.load(Ordering::Relaxed),
                                "gateway OpenAI image stream exceeded total timeout"
                            );
                            telemetry = Some(ExecutionTelemetry {
                                ttfb_ms: telemetry
                                    .as_ref()
                                    .and_then(|telemetry| telemetry.ttfb_ms)
                                    .or_else(|| {
                                        usage_stream_telemetry
                                            .as_ref()
                                            .and_then(|telemetry| telemetry.ttfb_ms)
                                    }),
                                elapsed_ms: Some(elapsed_ms),
                                upstream_bytes: Some(provider_stream_bytes.load(Ordering::Relaxed)),
                            });
                            terminal_failure = Some(build_stream_failure_report(
                                "image_stream_total_timeout",
                                format!(
                                    "OpenAI image stream exceeded total timeout of {timeout_ms}ms"
                                ),
                                504,
                            ));
                            break;
                        }
                    }
                } else {
                    next_stream_frame_until_downstream_closed(&mut buffered_frames, &mut lines, &tx)
                        .await
                };
                let next_frame = match next_frame_result {
                    Ok(frame) => frame,
                    Err(err) => {
                        warn!(
                            event_name = "stream_execution_frame_decode_failed",
                            log_type = "ops",
                            trace_id = %trace_id_owned,
                            request_id = %request_id_for_report_log,
                            candidate_id = ?candidate_id_for_report.as_deref(),
                            error = ?err,
                            "gateway failed to decode execution runtime stream frame"
                        );
                        terminal_failure = Some(build_stream_failure_report(
                            "execution_runtime_stream_frame_decode_error",
                            format!("failed to decode execution runtime stream frame: {err:?}"),
                            502,
                        ));
                        break;
                    }
                };
                let Some(frame) = next_frame else {
                    if tx.is_closed() {
                        downstream_dropped = true;
                    }
                    break;
                };
                let frame_elapsed_ms = stream_started_at_for_report
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64;
                last_upstream_frame_elapsed_ms.store(frame_elapsed_ms, Ordering::Relaxed);
                match frame.payload {
                    StreamFramePayload::Data { chunk_b64, text } => {
                        if sync_json_stream_bridge_active_for_report {
                            continue;
                        }
                        let chunk =
                            match decode_stream_data_chunk(chunk_b64.as_deref(), text.as_deref()) {
                                Ok(chunk) => chunk,
                                Err(err) => {
                                    warn!(
                                        event_name = "stream_execution_chunk_decode_failed",
                                        log_type = "ops",
                                        trace_id = %trace_id_owned,
                                        request_id = %request_id_for_report_log,
                                        candidate_id = ?candidate_id_for_report.as_deref(),
                                        error = ?err,
                                        "gateway failed to decode execution runtime chunk"
                                    );
                                    terminal_failure = Some(build_stream_failure_report(
                                        "execution_runtime_stream_chunk_decode_error",
                                        format!(
                                        "failed to decode execution runtime stream chunk: {err:?}"
                                    ),
                                        502,
                                    ));
                                    break;
                                }
                            };

                        if chunk.is_empty() {
                            continue;
                        }

                        provider_stream_bytes.fetch_add(
                            u64::try_from(chunk.len()).unwrap_or(u64::MAX),
                            Ordering::Relaxed,
                        );
                        provider_object_capture.write(&chunk).await;
                        append_stream_capture_bytes(
                            &mut provider_buffered_body,
                            &chunk,
                            max_stream_body_buffer_bytes,
                            &mut provider_body_truncated,
                        );
                        let normalized_chunk = if let Some(normalizer) =
                            private_stream_normalizer.as_mut()
                        {
                            match normalizer.push_chunk(&chunk) {
                                Ok(normalized_chunk) => normalized_chunk,
                                Err(err) => {
                                    warn!(
                                        event_name = "stream_execution_chunk_normalize_failed",
                                        log_type = "ops",
                                        trace_id = %trace_id_owned,
                                        request_id = %request_id_for_report_log,
                                        candidate_id = ?candidate_id_for_report.as_deref(),
                                        error = ?err,
                                        "gateway failed to normalize execution runtime stream chunk"
                                    );
                                    terminal_failure = Some(build_stream_failure_report(
                                            "execution_runtime_stream_rewrite_error",
                                            format!("failed to normalize execution runtime stream chunk: {err:?}"),
                                            502,
                                        ));
                                    break;
                                }
                            }
                        } else {
                            chunk
                        };
                        if let (Some(observer), Some(report_context)) = (
                            stream_usage_observer.as_mut(),
                            stream_usage_report_context.as_ref(),
                        ) {
                            observe_stream_usage_bytes(
                                observer,
                                report_context,
                                &mut stream_usage_observer_buffered,
                                &normalized_chunk,
                            );
                        }
                        let rewritten_chunk = if let Some(rewriter) = local_stream_rewriter.as_mut()
                        {
                            match rewriter.push_chunk(&normalized_chunk) {
                                Ok(rewritten_chunk) => rewritten_chunk,
                                Err(err) => {
                                    warn!(
                                        event_name = "stream_execution_chunk_rewrite_failed",
                                        log_type = "ops",
                                        trace_id = %trace_id_owned,
                                        request_id = %request_id_for_report_log,
                                        candidate_id = ?candidate_id_for_report.as_deref(),
                                        error = ?err,
                                        "gateway failed to rewrite execution runtime stream chunk"
                                    );
                                    terminal_failure = Some(build_stream_failure_report(
                                        "execution_runtime_stream_rewrite_error",
                                        format!("failed to rewrite execution runtime stream chunk: {err:?}"),
                                        502,
                                    ));
                                    break;
                                }
                            }
                        } else {
                            normalized_chunk
                        };

                        if rewritten_chunk.is_empty() {
                            continue;
                        }

                        if usage_stream_telemetry
                            .as_ref()
                            .and_then(|telemetry| telemetry.ttfb_ms)
                            .is_none()
                        {
                            let first_data_elapsed_ms = stream_started_at_for_report
                                .elapsed()
                                .as_millis()
                                .min(u128::from(u64::MAX))
                                as u64;
                            let first_data_telemetry = ExecutionTelemetry {
                                ttfb_ms: Some(first_data_elapsed_ms),
                                elapsed_ms: Some(first_data_elapsed_ms),
                                upstream_bytes: telemetry
                                    .as_ref()
                                    .and_then(|telemetry| telemetry.upstream_bytes),
                            };
                            state_for_report.usage_runtime.record_stream_started(
                                state_for_report.data.as_ref(),
                                &lifecycle_seed_for_report,
                                status_code,
                                Some(&first_data_telemetry),
                            );
                            usage_stream_telemetry = Some(first_data_telemetry);
                        }

                        append_stream_capture_bytes(
                            &mut buffered_body,
                            &rewritten_chunk,
                            max_stream_body_buffer_bytes,
                            &mut client_body_truncated,
                        );
                        client_object_capture.write(&rewritten_chunk).await;
                        image_terminal_tracker.push(&rewritten_chunk);
                        let rewritten_chunk_len =
                            u64::try_from(rewritten_chunk.len()).unwrap_or(u64::MAX);
                        let chunk_completed_stream =
                            stream_chunk_contains_sse_done(&rewritten_chunk);
                        if tx.send(Ok(Bytes::from(rewritten_chunk))).await.is_err() {
                            warn!(
                                event_name = "stream_execution_downstream_disconnected",
                                log_type = "ops",
                                trace_id = %trace_id_owned,
                                request_id = %request_id_for_report_log,
                                candidate_id = ?candidate_id_for_report.as_deref(),
                                "gateway stream downstream dropped; stopping execution runtime stream forwarding"
                            );
                            downstream_dropped = true;
                            break;
                        } else {
                            client_visible_stream_completed |= chunk_completed_stream;
                            client_stream_bytes.fetch_add(rewritten_chunk_len, Ordering::Relaxed);
                            last_client_chunk_elapsed_ms.store(
                                stream_started_at_for_report
                                    .elapsed()
                                    .as_millis()
                                    .min(u128::from(u64::MAX))
                                    as u64,
                                Ordering::Relaxed,
                            );
                        }
                    }
                    StreamFramePayload::Telemetry {
                        telemetry: frame_telemetry,
                    } => {
                        let should_refresh_stream_usage = should_refresh_stream_usage_telemetry(
                            usage_stream_telemetry.as_ref(),
                            &frame_telemetry,
                        );
                        if should_refresh_stream_usage {
                            state_for_report.usage_runtime.record_stream_started(
                                state_for_report.data.as_ref(),
                                &lifecycle_seed_for_report,
                                status_code,
                                Some(&frame_telemetry),
                            );
                            usage_stream_telemetry = Some(frame_telemetry.clone());
                        }
                        telemetry = Some(frame_telemetry);
                    }
                    StreamFramePayload::Eof { summary } => {
                        if summary.is_some() {
                            stream_terminal_summary = summary;
                        }
                        break;
                    }
                    StreamFramePayload::Error { error } => {
                        warn!(
                            event_name = "stream_execution_error_frame",
                            log_type = "ops",
                            trace_id = %trace_id_owned,
                            request_id = %request_id_for_report_log,
                            candidate_id = ?candidate_id_for_report.as_deref(),
                            error = %error.message,
                            "execution runtime stream emitted error frame"
                        );
                        terminal_failure = Some(build_stream_failure_from_execution_error(&error));
                        break;
                    }
                    StreamFramePayload::Headers { .. } => {}
                }
            }
        }

        if downstream_dropped {
            drop(lines);
            debug!(
                event_name = "execution_runtime_stream_flush_skipped",
                log_type = "debug",
                debug_context = "redacted",
                stream_status = "downstream_disconnected",
                trace_id = %trace_id_owned,
                "gateway skipped local stream flush after downstream disconnect"
            );
        } else {
            if let Some(normalizer) = private_stream_normalizer.as_mut() {
                match normalizer.finish() {
                    Ok(normalized_chunk) if !normalized_chunk.is_empty() => {
                        if let (Some(observer), Some(report_context)) = (
                            stream_usage_observer.as_mut(),
                            stream_usage_report_context.as_ref(),
                        ) {
                            observe_stream_usage_bytes(
                                observer,
                                report_context,
                                &mut stream_usage_observer_buffered,
                                &normalized_chunk,
                            );
                        }
                        let rewritten_chunk = if let Some(rewriter) = local_stream_rewriter.as_mut()
                        {
                            match rewriter.push_chunk(&normalized_chunk) {
                                Ok(rewritten_chunk) => rewritten_chunk,
                                Err(err) => {
                                    warn!(
                                        event_name = "stream_execution_normalized_flush_rewrite_failed",
                                        log_type = "ops",
                                        trace_id = %trace_id_owned,
                                        request_id = %request_id_for_report_log,
                                        candidate_id = ?candidate_id_for_report.as_deref(),
                                        error = ?err,
                                        "gateway failed to rewrite normalized private stream chunk during flush"
                                    );
                                    terminal_failure.get_or_insert_with(|| {
                                        build_stream_failure_report(
                                            "execution_runtime_stream_rewrite_flush_error",
                                            format!("failed to rewrite normalized private stream chunk during flush: {err:?}"),
                                            502,
                                        )
                                    });
                                    Vec::new()
                                }
                            }
                        } else {
                            normalized_chunk
                        };
                        if !rewritten_chunk.is_empty() {
                            append_stream_capture_bytes(
                                &mut buffered_body,
                                &rewritten_chunk,
                                max_stream_body_buffer_bytes,
                                &mut client_body_truncated,
                            );
                            client_object_capture.write(&rewritten_chunk).await;
                            image_terminal_tracker.push(&rewritten_chunk);
                            let rewritten_chunk_len =
                                u64::try_from(rewritten_chunk.len()).unwrap_or(u64::MAX);
                            let chunk_completed_stream =
                                stream_chunk_contains_sse_done(&rewritten_chunk);
                            if tx.send(Ok(Bytes::from(rewritten_chunk))).await.is_err() {
                                warn!(
                                    event_name = "stream_execution_downstream_flush_disconnected",
                                    log_type = "ops",
                                    trace_id = %trace_id_owned,
                                    request_id = %request_id_for_report_log,
                                candidate_id = ?candidate_id_for_report.as_deref(),
                                "gateway stream downstream dropped while flushing private stream normalization"
                                );
                                downstream_dropped = true;
                            } else {
                                client_visible_stream_completed |= chunk_completed_stream;
                                client_stream_bytes
                                    .fetch_add(rewritten_chunk_len, Ordering::Relaxed);
                                last_client_chunk_elapsed_ms.store(
                                    stream_started_at_for_report
                                        .elapsed()
                                        .as_millis()
                                        .min(u128::from(u64::MAX))
                                        as u64,
                                    Ordering::Relaxed,
                                );
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!(
                            event_name = "stream_execution_normalization_flush_failed",
                            log_type = "ops",
                            trace_id = %trace_id_owned,
                            request_id = %request_id_for_report_log,
                            candidate_id = ?candidate_id_for_report.as_deref(),
                            error = ?err,
                            "gateway failed to flush private stream normalization"
                        );
                        terminal_failure.get_or_insert_with(|| {
                            build_stream_failure_report(
                                "execution_runtime_stream_rewrite_flush_error",
                                format!("failed to flush private stream normalization: {err:?}"),
                                502,
                            )
                        });
                    }
                }
            }
            if !downstream_dropped {
                if let Some(rewriter) = local_stream_rewriter.as_mut() {
                    match rewriter.finish() {
                        Ok(flushed_chunk) if !flushed_chunk.is_empty() => {
                            append_stream_capture_bytes(
                                &mut buffered_body,
                                &flushed_chunk,
                                max_stream_body_buffer_bytes,
                                &mut client_body_truncated,
                            );
                            client_object_capture.write(&flushed_chunk).await;
                            image_terminal_tracker.push(&flushed_chunk);
                            let flushed_chunk_len =
                                u64::try_from(flushed_chunk.len()).unwrap_or(u64::MAX);
                            let chunk_completed_stream =
                                stream_chunk_contains_sse_done(&flushed_chunk);
                            if tx.send(Ok(Bytes::from(flushed_chunk))).await.is_err() {
                                warn!(
                                    event_name = "stream_execution_downstream_rewrite_flush_disconnected",
                                    log_type = "ops",
                                    trace_id = %trace_id_owned,
                                    request_id = %request_id_for_report_log,
                                candidate_id = ?candidate_id_for_report.as_deref(),
                                "gateway stream downstream dropped while flushing local stream rewrite"
                                );
                                downstream_dropped = true;
                            } else {
                                client_visible_stream_completed |= chunk_completed_stream;
                                client_stream_bytes.fetch_add(flushed_chunk_len, Ordering::Relaxed);
                                last_client_chunk_elapsed_ms.store(
                                    stream_started_at_for_report
                                        .elapsed()
                                        .as_millis()
                                        .min(u128::from(u64::MAX))
                                        as u64,
                                    Ordering::Relaxed,
                                );
                            }
                        }
                        Ok(_) => {}
                        Err(err) => {
                            warn!(
                                event_name = "stream_execution_rewrite_flush_failed",
                                log_type = "ops",
                                trace_id = %trace_id_owned,
                                request_id = %request_id_for_report_log,
                                candidate_id = ?candidate_id_for_report.as_deref(),
                                error = ?err,
                                "gateway failed to flush local stream rewrite"
                            );
                            terminal_failure.get_or_insert_with(|| {
                                build_stream_failure_report(
                                    "execution_runtime_stream_rewrite_flush_error",
                                    format!("failed to flush local stream rewrite: {err:?}"),
                                    502,
                                )
                            });
                        }
                    }
                }
            }
        }

        if !downstream_dropped
            && terminal_failure.is_none()
            && image_terminal_tracker.incomplete_at_eof()
        {
            terminal_failure = Some(build_stream_failure_report(
                "incomplete_image_generation_stream",
                "image generation stream ended before a final image result and response.completed",
                502,
            ));
        }

        if !downstream_dropped {
            if let Some(failure) = terminal_failure.as_ref() {
                let terminal_event = if failure.error_type == "incomplete_image_generation_stream"
                    && plan_for_report
                        .client_api_format
                        .eq_ignore_ascii_case("openai:responses")
                {
                    Some(encode_openai_responses_failed_event(failure))
                } else if is_openai_image_stream_for_report {
                    Some(encode_openai_image_failed_event(
                        report_context_owned.as_ref(),
                        failure,
                    ))
                } else if emit_passthrough_sse_terminal_error {
                    Some(encode_terminal_sse_error_event(failure))
                } else {
                    None
                };
                if let Some(terminal_event) = terminal_event {
                    match terminal_event {
                        Ok(error_event) => {
                            let error_event_len =
                                u64::try_from(error_event.len()).unwrap_or(u64::MAX);
                            append_stream_capture_bytes(
                                &mut buffered_body,
                                error_event.as_ref(),
                                max_stream_body_buffer_bytes,
                                &mut client_body_truncated,
                            );
                            client_object_capture.write(error_event.as_ref()).await;
                            if tx.send(Ok(error_event)).await.is_err() {
                                warn!(
                                event_name = "stream_execution_downstream_terminal_error_disconnected",
                                log_type = "ops",
                                trace_id = %trace_id_owned,
                                request_id = %request_id_for_report_log,
                                candidate_id = ?candidate_id_for_report.as_deref(),
                                "gateway stream downstream dropped while sending terminal SSE error event"
                                );
                                downstream_dropped = true;
                            } else {
                                client_stream_bytes.fetch_add(error_event_len, Ordering::Relaxed);
                                last_client_chunk_elapsed_ms.store(
                                    stream_started_at_for_report
                                        .elapsed()
                                        .as_millis()
                                        .min(u128::from(u64::MAX))
                                        as u64,
                                    Ordering::Relaxed,
                                );
                            }
                        }
                        Err(err) => {
                            warn!(
                            event_name = "stream_execution_terminal_error_event_encode_failed",
                            log_type = "ops",
                            trace_id = %trace_id_owned,
                            request_id = %request_id_for_report_log,
                            candidate_id = ?candidate_id_for_report.as_deref(),
                            error = ?err,
                                "gateway failed to encode terminal SSE error event"
                            );
                        }
                    }
                }
            }
        }

        drop(tx);
        idle_monitor_done.store(true, Ordering::Relaxed);
        idle_monitor_handle.abort();

        let (provider_capture_result, client_capture_result) = tokio::join!(
            provider_object_capture.finish(),
            client_object_capture.finish(),
        );
        let (provider_descriptor, provider_body_state) = provider_capture_result;
        let (client_descriptor, client_body_state) = client_capture_result;
        attach_stream_body_object_descriptors(
            &mut report_context_owned,
            provider_descriptor,
            client_descriptor,
        );

        stream_terminal_summary = merge_stream_terminal_summary(
            stream_terminal_summary,
            finalize_stream_usage_observer(
                &mut stream_usage_observer,
                stream_usage_report_context.as_ref(),
                &mut stream_usage_observer_buffered,
            ),
        );

        if downstream_dropped && client_visible_stream_completed && terminal_failure.is_none() {
            debug!(
                event_name = "execution_runtime_stream_downstream_closed_after_done",
                log_type = "debug",
                trace_id = %trace_id_owned,
                request_id = %request_id_for_report_log,
                candidate_id = ?candidate_id_for_report.as_deref(),
                "gateway treats downstream close after client-visible SSE DONE as completed"
            );
            downstream_dropped = false;
        }

        if downstream_dropped {
            debug!(
                event_name = "execution_runtime_stream_report_skipped",
                log_type = "debug",
                debug_context = "redacted",
                stream_status = "downstream_disconnected",
                status_code = 499_u16,
                trace_id = %trace_id_owned,
                "gateway skipped stream report because downstream disconnected before completion"
            );
            let terminal_telemetry = Some(build_terminal_stream_telemetry(
                stream_started_at_for_report,
                telemetry.as_ref(),
                usage_stream_telemetry.as_ref(),
                provider_stream_bytes.load(Ordering::Relaxed),
            ));
            let usage_payload = build_stream_usage_payload(
                trace_id_owned,
                report_kind_owned.unwrap_or_default(),
                report_context_owned,
                499,
                headers_for_report,
                &provider_buffered_body,
                provider_body_state,
                provider_body_truncated,
                &buffered_body,
                client_body_state,
                client_body_truncated,
                stream_terminal_summary,
                terminal_telemetry,
            );
            record_stream_terminal_usage(
                &state_for_report,
                &plan_for_report,
                usage_payload.report_context.as_ref(),
                &usage_payload,
                true,
            );
            submit_local_request_candidate_status(
                &state_for_report,
                &plan_for_report,
                usage_payload.report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Cancelled,
                    status_code: Some(499),
                    error_type: Some("downstream_disconnect".to_string()),
                    error_message: Some("client disconnected before stream completion".to_string()),
                    latency_ms: usage_payload
                        .telemetry
                        .as_ref()
                        .and_then(|value| value.elapsed_ms),
                    started_at_unix_ms: Some(candidate_started_unix_secs_for_report),
                    finished_at_unix_ms: Some(current_request_candidate_unix_ms()),
                },
            )
            .await;
            return;
        }

        if let Some(failure) = terminal_failure {
            record_manual_proxy_stream_error(&state_for_report, &plan_for_report).await;
            let terminal_telemetry = Some(build_terminal_stream_telemetry(
                stream_started_at_for_report,
                telemetry.as_ref(),
                usage_stream_telemetry.as_ref(),
                provider_stream_bytes.load(Ordering::Relaxed),
            ));
            submit_midstream_stream_failure(
                &state_for_report,
                &trace_id_owned,
                &plan_for_report,
                direct_stream_finalize_kind_owned.as_deref(),
                report_context_owned,
                headers_for_report,
                terminal_telemetry,
                &provider_buffered_body,
                candidate_started_unix_secs_for_report,
                failure,
            )
            .await;
            return;
        }

        maybe_apply_kiro_prompt_cache_usage_to_stream_summary(
            &plan_for_report,
            report_context_owned.as_ref(),
            &mut stream_terminal_summary,
        );

        let should_submit_report = report_kind_owned.is_some();
        let terminal_telemetry = Some(build_terminal_stream_telemetry(
            stream_started_at_for_report,
            telemetry.as_ref(),
            usage_stream_telemetry.as_ref(),
            provider_stream_bytes.load(Ordering::Relaxed),
        ));
        let usage_payload = build_stream_usage_payload(
            trace_id_owned.clone(),
            report_kind_owned.unwrap_or_default(),
            report_context_owned,
            status_code,
            headers_for_report,
            &provider_buffered_body,
            provider_body_state,
            provider_body_truncated,
            &buffered_body,
            client_body_state,
            client_body_truncated,
            stream_terminal_summary,
            terminal_telemetry,
        );
        apply_local_execution_effect(
            &state_for_report,
            LocalExecutionEffectContext {
                plan: &plan_for_report,
                report_context: usage_payload.report_context.as_ref(),
            },
            LocalExecutionEffect::HealthSuccess(LocalHealthSuccessEffect),
        )
        .await;
        apply_local_execution_effect(
            &state_for_report,
            LocalExecutionEffectContext {
                plan: &plan_for_report,
                report_context: usage_payload.report_context.as_ref(),
            },
            LocalExecutionEffect::AdaptiveSuccess(LocalAdaptiveSuccessEffect),
        )
        .await;
        apply_local_execution_effect(
            &state_for_report,
            LocalExecutionEffectContext {
                plan: &plan_for_report,
                report_context: usage_payload.report_context.as_ref(),
            },
            LocalExecutionEffect::PoolSuccessStream {
                payload: &usage_payload,
            },
        )
        .await;
        record_stream_terminal_usage(
            &state_for_report,
            &plan_for_report,
            usage_payload.report_context.as_ref(),
            &usage_payload,
            false,
        );
        submit_local_request_candidate_status(
            &state_for_report,
            &plan_for_report,
            usage_payload.report_context.as_ref(),
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Success,
                status_code: Some(status_code),
                error_type: None,
                error_message: None,
                latency_ms: usage_payload
                    .telemetry
                    .as_ref()
                    .and_then(|value| value.elapsed_ms),
                started_at_unix_ms: Some(candidate_started_unix_secs_for_report),
                finished_at_unix_ms: Some(current_request_candidate_unix_ms()),
            },
        )
        .await;

        if should_submit_report {
            if let Err(err) = submit_stream_report(&state_for_report, usage_payload).await {
                warn!(
                    event_name = "execution_report_submit_failed",
                    log_type = "ops",
                    trace_id = %trace_id_owned,
                    request_id = %request_id_for_report_log,
                    candidate_id = ?candidate_id_for_report.as_deref(),
                    report_scope = "stream",
                    error = ?err,
                    "gateway failed to submit stream execution report"
                );
            }
        }
    });

    headers.insert(CONTROL_REQUEST_ID_HEADER.to_string(), request_id.clone());

    if let Some(candidate_id) = candidate_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.insert(
            CONTROL_CANDIDATE_ID_HEADER.to_string(),
            candidate_id.to_string(),
        );
    }

    let emit_sse_keepalive = response_headers_indicate_sse(&headers);
    if emit_sse_keepalive {
        headers.remove("content-length");
    }
    let body_stream = build_sse_body_stream(
        prefetched_chunks_for_body,
        rx,
        emit_sse_keepalive,
        SSE_KEEPALIVE_INTERVAL,
    );

    Ok(Some(build_client_response_from_parts(
        status_code,
        &headers,
        Body::from_stream(body_stream),
        trace_id,
        Some(decision),
    )?))
}

fn apply_stream_summary_report_context(
    execution: &mut DirectUpstreamStreamExecution,
    report_context: Option<&Value>,
) {
    if let Some(report_context) = report_context.cloned() {
        execution.stream_summary_report_context = report_context;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use aether_contracts::{
        ExecutionPlan, ExecutionStreamTerminalSummary, ExecutionTimeouts, RequestBody,
        StandardizedUsage,
    };
    use aether_data::repository::candidates::InMemoryRequestCandidateRepository;
    use aether_data::repository::usage::InMemoryUsageReadRepository;
    use aether_data::{UsageObjectStore, UsageObjectStoreConfig};
    use aether_data_contracts::repository::candidates::{
        RequestCandidateReadRepository, RequestCandidateStatus,
    };
    use aether_data_contracts::repository::usage::{
        UsageBodyCaptureState, UsageBodyField, UsageReadRepository,
    };
    use aether_usage_runtime::UsageRuntimeConfig;
    use async_stream::stream;
    use axum::body::{to_bytes, Body, Bytes};
    use axum::extract::ws::Message;
    use axum::extract::Request;
    use axum::routing::any;
    use axum::{http::header, http::HeaderValue, Router};
    use futures_util::StreamExt as _;
    use serde_json::{json, Value};
    use sha2::Digest as _;
    use tokio::sync::{mpsc, watch, Notify};

    use super::{
        attach_stream_body_object_descriptors, build_sse_body_stream, build_stream_body_capture,
        encode_openai_responses_failed_event, execute_execution_runtime_stream,
        execute_stream_from_frame_stream, execution_frame_max_bytes, extend_bounded,
        maybe_apply_kiro_prompt_cache_usage_to_stream_summary, merge_stream_terminal_summary,
        merge_stream_usage_report_context, should_limit_direct_finalize_prefetch,
        should_probe_success_failover_before_stream, should_skip_direct_finalize_prefetch,
        OpenAiResponsesImageTerminalTracker, StreamObjectCapture,
    };
    use crate::control::GatewayControlDecision;
    use crate::request_candidate_runtime::flush_request_candidate_status_writes;
    use crate::tunnel::{tunnel_protocol, TunnelProxyConn};
    use crate::AppState;

    #[test]
    fn stream_failover_execution_frames_have_a_stricter_hard_limit() {
        assert_eq!(
            execution_frame_max_bytes(true, "openai:responses"),
            2 * 1024 * 1024
        );
        assert_eq!(
            execution_frame_max_bytes(false, "openai:responses"),
            64 * 1024 * 1024
        );
        assert_eq!(
            execution_frame_max_bytes(true, "openai:chat"),
            64 * 1024 * 1024
        );
    }

    #[test]
    fn bounded_prefetch_buffer_never_exceeds_configured_limit() {
        let mut buffer = b"1234".to_vec();
        extend_bounded(&mut buffer, b"567890", 7);
        assert_eq!(buffer, b"1234567");
        extend_bounded(&mut buffer, b"ignored", 7);
        assert_eq!(buffer, b"1234567");
    }

    #[test]
    fn stream_usage_report_context_falls_back_to_execution_plan_formats() {
        let plan = openai_responses_stream_failover_test_plan("req-context-fallback-1");
        let context = merge_stream_usage_report_context(Some(json!({})), &plan);

        assert_eq!(
            context.get("provider_api_format").and_then(Value::as_str),
            Some("openai:responses")
        );
        assert_eq!(
            context.get("client_api_format").and_then(Value::as_str),
            Some("openai:responses")
        );
    }

    fn test_decision() -> GatewayControlDecision {
        GatewayControlDecision::synthetic(
            "/v1/chat/completions",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
        )
        .with_execution_runtime_candidate(true)
    }

    fn openai_responses_stream_failover_test_plan(request_id: &str) -> ExecutionPlan {
        ExecutionPlan {
            request_id: request_id.into(),
            candidate_id: Some(format!("cand-{request_id}")),
            provider_name: Some("codex-pro".into()),
            provider_id: "prov-capacity".into(),
            endpoint_id: "ep-capacity".into(),
            key_id: "key-capacity".into(),
            method: "POST".into(),
            url: "https://chatgpt.com/backend-api/codex/responses".into(),
            headers: BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
            ]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-5.6-sol",
                "input": "hello",
                "stream": true
            })),
            stream: true,
            client_api_format: "openai:responses".into(),
            provider_api_format: "openai:responses".into(),
            model_name: Some("gpt-5.6-sol".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        }
    }

    fn openai_responses_stream_failover_test_context(request_id: &str) -> Value {
        json!({
            "request_id": request_id,
            "candidate_id": format!("cand-{request_id}"),
            "candidate_index": 0,
            "retry_index": 0,
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:responses",
            "local_failover_policy": {},
            "stream_failover_policy": {
                "enabled": true,
                "max_wait_ms": 5000,
                "max_buffer_bytes": 65536,
                "cooldown_seconds": 30
            }
        })
    }

    #[tokio::test]
    async fn stream_object_capture_preserves_large_body_and_hash() {
        let root = std::env::temp_dir().join(format!(
            "aether-gateway-stream-object-capture-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("create stream object store");
        let store = Arc::new(
            UsageObjectStore::from_config(&UsageObjectStoreConfig::new(
                format!("file://{}", root.display()),
                "usage-test",
            ))
            .expect("build stream object store"),
        );
        let mut capture = StreamObjectCapture::new(
            Some(Arc::clone(&store)),
            "request-stream-object-1",
            UsageBodyField::ResponseBody,
            Some("text/event-stream"),
            true,
        );
        let first = vec![b'a'; 5 * 1024 * 1024];
        let second = vec![b'b'; 1024 * 1024 + 17];
        capture.write(&first).await;
        capture.write(&second).await;

        let (descriptor, state) = capture.finish().await;
        assert_eq!(state, UsageBodyCaptureState::Reference);
        let descriptor = descriptor.expect("stream object descriptor should exist");
        let object_key = descriptor["object_key"]
            .as_str()
            .expect("descriptor should contain object key");
        assert_eq!(
            descriptor["size_bytes"].as_u64(),
            Some((first.len() + second.len()) as u64)
        );

        let mut expected = first;
        expected.extend_from_slice(&second);
        let expected_hash = format!("{:x}", sha2::Sha256::digest(&expected));
        assert_eq!(descriptor["sha256"].as_str(), Some(expected_hash.as_str()));
        assert_eq!(
            store
                .get(object_key)
                .await
                .expect("read stream object")
                .as_ref(),
            expected.as_slice()
        );

        store
            .delete(object_key)
            .await
            .expect("delete stream object");
        std::fs::remove_dir_all(root).expect("remove stream object store");
    }

    #[test]
    fn unavailable_stream_object_capture_falls_back_to_complete_inline_body() {
        let (body_base64, state) = build_stream_body_capture(
            b"complete response",
            UsageBodyCaptureState::Unavailable,
            false,
        );

        assert_eq!(body_base64.as_deref(), Some("Y29tcGxldGUgcmVzcG9uc2U="));
        assert_eq!(state, Some(UsageBodyCaptureState::Inline));
    }

    #[test]
    fn unavailable_stream_object_capture_falls_back_to_truncated_inline_body() {
        let (body_base64, state) = build_stream_body_capture(
            b"partial response",
            UsageBodyCaptureState::Unavailable,
            true,
        );

        assert_eq!(body_base64.as_deref(), Some("cGFydGlhbCByZXNwb25zZQ=="));
        assert_eq!(state, Some(UsageBodyCaptureState::Truncated));
    }

    #[test]
    fn unavailable_stream_object_descriptor_does_not_override_inline_fallback() {
        let mut context = Some(json!({
            "response_body_ref": "usage://request/stale/response_body",
            "client_response_body_ref": "usage://request/stale/client_response_body"
        }));
        attach_stream_body_object_descriptors(
            &mut context,
            Some(json!({
                "body_ref": "usage://request/current/response_body",
                "storage_status": "unavailable",
                "error_message": "usage object store is not configured"
            })),
            Some(json!({
                "body_ref": "usage://request/current/client_response_body",
                "storage_status": "unavailable",
                "error_message": "usage object store is not configured"
            })),
        );

        let context = context.expect("report context should exist");
        assert!(context.get("response_body_ref").is_none());
        assert!(context.get("client_response_body_ref").is_none());
        assert_eq!(
            context["usage_body_objects"]["response_body"]["storage_status"],
            json!("unavailable")
        );
        assert_eq!(
            context["usage_body_objects"]["client_response_body"]["storage_status"],
            json!("unavailable")
        );
    }

    #[test]
    fn incomplete_image_stream_is_rejected_at_eof() {
        let mut tracker = OpenAiResponsesImageTerminalTracker::default();
        tracker.push(br#"event: response.output_item.added
data: {"type":"response.output_item.added","item":{"type":"image_generation_call","status":"in_progress"}}

event: response.image_generation_call.generating
data: {"type":"response.image_generation_call.generating"}

"#);

        assert!(tracker.incomplete_at_eof());
    }

    #[test]
    fn completed_image_stream_with_result_is_accepted_at_eof() {
        let mut tracker = OpenAiResponsesImageTerminalTracker::default();
        tracker.push(br#"event: response.output_item.added
data: {"type":"response.output_item.added","item":{"type":"image_generation_call","status":"in_progress"}}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"type":"image_generation_call","status":"completed","result":"aGVsbG8="}}

event: response.completed
data: {"type":"response.completed","response":{"status":"completed"}}

"#);

        assert!(!tracker.incomplete_at_eof());
    }

    #[test]
    fn multiple_image_calls_require_a_result_for_each_call() {
        let mut tracker = OpenAiResponsesImageTerminalTracker::default();
        tracker.push(br#"event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"ig_1","type":"image_generation_call"}}

event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"ig_2","type":"image_generation_call"}}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"id":"ig_1","type":"image_generation_call","result":"aGVsbG8="}}

event: response.completed
data: {"type":"response.completed","response":{"status":"completed"}}

"#);

        assert!(tracker.incomplete_at_eof());
    }

    #[test]
    fn response_completed_event_name_without_data_is_not_terminal() {
        let mut tracker = OpenAiResponsesImageTerminalTracker::default();
        tracker.push(br#"event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"ig_1","type":"image_generation_call"}}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"id":"ig_1","type":"image_generation_call","result":"aGVsbG8="}}

event: response.completed
"#);

        assert!(tracker.incomplete_at_eof());
    }

    #[test]
    fn image_terminal_tracker_bounds_unterminated_sse_line_memory() {
        let mut tracker = OpenAiResponsesImageTerminalTracker::default();
        let mut chunk = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"image_generation_call\",\"result\":\"".to_vec();
        chunk.extend(std::iter::repeat_n(b'A', 1_000_000));
        tracker.push(&chunk);

        assert!(tracker.buffered.len() <= 64 * 1024);
    }

    #[test]
    fn incomplete_image_stream_failure_uses_responses_failed_event() {
        let failure = super::build_stream_failure_report(
            "incomplete_image_generation_stream",
            "image stream ended early",
            502,
        );
        let event = encode_openai_responses_failed_event(&failure).expect("event should encode");
        let text = String::from_utf8(event.to_vec()).expect("event should be utf-8");

        assert!(text.starts_with("event: response.failed\n"));
        assert!(text.contains("\"type\":\"response.failed\""));
        assert!(text.contains("incomplete_image_generation_stream"));
    }

    fn tunnel_proxy_snapshot(base_url: String) -> aether_contracts::ProxySnapshot {
        aether_contracts::ProxySnapshot {
            enabled: Some(true),
            mode: Some("tunnel".into()),
            node_id: Some("node-1".into()),
            label: Some("relay-node".into()),
            url: None,
            extra: Some(json!({"tunnel_base_url": base_url})),
        }
    }

    #[test]
    fn merge_stream_terminal_summary_prefers_more_complete_observed_usage() {
        let mut runtime_usage = StandardizedUsage::new();
        runtime_usage.output_tokens = 137;
        let mut observed_usage = StandardizedUsage::new();
        observed_usage.input_tokens = 26;
        observed_usage.output_tokens = 137;

        let merged = merge_stream_terminal_summary(
            Some(ExecutionStreamTerminalSummary {
                standardized_usage: Some(runtime_usage),
                model: Some("gpt-5.5".to_string()),
                unknown_event_count: 1,
                ..ExecutionStreamTerminalSummary::default()
            }),
            Some(ExecutionStreamTerminalSummary {
                standardized_usage: Some(observed_usage),
                response_id: Some("resp_123".to_string()),
                observed_finish: true,
                unknown_event_count: 2,
                ..ExecutionStreamTerminalSummary::default()
            }),
        )
        .expect("summary should merge");
        let usage = merged
            .standardized_usage
            .expect("merged usage should exist");

        assert_eq!(usage.input_tokens, 26);
        assert_eq!(usage.output_tokens, 137);
        assert_eq!(merged.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(merged.response_id.as_deref(), Some("resp_123"));
        assert!(merged.observed_finish);
        assert_eq!(merged.unknown_event_count, 3);
    }

    #[test]
    fn kiro_stream_summary_applies_prompt_cache_usage_from_original_request() {
        let request_body = json!({
            "model": "claude-opus-4-7",
            "system": [
                {
                    "type": "text",
                    "text": "cacheable system ".repeat(600),
                    "cache_control": {"type": "ephemeral"}
                }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "cacheable prompt ".repeat(1200),
                            "cache_control": {"type": "ephemeral"}
                        }
                    ]
                }
            ]
        });
        let report_context = json!({
            "original_request_body": request_body,
            "kiro_simulated_cache_enabled": true,
        });
        let plan = ExecutionPlan {
            request_id: "req-kiro-cache-stream".into(),
            candidate_id: Some("cand-kiro-cache-stream".into()),
            provider_name: Some("Kiro".into()),
            provider_id: "provider-kiro-cache-stream".into(),
            endpoint_id: "endpoint-kiro-cache-stream".into(),
            key_id: "key-kiro-cache-stream".into(),
            method: "POST".into(),
            url: "https://q.us-east-1.amazonaws.com/generateAssistantResponse?beta=true".into(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"conversationState": {}})),
            stream: true,
            client_api_format: "claude:messages".into(),
            provider_api_format: "claude:messages".into(),
            model_name: Some("claude-opus-4-7".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };

        let mut first_summary = Some(ExecutionStreamTerminalSummary {
            standardized_usage: Some(StandardizedUsage {
                input_tokens: 6_000,
                output_tokens: 17,
                ..StandardizedUsage::new()
            }),
            ..ExecutionStreamTerminalSummary::default()
        });
        maybe_apply_kiro_prompt_cache_usage_to_stream_summary(
            &plan,
            Some(&report_context),
            &mut first_summary,
        );
        let first_usage = first_summary
            .as_ref()
            .and_then(|summary| summary.standardized_usage.as_ref())
            .expect("first usage should exist");
        assert!(first_usage.cache_creation_tokens > 0);
        assert_eq!(first_usage.cache_read_tokens, 0);

        let mut second_summary = Some(ExecutionStreamTerminalSummary {
            standardized_usage: Some(StandardizedUsage {
                input_tokens: 6_000,
                output_tokens: 19,
                ..StandardizedUsage::new()
            }),
            ..ExecutionStreamTerminalSummary::default()
        });
        maybe_apply_kiro_prompt_cache_usage_to_stream_summary(
            &plan,
            Some(&report_context),
            &mut second_summary,
        );
        let second_usage = second_summary
            .as_ref()
            .and_then(|summary| summary.standardized_usage.as_ref())
            .expect("second usage should exist");
        assert!(second_usage.cache_read_tokens > 0);
        assert_eq!(second_usage.cache_creation_tokens, 0);
        assert!(second_usage.input_tokens < 6_000);
        assert_eq!(second_usage.output_tokens, 19);
    }

    #[test]
    fn kiro_stream_summary_seeds_input_tokens_without_cache_control() {
        let request_body = json!({
            "model": "claude-opus-4-7",
            "system": [
                {
                    "type": "text",
                    "text": "non cacheable system ".repeat(400)
                }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "non cacheable prompt ".repeat(800)
                        }
                    ]
                }
            ]
        });
        let report_context = json!({
            "original_request_body": request_body,
            "kiro_simulated_cache_enabled": true,
        });
        let plan = ExecutionPlan {
            request_id: "req-kiro-non-cache".into(),
            candidate_id: Some("cand-kiro-non-cache".into()),
            provider_name: Some("Kiro".into()),
            provider_id: "provider-kiro-non-cache".into(),
            endpoint_id: "endpoint-kiro-non-cache".into(),
            key_id: "key-kiro-non-cache".into(),
            method: "POST".into(),
            url: "https://q.us-east-1.amazonaws.com/generateAssistantResponse?beta=true".into(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"conversationState": {}})),
            stream: true,
            client_api_format: "claude:messages".into(),
            provider_api_format: "claude:messages".into(),
            model_name: Some("claude-opus-4-7".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };

        let mut summary = Some(ExecutionStreamTerminalSummary {
            standardized_usage: Some(StandardizedUsage {
                input_tokens: 0,
                output_tokens: 13,
                ..StandardizedUsage::new()
            }),
            ..ExecutionStreamTerminalSummary::default()
        });

        maybe_apply_kiro_prompt_cache_usage_to_stream_summary(
            &plan,
            Some(&report_context),
            &mut summary,
        );

        let usage = summary
            .as_ref()
            .and_then(|summary| summary.standardized_usage.as_ref())
            .expect("usage should exist");

        assert!(usage.input_tokens > 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.output_tokens, 13);
    }

    #[test]
    fn kiro_stream_summary_bills_existing_cache_usage_when_input_is_zero() {
        let request_body = json!({
            "model": "claude-opus-4-7",
            "system": [
                {
                    "type": "text",
                    "text": "cached system ".repeat(800)
                }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "cached prompt ".repeat(1400)
                        }
                    ]
                }
            ]
        });
        let report_context = json!({
            "original_request_body": request_body,
            "kiro_simulated_cache_enabled": true,
        });
        let plan = ExecutionPlan {
            request_id: "req-kiro-existing-cache".into(),
            candidate_id: Some("cand-kiro-existing-cache".into()),
            provider_name: Some("Kiro".into()),
            provider_id: "provider-kiro-existing-cache".into(),
            endpoint_id: "endpoint-kiro-existing-cache".into(),
            key_id: "key-kiro-existing-cache".into(),
            method: "POST".into(),
            url: "https://q.us-east-1.amazonaws.com/generateAssistantResponse?beta=true".into(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"conversationState": {}})),
            stream: true,
            client_api_format: "claude:messages".into(),
            provider_api_format: "claude:messages".into(),
            model_name: Some("claude-opus-4-7".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };

        let mut summary = Some(ExecutionStreamTerminalSummary {
            standardized_usage: Some(StandardizedUsage {
                input_tokens: 0,
                output_tokens: 23,
                cache_read_tokens: 200,
                ..StandardizedUsage::new()
            }),
            ..ExecutionStreamTerminalSummary::default()
        });

        maybe_apply_kiro_prompt_cache_usage_to_stream_summary(
            &plan,
            Some(&report_context),
            &mut summary,
        );

        let usage = summary
            .as_ref()
            .and_then(|summary| summary.standardized_usage.as_ref())
            .expect("usage should exist");

        assert!(usage.input_tokens > 0);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.output_tokens, 23);
    }

    #[test]
    fn kiro_stream_summary_clears_cache_usage_when_simulated_cache_disabled() {
        let request_body = json!({
            "model": "claude-opus-4-7",
            "system": [
                {
                    "type": "text",
                    "text": "disabled cache summary system ".repeat(800),
                    "cache_control": {"type": "ephemeral"}
                }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "disabled cache summary prompt ".repeat(1400),
                            "cache_control": {"type": "ephemeral"}
                        }
                    ]
                }
            ]
        });
        let report_context = json!({
            "original_request_body": request_body,
        });
        let plan = ExecutionPlan {
            request_id: "req-kiro-summary-cache-disabled".into(),
            candidate_id: Some("cand-kiro-summary-cache-disabled".into()),
            provider_name: Some("Kiro".into()),
            provider_id: "provider-kiro-summary-cache-disabled".into(),
            endpoint_id: "endpoint-kiro-summary-cache-disabled".into(),
            key_id: "key-kiro-summary-cache-disabled".into(),
            method: "POST".into(),
            url: "https://q.us-east-1.amazonaws.com/generateAssistantResponse?beta=true".into(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"conversationState": {}})),
            stream: true,
            client_api_format: "claude:messages".into(),
            provider_api_format: "claude:messages".into(),
            model_name: Some("claude-opus-4-7".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };

        let mut summary = Some(ExecutionStreamTerminalSummary {
            standardized_usage: Some(StandardizedUsage {
                input_tokens: 0,
                output_tokens: 23,
                cache_creation_tokens: 500,
                cache_read_tokens: 700,
                ..StandardizedUsage::new()
            }),
            ..ExecutionStreamTerminalSummary::default()
        });

        maybe_apply_kiro_prompt_cache_usage_to_stream_summary(
            &plan,
            Some(&report_context),
            &mut summary,
        );

        let usage = summary
            .as_ref()
            .and_then(|summary| summary.standardized_usage.as_ref())
            .expect("usage should exist");

        assert!(usage.input_tokens > 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.output_tokens, 23);
    }

    #[test]
    fn kiro_stream_summary_does_not_subtract_cache_from_already_billed_input() {
        let request_body = json!({
            "model": "claude-opus-4-7",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "cached history ".repeat(400),
                            "cache_control": {"type": "ephemeral"}
                        },
                        {
                            "type": "text",
                            "text": "new user turn"
                        }
                    ]
                }
            ]
        });
        let report_context = json!({
            "original_request_body": request_body,
            "input_tokens": 24_770,
            "cache_creation_input_tokens": 175,
            "cache_read_input_tokens": 24_463,
            "kiro_simulated_cache_enabled": true
        });
        let plan = ExecutionPlan {
            request_id: "req-kiro-billed-input".into(),
            candidate_id: Some("cand-kiro-billed-input".into()),
            provider_name: Some("Kiro".into()),
            provider_id: "provider-kiro-billed-input".into(),
            endpoint_id: "endpoint-kiro-billed-input".into(),
            key_id: "key-kiro-billed-input".into(),
            method: "POST".into(),
            url: "https://q.us-east-1.amazonaws.com/generateAssistantResponse?beta=true".into(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"conversationState": {}})),
            stream: true,
            client_api_format: "claude:messages".into(),
            provider_api_format: "claude:messages".into(),
            model_name: Some("claude-opus-4-7".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };

        let mut summary = Some(ExecutionStreamTerminalSummary {
            standardized_usage: Some(StandardizedUsage {
                input_tokens: 132,
                output_tokens: 167,
                cache_creation_tokens: 175,
                cache_read_tokens: 24_463,
                ..StandardizedUsage::new()
            }),
            ..ExecutionStreamTerminalSummary::default()
        });

        maybe_apply_kiro_prompt_cache_usage_to_stream_summary(
            &plan,
            Some(&report_context),
            &mut summary,
        );

        let usage = summary
            .as_ref()
            .and_then(|summary| summary.standardized_usage.as_ref())
            .expect("usage should exist");

        assert_eq!(usage.input_tokens, 132);
        assert_eq!(usage.cache_creation_tokens, 175);
        assert_eq!(usage.cache_read_tokens, 24_463);
        assert_eq!(usage.output_tokens, 167);
    }

    #[tokio::test]
    async fn kiro_report_context_seeds_input_tokens_from_original_request_body() {
        let request_body = json!({
            "model": "claude-opus-4-7",
            "system": [
                {
                    "type": "text",
                    "text": "seeded system ".repeat(600)
                }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "seeded prompt ".repeat(1200)
                        }
                    ]
                }
            ]
        });
        let plan = ExecutionPlan {
            request_id: "req-kiro-seed".into(),
            candidate_id: Some("cand-kiro-seed".into()),
            provider_name: Some("Kiro".into()),
            provider_id: "provider-kiro-seed".into(),
            endpoint_id: "endpoint-kiro-seed".into(),
            key_id: "key-kiro-seed".into(),
            method: "POST".into(),
            url: "https://q.us-east-1.amazonaws.com/generateAssistantResponse?beta=true".into(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"conversationState": {}})),
            stream: true,
            client_api_format: "claude:messages".into(),
            provider_api_format: "claude:messages".into(),
            model_name: Some("claude-opus-4-7".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };
        let mut report_context = Some(json!({
            "original_request_body": request_body,
            "kiro_simulated_cache_enabled": true,
        }));

        super::seed_kiro_report_context_input_tokens(&plan, &mut report_context);

        let input_tokens = report_context
            .as_ref()
            .and_then(|context| context.get("input_tokens"))
            .and_then(Value::as_u64)
            .expect("kiro input tokens should be seeded");
        assert!(input_tokens > 0);
    }

    #[tokio::test]
    async fn kiro_report_context_seeds_prompt_cache_usage_before_stream_rewrite() {
        let request_body = json!({
            "model": "claude-opus-4-7",
            "system": [
                {
                    "type": "text",
                    "text": "cache seed system ".repeat(600),
                    "cache_control": {"type": "ephemeral"}
                }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "cache seed prompt ".repeat(1200),
                            "cache_control": {"type": "ephemeral"}
                        }
                    ]
                }
            ]
        });
        let plan = ExecutionPlan {
            request_id: "req-kiro-cache-seed".into(),
            candidate_id: Some("cand-kiro-cache-seed".into()),
            provider_name: Some("Kiro".into()),
            provider_id: "provider-kiro-cache-seed".into(),
            endpoint_id: "endpoint-kiro-cache-seed".into(),
            key_id: "key-kiro-cache-seed".into(),
            method: "POST".into(),
            url: "https://q.us-east-1.amazonaws.com/generateAssistantResponse?beta=true".into(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"conversationState": {}})),
            stream: true,
            client_api_format: "claude:messages".into(),
            provider_api_format: "claude:messages".into(),
            model_name: Some("claude-opus-4-7".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };
        let mut report_context = Some(json!({
            "original_request_body": request_body,
            "kiro_simulated_cache_enabled": true,
        }));

        super::seed_kiro_report_context_input_tokens(&plan, &mut report_context);
        super::seed_kiro_report_context_prompt_cache_usage(&plan, &mut report_context);

        let context = report_context.as_ref().expect("context should exist");
        assert!(context
            .get("input_tokens")
            .and_then(Value::as_u64)
            .is_some_and(|value| value > 0));
        assert!(context
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .is_some_and(|value| value > 0));
        assert_eq!(
            context
                .get("cache_read_input_tokens")
                .and_then(Value::as_u64),
            Some(0)
        );
    }

    #[tokio::test]
    async fn kiro_report_context_skips_prompt_cache_usage_when_disabled() {
        let request_body = json!({
            "model": "claude-opus-4-7",
            "system": [
                {
                    "type": "text",
                    "text": "disabled cache system ".repeat(600),
                    "cache_control": {"type": "ephemeral"}
                }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "disabled cache prompt ".repeat(1200),
                            "cache_control": {"type": "ephemeral"}
                        }
                    ]
                }
            ]
        });
        let plan = ExecutionPlan {
            request_id: "req-kiro-cache-disabled".into(),
            candidate_id: Some("cand-kiro-cache-disabled".into()),
            provider_name: Some("Kiro".into()),
            provider_id: "provider-kiro-cache-disabled".into(),
            endpoint_id: "endpoint-kiro-cache-disabled".into(),
            key_id: "key-kiro-cache-disabled".into(),
            method: "POST".into(),
            url: "https://q.us-east-1.amazonaws.com/generateAssistantResponse?beta=true".into(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"conversationState": {}})),
            stream: true,
            client_api_format: "claude:messages".into(),
            provider_api_format: "claude:messages".into(),
            model_name: Some("claude-opus-4-7".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };
        let mut report_context = Some(json!({
            "original_request_body": request_body,
        }));

        super::seed_kiro_report_context_input_tokens(&plan, &mut report_context);
        super::seed_kiro_report_context_prompt_cache_usage(&plan, &mut report_context);

        let context = report_context.as_ref().expect("context should exist");
        assert!(context
            .get("input_tokens")
            .and_then(Value::as_u64)
            .is_some_and(|value| value > 0));
        assert_eq!(context.get("cache_creation_input_tokens"), None);
        assert_eq!(context.get("cache_read_input_tokens"), None);
    }

    #[test]
    fn skips_prefetch_for_same_format_passthrough_event_streams() {
        assert!(should_skip_direct_finalize_prefetch(
            Some("claude_cli_sync_finalize"),
            Some("text/event-stream"),
            "claude:messages",
            "claude:messages",
            false,
            false,
        ));
    }

    #[test]
    fn keeps_prefetch_for_openai_responses_event_streams() {
        assert!(!should_skip_direct_finalize_prefetch(
            Some("openai_responses_stream_finalize"),
            Some("text/event-stream"),
            "openai:responses",
            "openai:responses",
            false,
            false,
        ));
    }

    #[test]
    fn skips_prefetch_for_same_format_passthrough_streams_without_content_type() {
        assert!(should_skip_direct_finalize_prefetch(
            Some("claude_cli_sync_finalize"),
            None,
            "claude:messages",
            "claude:messages",
            false,
            false,
        ));
    }

    #[test]
    fn keeps_prefetch_for_same_format_json_streams() {
        assert!(!should_skip_direct_finalize_prefetch(
            Some("claude_cli_sync_finalize"),
            Some("application/json"),
            "claude:messages",
            "claude:messages",
            false,
            false,
        ));
    }

    #[test]
    fn skips_prefetch_for_event_streams_even_when_cross_format_or_rewritten() {
        assert!(should_skip_direct_finalize_prefetch(
            Some("claude_cli_sync_finalize"),
            Some("text/event-stream"),
            "openai:chat",
            "claude:messages",
            false,
            true,
        ));
    }

    #[tokio::test]
    async fn openai_responses_capacity_failure_retries_before_client_output() {
        let state = AppState::new().expect("app state should build");
        let plan = ExecutionPlan {
            request_id: "req-openai-capacity-failover".into(),
            candidate_id: Some("cand-openai-capacity-failover".into()),
            provider_name: Some("codex-pro".into()),
            provider_id: "prov-capacity".into(),
            endpoint_id: "ep-capacity".into(),
            key_id: "key-capacity".into(),
            method: "POST".into(),
            url: "https://chatgpt.com/backend-api/codex/responses".into(),
            headers: BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
            ]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-5.4",
                "input": "hello",
                "stream": true
            })),
            stream: true,
            client_api_format: "openai:responses".into(),
            provider_api_format: "openai:responses".into(),
            model_name: Some("gpt-5.4".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };
        let frame_stream = stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"text/event-stream\"}}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.created\\ndata: {\\\"type\\\":\\\"response.created\\\",\\\"response\\\":{\\\"status\\\":\\\"in_progress\\\"}}\\n\\n\"}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.failed\\ndata: {\\\"type\\\":\\\"response.failed\\\",\\\"response\\\":{\\\"status\\\":\\\"failed\\\",\\\"error\\\":{\\\"code\\\":\\\"server_is_overloaded\\\",\\\"message\\\":\\\"Selected model is at capacity. Please try a different model.\\\"}}}\\n\\n\"}}\n",
            ));
        }
        .boxed();

        let response = execute_stream_from_frame_stream(
            &state,
            plan,
            "trace-openai-capacity-failover",
            &test_decision(),
            "openai_responses_stream",
            Some("openai_responses_stream_success".to_string()),
            Some(json!({
                "request_id": "req-openai-capacity-failover",
                "candidate_id": "cand-openai-capacity-failover",
                "candidate_index": 0,
                "retry_index": 0,
                "provider_api_format": "openai:responses",
                "client_api_format": "openai:responses",
                "local_failover_policy": {
                    "stop_status_codes": [400, 503]
                }
            })),
            crate::clock::current_unix_ms(),
            Instant::now(),
            frame_stream,
            None,
        )
        .await
        .expect("execution should succeed");

        assert!(
            response.is_none(),
            "capacity failure should try the next account"
        );
    }

    #[test]
    fn skips_success_failover_probe_for_event_streams() {
        assert!(!should_probe_success_failover_before_stream(
            &BTreeMap::from([(
                "content-type".to_string(),
                "text/event-stream; charset=utf-8".to_string(),
            )])
        ));
        assert!(should_probe_success_failover_before_stream(
            &BTreeMap::from([("content-type".to_string(), "application/json".to_string(),)])
        ));
    }

    #[test]
    fn limits_prefetch_for_openai_image_and_rewritten_streams() {
        assert!(should_limit_direct_finalize_prefetch(
            "openai_image_stream",
            false
        ));
        assert!(should_limit_direct_finalize_prefetch(
            "openai_chat_stream",
            true
        ));
        assert!(!should_limit_direct_finalize_prefetch(
            "openai_chat_stream",
            false
        ));
    }

    #[tokio::test]
    async fn openai_responses_capacity_failure_retries_after_more_than_five_lifecycle_frames() {
        let state = AppState::new().expect("app state should build");
        let plan = openai_responses_stream_failover_test_plan("req-openai-capacity-failover");
        let frame_stream = stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"text/event-stream\"}}}\n",
            ));
            for _ in 0..8 {
                yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.in_progress\\ndata: {\\\"type\\\":\\\"response.in_progress\\\",\\\"response\\\":{\\\"status\\\":\\\"in_progress\\\"}}\\n\\n\"}}\n",
                ));
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.failed\\ndata: {\\\"type\\\":\\\"response.failed\\\",\\\"response\\\":{\\\"status\\\":\\\"failed\\\",\\\"error\\\":{\\\"code\\\":\\\"server_is_overloaded\\\",\\\"message\\\":\\\"Selected model is at capacity. Please try a different model.\\\"}}}\\n\\n\"}}\n",
            ));
        }
        .boxed();

        let response = execute_stream_from_frame_stream(
            &state,
            plan,
            "trace-openai-capacity-failover",
            &test_decision(),
            "openai_responses_stream",
            Some("openai_responses_stream_success".to_string()),
            Some(openai_responses_stream_failover_test_context(
                "req-openai-capacity-failover",
            )),
            crate::clock::current_unix_ms(),
            Instant::now(),
            frame_stream,
            None,
        )
        .await
        .expect("execution should succeed");

        assert!(
            response.is_none(),
            "capacity failure should try another account"
        );
        let cooldown_reason = crate::handlers::shared::provider_pool::read_admin_provider_pool_key_model_cooldown_reason(
            state.runtime_state.as_ref(),
            "prov-capacity",
            "key-capacity",
            "gpt-5.6-sol",
        )
        .await
        .expect("model cooldown should be readable");
        assert_eq!(
            cooldown_reason.as_deref(),
            Some("openai_transient_model_capacity")
        );
    }

    #[tokio::test]
    async fn openai_responses_invalid_request_does_not_switch_accounts() {
        let state = AppState::new().expect("app state should build");
        let frame_stream = stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"text/event-stream\"}}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.failed\\ndata: {\\\"type\\\":\\\"response.failed\\\",\\\"response\\\":{\\\"status\\\":\\\"failed\\\",\\\"error\\\":{\\\"code\\\":\\\"invalid_request_error\\\",\\\"message\\\":\\\"Unknown parameter.\\\"}}}\\n\\n\"}}\n",
            ));
        }
        .boxed();

        let response = execute_stream_from_frame_stream(
            &state,
            openai_responses_stream_failover_test_plan("req-openai-invalid-request"),
            "trace-openai-invalid-request",
            &test_decision(),
            "openai_responses_stream",
            Some("openai_responses_stream_success".to_string()),
            Some(openai_responses_stream_failover_test_context(
                "req-openai-invalid-request",
            )),
            crate::clock::current_unix_ms(),
            Instant::now(),
            frame_stream,
            None,
        )
        .await
        .expect("execution should return the request error");

        assert!(response.is_some(), "invalid input must not switch accounts");
    }

    #[tokio::test]
    async fn openai_responses_direct_http_invalid_request_does_not_switch_accounts() {
        let state = AppState::new().expect("app state should build");
        let error_frame = format!(
            "{}\n",
            json!({
                "type": "data",
                "payload": {
                    "kind": "data",
                    "text": "{\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Invalid 'input[67].id'. Expected an ID that begins with 'rs'.\"}}"
                }
            })
        );
        let frame_stream = stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":400,\"headers\":{\"content-type\":\"application/json\"}}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from(error_frame));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"eof\",\"payload\":{\"kind\":\"eof\"}}\n",
            ));
        }
        .boxed();

        let response = execute_stream_from_frame_stream(
            &state,
            openai_responses_stream_failover_test_plan("req-openai-direct-invalid-request"),
            "trace-openai-direct-invalid-request",
            &test_decision(),
            "openai_responses_stream",
            Some("openai_responses_stream_success".to_string()),
            Some(openai_responses_stream_failover_test_context(
                "req-openai-direct-invalid-request",
            )),
            crate::clock::current_unix_ms(),
            Instant::now(),
            frame_stream,
            None,
        )
        .await
        .expect("execution should return the request error");

        assert!(response.is_some(), "invalid input must not switch accounts");
    }

    #[tokio::test]
    async fn openai_responses_failure_after_output_keeps_the_current_stream() {
        let state = AppState::new().expect("app state should build");
        let frame_stream = stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"text/event-stream\"}}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.output_text.delta\\ndata: {\\\"type\\\":\\\"response.output_text.delta\\\",\\\"delta\\\":\\\"hello\\\"}\\n\\n\"}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.failed\\ndata: {\\\"type\\\":\\\"response.failed\\\",\\\"response\\\":{\\\"error\\\":{\\\"code\\\":\\\"server_is_overloaded\\\"}}}\\n\\n\"}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"eof\",\"payload\":{\"kind\":\"eof\"}}\n",
            ));
        }
        .boxed();

        let response = execute_stream_from_frame_stream(
            &state,
            openai_responses_stream_failover_test_plan("req-openai-output-then-failure"),
            "trace-openai-output-then-failure",
            &test_decision(),
            "openai_responses_stream",
            Some("openai_responses_stream_success".to_string()),
            Some(openai_responses_stream_failover_test_context(
                "req-openai-output-then-failure",
            )),
            crate::clock::current_unix_ms(),
            Instant::now(),
            frame_stream,
            None,
        )
        .await
        .expect("execution should keep the committed stream")
        .expect("committed output must return a client response");
        let body = to_bytes(response.into_body(), 16 * 1024)
            .await
            .expect("response body should be readable");
        let body = String::from_utf8_lossy(&body);

        assert!(body.contains("hello"));
        assert!(body.contains("response.failed"));
    }

    #[tokio::test]
    async fn sse_body_stream_emits_initial_and_periodic_keepalive_without_business_chunks() {
        let (_tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(1);
        let mut body_stream = Box::pin(build_sse_body_stream(
            Vec::new(),
            rx,
            true,
            Duration::from_millis(10),
        ));

        let first = tokio::time::timeout(Duration::from_millis(50), body_stream.next())
            .await
            .expect("initial keepalive should be immediate")
            .expect("stream should yield initial keepalive")
            .expect("initial keepalive should be ok");
        assert_eq!(first.as_ref(), b": aether-keepalive\n\n");

        let second = tokio::time::timeout(Duration::from_millis(100), body_stream.next())
            .await
            .expect("periodic keepalive should arrive")
            .expect("stream should yield periodic keepalive")
            .expect("periodic keepalive should be ok");
        assert_eq!(second.as_ref(), b": aether-keepalive\n\n");
    }

    #[tokio::test]
    async fn openai_image_stream_total_timeout_emits_image_failed_event() {
        let state = AppState::new().expect("app state should build");
        let plan = ExecutionPlan {
            request_id: "req-image-stream-timeout".into(),
            candidate_id: Some("cand-image-stream-timeout".into()),
            provider_name: Some("codex".into()),
            provider_id: "prov-1".into(),
            endpoint_id: "ep-1".into(),
            key_id: "key-1".into(),
            method: "POST".into(),
            url: "https://chatgpt.com/backend-api/codex/responses".into(),
            headers: BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
            ]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-image-1",
                "prompt": "hello",
                "stream": true
            })),
            stream: true,
            client_api_format: "openai:image".into(),
            provider_api_format: "openai:image".into(),
            model_name: Some("gpt-image-1".into()),
            proxy: None,
            transport_profile: None,
            timeouts: Some(ExecutionTimeouts {
                total_ms: Some(25),
                ..ExecutionTimeouts::default()
            }),
        };
        let decision = GatewayControlDecision::synthetic(
            "/v1/images/generations",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("image".to_string()),
            Some("openai:image".to_string()),
        )
        .with_execution_runtime_candidate(true);
        let frame_stream = stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"text/event-stream\"}}}\n",
            ));
            std::future::pending::<()>().await;
        }
        .boxed();

        let response = execute_stream_from_frame_stream(
            &state,
            plan,
            "trace-image-stream-timeout",
            &decision,
            "openai_image_stream",
            None,
            Some(json!({
                "provider_api_format": "openai:image",
                "client_api_format": "openai:image",
                "image_request": {
                    "operation": "generate"
                }
            })),
            crate::clock::current_unix_ms(),
            Instant::now(),
            frame_stream,
            None,
        )
        .await
        .expect("execution should succeed")
        .expect("execution should return a client response");

        let body = tokio::time::timeout(
            Duration::from_secs(2),
            to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("timeout failure should close the response body")
        .expect("response body should read");
        let text = String::from_utf8(body.to_vec()).expect("response body should be utf8");
        assert!(text.contains(": aether-keepalive\n\n"));
        assert!(text.contains("event: image_generation.failed"));
        assert!(text.contains("\"type\":\"image_stream_total_timeout\""));
    }

    #[tokio::test]
    async fn execute_stream_from_frame_stream_stops_upstream_when_client_drops_body() {
        let usage_repository = Arc::new(InMemoryUsageReadRepository::default());
        let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state = AppState::new()
            .expect("app state should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_request_candidate_and_usage_repository_for_tests(
                    Arc::clone(&request_candidate_repository),
                    Arc::clone(&usage_repository),
                ),
            )
            .with_usage_runtime_for_tests(UsageRuntimeConfig {
                enabled: true,
                ..UsageRuntimeConfig::default()
            });
        let plan = ExecutionPlan {
            request_id: "req-client-drop-cancels-upstream".into(),
            candidate_id: Some("cand-client-drop-cancels-upstream".into()),
            provider_name: Some("openai".into()),
            provider_id: "prov-1".into(),
            endpoint_id: "ep-1".into(),
            key_id: "key-1".into(),
            method: "POST".into(),
            url: "https://example.com/v1/chat/completions".into(),
            headers: BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
            ]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-5.4",
                "messages": [],
                "stream": true
            })),
            stream: true,
            client_api_format: "openai:chat".into(),
            provider_api_format: "openai:chat".into(),
            model_name: Some("gpt-5.4".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };
        let frame_stream_dropped = Arc::new(Notify::new());
        let frame_stream_dropped_for_stream = Arc::clone(&frame_stream_dropped);
        let frame_stream = stream! {
            struct NotifyOnDrop(Arc<Notify>);
            impl Drop for NotifyOnDrop {
                fn drop(&mut self) {
                    self.0.notify_waiters();
                }
            }

            let _drop_guard = NotifyOnDrop(frame_stream_dropped_for_stream);
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"text/event-stream\"}}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"data: {\\\"id\\\":\\\"first\\\"}\\n\\n\"}}\n",
            ));
            std::future::pending::<()>().await;
        }
        .boxed();

        let response = execute_stream_from_frame_stream(
            &state,
            plan,
            "trace-client-drop-cancels-upstream",
            &test_decision(),
            "openai_chat_stream",
            None,
            Some(json!({
                "request_id": "req-client-drop-cancels-upstream",
                "candidate_id": "cand-client-drop-cancels-upstream",
                "candidate_index": 0,
                "retry_index": 0,
                "provider_api_format": "openai:chat",
                "client_api_format": "openai:chat"
            })),
            crate::clock::current_unix_ms(),
            Instant::now(),
            frame_stream,
            None,
        )
        .await
        .expect("execution should succeed")
        .expect("execution should return a client response");

        let mut body_stream = response.into_body().into_data_stream();
        let first = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let chunk = body_stream
                    .next()
                    .await
                    .expect("body should yield first chunk")
                    .expect("first chunk should be ok");
                if chunk.as_ref() != b": aether-keepalive\n\n" {
                    break chunk;
                }
            }
        })
        .await
        .expect("first business chunk should arrive");
        assert_eq!(first.as_ref(), b"data: {\"id\":\"first\"}\n\n");
        tokio::time::sleep(Duration::from_millis(30)).await;
        drop(body_stream);

        tokio::time::timeout(Duration::from_secs(1), frame_stream_dropped.notified())
            .await
            .expect("upstream frame stream should be dropped after client disconnect");
        let candidates = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let candidates = request_candidate_repository
                    .list_by_request_id("req-client-drop-cancels-upstream")
                    .await
                    .expect("request candidates should read");
                if candidates
                    .first()
                    .is_some_and(|candidate| candidate.status == RequestCandidateStatus::Cancelled)
                {
                    break candidates;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("candidate should be marked cancelled");
        assert_eq!(candidates[0].status_code, Some(499));
        assert_eq!(
            candidates[0].error_type.as_deref(),
            Some("downstream_disconnect")
        );

        let stored_usage = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let usage = usage_repository
                    .find_by_request_id("req-client-drop-cancels-upstream")
                    .await
                    .expect("usage should read");
                if usage
                    .as_ref()
                    .is_some_and(|usage| usage.status == "cancelled")
                {
                    break usage.expect("cancelled usage should exist");
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("usage should be marked cancelled");
        assert_eq!(stored_usage.billing_status, "pending");
        assert_eq!(stored_usage.status_code, Some(499));
        let first_byte_time_ms = stored_usage
            .first_byte_time_ms
            .expect("cancelled stream should retain first byte time");
        let response_time_ms = stored_usage
            .response_time_ms
            .expect("cancelled stream should record terminal duration");
        assert!(
            response_time_ms > first_byte_time_ms,
            "terminal duration should include time after the first byte"
        );
    }

    #[tokio::test]
    async fn image_stream_downstream_close_after_done_is_recorded_success() {
        let usage_repository = Arc::new(InMemoryUsageReadRepository::default());
        let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state = AppState::new()
            .expect("app state should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_request_candidate_and_usage_repository_for_tests(
                    Arc::clone(&request_candidate_repository),
                    Arc::clone(&usage_repository),
                ),
            )
            .with_usage_runtime_for_tests(UsageRuntimeConfig {
                enabled: true,
                ..UsageRuntimeConfig::default()
            });
        let plan = ExecutionPlan {
            request_id: "req-image-done-close-success".into(),
            candidate_id: Some("cand-image-done-close-success".into()),
            provider_name: Some("openai".into()),
            provider_id: "prov-1".into(),
            endpoint_id: "ep-1".into(),
            key_id: "key-1".into(),
            method: "POST".into(),
            url: "https://example.com/v1/images/generations".into(),
            headers: BTreeMap::from([("accept".into(), "text/event-stream".into())]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-image-2",
                "prompt": "draw a small image",
                "stream": true
            })),
            stream: true,
            client_api_format: "openai:chat".into(),
            provider_api_format: "openai:image".into(),
            model_name: Some("gpt-image-2".into()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        };
        let frame_stream = stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"text/event-stream\"}}}\n",
            ));
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.output_item.done\\ndata: {\\\"type\\\":\\\"response.output_item.done\\\",\\\"output_index\\\":0,\\\"item\\\":{\\\"id\\\":\\\"ig_1\\\",\\\"type\\\":\\\"image_generation_call\\\",\\\"result\\\":\\\"aGVsbG8=\\\"}}\\n\\nevent: response.completed\\ndata: {\\\"type\\\":\\\"response.completed\\\",\\\"response\\\":{\\\"id\\\":\\\"resp_1\\\",\\\"model\\\":\\\"gpt-image-2\\\",\\\"status\\\":\\\"completed\\\",\\\"usage\\\":null}}\\n\\n\"}}\n",
            ));
            std::future::pending::<()>().await;
        }
        .boxed();

        let response = execute_stream_from_frame_stream(
            &state,
            plan,
            "trace-image-done-close-success",
            &test_decision(),
            "openai_chat_stream",
            Some("openai_chat_stream_success".to_string()),
            Some(json!({
                "request_id": "req-image-done-close-success",
                "candidate_id": "cand-image-done-close-success",
                "candidate_index": 0,
                "retry_index": 0,
                "provider_api_format": "openai:image",
                "client_api_format": "openai:chat",
                "image_request": {
                    "size": "1024x1024",
                    "quality": "medium"
                }
            })),
            crate::clock::current_unix_ms(),
            Instant::now(),
            frame_stream,
            None,
        )
        .await
        .expect("execution should succeed")
        .expect("execution should return a client response");

        let mut body_stream = response.into_body().into_data_stream();
        let mut body = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), async {
            while !String::from_utf8_lossy(&body).contains("data: [DONE]") {
                let chunk = body_stream
                    .next()
                    .await
                    .expect("body should yield until done")
                    .expect("chunk should be ok");
                body.extend_from_slice(&chunk);
            }
        })
        .await
        .expect("final DONE should arrive");
        drop(body_stream);

        let candidates = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let candidates = request_candidate_repository
                    .list_by_request_id("req-image-done-close-success")
                    .await
                    .expect("request candidates should read");
                if candidates
                    .first()
                    .is_some_and(|candidate| candidate.status == RequestCandidateStatus::Success)
                {
                    break candidates;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("candidate should be marked success");
        assert_eq!(candidates[0].status_code, Some(200));

        let stored_usage = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let usage = usage_repository
                    .find_by_request_id("req-image-done-close-success")
                    .await
                    .expect("usage should read");
                if usage
                    .as_ref()
                    .is_some_and(|usage| usage.status == "completed")
                {
                    break usage.expect("completed usage should exist");
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("usage should be marked completed");
        assert_eq!(stored_usage.status_code, Some(200));
        assert!(stored_usage.total_tokens > 0);
    }

    #[tokio::test]
    async fn execute_execution_runtime_stream_records_first_data_as_streaming_before_terminal_telemetry(
    ) {
        let listener = crate::test_support::bind_loopback_listener()
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("local addr should resolve");
        let first_data_seen = Arc::new(Notify::new());
        let release_terminal = Arc::new(Notify::new());
        let first_data_seen_for_route = Arc::clone(&first_data_seen);
        let release_terminal_for_route = Arc::clone(&release_terminal);
        let server = tokio::spawn(async move {
            let app = Router::new().route(
                "/v1/execute/stream",
                any(move |_request: Request| {
                    let first_data_seen = Arc::clone(&first_data_seen_for_route);
                    let release_terminal = Arc::clone(&release_terminal_for_route);
                    async move {
                        let frames = stream! {
                            yield Ok::<Bytes, Infallible>(Bytes::from_static(
                                b"{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"text/event-stream\"}}}\n",
                            ));
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            yield Ok::<Bytes, Infallible>(Bytes::from_static(
                                b"{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"event: response.output_text.delta\\ndata: {\\\"type\\\":\\\"response.output_text.delta\\\",\\\"delta\\\":\\\"hi\\\"}\\n\\n\"}}\n",
                            ));
                            first_data_seen.notify_one();
                            release_terminal.notified().await;
                            yield Ok::<Bytes, Infallible>(Bytes::from_static(
                                b"{\"type\":\"telemetry\",\"payload\":{\"kind\":\"telemetry\",\"telemetry\":{\"ttfb_ms\":123,\"elapsed_ms\":456}}}\n",
                            ));
                            yield Ok::<Bytes, Infallible>(Bytes::from_static(
                                b"{\"type\":\"eof\",\"payload\":{\"kind\":\"eof\"}}\n",
                            ));
                        };
                        let mut response = axum::http::Response::new(Body::from_stream(frames));
                        response.headers_mut().insert(
                            header::CONTENT_TYPE,
                            HeaderValue::from_static("application/x-ndjson"),
                        );
                        response
                    }
                }),
            );
            axum::serve(listener, app)
                .await
                .expect("server should start");
        });

        let usage_repository = Arc::new(InMemoryUsageReadRepository::default());
        let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state = AppState::new()
            .expect("app state should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_request_candidate_and_usage_repository_for_tests(
                    Arc::clone(&request_candidate_repository),
                    Arc::clone(&usage_repository),
                ),
            )
            .with_usage_runtime_for_tests(UsageRuntimeConfig {
                enabled: true,
                ..UsageRuntimeConfig::default()
            })
            .with_execution_runtime_override_base_url(format!("http://{addr}"));
        let plan = ExecutionPlan {
            request_id: "req-live-stream-first-data".into(),
            candidate_id: Some("cand-live-stream-first-data".into()),
            provider_name: Some("openai".into()),
            provider_id: "prov-1".into(),
            endpoint_id: "ep-1".into(),
            key_id: "key-1".into(),
            method: "POST".into(),
            url: "https://chatgpt.com/backend-api/codex/responses".into(),
            headers: BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
            ]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-5.4",
                "input": "hello",
                "stream": true
            })),
            stream: true,
            client_api_format: "openai:responses".into(),
            provider_api_format: "openai:responses".into(),
            model_name: Some("gpt-5.4".into()),
            proxy: None,
            transport_profile: None,
            timeouts: Some(ExecutionTimeouts {
                connect_ms: Some(5_000),
                total_ms: Some(5_000),
                ..ExecutionTimeouts::default()
            }),
        };
        let decision = GatewayControlDecision::synthetic(
            "/v1/responses",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("cli".to_string()),
            Some("openai:responses".to_string()),
        )
        .with_execution_runtime_candidate(true);

        let response = execute_execution_runtime_stream(
            &state,
            plan,
            "trace-live-stream-first-data",
            &decision,
            "openai_responses_stream",
            None,
            Some(json!({
                "provider_api_format": "openai:responses",
                "client_api_format": "openai:responses",
            })),
        )
        .await
        .expect("execution should succeed")
        .expect("execution should return a client response");

        first_data_seen.notified().await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let live_usage = loop {
            let usage = usage_repository
                .find_by_request_id("trace-live-stream-first-data")
                .await
                .expect("usage should read");
            if usage.as_ref().is_some_and(|usage| {
                usage.status == "streaming" && usage.first_byte_time_ms.is_some()
            }) {
                break usage.expect("live usage should exist");
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "usage should record streaming status with first byte before terminal telemetry"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        assert_eq!(live_usage.status, "streaming");
        assert!(live_usage.first_byte_time_ms.is_some());

        release_terminal.notify_one();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let text = String::from_utf8(body.to_vec()).expect("response body should be utf8");
        assert!(text.contains("response.output_text.delta"));

        server.abort();
    }

    #[tokio::test]
    async fn execute_execution_runtime_stream_bridges_sync_json_body_from_remote_runtime_to_sse() {
        let listener = crate::test_support::bind_loopback_listener()
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("local addr should resolve");
        let server = tokio::spawn(async move {
            let app = Router::new().route(
                "/v1/execute/stream",
                any(|_request: Request| async move {
                    let frames = concat!(
                        "{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"application/json\"}}}\n",
                        "{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"{\\\"id\\\":\\\"resp-remote-runtime-sync-json-123\\\",\\\"object\\\":\\\"response\\\",\\\"model\\\":\\\"gpt-5.4\\\",\\\"status\\\":\\\"completed\\\",\\\"output\\\":[{\\\"type\\\":\\\"message\\\",\\\"id\\\":\\\"msg-remote-runtime-sync-json-123\\\",\\\"role\\\":\\\"assistant\\\",\\\"content\\\":[{\\\"type\\\":\\\"output_text\\\",\\\"text\\\":\\\"Hello from remote runtime sync json\\\",\\\"annotations\\\":[]}]}],\\\"usage\\\":{\\\"input_tokens\\\":1,\\\"output_tokens\\\":2,\\\"total_tokens\\\":3}}\"}}\n",
                        "{\"type\":\"telemetry\",\"payload\":{\"kind\":\"telemetry\",\"telemetry\":{\"elapsed_ms\":41}}}\n",
                        "{\"type\":\"eof\",\"payload\":{\"kind\":\"eof\"}}\n"
                    );
                    let mut response = axum::http::Response::new(Body::from(frames));
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/x-ndjson"),
                    );
                    response
                }),
            );
            axum::serve(listener, app)
                .await
                .expect("server should start");
        });

        let state = AppState::new()
            .expect("app state should build")
            .with_execution_runtime_override_base_url(format!("http://{addr}"));
        let plan = ExecutionPlan {
            request_id: "req-remote-runtime-sync-json-stream".into(),
            candidate_id: Some("cand-remote-runtime-sync-json-stream".into()),
            provider_name: Some("openai".into()),
            provider_id: "prov-1".into(),
            endpoint_id: "ep-1".into(),
            key_id: "key-1".into(),
            method: "POST".into(),
            url: "https://chatgpt.com/backend-api/codex/responses".into(),
            headers: BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
            ]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-5.4",
                "input": "hello",
                "stream": true
            })),
            stream: true,
            client_api_format: "openai:responses".into(),
            provider_api_format: "openai:responses".into(),
            model_name: Some("gpt-5.4".into()),
            proxy: None,
            transport_profile: None,
            timeouts: Some(ExecutionTimeouts {
                connect_ms: Some(5_000),
                total_ms: Some(5_000),
                ..ExecutionTimeouts::default()
            }),
        };
        let decision = GatewayControlDecision::synthetic(
            "/v1/responses",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("cli".to_string()),
            Some("openai:responses".to_string()),
        )
        .with_execution_runtime_candidate(true);

        let response = execute_execution_runtime_stream(
            &state,
            plan,
            "trace-remote-runtime-sync-json-stream",
            &decision,
            "openai_responses_stream",
            None,
            Some(json!({
                "provider_api_format": "openai:responses",
                "client_api_format": "openai:responses",
            })),
        )
        .await
        .expect("execution should succeed")
        .expect("execution should return a client response");

        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let text = String::from_utf8(body.to_vec()).expect("response body should be utf8");
        assert!(text.contains("event: response.output_text.delta"));
        assert!(text.contains("Hello from remote runtime sync json"));
        assert!(text.contains("event: response.completed"));

        server.abort();
    }

    #[tokio::test]
    async fn execute_execution_runtime_stream_rewrites_redirect_to_structured_failure() {
        let listener = crate::test_support::bind_loopback_listener()
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("local addr should resolve");
        let server = tokio::spawn(async move {
            let app = Router::new().route(
                "/v1/execute/stream",
                any(|_request: Request| async move {
                    let frames = concat!(
                        "{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":302,\"headers\":{\"location\":\"/\",\"content-type\":\"text/html\",\"content-length\":\"0\"}}}\n",
                        "{\"type\":\"eof\",\"payload\":{\"kind\":\"eof\"}}\n"
                    );
                    let mut response = axum::http::Response::new(Body::from(frames));
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/x-ndjson"),
                    );
                    response
                }),
            );
            axum::serve(listener, app)
                .await
                .expect("server should start");
        });

        let usage_repository = Arc::new(InMemoryUsageReadRepository::default());
        let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state = AppState::new()
            .expect("app state should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_request_candidate_and_usage_repository_for_tests(
                    Arc::clone(&request_candidate_repository),
                    Arc::clone(&usage_repository),
                ),
            )
            .with_usage_runtime_for_tests(UsageRuntimeConfig {
                enabled: true,
                ..UsageRuntimeConfig::default()
            })
            .with_execution_runtime_override_base_url(format!("http://{addr}"));
        let plan = ExecutionPlan {
            request_id: "req-remote-runtime-stream-redirect".into(),
            candidate_id: Some("cand-remote-runtime-stream-redirect".into()),
            provider_name: Some("ChatGPTWeb".into()),
            provider_id: "prov-redirect".into(),
            endpoint_id: "ep-redirect".into(),
            key_id: "key-redirect".into(),
            method: "POST".into(),
            url: "https://chatgpt.com/backend-api/codex/responses".into(),
            headers: BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
            ]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-5.4",
                "input": "hello",
                "stream": true
            })),
            stream: true,
            client_api_format: "gemini:generate_content".into(),
            provider_api_format: "openai:responses".into(),
            model_name: Some("gemini-3.1-flash-image-preview".into()),
            proxy: None,
            transport_profile: None,
            timeouts: Some(ExecutionTimeouts {
                connect_ms: Some(5_000),
                total_ms: Some(5_000),
                ..ExecutionTimeouts::default()
            }),
        };
        let decision = GatewayControlDecision::synthetic(
            "/v1beta/models/gemini-3.1-flash-image-preview:streamGenerateContent",
            Some("ai_public".to_string()),
            Some("gemini".to_string()),
            Some("generate_content".to_string()),
            Some("gemini:generate_content".to_string()),
        )
        .with_execution_runtime_candidate(true);

        let response = execute_execution_runtime_stream(
            &state,
            plan,
            "trace-remote-runtime-stream-redirect",
            &decision,
            "gemini_chat_stream",
            None,
            Some(json!({
                "request_id": "req-remote-runtime-stream-redirect",
                "candidate_id": "cand-remote-runtime-stream-redirect",
                "candidate_index": 0,
                "retry_index": 0,
                "provider_api_format": "openai:responses",
                "client_api_format": "gemini:generate_content",
                "needs_conversion": true
            })),
        )
        .await
        .expect("execution should succeed")
        .expect("execution should return a client response");

        assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            response
                .headers()
                .get("x-aether-upstream-status")
                .and_then(|value| value.to_str().ok()),
            Some("302")
        );
        assert!(
            response.headers().get(header::LOCATION).is_none(),
            "redirect location should not be forwarded to AI clients"
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let body_json: Value =
            serde_json::from_slice(&body).expect("response body should decode as json");
        assert_eq!(
            body_json["error"]["type"],
            json!("execution_runtime_non_success_status")
        );
        assert_eq!(body_json["error"]["upstream_status"], json!(302));
        assert_eq!(body_json["error"]["location"], json!("/"));
        assert!(body_json["error"]["message"]
            .as_str()
            .is_some_and(|value| value.contains("non-success status 302")));

        let usage = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(usage) = usage_repository
                    .find_by_request_id("trace-remote-runtime-stream-redirect")
                    .await
                    .expect("usage should read")
                    .filter(|usage| usage.status == "failed")
                {
                    break usage;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("usage should be written");

        assert_eq!(usage.status_code, Some(302));
        assert_eq!(usage.error_category.as_deref(), Some("redirect"));
        assert!(usage
            .error_message
            .as_deref()
            .is_some_and(|value| value.contains("non-success status 302")));
        assert_eq!(
            usage
                .client_response_headers
                .as_ref()
                .and_then(|headers| headers.get("x-aether-upstream-status")),
            Some(&json!("302"))
        );
        assert_eq!(
            usage
                .response_headers
                .as_ref()
                .and_then(|headers| headers.get("location")),
            Some(&json!("/"))
        );
        assert!(
            usage.response_body.is_none(),
            "upstream redirect did not include a body"
        );
        assert!(
            usage.client_response_body.is_none(),
            "basic usage capture stores response headers and status without keeping the client body"
        );
        flush_request_candidate_status_writes(&state).await;
        let candidates = request_candidate_repository
            .list_by_request_id("trace-remote-runtime-stream-redirect")
            .await
            .expect("candidate trace should read");
        let candidate_extra = candidates
            .first()
            .and_then(|candidate| candidate.extra_data.as_ref())
            .expect("failed candidate extra_data should exist");
        assert_eq!(
            candidate_extra["upstream_response"]["status_code"],
            json!(302)
        );
        assert_eq!(
            candidate_extra["upstream_response"]["headers"]["location"],
            json!("/")
        );
        assert!(candidate_extra["upstream_response"].get("body").is_none());
        assert!(candidate_extra.get("client_response").is_none());

        server.abort();
    }

    #[tokio::test]
    async fn execute_execution_runtime_stream_bridges_openai_image_sync_json_from_remote_runtime_to_image_sse(
    ) {
        let listener = crate::test_support::bind_loopback_listener()
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("local addr should resolve");
        let server = tokio::spawn(async move {
            let app = Router::new().route(
                "/v1/execute/stream",
                any(|_request: Request| async move {
                    let frames = concat!(
                        "{\"type\":\"headers\",\"payload\":{\"kind\":\"headers\",\"status_code\":200,\"headers\":{\"content-type\":\"application/json\"}}}\n",
                        "{\"type\":\"data\",\"payload\":{\"kind\":\"data\",\"text\":\"{\\\"created\\\":1776972364,\\\"data\\\":[{\\\"b64_json\\\":\\\"aGVsbG8=\\\"}],\\\"usage\\\":{\\\"total_tokens\\\":100,\\\"input_tokens\\\":50,\\\"output_tokens\\\":50,\\\"input_tokens_details\\\":{\\\"text_tokens\\\":10,\\\"image_tokens\\\":40}}}\"}}\n",
                        "{\"type\":\"telemetry\",\"payload\":{\"kind\":\"telemetry\",\"telemetry\":{\"elapsed_ms\":41}}}\n",
                        "{\"type\":\"eof\",\"payload\":{\"kind\":\"eof\"}}\n"
                    );
                    let mut response = axum::http::Response::new(Body::from(frames));
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/x-ndjson"),
                    );
                    response
                }),
            );
            axum::serve(listener, app)
                .await
                .expect("server should start");
        });

        let state = AppState::new()
            .expect("app state should build")
            .with_execution_runtime_override_base_url(format!("http://{addr}"));
        let plan = ExecutionPlan {
            request_id: "req-remote-runtime-image-sync-json-stream".into(),
            candidate_id: Some("cand-remote-runtime-image-sync-json-stream".into()),
            provider_name: Some("openai".into()),
            provider_id: "prov-1".into(),
            endpoint_id: "ep-1".into(),
            key_id: "key-1".into(),
            method: "POST".into(),
            url: "https://chatgpt.com/backend-api/codex/responses".into(),
            headers: BTreeMap::from([
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
            ]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({
                "model": "gpt-image-1",
                "prompt": "hello",
                "stream": true
            })),
            stream: true,
            client_api_format: "openai:image".into(),
            provider_api_format: "openai:image".into(),
            model_name: Some("gpt-image-1".into()),
            proxy: None,
            transport_profile: None,
            timeouts: Some(ExecutionTimeouts {
                connect_ms: Some(5_000),
                total_ms: Some(5_000),
                ..ExecutionTimeouts::default()
            }),
        };
        let decision = GatewayControlDecision::synthetic(
            "/v1/images/generations",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("image".to_string()),
            Some("openai:image".to_string()),
        )
        .with_execution_runtime_candidate(true);

        let response = execute_execution_runtime_stream(
            &state,
            plan,
            "trace-remote-runtime-image-sync-json-stream",
            &decision,
            "openai_image_stream",
            None,
            Some(json!({
                "provider_api_format": "openai:image",
                "client_api_format": "openai:image",
                "mapped_model": "gpt-image-1",
                "image_request": {
                    "operation": "generate"
                }
            })),
        )
        .await
        .expect("execution should succeed")
        .expect("execution should return a client response");

        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let text = String::from_utf8(body.to_vec()).expect("response body should be utf8");
        assert!(text.contains("event: image_generation.completed"));
        assert!(text.contains("\"type\":\"image_generation.completed\""));
        assert!(text.contains("\"b64_json\":\"aGVsbG8=\""));
        assert!(text.contains("\"total_tokens\":100"));

        server.abort();
    }

    #[tokio::test]
    async fn execute_execution_runtime_stream_returns_client_error_with_local_tunnel_message_before_first_data(
    ) {
        let state = AppState::new().expect("app state should build");
        let tunnel_app = state.tunnel.app_state();
        let (proxy_tx, mut proxy_rx) = aether_runtime::bounded_queue(8);
        let (proxy_close_tx, _) = watch::channel(false);
        tunnel_app.hub.register_proxy(Arc::new(TunnelProxyConn::new(
            901,
            "node-1".to_string(),
            "Node 1".to_string(),
            proxy_tx,
            proxy_close_tx,
            16,
            2,
        )));

        let plan = ExecutionPlan {
            request_id: "req-client-stream-error-1".into(),
            candidate_id: Some("cand-client-stream-error-1".into()),
            provider_name: Some("openai".into()),
            provider_id: "prov-1".into(),
            endpoint_id: "ep-1".into(),
            key_id: "key-1".into(),
            method: "POST".into(),
            url: "https://example.com/chat".into(),
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"stream": true})),
            stream: true,
            client_api_format: "openai:chat".into(),
            provider_api_format: "openai:chat".into(),
            model_name: Some("gpt-5".into()),
            proxy: Some(tunnel_proxy_snapshot("http://127.0.0.1:1".to_string())),
            transport_profile: None,
            timeouts: Some(ExecutionTimeouts {
                connect_ms: Some(5_000),
                total_ms: Some(5_000),
                ..ExecutionTimeouts::default()
            }),
        };
        let decision = test_decision();

        let state_for_task = state.clone();
        let plan_for_task = plan.clone();
        let decision_for_task = decision.clone();
        let execution_task = tokio::spawn(async move {
            execute_execution_runtime_stream(
                &state_for_task,
                plan_for_task,
                "trace-local-stream-client-error",
                &decision_for_task,
                "openai_chat_stream",
                None,
                Some(json!({
                    "client_api_format": "openai:chat",
                    "provider_api_format": "openai:chat",
                })),
            )
            .await
        });

        let request_headers = match proxy_rx.recv().await.expect("headers frame should arrive") {
            Message::Binary(data) => data,
            other => panic!("unexpected message: {other:?}"),
        };
        let request_header = tunnel_protocol::FrameHeader::parse(&request_headers)
            .expect("request header frame should parse");
        assert_eq!(request_header.msg_type, tunnel_protocol::REQUEST_HEADERS);

        let request_body = match proxy_rx.recv().await.expect("body frame should arrive") {
            Message::Binary(data) => data,
            other => panic!("unexpected message: {other:?}"),
        };
        let request_body_header = tunnel_protocol::FrameHeader::parse(&request_body)
            .expect("request body frame should parse");
        assert_eq!(request_body_header.msg_type, tunnel_protocol::REQUEST_BODY);

        let response_meta = tunnel_protocol::ResponseMeta {
            status: 200,
            // Use a non-SSE content type so direct finalize prefetch stays enabled and the
            // pre-body tunnel error is surfaced as a client-visible structured error response.
            headers: vec![("content-type".to_string(), "application/json".to_string())],
        };
        let response_payload =
            serde_json::to_vec(&response_meta).expect("response meta should serialize");
        let mut response_headers_frame = tunnel_protocol::encode_frame(
            request_header.stream_id,
            tunnel_protocol::RESPONSE_HEADERS,
            0,
            &response_payload,
        );
        tunnel_app
            .hub
            .handle_proxy_frame(901, &mut response_headers_frame)
            .await;

        let original_error = "proxy disconnected before first upstream event";
        let mut response_error_frame =
            tunnel_protocol::encode_stream_error(request_header.stream_id, original_error);
        tunnel_app
            .hub
            .handle_proxy_frame(901, &mut response_error_frame)
            .await;

        let response = execution_task
            .await
            .expect("execution task should complete")
            .expect("execution should succeed")
            .expect("execution should return a client response");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let body_json: Value =
            serde_json::from_slice(&body).expect("response body should decode as json");

        let error_message = body_json
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .expect("response body should contain error.message");

        assert_eq!(error_message, original_error);
        assert!(
            !error_message.contains("unexpected EOF during chunk size line"),
            "client-facing response should preserve the original local tunnel error"
        );
    }

    #[tokio::test]
    async fn execute_execution_runtime_stream_emits_terminal_sse_error_event_after_body_started() {
        let state = AppState::new().expect("app state should build");
        let tunnel_app = state.tunnel.app_state();
        let (proxy_tx, mut proxy_rx) = aether_runtime::bounded_queue(8);
        let (proxy_close_tx, _) = watch::channel(false);
        tunnel_app.hub.register_proxy(Arc::new(TunnelProxyConn::new(
            902,
            "node-1".to_string(),
            "Node 1".to_string(),
            proxy_tx,
            proxy_close_tx,
            16,
            2,
        )));

        let plan = ExecutionPlan {
            request_id: "req-client-stream-sse-error-1".into(),
            candidate_id: Some("cand-client-stream-sse-error-1".into()),
            provider_name: Some("openai".into()),
            provider_id: "prov-1".into(),
            endpoint_id: "ep-1".into(),
            key_id: "key-1".into(),
            method: "POST".into(),
            url: "https://example.com/chat".into(),
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            content_type: Some("application/json".into()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"stream": true})),
            stream: true,
            client_api_format: "openai:chat".into(),
            provider_api_format: "openai:chat".into(),
            model_name: Some("gpt-5".into()),
            proxy: Some(tunnel_proxy_snapshot("http://127.0.0.1:1".to_string())),
            transport_profile: None,
            timeouts: Some(ExecutionTimeouts {
                connect_ms: Some(5_000),
                total_ms: Some(5_000),
                ..ExecutionTimeouts::default()
            }),
        };
        let decision = test_decision();

        let state_for_task = state.clone();
        let plan_for_task = plan.clone();
        let decision_for_task = decision.clone();
        let execution_task = tokio::spawn(async move {
            execute_execution_runtime_stream(
                &state_for_task,
                plan_for_task,
                "trace-local-stream-sse-error",
                &decision_for_task,
                "openai_chat_stream",
                None,
                Some(json!({
                    "client_api_format": "openai:chat",
                    "provider_api_format": "openai:chat",
                })),
            )
            .await
        });

        let request_headers = match proxy_rx.recv().await.expect("headers frame should arrive") {
            Message::Binary(data) => data,
            other => panic!("unexpected message: {other:?}"),
        };
        let request_header = tunnel_protocol::FrameHeader::parse(&request_headers)
            .expect("request header frame should parse");
        assert_eq!(request_header.msg_type, tunnel_protocol::REQUEST_HEADERS);

        let request_body = match proxy_rx.recv().await.expect("body frame should arrive") {
            Message::Binary(data) => data,
            other => panic!("unexpected message: {other:?}"),
        };
        let request_body_header = tunnel_protocol::FrameHeader::parse(&request_body)
            .expect("request body frame should parse");
        assert_eq!(request_body_header.msg_type, tunnel_protocol::REQUEST_BODY);

        let response_meta = tunnel_protocol::ResponseMeta {
            status: 200,
            headers: vec![("content-type".to_string(), "text/event-stream".to_string())],
        };
        let response_payload =
            serde_json::to_vec(&response_meta).expect("response meta should serialize");
        let mut response_headers_frame = tunnel_protocol::encode_frame(
            request_header.stream_id,
            tunnel_protocol::RESPONSE_HEADERS,
            0,
            &response_payload,
        );
        tunnel_app
            .hub
            .handle_proxy_frame(902, &mut response_headers_frame)
            .await;

        let mut response_body_frame = tunnel_protocol::encode_frame(
            request_header.stream_id,
            tunnel_protocol::RESPONSE_BODY,
            0,
            b"data: hello\n\n",
        );
        tunnel_app
            .hub
            .handle_proxy_frame(902, &mut response_body_frame)
            .await;

        let response = execution_task
            .await
            .expect("execution task should complete")
            .expect("execution should succeed")
            .expect("execution should return a client response");
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );

        let body_task = tokio::spawn(async move {
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body should read");
            String::from_utf8(body.to_vec()).expect("response body should be utf8")
        });

        let original_error = "proxy disconnected while forwarding upstream body";
        let mut response_error_frame =
            tunnel_protocol::encode_stream_error(request_header.stream_id, original_error);
        tunnel_app
            .hub
            .handle_proxy_frame(902, &mut response_error_frame)
            .await;

        let body = body_task.await.expect("body task should complete");
        assert!(body.contains("data: hello\n\n"));
        assert!(body.contains("event: aether.error\n"));
        assert!(body.contains(original_error));
        assert!(
            !body.contains("unexpected EOF during chunk size line"),
            "same-format SSE path should surface the original terminal error event"
        );
    }
}

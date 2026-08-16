use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use aether_contracts::ExecutionPlan;
use aether_data_contracts::repository::candidates::{
    RequestCandidateStatus, StoredRequestCandidate,
};
use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use aether_usage_runtime::{
    build_usage_event_data_seed, UsageEvent, UsageEventData, UsageEventType,
};
use axum::body::Body;
use axum::body::Bytes;
use axum::http::{self, HeaderMap, Response};
use base64::Engine as _;
use serde_json::{json, Map, Value};
use tracing::warn;

use crate::constants::{
    EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS, LOCAL_EXECUTION_RUNTIME_MISS_REASON_HEADER,
    TRACE_ID_HEADER,
};
use crate::control::GatewayControlDecision;
use crate::headers::header_value_str;
use crate::state::LocalExecutionRuntimeMissDiagnostic;
use crate::AppState;

#[derive(Debug)]
pub(crate) enum LocalExecutionRequestOutcome {
    Responded(Response<Body>),
    Exhausted(LocalExecutionExhaustion),
    NoPath,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalExecutionExhaustion {
    request_id: String,
    data: UsageEventData,
    candidate_id: Option<String>,
    candidate_index: Option<u32>,
    upstream_status_code: Option<u16>,
    upstream_error_type: Option<String>,
    upstream_error_message: Option<String>,
    upstream_error_response: Option<LocalExecutionUpstreamErrorResponse>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalExecutionUpstreamErrorResponse {
    pub(crate) status_code: u16,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Value,
    pub(crate) body_bytes: Vec<u8>,
    pub(crate) error_type: Option<String>,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LocalExecutionRuntimeMissContext {
    pub(crate) auth_user_id: Option<String>,
    pub(crate) auth_api_key_id: Option<String>,
    pub(crate) auth_username: Option<String>,
    pub(crate) auth_api_key_name: Option<String>,
    candidate_contexts: Vec<RuntimeMissCandidateContext>,
}

#[derive(Debug, Clone)]
struct RuntimeMissCandidateContext {
    candidate: StoredRequestCandidate,
    provider_name: Option<String>,
    key_name: Option<String>,
    client_api_format: Option<String>,
    provider_api_format: Option<String>,
    global_model_name: Option<String>,
    selected_provider_model_name: Option<String>,
    endpoint_url: Option<String>,
}

impl LocalExecutionRequestOutcome {
    pub(crate) fn responded(response: Response<Body>) -> Self {
        Self::Responded(response)
    }
}

impl LocalExecutionExhaustion {
    pub(crate) fn upstream_error_response(&self) -> Option<LocalExecutionUpstreamErrorResponse> {
        self.upstream_error_response.clone()
    }
}

impl LocalExecutionRuntimeMissContext {
    pub(crate) fn persisted_candidate_count(&self) -> usize {
        self.candidate_contexts.len()
    }

    pub(crate) fn upstream_error_response(&self) -> Option<LocalExecutionUpstreamErrorResponse> {
        select_last_runtime_miss_executed_candidate(&self.candidate_contexts).and_then(|value| {
            local_execution_upstream_error_response_from_candidate(&value.candidate)
        })
    }

    pub(crate) fn all_candidates_skipped_for_reason(&self, reason: &str) -> bool {
        let reason = reason.trim();
        if reason.is_empty() || self.candidate_contexts.is_empty() {
            return false;
        }

        self.candidate_contexts.iter().all(|candidate| {
            candidate.candidate.status == RequestCandidateStatus::Skipped
                && candidate
                    .candidate
                    .skip_reason
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| value == reason)
        })
    }

    pub(crate) fn candidate_summary(&self) -> Option<String> {
        const MAX_ITEMS: usize = 5;

        if self.candidate_contexts.is_empty() {
            return None;
        }

        let mut summaries = self
            .candidate_contexts
            .iter()
            .take(MAX_ITEMS)
            .map(format_runtime_miss_candidate_summary)
            .collect::<Vec<_>>();
        let remaining = self.candidate_contexts.len().saturating_sub(MAX_ITEMS);
        if remaining > 0 {
            summaries.push(format!("+{remaining} more"));
        }
        Some(summaries.join(" | "))
    }
}

pub(crate) async fn build_local_execution_exhaustion(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
) -> LocalExecutionExhaustion {
    let mut data = build_usage_event_data_seed(plan, report_context);
    let last_failed_candidate = match state
        .read_request_candidates_by_request_id(plan.request_id.as_str())
        .await
    {
        Ok(candidates) => select_last_failed_request_candidate(&candidates).cloned(),
        Err(err) => {
            warn!(
                request_id = %plan.request_id,
                error = ?err,
                "gateway failed to load request candidates for exhausted local execution"
            );
            None
        }
    };

    if let Some(candidate) = last_failed_candidate.as_ref() {
        data.user_id = data.user_id.or_else(|| candidate.user_id.clone());
        data.api_key_id = data.api_key_id.or_else(|| candidate.api_key_id.clone());
        data.username = data.username.or_else(|| candidate.username.clone());
        data.api_key_name = data.api_key_name.or_else(|| candidate.api_key_name.clone());
        data.provider_id = data.provider_id.or_else(|| candidate.provider_id.clone());
        data.provider_endpoint_id = data
            .provider_endpoint_id
            .or_else(|| candidate.endpoint_id.clone());
        data.provider_api_key_id = data
            .provider_api_key_id
            .or_else(|| candidate.key_id.clone());
    }

    let upstream_error_response = last_failed_candidate
        .as_ref()
        .and_then(local_execution_upstream_error_response_from_candidate);

    LocalExecutionExhaustion {
        request_id: plan.request_id.clone(),
        data,
        candidate_id: last_failed_candidate
            .as_ref()
            .map(|candidate| candidate.id.clone()),
        candidate_index: last_failed_candidate
            .as_ref()
            .map(|candidate| candidate.candidate_index),
        upstream_status_code: upstream_error_response
            .as_ref()
            .map(|value| value.status_code)
            .or_else(|| {
                last_failed_candidate
                    .as_ref()
                    .and_then(|candidate| candidate.status_code)
            }),
        upstream_error_type: last_failed_candidate
            .as_ref()
            .and_then(|candidate| {
                upstream_error_response
                    .as_ref()
                    .and_then(|value| value.error_type.clone())
                    .or_else(|| candidate.error_type.clone())
            })
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        upstream_error_message: last_failed_candidate
            .as_ref()
            .and_then(|candidate| {
                upstream_error_response
                    .as_ref()
                    .and_then(|value| value.message.clone())
                    .or_else(|| candidate.error_message.clone())
            })
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        upstream_error_response,
    }
}

pub(crate) async fn build_local_execution_runtime_miss_context(
    state: &AppState,
    request_id: &str,
    decision: Option<&GatewayControlDecision>,
) -> LocalExecutionRuntimeMissContext {
    let auth_context = decision.and_then(|value| value.auth_context.as_ref());

    LocalExecutionRuntimeMissContext {
        auth_user_id: auth_context.map(|value| value.user_id.clone()),
        auth_api_key_id: auth_context.map(|value| value.api_key_id.clone()),
        auth_username: auth_context.and_then(|value| value.username.clone()),
        auth_api_key_name: auth_context.and_then(|value| value.api_key_name.clone()),
        candidate_contexts: load_runtime_miss_candidate_contexts_with_retry(
            state, request_id, decision,
        )
        .await,
    }
}

pub(crate) async fn record_failed_usage_for_exhausted_request(
    state: &AppState,
    exhaustion: LocalExecutionExhaustion,
    started_at: &Instant,
    local_execution_runtime_miss_detail: &str,
    execution_path: &str,
    diagnostic: Option<&LocalExecutionRuntimeMissDiagnostic>,
) {
    if !state.usage_runtime.is_enabled() {
        return;
    }

    let LocalExecutionExhaustion {
        request_id,
        mut data,
        candidate_id,
        candidate_index,
        upstream_status_code,
        upstream_error_type,
        upstream_error_message,
        upstream_error_response,
    } = exhaustion;

    let status_code = upstream_error_response
        .as_ref()
        .map(|value| value.status_code)
        .unwrap_or_else(|| http::StatusCode::SERVICE_UNAVAILABLE.as_u16());
    let candidate_status_code = upstream_status_code.unwrap_or(status_code);
    data.status_code = Some(status_code);
    data.error_message = upstream_error_message
        .clone()
        .or_else(|| Some(local_execution_runtime_miss_detail.to_string()));
    data.error_category = error_category_for_failed_status(status_code);
    data.response_time_ms = Some(started_at.elapsed().as_millis() as u64);
    let response_body = upstream_error_response
        .as_ref()
        .map(|value| value.body.clone())
        .unwrap_or_else(|| {
            json!({
                "error": {
                    "type": upstream_error_type
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("upstream_error"),
                    "message": upstream_error_message
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or(local_execution_runtime_miss_detail),
                    "code": candidate_status_code,
                }
            })
        });
    data.response_headers = Some(
        upstream_error_response
            .as_ref()
            .map(|value| local_execution_header_map_json(&value.headers))
            .unwrap_or_else(json_header_map),
    );
    data.response_body = Some(response_body.clone());

    let mut client_headers = upstream_error_response
        .as_ref()
        .map(|value| {
            value
                .headers
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect::<Map<_, _>>()
        })
        .unwrap_or_else(|| {
            Map::from_iter([(
                "content-type".to_string(),
                Value::String("application/json".to_string()),
            )])
        });
    if let Some(reason) = diagnostic
        .map(|value| value.reason.trim())
        .filter(|value| !value.is_empty())
    {
        client_headers.insert(
            LOCAL_EXECUTION_RUNTIME_MISS_REASON_HEADER.to_string(),
            Value::String(reason.to_string()),
        );
    }
    data.client_response_headers = Some(Value::Object(client_headers));
    data.client_response_body = Some(
        upstream_error_response
            .as_ref()
            .map(|_| response_body)
            .unwrap_or_else(|| {
                json!({
                    "error": {
                        "type": "http_error",
                        "message": beautify_local_execution_client_error_message(local_execution_runtime_miss_detail),
                    }
                })
            }),
    );

    let mut request_metadata = match data.request_metadata.take() {
        Some(Value::Object(object)) => object,
        Some(other) => Map::from_iter([("seed".to_string(), other)]),
        None => Map::new(),
    };
    request_metadata
        .entry("trace_id".to_string())
        .or_insert_with(|| Value::String(request_id.clone()));
    apply_runtime_miss_usage_routing(
        &mut data,
        &mut request_metadata,
        execution_path,
        candidate_id.as_deref(),
        candidate_index,
        None,
        diagnostic,
        None,
        None,
    );
    data.request_metadata = Some(Value::Object(request_metadata));

    state.usage_runtime.submit_terminal_event(
        state.data.as_ref(),
        UsageEvent::new(UsageEventType::Failed, request_id, data),
    );
}

fn record_unauthenticated_ai_security_log(
    request_id: &str,
    decision: Option<&GatewayControlDecision>,
    diagnostic: Option<&LocalExecutionRuntimeMissDiagnostic>,
    request_headers: &HeaderMap,
    client_ip: Option<&str>,
    request_body: Option<&Bytes>,
    local_execution_runtime_miss_detail: &str,
) {
    let user_agent = header_value_str(request_headers, http::header::USER_AGENT.as_str())
        .unwrap_or_else(|| "-".to_string());
    let public_path = decision
        .map(GatewayControlDecision::proxy_path_and_query)
        .unwrap_or_else(|| "-".to_string());
    warn!(
        event_name = "unauthenticated_ai_request_blocked",
        log_type = "security",
        trace_id = %request_id,
        client_ip = client_ip.unwrap_or("-"),
        user_agent = user_agent.as_str(),
        route_family = decision
            .and_then(|value| value.route_family.as_deref())
            .unwrap_or("unknown"),
        route_kind = decision
            .and_then(|value| value.route_kind.as_deref())
            .unwrap_or("unknown"),
        api_format = decision
            .and_then(|value| value.auth_endpoint_signature.as_deref())
            .unwrap_or("unknown"),
        public_path = public_path.as_str(),
        requested_model = diagnostic
            .and_then(|value| value.requested_model.as_deref())
            .unwrap_or("unknown"),
        miss_reason = diagnostic
            .map(|value| value.reason.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown"),
        request_body_bytes = request_body.map(Bytes::len).unwrap_or_default(),
        detail = local_execution_runtime_miss_detail,
        "blocked unauthenticated AI request"
    );
}

pub(crate) async fn record_failed_usage_for_runtime_miss_request(
    state: &AppState,
    request_id: &str,
    started_at: &Instant,
    local_execution_runtime_miss_detail: &str,
    execution_path: &str,
    decision: Option<&GatewayControlDecision>,
    diagnostic: Option<&LocalExecutionRuntimeMissDiagnostic>,
    context: &LocalExecutionRuntimeMissContext,
    uri: &http::Uri,
    request_headers: &HeaderMap,
    client_ip: Option<&str>,
    request_body: Option<&Bytes>,
) {
    let unauthenticated_context = context
        .auth_user_id
        .as_deref()
        .unwrap_or_default()
        .is_empty()
        || context
            .auth_api_key_id
            .as_deref()
            .unwrap_or_default()
            .is_empty();
    let missing_auth_context = diagnostic
        .map(|value| value.reason.trim() == "missing_auth_context")
        .unwrap_or(false);
    if unauthenticated_context {
        record_unauthenticated_ai_security_log(
            request_id,
            decision,
            diagnostic,
            request_headers,
            client_ip,
            request_body,
            local_execution_runtime_miss_detail,
        );
    }

    if !state.usage_runtime.is_enabled() {
        return;
    }

    let selected_candidate =
        select_last_runtime_miss_executed_candidate(&context.candidate_contexts);
    let api_format = selected_candidate
        .and_then(|value| value.client_api_format.clone())
        .or_else(|| {
            trimmed_non_empty(decision.and_then(|value| value.auth_endpoint_signature.as_deref()))
        });
    let provider_api_format = selected_candidate
        .and_then(|value| value.provider_api_format.clone())
        .or_else(|| api_format.clone());
    let provider_name = selected_candidate
        .and_then(|value| value.provider_name.clone())
        .or_else(|| selected_candidate.and_then(|value| value.candidate.provider_id.clone()))
        .unwrap_or_else(|| {
            if unauthenticated_context {
                "Niffler 平台".to_string()
            } else {
                "unknown".to_string()
            }
        });
    let record_as_platform_rejection = unauthenticated_context || provider_name == "Niffler 平台";
    let empty_body = Bytes::new();
    let requested_model = decision.and_then(|value| {
        crate::control::extract_requested_model(
            value,
            uri,
            request_headers,
            request_body.unwrap_or(&empty_body),
        )
    });
    let model = trimmed_non_empty(diagnostic.and_then(|value| value.requested_model.as_deref()))
        .or(requested_model)
        .or_else(|| selected_candidate.and_then(|value| value.global_model_name.clone()))
        .or_else(|| selected_candidate.and_then(|value| value.selected_provider_model_name.clone()))
        .unwrap_or_else(|| "unknown".to_string());
    let target_model = selected_candidate
        .and_then(|value| value.selected_provider_model_name.clone())
        .filter(|value| !value.eq_ignore_ascii_case(model.as_str()));

    let upstream_error_response = selected_candidate
        .and_then(|value| local_execution_upstream_error_response_from_candidate(&value.candidate));
    let default_status_code = if missing_auth_context {
        http::StatusCode::UNAUTHORIZED.as_u16()
    } else {
        http::StatusCode::SERVICE_UNAVAILABLE.as_u16()
    };
    let status_code = upstream_error_response
        .as_ref()
        .map(|value| value.status_code)
        .unwrap_or(default_status_code);
    let client_body = upstream_error_response
        .as_ref()
        .map(|value| value.body.clone())
        .unwrap_or_else(|| {
            let client_message =
                beautify_local_execution_client_error_message(local_execution_runtime_miss_detail);
            let error_type = if missing_auth_context {
                "authentication_error"
            } else {
                "http_error"
            };
            json!({
                "error": {
                    "type": error_type,
                    "message": client_message,
                }
            })
        });
    let mut client_headers = upstream_error_response
        .as_ref()
        .map(|value| {
            value
                .headers
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect::<Map<_, _>>()
        })
        .unwrap_or_else(|| {
            Map::from_iter([(
                "content-type".to_string(),
                Value::String("application/json".to_string()),
            )])
        });
    if let Some(reason) = diagnostic
        .map(|value| value.reason.trim())
        .filter(|value| !value.is_empty())
    {
        client_headers.insert(
            LOCAL_EXECUTION_RUNTIME_MISS_REASON_HEADER.to_string(),
            Value::String(reason.to_string()),
        );
    }

    let mut request_metadata = Map::new();
    let trace_id = header_value_str(request_headers, TRACE_ID_HEADER)
        .unwrap_or_else(|| request_id.to_string());
    request_metadata.insert("trace_id".to_string(), Value::String(trace_id));
    if let Some(client_request_id) = header_value_str(request_headers, "x-request-id") {
        request_metadata.insert(
            "client_request_id".to_string(),
            Value::String(client_request_id),
        );
    }
    if record_as_platform_rejection {
        let reason = diagnostic
            .map(|value| value.reason.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("missing_auth_context");
        request_metadata.insert(
            "source".to_string(),
            Value::String("platform_rejection".to_string()),
        );
        request_metadata.insert(
            "platform_rejection_reason".to_string(),
            Value::String(reason.to_string()),
        );
        request_metadata.insert(
            "platform_reason".to_string(),
            Value::String(reason.to_string()),
        );
    }
    let mut data = UsageEventData {
        user_id: context.auth_user_id.clone(),
        api_key_id: context.auth_api_key_id.clone(),
        username: context.auth_username.clone(),
        api_key_name: context.auth_api_key_name.clone(),
        provider_name,
        model,
        target_model,
        provider_id: selected_candidate.and_then(|value| value.candidate.provider_id.clone()),
        provider_endpoint_id: selected_candidate
            .and_then(|value| value.candidate.endpoint_id.clone()),
        provider_api_key_id: selected_candidate.and_then(|value| value.candidate.key_id.clone()),
        request_type: Some(infer_request_type(api_format.as_deref())),
        api_format: api_format.clone(),
        api_family: api_format
            .as_deref()
            .and_then(infer_api_family)
            .map(ToOwned::to_owned),
        endpoint_kind: api_format
            .as_deref()
            .and_then(infer_endpoint_kind)
            .map(ToOwned::to_owned),
        endpoint_api_format: provider_api_format.clone(),
        provider_api_family: provider_api_format
            .as_deref()
            .and_then(infer_api_family)
            .map(ToOwned::to_owned),
        provider_endpoint_kind: provider_api_format
            .as_deref()
            .and_then(infer_endpoint_kind)
            .map(ToOwned::to_owned),
        has_format_conversion: selected_candidate.and_then(|value| {
            value
                .client_api_format
                .as_deref()
                .zip(value.provider_api_format.as_deref())
                .map(|(left, right)| !left.eq_ignore_ascii_case(right))
        }),
        status_code: Some(status_code),
        error_message: upstream_error_response
            .as_ref()
            .and_then(|value| value.message.clone())
            .or_else(|| Some(local_execution_runtime_miss_detail.to_string())),
        error_category: error_category_for_failed_status(status_code),
        response_time_ms: Some(started_at.elapsed().as_millis() as u64),
        request_headers: Some(runtime_miss_original_headers_json(request_headers)),
        request_body: runtime_miss_original_request_body_json(request_headers, request_body),
        response_headers: Some(
            upstream_error_response
                .as_ref()
                .map(|value| local_execution_header_map_json(&value.headers))
                .unwrap_or_else(json_header_map),
        ),
        response_body: Some(client_body.clone()),
        client_response_headers: Some(Value::Object(client_headers)),
        client_response_body: Some(client_body),
        ..UsageEventData::default()
    };
    apply_runtime_miss_usage_routing(
        &mut data,
        &mut request_metadata,
        execution_path,
        selected_candidate.map(|value| value.candidate.id.as_str()),
        selected_candidate.map(|value| value.candidate.candidate_index),
        selected_candidate.and_then(|value| value.key_name.as_deref()),
        diagnostic,
        decision.and_then(|value| value.route_family.as_deref()),
        decision.and_then(|value| value.route_kind.as_deref()),
    );
    data.request_metadata =
        (!request_metadata.is_empty()).then_some(Value::Object(request_metadata));

    state.usage_runtime.submit_terminal_event(
        state.data.as_ref(),
        UsageEvent::new(UsageEventType::Failed, request_id, data),
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn record_platform_rejection_usage(
    state: &AppState,
    request_id: &str,
    started_at: &Instant,
    decision: Option<&GatewayControlDecision>,
    uri: &http::Uri,
    headers: &HeaderMap,
    request_body: Option<&Bytes>,
    status_code: u16,
    reason: &str,
    message: &str,
    execution_path: &str,
) {
    record_platform_usage(
        state,
        request_id,
        started_at,
        decision,
        uri,
        headers,
        request_body,
        status_code,
        UsageEventType::Failed,
        "platform_rejection",
        reason,
        message,
        execution_path,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn record_platform_handled_usage(
    state: &AppState,
    request_id: &str,
    started_at: &Instant,
    decision: Option<&GatewayControlDecision>,
    uri: &http::Uri,
    headers: &HeaderMap,
    request_body: Option<&Bytes>,
    status_code: u16,
    reason: &str,
    message: &str,
    execution_path: &str,
) {
    let event_type = if status_code >= 400 {
        UsageEventType::Failed
    } else {
        UsageEventType::Completed
    };
    let source = if matches!(event_type, UsageEventType::Failed) {
        "platform_rejection"
    } else {
        "platform_handled"
    };
    record_platform_usage(
        state,
        request_id,
        started_at,
        decision,
        uri,
        headers,
        request_body,
        status_code,
        event_type,
        source,
        reason,
        message,
        execution_path,
    )
    .await;
}

pub(crate) async fn record_platform_pending_usage(
    state: &AppState,
    request_id: &str,
    decision: Option<&GatewayControlDecision>,
    uri: &http::Uri,
    headers: &HeaderMap,
    request_body: Option<&Bytes>,
) -> Result<(), crate::GatewayError> {
    if !state.usage_runtime.is_enabled() {
        return Ok(());
    }
    let Some(decision) = decision else {
        return Ok(());
    };
    if decision.route_class.as_deref() != Some("ai_public") {
        return Ok(());
    }

    let auth_context = decision.auth_context.as_ref();
    let api_format = trimmed_non_empty(decision.auth_endpoint_signature.as_deref());
    let requested_model = crate::control::extract_requested_model(
        decision,
        uri,
        headers,
        request_body.unwrap_or(&Bytes::new()),
    );
    let model = requested_model
        .or_else(|| trimmed_non_empty(decision.route_kind.as_deref()))
        .unwrap_or_else(|| "unknown".to_string());
    let mut request_metadata = Map::new();
    request_metadata.insert(
        "source".to_string(),
        Value::String("platform_pending".to_string()),
    );
    let trace_id =
        header_value_str(headers, TRACE_ID_HEADER).unwrap_or_else(|| request_id.to_string());
    request_metadata.insert("trace_id".to_string(), Value::String(trace_id));
    if let Some(client_request_id) = header_value_str(headers, "x-request-id") {
        request_metadata.insert(
            "client_request_id".to_string(),
            Value::String(client_request_id),
        );
    }
    request_metadata.insert(
        "request_path".to_string(),
        Value::String(
            uri.path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/")
                .to_string(),
        ),
    );

    let event = UsageEvent::new(
        UsageEventType::Pending,
        request_id,
        UsageEventData {
            user_id: auth_context.map(|value| value.user_id.clone()),
            api_key_id: auth_context.map(|value| value.api_key_id.clone()),
            username: auth_context.and_then(|value| value.username.clone()),
            api_key_name: auth_context.and_then(|value| value.api_key_name.clone()),
            provider_name: "Niffler 平台".to_string(),
            model,
            request_type: Some(infer_request_type(api_format.as_deref())),
            api_format: api_format.clone(),
            api_family: api_format
                .as_deref()
                .and_then(infer_api_family)
                .map(ToOwned::to_owned),
            endpoint_kind: api_format
                .as_deref()
                .and_then(infer_endpoint_kind)
                .map(ToOwned::to_owned),
            endpoint_api_format: api_format.clone(),
            provider_api_family: api_format
                .as_deref()
                .and_then(infer_api_family)
                .map(ToOwned::to_owned),
            provider_endpoint_kind: api_format
                .as_deref()
                .and_then(infer_endpoint_kind)
                .map(ToOwned::to_owned),
            has_format_conversion: Some(false),
            request_metadata: Some(Value::Object(request_metadata)),
            ..UsageEventData::default()
        },
    );
    state
        .usage_runtime
        .record_pending_event_direct(state.data.as_ref(), event)
        .await
        .map_err(|error| crate::GatewayError::Internal(format!("创建请求使用记录失败: {error}")))
}

#[allow(clippy::too_many_arguments)]
async fn record_platform_usage(
    state: &AppState,
    request_id: &str,
    started_at: &Instant,
    decision: Option<&GatewayControlDecision>,
    uri: &http::Uri,
    headers: &HeaderMap,
    request_body: Option<&Bytes>,
    status_code: u16,
    event_type: UsageEventType,
    source: &str,
    reason: &str,
    message: &str,
    execution_path: &str,
) {
    if !state.usage_runtime.is_enabled() {
        return;
    }
    let Some(decision) = decision else {
        return;
    };
    if decision.route_class.as_deref() != Some("ai_public") {
        return;
    }

    let auth_context = decision.auth_context.as_ref();
    let api_format = trimmed_non_empty(decision.auth_endpoint_signature.as_deref());
    let requested_model = crate::control::extract_requested_model(
        decision,
        uri,
        headers,
        request_body.unwrap_or(&Bytes::new()),
    );
    let model = requested_model
        .or_else(|| trimmed_non_empty(decision.route_kind.as_deref()))
        .unwrap_or_else(|| "unknown".to_string());
    let failed = matches!(event_type, UsageEventType::Failed);
    let client_body = if failed {
        json!({
            "error": {
                "type": "http_error",
                "message": message,
            }
        })
    } else {
        json!({
            "message": message,
        })
    };
    let mut request_metadata = Map::new();
    request_metadata.insert("source".to_string(), Value::String(source.to_string()));
    let trace_id =
        header_value_str(headers, TRACE_ID_HEADER).unwrap_or_else(|| request_id.to_string());
    request_metadata.insert("trace_id".to_string(), Value::String(trace_id));
    if let Some(client_request_id) = header_value_str(headers, "x-request-id") {
        request_metadata.insert(
            "client_request_id".to_string(),
            Value::String(client_request_id),
        );
    }
    request_metadata.insert(
        "platform_rejection_reason".to_string(),
        Value::String(reason.to_string()),
    );
    request_metadata.insert(
        "platform_reason".to_string(),
        Value::String(reason.to_string()),
    );
    request_metadata.insert(
        "request_path".to_string(),
        Value::String(
            uri.path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/")
                .to_string(),
        ),
    );

    let data = UsageEventData {
        user_id: auth_context.map(|value| value.user_id.clone()),
        api_key_id: auth_context.map(|value| value.api_key_id.clone()),
        username: auth_context.and_then(|value| value.username.clone()),
        api_key_name: auth_context.and_then(|value| value.api_key_name.clone()),
        provider_name: "Niffler 平台".to_string(),
        model,
        request_type: Some(infer_request_type(api_format.as_deref())),
        api_format: api_format.clone(),
        api_family: api_format
            .as_deref()
            .and_then(infer_api_family)
            .map(ToOwned::to_owned),
        endpoint_kind: api_format
            .as_deref()
            .and_then(infer_endpoint_kind)
            .map(ToOwned::to_owned),
        endpoint_api_format: api_format.clone(),
        provider_api_family: api_format
            .as_deref()
            .and_then(infer_api_family)
            .map(ToOwned::to_owned),
        provider_endpoint_kind: api_format
            .as_deref()
            .and_then(infer_endpoint_kind)
            .map(ToOwned::to_owned),
        has_format_conversion: Some(false),
        status_code: Some(status_code),
        error_message: failed.then(|| message.to_string()),
        error_category: failed
            .then(|| error_category_for_failed_status(status_code))
            .flatten(),
        response_time_ms: Some(started_at.elapsed().as_millis() as u64),
        request_headers: Some(runtime_miss_original_headers_json(headers)),
        request_body: runtime_miss_original_request_body_json(headers, request_body),
        response_headers: Some(json_header_map()),
        response_body: Some(client_body.clone()),
        client_response_headers: Some(json_header_map()),
        client_response_body: Some(client_body),
        execution_path: Some(execution_path.to_string()),
        local_execution_runtime_miss_reason: failed.then(|| reason.to_string()),
        request_metadata: Some(Value::Object(request_metadata)),
        billing_status_override: Some("void".to_string()),
        ..UsageEventData::default()
    };

    state
        .usage_runtime
        .record_terminal_event_direct(
            state.data.as_ref(),
            UsageEvent::new(event_type, request_id, data),
        )
        .await;
}

pub(crate) fn beautify_local_execution_client_error_message(message: &str) -> String {
    let without_reason_code = strip_parenthesized_reason_code(message);
    let mut simplified = collapse_whitespace(without_reason_code.as_str());
    if let Some(unavailable_message) =
        simplify_all_candidates_skipped_client_error_message(simplified.as_str())
    {
        return unavailable_message;
    }
    for marker in [
        "。请检查",
        "。请确认",
        ". 请检查",
        ". 请确认",
        "! 请检查",
        "! 请确认",
        "? 请检查",
        "? 请确认",
        "。Reason",
        ". Reason",
        "。Code",
        ". Code",
    ] {
        if let Some(index) = simplified.find(marker) {
            simplified.truncate(index);
            break;
        }
    }
    trim_trailing_message_punctuation(simplified.as_str()).to_string()
}

fn simplify_all_candidates_skipped_client_error_message(message: &str) -> Option<String> {
    if !(message.contains("候选提供商") || message.contains("可尝试提供商"))
        || !(message.contains("全部不可用") || message.contains("都不满足本次"))
    {
        return None;
    }

    let request_mode = extract_local_execution_request_mode(message)?;
    if let Some(model) = extract_candidate_supported_model(message) {
        return Some(format!(
            "没有可用提供商支持模型 {model} 的{request_mode}请求"
        ));
    }

    Some(format!("没有可用提供商支持本次{request_mode}请求"))
}

fn extract_local_execution_request_mode(message: &str) -> Option<&str> {
    let rest = message.get(message.find("本次")? + "本次".len()..)?;
    let mode = rest.get(..rest.find("请求")?)?.trim();
    (!mode.is_empty()).then_some(mode)
}

fn extract_candidate_supported_model(message: &str) -> Option<&str> {
    let rest = message.get(message.find("支持模型 ")? + "支持模型 ".len()..)?;
    let model = rest.get(..rest.find(" 的")?)?.trim();
    (!model.is_empty()).then_some(model)
}

fn strip_parenthesized_reason_code(message: &str) -> String {
    let Some(reason_index) = message.find("原因代码") else {
        return message.to_string();
    };
    let Some((start, open)) = message[..reason_index]
        .char_indices()
        .rev()
        .find(|(_, ch)| *ch == '（' || *ch == '(')
    else {
        return message.to_string();
    };
    let close = if open == '(' { ')' } else { '）' };
    let Some(close_offset) = message[start..].find(close) else {
        return message[..start].to_string();
    };
    let end = start + close_offset + close.len_utf8();
    format!("{}{}", &message[..start], &message[end..])
}

fn collapse_whitespace(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn trim_trailing_message_punctuation(message: &str) -> &str {
    message
        .trim_end_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '。' | '.' | '!' | '?' | '；' | ';' | '，' | ',' | '：' | ':'
                )
        })
        .trim()
}

fn select_last_failed_request_candidate(
    candidates: &[StoredRequestCandidate],
) -> Option<&StoredRequestCandidate> {
    candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.status,
                RequestCandidateStatus::Failed | RequestCandidateStatus::Cancelled
            )
        })
        .max_by_key(|candidate| {
            (
                candidate.retry_index,
                candidate.candidate_index,
                candidate
                    .finished_at_unix_ms
                    .or(candidate.started_at_unix_ms)
                    .unwrap_or(candidate.created_at_unix_ms),
            )
        })
}

fn select_last_runtime_miss_executed_candidate(
    candidates: &[RuntimeMissCandidateContext],
) -> Option<&RuntimeMissCandidateContext> {
    candidates
        .iter()
        .filter(|candidate| request_candidate_represents_provider_execution(&candidate.candidate))
        .max_by_key(|candidate| {
            (
                candidate.candidate.retry_index,
                candidate.candidate.candidate_index,
                candidate
                    .candidate
                    .finished_at_unix_ms
                    .or(candidate.candidate.started_at_unix_ms)
                    .unwrap_or(candidate.candidate.created_at_unix_ms),
            )
        })
}

fn request_candidate_represents_provider_execution(candidate: &StoredRequestCandidate) -> bool {
    matches!(
        candidate.status,
        RequestCandidateStatus::Pending
            | RequestCandidateStatus::Streaming
            | RequestCandidateStatus::Success
            | RequestCandidateStatus::Failed
            | RequestCandidateStatus::Cancelled
    )
}

fn error_category_for_failed_status(status_code: u16) -> Option<String> {
    if status_code >= 500 {
        Some("server_error".to_string())
    } else if status_code >= 400 {
        Some("client_error".to_string())
    } else {
        None
    }
}

fn json_header_map() -> Value {
    Value::Object(Map::from_iter([(
        "content-type".to_string(),
        Value::String("application/json".to_string()),
    )]))
}

fn local_execution_header_map_json(headers: &BTreeMap<String, String>) -> Value {
    Value::Object(
        headers
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect(),
    )
}

fn local_execution_upstream_error_response_from_candidate(
    candidate: &StoredRequestCandidate,
) -> Option<LocalExecutionUpstreamErrorResponse> {
    let upstream_response = candidate.extra_data.as_ref()?.get("upstream_response")?;
    let status_code = upstream_response
        .get("status_code")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .or(candidate.status_code)?;
    if status_code < 400 {
        return None;
    }

    let body = upstream_response.get("body")?.clone();
    let body_bytes = local_execution_upstream_body_bytes(&body)?;
    let message = local_execution_upstream_error_message_from_body(&body)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if message.is_none() && body.is_null() {
        return None;
    }
    let error_type = local_execution_upstream_error_type_from_body(&body)
        .or_else(|| candidate.error_type.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let headers =
        local_execution_upstream_headers_from_value(upstream_response.get("headers"), &body);

    Some(LocalExecutionUpstreamErrorResponse {
        status_code,
        headers,
        body,
        body_bytes,
        error_type,
        message,
    })
}

fn local_execution_upstream_headers_from_value(
    headers: Option<&Value>,
    body: &Value,
) -> BTreeMap<String, String> {
    let mut out = headers
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    let value = value
                        .as_str()
                        .map(ToOwned::to_owned)
                        .or_else(|| (!value.is_null()).then(|| value.to_string()))?;
                    (!value.trim().is_empty()).then(|| (key.to_ascii_lowercase(), value))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    out.remove("content-length");
    out.remove("content-encoding");

    if !out
        .keys()
        .any(|key| key.eq_ignore_ascii_case("content-type"))
    {
        let content_type = if matches!(body, Value::String(_)) {
            "text/plain; charset=utf-8"
        } else {
            "application/json"
        };
        out.insert("content-type".to_string(), content_type.to_string());
    }

    out
}

fn local_execution_upstream_body_bytes(body: &Value) -> Option<Vec<u8>> {
    match body {
        Value::Null => None,
        Value::String(text) => Some(text.as_bytes().to_vec()),
        Value::Object(object) => {
            let is_base64 = object
                .get("encoding")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case("base64"));
            if is_base64 {
                if let Some(data) = object.get("data").and_then(Value::as_str) {
                    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(data) {
                        return Some(decoded);
                    }
                }
            }
            serde_json::to_vec(body).ok()
        }
        _ => serde_json::to_vec(body).ok(),
    }
}

fn local_execution_upstream_error_message_from_body(body: &Value) -> Option<String> {
    if let Value::String(message) = body {
        return Some(message.trim().to_string()).filter(|value| !value.is_empty());
    }

    body.get("error")
        .and_then(|error| match error {
            Value::Object(object) => object.get("message").and_then(Value::as_str),
            Value::String(message) => Some(message.as_str()),
            _ => None,
        })
        .or_else(|| body.get("message").and_then(Value::as_str))
        .or_else(|| body.get("detail").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn local_execution_upstream_error_type_from_body(body: &Value) -> Option<String> {
    body.get("error")
        .and_then(|error| match error {
            Value::Object(object) => object
                .get("type")
                .or_else(|| object.get("status"))
                .or_else(|| object.get("kind"))
                .and_then(Value::as_str),
            _ => None,
        })
        .or_else(|| body.get("type").and_then(Value::as_str))
        .or_else(|| body.get("status").and_then(Value::as_str))
        .or_else(|| body.get("kind").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn runtime_miss_original_headers_json(headers: &HeaderMap) -> Value {
    let mut headers = crate::headers::collect_control_headers(headers);
    for (name, value) in headers.iter_mut() {
        if runtime_miss_sensitive_header(name) {
            *value = runtime_miss_mask_header_value(value);
        }
    }
    serde_json::to_value(headers).unwrap_or_else(|_| json!({}))
}

fn runtime_miss_original_request_body_json(
    headers: &HeaderMap,
    body: Option<&Bytes>,
) -> Option<Value> {
    let body = body?;
    if crate::headers::is_json_request(headers) {
        if body.is_empty() {
            return Some(json!({}));
        }
        return serde_json::from_slice::<Value>(body.as_ref()).ok();
    }

    (!body.is_empty()).then(|| {
        json!({
            "body_bytes_b64": base64::engine::general_purpose::STANDARD.encode(body.as_ref())
        })
    })
}

fn runtime_miss_sensitive_header(name: &str) -> bool {
    const SENSITIVE_HEADERS: &[&str] = &[
        "authorization",
        "x-api-key",
        "api-key",
        "x-goog-api-key",
        "cookie",
        "proxy-authorization",
    ];

    SENSITIVE_HEADERS
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

fn runtime_miss_mask_header_value(value: &str) -> String {
    let value = value.trim();
    let char_count = value.chars().count();
    if char_count <= 8 {
        return "****".to_string();
    }

    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}****{suffix}")
}

async fn load_runtime_miss_candidate_contexts(
    state: &AppState,
    request_id: &str,
    decision: Option<&GatewayControlDecision>,
) -> Vec<RuntimeMissCandidateContext> {
    let mut candidates = match state
        .read_request_candidates_by_request_id(request_id)
        .await
    {
        Ok(candidates) => candidates,
        Err(err) => {
            warn!(
                request_id = %request_id,
                error = ?err,
                "gateway failed to load request candidates for local execution runtime miss"
            );
            return Vec::new();
        }
    };
    if candidates.is_empty() {
        return Vec::new();
    }

    candidates.sort_by_key(|candidate| {
        (
            candidate.candidate_index,
            candidate.retry_index,
            candidate.created_at_unix_ms,
        )
    });

    let (providers_by_id, endpoints_by_id, keys_by_id) = if state.has_provider_catalog_data_reader()
    {
        let provider_ids = collect_present_ids(
            candidates
                .iter()
                .filter_map(|value| value.provider_id.as_deref()),
        );
        let endpoint_ids = collect_present_ids(
            candidates
                .iter()
                .filter_map(|value| value.endpoint_id.as_deref()),
        );
        let key_ids = collect_present_ids(
            candidates
                .iter()
                .filter_map(|value| value.key_id.as_deref()),
        );
        let (providers_result, endpoints_result, keys_result) = tokio::join!(
            state.read_provider_catalog_providers_by_ids(&provider_ids),
            state.read_provider_catalog_endpoints_by_ids(&endpoint_ids),
            state.read_provider_catalog_keys_by_ids(&key_ids),
        );
        (
            match providers_result {
                Ok(values) => values
                    .into_iter()
                    .map(|value| (value.id.clone(), value))
                    .collect::<BTreeMap<_, _>>(),
                Err(err) => {
                    warn!(
                        request_id = %request_id,
                        error = ?err,
                        "gateway failed to load provider catalog providers for local execution runtime miss"
                    );
                    BTreeMap::new()
                }
            },
            match endpoints_result {
                Ok(values) => values
                    .into_iter()
                    .map(|value| (value.id.clone(), value))
                    .collect::<BTreeMap<_, _>>(),
                Err(err) => {
                    warn!(
                        request_id = %request_id,
                        error = ?err,
                        "gateway failed to load provider catalog endpoints for local execution runtime miss"
                    );
                    BTreeMap::new()
                }
            },
            match keys_result {
                Ok(values) => values
                    .into_iter()
                    .map(|value| (value.id.clone(), value))
                    .collect::<BTreeMap<_, _>>(),
                Err(err) => {
                    warn!(
                        request_id = %request_id,
                        error = ?err,
                        "gateway failed to load provider catalog keys for local execution runtime miss"
                    );
                    BTreeMap::new()
                }
            },
        )
    } else {
        (BTreeMap::new(), BTreeMap::new(), BTreeMap::new())
    };

    candidates
        .into_iter()
        .map(|candidate| {
            let provider = candidate
                .provider_id
                .as_deref()
                .and_then(|value| providers_by_id.get(value));
            let endpoint = candidate
                .endpoint_id
                .as_deref()
                .and_then(|value| endpoints_by_id.get(value));
            let key = candidate
                .key_id
                .as_deref()
                .and_then(|value| keys_by_id.get(value));
            RuntimeMissCandidateContext {
                provider_name: candidate_extra_data_string(&candidate, "provider_name")
                    .or_else(|| provider.map(|value| value.name.clone())),
                key_name: candidate_extra_data_string(&candidate, "key_name")
                    .or_else(|| key.map(|value| value.name.clone())),
                client_api_format: candidate_extra_data_string(&candidate, "client_api_format")
                    .or_else(|| candidate_extra_data_string(&candidate, "client_contract")),
                provider_api_format: candidate_extra_data_string(&candidate, "provider_api_format")
                    .or_else(|| candidate_extra_data_string(&candidate, "provider_contract"))
                    .or_else(|| endpoint.map(|value| value.api_format.clone())),
                global_model_name: candidate_extra_data_string(&candidate, "global_model_name"),
                selected_provider_model_name: candidate_extra_data_string(
                    &candidate,
                    "selected_provider_model_name",
                ),
                endpoint_url: endpoint.and_then(|value| {
                    build_runtime_miss_candidate_endpoint_url(&candidate, value, decision)
                }),
                candidate,
            }
        })
        .collect()
}

async fn load_runtime_miss_candidate_contexts_with_retry(
    state: &AppState,
    request_id: &str,
    decision: Option<&GatewayControlDecision>,
) -> Vec<RuntimeMissCandidateContext> {
    let mut contexts = load_runtime_miss_candidate_contexts(state, request_id, decision).await;
    if !contexts.is_empty() {
        return contexts;
    }

    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        contexts = load_runtime_miss_candidate_contexts(state, request_id, decision).await;
        if !contexts.is_empty() {
            break;
        }
    }

    contexts
}

fn collect_present_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<String> {
    ids.filter_map(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed.to_string())
    })
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect()
}

fn candidate_extra_data_string(candidate: &StoredRequestCandidate, key: &str) -> Option<String> {
    candidate
        .extra_data
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn build_runtime_miss_candidate_endpoint_url(
    candidate: &StoredRequestCandidate,
    endpoint: &StoredProviderCatalogEndpoint,
    decision: Option<&GatewayControlDecision>,
) -> Option<String> {
    if let Some(upstream_url) = candidate_extra_data_string(candidate, "upstream_url") {
        return Some(upstream_url);
    }

    let path = endpoint
        .custom_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            decision
                .map(|value| value.public_path.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        });
    let query = decision
        .and_then(|value| value.public_query_string.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());

    path.and_then(|value| {
        crate::provider_transport::url::build_passthrough_path_url(
            &endpoint.base_url,
            value,
            query,
            &[],
        )
    })
    .or_else(|| trimmed_non_empty(Some(endpoint.base_url.as_str())))
}

fn format_runtime_miss_candidate_summary(candidate: &RuntimeMissCandidateContext) -> String {
    let mut parts = Vec::new();
    parts.push(format!("idx={}", candidate.candidate.candidate_index));
    parts.push(format!("retry={}", candidate.candidate.retry_index));
    parts.push(format!(
        "status={}",
        request_candidate_status_label(candidate.candidate.status)
    ));
    if let Some(provider_label) = format_name_with_id(
        candidate.provider_name.as_deref(),
        candidate.candidate.provider_id.as_deref(),
    ) {
        parts.push(format!("provider={provider_label}"));
    }
    if let Some(endpoint_id) = candidate
        .candidate
        .endpoint_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("endpoint={endpoint_id}"));
    }
    if let Some(endpoint_url) = candidate
        .endpoint_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("url={endpoint_url}"));
    }
    if let Some(key_label) = format_name_with_id(
        candidate.key_name.as_deref(),
        candidate.candidate.key_id.as_deref(),
    ) {
        parts.push(format!("key={key_label}"));
    }
    if let Some(skip_reason) = candidate
        .candidate
        .skip_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("skip={skip_reason}"));
    }
    if let Some(status_code) = candidate.candidate.status_code {
        parts.push(format!("code={status_code}"));
    }
    if let Some(error_type) = candidate
        .candidate
        .error_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("error_type={error_type}"));
    }
    parts.join(" ")
}

fn format_name_with_id(name: Option<&str>, id: Option<&str>) -> Option<String> {
    let name = name.map(str::trim).filter(|value| !value.is_empty());
    let id = id.map(str::trim).filter(|value| !value.is_empty());

    match (name, id) {
        (Some(name), Some(id)) => Some(format!("{name}({id})")),
        (Some(name), None) => Some(name.to_string()),
        (None, Some(id)) => Some(id.to_string()),
        (None, None) => None,
    }
}

fn request_candidate_status_label(status: RequestCandidateStatus) -> &'static str {
    match status {
        RequestCandidateStatus::Available => "available",
        RequestCandidateStatus::Unused => "unused",
        RequestCandidateStatus::Pending => "pending",
        RequestCandidateStatus::Streaming => "streaming",
        RequestCandidateStatus::Success => "success",
        RequestCandidateStatus::Failed => "failed",
        RequestCandidateStatus::Cancelled => "cancelled",
        RequestCandidateStatus::Skipped => "skipped",
    }
}

fn infer_request_type(api_format: Option<&str>) -> String {
    match infer_endpoint_kind(api_format.unwrap_or_default()) {
        Some("video") => "video".to_string(),
        Some("image") => "image".to_string(),
        _ => "chat".to_string(),
    }
}

fn infer_api_family(api_format: &str) -> Option<&str> {
    api_format.split_once(':').map(|(family, _)| family)
}

fn infer_endpoint_kind(api_format: &str) -> Option<&str> {
    api_format.split_once(':').map(|(_, kind)| kind)
}

fn apply_runtime_miss_usage_routing(
    data: &mut UsageEventData,
    request_metadata: &mut Map<String, Value>,
    execution_path: &str,
    candidate_id: Option<&str>,
    candidate_index: Option<u32>,
    key_name: Option<&str>,
    diagnostic: Option<&LocalExecutionRuntimeMissDiagnostic>,
    route_family_fallback: Option<&str>,
    route_kind_fallback: Option<&str>,
) {
    data.candidate_id = data
        .candidate_id
        .clone()
        .or_else(|| trimmed_non_empty(candidate_id));
    data.candidate_index = data
        .candidate_index
        .or_else(|| candidate_index.map(u64::from));
    data.key_name = data
        .key_name
        .clone()
        .or_else(|| trimmed_non_empty(key_name));
    data.execution_path = data
        .execution_path
        .clone()
        .or_else(|| trimmed_non_empty(Some(execution_path)));
    data.local_execution_runtime_miss_reason = data
        .local_execution_runtime_miss_reason
        .clone()
        .or_else(|| trimmed_non_empty(diagnostic.map(|value| value.reason.as_str())));
    data.route_family = data.route_family.clone().or_else(|| {
        trimmed_non_empty(
            diagnostic
                .and_then(|value| value.route_family.as_deref())
                .or(route_family_fallback),
        )
    });
    data.route_kind = data.route_kind.clone().or_else(|| {
        trimmed_non_empty(
            diagnostic
                .and_then(|value| value.route_kind.as_deref())
                .or(route_kind_fallback),
        )
    });
    data.planner_kind = data
        .planner_kind
        .clone()
        .or_else(|| trimmed_non_empty(diagnostic.and_then(|value| value.plan_kind.as_deref())));
    let _ = request_metadata;
}

fn trimmed_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_runtime_miss_usage_routing, beautify_local_execution_client_error_message,
        local_execution_upstream_error_response_from_candidate,
        request_candidate_represents_provider_execution,
        select_last_runtime_miss_executed_candidate, LocalExecutionRuntimeMissContext,
        RuntimeMissCandidateContext,
    };
    use crate::constants::EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS;
    use crate::state::LocalExecutionRuntimeMissDiagnostic;
    use aether_data_contracts::repository::candidates::{
        RequestCandidateStatus, StoredRequestCandidate,
    };
    use aether_usage_runtime::UsageEventData;
    use serde_json::{json, Map, Value};

    #[test]
    fn local_execution_client_error_message_is_client_friendly() {
        assert_eq!(
            beautify_local_execution_client_error_message(
                "没有可用提供商支持模型 gpt-5.4 的同步请求。请检查模型映射、端点启用状态和 API Key 权限（原因代码: candidate_list_empty）",
            ),
            "没有可用提供商支持模型 gpt-5.4 的同步请求"
        );
        assert_eq!(
            beautify_local_execution_client_error_message(
                "请求缺少 model 字段，无法选择上游提供商（openai/chat，原因代码: missing_requested_model）",
            ),
            "请求缺少 model 字段，无法选择上游提供商"
        );
        assert_eq!(
            beautify_local_execution_client_error_message(
                "找到 1 个支持模型 gpt-5.4 的候选提供商，但本次流式请求全部不可用：provider_quota_blocked 2 次（原因代码: all_candidates_skipped）",
            ),
            "没有可用提供商支持模型 gpt-5.4 的流式请求"
        );
    }

    #[test]
    fn runtime_miss_routing_moves_to_typed_usage_fields_and_keeps_metadata_lightweight() {
        let mut data = UsageEventData::default();
        let mut request_metadata =
            Map::from_iter([("trace_id".to_string(), Value::String("trace-1".to_string()))]);

        apply_runtime_miss_usage_routing(
            &mut data,
            &mut request_metadata,
            EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS,
            Some("cand-1"),
            Some(2),
            Some("primary"),
            Some(&LocalExecutionRuntimeMissDiagnostic {
                reason: "all_candidates_skipped".to_string(),
                route_family: Some("claude".to_string()),
                route_kind: Some("cli".to_string()),
                plan_kind: Some("claude_cli_sync".to_string()),
                ..LocalExecutionRuntimeMissDiagnostic::default()
            }),
            None,
            None,
        );

        assert_eq!(data.candidate_id.as_deref(), Some("cand-1"));
        assert_eq!(data.candidate_index, Some(2));
        assert_eq!(data.key_name.as_deref(), Some("primary"));
        assert_eq!(data.planner_kind.as_deref(), Some("claude_cli_sync"));
        assert_eq!(data.route_family.as_deref(), Some("claude"));
        assert_eq!(data.route_kind.as_deref(), Some("cli"));
        assert_eq!(
            data.execution_path.as_deref(),
            Some(EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS)
        );
        assert_eq!(
            data.local_execution_runtime_miss_reason.as_deref(),
            Some("all_candidates_skipped")
        );
        assert_eq!(
            Value::Object(request_metadata),
            json!({
                "trace_id": "trace-1"
            })
        );
    }

    #[test]
    fn runtime_miss_executed_candidate_selection_ignores_skipped_only_histories() {
        let skipped_candidate = StoredRequestCandidate::new(
            "cand-skipped".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            0,
            0,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            RequestCandidateStatus::Skipped,
            Some("api_key_concurrency_limit_reached".to_string()),
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            100,
            None,
            None,
        )
        .expect("candidate should build");

        assert!(!request_candidate_represents_provider_execution(
            &skipped_candidate
        ));

        let contexts = vec![RuntimeMissCandidateContext {
            candidate: skipped_candidate,
            provider_name: Some("openai".to_string()),
            key_name: Some("prod".to_string()),
            client_api_format: Some("openai:responses".to_string()),
            provider_api_format: Some("openai:responses".to_string()),
            global_model_name: Some("gpt-5".to_string()),
            selected_provider_model_name: Some("gpt-5-upstream".to_string()),
            endpoint_url: Some("https://api.openai.example/v1/responses".to_string()),
        }];

        assert!(select_last_runtime_miss_executed_candidate(&contexts).is_none());
    }

    #[test]
    fn runtime_miss_extracts_real_upstream_error_from_failed_candidate() {
        let candidate = StoredRequestCandidate::new(
            "cand-failed".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            0,
            0,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            RequestCandidateStatus::Failed,
            None,
            false,
            Some(502),
            Some("retryable_upstream_status".to_string()),
            Some("execution runtime stream returned retryable status 502".to_string()),
            None,
            None,
            Some(json!({
                "upstream_response": {
                    "status_code": 502,
                    "headers": {
                        "content-type": "text/event-stream",
                        "x-oneapi-request-id": "202606131644039769451708268d9d6iULlPLG3"
                    },
                    "body": {
                        "type": "error",
                        "error": {
                            "type": "<nil>",
                            "message": "Upstream service temporarily unavailable (request id: 202606131644039769451708268d9d6iULlPLG3)"
                        }
                    }
                }
            })),
            None,
            100,
            Some(101),
            Some(109),
        )
        .expect("candidate should build");

        let upstream_error = local_execution_upstream_error_response_from_candidate(&candidate)
            .expect("upstream error should be extracted");

        assert_eq!(upstream_error.status_code, 502);
        assert_eq!(
            upstream_error.message.as_deref(),
            Some(
                "Upstream service temporarily unavailable (request id: 202606131644039769451708268d9d6iULlPLG3)"
            )
        );
        assert_eq!(upstream_error.error_type.as_deref(), Some("<nil>"));
        assert_eq!(
            upstream_error.body["error"]["message"],
            "Upstream service temporarily unavailable (request id: 202606131644039769451708268d9d6iULlPLG3)"
        );
        assert_eq!(
            upstream_error
                .headers
                .get("content-type")
                .map(String::as_str),
            Some("text/event-stream")
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&upstream_error.body_bytes)
                .expect("body bytes should stay json"),
            upstream_error.body
        );
    }

    #[test]
    fn runtime_miss_preserves_plain_text_upstream_error_body() {
        let candidate = StoredRequestCandidate::new(
            "cand-failed".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            0,
            0,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            RequestCandidateStatus::Failed,
            None,
            false,
            Some(503),
            Some("retryable_upstream_status".to_string()),
            Some("execution runtime stream returned retryable status 503".to_string()),
            None,
            None,
            Some(json!({
                "upstream_response": {
                    "status_code": 503,
                    "headers": {
                        "content-type": "text/plain; charset=utf-8",
                        "content-length": "49"
                    },
                    "body": "upstream 503: temporarily unavailable"
                }
            })),
            None,
            100,
            Some(101),
            Some(109),
        )
        .expect("candidate should build");

        let upstream_error = local_execution_upstream_error_response_from_candidate(&candidate)
            .expect("upstream error should be extracted");

        assert_eq!(upstream_error.status_code, 503);
        assert_eq!(
            upstream_error.message.as_deref(),
            Some("upstream 503: temporarily unavailable")
        );
        assert_eq!(
            upstream_error.body_bytes,
            b"upstream 503: temporarily unavailable"
        );
        assert_eq!(
            upstream_error
                .headers
                .get("content-type")
                .map(String::as_str),
            Some("text/plain; charset=utf-8")
        );
        assert!(!upstream_error.headers.contains_key("content-length"));
    }

    #[test]
    fn runtime_miss_context_exposes_failed_candidate_upstream_error() {
        let candidate = StoredRequestCandidate::new(
            "cand-failed".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            0,
            0,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            RequestCandidateStatus::Failed,
            None,
            false,
            Some(503),
            Some("retryable_upstream_status".to_string()),
            Some("execution runtime stream returned retryable status 503".to_string()),
            None,
            None,
            Some(json!({
                "upstream_response": {
                    "status_code": 503,
                    "headers": {
                        "content-type": "text/plain; charset=utf-8"
                    },
                    "body": "upstream 503: temporarily unavailable"
                }
            })),
            None,
            100,
            Some(101),
            Some(109),
        )
        .expect("candidate should build");
        let context = LocalExecutionRuntimeMissContext {
            candidate_contexts: vec![RuntimeMissCandidateContext {
                candidate,
                provider_name: Some("provider".to_string()),
                key_name: Some("key".to_string()),
                client_api_format: Some("openai:responses".to_string()),
                provider_api_format: Some("openai:responses".to_string()),
                global_model_name: Some("gpt-5".to_string()),
                selected_provider_model_name: Some("gpt-5".to_string()),
                endpoint_url: None,
            }],
            ..LocalExecutionRuntimeMissContext::default()
        };

        let upstream_error = context
            .upstream_error_response()
            .expect("context should expose upstream error");

        assert_eq!(upstream_error.status_code, 503);
        assert_eq!(
            upstream_error.message.as_deref(),
            Some("upstream 503: temporarily unavailable")
        );
    }

    #[test]
    fn runtime_miss_does_not_expose_synthetic_candidate_error_without_upstream_body() {
        let candidate = StoredRequestCandidate::new(
            "cand-failed".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            0,
            0,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            RequestCandidateStatus::Failed,
            None,
            false,
            Some(502),
            Some("retryable_upstream_status".to_string()),
            Some("execution runtime stream returned retryable status 502".to_string()),
            None,
            None,
            Some(json!({
                "upstream_response": {
                    "status_code": 502,
                    "headers": {
                        "content-type": "text/event-stream"
                    }
                }
            })),
            None,
            100,
            Some(101),
            Some(109),
        )
        .expect("candidate should build");

        assert!(local_execution_upstream_error_response_from_candidate(&candidate).is_none());
    }
}

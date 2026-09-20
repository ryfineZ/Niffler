use crate::observability::stats::{aggregate_usage_stats, parse_bounded_u32, round_to};
use aether_ai_formats::api::request_path_implies_stream_request;
use aether_billing::{
    normalize_input_tokens_for_billing, normalize_total_input_context_for_cache_hit_rate,
};
use aether_data::repository::users::StoredUserSummary;
use aether_data_contracts::repository::{
    candidates::{RequestCandidateStatus, StoredRequestCandidate},
    provider_catalog::{StoredProviderCatalogEndpoint, StoredProviderCatalogProvider},
    usage::{StoredRequestUsageAudit, StoredUsageAuditSummary, UsageBodyField},
};
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use url::form_urlencoded;

pub const ADMIN_USAGE_DATA_UNAVAILABLE_DETAIL: &str = "Admin usage data unavailable";
const ADMIN_USAGE_COST_EPSILON: f64 = 0.000_000_01;

struct AdminUsageChargeBreakdown {
    payload: Value,
    user_debit_usd: f64,
}

pub fn admin_usage_data_unavailable_response(detail: &'static str) -> Response<Body> {
    (
        http::StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "detail": detail })),
    )
        .into_response()
}

pub fn admin_usage_bad_request_response(detail: impl Into<String>) -> Response<Body> {
    (
        http::StatusCode::BAD_REQUEST,
        Json(json!({ "detail": detail.into() })),
    )
        .into_response()
}

fn query_param_value(query: Option<&str>, key: &str) -> Option<String> {
    let query = query?;
    for (entry_key, value) in form_urlencoded::parse(query.as_bytes()) {
        if entry_key == key {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn unix_secs_to_rfc3339(unix_secs: u64) -> Option<String> {
    let timestamp =
        chrono::DateTime::<chrono::Utc>::from_timestamp(i64::try_from(unix_secs).ok()?, 0)?;
    Some(timestamp.to_rfc3339())
}

pub fn admin_usage_parse_limit(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "limit") {
        None => Ok(100),
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "limit must be a positive integer".to_string())?;
            if parsed == 0 || parsed > 500 {
                return Err("limit must be between 1 and 500".to_string());
            }
            Ok(parsed)
        }
    }
}

pub fn admin_usage_parse_offset(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "offset") {
        None => Ok(0),
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| "offset must be a non-negative integer".to_string()),
    }
}

pub fn admin_usage_parse_ids(query: Option<&str>) -> Option<BTreeSet<String>> {
    let ids = query_param_value(query, "ids")?;
    let parsed: BTreeSet<String> = ids
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    Some(parsed)
}

pub fn admin_usage_parse_recent_hours(query: Option<&str>, default: u32) -> Result<u32, String> {
    match query_param_value(query, "hours") {
        Some(value) => parse_bounded_u32("hours", &value, 1, 720),
        None => Ok(default),
    }
}

pub fn admin_usage_parse_timeline_limit(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "limit") {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "limit must be an integer between 100 and 50000".to_string())?;
            if (100..=50_000).contains(&parsed) {
                Ok(parsed)
            } else {
                Err("limit must be an integer between 100 and 50000".to_string())
            }
        }
        None => Ok(3_000),
    }
}

pub fn admin_usage_parse_aggregation_limit(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "limit") {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "limit must be an integer between 1 and 100".to_string())?;
            if (1..=100).contains(&parsed) {
                Ok(parsed)
            } else {
                Err("limit must be an integer between 1 and 100".to_string())
            }
        }
        None => Ok(20),
    }
}

pub fn admin_usage_username(
    item: &StoredRequestUsageAudit,
    users_by_id: &BTreeMap<String, StoredUserSummary>,
    auth_user_reader_available: bool,
) -> Option<String> {
    item.user_id
        .as_ref()
        .and_then(|user_id| users_by_id.get(user_id))
        .map(|value| value.username.clone())
        .or_else(|| {
            (!auth_user_reader_available)
                .then(|| item.username.clone())
                .flatten()
        })
}

pub fn admin_usage_api_key_name(
    item: &StoredRequestUsageAudit,
    api_key_names: &BTreeMap<String, String>,
    auth_api_key_reader_available: bool,
) -> Option<String> {
    item.api_key_id
        .as_ref()
        .and_then(|api_key_id| api_key_names.get(api_key_id))
        .cloned()
        .or_else(|| {
            (!auth_api_key_reader_available)
                .then(|| item.api_key_name.clone())
                .flatten()
        })
}

pub fn admin_usage_matches_search(
    item: &StoredRequestUsageAudit,
    search: Option<&str>,
    users_by_id: &BTreeMap<String, StoredUserSummary>,
    api_key_names: &BTreeMap<String, String>,
    auth_user_reader_available: bool,
    auth_api_key_reader_available: bool,
) -> bool {
    let Some(search) = search.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let username = admin_usage_username(item, users_by_id, auth_user_reader_available);
    let api_key_name = admin_usage_api_key_name(item, api_key_names, auth_api_key_reader_available);
    let haystack = [
        username.as_deref(),
        api_key_name.as_deref(),
        Some(item.model.as_str()),
        Some(item.provider_name.as_str()),
    ];
    search.split_whitespace().all(|keyword| {
        let keyword = keyword.to_ascii_lowercase();
        haystack
            .iter()
            .flatten()
            .any(|value| value.to_ascii_lowercase().contains(keyword.as_str()))
    })
}

pub fn admin_usage_matches_username(
    item: &StoredRequestUsageAudit,
    username: Option<&str>,
    users_by_id: &BTreeMap<String, StoredUserSummary>,
    auth_user_reader_available: bool,
) -> bool {
    let Some(username) = username.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    admin_usage_username(item, users_by_id, auth_user_reader_available)
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains(username.to_ascii_lowercase().as_str())
}

pub fn admin_usage_matches_eq(value: &str, query: Option<&str>) -> bool {
    let Some(query) = query
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
    else {
        return true;
    };
    value.eq_ignore_ascii_case(query)
}

pub fn admin_usage_matches_api_format(
    item: &StoredRequestUsageAudit,
    api_format: Option<&str>,
) -> bool {
    let Some(api_format) = api_format.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    item.api_format
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case(api_format))
}

pub fn admin_usage_is_failed(item: &StoredRequestUsageAudit) -> bool {
    let has_failure_signal = item
        .status_code
        .is_some_and(|value| !(200..300).contains(&value))
        || item
            .error_message
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
    let status = item.status.trim().to_ascii_lowercase();
    if !status.is_empty() {
        return match status.as_str() {
            "completed" | "cancelled" => false,
            "pending" | "streaming" => has_failure_signal,
            "failed" => true,
            _ => false,
        };
    }
    has_failure_signal
}

pub fn admin_usage_has_fallback(item: &StoredRequestUsageAudit) -> bool {
    item.has_fallback()
}

#[derive(Clone, Debug, Default)]
pub struct AdminUsageAttemptInfo {
    pub has_fallback: bool,
    pub has_retry: bool,
    pub provider_route: Vec<String>,
}

pub fn admin_usage_attempt_status_filter(status: Option<&str>) -> Option<&'static str> {
    match status?.trim().to_ascii_lowercase().as_str() {
        "has_fallback" => Some("has_fallback"),
        "has_retry" => Some("has_retry"),
        _ => None,
    }
}

fn admin_usage_candidate_failed_before_fallback(candidate: &StoredRequestCandidate) -> bool {
    candidate.status.is_attempted(candidate.started_at_unix_ms)
        && (matches!(
            candidate.status,
            RequestCandidateStatus::Failed | RequestCandidateStatus::Cancelled
        ) || candidate.status_code.is_some_and(|code| code >= 400))
}

fn admin_usage_candidate_was_retried(candidate: &StoredRequestCandidate) -> bool {
    candidate.retry_index > 0 && candidate.status.is_attempted(candidate.started_at_unix_ms)
}

fn admin_usage_final_candidate_index(
    item: &StoredRequestUsageAudit,
    candidates: &[StoredRequestCandidate],
) -> Option<u64> {
    if let Some(candidate_id) = item.routing_candidate_id() {
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.id == candidate_id)
        {
            return Some(u64::from(candidate.candidate_index));
        }
    }

    item.routing_candidate_index().or_else(|| {
        candidates
            .iter()
            .filter(|candidate| candidate.status.is_attempted(candidate.started_at_unix_ms))
            .max_by(|left, right| {
                left.candidate_index
                    .cmp(&right.candidate_index)
                    .then(left.retry_index.cmp(&right.retry_index))
            })
            .map(|candidate| u64::from(candidate.candidate_index))
    })
}

fn admin_usage_provider_route(
    item: &StoredRequestUsageAudit,
    candidates: &[StoredRequestCandidate],
    provider_names_by_id: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut route = Vec::new();
    for candidate in candidates {
        if !candidate.status.is_attempted(candidate.started_at_unix_ms) {
            continue;
        }
        let Some(provider_id) = candidate.provider_id.as_deref() else {
            continue;
        };
        let name = provider_names_by_id
            .get(provider_id)
            .cloned()
            .unwrap_or_else(|| provider_id.to_string());
        if route.last().is_some_and(|last| last == &name) {
            continue;
        }
        route.push(name);
    }

    if route.is_empty() && !admin_usage_provider_label_is_reserved(&item.provider_name) {
        route.push(item.provider_name.clone());
    }

    route
}

pub fn admin_usage_provider_label_is_reserved(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "pending" | "unknown" | "unknow"
    )
}

fn admin_usage_provider_display_for_record(item: &StoredRequestUsageAudit) -> String {
    if !admin_usage_provider_label_is_reserved(&item.provider_name) {
        return item.provider_name.clone();
    }

    if item.routing_execution_path() != Some("local_execution_runtime_miss") {
        return item.provider_name.clone();
    }

    match item.routing_local_execution_runtime_miss_reason() {
        Some("candidate_list_empty") => "无可用供应商".to_string(),
        Some("all_candidates_skipped") => "可尝试服务全部不可用".to_string(),
        _ => "未进入上游".to_string(),
    }
}

pub fn admin_usage_apply_provider_route_display(record: &mut Value, provider_route: &[String]) {
    if provider_route.is_empty() {
        return;
    }
    let Some(provider) = record.get("provider").and_then(Value::as_str) else {
        return;
    };
    if !admin_usage_provider_label_is_reserved(provider) {
        return;
    }
    if let Some(display_name) = provider_route.last() {
        record["provider"] = json!(display_name);
    }
}

pub fn admin_usage_attempt_info_from_candidates(
    item: &StoredRequestUsageAudit,
    candidates: &[StoredRequestCandidate],
    provider_names_by_id: &BTreeMap<String, String>,
) -> AdminUsageAttemptInfo {
    let final_candidate_index = admin_usage_final_candidate_index(item, candidates);
    let has_retry = candidates.iter().any(admin_usage_candidate_was_retried);
    let provider_route = admin_usage_provider_route(item, candidates, provider_names_by_id);
    let has_fallback = provider_route.len() > 1
        && final_candidate_index.is_some_and(|final_index| {
            candidates.iter().any(|candidate| {
                u64::from(candidate.candidate_index) < final_index
                    && admin_usage_candidate_failed_before_fallback(candidate)
            })
        });

    AdminUsageAttemptInfo {
        has_fallback,
        has_retry,
        provider_route,
    }
}

pub fn admin_usage_attempt_info_for_item(
    item: &StoredRequestUsageAudit,
    info_by_usage_id: &BTreeMap<String, AdminUsageAttemptInfo>,
    request_candidate_reader_available: bool,
) -> AdminUsageAttemptInfo {
    info_by_usage_id.get(&item.id).cloned().unwrap_or_else(|| {
        if request_candidate_reader_available {
            AdminUsageAttemptInfo::default()
        } else {
            AdminUsageAttemptInfo {
                has_fallback: admin_usage_has_fallback(item),
                has_retry: false,
                provider_route: Vec::new(),
            }
        }
    })
}

pub fn admin_usage_matches_attempt_status(
    item: &StoredRequestUsageAudit,
    status: &str,
    info_by_usage_id: &BTreeMap<String, AdminUsageAttemptInfo>,
    request_candidate_reader_available: bool,
) -> bool {
    let info = admin_usage_attempt_info_for_item(
        item,
        info_by_usage_id,
        request_candidate_reader_available,
    );
    match status {
        "has_fallback" => info.has_fallback,
        "has_retry" => info.has_retry,
        _ => true,
    }
}

pub fn admin_usage_matches_client_family(
    item: &StoredRequestUsageAudit,
    client_family: Option<&str>,
) -> bool {
    let Some(client_family) = client_family
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    admin_usage_client_family(item).is_some_and(|value| value.eq_ignore_ascii_case(client_family))
}

pub fn admin_usage_matches_status(item: &StoredRequestUsageAudit, status: Option<&str>) -> bool {
    let Some(status) = status.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    match status {
        "stream" => item.is_stream,
        "standard" => !item.is_stream,
        "error" => {
            item.status_code
                .is_some_and(|value| !(200..300).contains(&value))
                || item.error_message.is_some()
        }
        "pending" | "streaming" | "completed" | "cancelled" => item.status == status,
        "failed" => admin_usage_is_failed(item),
        "active" => matches!(item.status.as_str(), "pending" | "streaming"),
        "has_fallback" => admin_usage_has_fallback(item),
        _ => true,
    }
}

fn admin_usage_has_body_value(
    item: &StoredRequestUsageAudit,
    body: Option<&Value>,
    field: UsageBodyField,
) -> bool {
    item.body_capture_result(field, body).available
}

fn admin_usage_strip_body_ref_metadata(metadata: &mut serde_json::Map<String, Value>) {
    metadata.remove(UsageBodyField::RequestBody.as_ref_key());
    metadata.remove(UsageBodyField::ProviderRequestBody.as_ref_key());
    metadata.remove(UsageBodyField::ResponseBody.as_ref_key());
    metadata.remove(UsageBodyField::ClientResponseBody.as_ref_key());
}

fn admin_usage_strip_routing_metadata(metadata: &mut serde_json::Map<String, Value>) {
    metadata.remove("candidate_id");
    metadata.remove("candidate_index");
    metadata.remove("key_name");
    metadata.remove("model_id");
    metadata.remove("global_model_id");
    metadata.remove("global_model_name");
    metadata.remove("planner_kind");
    metadata.remove("route_family");
    metadata.remove("route_kind");
    metadata.remove("execution_path");
    metadata.remove("local_execution_runtime_miss_reason");
}

fn admin_usage_strip_settlement_metadata(metadata: &mut serde_json::Map<String, Value>) {
    metadata.remove("billing_snapshot");
    metadata.remove("billing_snapshot_schema_version");
    metadata.remove("billing_snapshot_status");
    metadata.remove("settlement_snapshot");
    metadata.remove("settlement_snapshot_schema_version");
    metadata.remove("billing_dimensions");
    metadata.remove("rate_multiplier");
    metadata.remove("is_free_tier");
    metadata.remove("input_price_per_1m");
    metadata.remove("output_price_per_1m");
    metadata.remove("cache_creation_price_per_1m");
    metadata.remove("cache_read_price_per_1m");
    metadata.remove("price_per_request");
}

fn admin_usage_strip_trace_metadata(metadata: &mut serde_json::Map<String, Value>) {
    metadata.remove("trace_id");
}

fn admin_usage_string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn admin_usage_error_field<'a>(body: &'a Value, field: &str) -> Option<&'a str> {
    body.get("error")
        .and_then(|error| match error {
            Value::Object(object) => object.get(field).and_then(Value::as_str),
            _ => None,
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| admin_usage_string_field(body, field))
}

fn admin_usage_error_message_from_body(body: &Value) -> Option<String> {
    if let Value::String(message) = body {
        return Some(message.trim().to_string()).filter(|value| !value.is_empty());
    }

    body.get("error")
        .and_then(|error| match error {
            Value::Object(object) => object.get("message").and_then(Value::as_str),
            Value::String(message) => Some(message.as_str()),
            _ => None,
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            admin_usage_string_field(body, "message")
                .or_else(|| admin_usage_string_field(body, "detail"))
                .map(ToOwned::to_owned)
        })
}

fn admin_usage_error_type_from_body(body: &Value) -> Option<String> {
    admin_usage_error_field(body, "type")
        .or_else(|| admin_usage_error_field(body, "status"))
        .or_else(|| admin_usage_error_field(body, "kind"))
        .map(ToOwned::to_owned)
}

fn admin_usage_error_code_from_body(body: &Value) -> Option<Value> {
    body.get("error")
        .and_then(|error| match error {
            Value::Object(object) => object.get("code").cloned(),
            _ => None,
        })
        .or_else(|| body.get("code").cloned())
}

fn admin_usage_status_code_is_error(status_code: Option<u64>) -> bool {
    status_code.is_some_and(|value| !(200..300).contains(&value))
}

fn admin_usage_header_content_type(headers: Option<&Value>) -> Option<String> {
    let object = headers?.as_object()?;
    for (key, value) in object {
        if key.eq_ignore_ascii_case("content-type") {
            return value
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| (!value.is_null()).then(|| value.to_string()));
        }
    }
    None
}

fn admin_usage_error_domain_json(
    source: &str,
    status_code: Option<u16>,
    headers: Option<&Value>,
    body: Option<&Value>,
    fallback_type: Option<&str>,
    fallback_message: Option<&str>,
) -> Value {
    let error_type = body
        .and_then(admin_usage_error_type_from_body)
        .or_else(|| fallback_type.map(ToOwned::to_owned));
    let message = body
        .and_then(admin_usage_error_message_from_body)
        .or_else(|| {
            fallback_message
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let code = body.and_then(admin_usage_error_code_from_body);
    let content_type = admin_usage_header_content_type(headers);
    let status_is_error = admin_usage_status_code_is_error(status_code.map(u64::from));
    let body_has_error_payload = body
        .and_then(|value| value.get("error"))
        .is_some_and(|value| !value.is_null());

    if !status_is_error && message.is_none() && code.is_none() && !body_has_error_payload {
        return Value::Null;
    }

    json!({
        "source": source,
        "status_code": status_code,
        "type": error_type,
        "message": message,
        "code": code,
        "content_type": content_type,
        "body": body.cloned().unwrap_or(Value::Null),
    })
}

fn admin_usage_local_execution_client_error_message(message: &str) -> String {
    let without_reason = message
        .split_once("（原因代码:")
        .map(|(prefix, _)| prefix)
        .unwrap_or(message);
    let mut simplified = without_reason.trim().to_string();
    if let Some(unavailable_message) =
        admin_usage_simplify_all_candidates_skipped_client_error_message(simplified.as_str())
    {
        return unavailable_message;
    }
    for marker in ["。请检查", "。请确认", ". 请检查", ". 请确认"] {
        if let Some(index) = simplified.find(marker) {
            simplified.truncate(index);
            break;
        }
    }
    simplified
        .trim()
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .to_string()
}

fn admin_usage_simplify_all_candidates_skipped_client_error_message(
    message: &str,
) -> Option<String> {
    if !message.contains("候选提供商")
        || !(message.contains("全部不可用") || message.contains("都不满足本次"))
    {
        return None;
    }

    let request_mode = admin_usage_extract_local_execution_request_mode(message)?;
    if let Some(model) = admin_usage_extract_candidate_supported_model(message) {
        return Some(format!(
            "没有可用提供商支持模型 {model} 的{request_mode}请求"
        ));
    }

    Some(format!("没有可用提供商支持本次{request_mode}请求"))
}

fn admin_usage_local_runtime_miss_reason_label(reason: &str) -> &'static str {
    match reason {
        "all_candidates_skipped" => "可尝试服务全部不可用",
        "candidate_list_empty" => "没有可调度服务",
        "local_runtime_unavailable" => "本地执行运行时不可用",
        "provider_transport_unavailable" => "提供商传输不可用",
        _ => "本地调度未命中",
    }
}

fn admin_usage_extract_local_runtime_miss_reason_summary(message: &str) -> Option<String> {
    let without_reason_code = message
        .split_once("（原因代码:")
        .map(|(prefix, _)| prefix)
        .unwrap_or(message)
        .trim();
    let summary = without_reason_code
        .rsplit_once('：')
        .or_else(|| without_reason_code.rsplit_once(':'))
        .map(|(_, suffix)| suffix.trim())?;
    (!summary.is_empty()).then(|| summary.to_string())
}

fn admin_usage_scheduling_failure_json(
    item: &StoredRequestUsageAudit,
    client_error: &Value,
    upstream_error: &Value,
) -> Value {
    if item.routing_execution_path() != Some("local_execution_runtime_miss") {
        return Value::Null;
    }
    if admin_usage_has_upstream_response_error(upstream_error) {
        return Value::Null;
    }

    let reason = item
        .routing_local_execution_runtime_miss_reason()
        .unwrap_or("local_execution_runtime_miss");
    let message = admin_usage_error_domain_message(client_error)
        .or_else(|| admin_usage_client_error_fallback_message(item))
        .or_else(|| item.error_message.as_deref().map(str::to_string));
    let raw_message = item
        .error_message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let reason_summary =
        raw_message.and_then(admin_usage_extract_local_runtime_miss_reason_summary);

    json!({
        "source": "local_execution_runtime_miss",
        "reason": reason,
        "reason_label": admin_usage_local_runtime_miss_reason_label(reason),
        "title": format!("本地调度失败：{}", admin_usage_local_runtime_miss_reason_label(reason)),
        "message": message,
        "reason_summary": reason_summary,
        "status_code": item.status_code,
        "no_upstream_attempt": item.candidate_id.is_none()
            && item.provider_api_key_id.is_none()
            && item.provider_request_headers.is_none()
            && item.provider_request_body.is_none()
            && item.provider_request_body_ref.is_none(),
    })
}

fn admin_usage_extract_local_execution_request_mode(message: &str) -> Option<&str> {
    let rest = message.get(message.find("本次")? + "本次".len()..)?;
    let mode = rest.get(..rest.find("请求")?)?.trim();
    (!mode.is_empty()).then_some(mode)
}

fn admin_usage_extract_candidate_supported_model(message: &str) -> Option<&str> {
    let rest = message.get(message.find("支持模型 ")? + "支持模型 ".len()..)?;
    let model = rest.get(..rest.find(" 的")?)?.trim();
    (!model.is_empty()).then_some(model)
}

fn admin_usage_client_error_fallback_message(item: &StoredRequestUsageAudit) -> Option<String> {
    item.error_message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|message| {
            if item.routing_execution_path() == Some("local_execution_runtime_miss") {
                admin_usage_local_execution_client_error_message(message)
            } else {
                message.to_string()
            }
        })
}

fn admin_usage_error_domain_message(domain: &Value) -> Option<String> {
    domain
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn admin_usage_error_domain_type(domain: &Value) -> Option<String> {
    domain
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn admin_usage_error_domain_status_code(domain: &Value) -> Option<u64> {
    domain.get("status_code").and_then(Value::as_u64)
}

fn admin_usage_error_domain_display_message(domain: &Value) -> Option<String> {
    admin_usage_error_domain_message(domain).or_else(|| {
        let source = domain.get("source").and_then(Value::as_str)?;
        if source != "upstream_response" {
            return None;
        }
        let status_code = admin_usage_error_domain_status_code(domain)?;
        if !admin_usage_status_code_is_error(Some(status_code)) {
            return None;
        }
        Some(format!("上游返回 HTTP {status_code}"))
    })
}

fn admin_usage_has_upstream_response_error(domain: &Value) -> bool {
    if domain.is_null() {
        return false;
    }
    admin_usage_error_domain_message(domain).is_some()
        || admin_usage_status_code_is_error(admin_usage_error_domain_status_code(domain))
        || !domain.get("body").unwrap_or(&Value::Null).is_null()
}

fn admin_usage_error_domain_source(domain: &Value) -> Option<String> {
    domain
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn admin_usage_failure_summary_json(
    item: &StoredRequestUsageAudit,
    request_error: &Value,
    upstream_error: &Value,
    client_error: &Value,
) -> Value {
    let selected = [upstream_error, client_error, request_error]
        .into_iter()
        .find(|domain| {
            !domain.is_null() && admin_usage_error_domain_display_message(domain).is_some()
        });
    let message = selected
        .and_then(admin_usage_error_domain_display_message)
        .or_else(|| {
            item.error_message
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let Some(message) = message else {
        return Value::Null;
    };
    let error_type = selected
        .and_then(admin_usage_error_domain_type)
        .or_else(|| item.error_category.clone());
    let source = selected
        .and_then(admin_usage_error_domain_source)
        .unwrap_or_else(|| "usage_summary".to_string());

    json!({
        "source": source,
        "status_code": item.status_code,
        "type": error_type,
        "message": message,
        "category": item.error_category,
    })
}

fn admin_usage_error_domains_json(item: &StoredRequestUsageAudit) -> Value {
    let has_upstream_attempt = item.candidate_id.is_some()
        || item.provider_api_key_id.is_some()
        || item.provider_request_headers.is_some()
        || item.provider_request_body.is_some()
        || item.provider_request_body_ref.is_some();
    let upstream_error = if has_upstream_attempt {
        let upstream_fallback_message =
            if item.routing_execution_path() == Some("local_execution_runtime_miss") {
                None
            } else {
                item.error_message.as_deref()
            };
        admin_usage_error_domain_json(
            "upstream_response",
            item.status_code,
            item.response_headers.as_ref(),
            item.response_body.as_ref(),
            item.error_category.as_deref(),
            upstream_fallback_message,
        )
    } else {
        Value::Null
    };
    let client_error_fallback_message = admin_usage_client_error_fallback_message(item);
    let client_error = admin_usage_error_domain_json(
        "client_response",
        item.status_code,
        item.client_response_headers.as_ref(),
        item.client_response_body.as_ref(),
        item.error_category.as_deref(),
        client_error_fallback_message.as_deref(),
    );
    let request_error = Value::Null;
    let failure_summary =
        admin_usage_failure_summary_json(item, &request_error, &upstream_error, &client_error);

    json!({
        "request_error": request_error,
        "upstream_error": upstream_error,
        "client_error": client_error,
        "failure_summary": failure_summary,
    })
}

fn admin_usage_final_attempted_candidate<'a>(
    item: &StoredRequestUsageAudit,
    candidates: &'a [StoredRequestCandidate],
) -> Option<&'a StoredRequestCandidate> {
    if let Some(candidate_id) = item.routing_candidate_id() {
        if let Some(candidate) = candidates.iter().find(|candidate| {
            candidate.id == candidate_id
                && candidate.status.is_attempted(candidate.started_at_unix_ms)
        }) {
            return Some(candidate);
        }
    }

    if let Some(candidate_index) = item.routing_candidate_index() {
        if let Some(candidate) = candidates
            .iter()
            .filter(|candidate| {
                u64::from(candidate.candidate_index) == candidate_index
                    && candidate.status.is_attempted(candidate.started_at_unix_ms)
            })
            .max_by(|left, right| left.retry_index.cmp(&right.retry_index))
        {
            return Some(candidate);
        }
    }

    candidates
        .iter()
        .filter(|candidate| candidate.status.is_attempted(candidate.started_at_unix_ms))
        .max_by(|left, right| {
            left.candidate_index
                .cmp(&right.candidate_index)
                .then(left.retry_index.cmp(&right.retry_index))
        })
}

fn admin_usage_candidate_upstream_response_domain(
    candidate: &StoredRequestCandidate,
) -> Option<Value> {
    let upstream_response = candidate.extra_data.as_ref()?.get("upstream_response")?;
    let status_code = upstream_response
        .get("status_code")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .or(candidate.status_code);
    let headers = upstream_response.get("headers");
    let body = upstream_response.get("body");
    let domain = admin_usage_error_domain_json(
        "upstream_response",
        status_code,
        headers,
        body,
        candidate.error_type.as_deref(),
        candidate.error_message.as_deref(),
    );
    admin_usage_error_domain_message(&domain).map(|_| domain)
}

pub fn admin_usage_apply_candidate_upstream_error_display(
    payload: &mut Value,
    item: &StoredRequestUsageAudit,
    candidates: &[StoredRequestCandidate],
) {
    let Some(candidate) = admin_usage_final_attempted_candidate(item, candidates) else {
        return;
    };
    let Some(upstream_error) = admin_usage_candidate_upstream_response_domain(candidate) else {
        return;
    };
    let Some(message) = admin_usage_error_domain_message(&upstream_error) else {
        return;
    };

    let failure_summary = json!({
        "source": "upstream_response",
        "status_code": admin_usage_error_domain_status_code(&upstream_error).or(item.status_code.map(u64::from)),
        "type": admin_usage_error_domain_type(&upstream_error).or_else(|| item.error_category.clone()),
        "message": message,
        "category": item.error_category,
    });

    payload["upstream_error"] = upstream_error.clone();
    payload["failure_summary"] = failure_summary.clone();
    if let Some(errors) = payload.get_mut("errors").and_then(Value::as_object_mut) {
        errors.insert("upstream_error".to_string(), upstream_error.clone());
        errors.insert("failure_summary".to_string(), failure_summary.clone());
    }

    let error_domains = json!({
        "request_error": payload["request_error"].clone(),
        "upstream_error": upstream_error,
        "client_error": payload["client_error"].clone(),
        "failure_summary": failure_summary,
    });
    payload["scheduling_failure"] = admin_usage_scheduling_failure_json(
        item,
        &error_domains["client_error"],
        &error_domains["upstream_error"],
    );
    payload["error_flow"] = admin_usage_error_flow_json(item, &error_domains);
}

fn admin_usage_error_domain_search_text(domain: &Value) -> String {
    [
        domain.get("type").and_then(Value::as_str),
        domain.get("message").and_then(Value::as_str),
        domain.get("code").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .map(str::to_ascii_lowercase)
    .collect::<Vec<_>>()
    .join(" ")
}

fn admin_usage_upstream_error_is_sensitive(domain: &Value) -> bool {
    let text = admin_usage_error_domain_search_text(domain);
    [
        "insufficient_quota",
        "insufficient quota",
        "quota exhausted",
        "credits exhausted",
        "credit balance",
        "credit limit",
        "payment_required",
        "payment required",
        "account disabled",
        "account_deactivated",
        "subscription inactive",
        "verification required",
    ]
    .iter()
    .any(|pattern| text.contains(pattern))
}

fn admin_usage_error_flow_json(item: &StoredRequestUsageAudit, error_domains: &Value) -> Value {
    let request_error = &error_domains["request_error"];
    let upstream_error = &error_domains["upstream_error"];
    let client_error = &error_domains["client_error"];
    let failure_summary = &error_domains["failure_summary"];
    if request_error.is_null()
        && upstream_error.is_null()
        && client_error.is_null()
        && failure_summary.is_null()
    {
        return Value::Null;
    }

    let upstream_sensitive =
        !upstream_error.is_null() && admin_usage_upstream_error_is_sensitive(upstream_error);
    let propagation = if upstream_sensitive {
        "suppressed"
    } else if !client_error.is_null() && !upstream_error.is_null() {
        let upstream_message = admin_usage_error_domain_message(upstream_error);
        let client_message = admin_usage_error_domain_message(client_error);
        if upstream_message.is_some() && upstream_message == client_message {
            "passthrough"
        } else {
            "converted"
        }
    } else if !client_error.is_null() {
        "local"
    } else if !upstream_error.is_null() {
        "captured"
    } else {
        "none"
    };
    let source = if !request_error.is_null() {
        "request"
    } else if !upstream_error.is_null() {
        "upstream"
    } else if !client_error.is_null() {
        "gateway"
    } else {
        "summary"
    };
    let client_response_source = if client_error.is_null() {
        Value::Null
    } else if !upstream_error.is_null() {
        Value::String("converted_or_sanitized_upstream".to_string())
    } else {
        Value::String("gateway_generated".to_string())
    };

    json!({
        "source": source,
        "status_code": item.status_code,
        "propagation": propagation,
        "client_response_source": client_response_source,
        "safe_to_expose_upstream": !upstream_sensitive,
        "summary_source": failure_summary.get("source").cloned().unwrap_or(Value::Null),
    })
}

fn maybe_insert_number_field(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<f64>,
) {
    if let Some(value) = value {
        object.insert(key.to_string(), json!(value));
    }
}

fn maybe_insert_u64_field(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<u64>,
) {
    if let Some(value) = value {
        object.insert(key.to_string(), json!(value));
    }
}

fn maybe_insert_bool_field(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<bool>,
) {
    if let Some(value) = value {
        object.insert(key.to_string(), json!(value));
    }
}

fn admin_usage_body_capture_json(item: &StoredRequestUsageAudit) -> Value {
    item.body_capture_json_for_fields(&[
        UsageBodyField::RequestBody,
        UsageBodyField::ProviderRequestBody,
        UsageBodyField::ResponseBody,
        UsageBodyField::ClientResponseBody,
    ])
}

fn admin_usage_settlement_json(item: &StoredRequestUsageAudit) -> Value {
    let mut settlement = serde_json::Map::new();
    if let Some(snapshot) = item
        .request_metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("settlement_snapshot"))
        .cloned()
    {
        settlement.insert("settlement_snapshot".to_string(), snapshot);
    }
    if let Some(schema_version) = item
        .request_metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("settlement_snapshot_schema_version"))
        .and_then(Value::as_str)
    {
        settlement.insert(
            "settlement_snapshot_schema_version".to_string(),
            json!(schema_version),
        );
    }
    if let Some(dimensions) = item
        .request_metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("billing_dimensions"))
        .cloned()
    {
        settlement.insert("billing_dimensions".to_string(), dimensions);
    }
    if let Some(snapshot) = item
        .request_metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("billing_snapshot"))
        .cloned()
    {
        settlement.insert("billing_snapshot".to_string(), snapshot);
    }
    maybe_insert_string_field(
        &mut settlement,
        "billing_snapshot_schema_version",
        item.settlement_billing_snapshot_schema_version(),
    );
    maybe_insert_string_field(
        &mut settlement,
        "billing_snapshot_status",
        item.settlement_billing_snapshot_status(),
    );
    maybe_insert_number_field(
        &mut settlement,
        "rate_multiplier",
        item.settlement_rate_multiplier(),
    );
    maybe_insert_bool_field(
        &mut settlement,
        "is_free_tier",
        item.settlement_is_free_tier(),
    );
    maybe_insert_number_field(
        &mut settlement,
        "input_price_per_1m",
        item.settlement_input_price_per_1m(),
    );
    maybe_insert_number_field(
        &mut settlement,
        "output_price_per_1m",
        item.settlement_output_price_per_1m(),
    );
    maybe_insert_number_field(
        &mut settlement,
        "cache_creation_price_per_1m",
        item.settlement_cache_creation_price_per_1m(),
    );
    maybe_insert_number_field(
        &mut settlement,
        "cache_read_price_per_1m",
        item.settlement_cache_read_price_per_1m(),
    );
    maybe_insert_number_field(
        &mut settlement,
        "price_per_request",
        item.settlement_price_per_request(),
    );
    if settlement.is_empty() {
        Value::Null
    } else {
        Value::Object(settlement)
    }
}

fn admin_usage_trace_json(item: &StoredRequestUsageAudit) -> Value {
    let mut trace = serde_json::Map::new();
    if let Some(trace_id) = item.trace_id() {
        trace.insert("trace_id".to_string(), json!(trace_id));
    }
    if trace.is_empty() {
        Value::Null
    } else {
        Value::Object(trace)
    }
}

fn admin_usage_replay_body_capture_json(item: &StoredRequestUsageAudit) -> Value {
    item.body_capture_json_for_fields(&[UsageBodyField::RequestBody])
}

fn admin_usage_curl_body_capture_json(item: &StoredRequestUsageAudit) -> Value {
    let mut object = item.body_capture_json_object_for_fields(&[
        UsageBodyField::RequestBody,
        UsageBodyField::ProviderRequestBody,
    ]);
    object.insert(
        "body_source".to_string(),
        Value::String(item.curl_body_source().to_string()),
    );
    Value::Object(object)
}

pub fn admin_usage_provider_key_name(
    item: &StoredRequestUsageAudit,
    provider_key_names: &BTreeMap<String, String>,
) -> Option<String> {
    item.provider_api_key_id
        .as_ref()
        .and_then(|key_id| provider_key_names.get(key_id))
        .cloned()
        .or_else(|| item.routing_key_name().map(ToOwned::to_owned))
}

pub fn admin_usage_provider_key_account_label(
    item: &StoredRequestUsageAudit,
    provider_key_account_labels: &BTreeMap<String, String>,
) -> Option<String> {
    item.provider_api_key_id
        .as_ref()
        .and_then(|key_id| provider_key_account_labels.get(key_id))
        .cloned()
}

fn admin_usage_request_body_stream_flag(item: &StoredRequestUsageAudit) -> Option<bool> {
    item.request_body
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|body| body.get("stream"))
        .and_then(Value::as_bool)
}

fn admin_usage_api_format_defaults_to_non_stream(item: &StoredRequestUsageAudit) -> bool {
    let api_format = item
        .api_format
        .as_deref()
        .or(item.endpoint_api_format.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(value) = api_format else {
        return false;
    };
    matches!(
        aether_ai_formats::normalize_api_format_alias(value).as_str(),
        "openai:chat"
            | "openai:responses"
            | "openai:responses:compact"
            | "openai:image"
            | "claude:messages"
    )
}

fn admin_usage_request_body_implies_default_non_stream(item: &StoredRequestUsageAudit) -> bool {
    let Some(body) = item.request_body.as_ref().and_then(Value::as_object) else {
        return false;
    };
    !body.contains_key("stream") && admin_usage_api_format_defaults_to_non_stream(item)
}

fn admin_usage_headers_stream_flag(headers: Option<&Value>) -> Option<bool> {
    let object = headers.and_then(Value::as_object)?;
    let raw = object
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .and_then(|(_, value)| match value {
            Value::String(text) => Some(text.as_str()),
            Value::Array(values) => values.iter().find_map(Value::as_str),
            _ => None,
        })?
        .trim();
    if raw.is_empty() {
        return None;
    }

    let normalized = raw.to_ascii_lowercase();
    Some(
        normalized.contains("event-stream")
            || normalized.contains("eventstream")
            || normalized.contains("x-ndjson"),
    )
}

fn admin_usage_body_is_sse_capture(value: Option<&Value>) -> bool {
    let Some(object) = value.and_then(Value::as_object) else {
        return false;
    };
    object.get("chunks").and_then(Value::as_array).is_some()
        && object
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("stream"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn admin_usage_infer_client_stream_from_captured_bodies(
    item: &StoredRequestUsageAudit,
) -> Option<bool> {
    let provider_stream = admin_usage_body_is_sse_capture(item.response_body.as_ref());
    let client_stream = admin_usage_body_is_sse_capture(item.client_response_body.as_ref());
    if client_stream {
        Some(true)
    } else if provider_stream && item.client_response_body.is_some() {
        Some(false)
    } else {
        None
    }
}

fn admin_usage_infer_upstream_stream_from_captured_bodies(
    item: &StoredRequestUsageAudit,
) -> Option<bool> {
    let provider_stream = admin_usage_body_is_sse_capture(item.response_body.as_ref());
    let client_stream = admin_usage_body_is_sse_capture(item.client_response_body.as_ref());
    if provider_stream {
        Some(true)
    } else if client_stream && item.response_body.is_some() {
        Some(false)
    } else {
        None
    }
}

fn admin_usage_request_path_implies_client_stream(item: &StoredRequestUsageAudit) -> bool {
    let Some(metadata) = item.request_metadata.as_ref().and_then(Value::as_object) else {
        return false;
    };
    ["request_path", "request_path_and_query"]
        .into_iter()
        .filter_map(|field| metadata.get(field).and_then(Value::as_str))
        .any(request_path_implies_stream_request)
}

pub fn admin_usage_client_is_stream(item: &StoredRequestUsageAudit) -> bool {
    admin_usage_request_path_implies_client_stream(item)
        .then_some(true)
        .or_else(|| {
            item.request_metadata
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|metadata| metadata.get("client_requested_stream"))
                .and_then(Value::as_bool)
        })
        .or_else(|| admin_usage_request_body_stream_flag(item))
        .or_else(|| admin_usage_headers_stream_flag(item.client_response_headers.as_ref()))
        .or_else(|| admin_usage_request_body_implies_default_non_stream(item).then_some(false))
        .or_else(|| admin_usage_infer_client_stream_from_captured_bodies(item))
        .unwrap_or(item.is_stream)
}

fn admin_usage_upstream_is_stream(item: &StoredRequestUsageAudit) -> bool {
    item.request_metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("upstream_is_stream"))
        .and_then(Value::as_bool)
        .or_else(|| admin_usage_headers_stream_flag(item.response_headers.as_ref()))
        .or_else(|| admin_usage_infer_upstream_stream_from_captured_bodies(item))
        .unwrap_or(item.is_stream)
}

fn admin_usage_metadata_string<'a>(
    item: &'a StoredRequestUsageAudit,
    key: &str,
) -> Option<&'a str> {
    item.request_metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn admin_usage_metadata_number(item: &StoredRequestUsageAudit, key: &str) -> Option<f64> {
    item.request_metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map(|value| round_to(value, 6))
}

fn infer_client_family_from_user_agent(user_agent: &str) -> Option<&'static str> {
    let normalized = user_agent.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.starts_with("codex_vscode") {
        return Some("codex_vscode");
    }
    if normalized.starts_with("codex") {
        return Some("codex");
    }
    if normalized.contains("claude-code") || normalized.contains("claude_code") {
        return Some("claude_code");
    }
    if normalized.contains("opencode") {
        return Some("opencode");
    }
    if normalized.contains("geminicli") || normalized.contains("gemini-cli") {
        return Some("gemini_cli");
    }
    if normalized.starts_with("openai/js") {
        return Some("openai_js_sdk");
    }
    None
}

pub fn admin_usage_client_family(item: &StoredRequestUsageAudit) -> Option<&str> {
    item.client_family
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            item.request_metadata
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|metadata| {
                    metadata
                        .get("client_session_affinity")
                        .and_then(Value::as_object)
                        .and_then(|affinity| affinity.get("client_family"))
                        .and_then(Value::as_str)
                        .or_else(|| metadata.get("client_family").and_then(Value::as_str))
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            admin_usage_metadata_string(item, "user_agent")
                .and_then(infer_client_family_from_user_agent)
        })
}

fn admin_usage_charge_breakdown(
    item: &StoredRequestUsageAudit,
    official_cost: f64,
    sales_multiplier: Option<f64>,
) -> AdminUsageChargeBreakdown {
    let package_debit_snapshot = item.settlement_package_debit_usd();
    let wallet_debit_snapshot = item.settlement_wallet_debit_usd();
    let has_debit_snapshot = package_debit_snapshot.is_some() || wallet_debit_snapshot.is_some();
    let package_debit = package_debit_snapshot.unwrap_or_default().max(0.0);
    let wallet_debit = wallet_debit_snapshot.unwrap_or_else(|| {
        if has_debit_snapshot {
            0.0
        } else {
            item.total_cost_usd.max(0.0)
        }
    });
    let user_debit = (package_debit + wallet_debit).max(0.0);
    let wallet_multiplier = sales_multiplier.or_else(|| {
        if official_cost > ADMIN_USAGE_COST_EPSILON && wallet_debit > ADMIN_USAGE_COST_EPSILON {
            Some(wallet_debit / official_cost)
        } else {
            None
        }
    });
    let package_multiplier = admin_usage_package_multiplier_from_split(
        official_cost,
        package_debit,
        wallet_debit,
        wallet_multiplier,
    );

    AdminUsageChargeBreakdown {
        payload: json!({
            "official_cost": round_to(official_cost.max(0.0), 6),
            "package_debit": round_to(package_debit, 6),
            "package_multiplier": package_multiplier,
            "wallet_debit": round_to(wallet_debit.max(0.0), 6),
            "wallet_multiplier": wallet_multiplier,
            "user_debit": round_to(user_debit, 6),
        }),
        user_debit_usd: user_debit,
    }
}

fn admin_usage_package_multiplier_from_split(
    official_cost: f64,
    package_debit: f64,
    wallet_debit: f64,
    wallet_multiplier: Option<f64>,
) -> Option<f64> {
    if package_debit <= ADMIN_USAGE_COST_EPSILON {
        return None;
    }
    let wallet_base = wallet_multiplier
        .filter(|value| value.is_finite() && *value > ADMIN_USAGE_COST_EPSILON)
        .map(|multiplier| wallet_debit.max(0.0) / multiplier)
        .unwrap_or(0.0);
    let package_base = (official_cost.max(0.0) - wallet_base).max(0.0);
    if package_base > ADMIN_USAGE_COST_EPSILON {
        Some(package_debit / package_base)
    } else {
        Some(1.0)
    }
}

fn admin_usage_active_request_json(
    item: &StoredRequestUsageAudit,
    api_key_name: Option<String>,
    provider_key_name: Option<String>,
    provider_key_account_label: Option<String>,
    image_progress: Option<&Value>,
) -> Value {
    let cache_creation_input_tokens = admin_usage_cache_creation_tokens(item);
    let client_is_stream = admin_usage_client_is_stream(item);
    let upstream_is_stream = admin_usage_upstream_is_stream(item);
    let official_cost = item
        .settlement_base_cost_usd()
        .unwrap_or(item.total_cost_usd);
    let sales_multiplier = item.settlement_sales_multiplier().or_else(|| {
        if official_cost > 0.0 {
            Some(item.total_cost_usd / official_cost)
        } else {
            None
        }
    });
    let charge_breakdown = admin_usage_charge_breakdown(item, official_cost, sales_multiplier);
    let mut value = json!({
        "id": item.id,
        "status": item.status,
        "input_tokens": item.input_tokens,
        "effective_input_tokens": admin_usage_effective_input_tokens(item),
        "output_tokens": item.output_tokens,
        "cache_creation_input_tokens": cache_creation_input_tokens,
        "cache_creation_ephemeral_5m_input_tokens": item.cache_creation_ephemeral_5m_input_tokens,
        "cache_creation_ephemeral_1h_input_tokens": item.cache_creation_ephemeral_1h_input_tokens,
        "cache_read_input_tokens": item.cache_read_input_tokens,
        "official_cost": round_to(official_cost, 6),
        "cost": round_to(charge_breakdown.user_debit_usd, 6),
        "sales_multiplier": sales_multiplier,
        "charge_breakdown": charge_breakdown.payload,
        "model_cost": admin_usage_metadata_number(item, "model_cost_usd"),
        "moderation_cost": admin_usage_metadata_number(item, "content_moderation_cost_usd"),
        "actual_cost": round_to(item.actual_total_cost_usd, 6),
        "actual_model_cost": admin_usage_metadata_number(item, "actual_model_cost_usd"),
        "actual_moderation_cost": admin_usage_metadata_number(item, "content_moderation_actual_cost_usd"),
        "rate_multiplier": item.settlement_rate_multiplier(),
        "response_time_ms": item.response_time_ms,
        "first_byte_time_ms": item.first_byte_time_ms,
        "status_code": item.status_code,
        "error_message": item.error_message,
        "provider": item.provider_name,
        "requested_reasoning_effort": item.requested_reasoning_effort(),
        "reasoning_effort": item.provider_reasoning_effort(),
        "api_key_name": api_key_name,
        "provider_key_name": provider_key_name,
        "provider_key_account_label": provider_key_account_label,
        "is_stream": item.is_stream,
        "upstream_is_stream": upstream_is_stream,
        "client_requested_stream": client_is_stream,
        "client_is_stream": client_is_stream,
        "client_family": admin_usage_client_family(item),
        "client_ip": admin_usage_metadata_string(item, "client_ip"),
        "user_agent": admin_usage_metadata_string(item, "user_agent"),
        "request_path": admin_usage_metadata_string(item, "request_path"),
        "request_path_and_query": admin_usage_metadata_string(item, "request_path_and_query"),
        "has_fallback": admin_usage_has_fallback(item),
    });
    if let Some(api_format) = item.api_format.as_ref() {
        value["api_format"] = json!(api_format);
    }
    if let Some(endpoint_api_format) = item.endpoint_api_format.as_ref() {
        value["endpoint_api_format"] = json!(endpoint_api_format);
    }
    value["codex_turn_state"] = item
        .request_metadata
        .as_ref()
        .and_then(|m| m.get("codex_turn_state"))
        .cloned()
        .unwrap_or(Value::Null);
    value["has_format_conversion"] = json!(item.has_format_conversion);
    if let Some(target_model) = item.target_model.as_ref() {
        value["target_model"] = json!(target_model);
    }
    if let Some(image_progress) = image_progress {
        value["image_progress"] = image_progress.clone();
    }
    value
}

pub fn admin_usage_record_json(
    item: &StoredRequestUsageAudit,
    users_by_id: &BTreeMap<String, StoredUserSummary>,
    api_key_names: &BTreeMap<String, String>,
    auth_user_reader_available: bool,
    auth_api_key_reader_available: bool,
    provider_key_name: Option<&str>,
    provider_key_account_label: Option<&str>,
) -> Value {
    let rate_multiplier = item.settlement_rate_multiplier();
    let input_price_per_1m = item.settlement_input_price_per_1m();
    let output_price_per_1m = item.settlement_output_price_per_1m();
    let cache_creation_price_per_1m = item.settlement_cache_creation_price_per_1m();
    let cache_read_price_per_1m = item.settlement_cache_read_price_per_1m();
    let official_cost = item
        .settlement_base_cost_usd()
        .unwrap_or(item.total_cost_usd);
    let sales_multiplier = item.settlement_sales_multiplier().or_else(|| {
        if official_cost > 0.0 {
            Some(item.total_cost_usd / official_cost)
        } else {
            None
        }
    });
    let charge_breakdown = admin_usage_charge_breakdown(item, official_cost, sales_multiplier);
    let cache_creation_input_tokens = admin_usage_cache_creation_tokens(item);
    let is_free_tier = item.settlement_is_free_tier();
    let user = item
        .user_id
        .as_ref()
        .and_then(|user_id| users_by_id.get(user_id));
    let username = admin_usage_username(item, users_by_id, auth_user_reader_available)
        .unwrap_or_else(|| {
            if item.user_id.is_some() {
                "已删除用户".to_string()
            } else {
                "未认证请求".to_string()
            }
        });
    let api_key_name = admin_usage_api_key_name(item, api_key_names, auth_api_key_reader_available);
    let user_email = user.and_then(|value| value.email.clone()).or_else(|| {
        if item.user_id.is_some() {
            Some("已删除用户".to_string())
        } else {
            None
        }
    });
    let client_is_stream = admin_usage_client_is_stream(item);
    let upstream_is_stream = admin_usage_upstream_is_stream(item);
    let user_debit_usd = charge_breakdown.user_debit_usd;
    let charge_breakdown_payload = charge_breakdown.payload;

    let mut payload = json!({
        "id": item.id,
        "user_id": item.user_id,
        "user_email": user_email,
        "username": username,
        "api_key": item.api_key_id.as_ref().map(|api_key_id| json!({
            "id": api_key_id,
            "name": api_key_name.clone(),
            "display": api_key_name.clone().unwrap_or_else(|| api_key_id.clone()),
        })),
        "provider": admin_usage_provider_display_for_record(item),
        "model": item.model,
        "target_model": item.target_model,
        "requested_reasoning_effort": item.requested_reasoning_effort(),
        "reasoning_effort": item.provider_reasoning_effort(),
        "input_tokens": item.input_tokens,
        "effective_input_tokens": admin_usage_effective_input_tokens(item),
        "output_tokens": item.output_tokens,
        "cache_creation_input_tokens": cache_creation_input_tokens,
        "cache_creation_ephemeral_5m_input_tokens": item.cache_creation_ephemeral_5m_input_tokens,
        "cache_creation_ephemeral_1h_input_tokens": item.cache_creation_ephemeral_1h_input_tokens,
        "cache_read_input_tokens": item.cache_read_input_tokens,
        "total_tokens": admin_usage_total_tokens(item),
        "actual_cost": round_to(item.actual_total_cost_usd, 6),
        "rate_multiplier": rate_multiplier,
        "response_time_ms": item.response_time_ms,
        "first_byte_time_ms": item.first_byte_time_ms,
        "created_at": unix_secs_to_rfc3339(item.created_at_unix_ms),
        "input_price_per_1m": input_price_per_1m,
        "output_price_per_1m": output_price_per_1m,
        "cache_creation_price_per_1m": cache_creation_price_per_1m,
        "cache_read_price_per_1m": cache_read_price_per_1m,
        "status_code": item.status_code,
        "error_message": item.error_message,
        "status": item.status,
        "has_fallback": admin_usage_has_fallback(item),
        "has_retry": false,
        "has_rectified": false,
        "is_free_tier": is_free_tier,
        "api_format": item.api_format,
        "endpoint_api_format": item.endpoint_api_format,
        "has_format_conversion": item.has_format_conversion,
        "api_key_name": api_key_name,
        "provider_key_name": provider_key_name,
        "provider_key_account_label": provider_key_account_label,
        "model_version": Value::Null,
    });
    let object = payload
        .as_object_mut()
        .expect("admin usage record payload should be an object");
    object.insert(
        "official_cost".to_string(),
        json!(round_to(official_cost, 6)),
    );
    object.insert(
        "codex_turn_state".into(),
        item.request_metadata
            .as_ref()
            .and_then(|m| m.get("codex_turn_state"))
            .cloned()
            .unwrap_or(Value::Null),
    );
    object.insert("cost".to_string(), json!(round_to(user_debit_usd, 6)));
    object.insert("sales_multiplier".to_string(), json!(sales_multiplier));
    object.insert("charge_breakdown".to_string(), charge_breakdown_payload);
    object.insert(
        "model_cost".to_string(),
        json!(admin_usage_metadata_number(item, "model_cost_usd")),
    );
    object.insert(
        "moderation_cost".to_string(),
        json!(admin_usage_metadata_number(
            item,
            "content_moderation_cost_usd"
        )),
    );
    object.insert(
        "actual_model_cost".to_string(),
        json!(admin_usage_metadata_number(item, "actual_model_cost_usd")),
    );
    object.insert(
        "actual_moderation_cost".to_string(),
        json!(admin_usage_metadata_number(
            item,
            "content_moderation_actual_cost_usd"
        )),
    );
    object.insert("is_stream".to_string(), json!(item.is_stream));
    object.insert("upstream_is_stream".to_string(), json!(upstream_is_stream));
    object.insert(
        "client_requested_stream".to_string(),
        json!(client_is_stream),
    );
    object.insert("client_is_stream".to_string(), json!(client_is_stream));
    maybe_insert_string_field(object, "client_family", admin_usage_client_family(item));
    maybe_insert_string_field(
        object,
        "client_ip",
        admin_usage_metadata_string(item, "client_ip"),
    );
    maybe_insert_string_field(
        object,
        "user_agent",
        admin_usage_metadata_string(item, "user_agent"),
    );
    maybe_insert_string_field(
        object,
        "request_path",
        admin_usage_metadata_string(item, "request_path"),
    );
    maybe_insert_string_field(
        object,
        "request_path_and_query",
        admin_usage_metadata_string(item, "request_path_and_query"),
    );
    payload
}

pub fn admin_usage_total_tokens(item: &StoredRequestUsageAudit) -> u64 {
    admin_usage_effective_input_tokens(item)
        .saturating_add(item.output_tokens)
        .saturating_add(admin_usage_cache_creation_tokens(item))
        .saturating_add(item.cache_read_input_tokens)
}

pub fn admin_usage_cache_creation_tokens(item: &StoredRequestUsageAudit) -> u64 {
    let classified = item
        .cache_creation_ephemeral_5m_input_tokens
        .saturating_add(item.cache_creation_ephemeral_1h_input_tokens);
    if item.cache_creation_input_tokens == 0 && classified > 0 {
        classified
    } else {
        item.cache_creation_input_tokens
    }
}

pub fn admin_usage_total_input_context(item: &StoredRequestUsageAudit) -> u64 {
    let api_format = item
        .endpoint_api_format
        .as_deref()
        .or(item.api_format.as_deref());
    let input_tokens = i64::try_from(item.input_tokens).unwrap_or(i64::MAX);
    let cache_creation_tokens =
        i64::try_from(admin_usage_cache_creation_tokens(item)).unwrap_or(i64::MAX);
    let cache_read_tokens = i64::try_from(item.cache_read_input_tokens).unwrap_or(i64::MAX);
    normalize_total_input_context_for_cache_hit_rate(
        api_format,
        input_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    ) as u64
}

pub fn admin_usage_effective_input_tokens(item: &StoredRequestUsageAudit) -> u64 {
    let api_format = item
        .endpoint_api_format
        .as_deref()
        .or(item.api_format.as_deref());
    let input_tokens = i64::try_from(item.input_tokens).unwrap_or(i64::MAX);
    let cache_read_tokens = i64::try_from(item.cache_read_input_tokens).unwrap_or(i64::MAX);
    normalize_input_tokens_for_billing(api_format, input_tokens, cache_read_tokens) as u64
}

pub fn admin_usage_token_cache_hit_rate(total_input_context: u64, cache_read_tokens: u64) -> f64 {
    if total_input_context == 0 {
        0.0
    } else {
        round_to(
            cache_read_tokens as f64 / total_input_context as f64 * 100.0,
            2,
        )
    }
}

fn admin_usage_provider_display_name(item: &StoredRequestUsageAudit) -> Option<String> {
    if admin_usage_provider_label_is_reserved(&item.provider_name) {
        None
    } else {
        Some(item.provider_name.clone())
    }
}

fn maybe_insert_string_field(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        object.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn admin_usage_routing_json(
    item: &StoredRequestUsageAudit,
    provider_key_name: Option<&str>,
) -> Value {
    let mut routing = serde_json::Map::new();
    maybe_insert_string_field(&mut routing, "candidate_id", item.routing_candidate_id());
    maybe_insert_u64_field(
        &mut routing,
        "candidate_index",
        item.routing_candidate_index(),
    );
    maybe_insert_string_field(
        &mut routing,
        "key_name",
        provider_key_name.or_else(|| item.routing_key_name()),
    );
    maybe_insert_string_field(&mut routing, "model_id", item.routing_model_id());
    maybe_insert_string_field(
        &mut routing,
        "global_model_id",
        item.routing_global_model_id(),
    );
    maybe_insert_string_field(
        &mut routing,
        "global_model_name",
        item.routing_global_model_name(),
    );
    maybe_insert_string_field(&mut routing, "planner_kind", item.routing_planner_kind());
    maybe_insert_string_field(&mut routing, "route_family", item.routing_route_family());
    maybe_insert_string_field(&mut routing, "route_kind", item.routing_route_kind());
    maybe_insert_string_field(
        &mut routing,
        "execution_path",
        item.routing_execution_path(),
    );
    maybe_insert_string_field(
        &mut routing,
        "local_execution_runtime_miss_reason",
        item.routing_local_execution_runtime_miss_reason(),
    );
    if routing.is_empty() {
        Value::Null
    } else {
        Value::Object(routing)
    }
}

pub fn admin_usage_aggregation_by_model_json(
    usage: &[StoredRequestUsageAudit],
    limit: usize,
) -> Value {
    #[allow(clippy::type_complexity)]
    let mut grouped: BTreeMap<
        String,
        (
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            f64,
            f64,
            f64,
        ),
    > = BTreeMap::new();
    for item in usage {
        let key = item.model.clone();
        let entry = grouped
            .entry(key)
            .or_insert((0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.0, 0.0, 0.0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(item.total_tokens);
        entry.2 = entry.2.saturating_add(item.input_tokens);
        entry.3 = entry.3.saturating_add(item.output_tokens);
        entry.4 = entry
            .4
            .saturating_add(admin_usage_effective_input_tokens(item));
        entry.5 = entry
            .5
            .saturating_add(admin_usage_total_input_context(item));
        entry.6 = entry
            .6
            .saturating_add(admin_usage_cache_creation_tokens(item));
        entry.7 = entry
            .7
            .saturating_add(item.cache_creation_ephemeral_5m_input_tokens);
        entry.8 = entry
            .8
            .saturating_add(item.cache_creation_ephemeral_1h_input_tokens);
        entry.9 = entry.9.saturating_add(item.cache_read_input_tokens);
        entry.10 += item
            .settlement_base_cost_usd()
            .unwrap_or(item.total_cost_usd);
        entry.11 += item.total_cost_usd;
        entry.12 += item.actual_total_cost_usd;
    }

    let mut items: Vec<Value> = grouped
        .into_iter()
        .map(
            |(
                model,
                (
                    request_count,
                    total_tokens,
                    _input_tokens,
                    output_tokens,
                    effective_input_tokens,
                    total_input_context,
                    cache_creation_tokens,
                    cache_creation_ephemeral_5m_tokens,
                    cache_creation_ephemeral_1h_tokens,
                    cache_read_tokens,
                    official_cost,
                    total_cost,
                    actual_cost,
                ),
            )| {
                json!({
                    "model": model,
                    "request_count": request_count,
                    "total_tokens": total_tokens,
                    "effective_input_tokens": effective_input_tokens,
                    "total_input_context": total_input_context,
                    "output_tokens": output_tokens,
                    "official_cost": round_to(official_cost, 6),
                    "total_cost": round_to(total_cost, 6),
                    "actual_cost": round_to(actual_cost, 6),
                    "cache_creation_tokens": cache_creation_tokens,
                    "cache_creation_ephemeral_5m_tokens": cache_creation_ephemeral_5m_tokens,
                    "cache_creation_ephemeral_1h_tokens": cache_creation_ephemeral_1h_tokens,
                    "cache_read_tokens": cache_read_tokens,
                    "cache_hit_rate": admin_usage_token_cache_hit_rate(
                        total_input_context,
                        cache_read_tokens,
                    ),
                })
            },
        )
        .collect();
    items.sort_by(|left, right| {
        right["request_count"]
            .as_u64()
            .unwrap_or_default()
            .cmp(&left["request_count"].as_u64().unwrap_or_default())
            .then_with(|| {
                left["model"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["model"].as_str().unwrap_or_default())
            })
    });
    items.truncate(limit);
    json!(items)
}

pub fn admin_usage_aggregation_by_provider_json(
    usage: &[StoredRequestUsageAudit],
    limit: usize,
) -> Value {
    #[allow(clippy::type_complexity)]
    let mut grouped: BTreeMap<
        String,
        (
            String,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            f64,
            f64,
            u64,
            u64,
        ),
    > = BTreeMap::new();
    for item in usage {
        let key = item
            .provider_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let provider_name =
            admin_usage_provider_display_name(item).unwrap_or_else(|| "Unknown".to_string());
        let entry = grouped.entry(key).or_insert((
            provider_name.clone(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0.0,
            0.0,
            0,
            0,
        ));
        if entry.0 == "Unknown" && provider_name != "Unknown" {
            entry.0 = provider_name;
        }
        entry.1 = entry.1.saturating_add(1);
        entry.2 = entry.2.saturating_add(item.total_tokens);
        entry.3 = entry.3.saturating_add(item.input_tokens);
        entry.4 = entry.4.saturating_add(item.output_tokens);
        entry.5 = entry
            .5
            .saturating_add(admin_usage_effective_input_tokens(item));
        entry.6 = entry
            .6
            .saturating_add(admin_usage_total_input_context(item));
        entry.7 = entry
            .7
            .saturating_add(admin_usage_cache_creation_tokens(item));
        entry.8 = entry
            .8
            .saturating_add(item.cache_creation_ephemeral_5m_input_tokens);
        entry.9 = entry
            .9
            .saturating_add(item.cache_creation_ephemeral_1h_input_tokens);
        entry.10 = entry.10.saturating_add(item.cache_read_input_tokens);
        entry.11 += item.total_cost_usd;
        entry.12 += item.actual_total_cost_usd;
        entry.13 = entry
            .13
            .saturating_add(item.response_time_ms.unwrap_or_default());
        entry.14 = entry
            .14
            .saturating_add(if admin_usage_is_success(item) { 1 } else { 0 });
    }

    let mut items: Vec<Value> = grouped
        .into_iter()
        .map(
            |(
                provider_id,
                (
                    provider_name,
                    request_count,
                    total_tokens,
                    _input_tokens,
                    output_tokens,
                    effective_input_tokens,
                    total_input_context,
                    cache_creation_tokens,
                    cache_creation_ephemeral_5m_tokens,
                    cache_creation_ephemeral_1h_tokens,
                    cache_read_tokens,
                    total_cost,
                    actual_cost,
                    response_time_ms_sum,
                    success_count,
                ),
            )| {
                let avg_response_time_ms = if request_count == 0 {
                    0.0
                } else {
                    round_to(response_time_ms_sum as f64 / request_count as f64, 2)
                };
                let error_count = request_count.saturating_sub(success_count);
                let success_rate = if request_count == 0 {
                    0.0
                } else {
                    round_to(success_count as f64 / request_count as f64 * 100.0, 2)
                };
                json!({
                    "provider_id": provider_id,
                    "provider": provider_name,
                    "request_count": request_count,
                    "total_tokens": total_tokens,
                    "effective_input_tokens": effective_input_tokens,
                    "total_input_context": total_input_context,
                    "output_tokens": output_tokens,
                    "total_cost": round_to(total_cost, 6),
                    "actual_cost": round_to(actual_cost, 6),
                    "avg_response_time_ms": avg_response_time_ms,
                    "success_rate": success_rate,
                    "error_count": error_count,
                    "cache_creation_tokens": cache_creation_tokens,
                    "cache_creation_ephemeral_5m_tokens": cache_creation_ephemeral_5m_tokens,
                    "cache_creation_ephemeral_1h_tokens": cache_creation_ephemeral_1h_tokens,
                    "cache_read_tokens": cache_read_tokens,
                    "cache_hit_rate": admin_usage_token_cache_hit_rate(
                        total_input_context,
                        cache_read_tokens,
                    ),
                })
            },
        )
        .collect();
    items.sort_by(|left, right| {
        right["request_count"]
            .as_u64()
            .unwrap_or_default()
            .cmp(&left["request_count"].as_u64().unwrap_or_default())
            .then_with(|| {
                left["provider_id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["provider_id"].as_str().unwrap_or_default())
            })
    });
    items.truncate(limit);
    json!(items)
}

pub fn admin_usage_aggregation_by_api_format_json(
    usage: &[StoredRequestUsageAudit],
    limit: usize,
) -> Value {
    #[allow(clippy::type_complexity)]
    let mut grouped: BTreeMap<
        String,
        (
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            f64,
            f64,
            u64,
        ),
    > = BTreeMap::new();
    for item in usage {
        let key = item
            .api_format
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let entry = grouped
            .entry(key)
            .or_insert((0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.0, 0.0, 0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(item.total_tokens);
        entry.2 = entry.2.saturating_add(item.input_tokens);
        entry.3 = entry.3.saturating_add(item.output_tokens);
        entry.4 = entry
            .4
            .saturating_add(admin_usage_effective_input_tokens(item));
        entry.5 = entry
            .5
            .saturating_add(admin_usage_total_input_context(item));
        entry.6 = entry
            .6
            .saturating_add(admin_usage_cache_creation_tokens(item));
        entry.7 = entry
            .7
            .saturating_add(item.cache_creation_ephemeral_5m_input_tokens);
        entry.8 = entry
            .8
            .saturating_add(item.cache_creation_ephemeral_1h_input_tokens);
        entry.9 = entry.9.saturating_add(item.cache_read_input_tokens);
        entry.10 += item.total_cost_usd;
        entry.11 += item.actual_total_cost_usd;
        entry.12 = entry
            .12
            .saturating_add(item.response_time_ms.unwrap_or_default());
    }

    let mut items: Vec<Value> = grouped
        .into_iter()
        .map(
            |(
                api_format,
                (
                    request_count,
                    total_tokens,
                    _input_tokens,
                    output_tokens,
                    effective_input_tokens,
                    total_input_context,
                    cache_creation_tokens,
                    cache_creation_ephemeral_5m_tokens,
                    cache_creation_ephemeral_1h_tokens,
                    cache_read_tokens,
                    total_cost,
                    actual_cost,
                    response_time_ms_sum,
                ),
            )| {
                let avg_response_time_ms = if request_count == 0 {
                    0.0
                } else {
                    round_to(response_time_ms_sum as f64 / request_count as f64, 2)
                };
                json!({
                    "api_format": api_format,
                    "request_count": request_count,
                    "total_tokens": total_tokens,
                    "effective_input_tokens": effective_input_tokens,
                    "total_input_context": total_input_context,
                    "output_tokens": output_tokens,
                    "total_cost": round_to(total_cost, 6),
                    "actual_cost": round_to(actual_cost, 6),
                    "avg_response_time_ms": avg_response_time_ms,
                    "cache_creation_tokens": cache_creation_tokens,
                    "cache_creation_ephemeral_5m_tokens": cache_creation_ephemeral_5m_tokens,
                    "cache_creation_ephemeral_1h_tokens": cache_creation_ephemeral_1h_tokens,
                    "cache_read_tokens": cache_read_tokens,
                    "cache_hit_rate": admin_usage_token_cache_hit_rate(
                        total_input_context,
                        cache_read_tokens,
                    ),
                })
            },
        )
        .collect();
    items.sort_by(|left, right| {
        right["request_count"]
            .as_u64()
            .unwrap_or_default()
            .cmp(&left["request_count"].as_u64().unwrap_or_default())
            .then_with(|| {
                left["api_format"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["api_format"].as_str().unwrap_or_default())
            })
    });
    items.truncate(limit);
    json!(items)
}

pub fn admin_usage_heatmap_json(usage: &[StoredRequestUsageAudit]) -> Value {
    let today = chrono::Utc::now().date_naive();
    let start_date = today
        .checked_sub_signed(chrono::Duration::days(364))
        .unwrap_or(today);
    let mut grouped: BTreeMap<chrono::NaiveDate, (u64, u64, f64, f64)> = BTreeMap::new();
    for item in usage {
        let Ok(created_at_unix_ms) = i64::try_from(item.created_at_unix_ms) else {
            continue;
        };
        let Some(created_at) =
            chrono::DateTime::<chrono::Utc>::from_timestamp(created_at_unix_ms, 0)
        else {
            continue;
        };
        let date_key = created_at.date_naive();
        if date_key < start_date || date_key > today {
            continue;
        }
        let entry = grouped.entry(date_key).or_insert((0, 0, 0.0, 0.0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(admin_usage_total_tokens(item));
        entry.2 += item.total_cost_usd;
        entry.3 += item.actual_total_cost_usd;
    }

    let mut max_requests = 0_u64;
    let mut cursor = start_date;
    let mut days = Vec::new();
    while cursor <= today {
        let (requests, total_tokens, total_cost, actual_total_cost) =
            grouped.get(&cursor).copied().unwrap_or((0, 0, 0.0, 0.0));
        max_requests = max_requests.max(requests);
        days.push(json!({
            "date": cursor.to_string(),
            "requests": requests,
            "total_tokens": total_tokens,
            "total_cost": round_to(total_cost, 6),
            "actual_total_cost": round_to(actual_total_cost, 6),
        }));
        cursor = cursor
            .checked_add_signed(chrono::Duration::days(1))
            .unwrap_or(today + chrono::Duration::days(1));
    }

    json!({
        "start_date": start_date.to_string(),
        "end_date": today.to_string(),
        "total_days": days.len(),
        "max_requests": max_requests,
        "days": days,
    })
}

pub fn admin_usage_is_success(item: &StoredRequestUsageAudit) -> bool {
    matches!(
        item.status.as_str(),
        "completed" | "success" | "ok" | "billed" | "settled"
    ) && item
        .status_code
        .is_none_or(|code| (200..300).contains(&code))
}

pub fn admin_usage_matches_optional_id(value: Option<&str>, expected: Option<&str>) -> bool {
    let Some(expected) = expected.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    value.is_some_and(|candidate| candidate == expected)
}

pub fn admin_usage_group_completed_by_user(
    items: &[StoredRequestUsageAudit],
) -> BTreeMap<String, Vec<StoredRequestUsageAudit>> {
    let mut grouped = BTreeMap::new();
    for item in items.iter().filter(|item| item.user_id.is_some()) {
        grouped
            .entry(item.user_id.clone().unwrap_or_default())
            .or_insert_with(Vec::new)
            .push(item.clone());
    }
    grouped
}

pub fn admin_usage_group_completed_by_api_key(
    items: &[StoredRequestUsageAudit],
    api_key_id: Option<&str>,
) -> BTreeMap<String, Vec<StoredRequestUsageAudit>> {
    let mut grouped = BTreeMap::new();
    for item in items.iter().filter(|item| item.api_key_id.is_some()) {
        if !admin_usage_matches_optional_id(item.api_key_id.as_deref(), api_key_id) {
            continue;
        }
        grouped
            .entry(item.api_key_id.clone().unwrap_or_default())
            .or_insert_with(Vec::new)
            .push(item.clone());
    }
    grouped
}

pub fn admin_usage_collect_request_intervals_minutes(
    items: &[StoredRequestUsageAudit],
) -> Vec<f64> {
    let mut previous_created_at_unix_ms = None;
    let mut intervals = Vec::new();
    for item in items {
        if let Some(previous) = previous_created_at_unix_ms {
            intervals.push(item.created_at_unix_ms.saturating_sub(previous) as f64 / 60.0);
        }
        previous_created_at_unix_ms = Some(item.created_at_unix_ms);
    }
    intervals
}

pub fn admin_usage_percentile_cont(values: &[f64], percentile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    if values.len() == 1 {
        return Some(values[0]);
    }
    let position = percentile.clamp(0.0, 1.0) * (values.len() - 1) as f64;
    let lower_index = position.floor() as usize;
    let upper_index = position.ceil() as usize;
    let lower = values[lower_index];
    let upper = values[upper_index];
    Some(lower + (upper - lower) * (position - lower_index as f64))
}

pub fn admin_usage_calculate_recommended_ttl(
    p75_interval: Option<f64>,
    p90_interval: Option<f64>,
) -> u64 {
    let Some(p75_interval) = p75_interval else {
        return 5;
    };
    let Some(p90_interval) = p90_interval else {
        return 5;
    };

    if p90_interval <= 5.0 {
        5
    } else if p75_interval <= 15.0 {
        15
    } else if p75_interval <= 30.0 {
        30
    } else {
        60
    }
}

pub fn admin_usage_ttl_recommendation_reason(
    ttl: u64,
    p75_interval: Option<f64>,
    p90_interval: Option<f64>,
) -> String {
    let Some(p75_interval) = p75_interval else {
        return "数据不足，使用默认值".to_string();
    };
    let Some(p90_interval) = p90_interval else {
        return "数据不足，使用默认值".to_string();
    };

    match ttl {
        5 => format!("高频用户：90% 的请求间隔在 {:.1} 分钟内", p90_interval),
        15 => format!("中高频用户：75% 的请求间隔在 {:.1} 分钟内", p75_interval),
        30 => format!("中频用户：75% 的请求间隔在 {:.1} 分钟内", p75_interval),
        _ => format!(
            "低频用户：75% 的请求间隔为 {:.1} 分钟，建议使用长 TTL",
            p75_interval
        ),
    }
}

pub fn admin_usage_proportional_limits(
    grouped: &BTreeMap<String, Vec<Value>>,
    limit: usize,
    total_points: usize,
) -> BTreeMap<String, usize> {
    let mut limits = BTreeMap::new();
    for (group_id, items) in grouped {
        let computed = if total_points <= limit || total_points == 0 {
            items.len()
        } else {
            let scaled =
                ((items.len() as f64 * limit as f64) / total_points as f64).ceil() as usize;
            std::cmp::max(scaled, 1)
        };
        limits.insert(group_id.clone(), computed);
    }
    limits
}

pub fn admin_usage_point_sort_key(left: &Value, right: &Value) -> std::cmp::Ordering {
    left["x"]
        .as_str()
        .unwrap_or_default()
        .cmp(right["x"].as_str().unwrap_or_default())
        .then_with(|| {
            left["user_id"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["user_id"].as_str().unwrap_or_default())
        })
}

fn admin_usage_id_from_path_suffix(request_path: &str, suffix: Option<&str>) -> Option<String> {
    let mut value = request_path
        .strip_prefix("/api/admin/usage/")?
        .trim()
        .trim_matches('/')
        .to_string();
    if let Some(suffix) = suffix {
        value = value.strip_suffix(suffix)?.trim_matches('/').to_string();
    }
    if value.is_empty() || value.contains('/') {
        None
    } else {
        Some(value)
    }
}

pub fn admin_usage_id_from_detail_path(request_path: &str) -> Option<String> {
    admin_usage_id_from_path_suffix(request_path, None)
}

pub fn admin_usage_id_from_action_path(request_path: &str, action: &str) -> Option<String> {
    admin_usage_id_from_path_suffix(request_path, Some(action))
}

fn admin_usage_curl_shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn admin_usage_resolve_replay_mode(same_provider: bool, same_endpoint: bool) -> &'static str {
    if same_provider && same_endpoint {
        "same_endpoint_reuse"
    } else if same_provider {
        "same_provider_remap"
    } else {
        "cross_provider_remap"
    }
}

pub fn admin_usage_resolve_request_capture_body(
    item: &StoredRequestUsageAudit,
    body_override: Option<Value>,
) -> Option<Value> {
    let resolved_model = item.model.clone();
    let client_is_stream = admin_usage_client_is_stream(item);
    let mut request_body = body_override.or_else(|| item.request_body.clone())?;
    if let Some(body) = request_body.as_object_mut() {
        body.entry("model".to_string())
            .or_insert_with(|| json!(resolved_model));
        if !body.contains_key("stream") {
            body.insert("stream".to_string(), json!(client_is_stream));
        }
        if let Some(target_model) = item
            .target_model
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.entry("target_model".to_string())
                .or_insert_with(|| json!(target_model));
        }
        if let Some(request_type) = item
            .request_type
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.entry("request_type".to_string())
                .or_insert_with(|| json!(request_type));
        }
        if let Some(api_format) = item
            .api_format
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.entry("api_format".to_string())
                .or_insert_with(|| json!(api_format));
        }
    }
    Some(request_body)
}

pub fn admin_usage_headers_from_value(value: &Value) -> Option<BTreeMap<String, String>> {
    let object = value.as_object()?;
    Some(BTreeMap::from_iter(object.iter().filter_map(
        |(key, value)| {
            if value.is_null() {
                return None;
            }
            let value = value
                .as_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| value.to_string());
            Some((key.clone(), value))
        },
    )))
}

pub fn admin_usage_curl_headers() -> BTreeMap<String, String> {
    BTreeMap::from([("Content-Type".to_string(), "application/json".to_string())])
}

pub fn admin_usage_build_curl_command(
    url: Option<&str>,
    headers: &BTreeMap<String, String>,
    body: Option<&Value>,
) -> String {
    let mut parts = vec!["curl".to_string()];
    if let Some(url) = url {
        parts.push(admin_usage_curl_shell_quote(url));
    }
    parts.push("-X POST".to_string());
    for (key, value) in headers {
        parts.push(format!(
            "-H {}",
            admin_usage_curl_shell_quote(&format!("{key}: {value}"))
        ));
    }
    if let Some(body) = body {
        parts.push(format!(
            "-d {}",
            admin_usage_curl_shell_quote(&body.to_string())
        ));
    }
    parts.join(" \\\n  ")
}

fn summarize_admin_usage_stats(usage: &[StoredRequestUsageAudit]) -> StoredUsageAuditSummary {
    let aggregate = aggregate_usage_stats(usage);
    StoredUsageAuditSummary {
        total_requests: aggregate.total_requests,
        input_tokens: usage.iter().map(|item| item.input_tokens).sum(),
        output_tokens: usage.iter().map(|item| item.output_tokens).sum(),
        recorded_total_tokens: aggregate.total_tokens,
        cache_creation_tokens: usage.iter().map(admin_usage_cache_creation_tokens).sum(),
        cache_creation_ephemeral_5m_tokens: usage
            .iter()
            .map(|item| item.cache_creation_ephemeral_5m_input_tokens)
            .sum(),
        cache_creation_ephemeral_1h_tokens: usage
            .iter()
            .map(|item| item.cache_creation_ephemeral_1h_input_tokens)
            .sum(),
        cache_read_tokens: usage.iter().map(|item| item.cache_read_input_tokens).sum(),
        total_cost_usd: aggregate.total_cost,
        actual_total_cost_usd: aggregate.actual_total_cost,
        cache_creation_cost_usd: usage.iter().map(|item| item.cache_creation_cost_usd).sum(),
        cache_read_cost_usd: usage.iter().map(|item| item.cache_read_cost_usd).sum(),
        total_response_time_ms: aggregate.total_response_time_ms,
        error_requests: aggregate.error_requests,
    }
}

pub fn build_admin_usage_summary_stats_response_from_summary(
    summary: &StoredUsageAuditSummary,
) -> Response<Body> {
    let total_tokens = summary
        .input_tokens
        .saturating_add(summary.output_tokens)
        .saturating_add(summary.cache_creation_tokens)
        .saturating_add(summary.cache_read_tokens);
    let avg_response_time = if summary.total_requests == 0 {
        0.0
    } else {
        round_to(
            summary.total_response_time_ms / summary.total_requests as f64 / 1000.0,
            2,
        )
    };
    let error_rate = if summary.total_requests == 0 {
        0.0
    } else {
        round_to(
            (summary.error_requests as f64 / summary.total_requests as f64) * 100.0,
            2,
        )
    };

    Json(json!({
        "total_requests": summary.total_requests,
        "total_tokens": total_tokens,
        "total_cost": round_to(summary.total_cost_usd, 6),
        "total_actual_cost": round_to(summary.actual_total_cost_usd, 6),
        "avg_response_time": avg_response_time,
        "error_count": summary.error_requests,
        "error_rate": error_rate,
        "cache_stats": {
            "cache_creation_tokens": summary.cache_creation_tokens,
            "cache_creation_ephemeral_5m_tokens": summary.cache_creation_ephemeral_5m_tokens,
            "cache_creation_ephemeral_1h_tokens": summary.cache_creation_ephemeral_1h_tokens,
            "cache_read_tokens": summary.cache_read_tokens,
            "cache_creation_cost": round_to(summary.cache_creation_cost_usd, 6),
            "cache_read_cost": round_to(summary.cache_read_cost_usd, 6),
        }
    }))
    .into_response()
}

pub fn build_admin_usage_summary_stats_response(
    usage: &[StoredRequestUsageAudit],
) -> Response<Body> {
    build_admin_usage_summary_stats_response_from_summary(&summarize_admin_usage_stats(usage))
}

#[allow(clippy::too_many_arguments)] // response assembly keeps each precomputed lookup explicit
pub fn build_admin_usage_active_requests_response(
    items: &[StoredRequestUsageAudit],
    api_key_names: &BTreeMap<String, String>,
    auth_api_key_reader_available: bool,
    provider_key_names: &BTreeMap<String, String>,
    provider_key_account_labels: &BTreeMap<String, String>,
    fallback_flags_by_usage_id: &BTreeMap<String, bool>,
    provider_routes_by_usage_id: &BTreeMap<String, Vec<String>>,
    image_progress_by_request_id: &BTreeMap<String, Value>,
) -> Response<Body> {
    let payload: Vec<_> = items
        .iter()
        .map(|item| {
            let provider_key_name = admin_usage_provider_key_name(item, provider_key_names);
            let provider_key_account_label =
                admin_usage_provider_key_account_label(item, provider_key_account_labels);
            let api_key_name =
                admin_usage_api_key_name(item, api_key_names, auth_api_key_reader_available);
            let mut value = admin_usage_active_request_json(
                item,
                api_key_name,
                provider_key_name,
                provider_key_account_label,
                image_progress_by_request_id.get(&item.request_id),
            );
            if let Some(has_fallback) = fallback_flags_by_usage_id.get(&item.id) {
                value["has_fallback"] = json!(has_fallback);
            }
            if let Some(provider_route) = provider_routes_by_usage_id.get(&item.id) {
                value["provider_route"] = json!(provider_route);
                admin_usage_apply_provider_route_display(&mut value, provider_route);
            }
            value
        })
        .collect();

    Json(json!({ "requests": payload })).into_response()
}

#[allow(clippy::too_many_arguments)]
pub fn build_admin_usage_records_response(
    items: &[StoredRequestUsageAudit],
    users_by_id: &BTreeMap<String, StoredUserSummary>,
    api_key_names: &BTreeMap<String, String>,
    auth_user_reader_available: bool,
    auth_api_key_reader_available: bool,
    provider_key_names: &BTreeMap<String, String>,
    provider_key_account_labels: &BTreeMap<String, String>,
    total: usize,
    limit: usize,
    offset: usize,
) -> Response<Body> {
    let records: Vec<_> = items
        .iter()
        .map(|item| {
            let provider_key_name = admin_usage_provider_key_name(item, provider_key_names);
            let provider_key_account_label =
                admin_usage_provider_key_account_label(item, provider_key_account_labels);
            admin_usage_record_json(
                item,
                users_by_id,
                api_key_names,
                auth_user_reader_available,
                auth_api_key_reader_available,
                provider_key_name.as_deref(),
                provider_key_account_label.as_deref(),
            )
        })
        .collect();

    Json(json!({
        "records": records,
        "total": total,
        "limit": limit,
        "offset": offset,
    }))
    .into_response()
}

pub fn build_admin_usage_curl_response(
    item: &StoredRequestUsageAudit,
    url: Option<String>,
    headers_json: Option<Value>,
    headers: &BTreeMap<String, String>,
    body: Option<&Value>,
) -> Response<Body> {
    let curl = admin_usage_build_curl_command(url.as_deref(), headers, body);
    Json(json!({
        "url": url,
        "method": "POST",
        "headers": headers_json.unwrap_or_else(|| json!(headers.clone())),
        "body": body.cloned().unwrap_or(Value::Null),
        "curl": curl,
        "body_capture": admin_usage_curl_body_capture_json(item),
        "captured_request_body_available": admin_usage_has_body_value(
            item,
            item.request_body.as_ref(),
            UsageBodyField::RequestBody
        )
            || admin_usage_has_body_value(
                item,
                item.provider_request_body.as_ref(),
                UsageBodyField::ProviderRequestBody
            ),
    }))
    .into_response()
}

#[allow(clippy::too_many_arguments)]
pub fn build_admin_usage_detail_payload(
    item: &StoredRequestUsageAudit,
    users_by_id: &BTreeMap<String, StoredUserSummary>,
    api_key_names: &BTreeMap<String, String>,
    auth_user_reader_available: bool,
    auth_api_key_reader_available: bool,
    provider_key_name: Option<&str>,
    provider_key_account_label: Option<&str>,
    include_bodies: bool,
    request_body: Option<Value>,
    default_headers: &BTreeMap<String, String>,
) -> Value {
    let mut payload = admin_usage_record_json(
        item,
        users_by_id,
        api_key_names,
        auth_user_reader_available,
        auth_api_key_reader_available,
        provider_key_name,
        provider_key_account_label,
    );
    let mut metadata = match item.request_metadata.clone() {
        Some(Value::Object(object)) => Value::Object(object),
        Some(value) => json!({ "request_metadata": value }),
        None => json!({}),
    };
    if let Some(object) = metadata.as_object_mut() {
        admin_usage_strip_body_ref_metadata(object);
        admin_usage_strip_routing_metadata(object);
        admin_usage_strip_settlement_metadata(object);
        admin_usage_strip_trace_metadata(object);
    }
    payload["user"] = match item.user_id.as_ref() {
        Some(user_id) => json!({
            "id": user_id,
            "email": payload["user_email"].clone(),
            "username": payload["username"].clone(),
        }),
        None => Value::Null,
    };
    payload["request_id"] = json!(item.request_id);
    payload["billing_status"] = json!(item.billing_status);
    payload["request_type"] = json!(item.request_type);
    payload["provider_id"] = json!(item.provider_id);
    payload["provider_endpoint_id"] = json!(item.provider_endpoint_id);
    payload["provider_api_key_id"] = json!(item.provider_api_key_id);
    payload["error_category"] = json!(item.error_category);
    payload["cache_creation_cost"] = json!(round_to(item.cache_creation_cost_usd, 6));
    payload["cache_read_cost"] = json!(round_to(item.cache_read_cost_usd, 6));
    payload["request_cost"] = json!(round_to(item.total_cost_usd, 6));
    payload["request_headers"] = item
        .request_headers
        .clone()
        .unwrap_or_else(|| json!(default_headers.clone()));
    payload["provider_request_headers"] = item
        .provider_request_headers
        .clone()
        .unwrap_or_else(|| json!(default_headers.clone()));
    payload["response_headers"] = item.response_headers.clone().unwrap_or(Value::Null);
    payload["client_response_headers"] =
        item.client_response_headers.clone().unwrap_or(Value::Null);
    payload["metadata"] = metadata;
    payload["routing"] = admin_usage_routing_json(item, provider_key_name);
    payload["body_capture"] = admin_usage_body_capture_json(item);
    payload["settlement"] = admin_usage_settlement_json(item);
    payload["trace"] = admin_usage_trace_json(item);
    let error_domains = admin_usage_error_domains_json(item);
    let error_flow = admin_usage_error_flow_json(item, &error_domains);
    payload["errors"] = error_domains.clone();
    payload["request_error"] = error_domains["request_error"].clone();
    payload["upstream_error"] = error_domains["upstream_error"].clone();
    payload["client_error"] = error_domains["client_error"].clone();
    payload["failure_summary"] = error_domains["failure_summary"].clone();
    payload["scheduling_failure"] = admin_usage_scheduling_failure_json(
        item,
        &error_domains["client_error"],
        &error_domains["upstream_error"],
    );
    payload["error_flow"] = error_flow;
    payload["has_request_body"] = json!(admin_usage_has_body_value(
        item,
        item.request_body.as_ref(),
        UsageBodyField::RequestBody
    ));
    payload["has_provider_request_body"] = json!(admin_usage_has_body_value(
        item,
        item.provider_request_body.as_ref(),
        UsageBodyField::ProviderRequestBody
    ));
    payload["has_response_body"] = json!(admin_usage_has_body_value(
        item,
        item.response_body.as_ref(),
        UsageBodyField::ResponseBody
    ));
    payload["has_client_response_body"] = json!(admin_usage_has_body_value(
        item,
        item.client_response_body.as_ref(),
        UsageBodyField::ClientResponseBody
    ));
    payload["tiered_pricing"] = Value::Null;
    if include_bodies {
        payload["request_body"] = request_body.unwrap_or(Value::Null);
        payload["provider_request_body"] =
            item.provider_request_body.clone().unwrap_or(Value::Null);
        payload["response_body"] = item.response_body.clone().unwrap_or(Value::Null);
        payload["client_response_body"] = item.client_response_body.clone().unwrap_or(Value::Null);
    } else {
        payload["request_body"] = Value::Null;
        payload["provider_request_body"] = Value::Null;
        payload["response_body"] = Value::Null;
        payload["client_response_body"] = Value::Null;
    }
    payload
}

#[allow(clippy::too_many_arguments)]
pub fn build_admin_usage_replay_plan_response(
    item: &StoredRequestUsageAudit,
    target_provider: &StoredProviderCatalogProvider,
    target_endpoint: &StoredProviderCatalogEndpoint,
    target_api_key_id: Option<String>,
    request_body: Option<Value>,
    url: &str,
    headers: &BTreeMap<String, String>,
    same_provider: bool,
    same_endpoint: bool,
) -> Response<Body> {
    let resolved_model = item.model.clone();
    let mapping_source = "none";
    let curl = admin_usage_build_curl_command(Some(url), headers, request_body.as_ref());

    Json(json!({
        "dry_run": true,
        "usage_id": item.id,
        "request_id": item.request_id,
        "mode": admin_usage_resolve_replay_mode(same_provider, same_endpoint),
        "target_provider_id": target_provider.id,
        "target_provider_name": target_provider.name,
        "target_endpoint_id": target_endpoint.id,
        "target_api_key_id": target_api_key_id,
        "target_api_format": target_endpoint.api_format,
        "resolved_model": resolved_model,
        "mapping_source": mapping_source,
        "method": "POST",
        "url": url,
        "request_headers": headers,
        "request_body": request_body.clone().unwrap_or(Value::Null),
        "body_capture": admin_usage_replay_body_capture_json(item),
        "captured_request_body_available": admin_usage_has_body_value(
            item,
            item.request_body.as_ref(),
            UsageBodyField::RequestBody
        ),
        "request_body_available": request_body.is_some(),
        "note": "Rust local replay currently exposes a dry-run plan and does not dispatch upstream",
        "curl": curl,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{
        admin_usage_active_request_json as raw_admin_usage_active_request_json,
        admin_usage_client_is_stream, admin_usage_has_body_value, admin_usage_has_fallback,
        admin_usage_is_failed, admin_usage_is_success, admin_usage_matches_search,
        admin_usage_matches_status, admin_usage_matches_username,
        admin_usage_record_json as raw_admin_usage_record_json,
        admin_usage_resolve_request_capture_body, admin_usage_total_tokens,
        admin_usage_upstream_is_stream,
        build_admin_usage_detail_payload as raw_build_admin_usage_detail_payload,
    };
    use aether_data_contracts::repository::{
        candidates::{RequestCandidateStatus, StoredRequestCandidate},
        usage::{StoredRequestUsageAudit, UsageBodyField},
    };

    fn admin_usage_active_request_json(
        item: &StoredRequestUsageAudit,
        api_key_name: Option<String>,
        provider_key_name: Option<String>,
        image_progress: Option<&serde_json::Value>,
    ) -> serde_json::Value {
        raw_admin_usage_active_request_json(
            item,
            api_key_name,
            provider_key_name,
            None,
            image_progress,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn admin_usage_record_json(
        item: &StoredRequestUsageAudit,
        users_by_id: &BTreeMap<String, aether_data::repository::users::StoredUserSummary>,
        api_key_names: &BTreeMap<String, String>,
        auth_user_reader_available: bool,
        auth_api_key_reader_available: bool,
        provider_key_name: Option<&str>,
    ) -> serde_json::Value {
        raw_admin_usage_record_json(
            item,
            users_by_id,
            api_key_names,
            auth_user_reader_available,
            auth_api_key_reader_available,
            provider_key_name,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_admin_usage_detail_payload(
        item: &StoredRequestUsageAudit,
        users_by_id: &BTreeMap<String, aether_data::repository::users::StoredUserSummary>,
        api_key_names: &BTreeMap<String, String>,
        auth_user_reader_available: bool,
        auth_api_key_reader_available: bool,
        provider_key_name: Option<&str>,
        include_bodies: bool,
        request_body: Option<serde_json::Value>,
        default_headers: &BTreeMap<String, String>,
    ) -> serde_json::Value {
        raw_build_admin_usage_detail_payload(
            item,
            users_by_id,
            api_key_names,
            auth_user_reader_available,
            auth_api_key_reader_available,
            provider_key_name,
            None,
            include_bodies,
            request_body,
            default_headers,
        )
    }

    fn sample_usage(
        status: &str,
        status_code: Option<i32>,
        error_message: Option<&str>,
    ) -> StoredRequestUsageAudit {
        StoredRequestUsageAudit::new(
            "usage-1".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            "OpenAI".to_string(),
            "gpt-5".to_string(),
            None,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            false,
            false,
            10,
            20,
            30,
            0.0,
            0.0,
            status_code,
            error_message.map(ToOwned::to_owned),
            None,
            Some(120),
            None,
            status.to_string(),
            "settled".to_string(),
            100,
            101,
            Some(102),
        )
        .expect("usage should build")
    }

    #[test]
    fn explicit_completed_status_wins_over_legacy_failure_fields() {
        let item = sample_usage(
            "completed",
            Some(429),
            Some("rate limited on first attempt"),
        );
        assert!(!admin_usage_is_failed(&item));
        assert!(!admin_usage_matches_status(&item, Some("failed")));
        assert!(admin_usage_matches_status(&item, Some("completed")));
    }

    #[test]
    fn completed_http_200_detail_does_not_emit_failure_summary() {
        let item = StoredRequestUsageAudit {
            response_headers: Some(json!({
                "content-type": "application/json"
            })),
            response_body: Some(json!({
                "id": "resp-1",
                "object": "response",
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "content": [
                            { "type": "output_text", "text": "ok" }
                        ]
                    }
                ]
            })),
            client_response_headers: Some(json!({
                "content-type": "application/json"
            })),
            client_response_body: Some(json!({
                "id": "resp-1",
                "object": "response",
                "status": "completed"
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            true,
            Some(json!({"model": "gpt-5.5", "stream": true})),
            &BTreeMap::new(),
        );

        assert!(payload["upstream_error"].is_null());
        assert!(payload["client_error"].is_null());
        assert!(payload["failure_summary"].is_null());
        assert!(payload["error_flow"].is_null());
    }

    #[test]
    fn client_requested_stream_prefers_request_metadata_flag() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            request_metadata: Some(json!({
                "client_requested_stream": false
            })),
            ..sample_usage("completed", Some(200), None)
        };

        assert!(!admin_usage_client_is_stream(&item));

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        assert_eq!(record["is_stream"], true);
        assert_eq!(record["upstream_is_stream"], true);
        assert_eq!(record["client_requested_stream"], false);
        assert_eq!(record["client_is_stream"], false);
    }

    #[test]
    fn admin_usage_record_labels_local_runtime_miss_without_provider() {
        let item = StoredRequestUsageAudit {
            provider_name: "unknown".to_string(),
            provider_id: None,
            provider_endpoint_id: None,
            provider_api_key_id: None,
            candidate_id: None,
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("candidate_list_empty".to_string()),
            ..sample_usage(
                "failed",
                Some(503),
                Some("没有可用提供商支持模型 claude-fable-5 的流式请求（原因代码: candidate_list_empty）"),
            )
        };

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        let detail = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            None,
            &BTreeMap::new(),
        );

        assert_eq!(record["provider"], "无可用供应商");
        assert_eq!(detail["provider"], "无可用供应商");
    }

    #[test]
    fn admin_usage_record_keeps_generation_result_separate_from_state_usage() {
        for status in ["completed", "failed"] {
            let item = StoredRequestUsageAudit {
                request_metadata: Some(
                    json!({"codex_turn_state":{"mode":"passthrough","returned_state":"qualified"}}),
                ),
                ..sample_usage(
                    status,
                    Some(if status == "completed" { 200 } else { 503 }),
                    None,
                )
            };
            let record = admin_usage_record_json(
                &item,
                &BTreeMap::new(),
                &BTreeMap::new(),
                false,
                false,
                None,
            );
            assert_eq!(record["status"], status);
            assert_eq!(record["codex_turn_state"]["mode"], "passthrough");
            assert_eq!(record["codex_turn_state"]["returned_state"], "qualified");
        }
        let legacy = sample_usage("completed", Some(200), None);
        let record = admin_usage_record_json(
            &legacy,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        assert!(record["codex_turn_state"].is_null());
    }

    #[test]
    fn admin_usage_record_infers_client_family_from_user_agent() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "client_ip": "192.168.0.28",
                "user_agent": "codex_vscode/0.131.0-alpha.9 (Windows 10.0.26200; x86_64)"
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        let active = admin_usage_active_request_json(&item, None, None, None);

        assert_eq!(record["client_family"], "codex_vscode");
        assert_eq!(record["client_ip"], "192.168.0.28");
        assert_eq!(active["client_family"], "codex_vscode");
    }

    #[test]
    fn admin_usage_record_labels_openai_js_user_agent_as_sdk() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "user_agent": "OpenAI/JS 6.34.0"
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );

        assert_eq!(record["client_family"], "openai_js_sdk");
    }

    #[test]
    fn admin_usage_record_prefers_typed_client_family() {
        let item = StoredRequestUsageAudit {
            client_family: Some("codex".to_string()),
            request_metadata: Some(json!({
                "user_agent": "OpenAI/JS 6.34.0"
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );

        assert_eq!(record["client_family"], "codex");
    }

    #[test]
    fn admin_usage_payloads_include_content_moderation_cost_breakdown() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "model_cost_usd": 0.15,
                "actual_model_cost_usd": 0.8,
                "content_moderation_cost_usd": 0.0001,
                "content_moderation_actual_cost_usd": 0.00008
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        let active = admin_usage_active_request_json(&item, None, None, None);

        assert_eq!(record["model_cost"], json!(0.15));
        assert_eq!(record["actual_model_cost"], json!(0.8));
        assert_eq!(record["moderation_cost"], json!(0.0001));
        assert_eq!(record["actual_moderation_cost"], json!(0.00008));
        assert_eq!(active["model_cost"], json!(0.15));
        assert_eq!(active["actual_model_cost"], json!(0.8));
        assert_eq!(active["moderation_cost"], json!(0.0001));
        assert_eq!(active["actual_moderation_cost"], json!(0.00008));
    }

    #[test]
    fn client_requested_stream_uses_stream_generate_content_path_over_stale_metadata_flag() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            request_metadata: Some(json!({
                "client_requested_stream": false,
                "request_path": "/v1beta/models/gemini-3.1-flash-image-preview:streamGenerateContent",
                "request_path_and_query": "/v1beta/models/gemini-3.1-flash-image-preview:streamGenerateContent?alt=sse"
            })),
            ..sample_usage("completed", Some(200), None)
        };

        assert!(admin_usage_client_is_stream(&item));

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        assert_eq!(record["upstream_is_stream"], true);
        assert_eq!(record["client_requested_stream"], true);
        assert_eq!(record["client_is_stream"], true);
    }

    #[test]
    fn client_requested_stream_falls_back_to_request_body_stream_flag() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            request_body: Some(json!({
                "model": "gpt-5.4",
                "stream": false
            })),
            ..sample_usage("completed", Some(200), None)
        };

        assert!(!admin_usage_client_is_stream(&item));

        let active = admin_usage_active_request_json(&item, None, None, None);
        assert_eq!(active["is_stream"], true);
        assert_eq!(active["upstream_is_stream"], true);
        assert_eq!(active["client_requested_stream"], false);
        assert_eq!(active["client_is_stream"], false);
    }

    #[test]
    fn client_requested_stream_defaults_to_non_stream_for_openai_responses_request_body_without_flag(
    ) {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            api_format: Some("openai:responses".to_string()),
            request_body: Some(json!({
                "model": "gpt-5.4",
                "input": [{"role": "user", "content": "hi"}],
                "store": false
            })),
            ..sample_usage("completed", Some(200), None)
        };

        assert!(!admin_usage_client_is_stream(&item));

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        assert_eq!(record["is_stream"], true);
        assert_eq!(record["upstream_is_stream"], true);
        assert_eq!(record["client_requested_stream"], false);
        assert_eq!(record["client_is_stream"], false);
    }

    #[test]
    fn upstream_stream_prefers_request_metadata_flag() {
        let item = StoredRequestUsageAudit {
            is_stream: false,
            request_metadata: Some(json!({
                "client_requested_stream": false,
                "upstream_is_stream": true
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        assert_eq!(record["is_stream"], false);
        assert_eq!(record["upstream_is_stream"], true);
        assert_eq!(record["client_requested_stream"], false);
        assert_eq!(record["client_is_stream"], false);
    }

    #[test]
    fn stream_modes_fall_back_to_captured_response_bodies_when_request_metadata_is_missing() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            response_body: Some(json!({
                "chunks": [
                    {"type": "response.created"},
                    {"type": "response.output_text.delta", "delta": "Hello"}
                ],
                "metadata": {
                    "stream": true,
                    "stored_chunks": 2,
                    "total_chunks": 2
                }
            })),
            client_response_body: Some(json!({
                "id": "resp-1",
                "object": "response",
                "status": "completed",
                "output": []
            })),
            ..sample_usage("completed", Some(200), None)
        };

        assert!(!admin_usage_client_is_stream(&item));
        assert!(admin_usage_upstream_is_stream(&item));

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        assert_eq!(record["is_stream"], true);
        assert_eq!(record["upstream_is_stream"], true);
        assert_eq!(record["client_requested_stream"], false);
        assert_eq!(record["client_is_stream"], false);
    }

    #[test]
    fn stream_modes_fall_back_to_captured_response_headers_when_bodies_are_detached() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            response_headers: Some(json!({
                "content-type": "text/event-stream; charset=utf-8"
            })),
            client_response_headers: Some(json!({
                "content-type": "application/json"
            })),
            response_body_ref: Some("usage://request/req-1/response_body".to_string()),
            client_response_body_ref: Some(
                "usage://request/req-1/client_response_body".to_string(),
            ),
            ..sample_usage("completed", Some(200), None)
        };

        assert!(!admin_usage_client_is_stream(&item));
        assert!(admin_usage_upstream_is_stream(&item));

        let record = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
        );
        assert_eq!(record["is_stream"], true);
        assert_eq!(record["upstream_is_stream"], true);
        assert_eq!(record["client_requested_stream"], false);
        assert_eq!(record["client_is_stream"], false);
    }

    #[test]
    fn replay_body_defaults_stream_to_client_requested_mode() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            request_body: Some(json!({
                "model": "gpt-5.4",
                "input": "hello"
            })),
            request_metadata: Some(json!({
                "client_requested_stream": false
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let body = admin_usage_resolve_request_capture_body(&item, None)
            .expect("replay body should resolve");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn legacy_failure_signals_still_work_when_status_is_missing() {
        let item = StoredRequestUsageAudit {
            status: String::new(),
            ..sample_usage("completed", Some(429), Some("rate limited"))
        };
        assert!(admin_usage_is_failed(&item));
        assert!(admin_usage_matches_status(&item, Some("failed")));
    }

    #[test]
    fn redirect_status_is_not_admin_usage_success() {
        let item = sample_usage("completed", Some(302), None);

        assert!(!admin_usage_is_success(&item));
        assert!(!admin_usage_is_failed(&item));
        assert!(admin_usage_matches_status(&item, Some("error")));
        assert!(admin_usage_matches_status(&item, Some("completed")));
    }

    #[test]
    fn failed_redirect_status_counts_as_admin_usage_failed() {
        let item = sample_usage(
            "failed",
            Some(302),
            Some("execution runtime stream returned non-success status 302"),
        );

        assert!(admin_usage_is_failed(&item));
        assert!(admin_usage_matches_status(&item, Some("failed")));
        assert!(admin_usage_matches_status(&item, Some("error")));
        assert!(!admin_usage_is_success(&item));
    }

    #[test]
    fn active_status_with_failure_signal_counts_as_failed() {
        let item = sample_usage("pending", Some(503), Some("upstream failed"));

        assert!(admin_usage_is_failed(&item));
        assert!(admin_usage_matches_status(&item, Some("failed")));
    }

    #[test]
    fn admin_usage_fallback_flag_uses_routing_candidate_index() {
        let mut item = sample_usage("completed", Some(200), None);
        item.candidate_index = Some(2);

        assert!(admin_usage_has_fallback(&item));
        assert!(admin_usage_matches_status(&item, Some("has_fallback")));

        let payload = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            Some("primary"),
        );
        assert_eq!(payload["has_fallback"], true);
    }

    #[test]
    fn body_availability_considers_typed_reference_fields() {
        let item = StoredRequestUsageAudit {
            request_body_ref: Some("usage://request/req-1/request_body".to_string()),
            ..sample_usage("completed", Some(200), None)
        };

        assert!(admin_usage_has_body_value(
            &item,
            item.request_body.as_ref(),
            UsageBodyField::RequestBody
        ));
    }

    #[test]
    fn detail_payload_marks_reference_backed_bodies_as_available() {
        let item = StoredRequestUsageAudit {
            request_body: None,
            request_body_ref: Some("usage://request/req-1/request_body".to_string()),
            provider_request_body: None,
            provider_request_body_ref: Some(
                "usage://request/req-1/provider_request_body".to_string(),
            ),
            response_body: None,
            response_body_ref: Some("usage://request/req-1/response_body".to_string()),
            client_response_body: None,
            client_response_body_ref: Some(
                "usage://request/req-1/client_response_body".to_string(),
            ),
            request_metadata: Some(json!({
                "request_body_ref": "usage://request/req-1/request_body",
                "provider_request_body_ref": "usage://request/req-1/provider_request_body",
                "response_body_ref": "usage://request/req-1/response_body",
                "client_response_body_ref": "usage://request/req-1/client_response_body",
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "gpt-5"})),
            &BTreeMap::new(),
        );

        assert!(payload["metadata"]["request_body_ref"].is_null());
        assert!(payload["metadata"]["provider_request_body_ref"].is_null());
        assert!(payload["metadata"]["response_body_ref"].is_null());
        assert!(payload["metadata"]["client_response_body_ref"].is_null());
        assert_eq!(payload["body_capture"]["request"]["available"], true);
        assert_eq!(payload["body_capture"]["request"]["storage"], "reference");
        assert_eq!(
            payload["body_capture"]["request"]["body_ref"],
            "usage://request/req-1/request_body"
        );
        assert_eq!(
            payload["body_capture"]["request"]["capture_source"],
            "stored_reference"
        );
        assert_eq!(
            payload["body_capture"]["provider_request"]["body_ref"],
            "usage://request/req-1/provider_request_body"
        );
        assert_eq!(
            payload["body_capture"]["response"]["body_ref"],
            "usage://request/req-1/response_body"
        );
        assert_eq!(
            payload["body_capture"]["client_response"]["body_ref"],
            "usage://request/req-1/client_response_body"
        );
        assert_eq!(payload["body_capture"]["response"]["available"], true);
        assert_eq!(
            payload["body_capture"]["client_response"]["available"],
            true
        );
        assert_eq!(payload["has_provider_request_body"], true);
        assert_eq!(payload["has_response_body"], true);
        assert_eq!(payload["has_client_response_body"], true);
    }

    #[test]
    fn detail_payload_separates_upstream_client_and_summary_errors() {
        let item = StoredRequestUsageAudit {
            error_message: Some(
                "execution runtime stream returned retryable status 400".to_string(),
            ),
            error_category: Some("server_error".to_string()),
            response_headers: Some(json!({
                "content-type": "application/json"
            })),
            response_body: Some(json!({
                "error": {
                    "type": "retryable_upstream_status",
                    "message": "execution runtime stream returned retryable status 400",
                    "code": 400
                }
            })),
            client_response_headers: Some(json!({
                "content-type": "application/json"
            })),
            client_response_body: Some(json!({
                "error": {
                    "type": "http_error",
                    "message": "local execution runtime exhausted"
                }
            })),
            ..sample_usage(
                "failed",
                Some(503),
                Some("execution runtime stream returned retryable status 400"),
            )
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            true,
            Some(json!({"model": "gpt-5.4"})),
            &BTreeMap::new(),
        );

        assert_eq!(payload["upstream_error"]["source"], "upstream_response");
        assert_eq!(
            payload["errors"]["upstream_error"],
            payload["upstream_error"]
        );
        assert_eq!(
            payload["upstream_error"]["message"],
            "execution runtime stream returned retryable status 400"
        );
        assert_eq!(payload["client_error"]["source"], "client_response");
        assert_eq!(
            payload["client_error"]["message"],
            "local execution runtime exhausted"
        );
        assert_eq!(payload["failure_summary"]["source"], "upstream_response");
        assert_eq!(
            payload["failure_summary"]["message"],
            "execution runtime stream returned retryable status 400"
        );
        assert_eq!(payload["error_flow"]["propagation"], "converted");
        assert!(payload["request_error"].is_null());
    }

    #[test]
    fn detail_payload_prefers_upstream_error_over_local_runtime_miss_notice() {
        let item = StoredRequestUsageAudit {
            candidate_id: Some("candidate-1".to_string()),
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("no_local_stream_plans".to_string()),
            error_category: Some("http_error".to_string()),
            response_headers: Some(json!({
                "content-type": "application/json"
            })),
            response_body: Some(json!({
                "error": {
                    "type": "overloaded_error",
                    "message": "upstream overloaded",
                    "code": "overloaded"
                }
            })),
            client_response_headers: Some(json!({
                "content-type": "application/json"
            })),
            client_response_body: Some(json!({
                "error": {
                    "type": "http_error",
                    "message": "local execution runtime exhausted"
                }
            })),
            ..sample_usage(
                "failed",
                Some(503),
                Some("找到了可尝试提供商，但无法为本次流式请求构建本地执行计划（原因代码: no_local_stream_plans）"),
            )
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            true,
            Some(json!({"model": "claude-opus-4-8", "stream": true})),
            &BTreeMap::new(),
        );

        assert_eq!(payload["upstream_error"]["source"], "upstream_response");
        assert_eq!(payload["upstream_error"]["status_code"], json!(503));
        assert_eq!(payload["upstream_error"]["message"], "upstream overloaded");
        assert_eq!(payload["failure_summary"]["source"], "upstream_response");
        assert_eq!(payload["failure_summary"]["message"], "upstream overloaded");
        assert!(payload["scheduling_failure"].is_null());
    }

    #[test]
    fn detail_payload_uses_upstream_status_when_body_has_no_message() {
        let item = StoredRequestUsageAudit {
            candidate_id: Some("candidate-1".to_string()),
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("no_local_stream_plans".to_string()),
            error_category: Some("http_error".to_string()),
            response_headers: Some(json!({
                "content-type": "text/plain"
            })),
            response_body: None,
            client_response_headers: Some(json!({
                "content-type": "application/json"
            })),
            client_response_body: Some(json!({
                "error": {
                    "type": "http_error",
                    "message": "local execution runtime exhausted"
                }
            })),
            ..sample_usage(
                "failed",
                Some(503),
                Some("找到了可尝试提供商，但无法为本次流式请求构建本地执行计划（原因代码: no_local_stream_plans）"),
            )
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "claude-opus-4-8", "stream": true})),
            &BTreeMap::new(),
        );

        assert_eq!(payload["upstream_error"]["source"], "upstream_response");
        assert_eq!(payload["upstream_error"]["status_code"], json!(503));
        assert!(payload["upstream_error"]["message"].is_null());
        assert_eq!(payload["failure_summary"]["source"], "upstream_response");
        assert_eq!(payload["failure_summary"]["message"], "上游返回 HTTP 503");
        assert!(payload["scheduling_failure"].is_null());
    }

    #[test]
    fn detail_payload_promotes_failed_candidate_upstream_response_to_failure_summary() {
        let item = StoredRequestUsageAudit {
            candidate_id: Some("candidate-1".to_string()),
            candidate_index: Some(0),
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("all_candidates_skipped".to_string()),
            error_category: Some("server_error".to_string()),
            response_headers: Some(json!({
                "content-type": "application/json"
            })),
            response_body: None,
            response_body_ref: Some("usage://request/req-1/response_body".to_string()),
            client_response_headers: Some(json!({
                "content-type": "application/json"
            })),
            client_response_body: None,
            client_response_body_ref: Some(
                "usage://request/req-1/client_response_body".to_string(),
            ),
            ..sample_usage(
                "failed",
                Some(503),
                Some("execution runtime stream returned retryable status 400"),
            )
        };
        let candidate = StoredRequestCandidate::new(
            "candidate-1".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            None,
            None,
            0,
            0,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            RequestCandidateStatus::Failed,
            None,
            false,
            Some(400),
            Some("retryable_upstream_status".to_string()),
            Some("execution runtime stream returned retryable status 400".to_string()),
            None,
            None,
            Some(json!({
                "upstream_response": {
                    "status_code": 400,
                    "headers": {
                        "content-type": "text/event-stream"
                    },
                    "body": {
                        "type": "error",
                        "error": {
                            "type": "<nil>",
                            "message": "image dimension exceeds max allowed size"
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

        let mut payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "claude-opus-4-8", "stream": true})),
            &BTreeMap::new(),
        );

        super::admin_usage_apply_candidate_upstream_error_display(
            &mut payload,
            &item,
            std::slice::from_ref(&candidate),
        );

        assert_eq!(payload["upstream_error"]["status_code"], json!(400));
        assert_eq!(
            payload["upstream_error"]["message"],
            "image dimension exceeds max allowed size"
        );
        assert_eq!(payload["failure_summary"]["source"], "upstream_response");
        assert_eq!(payload["failure_summary"]["status_code"], json!(400));
        assert_eq!(
            payload["failure_summary"]["message"],
            "image dimension exceeds max allowed size"
        );
        assert_eq!(
            payload["errors"]["upstream_error"],
            payload["upstream_error"]
        );
        assert_eq!(
            payload["errors"]["failure_summary"],
            payload["failure_summary"]
        );
    }

    #[test]
    fn detail_payload_promotes_plain_text_candidate_upstream_response() {
        let item = StoredRequestUsageAudit {
            candidate_id: Some("candidate-1".to_string()),
            candidate_index: Some(0),
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("all_candidates_skipped".to_string()),
            error_category: Some("server_error".to_string()),
            response_headers: Some(json!({
                "content-type": "text/plain; charset=utf-8"
            })),
            response_body: Some(json!("upstream 503: temporarily unavailable")),
            client_response_headers: Some(json!({
                "content-type": "text/plain; charset=utf-8"
            })),
            client_response_body: Some(json!("upstream 503: temporarily unavailable")),
            ..sample_usage(
                "failed",
                Some(503),
                Some("execution runtime stream returned retryable status 503"),
            )
        };
        let candidate = StoredRequestCandidate::new(
            "candidate-1".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            None,
            None,
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

        let mut payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "claude-opus-4-8", "stream": true})),
            &BTreeMap::new(),
        );

        super::admin_usage_apply_candidate_upstream_error_display(
            &mut payload,
            &item,
            std::slice::from_ref(&candidate),
        );

        assert_eq!(payload["upstream_error"]["status_code"], json!(503));
        assert_eq!(
            payload["upstream_error"]["message"],
            "upstream 503: temporarily unavailable"
        );
        assert_eq!(payload["failure_summary"]["source"], "upstream_response");
        assert_eq!(
            payload["failure_summary"]["message"],
            "upstream 503: temporarily unavailable"
        );
    }

    #[test]
    fn detail_payload_promotes_retryable_502_candidate_upstream_message() {
        let item = StoredRequestUsageAudit {
            candidate_id: Some("candidate-1".to_string()),
            candidate_index: Some(0),
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("all_candidates_skipped".to_string()),
            error_category: Some("server_error".to_string()),
            response_headers: Some(json!({
                "content-type": "application/json"
            })),
            response_body: Some(json!({
                "error": {
                    "type": "retryable_upstream_status",
                    "message": "execution runtime stream returned retryable status 502",
                    "code": 502
                }
            })),
            client_response_headers: Some(json!({
                "content-type": "application/json"
            })),
            client_response_body: Some(json!({
                "error": {
                    "type": "http_error",
                    "message": "没有可用提供商支持模型 claude-opus-4-8 的流式请求"
                }
            })),
            ..sample_usage(
                "failed",
                Some(503),
                Some("execution runtime stream returned retryable status 502"),
            )
        };
        let candidate = StoredRequestCandidate::new(
            "candidate-1".to_string(),
            "req-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            None,
            None,
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

        let mut payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "claude-opus-4-8", "stream": true})),
            &BTreeMap::new(),
        );

        super::admin_usage_apply_candidate_upstream_error_display(
            &mut payload,
            &item,
            std::slice::from_ref(&candidate),
        );

        assert_eq!(payload["upstream_error"]["status_code"], json!(502));
        assert_eq!(
            payload["upstream_error"]["message"],
            "Upstream service temporarily unavailable (request id: 202606131644039769451708268d9d6iULlPLG3)"
        );
        assert_eq!(payload["failure_summary"]["source"], "upstream_response");
        assert_eq!(payload["failure_summary"]["status_code"], json!(502));
        assert_eq!(
            payload["failure_summary"]["message"],
            "Upstream service temporarily unavailable (request id: 202606131644039769451708268d9d6iULlPLG3)"
        );
    }

    #[test]
    fn detail_payload_marks_sensitive_upstream_account_errors_suppressed() {
        let item = StoredRequestUsageAudit {
            error_message: Some("credit balance exhausted".to_string()),
            error_category: Some("server_error".to_string()),
            response_body: Some(json!({
                "error": {
                    "type": "insufficient_quota",
                    "message": "credit balance exhausted"
                }
            })),
            client_response_body: Some(json!({
                "error": {
                    "type": "http_error",
                    "message": "upstream provider unavailable"
                }
            })),
            ..sample_usage("failed", Some(503), Some("credit balance exhausted"))
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            true,
            Some(json!({"model": "gpt-5.4"})),
            &BTreeMap::new(),
        );

        assert_eq!(payload["error_flow"]["propagation"], "suppressed");
        assert_eq!(payload["error_flow"]["safe_to_expose_upstream"], false);
        assert_eq!(payload["upstream_error"]["type"], "insufficient_quota");
        assert_eq!(
            payload["client_error"]["message"],
            "upstream provider unavailable"
        );
    }

    #[test]
    fn detail_payload_does_not_promote_local_client_error_to_upstream_error() {
        let message = "没有可用提供商支持模型 gpt-5.4 的同步请求。请检查模型映射、端点启用状态和 API Key 权限（原因代码: candidate_list_empty）";
        let client_message = "没有可用提供商支持模型 gpt-5.4 的同步请求";
        let item = StoredRequestUsageAudit {
            provider_api_key_id: None,
            provider_request_headers: None,
            provider_request_body: None,
            provider_request_body_ref: None,
            candidate_id: None,
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("candidate_list_empty".to_string()),
            error_category: Some("http_error".to_string()),
            client_response_body: Some(json!({
                "error": {
                    "type": "http_error",
                    "message": client_message
                }
            })),
            response_body: Some(json!({
                "error": {
                    "type": "http_error",
                    "message": message
                }
            })),
            ..sample_usage("failed", Some(503), Some(message))
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            true,
            Some(json!({"model": "gpt-5.4"})),
            &BTreeMap::new(),
        );

        assert!(payload["upstream_error"].is_null());
        assert_eq!(payload["client_error"]["message"], client_message);
        assert_eq!(
            payload["client_response_body"]["error"]["message"],
            client_message
        );
        assert_eq!(payload["error_flow"]["source"], "gateway");
        assert_eq!(payload["error_flow"]["propagation"], "local");
    }

    #[test]
    fn detail_payload_simplifies_local_client_error_when_client_body_is_unloaded() {
        let message = "没有可用提供商支持模型 gpt-5.4 的流式请求。请检查模型映射、端点启用状态和 API Key 权限（原因代码: candidate_list_empty）";
        let item = StoredRequestUsageAudit {
            provider_api_key_id: None,
            provider_request_headers: None,
            provider_request_body: None,
            provider_request_body_ref: None,
            candidate_id: None,
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("candidate_list_empty".to_string()),
            error_category: Some("http_error".to_string()),
            client_response_headers: Some(json!({"content-type": "application/json"})),
            client_response_body: None,
            client_response_body_ref: Some(
                "usage://request/req-1/client_response_body".to_string(),
            ),
            response_body: None,
            ..sample_usage("failed", Some(503), Some(message))
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "gpt-5.4", "stream": true})),
            &BTreeMap::new(),
        );

        assert_eq!(
            payload["client_error"]["message"],
            "没有可用提供商支持模型 gpt-5.4 的流式请求"
        );
        assert_eq!(
            payload["failure_summary"]["message"],
            "没有可用提供商支持模型 gpt-5.4 的流式请求"
        );
        assert_eq!(
            payload["scheduling_failure"]["title"],
            "本地调度失败：没有可调度服务"
        );
        assert_eq!(
            payload["scheduling_failure"]["reason"],
            "candidate_list_empty"
        );
        assert!(payload["scheduling_failure"]["reason_summary"].is_null());
        assert_eq!(
            payload["scheduling_failure"]["message"],
            "没有可用提供商支持模型 gpt-5.4 的流式请求"
        );
        assert_eq!(payload["scheduling_failure"]["no_upstream_attempt"], true);
    }

    #[test]
    fn detail_payload_simplifies_all_candidates_skipped_when_client_body_is_unloaded() {
        let message = "找到 1 个支持模型 gpt-5.4 的可尝试提供商，但本次流式请求全部不可用：provider_quota_blocked 2 次（原因代码: all_candidates_skipped）";
        let item = StoredRequestUsageAudit {
            provider_api_key_id: None,
            provider_request_headers: None,
            provider_request_body: None,
            provider_request_body_ref: None,
            candidate_id: None,
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("all_candidates_skipped".to_string()),
            error_category: Some("http_error".to_string()),
            client_response_headers: Some(json!({"content-type": "application/json"})),
            client_response_body: None,
            client_response_body_ref: Some(
                "usage://request/req-1/client_response_body".to_string(),
            ),
            response_body: None,
            ..sample_usage("failed", Some(503), Some(message))
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "gpt-5.4", "stream": true})),
            &BTreeMap::new(),
        );

        assert_eq!(
            payload["client_error"]["message"],
            "找到 1 个支持模型 gpt-5.4 的可尝试提供商，但本次流式请求全部不可用：provider_quota_blocked 2 次"
        );
        assert_eq!(
            payload["failure_summary"]["message"],
            "找到 1 个支持模型 gpt-5.4 的可尝试提供商，但本次流式请求全部不可用：provider_quota_blocked 2 次"
        );
        assert_eq!(
            payload["scheduling_failure"]["title"],
            "本地调度失败：可尝试服务全部不可用"
        );
        assert_eq!(
            payload["scheduling_failure"]["reason"],
            "all_candidates_skipped"
        );
        assert_eq!(
            payload["scheduling_failure"]["reason_summary"],
            "provider_quota_blocked 2 次"
        );
        assert_eq!(
            payload["scheduling_failure"]["message"],
            "找到 1 个支持模型 gpt-5.4 的可尝试提供商，但本次流式请求全部不可用：provider_quota_blocked 2 次"
        );
        assert_eq!(payload["scheduling_failure"]["no_upstream_attempt"], true);
    }

    #[test]
    fn detail_payload_preserves_legacy_body_capture_metadata_keys() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "request_preview_source": "stored_original",
                "original_request_body_available": true,
                "original_response_body_available": true,
                "custom_key": "keep-me"
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "gpt-5"})),
            &BTreeMap::new(),
        );

        assert_eq!(
            payload["metadata"]["request_preview_source"],
            "stored_original"
        );
        assert_eq!(payload["metadata"]["original_request_body_available"], true);
        assert_eq!(
            payload["metadata"]["original_response_body_available"],
            true
        );
        assert_eq!(payload["metadata"]["custom_key"], "keep-me");
    }

    #[test]
    fn admin_usage_record_prefers_typed_pricing_metadata_with_billing_snapshot_fallback() {
        let item = StoredRequestUsageAudit {
            output_price_per_1m: Some(15.0),
            request_metadata: Some(json!({
                "rate_multiplier": 0.5,
                "is_free_tier": false,
                "input_price_per_1m": 3.0,
                "cache_creation_price_per_1m": 3.75,
                "billing_snapshot": {
                    "resolved_variables": {
                        "cache_read_price_per_1m": 0.30,
                        "output_price_per_1m": 15.0
                    }
                }
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let payload = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            Some("primary"),
        );

        assert_eq!(payload["rate_multiplier"], 0.5);
        assert_eq!(payload["is_free_tier"], false);
        assert_eq!(payload["input_price_per_1m"], 3.0);
        assert_eq!(payload["output_price_per_1m"], 15.0);
        assert_eq!(payload["cache_creation_price_per_1m"], 3.75);
        assert_eq!(payload["cache_read_price_per_1m"], 0.30);
    }

    #[test]
    fn admin_usage_record_rehydrates_cache_creation_total_from_classified_fields() {
        let item = StoredRequestUsageAudit {
            cache_creation_input_tokens: 0,
            cache_creation_ephemeral_5m_input_tokens: 12,
            cache_creation_ephemeral_1h_input_tokens: 8,
            ..sample_usage("completed", Some(200), None)
        };

        let payload = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            Some("primary"),
        );

        assert_eq!(payload["cache_creation_input_tokens"], 20);
        assert_eq!(payload["cache_creation_ephemeral_5m_input_tokens"], 12);
        assert_eq!(payload["cache_creation_ephemeral_1h_input_tokens"], 8);
        assert_eq!(payload["total_tokens"], 50);
    }

    #[test]
    fn admin_usage_total_tokens_uses_effective_input_for_cached_openai_usage() {
        let item = StoredRequestUsageAudit {
            input_tokens: 100,
            output_tokens: 20,
            total_tokens: 999,
            cache_creation_input_tokens: 0,
            cache_creation_ephemeral_5m_input_tokens: 12,
            cache_creation_ephemeral_1h_input_tokens: 8,
            cache_read_input_tokens: 80,
            ..sample_usage("completed", Some(200), None)
        };

        assert_eq!(admin_usage_total_tokens(&item), 140);
    }

    #[test]
    fn detail_payload_exposes_typed_settlement_section() {
        let item = StoredRequestUsageAudit {
            output_price_per_1m: Some(15.0),
            request_metadata: Some(json!({
                "trace_id": "trace-detail",
                "billing_snapshot_schema_version": "v2",
                "billing_snapshot_status": "resolved",
                "rate_multiplier": 0.5,
                "is_free_tier": false,
                "input_price_per_1m": 3.0,
                "output_price_per_1m": 9.0,
                "cache_creation_price_per_1m": 3.75,
                "cache_read_price_per_1m": 0.30,
                "price_per_request": 0.02,
                "settlement_snapshot_schema_version": "3.0",
                "billing_dimensions": {
                    "input_tokens": 35,
                    "total_input_context": 42
                },
                "settlement_snapshot": {
                    "schema_version": "3.0",
                    "pricing_snapshot": {
                        "pricing_source": "provider_override"
                    }
                },
                "billing_snapshot": {
                    "resolved_variables": {
                        "output_price_per_1m": 11.0
                    }
                }
            })),
            ..sample_usage("completed", Some(200), None)
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            None,
            false,
            Some(json!({"model": "gpt-5"})),
            &BTreeMap::new(),
        );

        assert_eq!(payload["trace"]["trace_id"], "trace-detail");
        assert_eq!(
            payload["settlement"]["billing_snapshot_schema_version"],
            "v2"
        );
        assert_eq!(
            payload["settlement"]["billing_snapshot"]["resolved_variables"]["output_price_per_1m"],
            11.0
        );
        assert_eq!(
            payload["settlement"]["settlement_snapshot_schema_version"],
            "3.0"
        );
        assert_eq!(
            payload["settlement"]["settlement_snapshot"]["pricing_snapshot"]["pricing_source"],
            "provider_override"
        );
        assert_eq!(
            payload["settlement"]["billing_dimensions"]["input_tokens"],
            35
        );
        assert_eq!(payload["settlement"]["billing_snapshot_status"], "resolved");
        assert_eq!(payload["settlement"]["rate_multiplier"], 0.5);
        assert_eq!(payload["settlement"]["is_free_tier"], false);
        assert_eq!(payload["settlement"]["input_price_per_1m"], 3.0);
        assert_eq!(payload["settlement"]["output_price_per_1m"], 9.0);
        assert_eq!(payload["settlement"]["cache_creation_price_per_1m"], 3.75);
        assert_eq!(payload["settlement"]["cache_read_price_per_1m"], 0.30);
        assert_eq!(payload["settlement"]["price_per_request"], 0.02);
        assert!(payload["metadata"]["trace_id"].is_null());
        assert!(payload["metadata"]["billing_snapshot"].is_null());
        assert!(payload["metadata"]["billing_snapshot_schema_version"].is_null());
        assert!(payload["metadata"]["billing_snapshot_status"].is_null());
        assert!(payload["metadata"]["settlement_snapshot"].is_null());
        assert!(payload["metadata"]["settlement_snapshot_schema_version"].is_null());
        assert!(payload["metadata"]["billing_dimensions"].is_null());
        assert!(payload["metadata"]["rate_multiplier"].is_null());
        assert!(payload["metadata"]["is_free_tier"].is_null());
        assert!(payload["metadata"]["input_price_per_1m"].is_null());
        assert!(payload["metadata"]["output_price_per_1m"].is_null());
        assert!(payload["metadata"]["cache_creation_price_per_1m"].is_null());
        assert!(payload["metadata"]["cache_read_price_per_1m"].is_null());
        assert!(payload["metadata"]["price_per_request"].is_null());
    }

    #[test]
    fn admin_usage_provider_key_name_falls_back_to_typed_routing_metadata() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "key_name": "upstream-primary"
            })),
            ..sample_usage("completed", Some(200), None)
        };

        assert_eq!(
            super::admin_usage_provider_key_name(&item, &BTreeMap::new()),
            Some("upstream-primary".to_string())
        );
    }

    #[test]
    fn detail_payload_exposes_typed_routing_section() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "trace_id": "trace-routing-detail",
                "model_id": "model-1",
                "global_model_id": "global-model-1",
                "global_model_name": "gpt-5"
            })),
            candidate_id: Some("cand-1".to_string()),
            candidate_index: Some(2),
            key_name: Some("upstream-primary".to_string()),
            planner_kind: Some("claude_cli_sync".to_string()),
            route_family: Some("claude".to_string()),
            route_kind: Some("cli".to_string()),
            execution_path: Some("local_execution_runtime_miss".to_string()),
            local_execution_runtime_miss_reason: Some("all_candidates_skipped".to_string()),
            ..sample_usage("completed", Some(200), None)
        };

        let payload = build_admin_usage_detail_payload(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            false,
            Some("resolved-primary"),
            false,
            Some(json!({"model": "gpt-5"})),
            &BTreeMap::new(),
        );

        assert_eq!(payload["routing"]["candidate_id"], "cand-1");
        assert_eq!(payload["routing"]["candidate_index"], 2);
        assert_eq!(payload["routing"]["key_name"], "resolved-primary");
        assert_eq!(payload["routing"]["model_id"], "model-1");
        assert_eq!(payload["routing"]["global_model_id"], "global-model-1");
        assert_eq!(payload["routing"]["global_model_name"], "gpt-5");
        assert_eq!(payload["routing"]["planner_kind"], "claude_cli_sync");
        assert_eq!(payload["routing"]["route_family"], "claude");
        assert_eq!(payload["routing"]["route_kind"], "cli");
        assert_eq!(
            payload["routing"]["execution_path"],
            "local_execution_runtime_miss"
        );
        assert_eq!(
            payload["routing"]["local_execution_runtime_miss_reason"],
            "all_candidates_skipped"
        );
        assert!(payload["metadata"]["candidate_id"].is_null());
        assert!(payload["metadata"]["candidate_index"].is_null());
        assert!(payload["metadata"]["key_name"].is_null());
        assert!(payload["metadata"]["model_id"].is_null());
        assert!(payload["metadata"]["global_model_id"].is_null());
        assert!(payload["metadata"]["global_model_name"].is_null());
        assert!(payload["metadata"]["planner_kind"].is_null());
        assert!(payload["metadata"]["trace_id"].is_null());
        assert!(payload["metadata"]["route_family"].is_null());
        assert!(payload["metadata"]["route_kind"].is_null());
        assert!(payload["metadata"]["execution_path"].is_null());
        assert!(payload["metadata"]["local_execution_runtime_miss_reason"].is_null());
    }

    #[test]
    fn admin_usage_record_prefers_joined_user_and_api_key_names_over_legacy_columns() {
        let item = StoredRequestUsageAudit {
            username: Some("legacy-alice".to_string()),
            api_key_name: Some("legacy-default".to_string()),
            ..sample_usage("completed", Some(200), None)
        };
        let users_by_id = BTreeMap::from([(
            "user-1".to_string(),
            aether_data::repository::users::StoredUserSummary::new(
                "user-1".to_string(),
                "fresh-alice".to_string(),
                Some("fresh-alice@example.com".to_string()),
                "user".to_string(),
                true,
                false,
            )
            .expect("user summary should build"),
        )]);
        let api_key_names =
            BTreeMap::from([("api-key-1".to_string(), "fresh-default".to_string())]);

        let payload = admin_usage_record_json(
            &item,
            &users_by_id,
            &api_key_names,
            true,
            true,
            Some("primary"),
        );

        assert_eq!(payload["username"], "fresh-alice");
        assert_eq!(payload["api_key"]["name"], "fresh-default");
        assert_eq!(payload["api_key"]["display"], "fresh-default");
        assert_eq!(payload["api_key_name"], "fresh-default");
        assert!(admin_usage_matches_search(
            &item,
            Some("fresh-default"),
            &users_by_id,
            &api_key_names,
            true,
            true
        ));
        assert!(admin_usage_matches_username(
            &item,
            Some("fresh-alice"),
            &users_by_id,
            true
        ));
    }

    #[test]
    fn admin_usage_record_does_not_fallback_to_legacy_username_when_reader_is_available() {
        let item = StoredRequestUsageAudit {
            username: Some("legacy-alice".to_string()),
            ..sample_usage("completed", Some(200), None)
        };

        let payload = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            true,
            false,
            Some("primary"),
        );

        assert_eq!(payload["username"], "已删除用户");
        assert!(!admin_usage_matches_search(
            &item,
            Some("legacy-alice"),
            &BTreeMap::new(),
            &BTreeMap::new(),
            true,
            false
        ));
        assert!(!admin_usage_matches_username(
            &item,
            Some("legacy-alice"),
            &BTreeMap::new(),
            true
        ));
    }

    #[test]
    fn admin_usage_record_labels_missing_auth_context_as_unauthenticated_request() {
        let item = StoredRequestUsageAudit {
            user_id: None,
            api_key_id: None,
            username: None,
            api_key_name: None,
            ..sample_usage("failed", Some(503), Some("missing auth context"))
        };

        let payload = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            true,
            true,
            Some("primary"),
        );

        assert_eq!(payload["username"], "未认证请求");
        assert!(payload["user_email"].is_null());
        assert!(payload["api_key"].is_null());
    }

    #[test]
    fn admin_usage_record_does_not_fallback_to_legacy_api_key_name_when_reader_is_available() {
        let item = StoredRequestUsageAudit {
            api_key_name: Some("legacy-default".to_string()),
            ..sample_usage("completed", Some(200), None)
        };

        let payload = admin_usage_record_json(
            &item,
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            true,
            Some("primary"),
        );

        assert_eq!(payload["api_key"]["id"], "api-key-1");
        assert!(payload["api_key"]["name"].is_null());
        assert_eq!(payload["api_key"]["display"], "api-key-1");
        assert!(payload["api_key_name"].is_null());
        assert!(!admin_usage_matches_search(
            &item,
            Some("legacy-default"),
            &BTreeMap::new(),
            &BTreeMap::new(),
            false,
            true
        ));
    }
}

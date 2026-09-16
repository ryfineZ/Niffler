use std::collections::{BTreeMap, BTreeSet};

use aether_admin::observability::usage::{
    admin_usage_apply_provider_route_display, admin_usage_attempt_info_for_item,
    admin_usage_attempt_info_from_candidates, admin_usage_attempt_status_filter,
    admin_usage_matches_attempt_status, admin_usage_matches_client_family, AdminUsageAttemptInfo,
};
use aether_billing::{
    normalize_input_tokens_for_billing, normalize_total_input_context_for_cache_hit_rate,
};
use aether_data_contracts::repository::candidates::StoredRequestCandidate;
use aether_data_contracts::repository::usage::{
    StoredRequestUsageAudit, StoredUsageBreakdownSummaryRow, StoredUsageDailySummary,
    UsageAuditKeywordSearchQuery, UsageAuditListQuery, UsageBreakdownGroupBy,
    UsageBreakdownSummaryQuery, UsageCacheAffinityIntervalGroupBy, UsageCacheAffinityIntervalQuery,
    UsageDashboardSummaryQuery,
};
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde_json::json;

use crate::GatewayError;

use super::{
    admin_stats_bad_request_response, build_auth_error_response, build_auth_wallet_summary_payload,
    parse_bounded_u32, query_param_value, resolve_authenticated_local_user, resolve_user_portal,
    round_to, unix_secs_to_rfc3339, AdminStatsTimeRange, AppState, GatewayPublicRequestContext,
};

const USERS_ME_USAGE_DATA_UNAVAILABLE_DETAIL: &str = "用户用量数据暂不可用";
const USERS_ME_USAGE_COST_EPSILON: f64 = 0.000_000_01;

struct UsersMeUsageChargeBreakdown {
    payload: serde_json::Value,
    user_debit_usd: f64,
}

fn build_users_me_usage_reader_unavailable_response() -> Response<Body> {
    build_auth_error_response(
        http::StatusCode::SERVICE_UNAVAILABLE,
        USERS_ME_USAGE_DATA_UNAVAILABLE_DETAIL,
        false,
    )
}

fn parse_users_me_usage_limit(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "limit") {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "limit must be an integer between 1 and 200".to_string())?;
            if (1..=200).contains(&parsed) {
                Ok(parsed)
            } else {
                Err("limit must be an integer between 1 and 200".to_string())
            }
        }
        None => Ok(100),
    }
}

fn parse_users_me_usage_offset(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "offset") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| "offset must be a non-negative integer".to_string()),
        None => Ok(0),
    }
}

fn users_me_usage_should_include_records(query: Option<&str>) -> bool {
    match query_param_value(query, "include_records") {
        Some(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "false" | "0" | "no"
        ),
        None => true,
    }
}

fn apply_users_me_usage_status_filter(query: &mut UsageAuditListQuery, status: Option<&str>) {
    let Some(status) = status
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
    else {
        return;
    };

    match status {
        "stream" => query.is_stream = Some(true),
        "standard" => query.is_stream = Some(false),
        "error" | "failed" => query.error_only = true,
        "active" => {
            query.statuses = Some(vec!["pending".to_string(), "streaming".to_string()]);
        }
        "pending" | "streaming" | "completed" | "cancelled" => {
            query.statuses = Some(vec![status.to_string()]);
        }
        "has_fallback" | "has_retry" => {}
        _ => {}
    }
}

fn build_users_me_usage_records_query(
    created_from_unix_secs: u64,
    created_until_unix_secs: u64,
    user_id: &str,
    query: Option<&str>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> UsageAuditListQuery {
    let mut list_query = UsageAuditListQuery {
        created_from_unix_secs: Some(created_from_unix_secs),
        created_until_unix_secs: Some(created_until_unix_secs),
        user_id: Some(user_id.to_string()),
        provider_name: query_param_value(query, "provider"),
        model: query_param_value(query, "model"),
        api_format: query_param_value(query, "api_format"),
        limit,
        offset,
        newest_first: true,
        ..Default::default()
    };
    apply_users_me_usage_status_filter(
        &mut list_query,
        query_param_value(query, "status").as_deref(),
    );
    list_query
}

fn build_users_me_usage_keyword_search_query(
    base_query: &UsageAuditListQuery,
    keywords: Vec<String>,
    matched_api_key_ids_by_keyword: Vec<Vec<String>>,
    auth_api_key_reader_available: bool,
    limit: Option<usize>,
    offset: Option<usize>,
) -> UsageAuditKeywordSearchQuery {
    UsageAuditKeywordSearchQuery {
        created_from_unix_secs: base_query.created_from_unix_secs,
        created_until_unix_secs: base_query.created_until_unix_secs,
        user_id: base_query.user_id.clone(),
        api_key_ids: base_query.api_key_ids.clone(),
        provider_name: base_query.provider_name.clone(),
        model: base_query.model.clone(),
        api_format: base_query.api_format.clone(),
        statuses: base_query.statuses.clone(),
        is_stream: base_query.is_stream,
        error_only: base_query.error_only,
        keywords,
        matched_user_ids_by_keyword: Vec::new(),
        auth_user_reader_available: false,
        matched_api_key_ids_by_keyword,
        auth_api_key_reader_available,
        username_keyword: None,
        matched_user_ids_for_username: Vec::new(),
        limit,
        offset,
        newest_first: true,
    }
}

fn parse_users_me_usage_hours(query: Option<&str>) -> Result<u32, String> {
    match query_param_value(query, "hours") {
        Some(value) => parse_bounded_u32("hours", &value, 1, 720),
        None => Ok(24),
    }
}

fn parse_users_me_usage_timeline_limit(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "limit") {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "limit must be an integer between 100 and 20000".to_string())?;
            if (100..=20_000).contains(&parsed) {
                Ok(parsed)
            } else {
                Err("limit must be an integer between 100 and 20000".to_string())
            }
        }
        None => Ok(2_000),
    }
}

fn parse_users_me_usage_ids(query: Option<&str>) -> Option<BTreeSet<String>> {
    let ids = query_param_value(query, "ids")?;
    let values = ids
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    (!values.is_empty()).then_some(values)
}

fn users_me_usage_cache_creation_tokens(item: &StoredRequestUsageAudit) -> u64 {
    let classified = item
        .cache_creation_ephemeral_5m_input_tokens
        .saturating_add(item.cache_creation_ephemeral_1h_input_tokens);
    if item.cache_creation_input_tokens == 0 && classified > 0 {
        classified
    } else {
        item.cache_creation_input_tokens
    }
}

fn users_me_usage_total_input_context(item: &StoredRequestUsageAudit) -> u64 {
    let api_format = item
        .endpoint_api_format
        .as_deref()
        .or(item.api_format.as_deref());
    let input_tokens = i64::try_from(item.input_tokens).unwrap_or(i64::MAX);
    let cache_creation_tokens =
        i64::try_from(users_me_usage_cache_creation_tokens(item)).unwrap_or(i64::MAX);
    let cache_read_tokens = i64::try_from(item.cache_read_input_tokens).unwrap_or(i64::MAX);
    normalize_total_input_context_for_cache_hit_rate(
        api_format,
        input_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    ) as u64
}

fn users_me_usage_effective_input_tokens(item: &StoredRequestUsageAudit) -> u64 {
    let api_format = item
        .endpoint_api_format
        .as_deref()
        .or(item.api_format.as_deref());
    let input_tokens = i64::try_from(item.input_tokens).unwrap_or(i64::MAX);
    let cache_read_tokens = i64::try_from(item.cache_read_input_tokens).unwrap_or(i64::MAX);
    normalize_input_tokens_for_billing(api_format, input_tokens, cache_read_tokens) as u64
}

fn users_me_usage_effective_unix_secs(item: &StoredRequestUsageAudit) -> u64 {
    item.finalized_at_unix_secs
        .unwrap_or(item.created_at_unix_ms)
}

fn users_me_usage_cache_hit_rate(total_input_context: u64, cache_read_tokens: u64) -> f64 {
    if total_input_context == 0 {
        0.0
    } else {
        round_to(
            cache_read_tokens as f64 / total_input_context as f64 * 100.0,
            2,
        )
    }
}

fn users_me_usage_api_key_name(
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

fn parse_users_me_usage_search_keywords(search: &str) -> Vec<String> {
    search
        .split_whitespace()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn build_users_me_usage_api_key_payload(
    item: &StoredRequestUsageAudit,
    api_key_names: &BTreeMap<String, String>,
    auth_api_key_reader_available: bool,
) -> serde_json::Value {
    let api_key_name =
        users_me_usage_api_key_name(item, api_key_names, auth_api_key_reader_available);
    match item.api_key_id.as_deref() {
        Some(api_key_id) => json!({
            "id": api_key_id,
            "name": api_key_name.clone(),
            "display": api_key_name.unwrap_or_else(|| api_key_id.to_string()),
        }),
        None => serde_json::Value::Null,
    }
}

fn users_me_usage_request_body_stream_flag(item: &StoredRequestUsageAudit) -> Option<bool> {
    item.request_body
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|body| body.get("stream"))
        .and_then(serde_json::Value::as_bool)
}

fn users_me_usage_api_format_defaults_to_non_stream(item: &StoredRequestUsageAudit) -> bool {
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
        crate::ai_serving::normalize_api_format_alias(value).as_str(),
        "openai:chat"
            | "openai:responses"
            | "openai:responses:compact"
            | "openai:image"
            | "claude:messages"
    )
}

fn users_me_usage_request_body_implies_default_non_stream(item: &StoredRequestUsageAudit) -> bool {
    let Some(body) = item
        .request_body
        .as_ref()
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    !body.contains_key("stream") && users_me_usage_api_format_defaults_to_non_stream(item)
}

fn users_me_usage_headers_stream_flag(headers: Option<&serde_json::Value>) -> Option<bool> {
    let object = headers.and_then(serde_json::Value::as_object)?;
    let raw = object
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .and_then(|(_, value)| match value {
            serde_json::Value::String(text) => Some(text.as_str()),
            serde_json::Value::Array(values) => values.iter().find_map(serde_json::Value::as_str),
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

fn users_me_usage_body_is_sse_capture(value: Option<&serde_json::Value>) -> bool {
    let Some(object) = value.and_then(serde_json::Value::as_object) else {
        return false;
    };
    object
        .get("chunks")
        .and_then(serde_json::Value::as_array)
        .is_some()
        && object
            .get("metadata")
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata| metadata.get("stream"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
}

fn users_me_usage_infer_client_stream_from_captured_bodies(
    item: &StoredRequestUsageAudit,
) -> Option<bool> {
    let provider_stream = users_me_usage_body_is_sse_capture(item.response_body.as_ref());
    let client_stream = users_me_usage_body_is_sse_capture(item.client_response_body.as_ref());
    if client_stream {
        Some(true)
    } else if provider_stream && item.client_response_body.is_some() {
        Some(false)
    } else {
        None
    }
}

fn users_me_usage_infer_upstream_stream_from_captured_bodies(
    item: &StoredRequestUsageAudit,
) -> Option<bool> {
    let provider_stream = users_me_usage_body_is_sse_capture(item.response_body.as_ref());
    let client_stream = users_me_usage_body_is_sse_capture(item.client_response_body.as_ref());
    if provider_stream {
        Some(true)
    } else if client_stream && item.response_body.is_some() {
        Some(false)
    } else {
        None
    }
}

fn users_me_usage_client_is_stream(item: &StoredRequestUsageAudit) -> bool {
    item.request_metadata
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get("client_requested_stream"))
        .and_then(serde_json::Value::as_bool)
        .or_else(|| users_me_usage_request_body_stream_flag(item))
        .or_else(|| users_me_usage_headers_stream_flag(item.client_response_headers.as_ref()))
        .or_else(|| users_me_usage_request_body_implies_default_non_stream(item).then_some(false))
        .or_else(|| users_me_usage_infer_client_stream_from_captured_bodies(item))
        .unwrap_or(item.is_stream)
}

fn users_me_usage_upstream_is_stream(item: &StoredRequestUsageAudit) -> bool {
    item.request_metadata
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get("upstream_is_stream"))
        .and_then(serde_json::Value::as_bool)
        .or_else(|| users_me_usage_headers_stream_flag(item.response_headers.as_ref()))
        .or_else(|| users_me_usage_infer_upstream_stream_from_captured_bodies(item))
        .unwrap_or(item.is_stream)
}

fn users_me_usage_metadata_string<'a>(
    item: &'a StoredRequestUsageAudit,
    key: &str,
) -> Option<&'a str> {
    item.request_metadata
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn users_me_usage_metadata_number(item: &StoredRequestUsageAudit, key: &str) -> Option<f64> {
    item.request_metadata
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_f64)
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

fn users_me_usage_client_family(item: &StoredRequestUsageAudit) -> Option<&str> {
    item.client_family
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            item.request_metadata
                .as_ref()
                .and_then(serde_json::Value::as_object)
                .and_then(|metadata| {
                    metadata
                        .get("client_session_affinity")
                        .and_then(serde_json::Value::as_object)
                        .and_then(|affinity| affinity.get("client_family"))
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| {
                            metadata
                                .get("client_family")
                                .and_then(serde_json::Value::as_str)
                        })
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            users_me_usage_metadata_string(item, "user_agent")
                .and_then(infer_client_family_from_user_agent)
        })
}

fn users_me_usage_charge_breakdown(
    item: &StoredRequestUsageAudit,
    official_cost: f64,
    sales_multiplier: Option<f64>,
) -> UsersMeUsageChargeBreakdown {
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
        if official_cost > USERS_ME_USAGE_COST_EPSILON && wallet_debit > USERS_ME_USAGE_COST_EPSILON
        {
            Some(wallet_debit / official_cost)
        } else {
            None
        }
    });
    let package_multiplier = usage_package_multiplier_from_split(
        official_cost,
        package_debit,
        wallet_debit,
        wallet_multiplier,
        USERS_ME_USAGE_COST_EPSILON,
    );

    UsersMeUsageChargeBreakdown {
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

fn usage_package_multiplier_from_split(
    official_cost: f64,
    package_debit: f64,
    wallet_debit: f64,
    wallet_multiplier: Option<f64>,
    epsilon: f64,
) -> Option<f64> {
    if package_debit <= epsilon {
        return None;
    }
    let wallet_base = wallet_multiplier
        .filter(|value| value.is_finite() && *value > epsilon)
        .map(|multiplier| wallet_debit.max(0.0) / multiplier)
        .unwrap_or(0.0);
    let package_base = (official_cost.max(0.0) - wallet_base).max(0.0);
    if package_base > epsilon {
        Some(package_debit / package_base)
    } else {
        Some(1.0)
    }
}

fn build_users_me_usage_record_payload(
    item: &StoredRequestUsageAudit,
    include_actual_cost: bool,
    api_key_names: &BTreeMap<String, String>,
    auth_api_key_reader_available: bool,
) -> serde_json::Value {
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
    let cache_creation_input_tokens = users_me_usage_cache_creation_tokens(item);
    let rate_multiplier = item.settlement_rate_multiplier();
    let charge_breakdown = users_me_usage_charge_breakdown(item, official_cost, sales_multiplier);
    let client_is_stream = users_me_usage_client_is_stream(item);
    let upstream_is_stream = users_me_usage_upstream_is_stream(item);
    let mut payload = json!({
        "id": item.id,
        "model": item.model,
        "target_model": serde_json::Value::Null,
        "api_format": item.api_format,
        "endpoint_api_format": item.endpoint_api_format,
        "has_format_conversion": item.has_format_conversion,
        "input_tokens": item.input_tokens,
        "effective_input_tokens": users_me_usage_effective_input_tokens(item),
        "output_tokens": item.output_tokens,
        "total_tokens": item.total_tokens,
        "official_cost": round_to(official_cost, 6),
        "cost": round_to(charge_breakdown.user_debit_usd, 6),
        "sales_multiplier": sales_multiplier,
        "charge_breakdown": charge_breakdown.payload,
        "model_cost": users_me_usage_metadata_number(item, "model_cost_usd"),
        "moderation_cost": users_me_usage_metadata_number(item, "content_moderation_cost_usd"),
        "response_time_ms": item.response_time_ms,
        "first_byte_time_ms": item.first_byte_time_ms,
        "is_stream": item.is_stream,
        "upstream_is_stream": upstream_is_stream,
        "client_requested_stream": client_is_stream,
        "client_is_stream": client_is_stream,
        "client_family": users_me_usage_client_family(item),
        "client_ip": users_me_usage_metadata_string(item, "client_ip"),
        "user_agent": users_me_usage_metadata_string(item, "user_agent"),
        "request_path": users_me_usage_metadata_string(item, "request_path"),
        "request_path_and_query": users_me_usage_metadata_string(item, "request_path_and_query"),
        "status": item.status,
        "has_fallback": item.has_fallback(),
        "created_at": unix_secs_to_rfc3339(item.created_at_unix_ms),
        "cache_creation_input_tokens": cache_creation_input_tokens,
        "cache_creation_ephemeral_5m_input_tokens": item.cache_creation_ephemeral_5m_input_tokens,
        "cache_creation_ephemeral_1h_input_tokens": item.cache_creation_ephemeral_1h_input_tokens,
        "cache_read_input_tokens": item.cache_read_input_tokens,
        "status_code": item.status_code,
        "error_message": item.error_message,
        "input_price_per_1m": input_price_per_1m,
        "output_price_per_1m": output_price_per_1m,
        "cache_creation_price_per_1m": cache_creation_price_per_1m,
        "cache_read_price_per_1m": cache_read_price_per_1m,
        "api_key": build_users_me_usage_api_key_payload(
            item,
            api_key_names,
            auth_api_key_reader_available,
        ),
    });
    if let Some(reasoning_effort) = item.provider_reasoning_effort() {
        payload["reasoning_effort"] = json!(reasoning_effort);
    }
    if let Some(requested_reasoning_effort) = item.requested_reasoning_effort() {
        payload["requested_reasoning_effort"] = json!(requested_reasoning_effort);
    }

    if item.target_model.is_some() {
        payload["target_model"] = json!(item.target_model.clone());
    }
    if include_actual_cost {
        payload["actual_cost"] = json!(round_to(item.actual_total_cost_usd, 6));
        payload["actual_model_cost"] = json!(users_me_usage_metadata_number(
            item,
            "actual_model_cost_usd"
        ));
        payload["actual_moderation_cost"] = json!(users_me_usage_metadata_number(
            item,
            "content_moderation_actual_cost_usd"
        ));
        payload["rate_multiplier"] = json!(rate_multiplier);
    }
    payload
}

fn build_users_me_usage_active_payload(
    item: &StoredRequestUsageAudit,
    include_actual_cost: bool,
) -> serde_json::Value {
    let cache_creation_input_tokens = users_me_usage_cache_creation_tokens(item);
    let client_is_stream = users_me_usage_client_is_stream(item);
    let upstream_is_stream = users_me_usage_upstream_is_stream(item);
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
    let charge_breakdown = users_me_usage_charge_breakdown(item, official_cost, sales_multiplier);
    let mut payload = json!({
        "id": item.id,
        "status": item.status,
        "input_tokens": item.input_tokens,
        "effective_input_tokens": users_me_usage_effective_input_tokens(item),
        "output_tokens": item.output_tokens,
        "cache_creation_input_tokens": cache_creation_input_tokens,
        "cache_creation_ephemeral_5m_input_tokens": item.cache_creation_ephemeral_5m_input_tokens,
        "cache_creation_ephemeral_1h_input_tokens": item.cache_creation_ephemeral_1h_input_tokens,
        "cache_read_input_tokens": item.cache_read_input_tokens,
        "official_cost": round_to(official_cost, 6),
        "cost": round_to(charge_breakdown.user_debit_usd, 6),
        "sales_multiplier": sales_multiplier,
        "charge_breakdown": charge_breakdown.payload,
        "model_cost": users_me_usage_metadata_number(item, "model_cost_usd"),
        "moderation_cost": users_me_usage_metadata_number(item, "content_moderation_cost_usd"),
        "response_time_ms": item.response_time_ms,
        "first_byte_time_ms": item.first_byte_time_ms,
        "status_code": item.status_code,
        "error_message": item.error_message,
        "api_format": item.api_format,
        "endpoint_api_format": item.endpoint_api_format,
        "is_stream": item.is_stream,
        "upstream_is_stream": upstream_is_stream,
        "client_requested_stream": client_is_stream,
        "client_is_stream": client_is_stream,
        "has_format_conversion": item.has_format_conversion,
        "client_family": users_me_usage_client_family(item),
        "client_ip": users_me_usage_metadata_string(item, "client_ip"),
        "user_agent": users_me_usage_metadata_string(item, "user_agent"),
        "target_model": item.target_model,
        "has_fallback": item.has_fallback(),
    });
    if let Some(reasoning_effort) = item.provider_reasoning_effort() {
        payload["reasoning_effort"] = json!(reasoning_effort);
    }
    if let Some(requested_reasoning_effort) = item.requested_reasoning_effort() {
        payload["requested_reasoning_effort"] = json!(requested_reasoning_effort);
    }
    if include_actual_cost {
        payload["actual_cost"] = json!(round_to(item.actual_total_cost_usd, 6));
        payload["actual_model_cost"] = json!(users_me_usage_metadata_number(
            item,
            "actual_model_cost_usd"
        ));
        payload["actual_moderation_cost"] = json!(users_me_usage_metadata_number(
            item,
            "content_moderation_actual_cost_usd"
        ));
        payload["rate_multiplier"] = json!(item.settlement_rate_multiplier());
    }
    if item.api_format.is_none() {
        payload
            .as_object_mut()
            .expect("object")
            .remove("api_format");
    }
    if item.endpoint_api_format.is_none() {
        payload
            .as_object_mut()
            .expect("object")
            .remove("endpoint_api_format");
    }
    if item.target_model.is_none() {
        payload
            .as_object_mut()
            .expect("object")
            .remove("target_model");
    }
    payload
}

fn apply_users_me_usage_discount_terms(payload: &mut serde_json::Value) {
    let Some(record) = payload.as_object_mut() else {
        return;
    };
    if let Some(value) = record.remove("sales_multiplier") {
        record.insert("discount".to_string(), value);
    }
    if let Some(breakdown) = record
        .get_mut("charge_breakdown")
        .and_then(serde_json::Value::as_object_mut)
    {
        if let Some(value) = breakdown.remove("wallet_multiplier") {
            breakdown.insert("wallet_discount".to_string(), value);
        }
    }
}

fn users_me_usage_is_failed(item: &StoredRequestUsageAudit) -> bool {
    let has_failure_signal = item.status_code.is_some_and(|value| value >= 400)
        || item
            .error_message
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
    let status = item.status.trim().to_ascii_lowercase();
    if status.is_empty() {
        return has_failure_signal;
    }
    match status.as_str() {
        "completed" | "cancelled" => false,
        "pending" | "streaming" => has_failure_signal,
        "failed" => true,
        _ => false,
    }
}

fn build_users_me_usage_summary_by_model(
    rows: &[StoredUsageBreakdownSummaryRow],
    include_actual_cost: bool,
) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let mut value = json!({
                "model": row.group_key,
                "requests": row.request_count,
                "input_tokens": row.input_tokens,
                "effective_input_tokens": row.effective_input_tokens,
                "output_tokens": row.output_tokens,
                "total_tokens": row.total_tokens,
                "cache_read_tokens": row.cache_read_tokens,
                "cache_creation_tokens": row.cache_creation_tokens,
                "cache_creation_ephemeral_5m_tokens": row.cache_creation_ephemeral_5m_tokens,
                "cache_creation_ephemeral_1h_tokens": row.cache_creation_ephemeral_1h_tokens,
                "total_input_context": row.total_input_context,
                "cache_hit_rate": users_me_usage_cache_hit_rate(
                    row.total_input_context,
                    row.cache_read_tokens,
                ),
                "official_cost": round_to(row.official_cost_usd, 6),
                "total_cost_usd": round_to(row.total_cost_usd, 6),
            });
            if include_actual_cost {
                value["actual_total_cost_usd"] = json!(round_to(row.actual_total_cost_usd, 6));
            }
            value
        })
        .collect()
}

fn build_users_me_usage_summary_by_provider(
    rows: &[StoredUsageBreakdownSummaryRow],
) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            json!({
                "provider": row.group_key,
                "requests": row.request_count,
                "effective_input_tokens": row.effective_input_tokens,
                "total_tokens": row.total_tokens,
                "total_input_context": row.total_input_context,
                "output_tokens": row.output_tokens,
                "cache_read_tokens": row.cache_read_tokens,
                "cache_creation_tokens": row.cache_creation_tokens,
                "cache_creation_ephemeral_5m_tokens": row.cache_creation_ephemeral_5m_tokens,
                "cache_creation_ephemeral_1h_tokens": row.cache_creation_ephemeral_1h_tokens,
                "cache_hit_rate": users_me_usage_cache_hit_rate(
                    row.total_input_context,
                    row.cache_read_tokens,
                ),
                "total_cost_usd": round_to(row.total_cost_usd, 6),
                "success_rate": if row.request_count == 0 {
                    100.0
                } else {
                    round_to(row.success_count as f64 / row.request_count as f64 * 100.0, 2)
                },
                "avg_response_time_ms": if row.response_time_samples == 0 {
                    0.0
                } else {
                    round_to(row.response_time_sum_ms / row.response_time_samples as f64, 2)
                },
            })
        })
        .collect()
}

fn build_users_me_usage_summary_by_api_format(
    rows: &[StoredUsageBreakdownSummaryRow],
) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            json!({
                "api_format": row.group_key,
                "request_count": row.request_count,
                "total_tokens": row.total_tokens,
                "effective_input_tokens": row.effective_input_tokens,
                "total_input_context": row.total_input_context,
                "output_tokens": row.output_tokens,
                "cache_read_tokens": row.cache_read_tokens,
                "cache_creation_tokens": row.cache_creation_tokens,
                "cache_creation_ephemeral_5m_tokens": row.cache_creation_ephemeral_5m_tokens,
                "cache_creation_ephemeral_1h_tokens": row.cache_creation_ephemeral_1h_tokens,
                "cache_hit_rate": users_me_usage_cache_hit_rate(
                    row.total_input_context,
                    row.cache_read_tokens,
                ),
                "total_cost_usd": round_to(row.total_cost_usd, 6),
                "avg_response_time_ms": if row.overall_response_time_samples == 0 {
                    0.0
                } else {
                    round_to(
                        row.overall_response_time_sum_ms
                            / row.overall_response_time_samples as f64,
                        2,
                    )
                },
            })
        })
        .collect()
}

async fn load_users_me_usage_by_ids(
    state: &AppState,
    requested_ids: &BTreeSet<String>,
    user_id: &str,
) -> Result<Vec<StoredRequestUsageAudit>, GatewayError> {
    let usage_ids = requested_ids.iter().cloned().collect::<Vec<_>>();
    let mut items = state.list_request_usage_by_ids(&usage_ids).await?;
    items.retain(|item| item.user_id.as_deref() == Some(user_id));
    Ok(items)
}

async fn resolve_users_me_api_key_names(
    state: &AppState,
    items: &[StoredRequestUsageAudit],
) -> Result<BTreeMap<String, String>, GatewayError> {
    if !state.has_auth_api_key_data_reader() {
        return Ok(BTreeMap::new());
    }

    let api_key_ids = items
        .iter()
        .filter_map(|item| item.api_key_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if api_key_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    state.resolve_auth_api_key_names_by_ids(&api_key_ids).await
}

async fn resolve_users_me_search_api_key_context(
    state: &AppState,
    user_id: &str,
    keywords: &[String],
) -> Result<(BTreeMap<String, String>, Vec<Vec<String>>), GatewayError> {
    if !state.has_auth_api_key_data_reader() {
        return Ok((BTreeMap::new(), Vec::new()));
    }

    let user_ids = [user_id.to_string()];
    let export_records = state
        .list_auth_api_key_export_records_by_user_ids(&user_ids)
        .await?;
    let api_key_names = export_records
        .iter()
        .filter_map(|record| {
            record
                .name
                .clone()
                .map(|name| (record.api_key_id.clone(), name))
        })
        .collect::<BTreeMap<_, _>>();
    let matched_api_key_ids_by_keyword = keywords
        .iter()
        .map(|keyword| {
            export_records
                .iter()
                .filter(|record| {
                    record
                        .name
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(keyword)
                })
                .map(|record| record.api_key_id.clone())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    Ok((api_key_names, matched_api_key_ids_by_keyword))
}

async fn resolve_users_me_usage_provider_names_by_id<'a>(
    state: &AppState,
    candidate_groups: impl IntoIterator<Item = &'a Vec<StoredRequestCandidate>>,
) -> Result<BTreeMap<String, String>, GatewayError> {
    if !state.has_provider_catalog_data_reader() {
        return Ok(BTreeMap::new());
    }

    let provider_ids = candidate_groups
        .into_iter()
        .flat_map(|candidates| candidates.iter())
        .filter_map(|candidate| candidate.provider_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if provider_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let providers = state
        .read_provider_catalog_providers_by_ids(&provider_ids)
        .await?;
    Ok(providers
        .into_iter()
        .map(|provider| (provider.id, provider.name))
        .collect())
}

async fn resolve_users_me_usage_attempt_info_by_usage_id(
    state: &AppState,
    items: &[StoredRequestUsageAudit],
) -> Result<BTreeMap<String, AdminUsageAttemptInfo>, GatewayError> {
    if !state.has_request_candidate_data_reader() || items.is_empty() {
        return Ok(BTreeMap::new());
    }

    let request_ids = items
        .iter()
        .map(|item| item.request_id.clone())
        .collect::<BTreeSet<_>>();
    let mut candidates_by_request_id = BTreeMap::new();
    for request_id in request_ids {
        candidates_by_request_id.insert(
            request_id.clone(),
            state
                .read_request_candidates_by_request_id(&request_id)
                .await?,
        );
    }

    let provider_names_by_id =
        resolve_users_me_usage_provider_names_by_id(state, candidates_by_request_id.values())
            .await?;

    Ok(items
        .iter()
        .filter_map(|item| {
            let candidates = candidates_by_request_id.get(&item.request_id)?;
            Some((
                item.id.clone(),
                admin_usage_attempt_info_from_candidates(item, candidates, &provider_names_by_id),
            ))
        })
        .collect())
}

fn apply_users_me_usage_attempt_info_to_payload(
    record: &mut serde_json::Value,
    item: &StoredRequestUsageAudit,
    attempt_info_by_usage_id: &BTreeMap<String, AdminUsageAttemptInfo>,
    request_candidate_reader_available: bool,
) {
    let info = admin_usage_attempt_info_for_item(
        item,
        attempt_info_by_usage_id,
        request_candidate_reader_available,
    );
    record["has_fallback"] = json!(info.has_fallback);
    record["has_retry"] = json!(info.has_retry);
    if !info.provider_route.is_empty() {
        admin_usage_apply_provider_route_display(record, &info.provider_route);
        record["provider_route"] = json!(info.provider_route);
    }
}

pub(super) async fn handle_users_me_usage_get(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    if !state.has_usage_data_reader() {
        return build_users_me_usage_reader_unavailable_response();
    }

    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let use_discount_terms = match resolve_user_portal(state, &auth.user.id).await {
        Ok(portal) => portal.is_official_usd(),
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("user portal lookup failed: {err:?}"),
                false,
            )
        }
    };
    let query = request_context.request_query_string.as_deref();
    let time_range = match AdminStatsTimeRange::resolve_optional(query) {
        Ok(value) => value,
        Err(detail) => return admin_stats_bad_request_response(detail),
    };
    let search = query_param_value(query, "search");
    let limit = match parse_users_me_usage_limit(query) {
        Ok(value) => value,
        Err(detail) => return admin_stats_bad_request_response(detail),
    };
    let offset = match parse_users_me_usage_offset(query) {
        Ok(value) => value,
        Err(detail) => return admin_stats_bad_request_response(detail),
    };
    let include_records = users_me_usage_should_include_records(query);

    // When no time range is specified, default to 7 days to avoid full-table scans.
    let effective_time_range = time_range.or_else(|| {
        let today = Utc::now().date_naive();
        let start_date = today
            .checked_sub_signed(chrono::Duration::days(6))
            .unwrap_or(today);
        Some(AdminStatsTimeRange {
            start_date,
            end_date: today,
            tz_offset_minutes: 0,
            start_time: None,
            end_time: None,
        })
    });

    let include_actual_cost = auth.user.role.eq_ignore_ascii_case("admin");
    let auth_api_key_reader_available = state.has_auth_api_key_data_reader();
    let mut usage_summary =
        aether_data_contracts::repository::usage::StoredUsageDashboardSummary::default();
    let mut summary_by_model = Vec::<StoredUsageBreakdownSummaryRow>::new();
    let mut summary_by_provider = Vec::<StoredUsageBreakdownSummaryRow>::new();
    let mut summary_by_api_format = Vec::<StoredUsageBreakdownSummaryRow>::new();
    let mut api_key_names = BTreeMap::new();
    let mut total_record_count = 0usize;
    let mut record_items = Vec::<StoredRequestUsageAudit>::new();
    let mut record_attempt_info_by_usage_id = BTreeMap::<String, AdminUsageAttemptInfo>::new();

    if let Some((created_from_unix_secs, created_until_unix_secs)) = effective_time_range
        .as_ref()
        .and_then(AdminStatsTimeRange::to_unix_bounds)
    {
        usage_summary = match state
            .summarize_dashboard_usage(&UsageDashboardSummaryQuery {
                created_from_unix_secs,
                created_until_unix_secs,
                user_id: Some(auth.user.id.clone()),
            })
            .await
        {
            Ok(value) => value,
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("user usage summary lookup failed: {err:?}"),
                    false,
                );
            }
        };
        summary_by_model = match state
            .summarize_usage_breakdown(&UsageBreakdownSummaryQuery {
                created_from_unix_secs,
                created_until_unix_secs,
                user_id: Some(auth.user.id.clone()),
                group_by: UsageBreakdownGroupBy::Model,
            })
            .await
        {
            Ok(value) => value,
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("user usage model breakdown lookup failed: {err:?}"),
                    false,
                );
            }
        };
        if include_actual_cost {
            summary_by_provider = match state
                .summarize_usage_breakdown(&UsageBreakdownSummaryQuery {
                    created_from_unix_secs,
                    created_until_unix_secs,
                    user_id: Some(auth.user.id.clone()),
                    group_by: UsageBreakdownGroupBy::Provider,
                })
                .await
            {
                Ok(value) => value,
                Err(err) => {
                    return build_auth_error_response(
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("user usage provider breakdown lookup failed: {err:?}"),
                        false,
                    );
                }
            };
        }
        summary_by_api_format = match state
            .summarize_usage_breakdown(&UsageBreakdownSummaryQuery {
                created_from_unix_secs,
                created_until_unix_secs,
                user_id: Some(auth.user.id.clone()),
                group_by: UsageBreakdownGroupBy::ApiFormat,
            })
            .await
        {
            Ok(value) => value,
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("user usage api_format breakdown lookup failed: {err:?}"),
                    false,
                );
            }
        };

        if include_records {
            let active_search = search.as_deref().filter(|value| !value.trim().is_empty());
            let status_filter = query_param_value(query, "status");
            let attempt_status_filter = admin_usage_attempt_status_filter(status_filter.as_deref());
            let active_client_family_filter =
                query_param_value(query, "client_family").filter(|value| !value.trim().is_empty());
            let needs_attempt_or_client_filter =
                attempt_status_filter.is_some() || active_client_family_filter.is_some();
            let base_query = build_users_me_usage_records_query(
                created_from_unix_secs,
                created_until_unix_secs,
                &auth.user.id,
                query,
                None,
                None,
            );

            let keyword_query = if let Some(search) = active_search {
                let keywords = parse_users_me_usage_search_keywords(search);
                let matched_api_key_ids_by_keyword;
                (api_key_names, matched_api_key_ids_by_keyword) =
                    match resolve_users_me_search_api_key_context(state, &auth.user.id, &keywords)
                        .await
                    {
                        Ok(value) => value,
                        Err(err) => {
                            return build_auth_error_response(
                                http::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("user api key search context lookup failed: {err:?}"),
                                false,
                            );
                        }
                    };
                Some(build_users_me_usage_keyword_search_query(
                    &base_query,
                    keywords,
                    matched_api_key_ids_by_keyword,
                    auth_api_key_reader_available,
                    None,
                    None,
                ))
            } else {
                None
            };

            if needs_attempt_or_client_filter {
                let mut usage = match keyword_query.as_ref() {
                    Some(query) => match state.list_usage_audits_by_keyword_search(query).await {
                        Ok(value) => value,
                        Err(err) => {
                            return build_auth_error_response(
                                http::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("user usage search lookup failed: {err:?}"),
                                false,
                            );
                        }
                    },
                    None => match state.list_usage_audits(&base_query).await {
                        Ok(value) => value,
                        Err(err) => {
                            return build_auth_error_response(
                                http::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("user usage records lookup failed: {err:?}"),
                                false,
                            );
                        }
                    },
                };
                let attempt_info_by_usage_id =
                    match resolve_users_me_usage_attempt_info_by_usage_id(state, &usage).await {
                        Ok(value) => value,
                        Err(err) => {
                            return build_auth_error_response(
                                http::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("user usage attempt info lookup failed: {err:?}"),
                                false,
                            );
                        }
                    };
                let request_candidate_reader_available = state.has_request_candidate_data_reader();
                usage.retain(|item| {
                    attempt_status_filter.is_none_or(|attempt_status| {
                        admin_usage_matches_attempt_status(
                            item,
                            attempt_status,
                            &attempt_info_by_usage_id,
                            request_candidate_reader_available,
                        )
                    }) && admin_usage_matches_client_family(
                        item,
                        active_client_family_filter.as_deref(),
                    )
                });
                usage.sort_by(|left, right| {
                    right
                        .created_at_unix_ms
                        .cmp(&left.created_at_unix_ms)
                        .then_with(|| left.id.cmp(&right.id))
                });
                total_record_count = usage.len();
                record_items = usage
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>();
                record_attempt_info_by_usage_id = attempt_info_by_usage_id;
            } else if let Some(keyword_query) = keyword_query {
                total_record_count = match state
                    .count_usage_audits_by_keyword_search(&keyword_query)
                    .await
                {
                    Ok(value) => usize::try_from(value).unwrap_or(usize::MAX),
                    Err(err) => {
                        return build_auth_error_response(
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            format!("user usage search count lookup failed: {err:?}"),
                            false,
                        );
                    }
                };
                record_items = match state
                    .list_usage_audits_by_keyword_search(&UsageAuditKeywordSearchQuery {
                        limit: Some(limit),
                        offset: Some(offset),
                        ..keyword_query
                    })
                    .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        return build_auth_error_response(
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            format!("user usage search lookup failed: {err:?}"),
                            false,
                        );
                    }
                };
            } else {
                total_record_count = match state.count_usage_audits(&base_query).await {
                    Ok(value) => usize::try_from(value).unwrap_or(usize::MAX),
                    Err(err) => {
                        return build_auth_error_response(
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            format!("user usage count lookup failed: {err:?}"),
                            false,
                        );
                    }
                };
                record_items = match state
                    .list_usage_audits(&UsageAuditListQuery {
                        limit: Some(limit),
                        offset: Some(offset),
                        ..base_query
                    })
                    .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        return build_auth_error_response(
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            format!("user usage records lookup failed: {err:?}"),
                            false,
                        );
                    }
                };
            }

            if api_key_names.is_empty() {
                api_key_names = match resolve_users_me_api_key_names(state, &record_items).await {
                    Ok(value) => value,
                    Err(err) => {
                        return build_auth_error_response(
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            format!("user api key name lookup failed: {err:?}"),
                            false,
                        );
                    }
                };
            }
        }
    }

    let total_requests = usage_summary.total_requests;
    let total_input_tokens = usage_summary.input_tokens;
    let total_output_tokens = usage_summary.output_tokens;
    let total_tokens = usage_summary.total_tokens;
    let total_cost = round_to(usage_summary.total_cost_usd, 6);
    let total_actual_cost = round_to(usage_summary.actual_total_cost_usd, 6);
    let avg_response_time = if usage_summary.response_time_samples == 0 {
        0.0
    } else {
        round_to(
            usage_summary.response_time_sum_ms
                / usage_summary.response_time_samples as f64
                / 1000.0,
            2,
        )
    };

    if record_attempt_info_by_usage_id.is_empty() {
        record_attempt_info_by_usage_id =
            match resolve_users_me_usage_attempt_info_by_usage_id(state, &record_items).await {
                Ok(value) => value,
                Err(err) => {
                    return build_auth_error_response(
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("user usage attempt info lookup failed: {err:?}"),
                        false,
                    );
                }
            };
    }
    let request_candidate_reader_available = state.has_request_candidate_data_reader();
    let records = record_items
        .into_iter()
        .map(|item| {
            let mut record = build_users_me_usage_record_payload(
                &item,
                include_actual_cost,
                &api_key_names,
                auth_api_key_reader_available,
            );
            apply_users_me_usage_attempt_info_to_payload(
                &mut record,
                &item,
                &record_attempt_info_by_usage_id,
                request_candidate_reader_available,
            );
            if use_discount_terms {
                apply_users_me_usage_discount_terms(&mut record);
            }
            record
        })
        .collect::<Vec<_>>();

    let wallet = state
        .read_wallet_snapshot_for_auth(&auth.user.id, "", false)
        .await
        .ok()
        .flatten();

    let mut payload = json!({
        "total_requests": total_requests,
        "total_input_tokens": total_input_tokens,
        "total_output_tokens": total_output_tokens,
        "total_tokens": total_tokens,
        "total_cost": total_cost,
        "avg_response_time": avg_response_time,
        "billing": build_auth_wallet_summary_payload(wallet.as_ref()),
        "summary_by_model": build_users_me_usage_summary_by_model(&summary_by_model, include_actual_cost),
        "summary_by_api_format": build_users_me_usage_summary_by_api_format(&summary_by_api_format),
        "pagination": {
            "total": total_record_count,
            "limit": limit,
            "offset": offset,
            "has_more": offset.saturating_add(limit) < total_record_count,
        },
        "records": records,
    });
    if include_actual_cost {
        payload["total_actual_cost"] = json!(total_actual_cost);
        payload["summary_by_provider"] = json!(build_users_me_usage_summary_by_provider(
            &summary_by_provider
        ));
    }
    Json(payload).into_response()
}

pub(super) async fn handle_users_me_usage_active_get(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    if !state.has_usage_data_reader() {
        return build_users_me_usage_reader_unavailable_response();
    }

    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let use_discount_terms = match resolve_user_portal(state, &auth.user.id).await {
        Ok(portal) => portal.is_official_usd(),
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("user portal lookup failed: {err:?}"),
                false,
            )
        }
    };
    let ids = parse_users_me_usage_ids(request_context.request_query_string.as_deref());
    // When polling for active (pending/streaming) requests without specific ids,
    // limit to the last 1 hour to avoid scanning all historical records.
    let items = match ids.as_ref() {
        Some(ids) => match load_users_me_usage_by_ids(state, ids, &auth.user.id).await {
            Ok(mut value) => {
                value.sort_by(|left, right| {
                    right
                        .created_at_unix_ms
                        .cmp(&left.created_at_unix_ms)
                        .then_with(|| left.id.cmp(&right.id))
                });
                value
            }
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("user active usage lookup failed: {err:?}"),
                    false,
                );
            }
        },
        None => match state
            .list_usage_audits(&UsageAuditListQuery {
                created_from_unix_secs: Some(Utc::now().timestamp().saturating_sub(3600) as u64),
                created_until_unix_secs: None,
                user_id: Some(auth.user.id.clone()),
                api_key_ids: None,
                provider_name: None,
                model: None,
                api_format: None,
                statuses: Some(vec!["pending".to_string(), "streaming".to_string()]),
                is_stream: None,
                error_only: false,
                limit: Some(50),
                offset: None,
                newest_first: true,
            })
            .await
        {
            Ok(value) => value,
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("user active usage lookup failed: {err:?}"),
                    false,
                );
            }
        },
    };

    let items = if ids.is_some() {
        items
    } else {
        items
            .into_iter()
            .filter(|item| !users_me_usage_is_failed(item))
            .collect::<Vec<_>>()
    };

    let include_actual_cost = auth.user.role.eq_ignore_ascii_case("admin");

    Json(json!({
        "requests": items
            .iter()
            .map(|item| {
                let mut payload = build_users_me_usage_active_payload(item, include_actual_cost);
                if use_discount_terms {
                    apply_users_me_usage_discount_terms(&mut payload);
                }
                payload
            })
            .collect::<Vec<_>>(),
    }))
    .into_response()
}

pub(super) async fn handle_users_me_usage_interval_timeline_get(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    if !state.has_usage_data_reader() {
        return build_users_me_usage_reader_unavailable_response();
    }

    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let query = request_context.request_query_string.as_deref();
    let hours = match parse_users_me_usage_hours(query) {
        Ok(value) => value,
        Err(detail) => return admin_stats_bad_request_response(detail),
    };
    let limit = match parse_users_me_usage_timeline_limit(query) {
        Ok(value) => value,
        Err(detail) => return admin_stats_bad_request_response(detail),
    };
    let now_unix_secs = u64::try_from(Utc::now().timestamp()).unwrap_or_default();
    let created_from_unix_secs = now_unix_secs.saturating_sub(u64::from(hours) * 3600);

    let intervals = match state
        .list_usage_cache_affinity_intervals(&UsageCacheAffinityIntervalQuery {
            created_from_unix_secs,
            created_until_unix_secs: now_unix_secs.saturating_add(1),
            group_by: UsageCacheAffinityIntervalGroupBy::User,
            user_id: Some(auth.user.id.clone()),
            api_key_id: None,
        })
        .await
    {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("user interval timeline lookup failed: {err:?}"),
                false,
            );
        }
    };

    let mut points = Vec::new();
    for row in intervals {
        points.push(json!({
            "x": unix_secs_to_rfc3339(row.created_at_unix_secs),
            "y": round_to(row.interval_minutes, 2),
            "model": row.model,
        }));
        if points.len() >= limit {
            break;
        }
    }

    Json(json!({
        "analysis_period_hours": hours,
        "total_points": points.len(),
        "points": points,
    }))
    .into_response()
}

pub(super) async fn handle_users_me_usage_heatmap_get(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    if !state.has_usage_data_reader() {
        return build_users_me_usage_reader_unavailable_response();
    }

    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let today = Utc::now().date_naive();
    let start_date = today
        .checked_sub_signed(chrono::Duration::days(364))
        .unwrap_or(today);
    let Some(start_of_day) = start_date.and_hms_opt(0, 0, 0) else {
        return build_auth_error_response(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "heatmap start date is invalid",
            false,
        );
    };
    let created_from_unix_secs = u64::try_from(
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(start_of_day, chrono::Utc)
            .timestamp(),
    )
    .unwrap_or_default();

    let summaries = match build_usage_heatmap_summaries(
        state,
        created_from_unix_secs,
        start_date,
        today,
        Some(auth.user.id.as_str()),
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("user heatmap lookup failed: {err:?}"),
                false,
            );
        }
    };

    let include_actual_cost = auth.user.role.eq_ignore_ascii_case("admin");
    let grouped: std::collections::HashMap<String, _> =
        summaries.into_iter().map(|s| (s.date.clone(), s)).collect();

    let mut max_requests = 0_u64;
    let mut active_days = 0_u64;
    let mut cursor = start_date;
    let mut days = Vec::new();
    while cursor <= today {
        let date_str = cursor.to_string();
        let (requests, total_tokens, total_cost, actual_total_cost) =
            if let Some(s) = grouped.get(&date_str) {
                (
                    s.requests,
                    s.total_tokens,
                    s.total_cost_usd,
                    s.actual_total_cost_usd,
                )
            } else {
                (0, 0, 0.0, 0.0)
            };
        max_requests = max_requests.max(requests);
        if requests > 0 {
            active_days = active_days.saturating_add(1);
        }
        let mut day = json!({
            "date": date_str,
            "requests": requests,
            "total_tokens": total_tokens,
            "total_cost": round_to(total_cost, 6),
        });
        if include_actual_cost {
            day["actual_total_cost"] = json!(round_to(actual_total_cost, 6));
        }
        days.push(day);
        cursor = cursor
            .checked_add_signed(chrono::Duration::days(1))
            .unwrap_or(today + chrono::Duration::days(1));
    }

    Json(json!({
        "start_date": start_date.to_string(),
        "end_date": today.to_string(),
        "total_days": days.len(),
        "active_days": active_days,
        "max_requests": max_requests,
        "days": days,
    }))
    .into_response()
}

async fn build_usage_heatmap_summaries(
    state: &AppState,
    created_from_unix_secs: u64,
    _start_date: chrono::NaiveDate,
    _today: chrono::NaiveDate,
    user_id: Option<&str>,
) -> Result<Vec<StoredUsageDailySummary>, GatewayError> {
    let query = aether_data_contracts::repository::usage::UsageDailyHeatmapQuery {
        created_from_unix_secs,
        user_id: user_id.map(ToOwned::to_owned),
        admin_mode: user_id.is_none(),
    };
    let mut summaries = state.summarize_usage_daily_heatmap(&query).await?;
    summaries.sort_by(|left, right| left.date.cmp(&right.date));
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aether_data_contracts::repository::usage::StoredRequestUsageAudit;
    use serde_json::json;

    use super::{
        apply_users_me_usage_discount_terms, build_users_me_usage_active_payload,
        build_users_me_usage_record_payload, users_me_usage_client_is_stream,
        users_me_usage_is_failed, users_me_usage_upstream_is_stream,
    };

    #[test]
    fn dedicated_portal_usage_exposes_discount_terms_only() {
        let mut payload = json!({
            "sales_multiplier": 1.2,
            "charge_breakdown": {
                "package_multiplier": 1.0,
                "wallet_multiplier": 1.2
            }
        });

        apply_users_me_usage_discount_terms(&mut payload);

        assert_eq!(payload["discount"], json!(1.2));
        assert_eq!(payload["charge_breakdown"]["wallet_discount"], json!(1.2));
        assert_eq!(
            payload["charge_breakdown"]["package_multiplier"],
            json!(1.0)
        );
        assert!(payload.get("sales_multiplier").is_none());
        assert!(payload["charge_breakdown"]
            .get("wallet_multiplier")
            .is_none());
    }

    fn sample_usage(status: &str) -> StoredRequestUsageAudit {
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
            Some(200),
            None,
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
    fn user_usage_record_payload_rehydrates_cache_creation_total_from_classified_fields() {
        let item = StoredRequestUsageAudit {
            cache_creation_input_tokens: 0,
            cache_creation_ephemeral_5m_input_tokens: 9,
            cache_creation_ephemeral_1h_input_tokens: 11,
            ..sample_usage("completed")
        };

        let payload = build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);

        assert_eq!(payload["cache_creation_input_tokens"], 20);
        assert_eq!(payload["cache_creation_ephemeral_5m_input_tokens"], 9);
        assert_eq!(payload["cache_creation_ephemeral_1h_input_tokens"], 11);
    }

    #[test]
    fn user_usage_active_payload_rehydrates_cache_creation_total_from_classified_fields() {
        let item = StoredRequestUsageAudit {
            cache_creation_input_tokens: 0,
            cache_creation_ephemeral_5m_input_tokens: 4,
            cache_creation_ephemeral_1h_input_tokens: 6,
            ..sample_usage("streaming")
        };

        let payload = build_users_me_usage_active_payload(&item, false);

        assert_eq!(payload["cache_creation_input_tokens"], 10);
        assert_eq!(payload["cache_creation_ephemeral_5m_input_tokens"], 4);
        assert_eq!(payload["cache_creation_ephemeral_1h_input_tokens"], 6);
    }

    #[test]
    fn user_usage_payload_keeps_claude_effective_input_when_cache_read_is_large() {
        let item = StoredRequestUsageAudit {
            provider_name: "Claude".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            api_format: Some("claude:messages".to_string()),
            api_family: Some("claude".to_string()),
            endpoint_api_format: Some("claude:messages".to_string()),
            provider_api_family: Some("claude".to_string()),
            input_tokens: 4941,
            output_tokens: 973,
            total_tokens: 59474,
            cache_creation_input_tokens: 687,
            cache_read_input_tokens: 52873,
            ..sample_usage("completed")
        };

        let payload = build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);

        assert_eq!(payload["input_tokens"], 4941);
        assert_eq!(payload["effective_input_tokens"], 4941);
        assert_eq!(payload["cache_creation_input_tokens"], 687);
        assert_eq!(payload["cache_read_input_tokens"], 52873);
    }

    #[test]
    fn user_usage_active_pending_with_failure_signal_is_not_active() {
        let item = StoredRequestUsageAudit {
            status_code: Some(503),
            error_message: Some("upstream failed".to_string()),
            ..sample_usage("pending")
        };

        assert!(users_me_usage_is_failed(&item));
    }

    #[test]
    fn user_usage_payloads_include_symmetric_stream_fields() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            request_metadata: Some(json!({
                "client_requested_stream": false
            })),
            ..sample_usage("completed")
        };

        assert!(!users_me_usage_client_is_stream(&item));

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);
        assert_eq!(record_payload["is_stream"], true);
        assert_eq!(record_payload["upstream_is_stream"], true);
        assert_eq!(record_payload["client_requested_stream"], false);
        assert_eq!(record_payload["client_is_stream"], false);

        let active_payload = build_users_me_usage_active_payload(&item, false);
        assert_eq!(active_payload["is_stream"], true);
        assert_eq!(active_payload["upstream_is_stream"], true);
        assert_eq!(active_payload["client_requested_stream"], false);
        assert_eq!(active_payload["client_is_stream"], false);
    }

    #[test]
    fn user_usage_payload_infers_client_family_from_user_agent() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "client_ip": "192.168.0.28",
                "user_agent": "codex_vscode/0.131.0-alpha.9 (Windows 10.0.26200; x86_64)"
            })),
            ..sample_usage("completed")
        };

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);
        let active_payload = build_users_me_usage_active_payload(&item, false);

        assert_eq!(record_payload["client_family"], "codex_vscode");
        assert_eq!(record_payload["client_ip"], "192.168.0.28");
        assert_eq!(active_payload["client_family"], "codex_vscode");
        assert_eq!(active_payload["client_ip"], "192.168.0.28");
    }

    #[test]
    fn user_usage_payloads_include_moderation_cost_without_actual_cost_for_regular_users() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "model_cost_usd": 0.15,
                "actual_model_cost_usd": 0.8,
                "content_moderation_cost_usd": 0.0001,
                "content_moderation_actual_cost_usd": 0.00008
            })),
            ..sample_usage("completed")
        };

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);
        let active_payload = build_users_me_usage_active_payload(&item, false);

        assert_eq!(record_payload["model_cost"], json!(0.15));
        assert_eq!(record_payload["moderation_cost"], json!(0.0001));
        assert!(record_payload.get("actual_model_cost").is_none());
        assert!(record_payload.get("actual_moderation_cost").is_none());
        assert_eq!(active_payload["model_cost"], json!(0.15));
        assert_eq!(active_payload["moderation_cost"], json!(0.0001));
        assert!(active_payload.get("actual_model_cost").is_none());
        assert!(active_payload.get("actual_moderation_cost").is_none());
    }

    #[test]
    fn user_usage_payloads_include_actual_moderation_cost_for_admin_users() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "model_cost_usd": 0.15,
                "actual_model_cost_usd": 0.8,
                "content_moderation_cost_usd": 0.0001,
                "content_moderation_actual_cost_usd": 0.00008
            })),
            ..sample_usage("completed")
        };

        let record_payload =
            build_users_me_usage_record_payload(&item, true, &BTreeMap::new(), false);
        let active_payload = build_users_me_usage_active_payload(&item, true);

        assert_eq!(record_payload["actual_model_cost"], json!(0.8));
        assert_eq!(record_payload["actual_moderation_cost"], json!(0.00008));
        assert_eq!(active_payload["actual_model_cost"], json!(0.8));
        assert_eq!(active_payload["actual_moderation_cost"], json!(0.00008));
    }

    #[test]
    fn user_usage_payload_labels_openai_js_user_agent_as_sdk() {
        let item = StoredRequestUsageAudit {
            request_metadata: Some(json!({
                "user_agent": "OpenAI/JS 6.34.0"
            })),
            ..sample_usage("completed")
        };

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);

        assert_eq!(record_payload["client_family"], "openai_js_sdk");
    }

    #[test]
    fn user_usage_stream_inference_falls_back_to_request_body_stream_flag() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            request_body: Some(json!({
                "model": "gpt-5.4",
                "stream": false
            })),
            ..sample_usage("completed")
        };

        assert!(!users_me_usage_client_is_stream(&item));

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);
        assert_eq!(record_payload["is_stream"], true);
        assert_eq!(record_payload["upstream_is_stream"], true);
        assert_eq!(record_payload["client_requested_stream"], false);
        assert_eq!(record_payload["client_is_stream"], false);
    }

    #[test]
    fn user_usage_stream_defaults_to_non_stream_for_openai_responses_request_body_without_flag() {
        let item = StoredRequestUsageAudit {
            is_stream: true,
            api_format: Some("openai:responses".to_string()),
            request_body: Some(json!({
                "model": "gpt-5.4",
                "input": [{"role": "user", "content": "hi"}],
                "store": false
            })),
            ..sample_usage("completed")
        };

        assert!(!users_me_usage_client_is_stream(&item));

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);
        assert_eq!(record_payload["is_stream"], true);
        assert_eq!(record_payload["upstream_is_stream"], true);
        assert_eq!(record_payload["client_requested_stream"], false);
        assert_eq!(record_payload["client_is_stream"], false);

        let active_payload = build_users_me_usage_active_payload(&item, false);
        assert_eq!(active_payload["is_stream"], true);
        assert_eq!(active_payload["upstream_is_stream"], true);
        assert_eq!(active_payload["client_requested_stream"], false);
        assert_eq!(active_payload["client_is_stream"], false);
    }

    #[test]
    fn user_usage_upstream_stream_prefers_request_metadata_flag() {
        let item = StoredRequestUsageAudit {
            is_stream: false,
            request_metadata: Some(json!({
                "client_requested_stream": false,
                "upstream_is_stream": true
            })),
            ..sample_usage("completed")
        };

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);
        assert_eq!(record_payload["is_stream"], false);
        assert_eq!(record_payload["upstream_is_stream"], true);
        assert_eq!(record_payload["client_requested_stream"], false);
        assert_eq!(record_payload["client_is_stream"], false);

        let active_payload = build_users_me_usage_active_payload(&item, false);
        assert_eq!(active_payload["is_stream"], false);
        assert_eq!(active_payload["upstream_is_stream"], true);
        assert_eq!(active_payload["client_requested_stream"], false);
        assert_eq!(active_payload["client_is_stream"], false);
    }

    #[test]
    fn user_usage_stream_modes_fall_back_to_captured_response_bodies_when_request_metadata_is_missing(
    ) {
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
            ..sample_usage("completed")
        };

        assert!(!users_me_usage_client_is_stream(&item));
        assert!(users_me_usage_upstream_is_stream(&item));

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);
        assert_eq!(record_payload["is_stream"], true);
        assert_eq!(record_payload["upstream_is_stream"], true);
        assert_eq!(record_payload["client_requested_stream"], false);
        assert_eq!(record_payload["client_is_stream"], false);

        let active_payload = build_users_me_usage_active_payload(&item, false);
        assert_eq!(active_payload["is_stream"], true);
        assert_eq!(active_payload["upstream_is_stream"], true);
        assert_eq!(active_payload["client_requested_stream"], false);
        assert_eq!(active_payload["client_is_stream"], false);
    }

    #[test]
    fn user_usage_stream_modes_fall_back_to_captured_response_headers_when_bodies_are_detached() {
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
            ..sample_usage("completed")
        };

        assert!(!users_me_usage_client_is_stream(&item));
        assert!(users_me_usage_upstream_is_stream(&item));

        let record_payload =
            build_users_me_usage_record_payload(&item, false, &BTreeMap::new(), false);
        assert_eq!(record_payload["is_stream"], true);
        assert_eq!(record_payload["upstream_is_stream"], true);
        assert_eq!(record_payload["client_requested_stream"], false);
        assert_eq!(record_payload["client_is_stream"], false);

        let active_payload = build_users_me_usage_active_payload(&item, false);
        assert_eq!(active_payload["is_stream"], true);
        assert_eq!(active_payload["upstream_is_stream"], true);
        assert_eq!(active_payload["client_requested_stream"], false);
        assert_eq!(active_payload["client_is_stream"], false);
    }
}

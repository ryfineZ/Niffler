use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::GatewayError;
use aether_admin::observability::usage::{
    admin_usage_bad_request_response, admin_usage_data_unavailable_response,
    ADMIN_USAGE_DATA_UNAVAILABLE_DETAIL,
};
use aether_data_contracts::repository::{
    provider_catalog::StoredProviderCatalogEndpoint,
    usage::{StoredRequestUsageAudit, UsageBodyCaptureState, UsageBodyField},
};
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(super) fn admin_usage_id_from_detail_path(request_path: &str) -> Option<String> {
    aether_admin::observability::usage::admin_usage_id_from_detail_path(request_path)
}

pub(super) fn admin_usage_id_from_action_path(request_path: &str, action: &str) -> Option<String> {
    aether_admin::observability::usage::admin_usage_id_from_action_path(request_path, action)
}

#[derive(Debug, Default, serde::Deserialize)]
struct AdminUsageReplayRequest {
    #[serde(default, alias = "target_provider_id")]
    provider_id: Option<String>,
    #[serde(default, alias = "target_endpoint_id")]
    endpoint_id: Option<String>,
    #[serde(default, alias = "target_api_key_id")]
    api_key_id: Option<String>,
    #[serde(default)]
    body_override: Option<serde_json::Value>,
}

pub(super) fn admin_usage_resolve_request_capture_body(
    item: &StoredRequestUsageAudit,
    body_override: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    aether_admin::observability::usage::admin_usage_resolve_request_capture_body(
        item,
        body_override,
    )
}

pub(super) async fn admin_usage_resolve_body_value(
    state: &AdminAppState<'_>,
    item: &StoredRequestUsageAudit,
    inline_body: Option<&Value>,
    field: UsageBodyField,
) -> Result<Option<Value>, GatewayError> {
    match item.body_state(field) {
        Some(UsageBodyCaptureState::Disabled)
        | Some(UsageBodyCaptureState::Unavailable)
        | Some(UsageBodyCaptureState::None) => return Ok(None),
        Some(UsageBodyCaptureState::Inline) | Some(UsageBodyCaptureState::Truncated) => {
            return Ok(inline_body.cloned());
        }
        Some(UsageBodyCaptureState::Reference) | None => {}
    }
    let resolved_ref_body = match item.body_ref(field) {
        Some(body_ref) => state.resolve_request_usage_body_ref(body_ref).await?,
        None => None,
    };
    Ok(admin_usage_body_value_from_sources(
        resolved_ref_body,
        inline_body,
    ))
}

fn admin_usage_body_value_from_sources(
    resolved_ref_body: Option<Value>,
    inline_body: Option<&Value>,
) -> Option<Value> {
    resolved_ref_body.or_else(|| inline_body.cloned())
}

pub(super) async fn admin_usage_resolve_request_capture_body_for_item(
    state: &AdminAppState<'_>,
    item: &StoredRequestUsageAudit,
    body_override: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, GatewayError> {
    if let Some(body_override) = body_override {
        return Ok(Some(body_override));
    }
    if let Some(body) = admin_usage_resolve_body_value(
        state,
        item,
        item.request_body.as_ref(),
        UsageBodyField::RequestBody,
    )
    .await?
    {
        return Ok(Some(body));
    }
    Ok(admin_usage_resolve_request_capture_body(item, None))
}

pub(super) fn build_admin_usage_curl_response(
    item: &StoredRequestUsageAudit,
    url: Option<String>,
    headers_json: Option<Value>,
    headers: &BTreeMap<String, String>,
    body: Option<&Value>,
) -> Response<Body> {
    aether_admin::observability::usage::build_admin_usage_curl_response(
        item,
        url,
        headers_json,
        headers,
        body,
    )
}

pub(super) fn build_admin_usage_detail_payload(
    item: &StoredRequestUsageAudit,
    users_by_id: &BTreeMap<String, aether_data::repository::users::StoredUserSummary>,
    api_key_names: &BTreeMap<String, String>,
    auth_user_reader_available: bool,
    auth_api_key_reader_available: bool,
    provider_key_name: Option<&str>,
    provider_key_account_label: Option<&str>,
    include_bodies: bool,
    request_body: Option<Value>,
    default_headers: &BTreeMap<String, String>,
) -> Value {
    aether_admin::observability::usage::build_admin_usage_detail_payload(
        item,
        users_by_id,
        api_key_names,
        auth_user_reader_available,
        auth_api_key_reader_available,
        provider_key_name,
        provider_key_account_label,
        include_bodies,
        request_body,
        default_headers,
    )
}

mod execution;
mod result;

fn replay_bad_request(message: &str) -> Response<Body> {
    admin_usage_bad_request_response(message)
}

fn replay_supported_format(format: &str) -> bool {
    matches!(
        format,
        "openai:chat"
            | "openai:responses"
            | "openai:responses:compact"
            | "claude:messages"
            | "gemini:generate_content"
            | "openai:embedding"
            | "gemini:embedding"
            | "jina:embedding"
            | "doubao:embedding"
            | "openai:rerank"
            | "jina:rerank"
    )
}

pub(super) async fn build_admin_usage_replay_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_usage_data_reader() || !state.has_provider_catalog_data_reader() {
        return Ok(admin_usage_data_unavailable_response(
            ADMIN_USAGE_DATA_UNAVAILABLE_DETAIL,
        ));
    }
    let Some(usage_id) = admin_usage_id_from_action_path(&request_context.request_path, "/replay")
    else {
        return Ok(replay_bad_request("usage_id 无效"));
    };
    let payload = match request_body.filter(|body| !body.is_empty()) {
        Some(body) => match serde_json::from_slice::<AdminUsageReplayRequest>(body) {
            Ok(payload) => payload,
            Err(_) => return Ok(replay_bad_request("回放参数不是有效的 JSON 对象")),
        },
        None => AdminUsageReplayRequest::default(),
    };
    let Some(item) = state.find_request_usage_by_id(&usage_id).await? else {
        return Ok((
            http::StatusCode::NOT_FOUND,
            Json(json!({ "detail": "原请求记录不存在" })),
        )
            .into_response());
    };
    let has_override = payload.body_override.is_some();
    let body = if has_override {
        payload.body_override
    } else {
        admin_usage_resolve_body_value(
            state,
            &item,
            item.request_body.as_ref(),
            UsageBodyField::RequestBody,
        )
        .await?
    };
    let Some(mut body) = body.filter(Value::is_object) else {
        return Ok(replay_bad_request(
            "完整请求体未保存、已过期或无法读取，不能回放",
        ));
    };
    if body.as_object().is_none_or(|body| body.is_empty())
        || body.get("truncated").and_then(Value::as_bool) == Some(true)
        || (!has_override
            && item.body_state(UsageBodyField::RequestBody)
                == Some(UsageBodyCaptureState::Truncated))
    {
        return Ok(replay_bad_request("请求体不完整，不能回放被截断的请求"));
    }
    let client_format = crate::ai_serving::normalize_api_format_alias(
        item.api_format.as_deref().unwrap_or_default(),
    );
    if !replay_supported_format(&client_format) {
        return Ok(replay_bad_request("该请求格式暂不支持回放"));
    }
    let source_model = body
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&item.model)
        .to_string();
    // Only restore the stream flag from capture metadata; never synthesize missing input.
    body.as_object_mut()
        .unwrap()
        .entry("stream")
        .or_insert_with(|| {
            json!(aether_admin::observability::usage::admin_usage_client_is_stream(&item))
        });

    let provider_id = payload
        .provider_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or(item.provider_id.as_deref())
        .unwrap_or_default();
    let Some(provider) = state
        .read_provider_catalog_providers_by_ids(&[provider_id.to_string()])
        .await?
        .into_iter()
        .find(|p| p.id == provider_id && p.is_active)
    else {
        return Ok(replay_bad_request("目标提供商不存在或已停用"));
    };
    let same_provider = item.provider_id.as_deref() == Some(provider.id.as_str());
    let endpoints = state
        .list_provider_catalog_endpoints_by_provider_ids(std::slice::from_ref(&provider.id))
        .await?;
    let endpoint_id = payload
        .endpoint_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            same_provider
                .then_some(item.provider_endpoint_id.as_deref())
                .flatten()
        });
    let endpoint = if let Some(endpoint_id) = endpoint_id {
        endpoints.iter().find(|e| e.id == endpoint_id)
    } else {
        endpoints
            .iter()
            .filter(|e| e.is_active)
            .find(|e| crate::ai_serving::normalize_api_format_alias(&e.api_format) == client_format)
            .or_else(|| {
                endpoints.iter().find(|e| {
                    e.is_active
                        && replay_supported_format(&crate::ai_serving::normalize_api_format_alias(
                            &e.api_format,
                        ))
                })
            })
    };
    let Some(endpoint) = endpoint.filter(|e| e.is_active && e.provider_id == provider.id) else {
        return Ok(replay_bad_request(
            "目标端点不存在、已停用或不属于所选提供商",
        ));
    };
    let target_format = crate::ai_serving::normalize_api_format_alias(&endpoint.api_format);
    if !replay_supported_format(&target_format) {
        return Ok(replay_bad_request("目标端点的请求格式暂不支持回放"));
    }
    let keys = state
        .list_provider_catalog_keys_by_provider_ids(std::slice::from_ref(&provider.id))
        .await?;
    let key_id = payload
        .api_key_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            (same_provider
                && payload
                    .provider_id
                    .as_deref()
                    .is_none_or(|id| id.trim().is_empty()))
            .then_some(item.provider_api_key_id.as_deref())
            .flatten()
        });
    let supports_key =
        |key: &&aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey| {
            key.provider_id == provider.id
                && key.is_active
                && key
                    .expires_at_unix_secs
                    .is_none_or(|expiry| expiry > crate::clock::current_unix_ms() / 1000)
                && crate::handlers::shared::provider_catalog_key_supports_format(
                    key,
                    &provider.provider_type,
                    &target_format,
                )
        };
    let key = if let Some(key_id) = key_id {
        keys.iter()
            .find(|key| key.id == key_id)
            .filter(supports_key)
    } else {
        keys.iter()
            .filter(supports_key)
            .min_by(|a, b| a.id.cmp(&b.id))
    };
    let Some(key) = key else {
        return Ok(replay_bad_request(
            "回放账号不存在、已停用或过期、不属于所选提供商或不支持目标格式",
        ));
    };
    let same_endpoint =
        same_provider && item.provider_endpoint_id.as_deref() == Some(endpoint.id.as_str());
    let (resolved_model, mapping_source) = if same_endpoint
        && source_model == item.model
        && item.target_model.as_ref().is_some_and(|m| !m.is_empty())
    {
        (item.target_model.clone().unwrap(), "original_target_model")
    } else {
        let mapped = crate::handlers::admin::provider::query::models::provider_query_resolve_global_effective_model(
            state, &provider.id, &source_model, endpoint,
        ).await?;
        let source = if mapped == source_model {
            "none"
        } else {
            "model_mapping"
        };
        (mapped, source)
    };
    let Some(transport) = state
        .read_provider_transport_snapshot(&provider.id, &endpoint.id, &key.id)
        .await?
    else {
        return Ok(replay_bad_request("无法读取回放账号的认证和代理配置"));
    };
    if let Some(reason) =
        crate::provider_transport::policy::local_standard_transport_unsupported_reason_with_network(
            &transport,
            &target_format,
        )
    {
        tracing::debug!(reason, provider_id = %provider.id, "admin replay target unsupported");
        let message = match reason {
            "transport_provider_type_unsupported" => "该提供商使用专用协议，暂不支持请求回放",
            "transport_oauth_resolution_unsupported" => {
                "该账号的授权类型暂不支持回放，请选择其他账号"
            }
            "transport_proxy_unsupported" | "transport_profile_unsupported" => {
                "该目标的代理或传输配置暂不支持回放"
            }
            "transport_header_rules_unsupported" | "transport_body_rules_unsupported" => {
                "该端点的请求改写规则暂不支持回放"
            }
            _ => "该端点暂不支持回放，请选择其他提供商或账号",
        };
        return Ok(replay_bad_request(message));
    }
    let mut headers = http::HeaderMap::new();
    // Rebuild auth from the selected account. Captured secrets or transport controls
    // must not survive replay or cross-provider remapping.
    for (name, value) in item
        .request_headers
        .as_ref()
        .and_then(admin_usage_headers_from_value)
        .unwrap_or_default()
    {
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "authorization"
                | "proxy-authorization"
                | "cookie"
                | "host"
                | "content-length"
                | "content-encoding"
                | "accept-encoding"
                | "connection"
                | "transfer-encoding"
                | "x-api-key"
                | "x-goog-api-key"
        ) || lower.starts_with("x-aether-")
            || lower == "chatgpt-account-id"
        {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            http::HeaderName::try_from(name),
            http::HeaderValue::try_from(value),
        ) {
            headers.insert(name, value);
        }
    }
    let replay_id = uuid::Uuid::new_v4().to_string();
    let plan = match execution::build_replay_plan(
        state,
        execution::ReplayPlanInput {
            transport: &transport,
            body,
            client_api_format: &client_format,
            model: &resolved_model,
            headers,
            replay_id: &replay_id,
        },
    )
    .await
    {
        Ok(plan) => plan,
        Err(err) => {
            tracing::warn!(error = ?err, %replay_id, "admin replay preparation failed");
            return Ok(replay_bad_request(
                "回放准备失败，请检查目标账号授权、请求格式和代理配置",
            ));
        }
    };
    let started_at = crate::clock::current_unix_ms();
    let mut record = replay_record(&replay_id, &item, &plan, started_at);
    if let Some(metadata) = record.extra_data.as_mut() {
        metadata["admin_replay"]["admin_user_id"] = json!(request_context
            .decision()
            .and_then(|d| d.admin_principal.as_ref())
            .map(|p| &p.user_id));
    }
    match state.app().upsert_request_candidate(record.clone()).await {
        Ok(Some(_)) => {}
        result => {
            tracing::warn!(?result, %replay_id, "admin replay start could not be recorded; request not sent");
            return Ok(admin_usage_data_unavailable_response(
                "回放记录存储不可用，尚未发送请求",
            ));
        }
    }
    let started = std::time::Instant::now();
    let execution = tokio::time::timeout(
        std::time::Duration::from_secs(execution::REPLAY_TIMEOUT_SECS),
        state.execute_execution_runtime_sync_plan(Some(&replay_id), &plan),
    )
    .await;
    let (status_code, response_headers, response_body, error_message) = match execution {
        Ok(Ok(result)) => {
            let decoded = result::decode_replay_result(&target_format, &result);
            (
                Some(result.status_code),
                result.headers,
                decoded.body,
                decoded.error,
            )
        }
        Ok(Err(err)) => {
            tracing::warn!(error = ?err, %replay_id, "admin replay transport failed");
            let detail = format!("{err:?}").to_ascii_lowercase();
            let message = if detail.contains("timeout") || detail.contains("timed out") {
                "上游请求超时，请查看回放记录后再决定是否重试"
            } else {
                "无法完成上游请求，请检查账号、代理及网络连接"
            };
            (
                None,
                BTreeMap::new(),
                Value::Null,
                Some(message.to_string()),
            )
        }
        Err(_) => (
            None,
            BTreeMap::new(),
            Value::Null,
            Some("回放等待上游响应超时（120 秒）".to_string()),
        ),
    };
    let elapsed = started.elapsed().as_millis() as u64;
    record.status = if error_message.is_some() {
        aether_data_contracts::repository::candidates::RequestCandidateStatus::Failed
    } else {
        aether_data_contracts::repository::candidates::RequestCandidateStatus::Success
    };
    record.status_code = status_code;
    record.latency_ms = Some(elapsed);
    record.error_message = error_message.clone();
    record.finished_at_unix_ms = Some(crate::clock::current_unix_ms());
    let record_warning = match state.app().upsert_request_candidate(record).await {
        Ok(Some(_)) => None,
        outcome => {
            tracing::error!(?outcome, %replay_id, "admin replay result could not be recorded");
            Some("请求已执行，但回放结果保存失败；请保留回放编号，避免重复发送")
        }
    };
    Ok(Json(json!({
        "replay_id": replay_id, "original_request_id": item.request_id,
        "status": if error_message.is_some() { "failed" } else { "success" },
        "url": replay_display_url(&plan.url), "provider": provider.name, "status_code": status_code,
        "response_headers": response_headers, "response_body": response_body,
        "response_time_ms": elapsed, "error_message": error_message, "record_warning": record_warning,
        "mapping": {
            "source_model": source_model, "original_target_model": item.target_model,
            "resolved_model": resolved_model, "target_provider_id": provider.id,
            "target_provider": provider.name, "target_endpoint_id": endpoint.id,
            "target_api_format": target_format, "target_api_key_id": key.id,
            "replay_mode": if same_endpoint { "same_endpoint_reuse" } else if same_provider { "same_provider_remap" } else { "cross_provider_remap" },
            "mapping_applied": source_model != resolved_model, "mapping_source": mapping_source,
        },
    })).into_response())
}

fn replay_display_url(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        return String::new();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn replay_record(
    replay_id: &str,
    item: &StoredRequestUsageAudit,
    plan: &aether_contracts::ExecutionPlan,
    started_at: u64,
) -> aether_data_contracts::repository::candidates::UpsertRequestCandidateRecord {
    use aether_data_contracts::repository::candidates::{
        RequestCandidateStatus, UpsertRequestCandidateRecord,
    };
    UpsertRequestCandidateRecord {
        id: replay_id.to_string(),
        request_id: replay_id.to_string(),
        user_id: None,
        api_key_id: None,
        username: None,
        api_key_name: None,
        candidate_index: 0,
        retry_index: 0,
        provider_id: Some(plan.provider_id.clone()),
        endpoint_id: Some(plan.endpoint_id.clone()),
        key_id: Some(plan.key_id.clone()),
        status: RequestCandidateStatus::Pending,
        skip_reason: None,
        is_cached: Some(false),
        status_code: None,
        error_type: None,
        error_message: None,
        latency_ms: None,
        concurrent_requests: None,
        extra_data: Some(
            json!({"admin_replay": {"usage_id": item.id, "original_request_id": item.request_id, "billed_to_user": false}, "api_format": plan.provider_api_format, "model": plan.model_name}),
        ),
        required_capabilities: None,
        created_at_unix_ms: Some(started_at),
        started_at_unix_ms: Some(started_at),
        finished_at_unix_ms: None,
    }
}

pub(super) fn admin_usage_headers_from_value(
    value: &serde_json::Value,
) -> Option<BTreeMap<String, String>> {
    aether_admin::observability::usage::admin_usage_headers_from_value(value)
}

pub(super) fn admin_usage_curl_headers() -> BTreeMap<String, String> {
    aether_admin::observability::usage::admin_usage_curl_headers()
}

pub(super) fn admin_usage_curl_url(
    state: &AdminAppState<'_>,
    endpoint: &StoredProviderCatalogEndpoint,
    item: &StoredRequestUsageAudit,
) -> String {
    let api_format = item
        .endpoint_api_format
        .as_deref()
        .or(item.api_format.as_deref())
        .unwrap_or(endpoint.api_format.as_str());

    if let Some(custom_path) = endpoint
        .custom_path
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return state
            .build_passthrough_path_url(&endpoint.base_url, custom_path, None, &[])
            .unwrap_or_else(|| endpoint.base_url.clone());
    }

    match api_format {
        value if value.starts_with("claude:") => {
            state.build_claude_messages_url(&endpoint.base_url, None)
        }
        value if value.starts_with("gemini:") => state
            .build_gemini_content_url(
                &endpoint.base_url,
                item.target_model.as_deref().unwrap_or(item.model.as_str()),
                item.is_stream,
                None,
            )
            .unwrap_or_else(|| endpoint.base_url.clone()),
        value if value.starts_with("openai:") => {
            state.build_openai_chat_url(&endpoint.base_url, None)
        }
        _ => endpoint.base_url.clone(),
    }
}

pub(super) fn admin_usage_build_curl_command(
    url: Option<&str>,
    headers: &BTreeMap<String, String>,
    body: Option<&serde_json::Value>,
) -> String {
    aether_admin::observability::usage::admin_usage_build_curl_command(url, headers, body)
}

#[cfg(test)]
mod tests {
    use super::admin_usage_body_value_from_sources;
    use serde_json::json;

    #[test]
    fn resolved_reference_body_wins_over_inline_fallback() {
        let inline_body = json!({
            "truncated": true,
            "reason": "usage_capture_limits_exceeded"
        });
        let ref_body = json!({
            "messages": [{"role": "user", "content": "real request body"}]
        });

        assert_eq!(
            admin_usage_body_value_from_sources(Some(ref_body.clone()), Some(&inline_body)),
            Some(ref_body)
        );
    }

    #[test]
    fn inline_body_is_used_when_reference_body_is_unavailable() {
        let inline_body = json!({
            "messages": [{"role": "user", "content": "fallback inline body"}]
        });

        assert_eq!(
            admin_usage_body_value_from_sources(None, Some(&inline_body)),
            Some(inline_body)
        );
    }
}

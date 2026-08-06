use crate::handlers::admin::niffler_legacy_freeze::maybe_freeze_migrated_legacy_provider_key_write;
use crate::handlers::admin::provider::oauth::quota::codex::{
    consume_codex_quota_reset_credit, refresh_codex_provider_quota_locally, CodexQuotaResetAttempt,
};
use crate::handlers::admin::provider::oauth::quota::shared::{
    provider_quota_refresh_endpoint_for_provider, provider_quota_refresh_missing_endpoint_message,
};
use crate::handlers::admin::provider::shared::paths::admin_reset_codex_quota_key_id;
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::handlers::admin::shared::attach_admin_audit_response;
use crate::GatewayError;
use axum::{
    body::{Body, Bytes},
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use tracing::warn;

const CODEX_QUOTA_RESET_LOCK_TTL_SECONDS: u64 = 300;

#[derive(Debug, Deserialize)]
struct AdminCodexQuotaResetRequest {
    idempotency_key: String,
}

pub(super) async fn maybe_handle(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&Bytes>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let Some(decision) = request_context.decision() else {
        return Ok(None);
    };
    if decision.route_family.as_deref() != Some("endpoints_manage")
        || decision.route_kind.as_deref() != Some("reset_codex_quota")
        || request_context.method() != http::Method::POST
        || !request_context
            .path()
            .starts_with("/api/admin/endpoints/keys/")
        || !request_context.path().ends_with("/reset-codex-quota")
    {
        return Ok(None);
    }

    let Some(key_id) = admin_reset_codex_quota_key_id(request_context.path()) else {
        return Ok(Some(not_found_response("Key 不存在")));
    };
    if let Some(response) = maybe_freeze_migrated_legacy_provider_key_write(state, &key_id).await? {
        return Ok(Some(response));
    }
    let payload = match request_body.filter(|body| !body.is_empty()) {
        Some(body) => match serde_json::from_slice::<AdminCodexQuotaResetRequest>(body) {
            Ok(payload) => payload,
            Err(_) => return Ok(Some(bad_request_response("请求体必须包含 idempotency_key"))),
        },
        None => return Ok(Some(bad_request_response("请求体必须包含 idempotency_key"))),
    };
    let idempotency_key = payload.idempotency_key.trim();
    if uuid::Uuid::parse_str(idempotency_key).is_err() {
        return Ok(Some(bad_request_response(
            "idempotency_key 必须是有效的 UUID",
        )));
    }

    let Some(key) = state
        .read_provider_catalog_keys_by_ids(std::slice::from_ref(&key_id))
        .await?
        .into_iter()
        .next()
    else {
        return Ok(Some(not_found_response(format!("Key {key_id} 不存在"))));
    };
    let Some(provider) = state
        .read_provider_catalog_providers_by_ids(std::slice::from_ref(&key.provider_id))
        .await?
        .into_iter()
        .next()
    else {
        return Ok(Some(not_found_response(format!(
            "Provider {} 不存在",
            key.provider_id
        ))));
    };
    if !provider.provider_type.trim().eq_ignore_ascii_case("codex") {
        return Ok(Some(bad_request_response(
            "仅 Codex Provider 支持主动重置额度",
        )));
    }
    let endpoints = state
        .list_provider_catalog_endpoints_by_provider_ids(std::slice::from_ref(&provider.id))
        .await?;
    let Some(endpoint) = provider_quota_refresh_endpoint_for_provider("codex", &endpoints, true)
    else {
        return Ok(Some(bad_request_response(
            provider_quota_refresh_missing_endpoint_message("codex"),
        )));
    };

    let lock_key = format!("codex_quota_reset:{key_id}");
    let lock_owner = format!("aether-gateway-codex-quota-reset-{}", std::process::id());
    let lease = match state
        .runtime_state()
        .lock_try_acquire(
            &lock_key,
            &lock_owner,
            Duration::from_secs(CODEX_QUOTA_RESET_LOCK_TTL_SECONDS),
        )
        .await
    {
        Ok(Some(lease)) => lease,
        Ok(None) => {
            return Ok(Some(
                (
                    http::StatusCode::CONFLICT,
                    Json(json!({ "detail": "该账号正在重置额度" })),
                )
                    .into_response(),
            ));
        }
        Err(error) => return Err(GatewayError::Internal(error.to_string())),
    };

    let response = execute_locked_reset(state, &provider, &endpoint, &key, idempotency_key).await;
    if let Err(error) = state.runtime_state().lock_release(&lease).await {
        warn!(
            key_id,
            error = %error,
            "failed to release codex quota reset lock"
        );
    }
    response.map(Some)
}

async fn execute_locked_reset(
    state: &AdminAppState<'_>,
    provider: &aether_data_contracts::repository::provider_catalog::StoredProviderCatalogProvider,
    endpoint: &aether_data_contracts::repository::provider_catalog::StoredProviderCatalogEndpoint,
    key: &aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey,
    idempotency_key: &str,
) -> Result<Response<Body>, GatewayError> {
    let reset_result =
        consume_codex_quota_reset_credit(state, provider, endpoint, key, idempotency_key).await?;
    let reset_result = match reset_result {
        CodexQuotaResetAttempt::Result(reset_result) => reset_result,
        CodexQuotaResetAttempt::Failure {
            status_code,
            message,
        } => {
            return Ok((
                http::StatusCode::BAD_GATEWAY,
                Json(json!({
                    "detail": format!("Codex 额度重置失败：{message}"),
                    "upstream_status": status_code,
                })),
            )
                .into_response());
        }
    };

    let reset_applied = reset_result.outcome.reset_applied();
    if reset_applied {
        state
            .clear_admin_provider_pool_cooldown(&provider.id, &key.id)
            .await;
    }

    let refresh_result =
        refresh_codex_provider_quota_locally(state, provider, endpoint, vec![key.clone()], None)
            .await;
    let (refresh_succeeded, quota_snapshot, refresh_message) =
        summarize_post_reset_refresh(refresh_result);
    let message = match reset_result.outcome.as_str() {
        "reset" => "额度已重置",
        "already_redeemed" => "额度已经重置",
        "nothing_to_reset" => "当前没有可重置的额度窗口",
        "no_credit" => "当前没有可用的主动重置次数",
        _ => "额度重置结果未知",
    };
    let response = Json(json!({
        "message": message,
        "outcome": reset_result.outcome.as_str(),
        "reset_applied": reset_applied,
        "windows_reset": reset_result.windows_reset,
        "refresh_succeeded": refresh_succeeded,
        "quota_snapshot": quota_snapshot,
        "refresh_message": refresh_message,
    }))
    .into_response();

    Ok(attach_admin_audit_response(
        response,
        "admin_provider_key_codex_quota_reset",
        "reset_provider_key_codex_quota",
        "provider_key",
        &key.id,
    ))
}

fn summarize_post_reset_refresh(
    refresh_result: Result<Option<serde_json::Value>, GatewayError>,
) -> (bool, Option<serde_json::Value>, Option<String>) {
    match refresh_result {
        Ok(Some(payload)) => {
            let result = payload
                .get("results")
                .and_then(serde_json::Value::as_array)
                .and_then(|results| results.first());
            let succeeded = result
                .and_then(|result| result.get("status"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|status| status == "success");
            let quota_snapshot = result
                .and_then(|result| result.get("quota_snapshot"))
                .cloned();
            let message = if succeeded {
                None
            } else {
                result
                    .and_then(|result| result.get("message"))
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| Some("额度已处理，但最新额度刷新失败".to_string()))
            };
            (succeeded, quota_snapshot, message)
        }
        Ok(None) => (
            false,
            None,
            Some("额度已处理，但最新额度刷新失败".to_string()),
        ),
        Err(error) => {
            let message = match error {
                GatewayError::UpstreamUnavailable { message, .. }
                | GatewayError::ControlUnavailable { message, .. }
                | GatewayError::Client { message, .. }
                | GatewayError::Internal(message) => message,
            };
            (false, None, Some(message))
        }
    }
}

fn bad_request_response(detail: impl Into<String>) -> Response<Body> {
    (
        http::StatusCode::BAD_REQUEST,
        Json(json!({ "detail": detail.into() })),
    )
        .into_response()
}

fn not_found_response(detail: impl Into<String>) -> Response<Body> {
    (
        http::StatusCode::NOT_FOUND,
        Json(json!({ "detail": detail.into() })),
    )
        .into_response()
}

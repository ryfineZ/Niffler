use crate::handlers::admin::niffler_legacy_projection::project_user_api_key_with_product_plan;
use crate::handlers::admin::request::AdminAppState;
use crate::handlers::admin::shared::{
    attach_admin_audit_response, decrypt_catalog_secret_with_fallbacks,
};
use crate::handlers::public::resolve_user_portal;
use crate::handlers::shared::{
    api_key_placeholder_display, generate_gateway_api_key_plaintext, masked_gateway_api_key_display,
};
use crate::GatewayError;
use axum::{body::Body, response::Response};
use serde_json::json;
use std::collections::BTreeSet;

use super::super::build_admin_users_bad_request_response;

pub(crate) fn format_optional_unix_secs_iso8601(value: Option<u64>) -> Option<String> {
    let secs = value?;
    let secs = i64::try_from(secs).ok()?;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0).map(|value| value.to_rfc3339())
}

pub(crate) fn masked_user_api_key_display(
    state: &AdminAppState<'_>,
    ciphertext: Option<&str>,
) -> String {
    let Some(ciphertext) = ciphertext.map(str::trim).filter(|value| !value.is_empty()) else {
        return api_key_placeholder_display();
    };
    let Some(full_key) = decrypt_catalog_secret_with_fallbacks(state.encryption_key(), ciphertext)
    else {
        return api_key_placeholder_display();
    };
    masked_gateway_api_key_display(Some(full_key.as_str()))
}

pub(super) fn build_admin_user_api_key_detail_payload(
    state: &AdminAppState<'_>,
    record: &aether_data::repository::auth::StoredAuthApiKeyExportRecord,
    is_locked: bool,
) -> serde_json::Value {
    json!({
        "id": record.api_key_id,
        "name": record.name,
        "group_id": record.group_id,
        "group_name": record.group_name,
        "key_display": masked_user_api_key_display(state, record.key_encrypted.as_deref()),
        "is_active": record.is_active,
        "is_locked": is_locked,
        "total_requests": record.total_requests,
        "total_cost_usd": record.total_cost_usd,
        "rate_limit": record.rate_limit,
        "concurrent_limit": record.concurrent_limit,
        "feature_settings": record.feature_settings,
        "expires_at": format_optional_unix_secs_iso8601(record.expires_at_unix_secs),
        "last_used_at": format_optional_unix_secs_iso8601(record.last_used_at_unix_secs),
        "created_at": format_optional_unix_secs_iso8601(record.created_at_unix_secs),
    })
}

pub(super) async fn project_admin_user_api_key_product_plan_binding(
    state: &AdminAppState<'_>,
    api_key_id: &str,
    payload: &mut serde_json::Value,
) -> Result<(), GatewayError> {
    let Some(binding) = state
        .find_niffler_api_key_product_plan_binding_by_api_key_id(api_key_id)
        .await?
    else {
        return Ok(());
    };
    let Some(plan) = state
        .find_niffler_product_plan_by_id(&binding.product_plan_id)
        .await?
    else {
        return Ok(());
    };
    project_user_api_key_with_product_plan(payload, &binding.id, &plan);
    Ok(())
}

pub(crate) fn normalize_admin_optional_api_key_name(
    value: Option<String>,
) -> Result<Option<String>, String> {
    match value {
        None => Ok(None),
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err("API密钥名称不能为空".to_string());
            }
            Ok(Some(trimmed.chars().take(100).collect()))
        }
    }
}

pub(super) fn normalize_admin_api_key_providers(
    value: Option<Vec<String>>,
) -> Result<Option<Vec<String>>, String> {
    let Some(values) = value else {
        return Ok(None);
    };
    let mut normalized = Vec::new();
    let mut seen = BTreeSet::new();
    for provider_id in values {
        let provider_id = provider_id.trim();
        if provider_id.is_empty() {
            return Err("提供商ID不能为空".to_string());
        }
        if seen.insert(provider_id.to_string()) {
            normalized.push(provider_id.to_string());
        }
    }
    Ok(Some(normalized))
}

pub(super) async fn validate_admin_user_api_key_group_id(
    state: &AdminAppState<'_>,
    user_id: &str,
    group_id: &str,
) -> Result<Result<(), Response<Body>>, GatewayError> {
    let Some(group) = state.find_user_group_by_id(group_id).await? else {
        return Ok(Err(build_admin_users_bad_request_response("分组不存在")));
    };

    let portal = resolve_user_portal(state.app(), user_id).await?;
    if portal.is_official_usd() && portal.group_id.as_deref() != Some(group_id) {
        return Ok(Err(build_admin_users_bad_request_response(
            "专属门户用户的 API Key 必须使用专属计费分组",
        )));
    }

    if group.visibility.trim().eq_ignore_ascii_case("public") {
        return Ok(Ok(()));
    }

    let user_group_ids = state
        .list_user_groups_for_user(user_id)
        .await?
        .into_iter()
        .map(|group| group.id)
        .collect::<BTreeSet<_>>();
    if user_group_ids.contains(group_id) {
        Ok(Ok(()))
    } else {
        Ok(Err(build_admin_users_bad_request_response(
            "这个分组不可用于该用户",
        )))
    }
}

pub(crate) fn generate_admin_user_api_key_plaintext() -> String {
    generate_gateway_api_key_plaintext()
}

pub(crate) fn hash_admin_user_api_key(value: &str) -> String {
    use sha2::Digest;

    let mut hasher = sha2::Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn default_admin_user_api_key_name() -> String {
    format!("API Key {}", chrono::Utc::now().format("%Y%m%d%H%M%S"))
}

pub(super) fn attach_audit_response(
    response: Response<Body>,
    action: &'static str,
    event_type: &'static str,
    object_type: &'static str,
    object_id: &str,
) -> Response<Body> {
    attach_admin_audit_response(response, action, event_type, object_type, object_id)
}

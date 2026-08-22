use super::{
    build_admin_users_bad_request_response, build_admin_users_read_only_response,
    format_optional_datetime_iso8601, normalize_admin_user_api_formats,
    normalize_admin_user_string_list,
};
use crate::constants::DEFAULT_USER_GROUP_CONFIG_KEY;
use crate::handlers::admin::niffler_legacy_freeze::maybe_freeze_migrated_legacy_user_group_write;
use crate::handlers::admin::niffler_legacy_projection::product_plan_user_group_projection;
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::handlers::admin::shared::attach_admin_audit_response;
use crate::managed_instructions::{
    managed_instructions_profiles, validate_managed_instructions_config,
    MANAGED_INSTRUCTIONS_SUPPORTED_FORMATS,
};
use crate::GatewayError;
use aether_data_contracts::repository::niffler_core::{
    NifflerProductPlanListQuery, NifflerProductPlanModelListQuery,
};
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, serde::Deserialize)]
struct AdminUserGroupPayload {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "default_group_visibility")]
    visibility: String,
    #[serde(default = "default_sales_multiplier")]
    sales_multiplier: f64,
    #[serde(default)]
    model_sales_multipliers: Option<serde_json::Value>,
    #[serde(default)]
    managed_instructions: Option<serde_json::Value>,
    #[serde(default)]
    allowed_providers: Option<Vec<String>>,
    #[serde(default = "default_list_mode")]
    allowed_providers_mode: String,
    #[serde(default)]
    allowed_api_formats: Option<Vec<String>>,
    #[serde(default = "default_list_mode")]
    allowed_api_formats_mode: String,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
    #[serde(default = "default_list_mode")]
    allowed_models_mode: String,
    #[serde(default)]
    rate_limit: Option<i32>,
    #[serde(default = "default_rate_limit_mode")]
    rate_limit_mode: String,
    #[serde(default)]
    concurrent_limit: Option<i32>,
    #[serde(default = "default_rate_limit_mode")]
    concurrent_limit_mode: String,
}

#[derive(Debug, serde::Deserialize)]
struct AdminUserGroupMembersPayload {
    user_ids: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AdminDeleteUserGroupWithReplacementPayload {
    #[serde(default)]
    replacement_group_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct AdminDefaultUserGroupPayload {
    #[serde(default)]
    group_id: Option<String>,
}

const DELETE_USER_GROUP_API_KEY_REFERENCE_LIMIT: usize = 5;

pub(in super::super) fn build_admin_managed_instruction_profiles_response(
) -> Result<Response<Body>, GatewayError> {
    let profiles = managed_instructions_profiles().map_err(GatewayError::Internal)?;
    Ok(Json(json!({
        "profiles": profiles.iter().map(|profile| json!({
            "profile_id": profile.profile_id,
            "display_name": profile.display_name,
            "description": profile.description,
            "core_version": profile.core_version,
            "domain_version": profile.domain_version,
            "profile_sha256": profile.profile_sha256,
        })).collect::<Vec<_>>(),
        "merge_modes": ["prepend", "if_missing"],
        "supported_provider_api_formats": MANAGED_INSTRUCTIONS_SUPPORTED_FORMATS,
        "composition_order": [
            "managed_instructions",
            "client_instructions",
            "image_generation_bridge"
        ]
    }))
    .into_response())
}

pub(in super::super) async fn build_admin_list_user_groups_response(
    state: &AdminAppState<'_>,
) -> Result<Response<Body>, GatewayError> {
    let default_group_id = read_default_user_group_id(state).await?;
    let groups = state.list_user_groups().await?;
    let product_plans = state
        .list_niffler_product_plans(&NifflerProductPlanListQuery {
            include_inactive: true,
            public_only: false,
            search: None,
            offset: 0,
            limit: 1000,
        })
        .await
        .ok()
        .map(|page| {
            page.items
                .into_iter()
                .map(|plan| (plan.id.clone(), plan))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut items = Vec::with_capacity(groups.len());
    for group in groups {
        if let Some(plan) = product_plans.get(&group.id) {
            let models = state
                .list_niffler_product_plan_models(&NifflerProductPlanModelListQuery {
                    product_plan_id: plan.id.clone(),
                    enabled_only: false,
                    search: None,
                    offset: 0,
                    limit: 1000,
                })
                .await
                .ok()
                .map(|page| page.items)
                .unwrap_or_default();
            items.push(product_plan_user_group_projection(
                plan,
                &models,
                default_group_id.as_deref(),
            ));
        } else {
            items.push(user_group_payload(group, default_group_id.as_deref()));
        }
    }
    Ok(Json(json!({
        "items": items,
        "default_group_id": default_group_id,
    }))
    .into_response())
}

pub(in super::super) async fn build_admin_create_user_group_response(
    state: &AdminAppState<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_auth_user_write_capability() {
        return Ok(build_admin_users_read_only_response(
            "当前为只读模式，无法创建用户分组",
        ));
    }
    let record = match parse_group_record(request_body) {
        Ok(value) => value,
        Err(detail) => return Ok(bad_request_owned(detail)),
    };
    if let Some(response) = validate_group_allowed_providers(state, &record).await? {
        return Ok(response);
    }
    let group = match state.create_user_group(record).await {
        Ok(Some(group)) => group,
        Ok(None) => {
            return Ok(build_admin_users_read_only_response(
                "当前为只读模式，无法创建用户分组",
            ))
        }
        Err(err) if is_duplicate_group_name_error(&err) => {
            return Ok(bad_request_owned("用户分组名称已存在".to_string()))
        }
        Err(err) => return Err(err),
    };
    let default_group_id = read_default_user_group_id(state).await?;
    Ok(attach_admin_audit_response(
        Json(user_group_payload(group, default_group_id.as_deref())).into_response(),
        "admin_user_group_created",
        "create_user_group",
        "user_group",
        "user_groups",
    ))
}

pub(in super::super) async fn build_admin_update_user_group_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_auth_user_write_capability() {
        return Ok(build_admin_users_read_only_response(
            "当前为只读模式，无法更新用户分组",
        ));
    }
    let Some(group_id) = user_group_id_from_path(request_context.path()) else {
        return Ok(build_admin_users_bad_request_response("缺少 group_id"));
    };
    if let Some(response) = maybe_freeze_migrated_legacy_user_group_write(state, &group_id).await? {
        return Ok(response);
    }
    let record = match parse_group_record(request_body) {
        Ok(value) => value,
        Err(detail) => return Ok(bad_request_owned(detail)),
    };
    if let Some(response) = validate_group_allowed_providers(state, &record).await? {
        return Ok(response);
    }
    let group = match state.update_user_group(&group_id, record).await {
        Ok(Some(group)) => group,
        Ok(None) => return Ok(not_found("用户分组不存在")),
        Err(err) if is_duplicate_group_name_error(&err) => {
            return Ok(bad_request_owned("用户分组名称已存在".to_string()))
        }
        Err(err) => return Err(err),
    };
    let default_group_id = read_default_user_group_id(state).await?;
    Ok(attach_admin_audit_response(
        Json(user_group_payload(group, default_group_id.as_deref())).into_response(),
        "admin_user_group_updated",
        "update_user_group",
        "user_group",
        &group_id,
    ))
}

pub(in super::super) async fn build_admin_delete_user_group_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_auth_user_write_capability() {
        return Ok(build_admin_users_read_only_response(
            "当前为只读模式，无法删除用户分组",
        ));
    }
    let Some(group_id) = user_group_id_from_path(request_context.path()) else {
        return Ok(build_admin_users_bad_request_response("缺少 group_id"));
    };
    if let Some(response) = maybe_freeze_migrated_legacy_user_group_write(state, &group_id).await? {
        return Ok(response);
    }
    if read_default_user_group_id(state).await?.as_deref() == Some(group_id.as_str()) {
        return Ok(bad_request_owned("默认用户组不能删除".to_string()));
    }
    let api_key_references = state
        .summarize_auth_api_key_group_references(
            &group_id,
            DELETE_USER_GROUP_API_KEY_REFERENCE_LIMIT,
        )
        .await?;
    if api_key_references.total > 0 {
        return Ok(user_group_api_key_conflict_response(api_key_references));
    }
    let deleted = match state.delete_user_group(&group_id).await {
        Ok(deleted) => deleted,
        Err(err) if is_api_key_group_reference_delete_error(&err) => {
            return Ok(user_group_api_key_conflict_response(
                aether_data::repository::auth::AuthApiKeyGroupReferenceSummary::default(),
            ))
        }
        Err(err) => return Err(err),
    };
    if !deleted {
        return Ok(not_found("用户分组不存在"));
    }
    Ok(attach_admin_audit_response(
        Json(json!({ "deleted": true })).into_response(),
        "admin_user_group_deleted",
        "delete_user_group",
        "user_group",
        &group_id,
    ))
}

pub(in super::super) async fn build_admin_delete_user_group_with_replacement_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_auth_user_write_capability() {
        return Ok(build_admin_users_read_only_response(
            "当前为只读模式，无法删除用户分组",
        ));
    }
    let Some(group_id) = user_group_delete_replacement_group_id_from_path(request_context.path())
    else {
        return Ok(build_admin_users_bad_request_response("缺少 group_id"));
    };
    if let Some(response) = maybe_freeze_migrated_legacy_user_group_write(state, &group_id).await? {
        return Ok(response);
    }
    if read_default_user_group_id(state).await?.as_deref() == Some(group_id.as_str()) {
        return Ok(bad_request_owned("默认用户组不能删除".to_string()));
    }
    let payload = match parse_delete_replacement_payload(request_body) {
        Ok(value) => value,
        Err(detail) => return Ok(bad_request_owned(detail)),
    };
    let replacement_group_id = payload.replacement_group_id.trim().to_string();
    if replacement_group_id.is_empty() {
        return Ok(bad_request_owned("请选择替换目标分组".to_string()));
    }
    if replacement_group_id == group_id {
        return Ok(bad_request_owned("替换目标不能是当前分组".to_string()));
    }
    if let Some(response) =
        maybe_freeze_migrated_legacy_user_group_write(state, &replacement_group_id).await?
    {
        return Ok(response);
    }
    if state
        .find_user_group_by_id(&replacement_group_id)
        .await?
        .is_none()
    {
        return Ok(bad_request_owned("替换目标分组不存在".to_string()));
    }

    let outcome = state
        .delete_user_group_replacing_api_keys(&group_id, &replacement_group_id)
        .await?;
    if !outcome.deleted {
        return Ok(not_found("用户分组不存在"));
    }
    Ok(attach_admin_audit_response(
        Json(json!({
            "deleted": true,
            "migrated_api_key_count": outcome.migrated_api_key_count,
        }))
        .into_response(),
        "admin_user_group_deleted_with_replacement",
        "delete_user_group_with_replacement",
        "user_group",
        &group_id,
    ))
}

pub(in super::super) async fn build_admin_list_user_group_members_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
) -> Result<Response<Body>, GatewayError> {
    let Some(group_id) = user_group_member_group_id_from_path(request_context.path()) else {
        return Ok(build_admin_users_bad_request_response("缺少 group_id"));
    };
    if state.find_user_group_by_id(&group_id).await?.is_none() {
        return Ok(not_found("用户分组不存在"));
    }
    let items = state
        .list_user_group_members(&group_id)
        .await?
        .into_iter()
        .map(|member| {
            json!({
                "group_id": member.group_id,
                "user_id": member.user_id,
                "username": member.username,
                "email": member.email,
                "role": member.role,
                "is_active": member.is_active,
                "is_deleted": member.is_deleted,
                "created_at": format_optional_datetime_iso8601(member.created_at),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "items": items })).into_response())
}

pub(in super::super) async fn build_admin_replace_user_group_members_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_auth_user_write_capability() {
        return Ok(build_admin_users_read_only_response(
            "当前为只读模式，无法更新分组成员",
        ));
    }
    let Some(group_id) = user_group_member_group_id_from_path(request_context.path()) else {
        return Ok(build_admin_users_bad_request_response("缺少 group_id"));
    };
    if state.find_user_group_by_id(&group_id).await?.is_none() {
        return Ok(not_found("用户分组不存在"));
    }
    let payload = match parse_members_payload(request_body) {
        Ok(value) => value,
        Err(detail) => return Ok(bad_request_owned(detail)),
    };
    let user_ids = normalize_ids(payload.user_ids);
    if read_default_user_group_id(state).await?.as_deref() == Some(group_id.as_str()) {
        if let Some(response) =
            validate_default_group_member_replacement(state, &group_id, &user_ids).await?
        {
            return Ok(response);
        }
    }
    let known_users = state.resolve_auth_user_summaries_by_ids(&user_ids).await?;
    if known_users.len() != user_ids.len() {
        return Ok(bad_request_owned("成员包含不存在的用户".to_string()));
    }
    let items = state
        .replace_user_group_members(&group_id, &user_ids)
        .await?;
    Ok(attach_admin_audit_response(
        Json(json!({
            "items": items.into_iter().map(|member| json!({
                "group_id": member.group_id,
                "user_id": member.user_id,
                "username": member.username,
                "email": member.email,
                "role": member.role,
                "is_active": member.is_active,
                "is_deleted": member.is_deleted,
                "created_at": format_optional_datetime_iso8601(member.created_at),
            })).collect::<Vec<_>>()
        }))
        .into_response(),
        "admin_user_group_members_updated",
        "update_user_group_members",
        "user_group",
        &group_id,
    ))
}

async fn validate_default_group_member_replacement(
    state: &AdminAppState<'_>,
    group_id: &str,
    next_user_ids: &[String],
) -> Result<Option<Response<Body>>, GatewayError> {
    let next_user_ids = next_user_ids.iter().cloned().collect::<BTreeSet<String>>();
    let removed_user_ids = state
        .list_user_group_members(group_id)
        .await?
        .into_iter()
        .filter(|member| !next_user_ids.contains(&member.user_id))
        .map(|member| member.user_id)
        .collect::<Vec<_>>();
    if removed_user_ids.is_empty() {
        return Ok(None);
    }

    let summaries = state
        .resolve_auth_user_summaries_by_ids(&removed_user_ids)
        .await?;
    let users_with_other_groups = state
        .list_user_group_memberships_by_user_ids(&removed_user_ids)
        .await?
        .into_iter()
        .filter(|membership| membership.group_id != group_id)
        .map(|membership| membership.user_id)
        .collect::<BTreeSet<_>>();

    for user_id in removed_user_ids {
        let Some(summary) = summaries.get(&user_id) else {
            continue;
        };
        if crate::roles::can_access_admin_console(&summary.role) {
            continue;
        }
        if !users_with_other_groups.contains(&user_id) {
            return Ok(Some(bad_request_owned(format!(
                "用户 {} 移出默认组后将不属于任何用户组",
                summary.username
            ))));
        }
    }

    Ok(None)
}

pub(in super::super) async fn build_admin_set_default_user_group_response(
    state: &AdminAppState<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_auth_user_write_capability() {
        return Ok(build_admin_users_read_only_response(
            "当前为只读模式，无法设置默认用户组",
        ));
    }
    let payload = match request_body {
        Some(body) if !body.is_empty() => {
            serde_json::from_slice::<AdminDefaultUserGroupPayload>(body)
                .map_err(|_| "请求数据验证失败".to_string())
        }
        _ => Err("请求数据验证失败".to_string()),
    };
    let payload = match payload {
        Ok(value) => value,
        Err(detail) => return Ok(bad_request_owned(detail)),
    };
    let group_id = payload
        .group_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(group_id) = group_id.as_deref() {
        if state.find_user_group_by_id(group_id).await?.is_none() {
            return Ok(bad_request_owned("默认用户组不存在".to_string()));
        }
        state
            .upsert_system_config_json_value(
                DEFAULT_USER_GROUP_CONFIG_KEY,
                &json!(group_id),
                Some("Default group for self-registered users"),
            )
            .await?;
    } else {
        state
            .delete_system_config_value(DEFAULT_USER_GROUP_CONFIG_KEY)
            .await?;
    }
    let effective_group_id = read_default_user_group_id(state).await?;
    if let Some(group_id) = effective_group_id.as_deref() {
        state.add_all_users_to_group(group_id).await?;
    }
    Ok(attach_admin_audit_response(
        Json(json!({ "default_group_id": effective_group_id })).into_response(),
        "admin_default_user_group_set",
        "set_default_user_group",
        "user_group",
        group_id.as_deref().unwrap_or("default_user_group"),
    ))
}

pub(crate) async fn read_default_user_group_id(
    state: &AdminAppState<'_>,
) -> Result<Option<String>, GatewayError> {
    state.effective_default_user_group_id().await
}

fn parse_group_record(
    request_body: Option<&axum::body::Bytes>,
) -> Result<aether_data::repository::users::UpsertUserGroupRecord, String> {
    let Some(body) = request_body.filter(|body| !body.is_empty()) else {
        return Err("请求数据验证失败".to_string());
    };
    let payload = serde_json::from_slice::<AdminUserGroupPayload>(body)
        .map_err(|_| "请求数据验证失败".to_string())?;
    let name = aether_data::repository::users::normalize_user_group_name(&payload.name);
    if name.is_empty() {
        return Err("分组名称不能为空".to_string());
    }
    if payload.rate_limit.is_some_and(|value| value < 0) {
        return Err("rate_limit 必须大于等于 0".to_string());
    }
    if payload.concurrent_limit.is_some_and(|value| value < 0) {
        return Err("concurrent_limit 必须大于等于 0".to_string());
    }
    let visibility = normalize_group_visibility(&payload.visibility)?;
    let sales_multiplier = normalize_sales_multiplier(payload.sales_multiplier)?;
    let model_sales_multipliers =
        normalize_model_sales_multipliers(payload.model_sales_multipliers)?;
    validate_managed_instructions_config(payload.managed_instructions.as_ref())?;
    let allowed_providers =
        normalize_admin_user_string_list(payload.allowed_providers, "allowed_providers")?;
    let allowed_api_formats = normalize_admin_user_api_formats(payload.allowed_api_formats)?;
    let allowed_models =
        normalize_admin_user_string_list(payload.allowed_models, "allowed_models")?;
    Ok(aether_data::repository::users::UpsertUserGroupRecord {
        name,
        description: payload
            .description
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        visibility,
        sales_multiplier,
        model_sales_multipliers,
        managed_instructions: payload.managed_instructions,
        priority: 0,
        allowed_providers,
        allowed_providers_mode: normalize_list_mode(&payload.allowed_providers_mode)?,
        allowed_api_formats,
        allowed_api_formats_mode: normalize_list_mode(&payload.allowed_api_formats_mode)?,
        allowed_models,
        allowed_models_mode: normalize_list_mode(&payload.allowed_models_mode)?,
        rate_limit: payload.rate_limit,
        rate_limit_mode: normalize_rate_mode(&payload.rate_limit_mode)?,
        concurrent_limit: payload.concurrent_limit,
        concurrent_limit_mode: normalize_rate_mode(&payload.concurrent_limit_mode)?,
    })
}

async fn validate_group_allowed_providers(
    state: &AdminAppState<'_>,
    record: &aether_data::repository::users::UpsertUserGroupRecord,
) -> Result<Option<Response<Body>>, GatewayError> {
    if record.allowed_providers_mode != "specific" {
        return Ok(None);
    }
    let Some(provider_ids) = record
        .allowed_providers
        .as_ref()
        .filter(|provider_ids| !provider_ids.is_empty())
    else {
        return Ok(None);
    };
    if !state.has_provider_catalog_data_reader() {
        return Ok(Some(bad_request_owned(
            "Provider 数据不可读，无法保存可用 Provider 限制".to_string(),
        )));
    }

    let providers = state
        .read_provider_catalog_providers_by_ids(provider_ids)
        .await?;
    let providers_by_id = providers
        .into_iter()
        .map(|provider| (provider.id.clone(), provider))
        .collect::<BTreeMap<_, _>>();
    let missing_provider_ids = provider_ids
        .iter()
        .filter(|provider_id| !providers_by_id.contains_key(*provider_id))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_provider_ids.is_empty() {
        return Ok(Some(bad_request_owned(format!(
            "可用 Provider 不存在：{}",
            missing_provider_ids.join("、")
        ))));
    }

    let inactive_provider_names = provider_ids
        .iter()
        .filter_map(|provider_id| providers_by_id.get(provider_id))
        .filter(|provider| !provider.is_active)
        .map(|provider| provider.name.clone())
        .collect::<Vec<_>>();
    if !inactive_provider_names.is_empty() {
        return Ok(Some(bad_request_owned(format!(
            "不能选择已停用 Provider：{}",
            inactive_provider_names.join("、")
        ))));
    }

    Ok(None)
}

fn parse_members_payload(
    request_body: Option<&axum::body::Bytes>,
) -> Result<AdminUserGroupMembersPayload, String> {
    let Some(body) = request_body.filter(|body| !body.is_empty()) else {
        return Err("请求数据验证失败".to_string());
    };
    serde_json::from_slice::<AdminUserGroupMembersPayload>(body)
        .map_err(|_| "请求数据验证失败".to_string())
}

fn parse_delete_replacement_payload(
    request_body: Option<&axum::body::Bytes>,
) -> Result<AdminDeleteUserGroupWithReplacementPayload, String> {
    let Some(body) = request_body.filter(|body| !body.is_empty()) else {
        return Err("请求数据验证失败".to_string());
    };
    serde_json::from_slice::<AdminDeleteUserGroupWithReplacementPayload>(body)
        .map_err(|_| "请求数据验证失败".to_string())
}

fn user_group_payload(
    group: aether_data::repository::users::StoredUserGroup,
    default_group_id: Option<&str>,
) -> serde_json::Value {
    json!({
        "id": group.id,
        "name": group.name,
        "normalized_name": group.normalized_name,
        "description": group.description,
        "visibility": group.visibility,
        "sales_multiplier": group.sales_multiplier,
        "model_sales_multipliers": group.model_sales_multipliers,
        "managed_instructions": group.managed_instructions,
        "allowed_providers": group.allowed_providers,
        "allowed_providers_mode": group.allowed_providers_mode,
        "allowed_api_formats": group.allowed_api_formats,
        "allowed_api_formats_mode": group.allowed_api_formats_mode,
        "allowed_models": group.allowed_models,
        "allowed_models_mode": group.allowed_models_mode,
        "rate_limit": group.rate_limit,
        "rate_limit_mode": group.rate_limit_mode,
        "concurrent_limit": group.concurrent_limit,
        "concurrent_limit_mode": group.concurrent_limit_mode,
        "is_default": default_group_id == Some(group.id.as_str()),
        "created_at": format_optional_datetime_iso8601(group.created_at),
        "updated_at": format_optional_datetime_iso8601(group.updated_at),
    })
}

fn normalize_list_mode(value: &str) -> Result<String, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "inherit" | "unrestricted" | "specific" | "deny_all" => {
            Ok(value.trim().to_ascii_lowercase())
        }
        _ => Err("权限列表模式不合法".to_string()),
    }
}

fn normalize_rate_mode(value: &str) -> Result<String, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "inherit" | "system" | "custom" => Ok(value.trim().to_ascii_lowercase()),
        _ => Err("限速模式不合法".to_string()),
    }
}

pub(crate) fn normalize_group_visibility(value: &str) -> Result<String, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "public" | "internal" => Ok(value.trim().to_ascii_lowercase()),
        _ => Err("分组可见性不合法".to_string()),
    }
}

pub(crate) fn normalize_sales_multiplier(value: f64) -> Result<f64, String> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err("折扣必须大于等于 0".to_string())
    }
}

pub(crate) fn normalize_model_sales_multipliers(
    value: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(object) = value.as_object() else {
        return Err("模型折扣必须是对象".to_string());
    };
    for (model_id, multiplier) in object {
        if model_id.trim().is_empty() {
            return Err("模型ID不能为空".to_string());
        }
        let Some(multiplier) = multiplier.as_f64() else {
            return Err("模型折扣必须是数字".to_string());
        };
        normalize_sales_multiplier(multiplier)?;
    }
    Ok(Some(value))
}

fn default_list_mode() -> String {
    "inherit".to_string()
}

fn default_rate_limit_mode() -> String {
    "inherit".to_string()
}

fn default_group_visibility() -> String {
    "public".to_string()
}

fn default_sales_multiplier() -> f64 {
    1.0
}

fn normalize_ids(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn user_group_id_from_path(request_path: &str) -> Option<String> {
    let value = request_path
        .strip_prefix("/api/admin/user-groups/")?
        .trim()
        .trim_matches('/')
        .to_string();
    if value.is_empty() || value.contains('/') || value == "default" {
        None
    } else {
        Some(value)
    }
}

fn user_group_delete_replacement_group_id_from_path(request_path: &str) -> Option<String> {
    let value = request_path
        .strip_prefix("/api/admin/user-groups/")?
        .trim()
        .trim_matches('/');
    let group_id = value
        .strip_suffix("/delete-with-replacement")?
        .trim_matches('/');
    if group_id.is_empty() || group_id.contains('/') || group_id == "default" {
        None
    } else {
        Some(group_id.to_string())
    }
}

fn user_group_member_group_id_from_path(request_path: &str) -> Option<String> {
    let value = request_path
        .strip_prefix("/api/admin/user-groups/")?
        .trim()
        .trim_matches('/');
    let group_id = value.strip_suffix("/members")?.trim_matches('/');
    if group_id.is_empty() || group_id.contains('/') {
        None
    } else {
        Some(group_id.to_string())
    }
}

fn bad_request_owned(detail: String) -> Response<Body> {
    (
        http::StatusCode::BAD_REQUEST,
        Json(json!({ "detail": detail })),
    )
        .into_response()
}

fn not_found(detail: &'static str) -> Response<Body> {
    (
        http::StatusCode::NOT_FOUND,
        Json(json!({ "detail": detail })),
    )
        .into_response()
}

fn user_group_api_key_conflict_response(
    summary: aether_data::repository::auth::AuthApiKeyGroupReferenceSummary,
) -> Response<Body> {
    let examples = summary
        .items
        .iter()
        .map(format_api_key_group_reference)
        .collect::<Vec<_>>();
    let example_count = u64::try_from(examples.len()).unwrap_or(u64::MAX);
    let extra_suffix = if summary.total > example_count {
        " 等"
    } else {
        ""
    };
    let detail = if examples.is_empty() && summary.total == 0 {
        "这个分组还有 API Key 正在使用，不能删除。先在「用户管理」里编辑这些 API Key，改到其他分组或删除后再试。".to_string()
    } else if examples.is_empty() {
        format!(
            "这个分组还有 {} 把 API Key 正在使用，不能删除。先在「用户管理」里编辑这些 API Key，改到其他分组或删除后再试。",
            summary.total
        )
    } else {
        format!(
            "这个分组还有 {} 把 API Key 正在使用，不能删除。先在「用户管理」里编辑这些 API Key，改到其他分组或删除后再试。当前占用：{}{}。",
            summary.total,
            examples.join("、"),
            extra_suffix
        )
    };
    let api_keys = summary
        .items
        .into_iter()
        .map(|item| {
            json!({
                "id": item.api_key_id,
                "name": item.api_key_name,
                "user_id": item.user_id,
                "username": item.username,
                "email": item.email,
            })
        })
        .collect::<Vec<_>>();
    (
        http::StatusCode::CONFLICT,
        Json(json!({
            "detail": detail,
            "api_key_count": summary.total,
            "api_keys": api_keys,
        })),
    )
        .into_response()
}

fn format_api_key_group_reference(
    item: &aether_data::repository::auth::AuthApiKeyGroupReference,
) -> String {
    let key_name = item
        .api_key_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(item.api_key_id.as_str());
    let email = item
        .email
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let user_label = match email {
        Some(email) if email != item.username => format!("{} / {}", item.username, email),
        _ => item.username.clone(),
    };
    format!("{key_name}（用户：{user_label}）")
}

fn is_duplicate_group_name_error(err: &GatewayError) -> bool {
    match err {
        GatewayError::Internal(message) => message.contains("duplicate user group name"),
        _ => false,
    }
}

fn is_api_key_group_reference_delete_error(err: &GatewayError) -> bool {
    match err {
        GatewayError::Internal(message) => message.contains("api_keys_group_id_fkey"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_group_payload_accepts_managed_instructions() {
        let body = axum::body::Bytes::from(
            serde_json::to_vec(&json!({
                "name": "CTF 与渗透",
                "managed_instructions": {
                    "enabled": true,
                    "profile_id": "security_research_v1",
                    "merge_mode": "prepend"
                }
            }))
            .expect("payload should serialize"),
        );

        let record = parse_group_record(Some(&body)).expect("profile should be valid");
        assert_eq!(
            record.managed_instructions,
            Some(json!({
                "enabled": true,
                "profile_id": "security_research_v1",
                "merge_mode": "prepend"
            }))
        );
    }

    #[test]
    fn user_group_payload_rejects_unknown_managed_profile() {
        let body = axum::body::Bytes::from(
            serde_json::to_vec(&json!({
                "name": "无效配置",
                "managed_instructions": {
                    "enabled": false,
                    "profile_id": "missing_v1",
                    "merge_mode": "prepend"
                }
            }))
            .expect("payload should serialize"),
        );

        let error = parse_group_record(Some(&body)).expect_err("unknown profile should fail");
        assert!(error.contains("missing_v1"));
    }
}

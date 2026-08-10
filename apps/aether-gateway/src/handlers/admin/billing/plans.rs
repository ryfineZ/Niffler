use super::{
    build_admin_billing_bad_request_response, build_admin_billing_conflict_response,
    build_admin_billing_data_unavailable_response, build_admin_billing_not_found_response,
};
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::{GatewayError, LocalMutationOutcome};
use aether_data_contracts::repository::billing::{BillingPlanRecord, BillingPlanWriteInput};
use axum::{
    body::{Body, Bytes},
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
struct BillingPlanRequest {
    title: String,
    #[serde(default)]
    description: Option<String>,
    price_amount: f64,
    #[serde(default = "default_price_currency")]
    price_currency: String,
    duration_unit: String,
    duration_value: i64,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    sort_order: i64,
    #[serde(default = "default_max_active_per_user")]
    max_active_per_user: i64,
    #[serde(default = "default_purchase_limit_scope")]
    purchase_limit_scope: String,
    #[serde(default)]
    allowed_provider_ids: Vec<String>,
    entitlements: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct BillingPlanStatusRequest {
    enabled: bool,
}

fn default_price_currency() -> String {
    "CNY".to_string()
}

fn default_enabled() -> bool {
    true
}

fn default_max_active_per_user() -> i64 {
    1
}

fn default_purchase_limit_scope() -> String {
    "active_period".to_string()
}

fn normalize_text(value: impl Into<String>, field: &str, max_len: usize) -> Result<String, String> {
    let value = value.into();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if trimmed.chars().count() > max_len {
        return Err(format!("{field} exceeds maximum length {max_len}"));
    }
    Ok(trimmed.to_string())
}

fn normalize_optional_text(
    value: Option<String>,
    max_len: usize,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > max_len {
        return Err(format!("field exceeds maximum length {max_len}"));
    }
    Ok(Some(trimmed.to_string()))
}

fn validate_entitlements(value: &serde_json::Value) -> Result<(), String> {
    let items = value
        .as_array()
        .ok_or_else(|| "entitlements must be an array".to_string())?;
    if items.is_empty() {
        return Err("entitlements must not be empty".to_string());
    }
    for item in items {
        let kind = item
            .get("type")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "entitlement.type is required".to_string())?;
        match kind {
            "wallet_credit" => {
                let amount = item
                    .get("amount_usd")
                    .and_then(|value| value.as_f64())
                    .ok_or_else(|| "wallet_credit.amount_usd is required".to_string())?;
                if !amount.is_finite() || amount <= 0.0 {
                    return Err("wallet_credit.amount_usd must be positive".to_string());
                }
                if let Some(bucket) = item.get("balance_bucket") {
                    let bucket = bucket.as_str().ok_or_else(|| {
                        "wallet_credit.balance_bucket must be a string".to_string()
                    })?;
                    if !matches!(bucket, "recharge" | "gift") {
                        return Err(
                            "wallet_credit.balance_bucket must be recharge/gift".to_string()
                        );
                    }
                }
            }
            "daily_quota" | "usage_quota" => {
                let has_positive_quota = [
                    "daily_quota_usd",
                    "five_hour_quota_usd",
                    "weekly_quota_usd",
                    "monthly_quota_usd",
                ]
                .iter()
                .any(|field| {
                    item.get(*field)
                        .and_then(|value| value.as_f64())
                        .is_some_and(|value| value.is_finite() && value > 0.0)
                });
                if !has_positive_quota {
                    return Err(format!(
                        "{kind} must include at least one positive quota amount"
                    ));
                }
                for field in [
                    "daily_quota_usd",
                    "five_hour_quota_usd",
                    "weekly_quota_usd",
                    "monthly_quota_usd",
                ] {
                    if let Some(value) = item.get(field) {
                        let amount = value
                            .as_f64()
                            .ok_or_else(|| format!("{kind}.{field} must be a number"))?;
                        if !amount.is_finite() || amount < 0.0 {
                            return Err(format!("{kind}.{field} must be non-negative"));
                        }
                    }
                }
                if let Some(value) = item.get("quota_multiplier") {
                    let multiplier = value
                        .as_f64()
                        .ok_or_else(|| format!("{kind}.quota_multiplier must be a number"))?;
                    if !multiplier.is_finite() || multiplier <= 0.0 {
                        return Err(format!("{kind}.quota_multiplier must be positive"));
                    }
                }
                if let Some(reset_timezone) = item.get("reset_timezone") {
                    let reset_timezone = reset_timezone
                        .as_str()
                        .ok_or_else(|| format!("{kind}.reset_timezone must be a string"))?
                        .trim();
                    if reset_timezone.is_empty() {
                        return Err(format!("{kind}.reset_timezone must not be empty"));
                    }
                    reset_timezone
                        .parse::<chrono_tz::Tz>()
                        .map_err(|_| format!("{kind}.reset_timezone must be a valid timezone"))?;
                }
                if let Some(carry_over) = item.get("carry_over") {
                    let carry_over = carry_over
                        .as_bool()
                        .ok_or_else(|| format!("{kind}.carry_over must be a boolean"))?;
                    if carry_over {
                        return Err(format!("{kind}.carry_over is not supported"));
                    }
                }
                if item
                    .get("allow_wallet_overage")
                    .is_some_and(|value| !value.is_boolean())
                {
                    return Err(format!("{kind}.allow_wallet_overage must be a boolean"));
                }
                if let Some(rpm_limit) = item.get("rpm_limit") {
                    let rpm_limit = rpm_limit
                        .as_i64()
                        .ok_or_else(|| format!("{kind}.rpm_limit must be an integer"))?;
                    if rpm_limit < 0 {
                        return Err(format!("{kind}.rpm_limit must be non-negative"));
                    }
                }
            }
            "membership_group" => {
                let groups = item
                    .get("grant_user_groups")
                    .and_then(|value| value.as_array())
                    .ok_or_else(|| "membership_group.grant_user_groups is required".to_string())?;
                if groups.is_empty() {
                    return Err("membership_group.grant_user_groups must not be empty".to_string());
                }
                for group in groups {
                    let group = group.as_str().ok_or_else(|| {
                        "membership_group.grant_user_groups must contain strings".to_string()
                    })?;
                    if group.trim().is_empty() {
                        return Err(
                            "membership_group.grant_user_groups must not contain empty values"
                                .to_string(),
                        );
                    }
                }
            }
            _ => return Err(format!("unsupported entitlement type: {kind}")),
        }
    }
    if !entitlements_include_package_rights(items) {
        return Err("套餐至少需要包含周期额度或会员分组；钱包充值请使用充值功能".to_string());
    }
    Ok(())
}

fn entitlements_include_package_rights(items: &[serde_json::Value]) -> bool {
    items.iter().any(|item| {
        matches!(
            item.get("type").and_then(|value| value.as_str()),
            Some("daily_quota" | "usage_quota" | "membership_group")
        )
    })
}

fn entitlements_include_usage_quota(items: &[serde_json::Value]) -> bool {
    items.iter().any(|item| {
        matches!(
            item.get("type").and_then(|value| value.as_str()),
            Some("daily_quota" | "usage_quota")
        )
    })
}

fn normalize_provider_ids(values: Vec<String>) -> Result<Vec<String>, String> {
    if values.len() > 100 {
        return Err("allowed_provider_ids exceeds maximum length 100".to_string());
    }
    let mut unique = BTreeSet::new();
    for value in values {
        let provider_id = normalize_text(value, "allowed_provider_ids", 64)?;
        unique.insert(provider_id);
    }
    Ok(unique.into_iter().collect())
}

fn remove_static_model_scope(entitlements: &mut serde_json::Value) {
    let Some(items) = entitlements.as_array_mut() else {
        return;
    };
    for item in items {
        if matches!(
            item.get("type").and_then(|value| value.as_str()),
            Some("daily_quota" | "usage_quota")
        ) {
            if let Some(item) = item.as_object_mut() {
                item.remove("allowed_global_model_ids");
            }
        }
    }
}

fn enforce_wallet_overage_settlement(entitlements: &mut serde_json::Value) {
    let Some(items) = entitlements.as_array_mut() else {
        return;
    };
    for item in items {
        if matches!(
            item.get("type").and_then(|value| value.as_str()),
            Some("daily_quota" | "usage_quota")
        ) {
            if let Some(item) = item.as_object_mut() {
                item.insert("allow_wallet_overage".to_string(), json!(true));
            }
        }
    }
}

async fn validate_plan_providers(
    state: &crate::AppState,
    provider_ids: &[String],
) -> Result<Option<String>, GatewayError> {
    if provider_ids.is_empty() {
        return Ok(None);
    }
    let providers = state
        .read_provider_catalog_providers_by_ids(provider_ids)
        .await?;
    let providers_by_id = providers
        .into_iter()
        .map(|provider| (provider.id.clone(), provider))
        .collect::<std::collections::BTreeMap<_, _>>();
    let missing = provider_ids
        .iter()
        .filter(|provider_id| !providers_by_id.contains_key(*provider_id))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Ok(Some(format!("供应商不存在：{}", missing.join(", "))));
    }
    let inactive = provider_ids
        .iter()
        .filter(|provider_id| {
            providers_by_id
                .get(*provider_id)
                .is_some_and(|provider| !provider.is_active)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !inactive.is_empty() {
        return Ok(Some(format!("供应商已停用：{}", inactive.join(", "))));
    }
    Ok(None)
}

enum PlanPriceRule {
    Create,
    UpdateExistingZero { existing_price_amount: f64 },
}

fn validate_plan_price_amount(price_amount: f64, rule: PlanPriceRule) -> Result<(), String> {
    if !price_amount.is_finite() || price_amount < 0.0 {
        return Err("price_amount must be positive".to_string());
    }
    if price_amount == 0.0 {
        match rule {
            PlanPriceRule::Create => return Err("price_amount must be positive".to_string()),
            PlanPriceRule::UpdateExistingZero {
                existing_price_amount: 0.0,
            } => {}
            PlanPriceRule::UpdateExistingZero { .. } => {
                return Err("price_amount must be positive".to_string())
            }
        }
    }
    Ok(())
}

fn normalize_plan_input_for_create(
    payload: BillingPlanRequest,
) -> Result<BillingPlanWriteInput, String> {
    normalize_plan_input(payload, PlanPriceRule::Create)
}

fn normalize_plan_input_for_update(
    payload: BillingPlanRequest,
    existing: &BillingPlanRecord,
) -> Result<BillingPlanWriteInput, String> {
    normalize_plan_input(
        payload,
        PlanPriceRule::UpdateExistingZero {
            existing_price_amount: existing.price_amount,
        },
    )
}

fn normalize_plan_input(
    mut payload: BillingPlanRequest,
    price_rule: PlanPriceRule,
) -> Result<BillingPlanWriteInput, String> {
    validate_plan_price_amount(payload.price_amount, price_rule)?;
    if payload.duration_value <= 0 {
        return Err("duration_value must be positive".to_string());
    }
    if payload.max_active_per_user <= 0 {
        return Err("max_active_per_user must be positive".to_string());
    }
    let duration_unit = normalize_text(payload.duration_unit, "duration_unit", 32)?;
    if !matches!(duration_unit.as_str(), "day" | "month" | "year" | "custom") {
        return Err("duration_unit must be day/month/year/custom".to_string());
    }
    let purchase_limit_scope =
        normalize_text(payload.purchase_limit_scope, "purchase_limit_scope", 32)?;
    if !matches!(
        purchase_limit_scope.as_str(),
        "active_period" | "lifetime" | "unlimited"
    ) {
        return Err("purchase_limit_scope must be active_period/lifetime/unlimited".to_string());
    }
    validate_entitlements(&payload.entitlements)?;
    let allowed_provider_ids = normalize_provider_ids(payload.allowed_provider_ids)?;
    if payload
        .entitlements
        .as_array()
        .is_some_and(|items| entitlements_include_usage_quota(items))
        && allowed_provider_ids.is_empty()
    {
        return Err("周期额度套餐至少需要选择一个供应商".to_string());
    }
    remove_static_model_scope(&mut payload.entitlements);
    enforce_wallet_overage_settlement(&mut payload.entitlements);
    Ok(BillingPlanWriteInput {
        title: normalize_text(payload.title, "title", 128)?,
        description: normalize_optional_text(payload.description, 2048)?,
        price_amount: payload.price_amount,
        price_currency: normalize_text(payload.price_currency, "price_currency", 16)?,
        duration_unit,
        duration_value: payload.duration_value,
        enabled: payload.enabled,
        sort_order: payload.sort_order,
        max_active_per_user: payload.max_active_per_user,
        purchase_limit_scope,
        allowed_provider_ids,
        entitlements_json: payload.entitlements,
    })
}

pub(crate) fn billing_plan_payload(record: &BillingPlanRecord) -> serde_json::Value {
    json!({
        "id": record.id,
        "title": record.title,
        "description": record.description,
        "price_amount": record.price_amount,
        "price_currency": record.price_currency,
        "duration_unit": record.duration_unit,
        "duration_value": record.duration_value,
        "enabled": record.enabled,
        "sort_order": record.sort_order,
        "max_active_per_user": record.max_active_per_user,
        "purchase_limit_scope": record.purchase_limit_scope,
        "allowed_provider_ids": record.allowed_provider_ids,
        "entitlements": record.entitlements_json,
        "created_at": record.created_at_unix_secs,
        "updated_at": record.updated_at_unix_secs,
    })
}

fn plan_id_from_path(path: &str, suffix: Option<&str>) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let rest = trimmed.strip_prefix("/api/admin/billing/plans/")?;
    let id = if let Some(suffix) = suffix {
        rest.strip_suffix(suffix)?.trim_end_matches('/')
    } else {
        rest
    };
    if id.is_empty() || id.contains('/') {
        None
    } else {
        Some(id.to_string())
    }
}

pub(super) async fn maybe_build_local_admin_billing_plans_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&Bytes>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let path = request_context.path().trim_end_matches('/');
    match (request_context.method(), path) {
        (&http::Method::GET, "/api/admin/billing/plans") => {
            let items = state
                .app()
                .list_billing_plans(true)
                .await?
                .unwrap_or_default()
                .iter()
                .map(billing_plan_payload)
                .collect::<Vec<_>>();
            Ok(Some(
                Json(json!({"items": items, "total": items.len()})).into_response(),
            ))
        }
        (&http::Method::POST, "/api/admin/billing/plans") => {
            let Some(body) = request_body else {
                return Ok(Some(build_admin_billing_bad_request_response("缺少请求体")));
            };
            let payload = match serde_json::from_slice::<BillingPlanRequest>(body) {
                Ok(value) => value,
                Err(_) => {
                    return Ok(Some(build_admin_billing_bad_request_response(
                        "输入验证失败",
                    )))
                }
            };
            let input = match normalize_plan_input_for_create(payload) {
                Ok(value) => value,
                Err(detail) => return Ok(Some(build_admin_billing_bad_request_response(detail))),
            };
            if let Some(detail) =
                validate_plan_providers(state.app(), &input.allowed_provider_ids).await?
            {
                return Ok(Some(build_admin_billing_bad_request_response(detail)));
            }
            match state.app().create_billing_plan(&input).await? {
                LocalMutationOutcome::Applied(record) => {
                    Ok(Some(Json(billing_plan_payload(&record)).into_response()))
                }
                _ => Ok(Some(build_admin_billing_data_unavailable_response())),
            }
        }
        _ if request_context.method() == http::Method::PUT
            && path.starts_with("/api/admin/billing/plans/") =>
        {
            let Some(plan_id) = plan_id_from_path(path, None) else {
                return Ok(None);
            };
            let Some(body) = request_body else {
                return Ok(Some(build_admin_billing_bad_request_response("缺少请求体")));
            };
            let payload = match serde_json::from_slice::<BillingPlanRequest>(body) {
                Ok(value) => value,
                Err(_) => {
                    return Ok(Some(build_admin_billing_bad_request_response(
                        "输入验证失败",
                    )))
                }
            };
            let Some(existing) = state.app().find_billing_plan(&plan_id).await? else {
                return Ok(Some(build_admin_billing_not_found_response(
                    "Billing plan not found",
                )));
            };
            let input = match normalize_plan_input_for_update(payload, &existing) {
                Ok(value) => value,
                Err(detail) => return Ok(Some(build_admin_billing_bad_request_response(detail))),
            };
            if let Some(detail) =
                validate_plan_providers(state.app(), &input.allowed_provider_ids).await?
            {
                return Ok(Some(build_admin_billing_bad_request_response(detail)));
            }
            match state.app().update_billing_plan(&plan_id, &input).await? {
                LocalMutationOutcome::Applied(record) => {
                    Ok(Some(Json(billing_plan_payload(&record)).into_response()))
                }
                LocalMutationOutcome::NotFound => Ok(Some(build_admin_billing_not_found_response(
                    "Billing plan not found",
                ))),
                _ => Ok(Some(build_admin_billing_data_unavailable_response())),
            }
        }
        _ if request_context.method() == http::Method::PATCH
            && path.ends_with("/status")
            && path.starts_with("/api/admin/billing/plans/") =>
        {
            let Some(plan_id) = plan_id_from_path(path, Some("/status")) else {
                return Ok(None);
            };
            let Some(body) = request_body else {
                return Ok(Some(build_admin_billing_bad_request_response("缺少请求体")));
            };
            let payload = match serde_json::from_slice::<BillingPlanStatusRequest>(body) {
                Ok(value) => value,
                Err(_) => {
                    return Ok(Some(build_admin_billing_bad_request_response(
                        "输入验证失败",
                    )))
                }
            };
            if payload.enabled {
                let Some(existing) = state.app().find_billing_plan(&plan_id).await? else {
                    return Ok(Some(build_admin_billing_not_found_response(
                        "Billing plan not found",
                    )));
                };
                if let Some(detail) =
                    validate_plan_providers(state.app(), &existing.allowed_provider_ids).await?
                {
                    return Ok(Some(build_admin_billing_bad_request_response(detail)));
                }
            }
            match state
                .app()
                .set_billing_plan_enabled(&plan_id, payload.enabled)
                .await?
            {
                LocalMutationOutcome::Applied(record) => {
                    Ok(Some(Json(billing_plan_payload(&record)).into_response()))
                }
                LocalMutationOutcome::NotFound => Ok(Some(build_admin_billing_not_found_response(
                    "Billing plan not found",
                ))),
                _ => Ok(Some(build_admin_billing_data_unavailable_response())),
            }
        }
        _ if request_context.method() == http::Method::DELETE
            && path.starts_with("/api/admin/billing/plans/") =>
        {
            let Some(plan_id) = plan_id_from_path(path, None) else {
                return Ok(None);
            };
            match state.app().delete_billing_plan(&plan_id).await? {
                LocalMutationOutcome::Applied(()) => {
                    Ok(Some(http::StatusCode::NO_CONTENT.into_response()))
                }
                LocalMutationOutcome::NotFound => Ok(Some(build_admin_billing_not_found_response(
                    "Billing plan not found",
                ))),
                LocalMutationOutcome::Invalid(detail) => {
                    Ok(Some(build_admin_billing_conflict_response(detail)))
                }
                LocalMutationOutcome::Unavailable => {
                    Ok(Some(build_admin_billing_data_unavailable_response()))
                }
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan_request(price_amount: f64) -> BillingPlanRequest {
        BillingPlanRequest {
            title: "同步套餐".to_string(),
            description: Some("从旧系统同步".to_string()),
            price_amount,
            price_currency: "CNY".to_string(),
            duration_unit: "day".to_string(),
            duration_value: 30,
            enabled: true,
            sort_order: 0,
            max_active_per_user: 1,
            purchase_limit_scope: "active_period".to_string(),
            allowed_provider_ids: vec!["provider-codex".to_string()],
            entitlements: json!([
                {
                    "type": "daily_quota",
                    "daily_quota_usd": 10.0,
                    "allowed_global_model_ids": ["global-codex"],
                    "reset_timezone": "Asia/Shanghai",
                    "carry_over": false
                }
            ]),
        }
    }

    #[test]
    fn create_plan_rejects_zero_price() {
        let error = normalize_plan_input_for_create(sample_plan_request(0.0))
            .expect_err("zero price rejected");

        assert_eq!(error, "price_amount must be positive");
    }

    #[test]
    fn update_existing_zero_price_plan_allows_zero_price() {
        let existing = BillingPlanRecord {
            id: "sub2api-plan-group-1".to_string(),
            title: "旧套餐".to_string(),
            description: None,
            price_amount: 0.0,
            price_currency: "CNY".to_string(),
            duration_unit: "day".to_string(),
            duration_value: 30,
            enabled: true,
            sort_order: 0,
            max_active_per_user: 1,
            purchase_limit_scope: "active_period".to_string(),
            allowed_provider_ids: vec![],
            entitlements_json: json!([]),
            created_at_unix_secs: 1,
            updated_at_unix_secs: 1,
        };
        let input = normalize_plan_input_for_update(sample_plan_request(0.0), &existing)
            .expect("existing zero price saves");

        assert_eq!(input.price_amount, 0.0);
    }

    #[test]
    fn update_paid_plan_rejects_zero_price() {
        let existing = BillingPlanRecord {
            id: "paid-plan".to_string(),
            title: "付费套餐".to_string(),
            description: None,
            price_amount: 100.0,
            price_currency: "CNY".to_string(),
            duration_unit: "month".to_string(),
            duration_value: 1,
            enabled: true,
            sort_order: 0,
            max_active_per_user: 1,
            purchase_limit_scope: "active_period".to_string(),
            allowed_provider_ids: vec![],
            entitlements_json: json!([]),
            created_at_unix_secs: 1,
            updated_at_unix_secs: 1,
        };
        let error = normalize_plan_input_for_update(sample_plan_request(0.0), &existing)
            .expect_err("paid plan cannot become free");

        assert_eq!(error, "price_amount must be positive");
    }

    #[test]
    fn usage_quota_requires_allowed_provider_ids() {
        let mut request = sample_plan_request(9.9);
        request.allowed_provider_ids.clear();
        request.entitlements = json!([{
            "type": "daily_quota",
            "daily_quota_usd": 100.0,
            "allow_wallet_overage": false
        }]);
        let error = normalize_plan_input_for_create(request)
            .expect_err("provider scope should be required");

        assert!(error.contains("供应商"));
    }

    #[test]
    fn usage_quota_saves_providers_and_removes_static_models() {
        let mut request = sample_plan_request(9.9);
        request.entitlements = json!([{
            "type": "daily_quota",
            "daily_quota_usd": 100.0,
            "allow_wallet_overage": false,
            "allowed_global_model_ids": ["global-codex"]
        }]);
        let input = normalize_plan_input_for_create(request).expect("scoped quota should save");

        assert_eq!(input.allowed_provider_ids, vec!["provider-codex"]);
        assert!(input.entitlements_json[0]
            .get("allowed_global_model_ids")
            .is_none());
        assert_eq!(
            input.entitlements_json[0]["allow_wallet_overage"],
            json!(true)
        );
    }

    #[test]
    fn usage_quota_rejects_non_positive_quota_multiplier() {
        for quota_multiplier in [0.0, -1.0] {
            let mut request = sample_plan_request(9.9);
            request.entitlements = json!([{
                "type": "daily_quota",
                "daily_quota_usd": 100.0,
                "quota_multiplier": quota_multiplier,
                "allowed_global_model_ids": ["global-codex"]
            }]);

            let error = normalize_plan_input_for_create(request)
                .expect_err("quota multiplier should be positive");

            assert!(error.contains("quota_multiplier"));
        }
    }
}

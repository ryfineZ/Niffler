use super::super::GatewayControlDecision;
use super::credentials::{contains_string, extract_requested_model};
use super::GatewayControlAuthContext;
use crate::{AppState, GatewayError};
use axum::body::Bytes;
use axum::http::Uri;

const DAILY_QUOTA_EPSILON_USD: f64 = 0.000_000_01;
const UNRESOLVED_REQUESTED_MODEL_QUOTA_SCOPE: &str = "__niffler_unresolved_requested_model__";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestWalletReservationEstimate {
    pub(crate) requested_model: String,
    pub(crate) estimated_cost_usd: f64,
    pub(crate) needed_usd: f64,
    pub(crate) available_usd: Option<f64>,
    pub(crate) wallet_reservation_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GatewayLocalAuthRejection {
    InvalidApiKey,
    LockedApiKey,
    WalletUnavailable,
    BalanceDenied { remaining: Option<f64> },
    ProviderNotAllowed { provider: String },
    ApiFormatNotAllowed { api_format: String },
    ModelNotAllowed { model: String },
}

pub(crate) fn trusted_auth_local_rejection(
    decision: Option<&GatewayControlDecision>,
    headers: &http::HeaderMap,
) -> Option<GatewayLocalAuthRejection> {
    let decision = decision?;
    if decision.route_class.as_deref() != Some("ai_public") {
        return None;
    }

    let rejection = decision
        .local_auth_rejection
        .clone()
        .or_else(|| decision.auth_context.as_ref()?.local_rejection.clone())?;
    if crate::headers::is_json_request(headers)
        && matches!(
            rejection,
            GatewayLocalAuthRejection::BalanceDenied { .. }
                | GatewayLocalAuthRejection::ProviderNotAllowed { .. }
                | GatewayLocalAuthRejection::ApiFormatNotAllowed { .. }
                | GatewayLocalAuthRejection::ModelNotAllowed { .. }
        )
    {
        return None;
    }
    Some(rejection)
}

pub(crate) fn should_buffer_request_for_local_auth(
    decision: Option<&GatewayControlDecision>,
    headers: &http::HeaderMap,
) -> bool {
    let Some(decision) = decision else {
        return false;
    };
    decision.route_class.as_deref() == Some("ai_public")
        && decision.route_kind.as_deref() != Some("files")
        && crate::headers::is_json_request(headers)
}

pub(crate) async fn request_model_local_rejection(
    state: &AppState,
    decision: Option<&GatewayControlDecision>,
    uri: &Uri,
    headers: &http::HeaderMap,
    body: &Bytes,
) -> Result<Option<GatewayLocalAuthRejection>, GatewayError> {
    let Some(decision) = decision else {
        return Ok(None);
    };
    if decision.route_class.as_deref() != Some("ai_public") {
        return Ok(None);
    }
    let Some(auth_context) = decision.auth_context.as_ref() else {
        return Ok(None);
    };
    let requested_model = extract_requested_model(decision, uri, headers, body);
    let mut requested_global_model_id = None::<String>;
    let mut scoped_quota = None;
    let mut pay_as_you_go_allowed = true;
    let deferred_policy_rejection = auth_context
        .local_rejection
        .as_ref()
        .filter(|rejection| {
            matches!(
                rejection,
                GatewayLocalAuthRejection::ProviderNotAllowed { .. }
                    | GatewayLocalAuthRejection::ApiFormatNotAllowed { .. }
                    | GatewayLocalAuthRejection::ModelNotAllowed { .. }
            )
        })
        .cloned();
    if let Some(rejection) = auth_context.local_rejection.as_ref() {
        let deferred_billing_rejection =
            matches!(rejection, GatewayLocalAuthRejection::BalanceDenied { .. });
        if deferred_policy_rejection.is_none() && !deferred_billing_rejection {
            return Ok(Some(rejection.clone()));
        }
        pay_as_you_go_allowed = false;
    }
    if let (Some(allowed_models), Some(requested_model)) = (
        auth_context.allowed_models.as_deref(),
        requested_model.as_deref(),
    ) {
        let group_allows_model = contains_string(allowed_models, requested_model)
            || model_directive_base_model_is_allowed_for_request(
                state,
                decision,
                requested_model,
                allowed_models,
            )
            .await
            || request_model_resolves_to_allowed_model(
                state,
                decision,
                requested_model,
                allowed_models,
            )
            .await?;
        if !group_allows_model {
            pay_as_you_go_allowed = false;
            if niffler_runtime_product_policy_is_enforced(state, auth_context).await? {
                return Ok(Some(GatewayLocalAuthRejection::ModelNotAllowed {
                    model: requested_model.to_string(),
                }));
            }
            requested_global_model_id =
                resolve_requested_global_model_id_for_request(state, decision, requested_model)
                    .await?;
            if requested_global_model_id.is_none() {
                return Ok(Some(GatewayLocalAuthRejection::ModelNotAllowed {
                    model: requested_model.to_string(),
                }));
            }
            scoped_quota = load_user_daily_quota_availability(
                state,
                &auth_context.user_id,
                requested_global_model_id.as_deref(),
            )
            .await?
            .filter(|quota| quota.has_active_daily_quota);
            if !quota_allows_plan_bypass(scoped_quota.as_ref()) {
                return Ok(Some(GatewayLocalAuthRejection::ModelNotAllowed {
                    model: requested_model.to_string(),
                }));
            }
        }
    }
    if let Some(rejection) = deferred_policy_rejection.as_ref() {
        if requested_model.is_none() {
            return Ok(Some(rejection.clone()));
        }
        ensure_requested_global_model_id(
            state,
            decision,
            requested_model.as_deref(),
            &mut requested_global_model_id,
        )
        .await?;
        scoped_quota = load_user_daily_quota_availability(
            state,
            &auth_context.user_id,
            requested_global_model_id.as_deref(),
        )
        .await?
        .filter(|quota| quota.has_active_daily_quota);
        if !quota_allows_plan_bypass(scoped_quota.as_ref()) {
            return Ok(Some(rejection.clone()));
        }
    }

    balance_capacity_rejection(
        state,
        decision,
        auth_context,
        requested_model.as_deref(),
        requested_global_model_id,
        scoped_quota,
        pay_as_you_go_allowed,
    )
    .await
}

async fn niffler_runtime_product_policy_is_enforced(
    state: &AppState,
    auth_context: &GatewayControlAuthContext,
) -> Result<bool, GatewayError> {
    if auth_context.api_key_id.trim().is_empty() {
        return Ok(false);
    }
    Ok(
        crate::niffler_runtime::resolve_niffler_runtime_policy_snapshot(
            state,
            &auth_context.api_key_id,
        )
        .await?
        .is_some(),
    )
}

fn quota_allows_plan_bypass(
    quota: Option<&aether_data_contracts::repository::billing::UserDailyQuotaAvailabilityRecord>,
) -> bool {
    quota.is_some_and(|quota| quota.base_remaining_usd > DAILY_QUOTA_EPSILON_USD)
}

async fn load_user_daily_quota_availability(
    state: &AppState,
    user_id: &str,
    global_model_id: Option<&str>,
) -> Result<
    Option<aether_data_contracts::repository::billing::UserDailyQuotaAvailabilityRecord>,
    GatewayError,
> {
    state
        .find_user_daily_quota_availability_for_global_model(user_id, global_model_id)
        .await
}

async fn ensure_requested_global_model_id(
    state: &AppState,
    decision: &GatewayControlDecision,
    requested_model: Option<&str>,
    requested_global_model_id: &mut Option<String>,
) -> Result<(), GatewayError> {
    if requested_global_model_id.is_some() {
        return Ok(());
    }
    let Some(requested_model) = requested_model else {
        return Ok(());
    };
    *requested_global_model_id =
        resolve_requested_global_model_id_for_request(state, decision, requested_model).await?;
    Ok(())
}

async fn scoped_quota_for_request(
    state: &AppState,
    decision: &GatewayControlDecision,
    auth_context: &GatewayControlAuthContext,
    requested_model: Option<&str>,
    requested_global_model_id: &mut Option<String>,
    preloaded_quota: Option<
        aether_data_contracts::repository::billing::UserDailyQuotaAvailabilityRecord,
    >,
) -> Result<
    Option<aether_data_contracts::repository::billing::UserDailyQuotaAvailabilityRecord>,
    GatewayError,
> {
    if let Some(quota) = preloaded_quota {
        return Ok(Some(quota).filter(|quota| quota.has_active_daily_quota));
    }

    ensure_requested_global_model_id(state, decision, requested_model, requested_global_model_id)
        .await?;
    let quota_global_model_id = requested_global_model_id.as_deref().or_else(|| {
        requested_model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|_| UNRESOLVED_REQUESTED_MODEL_QUOTA_SCOPE)
    });
    load_user_daily_quota_availability(state, &auth_context.user_id, quota_global_model_id)
        .await
        .map(|quota| quota.filter(|quota| quota.has_active_daily_quota))
}

async fn balance_capacity_rejection(
    state: &AppState,
    decision: &GatewayControlDecision,
    auth_context: &GatewayControlAuthContext,
    requested_model: Option<&str>,
    mut requested_global_model_id: Option<String>,
    preloaded_quota: Option<
        aether_data_contracts::repository::billing::UserDailyQuotaAvailabilityRecord,
    >,
    pay_as_you_go_allowed: bool,
) -> Result<Option<GatewayLocalAuthRejection>, GatewayError> {
    if pay_as_you_go_allowed
        && auth_context
            .balance_remaining
            .is_none_or(|balance| balance > 0.0)
    {
        return Ok(None);
    }
    if requested_model.is_none() {
        return Ok(None);
    }
    let quota = scoped_quota_for_request(
        state,
        decision,
        auth_context,
        requested_model,
        &mut requested_global_model_id,
        preloaded_quota,
    )
    .await?;
    let plan_available = quota_allows_plan_bypass(quota.as_ref());
    if !pay_as_you_go_allowed {
        return Ok(
            (!plan_available).then_some(GatewayLocalAuthRejection::BalanceDenied {
                remaining: auth_context.balance_remaining,
            }),
        );
    }
    if plan_available {
        Ok(None)
    } else {
        Ok(Some(GatewayLocalAuthRejection::BalanceDenied {
            remaining: auth_context.balance_remaining,
        }))
    }
}

pub(crate) async fn estimate_request_wallet_reservation(
    state: &AppState,
    decision: &GatewayControlDecision,
    auth_context: &GatewayControlAuthContext,
    requested_model: Option<&str>,
    requested_global_model_id: &mut Option<String>,
    preloaded_quota: Option<
        aether_data_contracts::repository::billing::UserDailyQuotaAvailabilityRecord,
    >,
    pay_as_you_go_allowed: bool,
    headers: &http::HeaderMap,
    body: &Bytes,
) -> Result<Option<RequestWalletReservationEstimate>, GatewayError> {
    if auth_context.api_key_is_standalone {
        return Ok(None);
    }
    if auth_context.local_rejection.is_some() {
        return Ok(None);
    }
    let Some(requested_model) = requested_model else {
        return Ok(None);
    };
    let Some(estimated_cost_usd) =
        estimate_request_cost_upper_bound_usd(state, decision, requested_model, headers, body)
            .await?
    else {
        return Ok(None);
    };
    if estimated_cost_usd <= DAILY_QUOTA_EPSILON_USD {
        return Ok(None);
    }
    let quota = scoped_quota_for_request(
        state,
        decision,
        auth_context,
        Some(requested_model),
        requested_global_model_id,
        preloaded_quota,
    )
    .await?;
    let wallet = state
        .read_wallet_snapshot_for_auth(
            &auth_context.user_id,
            &auth_context.api_key_id,
            auth_context.api_key_is_standalone,
        )
        .await?;
    let wallet_available_usd = wallet.as_ref().and_then(wallet_finite_available_usd);
    let wallet_is_unlimited = wallet
        .as_ref()
        .is_some_and(|wallet| wallet.limit_mode.eq_ignore_ascii_case("unlimited"));
    let sales_multiplier =
        sales_multiplier_for_auth_context(auth_context, requested_global_model_id.as_deref());
    let (needed_usd, available_usd, wallet_reservation_usd) = match quota.as_ref() {
        Some(quota) if !quota.allow_wallet_overage => (
            estimated_cost_usd,
            Some(quota.base_remaining_usd.max(0.0)),
            0.0,
        ),
        Some(quota) if !pay_as_you_go_allowed => (
            estimated_cost_usd,
            Some(quota.base_remaining_usd.max(0.0)),
            0.0,
        ),
        Some(quota) if wallet_is_unlimited => {
            let base_overage = (estimated_cost_usd - quota.base_remaining_usd.max(0.0)).max(0.0);
            if base_overage <= DAILY_QUOTA_EPSILON_USD {
                return Ok(None);
            }
            return Ok(None);
        }
        Some(quota) => {
            let wallet_needed = (estimated_cost_usd - quota.base_remaining_usd.max(0.0)).max(0.0)
                * sales_multiplier;
            (wallet_needed, wallet_available_usd, wallet_needed)
        }
        None if wallet_is_unlimited => return Ok(None),
        None => {
            let wallet_needed = estimated_cost_usd * sales_multiplier;
            (wallet_needed, wallet_available_usd, wallet_needed)
        }
    };
    Ok(Some(RequestWalletReservationEstimate {
        requested_model: requested_model.to_string(),
        estimated_cost_usd,
        needed_usd,
        available_usd,
        wallet_reservation_usd: wallet_reservation_usd.max(0.0),
    }))
}

fn wallet_finite_available_usd(
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
) -> Option<f64> {
    if !wallet.status.eq_ignore_ascii_case("active")
        || wallet.limit_mode.eq_ignore_ascii_case("unlimited")
    {
        return None;
    }
    Some(wallet.balance.max(0.0) + wallet.gift_balance.max(0.0))
}

async fn estimate_request_cost_upper_bound_usd(
    state: &AppState,
    decision: &GatewayControlDecision,
    requested_model: &str,
    headers: &http::HeaderMap,
    body: &Bytes,
) -> Result<Option<f64>, GatewayError> {
    let Some(api_format) = decision
        .auth_endpoint_signature
        .as_deref()
        .map(crate::ai_serving::normalize_api_format_alias)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let body = crate::headers::decoded_request_body_bytes(headers, body.as_ref()).ok();
    let Some(body) = body else {
        return Ok(None);
    };
    let body_json = serde_json::from_slice::<serde_json::Value>(body.as_ref()).ok();
    let Some(input_tokens) = body_json
        .as_ref()
        .map(estimate_json_tokens)
        .filter(|value| *value > 0)
    else {
        return Ok(None);
    };
    let max_output_tokens = body_json.as_ref().and_then(max_output_tokens_from_request);
    let candidates = state
        .list_minimal_candidate_selection_rows_for_api_format_and_requested_model(
            &api_format,
            requested_model,
        )
        .await?;
    let mut max_estimate = None::<f64>;
    for candidate in candidates {
        let context = state
            .data
            .find_billing_model_context_by_model_id(
                &candidate.provider_id,
                Some(&candidate.key_id),
                &candidate.model_id,
            )
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;
        let Some(context) = context else {
            continue;
        };
        let Some(estimate) =
            estimate_cost_from_billing_context(&context, input_tokens, max_output_tokens)
        else {
            return Ok(None);
        };
        max_estimate = Some(max_estimate.map_or(estimate, |current| current.max(estimate)));
    }
    Ok(max_estimate.filter(|value| value.is_finite() && *value >= 0.0))
}

fn estimate_cost_from_billing_context(
    context: &aether_data_contracts::repository::billing::StoredBillingModelContext,
    input_tokens: u64,
    max_output_tokens: Option<u64>,
) -> Option<f64> {
    if context
        .provider_billing_type
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("free_tier"))
    {
        return Some(0.0);
    }
    let price_per_request = context
        .model_price_per_request
        .or(context.default_price_per_request)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0);
    let tiered_pricing = effective_tiered_pricing(context);
    let input_price_per_1m = tiered_price_per_1m(tiered_pricing, "input_price_per_1m")
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0);
    let output_price_per_1m = tiered_price_per_1m(tiered_pricing, "output_price_per_1m")
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0);
    let output_tokens = if output_price_per_1m > 0.0 {
        max_output_tokens?
    } else {
        0
    };
    let estimate = price_per_request
        + (input_tokens as f64 * input_price_per_1m / 1_000_000.0)
        + (output_tokens as f64 * output_price_per_1m / 1_000_000.0);
    Some(estimate)
}

fn sales_multiplier_for_auth_context(
    auth_context: &GatewayControlAuthContext,
    global_model_id: Option<&str>,
) -> f64 {
    if let Some(model_multiplier) = global_model_id.and_then(|model_id| {
        model_sales_multiplier(&auth_context.model_sales_multipliers, model_id)
    }) {
        return model_multiplier;
    }
    normalize_sales_multiplier(auth_context.sales_multiplier)
}

fn model_sales_multiplier(
    model_sales_multipliers: &Option<serde_json::Value>,
    model_id: &str,
) -> Option<f64> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return None;
    }
    model_sales_multipliers
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.get(model_id))
        .and_then(serde_json::Value::as_f64)
        .map(normalize_sales_multiplier)
}

fn normalize_sales_multiplier(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        1.0
    }
}

fn effective_tiered_pricing(
    context: &aether_data_contracts::repository::billing::StoredBillingModelContext,
) -> Option<&serde_json::Value> {
    context
        .model_tiered_pricing
        .as_ref()
        .filter(|value| tiered_pricing_has_rates(value))
        .or(context.default_tiered_pricing.as_ref())
}

fn tiered_pricing_has_rates(value: &serde_json::Value) -> bool {
    value
        .get("tiers")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|tiers| !tiers.is_empty())
        || ["input_price_per_1m", "output_price_per_1m"]
            .iter()
            .any(|field| {
                value
                    .get(*field)
                    .and_then(serde_json::Value::as_f64)
                    .is_some()
            })
}

fn rate_multiplier_for_api_format(
    context: &aether_data_contracts::repository::billing::StoredBillingModelContext,
    api_format: &str,
) -> f64 {
    let normalized_api_format = api_format.trim().to_ascii_lowercase();
    let Some(mapping) = context
        .provider_api_key_rate_multipliers
        .as_ref()
        .and_then(serde_json::Value::as_object)
    else {
        return 1.0;
    };
    mapping
        .get(&normalized_api_format)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(1.0)
}

fn tiered_price_per_1m(tiered_pricing: Option<&serde_json::Value>, field: &str) -> Option<f64> {
    let value = tiered_pricing?;
    value
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            value
                .get("tiers")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|tier| tier.get(field).and_then(serde_json::Value::as_f64))
                .filter(|price| price.is_finite() && *price >= 0.0)
                .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
        })
}

fn max_output_tokens_from_request(value: &serde_json::Value) -> Option<u64> {
    ["max_tokens", "max_completion_tokens", "max_output_tokens"]
        .iter()
        .find_map(|field| value.get(*field).and_then(serde_json::Value::as_u64))
        .filter(|value| *value > 0)
}

fn estimate_json_tokens(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::String(text) => estimate_text_tokens(text),
        serde_json::Value::Array(items) => items
            .iter()
            .map(estimate_json_tokens)
            .fold(0u64, u64::saturating_add),
        serde_json::Value::Object(object) => object
            .iter()
            .map(|(key, value)| {
                estimate_text_tokens(key).saturating_add(estimate_json_tokens(value))
            })
            .fold(0u64, u64::saturating_add),
        _ => 1,
    }
}

fn estimate_text_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    chars.div_ceil(4).max(1)
}

async fn model_directive_base_model_is_allowed_for_request(
    state: &AppState,
    decision: &GatewayControlDecision,
    requested_model: &str,
    allowed_models: &[String],
) -> bool {
    let Some(base_model) = crate::ai_serving::model_directive_base_model(requested_model) else {
        return false;
    };
    if !contains_string(allowed_models, &base_model) {
        return false;
    }
    let Some(client_api_format) = decision
        .auth_endpoint_signature
        .as_deref()
        .map(crate::ai_serving::normalize_api_format_alias)
        .filter(|value| !value.trim().is_empty())
    else {
        return false;
    };
    for api_format in candidate_api_formats_for_model_resolution(&client_api_format) {
        if crate::system_features::reasoning_model_directive_enabled_for_api_format_and_model(
            state,
            &api_format,
            Some(requested_model),
        )
        .await
        {
            return true;
        }
    }
    false
}

async fn request_model_resolves_to_allowed_model(
    state: &AppState,
    decision: &GatewayControlDecision,
    requested_model: &str,
    allowed_models: &[String],
) -> Result<bool, GatewayError> {
    let Some(client_api_format) = decision
        .auth_endpoint_signature
        .as_deref()
        .map(crate::ai_serving::normalize_api_format_alias)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(false);
    };

    for api_format in candidate_api_formats_for_model_resolution(&client_api_format) {
        let enable_model_directives =
            crate::system_features::reasoning_model_directive_enabled_for_api_format_and_model(
                state,
                &api_format,
                Some(requested_model),
            )
            .await;
        let rows = state
            .list_minimal_candidate_selection_rows_for_api_format(&api_format)
            .await?;
        let matching_rows = rows
            .into_iter()
            .filter(|row| {
                aether_scheduler_core::row_supports_requested_model_with_model_directives(
                    row,
                    requested_model,
                    &api_format,
                    enable_model_directives,
                )
            })
            .collect::<Vec<_>>();
        let Some(resolved_global_model) =
            aether_scheduler_core::resolve_requested_global_model_name_with_model_directives(
                &matching_rows,
                requested_model,
                &api_format,
                enable_model_directives,
            )
        else {
            continue;
        };
        if contains_string(allowed_models, &resolved_global_model) {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn resolve_requested_global_model_id_for_request(
    state: &AppState,
    decision: &GatewayControlDecision,
    requested_model: &str,
) -> Result<Option<String>, GatewayError> {
    let Some(client_api_format) = decision
        .auth_endpoint_signature
        .as_deref()
        .map(crate::ai_serving::normalize_api_format_alias)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };

    for api_format in candidate_api_formats_for_model_resolution(&client_api_format) {
        let enable_model_directives =
            crate::system_features::reasoning_model_directive_enabled_for_api_format_and_model(
                state,
                &api_format,
                Some(requested_model),
            )
            .await;
        let requested_rows = state
            .list_minimal_candidate_selection_rows_for_api_format_and_requested_model(
                &api_format,
                requested_model,
            )
            .await?;
        let mut matching_rows = requested_rows
            .into_iter()
            .filter(|row| {
                aether_scheduler_core::row_supports_requested_model_with_model_directives(
                    row,
                    requested_model,
                    &api_format,
                    enable_model_directives,
                )
            })
            .collect::<Vec<_>>();
        if matching_rows.is_empty() {
            matching_rows = state
                .list_minimal_candidate_selection_rows_for_api_format(&api_format)
                .await?
                .into_iter()
                .filter(|row| {
                    aether_scheduler_core::row_supports_requested_model_with_model_directives(
                        row,
                        requested_model,
                        &api_format,
                        enable_model_directives,
                    )
                })
                .collect::<Vec<_>>();
        }
        let Some(resolved_global_model) =
            aether_scheduler_core::resolve_requested_global_model_name_with_model_directives(
                &matching_rows,
                requested_model,
                &api_format,
                enable_model_directives,
            )
        else {
            continue;
        };
        if let Some(row) = matching_rows
            .iter()
            .find(|row| row.global_model_name == resolved_global_model)
            .or_else(|| matching_rows.first())
        {
            return Ok(Some(row.global_model_id.clone()));
        }
    }

    Ok(None)
}

fn candidate_api_formats_for_model_resolution(client_api_format: &str) -> Vec<String> {
    let mut api_formats = Vec::new();
    push_unique_api_format(&mut api_formats, client_api_format);
    for api_format in crate::ai_serving::request_candidate_api_formats(client_api_format, false) {
        push_unique_api_format(&mut api_formats, api_format);
    }
    api_formats
}

fn push_unique_api_format(api_formats: &mut Vec<String>, api_format: &str) {
    let api_format = crate::ai_serving::normalize_api_format_alias(api_format);
    if api_format.is_empty() || api_formats.iter().any(|value| value == &api_format) {
        return;
    }
    api_formats.push(api_format);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use aether_data::repository::candidate_selection::InMemoryMinimalCandidateSelectionReadRepository;
    use aether_data::repository::wallet::StoredWalletSnapshot;
    use aether_data_contracts::repository::billing::{
        BillingReadRepository, StoredBillingModelContext, UserDailyQuotaAvailabilityRecord,
    };
    use aether_data_contracts::repository::candidate_selection::{
        MinimalCandidateSelectionReadRepository, StoredMinimalCandidateSelectionRow,
        StoredPoolKeyCandidateRowsByKeyIdsQuery, StoredPoolKeyCandidateRowsQuery,
        StoredProviderModelMapping, StoredRequestedModelCandidateRowsQuery,
    };
    use aether_data_contracts::DataLayerError;
    use async_trait::async_trait;
    use axum::body::Bytes;
    use axum::http::{HeaderMap, Uri};
    use serde_json::json;

    use super::{
        estimate_cost_from_billing_context, request_model_local_rejection,
        trusted_auth_local_rejection, GatewayLocalAuthRejection,
    };
    use crate::control::{GatewayControlAuthContext, GatewayControlDecision};
    use crate::data::GatewayDataState;
    use crate::AppState;

    fn sample_row() -> StoredMinimalCandidateSelectionRow {
        StoredMinimalCandidateSelectionRow {
            provider_id: "provider-1".to_string(),
            provider_name: "Provider 1".to_string(),
            provider_type: "openai".to_string(),
            provider_priority: 0,
            provider_is_active: true,
            endpoint_id: "endpoint-1".to_string(),
            endpoint_api_format: "openai:chat".to_string(),
            endpoint_api_family: Some("openai".to_string()),
            endpoint_kind: Some("chat".to_string()),
            endpoint_is_active: true,
            key_id: "key-1".to_string(),
            key_name: "key".to_string(),
            key_auth_type: "api_key".to_string(),
            key_is_active: true,
            key_api_formats: Some(vec!["openai:chat".to_string()]),
            key_allowed_models: None,
            key_capabilities: None,
            key_internal_priority: 0,
            key_global_priority_by_format: None,
            model_id: "model-1".to_string(),
            global_model_id: "global-model-1".to_string(),
            global_model_name: "gpt-5".to_string(),
            global_model_mappings: Some(vec!["gpt-5(?:\\.\\d+)?".to_string()]),
            global_model_supports_streaming: Some(true),
            model_provider_model_name: "gpt-5-upstream".to_string(),
            model_provider_model_mappings: Some(vec![StoredProviderModelMapping {
                name: "gpt-5-upstream".to_string(),
                priority: 1,
                api_formats: Some(vec!["openai:chat".to_string()]),
                endpoint_ids: None,
            }]),
            model_supports_streaming: Some(true),
            model_is_active: true,
            model_is_available: true,
        }
    }

    fn sample_row_for_api_format(api_format: &str) -> StoredMinimalCandidateSelectionRow {
        let mut row = sample_row();
        let api_family = api_format
            .split_once(':')
            .map(|(family, _)| family)
            .unwrap_or(api_format);
        row.provider_id = format!("provider-{api_family}");
        row.provider_name = format!("Provider {api_family}");
        row.provider_type = api_family.to_string();
        row.endpoint_id = format!("endpoint-{api_family}");
        row.endpoint_api_format = api_format.to_string();
        row.endpoint_api_family = Some(api_family.to_string());
        row.key_id = format!("key-{api_family}");
        row.key_api_formats = Some(vec![api_format.to_string()]);
        if let Some(mappings) = row.model_provider_model_mappings.as_mut() {
            for mapping in mappings {
                mapping.api_formats = Some(vec![api_format.to_string()]);
            }
        }
        row
    }

    fn decision_with_allowed_models(allowed_models: Vec<String>) -> GatewayControlDecision {
        let mut decision = GatewayControlDecision::synthetic(
            "/v1/chat/completions",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
        );
        decision.auth_context = Some(GatewayControlAuthContext {
            user_id: "user-1".to_string(),
            api_key_id: "api-key-1".to_string(),
            username: None,
            api_key_name: None,
            api_key_group_id: None,
            api_key_group_name: None,
            sales_multiplier: 1.0,
            model_sales_multipliers: None,
            balance_remaining: None,
            access_allowed: true,
            user_rate_limit: None,
            api_key_rate_limit: None,
            api_key_is_standalone: false,
            admin_bypass_limits: false,
            local_rejection: None,
            allowed_models: Some(allowed_models),
        });
        decision
    }

    fn state_with_rows(rows: Vec<StoredMinimalCandidateSelectionRow>) -> AppState {
        let repository = Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(rows));
        let data = GatewayDataState::with_minimal_candidate_selection_reader_for_tests(repository);
        AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(data)
    }

    fn state_with_quota_and_wallet(
        quota: UserDailyQuotaAvailabilityRecord,
        context: StoredBillingModelContext,
    ) -> AppState {
        state_with_quota_wallet_and_counter(quota, context, None)
    }

    fn state_with_quota_wallet_and_counter(
        quota: UserDailyQuotaAvailabilityRecord,
        context: StoredBillingModelContext,
        quota_lookup_count: Option<Arc<AtomicUsize>>,
    ) -> AppState {
        state_with_quota_context_wallet_balance(quota, context, 30.0, quota_lookup_count)
    }

    fn state_with_quota_context_wallet_balance(
        quota: UserDailyQuotaAvailabilityRecord,
        context: StoredBillingModelContext,
        wallet_balance: f64,
        quota_lookup_count: Option<Arc<AtomicUsize>>,
    ) -> AppState {
        let candidate_repository =
            Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(vec![
                sample_row(),
            ]));
        let billing_repository = Arc::new(FixedBillingReadRepository {
            quota,
            context,
            quota_lookup_count,
        });
        let data = GatewayDataState::with_minimal_candidate_selection_and_billing_for_tests(
            candidate_repository,
            billing_repository,
        );
        AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(data)
            .with_auth_wallets_for_tests(vec![sample_wallet("user-1", wallet_balance)])
    }

    fn state_with_model_mapping() -> AppState {
        state_with_rows(vec![sample_row()])
    }

    fn json_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            "application/json"
                .parse()
                .expect("content type should parse"),
        );
        headers
    }

    fn billing_context_with_pricing(
        default_tiered_pricing: Option<serde_json::Value>,
        model_tiered_pricing: Option<serde_json::Value>,
        rate_multipliers: Option<serde_json::Value>,
        billing_type: Option<&str>,
    ) -> StoredBillingModelContext {
        StoredBillingModelContext::new(
            "provider-1".to_string(),
            billing_type.map(ToOwned::to_owned),
            None,
            Some("key-1".to_string()),
            rate_multipliers,
            Some(60),
            "global-model-1".to_string(),
            "gpt-5".to_string(),
            None,
            None,
            default_tiered_pricing,
            Some("model-1".to_string()),
            Some("gpt-5-upstream".to_string()),
            None,
            None,
            model_tiered_pricing,
        )
        .expect("billing context should build")
    }

    fn sample_wallet(user_id: &str, balance: f64) -> StoredWalletSnapshot {
        StoredWalletSnapshot::new(
            format!("wallet-{user_id}"),
            Some(user_id.to_string()),
            None,
            balance,
            0.0,
            "finite".to_string(),
            "USD".to_string(),
            "active".to_string(),
            balance,
            0.0,
            0.0,
            0.0,
            100,
        )
        .expect("wallet should build")
    }

    fn quota_availability(
        remaining_usd: f64,
        allow_wallet_overage: bool,
    ) -> UserDailyQuotaAvailabilityRecord {
        UserDailyQuotaAvailabilityRecord {
            has_active_daily_quota: true,
            total_quota_usd: remaining_usd,
            used_usd: 0.0,
            remaining_usd,
            base_remaining_usd: remaining_usd,
            allow_wallet_overage,
            eligible_entitlement_ids: Vec::new(),
            allowed_provider_ids: Vec::new(),
            provider_ids_by_entitlement: Default::default(),
        }
    }

    #[derive(Debug)]
    struct FixedBillingReadRepository {
        quota: UserDailyQuotaAvailabilityRecord,
        context: StoredBillingModelContext,
        quota_lookup_count: Option<Arc<AtomicUsize>>,
    }

    #[async_trait]
    impl BillingReadRepository for FixedBillingReadRepository {
        async fn find_model_context(
            &self,
            _provider_id: &str,
            _provider_api_key_id: Option<&str>,
            _global_model_name: &str,
        ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
            Ok(Some(self.context.clone()))
        }

        async fn find_model_context_by_model_id(
            &self,
            _provider_id: &str,
            _provider_api_key_id: Option<&str>,
            _model_id: &str,
        ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
            Ok(Some(self.context.clone()))
        }

        async fn find_user_daily_quota_availability(
            &self,
            _user_id: &str,
        ) -> Result<Option<UserDailyQuotaAvailabilityRecord>, DataLayerError> {
            Ok(Some(self.quota.clone()))
        }

        async fn find_user_daily_quota_availability_for_global_model(
            &self,
            _user_id: &str,
            _global_model_id: Option<&str>,
        ) -> Result<Option<UserDailyQuotaAvailabilityRecord>, DataLayerError> {
            if let Some(counter) = &self.quota_lookup_count {
                counter.fetch_add(1, Ordering::Relaxed);
            }
            Ok(Some(self.quota.clone()))
        }
    }

    #[derive(Debug, Default)]
    struct PanicCandidateSelectionReadRepository;

    #[async_trait]
    impl MinimalCandidateSelectionReadRepository for PanicCandidateSelectionReadRepository {
        async fn list_for_exact_api_format(
            &self,
            _api_format: &str,
        ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
            panic!("model resolution should not query candidates");
        }

        async fn list_for_exact_api_format_and_global_model(
            &self,
            _api_format: &str,
            _global_model_name: &str,
        ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
            panic!("model resolution should not query candidates");
        }

        async fn list_for_exact_api_format_and_requested_model(
            &self,
            _api_format: &str,
            _requested_model_name: &str,
        ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
            panic!("model resolution should not query candidates");
        }

        async fn list_for_exact_api_format_and_requested_model_page(
            &self,
            _query: &StoredRequestedModelCandidateRowsQuery,
        ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
            panic!("model resolution should not query candidates");
        }

        async fn list_pool_key_rows_for_group(
            &self,
            _query: &StoredPoolKeyCandidateRowsQuery,
        ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
            panic!("model resolution should not query candidates");
        }

        async fn list_pool_key_rows_for_group_key_ids(
            &self,
            _query: &StoredPoolKeyCandidateRowsByKeyIdsQuery,
        ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
            panic!("model resolution should not query candidates");
        }
    }

    #[tokio::test]
    async fn model_rejection_allows_requested_model_that_resolves_to_allowed_global_model() {
        let state = state_with_model_mapping();
        let decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"model":"gpt-5.2","messages":[]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("model rejection should resolve");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn model_rejection_allows_cross_format_provider_mapping_to_allowed_global_model() {
        let mut row = sample_row_for_api_format("gemini:generate_content");
        row.model_provider_model_name = "gemini-2.5-pro-upstream".to_string();
        row.model_provider_model_mappings = Some(vec![StoredProviderModelMapping {
            name: "gemini-2.5-pro-alias".to_string(),
            priority: 1,
            api_formats: Some(vec!["gemini:generate_content".to_string()]),
            endpoint_ids: None,
        }]);
        let state = state_with_rows(vec![row]);
        let decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"model":"gemini-2.5-pro-alias","messages":[]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("model rejection should resolve");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn model_rejection_allows_cross_format_regex_mapping_to_allowed_global_model() {
        let state = state_with_rows(vec![sample_row_for_api_format("claude:messages")]);
        let decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"model":"gpt-5.2","messages":[]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("model rejection should resolve");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn model_rejection_denies_requested_model_outside_allowed_global_models() {
        let state = state_with_model_mapping();
        let decision = decision_with_allowed_models(vec!["gpt-4.1".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"model":"gpt-5.2","messages":[]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("model rejection should resolve");

        assert_eq!(
            rejection,
            Some(GatewayLocalAuthRejection::ModelNotAllowed {
                model: "gpt-5.2".to_string(),
            })
        );
    }

    #[tokio::test]
    async fn model_rejection_allows_plan_scoped_model_outside_api_key_group() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 1.0,
                    "output_price_per_1m": 2.0
                }]
            })),
            None,
            None,
            None,
        );
        let state = state_with_quota_and_wallet(quota_availability(50.0, false), context);
        let decision = decision_with_allowed_models(vec!["gpt-4.1".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.2","messages":[{"role":"user","content":"hi"}],"max_tokens":1000}"#,
        );

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("plan scoped model should resolve");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn provider_rejection_waits_for_plan_scope_when_request_body_is_available() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 1.0,
                    "output_price_per_1m": 2.0
                }]
            })),
            None,
            None,
            None,
        );
        let state = state_with_quota_and_wallet(quota_availability(50.0, false), context);
        let mut decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        let deferred_rejection = GatewayLocalAuthRejection::ProviderNotAllowed {
            provider: "anthropic".to_string(),
        };
        decision.local_auth_rejection = Some(deferred_rejection.clone());
        if let Some(auth_context) = decision.auth_context.as_mut() {
            auth_context.local_rejection = Some(deferred_rejection.clone());
        }
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(
            br#"{"model":"gpt-5","messages":[{"role":"user","content":"hi"}],"max_tokens":1000}"#,
        );

        assert_eq!(
            trusted_auth_local_rejection(Some(&decision), &json_headers()),
            None
        );
        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("plan scoped provider rejection should resolve");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn plan_scoped_model_reuses_quota_lookup_for_capacity_check() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 1.0,
                    "output_price_per_1m": 2.0
                }]
            })),
            None,
            None,
            None,
        );
        let quota_lookup_count = Arc::new(AtomicUsize::new(0));
        let state = state_with_quota_wallet_and_counter(
            quota_availability(50.0, false),
            context,
            Some(quota_lookup_count.clone()),
        );
        let decision = decision_with_allowed_models(vec!["gpt-4.1".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.2","messages":[{"role":"user","content":"hi"}],"max_tokens":1000}"#,
        );

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("plan scoped model should resolve");

        assert_eq!(rejection, None);
        assert_eq!(quota_lookup_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn request_without_model_does_not_query_quota_availability() {
        let context = billing_context_with_pricing(None, None, None, None);
        let quota_lookup_count = Arc::new(AtomicUsize::new(0));
        let state = state_with_quota_wallet_and_counter(
            quota_availability(50.0, false),
            context,
            Some(quota_lookup_count.clone()),
        );
        let decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"messages":[{"role":"user","content":"hi"}]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("request without model should resolve");

        assert_eq!(rejection, None);
        assert_eq!(quota_lookup_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn zero_cost_request_does_not_query_quota_availability() {
        let context = billing_context_with_pricing(None, None, None, Some("free_tier"));
        let quota_lookup_count = Arc::new(AtomicUsize::new(0));
        let state = state_with_quota_wallet_and_counter(
            quota_availability(50.0, false),
            context,
            Some(quota_lookup_count.clone()),
        );
        let decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(
            br#"{"model":"gpt-5","messages":[{"role":"user","content":"hi"}],"max_tokens":1000}"#,
        );

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("zero-cost request should resolve");

        assert_eq!(rejection, None);
        assert_eq!(quota_lookup_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn unresolved_model_does_not_use_aggregate_plan_quota_to_bypass_group() {
        let candidate_repository = Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(
            Vec::new(),
        ));
        let billing_repository = Arc::new(FixedBillingReadRepository {
            quota: quota_availability(50.0, false),
            context: billing_context_with_pricing(None, None, None, None),
            quota_lookup_count: None,
        });
        let data = GatewayDataState::with_minimal_candidate_selection_and_billing_for_tests(
            candidate_repository,
            billing_repository,
        );
        let state = AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(data)
            .with_auth_wallets_for_tests(vec![sample_wallet("user-1", 0.0)]);
        let decision = decision_with_allowed_models(vec!["gpt-4.1".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"model":"unknown-model","messages":[]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("model rejection should resolve");

        assert_eq!(
            rejection,
            Some(GatewayLocalAuthRejection::ModelNotAllowed {
                model: "unknown-model".to_string(),
            })
        );
    }

    #[tokio::test]
    async fn standalone_directly_allowed_model_does_not_resolve_global_model() {
        let repository = Arc::new(PanicCandidateSelectionReadRepository);
        let data = GatewayDataState::with_minimal_candidate_selection_reader_for_tests(repository);
        let state = AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(data);
        let mut decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        if let Some(auth_context) = decision.auth_context.as_mut() {
            auth_context.api_key_is_standalone = true;
        }
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"model":"gpt-5","messages":[]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("standalone key should not resolve global model");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn positive_balance_allows_unbounded_output_request_without_cost_estimate() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 1.0,
                    "output_price_per_1m": 2.0
                }]
            })),
            None,
            None,
            None,
        );
        for allow_wallet_overage in [false, true] {
            let state = state_with_quota_and_wallet(
                quota_availability(50.0, allow_wallet_overage),
                context.clone(),
            );
            let decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
            let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
            let body = Bytes::from_static(
                br#"{"model":"gpt-5","messages":[{"role":"user","content":"hi"}],"stream":true}"#,
            );

            let rejection = request_model_local_rejection(
                &state,
                Some(&decision),
                &uri,
                &json_headers(),
                &body,
            )
            .await
            .expect("quota rejection should resolve");

            assert_eq!(rejection, None);
        }
    }

    #[tokio::test]
    async fn positive_wallet_remains_usable_when_daily_quota_is_exhausted() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 1.0,
                    "output_price_per_1m": 2.0
                }]
            })),
            None,
            None,
            None,
        );
        let state = state_with_quota_and_wallet(quota_availability(0.0, false), context);
        let mut decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        if let Some(auth_context) = decision.auth_context.as_mut() {
            auth_context.admin_bypass_limits = true;
        }
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(
            br#"{"model":"gpt-5","messages":[{"role":"user","content":"hi"}],"stream":true,"max_tokens":1000}"#,
        );

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("quota rejection should resolve");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn positive_balance_does_not_use_estimated_cost_as_an_admission_limit() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 0.0,
                    "output_price_per_1m": 60.0
                }]
            })),
            None,
            None,
            None,
        );
        let state = state_with_quota_and_wallet(quota_availability(50.0, false), context);
        let decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(
            br#"{"model":"gpt-5","messages":[{"role":"user","content":"hi"}],"max_tokens":1000000}"#,
        );

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("quota rejection should resolve");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn negative_wallet_is_rejected_without_remaining_plan_quota() {
        let context = billing_context_with_pricing(None, None, None, None);
        let state = state_with_quota_context_wallet_balance(
            quota_availability(0.0, false),
            context,
            -0.01,
            None,
        );
        let mut decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        decision
            .auth_context
            .as_mut()
            .expect("auth context should exist")
            .balance_remaining = Some(-0.01);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"model":"gpt-5","messages":[]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("billing rejection should resolve");

        assert_eq!(
            rejection,
            Some(GatewayLocalAuthRejection::BalanceDenied {
                remaining: Some(-0.01),
            })
        );
    }

    #[tokio::test]
    async fn negative_wallet_can_still_use_remaining_plan_quota() {
        let context = billing_context_with_pricing(None, None, None, None);
        let mut quota = quota_availability(2.0, false);
        quota.eligible_entitlement_ids = vec!["entitlement-1".to_string()];
        quota.allowed_provider_ids = vec!["provider-1".to_string()];
        let state = state_with_quota_context_wallet_balance(quota, context, -10.0, None);
        let mut decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        decision
            .auth_context
            .as_mut()
            .expect("auth context should exist")
            .balance_remaining = Some(-10.0);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(br#"{"model":"gpt-5","messages":[]}"#);

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("billing rejection should resolve");

        assert_eq!(rejection, None);
    }

    #[tokio::test]
    async fn wallet_overage_policy_extends_known_cost_capacity_when_enabled() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 0.0,
                    "output_price_per_1m": 70.0
                }]
            })),
            None,
            None,
            None,
        );
        let state = state_with_quota_and_wallet(quota_availability(50.0, true), context);
        let decision = decision_with_allowed_models(vec!["gpt-5".to_string()]);
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let body = Bytes::from_static(
            br#"{"model":"gpt-5","messages":[{"role":"user","content":"hi"}],"max_tokens":1000000}"#,
        );

        let rejection =
            request_model_local_rejection(&state, Some(&decision), &uri, &json_headers(), &body)
                .await
                .expect("quota rejection should resolve");

        assert_eq!(rejection, None);
    }

    #[test]
    fn daily_quota_estimate_falls_back_to_default_tiers_when_model_tiers_empty() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 3.0,
                    "output_price_per_1m": 15.0
                }]
            })),
            Some(json!({})),
            None,
            None,
        );

        let estimate = estimate_cost_from_billing_context(&context, 1_000_000, Some(1_000_000))
            .expect("estimate should resolve");

        assert_eq!(estimate, 18.0);
    }

    #[test]
    fn daily_quota_estimate_uses_base_model_price_not_provider_cost_multiplier() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 1.0,
                    "output_price_per_1m": 2.0
                }]
            })),
            None,
            Some(json!({ "openai:chat": 2.0 })),
            None,
        );

        let estimate = estimate_cost_from_billing_context(&context, 1_000_000, Some(1_000_000))
            .expect("estimate should resolve");

        assert_eq!(estimate, 3.0);
    }

    #[test]
    fn daily_quota_estimate_treats_free_tier_as_zero_cost() {
        let context = billing_context_with_pricing(
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 3.0,
                    "output_price_per_1m": 15.0
                }]
            })),
            None,
            Some(json!({ "openai:chat": 10.0 })),
            Some("free_tier"),
        );

        let estimate = estimate_cost_from_billing_context(&context, 1_000_000, Some(1_000_000))
            .expect("estimate should resolve");

        assert_eq!(estimate, 0.0);
    }
}

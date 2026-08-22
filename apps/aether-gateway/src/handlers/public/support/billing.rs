use super::support_payment::payment_dodopay::{
    cancel_dodopay_checkout_after_local_failure, create_dodopay_checkout,
    dodopay_callback_base_url, dodopay_return_url, load_dodopay_config,
    normalize_dodopay_checkout_payment_channel, DodopayCheckoutInput,
};
use super::support_payment::payment_epay::{
    build_epay_checkout_url, epay_callback_base_url, load_epay_config, resolve_epay_channel,
    EpayCheckoutInput,
};
use super::{
    build_auth_error_response, build_auth_json_response, resolve_authenticated_local_user,
    resolve_payment_exchange_rate, sanitize_wallet_gateway_response, unix_secs_to_rfc3339,
    AppState, GatewayPublicRequestContext,
};
use aether_data::repository::wallet::{
    CancelPaymentOrderInput, UpdatePendingPaymentOrderGatewayInput, WalletMutationOutcome,
};
use axum::{
    body::{Body, Bytes},
    http,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

const BILLING_STORAGE_UNAVAILABLE_DETAIL: &str = "套餐后端暂不可用";

#[derive(Debug, Deserialize, Default)]
struct BillingPlanCheckoutRequest {
    #[serde(default)]
    payment_method: Option<String>,
    #[serde(default)]
    payment_provider: Option<String>,
    #[serde(default)]
    payment_channel: Option<String>,
}

#[derive(Debug, Clone)]
struct NormalizedBillingPlanCheckoutRequest {
    payment_method: String,
    payment_provider: String,
    payment_channel: Option<String>,
}

fn billing_storage_unavailable_response() -> Response<Body> {
    build_auth_error_response(
        http::StatusCode::SERVICE_UNAVAILABLE,
        BILLING_STORAGE_UNAVAILABLE_DETAIL,
        false,
    )
}

fn normalize_optional_checkout_string(value: Option<String>, max_len: usize) -> Option<String> {
    value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value.chars().count() <= max_len)
}

fn normalize_checkout_request(
    payload: BillingPlanCheckoutRequest,
) -> Result<NormalizedBillingPlanCheckoutRequest, &'static str> {
    let payment_provider = normalize_optional_checkout_string(payload.payment_provider, 30)
        .or_else(|| normalize_optional_checkout_string(payload.payment_method.clone(), 30))
        .unwrap_or_else(|| "epay".to_string());
    if payment_provider != "epay" && payment_provider != "dodopay" {
        return Err("unsupported payment_provider");
    }
    let payment_method = normalize_optional_checkout_string(payload.payment_method, 30)
        .unwrap_or_else(|| payment_provider.clone());
    let payment_channel =
        normalize_optional_checkout_string(payload.payment_channel, 30).or_else(|| {
            (payment_provider == "epay" && payment_method != "epay").then_some(payment_method)
        });
    Ok(NormalizedBillingPlanCheckoutRequest {
        payment_method: payment_provider.clone(),
        payment_provider,
        payment_channel,
    })
}

fn plan_id_from_checkout_path(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let rest = trimmed.strip_prefix("/api/billing/plans/")?;
    let plan_id = rest.strip_suffix("/checkout")?.trim_matches('/');
    if plan_id.is_empty() || plan_id.contains('/') {
        None
    } else {
        Some(plan_id.to_string())
    }
}

fn billing_order_no(now: chrono::DateTime<chrono::Utc>) -> String {
    format!(
        "pp_{}_{}",
        now.format("%Y%m%d%H%M%S%6f"),
        &Uuid::new_v4().simple().to_string()[..12]
    )
}

fn billing_plan_payload(
    record: &aether_data_contracts::repository::billing::BillingPlanRecord,
) -> serde_json::Value {
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

fn billing_plan_snapshot(
    record: &aether_data_contracts::repository::billing::BillingPlanRecord,
) -> serde_json::Value {
    json!({
        "id": record.id,
        "title": record.title,
        "description": record.description,
        "price_amount": record.price_amount,
        "price_currency": record.price_currency,
        "duration_unit": record.duration_unit,
        "duration_value": record.duration_value,
        "max_active_per_user": record.max_active_per_user,
        "purchase_limit_scope": record.purchase_limit_scope,
        "allowed_provider_ids": record.allowed_provider_ids,
        "entitlements": record.entitlements_json,
    })
}

fn plan_has_package_rights(
    record: &aether_data_contracts::repository::billing::BillingPlanRecord,
) -> bool {
    record.entitlements_json.as_array().is_some_and(|items| {
        items.iter().any(|item| {
            matches!(
                item.get("type").and_then(|value| value.as_str()),
                Some("daily_quota" | "membership_group")
            )
        })
    })
}

fn payment_order_payload(
    record: &aether_data::repository::wallet::StoredAdminPaymentOrder,
    plan: &aether_data_contracts::repository::billing::BillingPlanRecord,
) -> serde_json::Value {
    let debt_repayment_usd = record.debt_repayment_usd.max(0.0);
    let plan_amount_usd =
        ((record.amount_usd - debt_repayment_usd).max(0.0) * 100_000_000.0).round() / 100_000_000.0;
    json!({
        "id": record.id,
        "order_no": record.order_no,
        "wallet_id": record.wallet_id,
        "user_id": record.user_id,
        "amount_usd": record.amount_usd,
        "plan_amount_usd": plan_amount_usd,
        "debt_repayment_usd": debt_repayment_usd,
        "pay_amount": record.pay_amount,
        "pay_currency": record.pay_currency,
        "exchange_rate": record.exchange_rate,
        "payment_method": record.payment_method,
        "payment_provider": record.payment_provider,
        "payment_channel": record.payment_channel,
        "gateway_order_id": record.gateway_order_id,
        "gateway_response": sanitize_wallet_gateway_response(record.gateway_response.clone()),
        "status": record.status,
        "order_kind": record.order_kind,
        "product_id": record.product_id,
        "product_snapshot": record.product_snapshot,
        "fulfillment_status": record.fulfillment_status,
        "fulfillment_error": record.fulfillment_error,
        "product": billing_plan_payload(plan),
        "created_at": unix_secs_to_rfc3339(record.created_at_unix_ms),
        "paid_at": record.paid_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "credited_at": record.credited_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "expires_at": record.expires_at_unix_secs.and_then(unix_secs_to_rfc3339),
    })
}

fn entitlement_payload(
    record: &aether_data_contracts::repository::billing::UserPlanEntitlementRecord,
) -> serde_json::Value {
    json!({
        "id": record.id,
        "user_id": record.user_id,
        "plan_id": record.plan_id,
        "payment_order_id": record.payment_order_id,
        "status": record.status,
        "starts_at": unix_secs_to_rfc3339(record.starts_at_unix_secs),
        "expires_at": unix_secs_to_rfc3339(record.expires_at_unix_secs),
        "allowed_provider_ids": record.allowed_provider_ids,
        "entitlements": record.entitlements_snapshot,
        "created_at": unix_secs_to_rfc3339(record.created_at_unix_secs),
        "updated_at": unix_secs_to_rfc3339(record.updated_at_unix_secs),
    })
}

fn quota_summary_payload(
    summary: &aether_data_contracts::repository::billing::UserPlanQuotaSummaryRecord,
) -> serde_json::Value {
    json!({
        "user_id": summary.user_id,
        "entitlement_id": summary.entitlement_id,
        "plan_id": summary.plan_id,
        "plan_title": summary.plan_title,
        "starts_at": unix_secs_to_rfc3339(summary.starts_at_unix_secs),
        "expires_at": unix_secs_to_rfc3339(summary.expires_at_unix_secs),
        "quota_total_usd": summary.quota_total_usd,
        "quota_used_usd": summary.quota_used_usd,
        "quota_remaining_usd": summary.quota_remaining_usd,
        "daily_total_usd": summary.daily_total_usd,
        "daily_used_usd": summary.daily_used_usd,
        "daily_remaining_usd": summary.daily_remaining_usd,
        "daily_window_started_at": summary
            .daily_window_started_at_unix_secs
            .and_then(unix_secs_to_rfc3339),
        "daily_window_ends_at": summary
            .daily_window_ends_at_unix_secs
            .and_then(unix_secs_to_rfc3339),
    })
}

fn compute_plan_payment_amounts(
    plan: &aether_data_contracts::repository::billing::BillingPlanRecord,
    pay_currency: &str,
    usd_exchange_rate: f64,
) -> Result<(f64, f64), &'static str> {
    if !plan.price_amount.is_finite() || plan.price_amount <= 0.0 || usd_exchange_rate <= 0.0 {
        return Err("套餐价格配置无效");
    }
    if plan.price_currency.eq_ignore_ascii_case(pay_currency) {
        let amount_usd =
            (plan.price_amount / usd_exchange_rate * 100_000_000.0).round() / 100_000_000.0;
        let pay_amount = (plan.price_amount * 100.0).round() / 100.0;
        return Ok((amount_usd, pay_amount));
    }
    if plan.price_currency.eq_ignore_ascii_case("USD") {
        let amount_usd = (plan.price_amount * 100_000_000.0).round() / 100_000_000.0;
        let pay_amount = (plan.price_amount * usd_exchange_rate * 100.0).round() / 100.0;
        return Ok((amount_usd, pay_amount));
    }
    Err("套餐币种与支付网关币种不匹配")
}

fn compute_dodopay_plan_payment_amounts(
    plan: &aether_data_contracts::repository::billing::BillingPlanRecord,
    pay_currency: &str,
    usd_exchange_rate: f64,
) -> Result<(f64, f64), &'static str> {
    compute_plan_payment_amounts(plan, pay_currency, usd_exchange_rate)
}

pub(super) async fn handle_billing_plans_list(state: &AppState) -> Response<Body> {
    let plans = match state.list_billing_plans(false).await {
        Ok(Some(value)) => value,
        Ok(None) => return billing_storage_unavailable_response(),
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("billing plan lookup failed: {err:?}"),
                false,
            )
        }
    };
    let items = plans
        .iter()
        .filter(|plan| plan_has_package_rights(plan))
        .map(billing_plan_payload)
        .collect::<Vec<_>>();
    Json(json!({"items": items, "total": items.len()})).into_response()
}

pub(super) async fn handle_billing_entitlements(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let user_ids = [auth.user.id.clone()];
    let (entitlements_result, quota_summaries_result) = tokio::join!(
        state.list_user_plan_entitlements(&auth.user.id),
        state.list_active_user_plan_quota_summaries(&user_ids),
    );
    let entitlements = match entitlements_result {
        Ok(Some(value)) => value,
        Ok(None) => return billing_storage_unavailable_response(),
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("billing entitlement lookup failed: {err:?}"),
                false,
            )
        }
    };
    let (quota_summary, quota_summary_status) = match quota_summaries_result {
        Ok(Some(value)) => (value.first().map(quota_summary_payload), "ok"),
        Ok(None) => (None, "unavailable"),
        Err(err) => {
            tracing::error!(
                user_id = %auth.user.id,
                error = ?err,
                "billing quota summary lookup failed"
            );
            (None, "unavailable")
        }
    };
    let now = Utc::now().timestamp().max(0) as u64;
    let items = entitlements
        .iter()
        .filter(|record| {
            record.status == "active"
                && record.starts_at_unix_secs <= now
                && record.expires_at_unix_secs > now
        })
        .map(|record| {
            let mut payload = entitlement_payload(record);
            payload["active"] = json!(true);
            payload
        })
        .collect::<Vec<_>>();
    Json(json!({
        "items": items,
        "total": items.len(),
        "quota_summary": quota_summary,
        "quota_summary_status": quota_summary_status,
    }))
    .into_response()
}

pub(super) async fn handle_billing_plan_checkout(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
    request_body: Option<&Bytes>,
) -> Response<Body> {
    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(plan_id) = plan_id_from_checkout_path(&request_context.request_path) else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "缺少套餐ID", false);
    };
    let payload = match request_body {
        Some(body) if !body.is_empty() => {
            match serde_json::from_slice::<BillingPlanCheckoutRequest>(body) {
                Ok(value) => value,
                Err(_) => {
                    return build_auth_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "输入验证失败",
                        false,
                    )
                }
            }
        }
        _ => BillingPlanCheckoutRequest::default(),
    };
    let checkout_request = match normalize_checkout_request(payload) {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false)
        }
    };

    let plan = match state.find_billing_plan(&plan_id).await {
        Ok(Some(value)) if value.enabled => value,
        Ok(Some(_)) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, "套餐已下架", false)
        }
        Ok(None) => {
            return build_auth_error_response(http::StatusCode::NOT_FOUND, "套餐不存在", false)
        }
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("billing plan lookup failed: {err:?}"),
                false,
            )
        }
    };
    if !plan_has_package_rights(&plan) {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "余额包已移除，请使用钱包充值功能",
            false,
        );
    }
    let now = Utc::now();
    let order_no = billing_order_no(now);
    let expires_at = now + chrono::Duration::minutes(30);
    if checkout_request.payment_provider == "dodopay" {
        let config = match load_dodopay_config(state).await {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false)
            }
        };
        let exchange_rate = match resolve_payment_exchange_rate(
            state,
            &auth.user.id,
            &config.pay_currency,
            config.usd_exchange_rate,
        )
        .await
        {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    detail,
                    false,
                )
            }
        };
        let (plan_amount_usd, plan_pay_amount) = match compute_dodopay_plan_payment_amounts(
            &plan,
            &config.pay_currency,
            exchange_rate.rate,
        ) {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false)
            }
        };
        let payment_channel = match normalize_dodopay_checkout_payment_channel(
            checkout_request.payment_channel.as_deref(),
        ) {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
            }
        };
        let Some(callback_base_url) = dodopay_callback_base_url(
            config.callback_base_url.as_deref(),
            headers,
            request_context,
        ) else {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "DoDoPay 回调站点根地址不可用",
                false,
            );
        };
        let mut pending_gateway_response = json!({
            "gateway": "dodopay",
            "provider_order_status": "pending_checkout",
        });
        exchange_rate.enrich_payload(&mut pending_gateway_response);
        let outcome = match state
            .create_plan_purchase_order(
                aether_data::repository::wallet::CreatePlanPurchaseOrderInput {
                    preferred_wallet_id: None,
                    user_id: auth.user.id.clone(),
                    amount_usd: plan_amount_usd,
                    pay_amount: plan_pay_amount,
                    pay_currency: config.pay_currency.clone(),
                    exchange_rate: exchange_rate.rate,
                    payment_method: checkout_request.payment_method.clone(),
                    payment_provider: Some(checkout_request.payment_provider.clone()),
                    payment_channel: Some(payment_channel.clone()),
                    gateway_order_id: order_no.clone(),
                    gateway_response: pending_gateway_response,
                    order_no: order_no.clone(),
                    product_id: plan.id.clone(),
                    product_snapshot: billing_plan_snapshot(&plan),
                    expires_at_unix_secs: expires_at.timestamp().max(0) as u64,
                },
            )
            .await
        {
            Ok(Some(value)) => value,
            Ok(None) => return billing_storage_unavailable_response(),
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("billing checkout create failed: {err:?}"),
                    false,
                )
            }
        };
        let pending_order = match outcome {
            aether_data::repository::wallet::CreatePlanPurchaseOrderOutcome::Created(order) => order,
            aether_data::repository::wallet::CreatePlanPurchaseOrderOutcome::WalletInactive => {
                return build_auth_error_response(
                    http::StatusCode::BAD_REQUEST,
                    "wallet is not active",
                    false,
                )
            }
            aether_data::repository::wallet::CreatePlanPurchaseOrderOutcome::OverlappingPlanExists => {
                return build_auth_error_response(
                    http::StatusCode::CONFLICT,
                    "当前已有生效套餐，只能续费原套餐",
                    false,
                )
            }
            aether_data::repository::wallet::CreatePlanPurchaseOrderOutcome::ActivePlanLimitReached => {
                return build_auth_error_response(
                    http::StatusCode::CONFLICT,
                    "套餐购买限制已达到上限",
                    false,
                )
            }
        };
        let Some(pay_amount) = pending_order
            .pay_amount
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            let _ = state
                .cancel_payment_order(CancelPaymentOrderInput {
                    order_no: pending_order.order_no.clone(),
                    expected_payment_provider: None,
                    cancel_reason: "invalid_plan_payment_amount".to_string(),
                    cancel_source: "server".to_string(),
                })
                .await;
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "套餐订单实付金额无效",
                false,
            );
        };
        let checkout = match create_dodopay_checkout(
            &config,
            &DodopayCheckoutInput {
                order_no: order_no.clone(),
                subject: plan.title.clone(),
                pay_amount,
                notify_url: format!("{callback_base_url}/api/payment/dodopay/notify"),
                return_url: dodopay_return_url(&config, &callback_base_url),
                cancel_base_url: callback_base_url.clone(),
                payment_channel: payment_channel.clone(),
                payer_name: Some(auth.user.username.clone()),
                metadata: json!({
                    "kind": "plan_purchase",
                    "plan_id": plan.id.clone(),
                    "user_id": auth.user.id.clone(),
                }),
            },
        )
        .await
        {
            Ok(value) => value,
            Err(detail) => {
                let _ = state
                    .cancel_payment_order(CancelPaymentOrderInput {
                        order_no: pending_order.order_no.clone(),
                        expected_payment_provider: None,
                        cancel_reason: "dodopay_checkout_create_failed".to_string(),
                        cancel_source: "server".to_string(),
                    })
                    .await;
                return build_auth_error_response(http::StatusCode::BAD_GATEWAY, detail, false);
            }
        };
        let mut gateway_response = checkout.payment_instructions.clone();
        exchange_rate.enrich_payload(&mut gateway_response);
        let updated_order = match state
            .update_pending_payment_order_gateway(UpdatePendingPaymentOrderGatewayInput {
                order_id: pending_order.id.clone(),
                gateway_order_id: checkout.gateway_order_id.clone(),
                gateway_response,
            })
            .await
        {
            Ok(Some(WalletMutationOutcome::Applied(order))) => order,
            Ok(Some(WalletMutationOutcome::NotFound)) | Ok(None) => {
                cancel_dodopay_checkout_after_local_failure(
                    &config,
                    &checkout,
                    "billing_order_not_found",
                )
                .await;
                let _ = state
                    .cancel_payment_order(CancelPaymentOrderInput {
                        order_no: pending_order.order_no.clone(),
                        expected_payment_provider: None,
                        cancel_reason: "dodopay_checkout_attach_failed".to_string(),
                        cancel_source: "server".to_string(),
                    })
                    .await;
                return billing_storage_unavailable_response();
            }
            Ok(Some(WalletMutationOutcome::Invalid(detail))) => {
                cancel_dodopay_checkout_after_local_failure(
                    &config,
                    &checkout,
                    "billing_order_invalid",
                )
                .await;
                let _ = state
                    .cancel_payment_order(CancelPaymentOrderInput {
                        order_no: pending_order.order_no.clone(),
                        expected_payment_provider: None,
                        cancel_reason: "dodopay_checkout_attach_failed".to_string(),
                        cancel_source: "server".to_string(),
                    })
                    .await;
                return build_auth_error_response(http::StatusCode::CONFLICT, detail, false);
            }
            Err(err) => {
                cancel_dodopay_checkout_after_local_failure(
                    &config,
                    &checkout,
                    "billing_checkout_attach_error",
                )
                .await;
                let _ = state
                    .cancel_payment_order(CancelPaymentOrderInput {
                        order_no: pending_order.order_no.clone(),
                        expected_payment_provider: None,
                        cancel_reason: "dodopay_checkout_attach_failed".to_string(),
                        cancel_source: "server".to_string(),
                    })
                    .await;
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("billing checkout attach failed: {err:?}"),
                    false,
                );
            }
        };
        return build_auth_json_response(
            http::StatusCode::OK,
            json!({
                "order": payment_order_payload(&updated_order, &plan),
                "payment_instructions": sanitize_wallet_gateway_response(Some(checkout.payment_instructions)),
            }),
            None,
        );
    }

    let config = match load_epay_config(state).await {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false)
        }
    };
    let resolved_exchange_rate = match resolve_payment_exchange_rate(
        state,
        &auth.user.id,
        &config.pay_currency,
        config.usd_exchange_rate,
    )
    .await
    {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::SERVICE_UNAVAILABLE, detail, false)
        }
    };
    let payment_channel =
        match resolve_epay_channel(&config, checkout_request.payment_channel.as_deref()) {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
            }
        };
    let (plan_amount_usd, plan_pay_amount) = match compute_plan_payment_amounts(
        &plan,
        &config.pay_currency,
        resolved_exchange_rate.rate,
    ) {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false)
        }
    };
    let Some(callback_base_url) = epay_callback_base_url(
        config.callback_base_url.as_deref(),
        headers,
        request_context,
    ) else {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "epay callback_base_url is required",
            false,
        );
    };
    let pay_currency = config.pay_currency.clone();
    let exchange_rate = resolved_exchange_rate.rate;
    let gateway_order_id = order_no.clone();
    let mut pending_gateway_response = json!({
        "gateway": "epay",
        "provider_order_status": "pending_checkout",
    });
    resolved_exchange_rate.enrich_payload(&mut pending_gateway_response);
    let outcome = match state
        .create_plan_purchase_order(
            aether_data::repository::wallet::CreatePlanPurchaseOrderInput {
                preferred_wallet_id: None,
                user_id: auth.user.id.clone(),
                amount_usd: plan_amount_usd,
                pay_amount: plan_pay_amount,
                pay_currency,
                exchange_rate,
                payment_method: checkout_request.payment_method.clone(),
                payment_provider: Some(checkout_request.payment_provider.clone()),
                payment_channel: Some(payment_channel.clone()),
                gateway_order_id,
                gateway_response: pending_gateway_response,
                order_no: order_no.clone(),
                product_id: plan.id.clone(),
                product_snapshot: billing_plan_snapshot(&plan),
                expires_at_unix_secs: expires_at.timestamp().max(0) as u64,
            },
        )
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return billing_storage_unavailable_response(),
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("billing checkout create failed: {err:?}"),
                false,
            )
        }
    };
    let order = match outcome {
        aether_data::repository::wallet::CreatePlanPurchaseOrderOutcome::Created(order) => order,
        aether_data::repository::wallet::CreatePlanPurchaseOrderOutcome::WalletInactive => {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "wallet is not active",
                false,
            )
        }
        aether_data::repository::wallet::CreatePlanPurchaseOrderOutcome::OverlappingPlanExists => {
            return build_auth_error_response(
                http::StatusCode::CONFLICT,
                "当前已有生效套餐，只能续费原套餐",
                false,
            )
        }
        aether_data::repository::wallet::CreatePlanPurchaseOrderOutcome::ActivePlanLimitReached => {
            return build_auth_error_response(
                http::StatusCode::CONFLICT,
                "套餐购买限制已达到上限",
                false,
            )
        }
    };
    let Some(pay_amount) = order
        .pay_amount
        .filter(|value| value.is_finite() && *value > 0.0)
    else {
        let _ = state
            .cancel_payment_order(CancelPaymentOrderInput {
                order_no: order.order_no.clone(),
                expected_payment_provider: None,
                cancel_reason: "invalid_plan_payment_amount".to_string(),
                cancel_source: "server".to_string(),
            })
            .await;
        return build_auth_error_response(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "套餐订单实付金额无效",
            false,
        );
    };
    let mut checkout = build_epay_checkout_url(
        &config,
        &EpayCheckoutInput {
            order_no: order_no.clone(),
            channel: payment_channel,
            subject: plan.title.clone(),
            pay_amount,
            notify_url: format!("{callback_base_url}/api/payment/epay/notify"),
            return_url: format!("{callback_base_url}/api/payment/epay/return"),
        },
    );
    resolved_exchange_rate.enrich_payload(&mut checkout);
    let updated_order = match state
        .update_pending_payment_order_gateway(UpdatePendingPaymentOrderGatewayInput {
            order_id: order.id.clone(),
            gateway_order_id: order_no,
            gateway_response: checkout.clone(),
        })
        .await
    {
        Ok(Some(WalletMutationOutcome::Applied(order))) => order,
        Ok(Some(WalletMutationOutcome::NotFound)) | Ok(None) => {
            let _ = state
                .cancel_payment_order(CancelPaymentOrderInput {
                    order_no: order.order_no.clone(),
                    expected_payment_provider: None,
                    cancel_reason: "epay_checkout_attach_failed".to_string(),
                    cancel_source: "server".to_string(),
                })
                .await;
            return billing_storage_unavailable_response();
        }
        Ok(Some(WalletMutationOutcome::Invalid(detail))) => {
            let _ = state
                .cancel_payment_order(CancelPaymentOrderInput {
                    order_no: order.order_no.clone(),
                    expected_payment_provider: None,
                    cancel_reason: "epay_checkout_attach_failed".to_string(),
                    cancel_source: "server".to_string(),
                })
                .await;
            return build_auth_error_response(http::StatusCode::CONFLICT, detail, false);
        }
        Err(err) => {
            let _ = state
                .cancel_payment_order(CancelPaymentOrderInput {
                    order_no: order.order_no.clone(),
                    expected_payment_provider: None,
                    cancel_reason: "epay_checkout_attach_failed".to_string(),
                    cancel_source: "server".to_string(),
                })
                .await;
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("billing checkout attach failed: {err:?}"),
                false,
            );
        }
    };
    build_auth_json_response(
        http::StatusCode::OK,
        json!({
            "order": payment_order_payload(&updated_order, &plan),
            "payment_instructions": sanitize_wallet_gateway_response(Some(checkout)),
        }),
        None,
    )
}

pub(super) async fn maybe_build_local_billing_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
    request_body: Option<&Bytes>,
) -> Option<Response<Body>> {
    let decision = request_context.control_decision.as_ref()?;
    if decision.route_family.as_deref() != Some("billing") {
        return None;
    }
    match decision.route_kind.as_deref() {
        Some("plans") if request_context.request_path == "/api/billing/plans" => {
            Some(handle_billing_plans_list(state).await)
        }
        Some("plan_checkout") => {
            Some(handle_billing_plan_checkout(state, request_context, headers, request_body).await)
        }
        Some("entitlements") if request_context.request_path == "/api/billing/entitlements" => {
            Some(handle_billing_entitlements(state, request_context, headers).await)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use aether_data_contracts::repository::billing::{
        BillingPlanRecord, UserPlanQuotaSummaryRecord,
    };
    use serde_json::json;

    fn billing_plan(price_amount: f64, price_currency: &str) -> BillingPlanRecord {
        BillingPlanRecord {
            id: "plan-test".to_string(),
            title: "测试套餐".to_string(),
            description: None,
            price_amount,
            price_currency: price_currency.to_string(),
            duration_unit: "month".to_string(),
            duration_value: 1,
            enabled: true,
            sort_order: 0,
            max_active_per_user: 1,
            purchase_limit_scope: "active_period".to_string(),
            allowed_provider_ids: vec!["provider-test".to_string()],
            entitlements_json: json!([
                {
                    "type": "daily_quota",
                    "daily_quota_usd": 1
                }
            ]),
            created_at_unix_secs: 0,
            updated_at_unix_secs: 0,
        }
    }

    #[test]
    fn dodopay_plan_amounts_respect_configured_pay_currency() {
        let usd_plan = billing_plan(10.0, "USD");
        assert_eq!(
            super::compute_dodopay_plan_payment_amounts(&usd_plan, "CNY", 7.2),
            Ok((10.0, 72.0))
        );

        let cny_plan = billing_plan(72.0, "CNY");
        assert_eq!(
            super::compute_dodopay_plan_payment_amounts(&cny_plan, "CNY", 7.2),
            Ok((10.0, 72.0))
        );
        assert_eq!(
            super::compute_dodopay_plan_payment_amounts(&cny_plan, "USD", 7.2),
            Err("套餐币种与支付网关币种不匹配")
        );
    }

    #[test]
    fn quota_summary_payload_includes_daily_refresh_and_remaining_quota() {
        let payload = super::quota_summary_payload(&UserPlanQuotaSummaryRecord {
            user_id: "user-1".to_string(),
            entitlement_id: "ent-1".to_string(),
            plan_id: "plan-1".to_string(),
            plan_title: "GPT Pro 月套餐".to_string(),
            starts_at_unix_secs: 1_755_283_329,
            expires_at_unix_secs: 1_757_875_329,
            quota_total_usd: 80.0,
            quota_used_usd: 20.0,
            quota_remaining_usd: 60.0,
            daily_total_usd: Some(80.0),
            daily_used_usd: Some(20.0),
            daily_remaining_usd: Some(60.0),
            daily_window_started_at_unix_secs: Some(1_755_283_329),
            daily_window_ends_at_unix_secs: Some(1_755_369_729),
        });

        assert_eq!(payload["plan_title"], "GPT Pro 月套餐");
        assert_eq!(payload["quota_remaining_usd"], 60.0);
        assert_eq!(payload["daily_remaining_usd"], 60.0);
        assert!(payload["daily_window_ends_at"].is_string());
    }
}

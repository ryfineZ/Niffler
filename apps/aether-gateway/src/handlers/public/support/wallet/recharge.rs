use super::super::support_payment::payment_dodopay::{
    cancel_dodopay_checkout_after_local_failure, configured_dodopay_channels,
    create_dodopay_checkout, dodopay_callback_base_url, dodopay_return_url, load_dodopay_config,
    normalize_dodopay_checkout_payment_channel, DodopayCheckoutInput,
};
use super::super::support_payment::payment_epay::{
    build_epay_checkout_url, configured_epay_channels, epay_callback_base_url, load_epay_config,
    resolve_epay_channel, EpayCheckoutInput,
};
use super::super::support_payment::payment_gateway::{
    CreateCheckoutSessionInput, PaymentGatewayRegistry,
};
use super::{
    build_auth_error_response, build_auth_json_response, build_wallet_payload,
    build_wallet_recharge_storage_unavailable_response, http, parse_wallet_limit,
    parse_wallet_offset, resolve_authenticated_local_user, resolve_payment_exchange_rate,
    unix_secs_to_rfc3339, wallet_normalize_optional_string_field, AppState, Body,
    GatewayPublicRequestContext, Response, WALLET_SAFE_GATEWAY_RESPONSE_KEYS,
};
#[cfg(test)]
use super::{
    record_wallet_test_recharge, wallet_test_recharge_order_by_id,
    wallet_test_recharge_orders_for_user,
};
use aether_data::repository::wallet::{
    CancelPaymentOrderInput, UpdatePendingPaymentOrderGatewayInput, WalletMutationOutcome,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct WalletCreateRechargeRequest {
    amount_usd: f64,
    payment_method: String,
    #[serde(default)]
    payment_provider: Option<String>,
    #[serde(default)]
    payment_channel: Option<String>,
    #[serde(default)]
    pay_amount: Option<f64>,
    #[serde(default)]
    pay_currency: Option<String>,
    #[serde(default)]
    exchange_rate: Option<f64>,
}

#[derive(Debug, Clone)]
struct NormalizedWalletCreateRechargeRequest {
    amount_usd: f64,
    payment_method: String,
    payment_provider: Option<String>,
    payment_channel: Option<String>,
    pay_amount: Option<f64>,
    pay_currency: Option<String>,
    exchange_rate: Option<f64>,
}

fn normalize_wallet_create_recharge_request(
    payload: WalletCreateRechargeRequest,
) -> Result<NormalizedWalletCreateRechargeRequest, &'static str> {
    if !payload.amount_usd.is_finite() || payload.amount_usd <= 0.0 {
        return Err("输入验证失败");
    }
    let payment_method = payload.payment_method.trim().to_ascii_lowercase();
    if payment_method.is_empty() || payment_method.chars().count() > 30 {
        return Err("输入验证失败");
    }
    let payment_provider = wallet_normalize_optional_string_field(payload.payment_provider, 30)?
        .map(|value| value.to_ascii_lowercase());
    let payment_channel = wallet_normalize_optional_string_field(payload.payment_channel, 30)?
        .map(|value| value.to_ascii_lowercase());
    if matches!(payload.pay_amount, Some(value) if !value.is_finite() || value <= 0.0) {
        return Err("输入验证失败");
    }
    if matches!(payload.exchange_rate, Some(value) if !value.is_finite() || value <= 0.0) {
        return Err("输入验证失败");
    }
    let pay_currency = wallet_normalize_optional_string_field(payload.pay_currency, 3)?;
    if matches!(pay_currency.as_deref(), Some(value) if value.chars().count() != 3) {
        return Err("输入验证失败");
    }

    Ok(NormalizedWalletCreateRechargeRequest {
        amount_usd: payload.amount_usd,
        payment_method,
        payment_provider,
        payment_channel,
        pay_amount: payload.pay_amount,
        pay_currency,
        exchange_rate: payload.exchange_rate,
    })
}

fn wallet_build_order_no(now: chrono::DateTime<chrono::Utc>) -> String {
    format!(
        "po_{}_{}",
        now.format("%Y%m%d%H%M%S%6f"),
        &Uuid::new_v4().simple().to_string()[..12]
    )
}

fn wallet_order_id_from_path(request_path: &str) -> Option<String> {
    let trimmed = request_path.trim_end_matches('/');
    let order_id = trimmed.strip_prefix("/api/wallet/recharge/")?.trim();
    if order_id.is_empty() || order_id.contains('/') {
        None
    } else {
        Some(order_id.to_string())
    }
}

pub(super) fn wallet_recharge_detail_path_matches(request_path: &str) -> bool {
    wallet_order_id_from_path(request_path).is_some()
}

pub(crate) fn sanitize_wallet_gateway_response(
    value: Option<serde_json::Value>,
) -> serde_json::Value {
    let Some(value) = value else {
        return json!({});
    };
    let Some(object) = value.as_object() else {
        return json!({});
    };
    let mut sanitized = serde_json::Map::new();
    for key in WALLET_SAFE_GATEWAY_RESPONSE_KEYS {
        if let Some(item) = object.get(*key) {
            sanitized.insert((*key).to_string(), item.clone());
        }
    }
    serde_json::Value::Object(sanitized)
}

fn build_wallet_payment_order_payload(
    id: String,
    order_no: String,
    wallet_id: String,
    user_id: Option<String>,
    amount_usd: f64,
    pay_amount: Option<f64>,
    pay_currency: Option<String>,
    exchange_rate: Option<f64>,
    refunded_amount_usd: f64,
    refundable_amount_usd: f64,
    payment_method: String,
    payment_provider: Option<String>,
    payment_channel: Option<String>,
    order_kind: String,
    product_id: Option<String>,
    product_snapshot: Option<serde_json::Value>,
    fulfillment_status: Option<String>,
    fulfillment_error: Option<String>,
    gateway_order_id: Option<String>,
    gateway_response: Option<serde_json::Value>,
    status: String,
    created_at: Option<String>,
    paid_at: Option<String>,
    credited_at: Option<String>,
    expires_at: Option<String>,
) -> serde_json::Value {
    json!({
        "id": id,
        "order_no": order_no,
        "wallet_id": wallet_id,
        "user_id": user_id,
        "amount_usd": amount_usd,
        "pay_amount": pay_amount,
        "pay_currency": pay_currency,
        "exchange_rate": exchange_rate,
        "refunded_amount_usd": refunded_amount_usd,
        "refundable_amount_usd": refundable_amount_usd,
        "payment_method": payment_method,
        "payment_provider": payment_provider,
        "payment_channel": payment_channel,
        "order_kind": order_kind,
        "product_id": product_id,
        "product_snapshot": product_snapshot,
        "fulfillment_status": fulfillment_status,
        "fulfillment_error": fulfillment_error,
        "gateway_order_id": gateway_order_id,
        "gateway_response": sanitize_wallet_gateway_response(gateway_response),
        "status": status,
        "created_at": created_at,
        "paid_at": paid_at,
        "credited_at": credited_at,
        "expires_at": expires_at,
    })
}

fn wallet_payment_order_payload_from_record(
    record: &aether_data::repository::wallet::StoredAdminPaymentOrder,
) -> serde_json::Value {
    build_wallet_payment_order_payload(
        record.id.clone(),
        record.order_no.clone(),
        record.wallet_id.clone(),
        record.user_id.clone(),
        record.amount_usd,
        record.pay_amount,
        record.pay_currency.clone(),
        record.exchange_rate,
        record.refunded_amount_usd,
        record.refundable_amount_usd,
        record.payment_method.clone(),
        record.payment_provider.clone(),
        record.payment_channel.clone(),
        record.order_kind.clone(),
        record.product_id.clone(),
        record.product_snapshot.clone(),
        record.fulfillment_status.clone(),
        record.fulfillment_error.clone(),
        record.gateway_order_id.clone(),
        record.gateway_response.clone(),
        record.status.clone(),
        Some(unix_secs_to_rfc3339(record.created_at_unix_ms)).flatten(),
        record.paid_at_unix_secs.and_then(unix_secs_to_rfc3339),
        record.credited_at_unix_secs.and_then(unix_secs_to_rfc3339),
        record.expires_at_unix_secs.and_then(unix_secs_to_rfc3339),
    )
}

pub(super) async fn handle_wallet_create_recharge(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
    request_body: Option<&axum::body::Bytes>,
) -> Response<Body> {
    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(request_body) = request_body else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "缺少请求体", false);
    };
    let payload = match serde_json::from_slice::<WalletCreateRechargeRequest>(request_body) {
        Ok(value) => value,
        Err(_) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, "输入验证失败", false)
        }
    };
    let payload = match normalize_wallet_create_recharge_request(payload) {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false)
        }
    };
    if payload.payment_method == "admin_manual" {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "admin_manual is reserved for admin recharge",
            false,
        );
    }

    let wallet = match state
        .find_wallet(aether_data::repository::wallet::WalletLookupKey::UserId(
            &auth.user.id,
        ))
        .await
    {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("wallet lookup failed: {err:?}"),
                false,
            )
        }
    };

    if !state.has_database_wallet_data_writer() {
        #[cfg(test)]
        {
            let Some(wallet) = wallet else {
                return build_auth_error_response(
                    http::StatusCode::BAD_REQUEST,
                    "wallet not available",
                    false,
                );
            };
            if wallet.status != "active" {
                return build_auth_error_response(
                    http::StatusCode::BAD_REQUEST,
                    "wallet is not active",
                    false,
                );
            }
            let now = Utc::now();
            let order_id = Uuid::new_v4().to_string();
            let order_no = wallet_build_order_no(now);
            let expires_at = now + chrono::Duration::minutes(30);
            let Some(adapter) = PaymentGatewayRegistry::get(&payload.payment_method) else {
                return build_auth_error_response(
                    http::StatusCode::BAD_REQUEST,
                    format!("unsupported payment_method: {}", payload.payment_method),
                    false,
                );
            };
            let checkout = match adapter.create_checkout_session(&CreateCheckoutSessionInput {
                order_no: order_no.clone(),
                amount_usd: payload.amount_usd,
                expires_at,
            }) {
                Ok(value) => value,
                Err(detail) => {
                    return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
                }
            };
            let order_payload = build_wallet_payment_order_payload(
                order_id,
                order_no,
                wallet.id.clone(),
                Some(auth.user.id.clone()),
                payload.amount_usd,
                payload.pay_amount,
                payload.pay_currency.clone(),
                payload.exchange_rate,
                0.0,
                0.0,
                payload.payment_method,
                payload.payment_provider,
                payload.payment_channel,
                "wallet_recharge".to_string(),
                None,
                None,
                Some("pending".to_string()),
                None,
                Some(checkout.gateway_order_id.clone()),
                Some(checkout.gateway_response.clone()),
                "pending".to_string(),
                Some(now.to_rfc3339()),
                None,
                None,
                Some(expires_at.to_rfc3339()),
            );
            record_wallet_test_recharge(auth.user.id, order_payload.clone());
            return build_auth_json_response(
                http::StatusCode::OK,
                json!({
                    "order": order_payload,
                    "payment_instructions": sanitize_wallet_gateway_response(Some(checkout.gateway_response)),
                }),
                None,
            );
        }
        #[cfg(not(test))]
        return build_wallet_recharge_storage_unavailable_response();
    }

    let now = Utc::now();
    let order_no = wallet_build_order_no(now);
    let expires_at = now + chrono::Duration::minutes(30);
    let uses_epay =
        payload.payment_provider.as_deref() == Some("epay") || payload.payment_method == "epay";
    let uses_dodopay = payload.payment_provider.as_deref() == Some("dodopay")
        || payload.payment_method == "dodopay";
    if uses_epay {
        let config = match load_epay_config(state).await {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
            }
        };
        if payload.amount_usd < config.min_recharge_usd {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "充值金额低于支付网关最小金额",
                false,
            );
        }
        let requested_channel = payload.payment_channel.as_deref().or_else(|| {
            (payload.payment_method != "epay").then_some(payload.payment_method.as_str())
        });
        let payment_channel = match resolve_epay_channel(&config, requested_channel) {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
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
        let pay_amount = (payload.amount_usd * exchange_rate.rate * 100.0).round() / 100.0;
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
        let mut checkout = build_epay_checkout_url(
            &config,
            &EpayCheckoutInput {
                order_no: order_no.clone(),
                channel: payment_channel.clone(),
                subject: "钱包充值".to_string(),
                pay_amount,
                notify_url: format!("{callback_base_url}/api/payment/epay/notify"),
                return_url: format!("{callback_base_url}/api/payment/epay/return"),
            },
        );
        exchange_rate.enrich_payload(&mut checkout);
        let outcome = match state
            .create_wallet_recharge_order(
                aether_data::repository::wallet::CreateWalletRechargeOrderInput {
                    preferred_wallet_id: wallet.as_ref().map(|value| value.id.clone()),
                    user_id: auth.user.id.clone(),
                    amount_usd: payload.amount_usd,
                    pay_amount: Some(pay_amount),
                    pay_currency: Some(config.pay_currency.clone()),
                    exchange_rate: Some(exchange_rate.rate),
                    payment_method: "epay".to_string(),
                    payment_provider: Some("epay".to_string()),
                    payment_channel: Some(payment_channel),
                    gateway_order_id: order_no.clone(),
                    gateway_response: checkout.clone(),
                    order_no,
                    expires_at_unix_secs: expires_at.timestamp().max(0) as u64,
                },
            )
            .await
        {
            Ok(Some(value)) => value,
            Ok(None) => return build_wallet_recharge_storage_unavailable_response(),
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("wallet recharge create failed: {err:?}"),
                    false,
                )
            }
        };
        let order_payload = match outcome {
            aether_data::repository::wallet::CreateWalletRechargeOrderOutcome::Created(order) => {
                wallet_payment_order_payload_from_record(&order)
            }
            aether_data::repository::wallet::CreateWalletRechargeOrderOutcome::WalletInactive => {
                return build_auth_error_response(
                    http::StatusCode::BAD_REQUEST,
                    "wallet is not active",
                    false,
                )
            }
        };
        return build_auth_json_response(
            http::StatusCode::OK,
            json!({
                "order": order_payload,
                "payment_instructions": sanitize_wallet_gateway_response(Some(checkout)),
            }),
            None,
        );
    }
    if uses_dodopay {
        let config = match load_dodopay_config(state).await {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
            }
        };
        if payload.amount_usd < config.min_recharge_usd {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "充值金额低于支付网关最小金额",
                false,
            );
        }
        let requested_channel = payload.payment_channel.as_deref().or_else(|| {
            (payload.payment_method != "dodopay").then_some(payload.payment_method.as_str())
        });
        let payment_channel = match normalize_dodopay_checkout_payment_channel(requested_channel) {
            Ok(value) => value,
            Err(detail) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
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
        let pay_amount = (payload.amount_usd * exchange_rate.rate * 100.0).round() / 100.0;
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
        let outcome = match state
            .create_wallet_recharge_order(
                aether_data::repository::wallet::CreateWalletRechargeOrderInput {
                    preferred_wallet_id: wallet.as_ref().map(|value| value.id.clone()),
                    user_id: auth.user.id.clone(),
                    amount_usd: payload.amount_usd,
                    pay_amount: Some(pay_amount),
                    pay_currency: Some(config.pay_currency.clone()),
                    exchange_rate: Some(exchange_rate.rate),
                    payment_method: "dodopay".to_string(),
                    payment_provider: Some("dodopay".to_string()),
                    payment_channel: Some(payment_channel.clone()),
                    gateway_order_id: order_no.clone(),
                    gateway_response: {
                        let mut payload = json!({
                        "gateway": "dodopay",
                        "provider_order_status": "pending_checkout",
                        });
                        exchange_rate.enrich_payload(&mut payload);
                        payload
                    },
                    order_no: order_no.clone(),
                    expires_at_unix_secs: expires_at.timestamp().max(0) as u64,
                },
            )
            .await
        {
            Ok(Some(value)) => value,
            Ok(None) => return build_wallet_recharge_storage_unavailable_response(),
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("wallet recharge create failed: {err:?}"),
                    false,
                )
            }
        };
        let pending_order = match outcome {
            aether_data::repository::wallet::CreateWalletRechargeOrderOutcome::Created(order) => {
                order
            }
            aether_data::repository::wallet::CreateWalletRechargeOrderOutcome::WalletInactive => {
                return build_auth_error_response(
                    http::StatusCode::BAD_REQUEST,
                    "wallet is not active",
                    false,
                )
            }
        };
        let checkout = match create_dodopay_checkout(
            &config,
            &DodopayCheckoutInput {
                order_no: order_no.clone(),
                subject: "钱包充值".to_string(),
                pay_amount,
                notify_url: format!("{callback_base_url}/api/payment/dodopay/notify"),
                return_url: dodopay_return_url(&config, &callback_base_url),
                cancel_base_url: callback_base_url.clone(),
                payment_channel: payment_channel.clone(),
                payer_name: Some(auth.user.username.clone()),
                metadata: json!({
                    "kind": "wallet_recharge",
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
                    "wallet_recharge_order_not_found",
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
                return build_wallet_recharge_storage_unavailable_response();
            }
            Ok(Some(WalletMutationOutcome::Invalid(detail))) => {
                cancel_dodopay_checkout_after_local_failure(
                    &config,
                    &checkout,
                    "wallet_recharge_order_invalid",
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
                    "wallet_recharge_attach_error",
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
                    format!("wallet recharge checkout attach failed: {err:?}"),
                    false,
                );
            }
        };
        let order_payload = wallet_payment_order_payload_from_record(&updated_order);
        return build_auth_json_response(
            http::StatusCode::OK,
            json!({
                "order": order_payload,
                "payment_instructions": sanitize_wallet_gateway_response(Some(checkout.payment_instructions)),
            }),
            None,
        );
    }
    build_auth_error_response(
        http::StatusCode::BAD_REQUEST,
        format!("unsupported payment_method: {}", payload.payment_method),
        false,
    )
}

pub(super) async fn handle_wallet_recharge_options(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let mut methods = Vec::new();
    let mut exchange_rate_error = None;
    if let Ok(config) = load_epay_config(state).await {
        match resolve_payment_exchange_rate(
            state,
            &auth.user.id,
            &config.pay_currency,
            config.usd_exchange_rate,
        )
        .await
        {
            Ok(exchange_rate) => {
                for channel in configured_epay_channels(&config) {
                    let mut option = json!({
                        "payment_method": "epay",
                        "payment_provider": "epay",
                        "payment_channel": channel.channel,
                        "display_name": channel.display_name,
                        "pay_currency": config.pay_currency,
                        "min_recharge_usd": config.min_recharge_usd,
                    });
                    exchange_rate.enrich_payload(&mut option);
                    methods.push(option);
                }
            }
            Err(detail) => exchange_rate_error = Some(detail),
        }
    }
    if let Ok(config) = load_dodopay_config(state).await {
        match resolve_payment_exchange_rate(
            state,
            &auth.user.id,
            &config.pay_currency,
            config.usd_exchange_rate,
        )
        .await
        {
            Ok(exchange_rate) => {
                for channel in configured_dodopay_channels() {
                    let mut option = json!({
                        "payment_method": "dodopay",
                        "payment_provider": "dodopay",
                        "payment_channel": channel.channel,
                        "display_name": format!("DoDoPay · {}", channel.display_name),
                        "pay_currency": config.pay_currency,
                        "min_recharge_usd": config.min_recharge_usd,
                    });
                    exchange_rate.enrich_payload(&mut option);
                    methods.push(option);
                }
            }
            Err(detail) => exchange_rate_error = Some(detail),
        }
    }
    if methods.is_empty() {
        if let Some(detail) = exchange_rate_error {
            return build_auth_error_response(http::StatusCode::SERVICE_UNAVAILABLE, detail, false);
        }
    }
    build_auth_json_response(http::StatusCode::OK, json!({ "items": methods }), None)
}

pub(super) async fn handle_wallet_recharge_list(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let query = request_context.request_query_string.as_deref();
    let limit = match parse_wallet_limit(query) {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false)
        }
    };
    let offset = match parse_wallet_offset(query) {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false)
        }
    };
    let wallet = match state
        .find_wallet(aether_data::repository::wallet::WalletLookupKey::UserId(
            &auth.user.id,
        ))
        .await
    {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("wallet lookup failed: {err:?}"),
                false,
            )
        }
    };

    let (items, total) = match state
        .list_wallet_payment_orders_by_user_id(&auth.user.id, limit, offset)
        .await
    {
        Ok(page) => (
            page.items
                .iter()
                .map(wallet_payment_order_payload_from_record)
                .collect::<Vec<_>>(),
            page.total,
        ),
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("wallet recharge lookup failed: {err:?}"),
                false,
            )
        }
    };
    #[cfg(test)]
    let (items, total) =
        if !state.has_database_wallet_data_writer() && items.is_empty() && total == 0 {
            wallet_test_recharge_orders_for_user(&auth.user.id, limit, offset)
        } else {
            (items, total)
        };

    let mut payload = json!({
        "items": items,
        "total": total,
        "limit": limit,
        "offset": offset,
    });
    if let Some(object) = payload.as_object_mut() {
        if let Some(wallet_payload) = build_wallet_payload(wallet.as_ref()).as_object() {
            object.extend(wallet_payload.clone());
        }
    }
    build_auth_json_response(http::StatusCode::OK, payload, None)
}

pub(super) async fn handle_wallet_recharge_detail(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(order_id) = wallet_order_id_from_path(&request_context.request_path) else {
        return build_auth_error_response(
            http::StatusCode::NOT_FOUND,
            "Payment order not found",
            false,
        );
    };
    match state
        .find_wallet_payment_order_by_user_id(&auth.user.id, &order_id)
        .await
    {
        Ok(Some(order)) => build_auth_json_response(
            http::StatusCode::OK,
            json!({ "order": wallet_payment_order_payload_from_record(&order) }),
            None,
        ),
        Ok(None) => {
            #[cfg(test)]
            if let Some(order) = wallet_test_recharge_order_by_id(&auth.user.id, &order_id) {
                return build_auth_json_response(
                    http::StatusCode::OK,
                    json!({ "order": order }),
                    None,
                );
            }
            build_auth_error_response(
                http::StatusCode::NOT_FOUND,
                "Payment order not found",
                false,
            )
        }
        Err(err) => build_auth_error_response(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("wallet recharge detail lookup failed: {err:?}"),
            false,
        ),
    }
}

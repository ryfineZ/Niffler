use axum::{body::Body, http, response::Response};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use tracing::warn;
use uuid::Uuid;

use super::{payment_shared::payment_callback_payload_hash, AppState, GatewayPublicRequestContext};

#[derive(Debug, Clone)]
pub(crate) struct DodopayConfig {
    pub(crate) base_url: String,
    pub(crate) app_id: String,
    pub(crate) app_secret: String,
    pub(crate) callback_base_url: Option<String>,
    pub(crate) return_path: String,
    pub(crate) pay_currency: String,
    pub(crate) usd_exchange_rate: f64,
    pub(crate) min_recharge_usd: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct DodopayCheckoutInput {
    pub(crate) order_no: String,
    pub(crate) subject: String,
    pub(crate) pay_amount: f64,
    pub(crate) notify_url: String,
    pub(crate) return_url: String,
    pub(crate) cancel_base_url: String,
    pub(crate) payment_channel: String,
    pub(crate) payer_name: Option<String>,
    pub(crate) metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub(crate) struct DodopayCheckoutOutput {
    pub(crate) gateway_order_id: String,
    pub(crate) pay_amount: f64,
    pub(crate) payment_instructions: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct DodopayCreateOrderResponse {
    order_id: String,
    #[serde(default)]
    payable_amount: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    checkout_url: String,
    #[serde(default)]
    selected_channel: Option<String>,
    #[serde(default)]
    confirmed_channel: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DodopayPaymentChannel {
    pub(crate) channel: &'static str,
    pub(crate) display_name: &'static str,
}

fn normalize_base_url(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches('/');
    let parsed = url::Url::parse(trimmed).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return None;
    }
    Some(trimmed.to_string())
}

fn forwarded_header_first(value: String) -> Option<String> {
    value
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) async fn load_dodopay_config(state: &AppState) -> Result<DodopayConfig, String> {
    let Some(record) = state
        .find_payment_gateway_config("dodopay")
        .await
        .map_err(|err| format!("dodopay config lookup failed: {err:?}"))?
    else {
        return Err("DoDoPay 未配置".to_string());
    };
    if !record.enabled {
        return Err("DoDoPay 未启用".to_string());
    }
    let Some(encrypted_secret) = record.merchant_key_encrypted.as_deref() else {
        return Err("DoDoPay App Secret 未配置".to_string());
    };
    let Some(app_secret) = crate::handlers::shared::decrypt_catalog_secret_with_fallbacks(
        state.encryption_key(),
        encrypted_secret,
    ) else {
        return Err("DoDoPay App Secret 解密失败".to_string());
    };
    let app_id = record.merchant_id.trim();
    if app_id.is_empty() {
        return Err("DoDoPay App ID 未配置".to_string());
    }
    let Some(base_url) = normalize_base_url(&record.endpoint_url) else {
        return Err("DoDoPay 服务地址必须是 http(s) 绝对地址".to_string());
    };
    let callback_base_url = record.callback_base_url;
    if let Some(value) = callback_base_url.as_deref() {
        if normalize_base_url(value).is_none() {
            return Err("DoDoPay 回调站点根地址必须是 http(s) 绝对地址".to_string());
        }
    }
    Ok(DodopayConfig {
        base_url,
        app_id: app_id.to_string(),
        app_secret,
        callback_base_url,
        return_path: "/dashboard/wallet".to_string(),
        pay_currency: record.pay_currency,
        usd_exchange_rate: record.usd_exchange_rate,
        min_recharge_usd: record.min_recharge_usd,
    })
}

pub(crate) fn dodopay_callback_base_url(
    configured: Option<&str>,
    headers: &http::HeaderMap,
    request_context: &GatewayPublicRequestContext,
) -> Option<String> {
    if let Some(value) = configured.and_then(normalize_base_url) {
        return Some(value);
    }

    if let Some(value) = std::env::var("AETHER_PUBLIC_BASE_URL")
        .ok()
        .or_else(|| std::env::var("PUBLIC_BASE_URL").ok())
        .and_then(|value| normalize_base_url(&value))
    {
        return Some(value);
    }

    let host = crate::headers::header_value_str(headers, crate::constants::FORWARDED_HOST_HEADER)
        .and_then(forwarded_header_first)
        .or_else(|| request_context.host_header.clone())
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| {
            !value.is_empty()
                && !value.contains('/')
                && !value.contains('\\')
                && !value.contains('@')
                && !value.contains(char::is_whitespace)
        })?;
    let proto = crate::headers::header_value_str(headers, crate::constants::FORWARDED_PROTO_HEADER)
        .and_then(forwarded_header_first)
        .map(|value| value.trim().trim_end_matches(':').to_ascii_lowercase())
        .filter(|value| value == "http" || value == "https")
        .unwrap_or_else(|| "http".to_string());
    normalize_base_url(&format!("{proto}://{host}"))
}

fn normalize_return_path(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

pub(crate) fn dodopay_return_url(config: &DodopayConfig, callback_base_url: &str) -> String {
    format!(
        "{}{}",
        callback_base_url.trim_end_matches('/'),
        normalize_return_path(&config.return_path)
    )
}

fn dodopay_cancel_token(secret: &str, order_no: &str, gateway_order_id: Option<&str>) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.trim().as_bytes()).expect("hmac key should work");
    mac.update(b"dodopay.cancel.");
    mac.update(order_no.as_bytes());
    if let Some(gateway_order_id) = gateway_order_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        mac.update(b".");
        mac.update(gateway_order_id.as_bytes());
    }
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn dodopay_verify_cancel_token(
    secret: &str,
    order_no: &str,
    gateway_order_id: Option<&str>,
    token: &str,
) -> bool {
    let expected = dodopay_cancel_token(secret, order_no, gateway_order_id);
    dodopay_timing_safe_equal(token.trim(), &expected)
}

pub(crate) fn dodopay_cancel_url(
    callback_base_url: &str,
    order_no: &str,
    gateway_order_id: Option<&str>,
    signing_secret: &str,
) -> String {
    let encoded_order_no =
        url::form_urlencoded::byte_serialize(order_no.as_bytes()).collect::<String>();
    let gateway_order_id = gateway_order_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let token = dodopay_cancel_token(signing_secret, order_no, gateway_order_id);
    let encoded_token = url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>();
    let mut url = format!(
        "{}/api/payment/dodopay/cancel?order_no={encoded_order_no}&token={encoded_token}",
        callback_base_url.trim_end_matches('/')
    );
    if let Some(gateway_order_id) = gateway_order_id {
        let encoded_gateway_order_id =
            url::form_urlencoded::byte_serialize(gateway_order_id.as_bytes()).collect::<String>();
        url.push_str("&gateway_order_id=");
        url.push_str(&encoded_gateway_order_id);
    }
    url
}

pub(crate) fn configured_dodopay_channels() -> [DodopayPaymentChannel; 1] {
    [DodopayPaymentChannel {
        channel: "we_chat_pay",
        display_name: "微信支付",
    }]
}

pub(crate) fn normalize_dodopay_payment_channel(value: Option<&str>) -> Result<String, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err("请选择 DoDoPay 支付方式".to_string());
    };
    let normalized = value
        .chars()
        .filter(|ch| *ch != '-' && *ch != '_' && !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "alipay" | "ali" => Ok("ali_pay".to_string()),
        "wechatpay" | "wechat" | "weixin" | "wxpay" | "wx" => Ok("we_chat_pay".to_string()),
        _ => Err("DoDoPay 只支持选择支付宝或微信支付".to_string()),
    }
}

pub(crate) fn normalize_dodopay_checkout_payment_channel(
    value: Option<&str>,
) -> Result<String, String> {
    let channel = normalize_dodopay_payment_channel(value)?;
    if channel == "we_chat_pay" {
        return Ok(channel);
    }
    Err("DoDoPay 当前只支持微信支付".to_string())
}

fn dodopay_payment_channel_display_name(channel: &str) -> &'static str {
    configured_dodopay_channels()
        .into_iter()
        .find(|item| item.channel == channel)
        .map(|item| item.display_name)
        .unwrap_or("DoDoPay")
}

fn dodopay_canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut items = map.iter().collect::<Vec<_>>();
            items.sort_by(|left, right| left.0.cmp(right.0));
            let mut object = serde_json::Map::new();
            for (key, value) in items {
                object.insert(key.clone(), dodopay_canonicalize_json(value));
            }
            serde_json::Value::Object(object)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(dodopay_canonicalize_json).collect())
        }
        _ => value.clone(),
    }
}

fn dodopay_unsigned_payload(payload: &serde_json::Value) -> serde_json::Value {
    let mut unsigned = payload.clone();
    if let serde_json::Value::Object(object) = &mut unsigned {
        object.remove("signature");
    }
    unsigned
}

pub(crate) fn dodopay_sign_payload(
    app_secret: &str,
    payload: &serde_json::Value,
) -> Result<String, String> {
    let canonical = dodopay_canonicalize_json(&dodopay_unsigned_payload(payload));
    let encoded = serde_json::to_string(&canonical)
        .map_err(|err| format!("dodopay payload encode failed: {err}"))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(app_secret.as_bytes())
        .map_err(|err| format!("dodopay hmac init failed: {err}"))?;
    mac.update(encoded.as_bytes());
    let bytes = mac.finalize().into_bytes();
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn dodopay_timing_safe_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .fold(0u8, |acc, (left, right)| acc | (left ^ right))
            == 0
}

pub(crate) fn dodopay_verify_payload_signature(
    app_secret: &str,
    payload: &serde_json::Value,
) -> Result<bool, String> {
    let provided = payload
        .get("signature")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase();
    if provided.is_empty() {
        return Ok(false);
    }
    let expected = dodopay_sign_payload(app_secret, payload)?;
    Ok(dodopay_timing_safe_equal(&provided, &expected))
}

fn dodopay_checkout_url(config: &DodopayConfig) -> String {
    format!("{}/api/v1/orders", config.base_url.trim_end_matches('/'))
}

fn dodopay_channel_url(config: &DodopayConfig, order_id: &str) -> String {
    let encoded_order_id =
        url::form_urlencoded::byte_serialize(order_id.as_bytes()).collect::<String>();
    format!(
        "{}/api/v1/orders/{encoded_order_id}/channel",
        config.base_url.trim_end_matches('/')
    )
}

fn dodopay_cancel_order_url(config: &DodopayConfig, order_id: &str) -> String {
    let encoded_order_id =
        url::form_urlencoded::byte_serialize(order_id.as_bytes()).collect::<String>();
    format!(
        "{}/api/v1/orders/{encoded_order_id}/cancel",
        config.base_url.trim_end_matches('/')
    )
}

fn dodopay_provider_channel(channel: &str) -> Result<&'static str, String> {
    match normalize_dodopay_checkout_payment_channel(Some(channel))?.as_str() {
        "we_chat_pay" => Ok("WECHAT"),
        _ => Err("DoDoPay 当前只支持微信支付".to_string()),
    }
}

fn dodopay_format_amount(amount: f64) -> String {
    format!("{:.2}", (amount * 100.0).round() / 100.0)
}

fn dodopay_optional_text(value: Option<&str>, max_chars: usize) -> Option<String> {
    let normalized = value?.trim().chars().take(max_chars).collect::<String>();
    (!normalized.is_empty()).then_some(normalized)
}

fn dodopay_parse_decimal_amount(value: Option<&str>) -> Option<f64> {
    let amount = value?.trim().parse::<f64>().ok()?;
    amount
        .is_finite()
        .then_some(amount)
        .filter(|value| *value > 0.0)
}

fn dodopay_signed_body(
    app_id: &str,
    app_secret: &str,
    mut body: serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, String> {
    body.insert(
        "app_id".to_string(),
        serde_json::Value::String(app_id.to_string()),
    );
    body.insert(
        "nonce".to_string(),
        serde_json::Value::String(Uuid::new_v4().simple().to_string()),
    );
    body.insert(
        "timestamp".to_string(),
        serde_json::Value::Number(serde_json::Number::from(Utc::now().timestamp())),
    );
    let mut value = serde_json::Value::Object(body);
    let signature = dodopay_sign_payload(app_secret, &value)?;
    if let serde_json::Value::Object(map) = &mut value {
        map.insert(
            "signature".to_string(),
            serde_json::Value::String(signature),
        );
    }
    Ok(value)
}

pub(crate) async fn cancel_dodopay_remote_order(
    config: &DodopayConfig,
    order_id: &str,
) -> Result<(), String> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Ok(());
    }
    let mut unsigned = serde_json::Map::new();
    unsigned.insert(
        "order_id".to_string(),
        serde_json::Value::String(order_id.to_string()),
    );
    let body = dodopay_signed_body(&config.app_id, &config.app_secret, unsigned)?;
    let response = reqwest::Client::new()
        .post(dodopay_cancel_order_url(config, order_id))
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("dodopay cancel order failed: {err}"))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let response_text = response.text().await.unwrap_or_else(|_| String::new());
    Err(format!(
        "dodopay cancel order returned {status}: {response_text}"
    ))
}

pub(crate) async fn cancel_dodopay_checkout_after_local_failure(
    config: &DodopayConfig,
    checkout: &DodopayCheckoutOutput,
    reason: &str,
) {
    if let Err(err) = cancel_dodopay_remote_order(config, &checkout.gateway_order_id).await {
        warn!(
            error = %err,
            reason,
            "failed to cancel dodopay order after local checkout failure"
        );
    }
}

pub(crate) async fn create_dodopay_checkout(
    config: &DodopayConfig,
    input: &DodopayCheckoutInput,
) -> Result<DodopayCheckoutOutput, String> {
    if !input.pay_amount.is_finite() || input.pay_amount <= 0.0 {
        return Err("dodopay amount is invalid".to_string());
    }
    let provider_channel = dodopay_provider_channel(&input.payment_channel)?;
    let mut metadata = match input.metadata.clone() {
        serde_json::Value::Object(map) => map,
        value if value.is_null() => serde_json::Map::new(),
        value => {
            let mut map = serde_json::Map::new();
            map.insert("raw_metadata".to_string(), value);
            map
        }
    };
    metadata.insert(
        "order_no".to_string(),
        serde_json::Value::String(input.order_no.clone()),
    );
    metadata.insert(
        "subject".to_string(),
        serde_json::Value::String(input.subject.clone()),
    );
    let mut unsigned = serde_json::Map::new();
    unsigned.insert(
        "merchant_order_id".to_string(),
        serde_json::Value::String(input.order_no.clone()),
    );
    unsigned.insert(
        "amount".to_string(),
        serde_json::Value::String(dodopay_format_amount(input.pay_amount)),
    );
    unsigned.insert(
        "subject".to_string(),
        serde_json::Value::String(input.subject.clone()),
    );
    unsigned.insert(
        "notify_url".to_string(),
        serde_json::Value::String(input.notify_url.clone()),
    );
    unsigned.insert(
        "return_url".to_string(),
        serde_json::Value::String(input.return_url.clone()),
    );
    if let Some(payer_name) = dodopay_optional_text(input.payer_name.as_deref(), 80) {
        unsigned.insert(
            "payer_name".to_string(),
            serde_json::Value::String(payer_name),
        );
    }
    unsigned.insert("metadata".to_string(), serde_json::Value::Object(metadata));
    let body = dodopay_signed_body(&config.app_id, &config.app_secret, unsigned)?;

    let client = reqwest::Client::new();
    let response = client
        .post(dodopay_checkout_url(config))
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("dodopay create order failed: {err}"))?;
    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|err| format!("dodopay response read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "dodopay create order returned {status}: {response_text}"
        ));
    }
    let checkout: DodopayCreateOrderResponse = serde_json::from_str(&response_text)
        .map_err(|err| format!("dodopay response parse failed: {err}"))?;
    let checkout_url = checkout.checkout_url.trim().to_string();
    if checkout_url.is_empty() {
        return Err("dodopay checkout_url is empty".to_string());
    }
    let gateway_order_id = checkout.order_id.trim().to_string();
    if gateway_order_id.is_empty() {
        return Err("dodopay order_id is empty".to_string());
    }
    let channel_response = client
        .post(dodopay_channel_url(config, &gateway_order_id))
        .json(&json!({ "channel": provider_channel }))
        .send()
        .await
        .map_err(|err| format!("dodopay save payment channel failed: {err}"))?;
    let channel_status = channel_response.status();
    let channel_response_text = channel_response
        .text()
        .await
        .unwrap_or_else(|_| String::new());
    if !channel_status.is_success() {
        if let Err(err) = cancel_dodopay_remote_order(config, &gateway_order_id).await {
            warn!(error = %err, "failed to cancel dodopay order after channel save failure");
        }
        return Err(format!(
            "dodopay save payment channel returned {channel_status}: {channel_response_text}"
        ));
    }
    let payable_amount = dodopay_parse_decimal_amount(checkout.payable_amount.as_deref())
        .unwrap_or(input.pay_amount);
    let provider_order_status = checkout.status.unwrap_or_else(|| "pending".to_string());
    let local_cancel_url = dodopay_cancel_url(
        &input.cancel_base_url,
        &input.order_no,
        Some(&gateway_order_id),
        &config.app_secret,
    );
    let payment_instructions = json!({
        "gateway": "dodopay",
        "display_name": dodopay_payment_channel_display_name(&input.payment_channel),
        "gateway_order_id": gateway_order_id,
        "provider_order_id": checkout.order_id,
        "payment_url": checkout_url,
        "local_cancel_url": local_cancel_url,
        "submit_method": "GET",
        "qr_code": serde_json::Value::Null,
        "pay_amount": payable_amount,
        "pay_currency": config.pay_currency,
        "payment_channel": input.payment_channel,
        "provider_channel": provider_channel,
        "provider_order_status": provider_order_status,
        "selected_channel": checkout.selected_channel,
        "confirmed_channel": checkout.confirmed_channel,
        "expires_at": checkout.expires_at,
    });

    Ok(DodopayCheckoutOutput {
        gateway_order_id,
        pay_amount: payable_amount,
        payment_instructions,
    })
}

fn dodopay_plain(status: http::StatusCode, body: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(body))
        .expect("dodopay plain response should build")
}

fn dodopay_redirect(location: String) -> Response<Body> {
    Response::builder()
        .status(http::StatusCode::FOUND)
        .header(http::header::LOCATION, location)
        .body(Body::empty())
        .expect("dodopay redirect response should build")
}

fn dodopay_return_location(query: Option<&str>) -> String {
    let suffix = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("?{value}"))
        .unwrap_or_default();
    format!("/dashboard/wallet{suffix}")
}

pub(super) async fn handle_dodopay_return(
    request_context: &GatewayPublicRequestContext,
) -> Response<Body> {
    dodopay_redirect(dodopay_return_location(
        request_context.request_query_string.as_deref(),
    ))
}

fn dodopay_query_param(query: Option<&str>, key: &str) -> Option<String> {
    url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn dodopay_cancel_location() -> String {
    "/dashboard/wallet?payment_cancelled=1".to_string()
}

fn dodopay_cancel_failed_location() -> String {
    "/dashboard/wallet?payment_cancel_failed=1".to_string()
}

fn dodopay_value_at<'a>(
    value: &'a serde_json::Value,
    path: &[&str],
) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn dodopay_string_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.trim().to_string()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
    .filter(|value| !value.is_empty())
}

fn dodopay_string_at(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    dodopay_value_at(value, path).and_then(dodopay_string_value)
}

fn dodopay_callback_object(payload: &serde_json::Value) -> &serde_json::Value {
    dodopay_value_at(payload, &["data", "object"])
        .or_else(|| dodopay_value_at(payload, &["data"]))
        .filter(|value| value.is_object())
        .unwrap_or(payload)
}

fn dodopay_callback_event_type(payload: &serde_json::Value) -> Option<String> {
    dodopay_string_at(payload, &["type"])
        .or_else(|| dodopay_string_at(payload, &["event_type"]))
        .map(|value| value.to_ascii_lowercase())
}

fn dodopay_is_success_event(payload: &serde_json::Value) -> bool {
    matches!(
        dodopay_callback_event_type(payload).as_deref(),
        Some("payment.succeeded" | "payment.success" | "payment_succeeded")
    )
}

fn dodopay_product_matches(payload: &serde_json::Value, product_id: &str) -> bool {
    let object = dodopay_callback_object(payload);
    let matches_direct_product = dodopay_string_at(payload, &["app_id"])
        .or_else(|| dodopay_string_at(object, &["app_id"]))
        .or_else(|| dodopay_string_at(payload, &["product_id"]))
        .or_else(|| dodopay_string_at(object, &["product_id"]))
        .map(|value| value == product_id);
    if let Some(matches) = matches_direct_product {
        return matches;
    }

    let carts = [
        dodopay_value_at(payload, &["product_cart"]),
        dodopay_value_at(object, &["product_cart"]),
    ];
    let mut saw_product_id = false;
    for cart in carts.into_iter().flatten() {
        let Some(items) = cart.as_array() else {
            continue;
        };
        for item in items {
            if let Some(value) = dodopay_string_at(item, &["product_id"]) {
                saw_product_id = true;
                if value == product_id {
                    return true;
                }
            }
        }
    }
    !saw_product_id
}

fn dodopay_callback_order_no(payload: &serde_json::Value) -> Option<String> {
    let object = dodopay_callback_object(payload);
    dodopay_string_at(object, &["metadata", "order_no"])
        .or_else(|| dodopay_string_at(payload, &["metadata", "order_no"]))
        .or_else(|| dodopay_string_at(object, &["merchant_order_id"]))
        .or_else(|| dodopay_string_at(payload, &["merchant_order_id"]))
}

fn dodopay_callback_gateway_order_id(payload: &serde_json::Value) -> Option<String> {
    let object = dodopay_callback_object(payload);
    dodopay_string_at(object, &["payment_id"])
        .or_else(|| dodopay_string_at(payload, &["payment_id"]))
        .or_else(|| dodopay_string_at(object, &["checkout_session_id"]))
        .or_else(|| dodopay_string_at(payload, &["checkout_session_id"]))
        .or_else(|| dodopay_string_at(object, &["order_id"]))
        .or_else(|| dodopay_string_at(payload, &["order_id"]))
        .or_else(|| dodopay_string_at(object, &["id"]))
}

fn dodopay_callback_channel(payload: &serde_json::Value) -> Option<String> {
    let object = dodopay_callback_object(payload);
    dodopay_string_at(object, &["payment_method_type"])
        .or_else(|| dodopay_string_at(object, &["payment_method"]))
        .or_else(|| dodopay_string_at(object, &["channel"]))
        .or_else(|| dodopay_string_at(payload, &["channel"]))
        .and_then(|value| normalize_dodopay_payment_channel(Some(&value)).ok())
}

fn dodopay_callback_currency(payload: &serde_json::Value) -> Option<String> {
    let object = dodopay_callback_object(payload);
    dodopay_string_at(object, &["currency"])
        .or_else(|| dodopay_string_at(payload, &["currency"]))
        .map(|value| value.to_ascii_uppercase())
}

fn dodopay_amount_value(value: &serde_json::Value, minor_units: bool) -> Option<f64> {
    let amount = match value {
        serde_json::Value::Number(number) if minor_units => number.as_f64()? / 100.0,
        serde_json::Value::Number(number) => number.as_f64()?,
        serde_json::Value::String(value) if minor_units && !value.contains('.') => {
            value.trim().parse::<f64>().ok()? / 100.0
        }
        serde_json::Value::String(value) => value.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    amount
        .is_finite()
        .then_some(amount)
        .filter(|value| *value > 0.0)
}

fn dodopay_amount_at(value: &serde_json::Value, path: &[&str], minor_units: bool) -> Option<f64> {
    dodopay_value_at(value, path).and_then(|value| dodopay_amount_value(value, minor_units))
}

fn dodopay_amount_at_allow_zero(
    value: &serde_json::Value,
    path: &[&str],
    minor_units: bool,
) -> Option<f64> {
    let amount = match dodopay_value_at(value, path)? {
        serde_json::Value::Number(number) if minor_units => number.as_f64()? / 100.0,
        serde_json::Value::Number(number) => number.as_f64()?,
        serde_json::Value::String(value) if minor_units && !value.contains('.') => {
            value.trim().parse::<f64>().ok()? / 100.0
        }
        serde_json::Value::String(value) => value.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    amount
        .is_finite()
        .then_some(amount)
        .filter(|value| *value >= 0.0)
}

fn dodopay_total_amount_excluding_tax(payload: &serde_json::Value) -> Option<f64> {
    let object = dodopay_callback_object(payload);
    let total_amount = dodopay_amount_at(object, &["total_amount"], true)
        .or_else(|| dodopay_amount_at(payload, &["total_amount"], true))?;
    let tax = dodopay_amount_at_allow_zero(object, &["tax"], true)
        .or_else(|| dodopay_amount_at_allow_zero(payload, &["tax"], true))
        .unwrap_or(0.0);
    let amount = ((total_amount - tax) * 100.0).round() / 100.0;
    amount
        .is_finite()
        .then_some(amount)
        .filter(|value| *value > 0.0)
}

fn dodopay_callback_pay_amount(payload: &serde_json::Value) -> Option<f64> {
    let object = dodopay_callback_object(payload);
    dodopay_amount_at(object, &["payment_amount"], true)
        .or_else(|| dodopay_amount_at(payload, &["payment_amount"], true))
        .or_else(|| dodopay_amount_at(object, &["amount"], true))
        .or_else(|| dodopay_amount_at(payload, &["amount"], true))
        .or_else(|| dodopay_amount_at(object, &["payable_amount"], false))
        .or_else(|| dodopay_amount_at(payload, &["payable_amount"], false))
        .or_else(|| dodopay_total_amount_excluding_tax(payload))
}

pub(super) async fn handle_dodopay_cancel(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
) -> Response<Body> {
    let query = request_context.request_query_string.as_deref();
    if let (Some(order_no), Some(token)) = (
        dodopay_query_param(query, "order_no"),
        dodopay_query_param(query, "token"),
    ) {
        match load_dodopay_config(state).await {
            Ok(config) => {
                let gateway_order_id = dodopay_query_param(query, "gateway_order_id")
                    .or_else(|| dodopay_query_param(query, "order_id"));
                if !dodopay_verify_cancel_token(
                    &config.app_secret,
                    &order_no,
                    gateway_order_id.as_deref(),
                    &token,
                ) {
                    warn!("rejected dodopay cancel callback with invalid token");
                    return dodopay_redirect(dodopay_cancel_failed_location());
                }
                if let Some(gateway_order_id) = gateway_order_id.as_deref() {
                    if let Err(err) = cancel_dodopay_remote_order(&config, gateway_order_id).await {
                        warn!(error = %err, "failed to cancel dodopay remote order");
                        return dodopay_redirect(dodopay_cancel_failed_location());
                    }
                }
                let outcome = state
                    .cancel_payment_order(
                        aether_data::repository::wallet::CancelPaymentOrderInput {
                            order_no,
                            expected_payment_provider: Some("dodopay".to_string()),
                            cancel_reason: "user_cancelled_at_gateway".to_string(),
                            cancel_source: "dodopay_cancel_url".to_string(),
                        },
                    )
                    .await;
                match outcome {
                    Ok(Some(aether_data::repository::wallet::WalletMutationOutcome::Applied(
                        _,
                    ))) => {}
                    Ok(other) => {
                        warn!(outcome = ?other, "dodopay local cancel did not apply");
                        return dodopay_redirect(dodopay_cancel_failed_location());
                    }
                    Err(err) => {
                        warn!(error = ?err, "failed to mark dodopay order as cancelled");
                        return dodopay_redirect(dodopay_cancel_failed_location());
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "failed to load dodopay config for cancel callback");
                return dodopay_redirect(dodopay_cancel_failed_location());
            }
        }
    } else {
        return dodopay_redirect(dodopay_cancel_failed_location());
    }
    dodopay_redirect(dodopay_cancel_location())
}

pub(super) async fn handle_dodopay_notify(
    state: &AppState,
    _headers: &http::HeaderMap,
    request_body: Option<&axum::body::Bytes>,
) -> Response<Body> {
    let config = match load_dodopay_config(state).await {
        Ok(value) => value,
        Err(_) => return dodopay_plain(http::StatusCode::SERVICE_UNAVAILABLE, "fail"),
    };
    let Some(request_body) = request_body else {
        return dodopay_plain(http::StatusCode::BAD_REQUEST, "fail");
    };
    let payload: serde_json::Value = match serde_json::from_slice(request_body) {
        Ok(value) => value,
        Err(_) => return dodopay_plain(http::StatusCode::BAD_REQUEST, "fail"),
    };
    let signature_valid =
        dodopay_verify_payload_signature(&config.app_secret, &payload).unwrap_or_default();
    if !signature_valid {
        return dodopay_plain(http::StatusCode::BAD_REQUEST, "fail");
    }
    if !dodopay_is_success_event(&payload) {
        return dodopay_plain(http::StatusCode::BAD_REQUEST, "fail");
    }
    if !dodopay_product_matches(&payload, &config.app_id) {
        return dodopay_plain(http::StatusCode::BAD_REQUEST, "fail");
    }
    let pay_currency = dodopay_callback_currency(&payload);
    let order_no = dodopay_callback_order_no(&payload);
    let gateway_order_id = dodopay_callback_gateway_order_id(&payload);
    if order_no.is_none() && gateway_order_id.is_none() {
        return dodopay_plain(http::StatusCode::BAD_REQUEST, "fail");
    }
    let Some(pay_amount) = dodopay_callback_pay_amount(&payload) else {
        return dodopay_plain(http::StatusCode::BAD_REQUEST, "fail");
    };
    let amount_usd = if config.usd_exchange_rate > 0.0 {
        pay_amount / config.usd_exchange_rate
    } else {
        pay_amount
    };
    let channel = dodopay_callback_channel(&payload);
    let payload_hash = match payment_callback_payload_hash(&payload) {
        Ok(value) => value,
        Err(_) => return dodopay_plain(http::StatusCode::BAD_REQUEST, "fail"),
    };
    let callback_key = dodopay_string_at(&payload, &["event_id"]).unwrap_or_else(|| {
        gateway_order_id
            .as_deref()
            .or(order_no.as_deref())
            .map(|value| format!("dodopay:{value}:{payload_hash}"))
            .unwrap_or_else(|| format!("dodopay:{payload_hash}"))
    });

    let outcome = state
        .process_payment_callback(
            aether_data::repository::wallet::ProcessPaymentCallbackInput {
                payment_method: "dodopay".to_string(),
                payment_provider: Some("dodopay".to_string()),
                payment_channel: channel,
                callback_key,
                order_no,
                gateway_order_id,
                amount_usd,
                pay_amount: Some(pay_amount),
                pay_currency,
                exchange_rate: None,
                payload_hash,
                payload,
                signature_valid: true,
            },
        )
        .await;

    match outcome {
        Ok(Some(aether_data::repository::wallet::ProcessPaymentCallbackOutcome::Applied {
            order,
            order_id,
            ..
        })) => {
            if let Err(err) = state.apply_referral_rewards_for_paid_order(&order).await {
                warn!(
                    error = ?err,
                    order_id = %order_id,
                    "failed to apply referral rewards for dodopay callback"
                );
            }
            dodopay_plain(http::StatusCode::OK, "success")
        }
        Ok(Some(
            aether_data::repository::wallet::ProcessPaymentCallbackOutcome::AlreadyCredited {
                ..
            },
        ))
        | Ok(Some(
            aether_data::repository::wallet::ProcessPaymentCallbackOutcome::DuplicateProcessed {
                ..
            },
        )) => dodopay_plain(http::StatusCode::OK, "success"),
        _ => dodopay_plain(http::StatusCode::INTERNAL_SERVER_ERROR, "fail"),
    }
}

#[cfg(test)]
mod tests {
    use aether_data::repository::wallet::{
        CreateWalletRechargeOrderInput, CreateWalletRechargeOrderOutcome, StoredAdminPaymentOrder,
    };
    use aether_data_contracts::repository::billing::PaymentGatewayConfigWriteInput;
    use serde_json::json;

    async fn dodopay_test_state_with_endpoint(
        endpoint_url: &str,
        app_secret: &str,
    ) -> crate::AppState {
        let auth_repository = std::sync::Arc::new(
            aether_data::repository::auth::InMemoryAuthApiKeySnapshotRepository::default(),
        );
        let billing_repository: std::sync::Arc<
            dyn aether_data_contracts::repository::billing::BillingReadRepository,
        > = std::sync::Arc::new(
            aether_data::repository::billing::InMemoryBillingReadRepository::default(),
        );
        let wallet_repository = std::sync::Arc::new(
            aether_data::repository::wallet::InMemoryWalletRepository::default(),
        );
        let state = crate::AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_auth_billing_and_wallet_for_tests(
                    auth_repository,
                    billing_repository,
                    wallet_repository,
                ),
            );
        let merchant_key_encrypted =
            crate::handlers::shared::encrypt_catalog_secret_with_fallbacks(&state, app_secret)
                .expect("app secret should encrypt");
        let outcome = state
            .upsert_payment_gateway_config(&PaymentGatewayConfigWriteInput {
                provider: "dodopay".to_string(),
                enabled: true,
                endpoint_url: endpoint_url.to_string(),
                callback_base_url: Some("https://aether.example.com".to_string()),
                merchant_id: "app_123".to_string(),
                merchant_key_encrypted: Some(merchant_key_encrypted),
                preserve_existing_secret: false,
                webhook_secret_encrypted: None,
                preserve_existing_webhook_secret: true,
                pay_currency: "USD".to_string(),
                usd_exchange_rate: 1.0,
                min_recharge_usd: 1.0,
                channels_json: json!({}),
            })
            .await
            .expect("gateway config should save");
        assert!(matches!(outcome, crate::LocalMutationOutcome::Applied(_)));
        state
    }

    async fn dodopay_test_state_with_app_secret(app_secret: &str) -> crate::AppState {
        dodopay_test_state_with_endpoint("https://pay.dodododo.org", app_secret).await
    }

    async fn dodopay_test_state(app_secret: &str) -> crate::AppState {
        dodopay_test_state_with_app_secret(app_secret).await
    }

    #[derive(Clone, Default)]
    struct CapturedDodopayRequests {
        create_order: std::sync::Arc<std::sync::Mutex<Option<serde_json::Value>>>,
        save_channel: std::sync::Arc<std::sync::Mutex<Option<serde_json::Value>>>,
    }

    async fn start_dodopay_orders_test_server(
        captured: CapturedDodopayRequests,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = crate::test_support::bind_loopback_listener()
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("local addr should resolve");
        let base_url = format!("http://{addr}");
        let checkout_url = format!("{base_url}/pay/order_remote_123");
        let create_capture = captured.create_order.clone();
        let channel_capture = captured.save_channel.clone();
        let app = axum::Router::new()
            .route(
                "/api/v1/orders",
                axum::routing::post(move |body: axum::body::Bytes| {
                    let create_capture = create_capture.clone();
                    let checkout_url = checkout_url.clone();
                    async move {
                        let payload: serde_json::Value =
                            serde_json::from_slice(&body).expect("request should be json");
                        *create_capture.lock().expect("capture lock") = Some(payload.clone());
                        (
                            axum::http::StatusCode::OK,
                            axum::Json(json!({
                                "order_id": "order_remote_123",
                                "merchant_order_id": payload["merchant_order_id"],
                                "amount": payload["amount"],
                                "payable_amount": payload["amount"],
                                "status": "pending",
                                "expires_at": "2026-06-23T12:30:00.000Z",
                                "paid_at": serde_json::Value::Null,
                                "selected_channel": serde_json::Value::Null,
                                "confirmed_channel": serde_json::Value::Null,
                                "payer_name": serde_json::Value::Null,
                                "payer_contact": serde_json::Value::Null,
                                "payer_message": serde_json::Value::Null,
                                "checkout_return_url": serde_json::Value::Null,
                                "checkout_return_label": serde_json::Value::Null,
                                "checkout_url": checkout_url,
                            })),
                        )
                    }
                }),
            )
            .route(
                "/api/v1/orders/order_remote_123/channel",
                axum::routing::post(move |body: axum::body::Bytes| {
                    let channel_capture = channel_capture.clone();
                    async move {
                        let payload: serde_json::Value =
                            serde_json::from_slice(&body).expect("request should be json");
                        *channel_capture.lock().expect("capture lock") = Some(payload);
                        (axum::http::StatusCode::OK, axum::Json(json!({"ok": true})))
                    }
                }),
            );
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server should run");
        });
        (base_url, handle)
    }

    async fn create_cancel_test_order(
        state: &crate::AppState,
        order_no: &str,
        provider: &str,
    ) -> StoredAdminPaymentOrder {
        let outcome = state
            .create_wallet_recharge_order(CreateWalletRechargeOrderInput {
                preferred_wallet_id: Some(format!("wallet-{order_no}")),
                user_id: format!("user-{order_no}"),
                amount_usd: 3.0,
                pay_amount: Some(3.0),
                pay_currency: Some("USD".to_string()),
                exchange_rate: Some(1.0),
                payment_method: provider.to_string(),
                payment_provider: Some(provider.to_string()),
                payment_channel: Some("ali_pay".to_string()),
                gateway_order_id: format!("gateway-{order_no}"),
                gateway_response: json!({ "gateway": provider }),
                order_no: order_no.to_string(),
                expires_at_unix_secs: 4_102_444_800,
            })
            .await
            .expect("order create should run")
            .expect("wallet writer should exist");
        match outcome {
            CreateWalletRechargeOrderOutcome::Created(order) => order,
            CreateWalletRechargeOrderOutcome::WalletInactive => {
                panic!("new test wallet should be active")
            }
        }
    }

    fn cancel_request_context(
        query: impl Into<String>,
    ) -> crate::control::GatewayPublicRequestContext {
        crate::control::GatewayPublicRequestContext {
            request_id: "request-dodopay-cancel-test".to_string(),
            trace_id: "trace-dodopay-cancel-test".to_string(),
            request_method: axum::http::Method::GET,
            request_path: "/api/payment/dodopay/cancel".to_string(),
            request_query_string: Some(query.into()),
            request_content_type: None,
            host_header: Some("aether.example.com".to_string()),
            control_decision: None,
        }
    }

    async fn read_cancel_test_order(
        state: &crate::AppState,
        user_id: &str,
        order_id: String,
    ) -> StoredAdminPaymentOrder {
        state
            .find_wallet_payment_order_by_user_id(user_id, &order_id)
            .await
            .expect("order read should run")
            .expect("order should exist")
    }

    #[test]
    fn dodopay_signs_stable_json_without_signature() {
        let mut payload = json!({
            "timestamp": 1710000000,
            "nonce": "nonce-123456",
            "app_id": "app_test",
            "merchant_order_id": "po_test",
            "amount": "9.90",
            "subject": "钱包充值",
            "metadata": {
                "signature": "kept"
            }
        });
        let first = super::dodopay_sign_payload("secret", &payload).expect("sign should work");
        let second = super::dodopay_sign_payload("secret", &payload).expect("sign should work");
        assert_eq!(first, second);

        payload["signature"] = json!("ignored");
        let with_signature =
            super::dodopay_sign_payload("secret", &payload).expect("sign should work");
        assert_eq!(first, with_signature);

        payload["metadata"]["signature"] = json!("still-signed");
        let with_nested_signature_changed =
            super::dodopay_sign_payload("secret", &payload).expect("sign should work");
        assert_ne!(first, with_nested_signature_changed);
    }

    #[test]
    fn dodopay_callback_signature_requires_matching_secret() {
        let mut payload = json!({
            "event_id": "evt_1",
            "event_type": "payment.succeeded",
            "app_id": "app_test",
            "order_id": "order_1",
            "merchant_order_id": "po_test",
            "amount": "9.90",
            "payable_amount": "9.91",
            "channel": "ALIPAY",
            "paid_at": "2026-05-26T10:08:00.000Z",
            "metadata": null,
            "timestamp": 1710000000
        });
        let signature =
            super::dodopay_sign_payload("secret", &payload).expect("signature should build");
        payload["signature"] = json!(signature);

        assert!(super::dodopay_verify_payload_signature("secret", &payload)
            .expect("verification should work"));
        assert!(!super::dodopay_verify_payload_signature("wrong", &payload)
            .expect("verification should work"));
    }

    #[tokio::test]
    async fn dodopay_config_uses_app_secret_without_webhook_secret() {
        let state = dodopay_test_state_with_app_secret("dodopay-api-key").await;

        let config = super::load_dodopay_config(&state)
            .await
            .expect("app secret should be enough for pay.dodododo.org");

        assert_eq!(config.app_id, "app_123");
        assert_eq!(config.app_secret, "dodopay-api-key");
    }

    #[tokio::test]
    async fn dodopay_checkout_uses_signed_orders_api_and_selected_channel() {
        let captured = CapturedDodopayRequests::default();
        let (base_url, handle) = start_dodopay_orders_test_server(captured.clone()).await;
        let config = super::DodopayConfig {
            base_url,
            app_id: "app_test".to_string(),
            app_secret: "dodopay-app-secret".to_string(),
            callback_base_url: Some("https://aether.example.com".to_string()),
            return_path: "/dashboard/wallet".to_string(),
            pay_currency: "CNY".to_string(),
            usd_exchange_rate: 7.2,
            min_recharge_usd: 1.0,
        };

        let checkout = super::create_dodopay_checkout(
            &config,
            &super::DodopayCheckoutInput {
                order_no: "po_test_123".to_string(),
                subject: "钱包充值".to_string(),
                pay_amount: 12.34,
                notify_url: "https://aether.example.com/api/payment/dodopay/notify".to_string(),
                return_url: "https://aether.example.com/dashboard/wallet".to_string(),
                cancel_base_url: "https://aether.example.com".to_string(),
                payment_channel: "we_chat_pay".to_string(),
                payer_name: Some("alice".to_string()),
                metadata: json!({"kind": "wallet_recharge"}),
            },
        )
        .await
        .expect("checkout should be created");
        handle.abort();

        assert_eq!(checkout.gateway_order_id, "order_remote_123");
        assert!(checkout.payment_instructions["payment_url"]
            .as_str()
            .is_some_and(|value| value.ends_with("/pay/order_remote_123")));
        assert!(checkout.payment_instructions["local_cancel_url"]
            .as_str()
            .is_some_and(|value| value.contains("gateway_order_id=order_remote_123")));

        let create_payload = captured
            .create_order
            .lock()
            .expect("capture lock")
            .clone()
            .expect("create request should be captured");
        assert_eq!(create_payload["app_id"], "app_test");
        assert_eq!(create_payload["merchant_order_id"], "po_test_123");
        assert_eq!(create_payload["amount"], "12.34");
        assert_eq!(
            create_payload["notify_url"],
            "https://aether.example.com/api/payment/dodopay/notify"
        );
        assert_eq!(
            create_payload["return_url"],
            "https://aether.example.com/dashboard/wallet"
        );
        assert_eq!(create_payload["payer_name"], "alice");
        assert!(create_payload.get("product_cart").is_none());
        assert!(create_payload.get("allowed_payment_method_types").is_none());
        assert!(
            super::dodopay_verify_payload_signature("dodopay-app-secret", &create_payload)
                .expect("signature should verify")
        );

        let channel_payload = captured
            .save_channel
            .lock()
            .expect("capture lock")
            .clone()
            .expect("channel request should be captured");
        assert_eq!(channel_payload, json!({"channel": "WECHAT"}));
    }

    #[test]
    fn dodopay_payment_channel_accepts_supported_aliases() {
        assert_eq!(
            super::normalize_dodopay_payment_channel(Some("ali_pay")).expect("channel"),
            "ali_pay"
        );
        assert_eq!(
            super::normalize_dodopay_payment_channel(Some("ALIPAY")).expect("channel"),
            "ali_pay"
        );
        assert_eq!(
            super::normalize_dodopay_payment_channel(Some("we_chat_pay")).expect("channel"),
            "we_chat_pay"
        );
        assert_eq!(
            super::normalize_dodopay_payment_channel(Some("wxpay")).expect("channel"),
            "we_chat_pay"
        );
        assert!(super::normalize_dodopay_payment_channel(Some("card")).is_err());
    }

    #[test]
    fn dodopay_configured_channels_only_exposes_wechat() {
        let channels = super::configured_dodopay_channels();

        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].channel, "we_chat_pay");
        assert_eq!(channels[0].display_name, "微信支付");
    }

    #[test]
    fn dodopay_checkout_payment_channel_only_accepts_wechat() {
        assert_eq!(
            super::normalize_dodopay_checkout_payment_channel(Some("we_chat_pay"))
                .expect("wechat should be accepted"),
            "we_chat_pay"
        );
        assert_eq!(
            super::normalize_dodopay_checkout_payment_channel(Some("wxpay"))
                .expect("wechat alias should be accepted"),
            "we_chat_pay"
        );
        assert!(super::normalize_dodopay_checkout_payment_channel(Some("ali_pay")).is_err());
        assert!(super::normalize_dodopay_checkout_payment_channel(Some("ALIPAY")).is_err());
    }

    #[test]
    fn dodopay_cancel_url_points_to_local_cancel_route() {
        let cancel_url = super::dodopay_cancel_url(
            "https://aether.example.com/",
            "po_1 2",
            Some("order_1"),
            "secret",
        );

        assert!(cancel_url.starts_with(
            "https://aether.example.com/api/payment/dodopay/cancel?order_no=po_1+2&token="
        ));
        assert!(cancel_url.contains("&gateway_order_id=order_1"));
        assert!(super::dodopay_verify_cancel_token(
            "secret",
            "po_1 2",
            Some("order_1"),
            cancel_url
                .split_once("token=")
                .map(|(_, token)| token)
                .and_then(|token| token
                    .split_once('&')
                    .map(|(token, _)| token)
                    .or(Some(token)))
                .expect("token should exist")
        ));
    }

    #[test]
    fn dodopay_cancel_token_is_bound_to_order_no() {
        let token = super::dodopay_cancel_token("secret", "po_1", Some("order_1"));

        assert!(super::dodopay_verify_cancel_token(
            "secret",
            "po_1",
            Some("order_1"),
            &token
        ));
        assert!(!super::dodopay_verify_cancel_token(
            "secret",
            "po_2",
            Some("order_1"),
            &token
        ));
        assert!(!super::dodopay_verify_cancel_token(
            "secret",
            "po_1",
            Some("order_2"),
            &token
        ));
    }

    #[tokio::test]
    async fn dodopay_cancel_callback_requires_valid_token_and_dodopay_provider() {
        let api_key = "dodopay-api-key";
        let state = dodopay_test_state(api_key).await;

        let signed_order = create_cancel_test_order(&state, "po-cancel-signed", "dodopay").await;
        let signed_url = super::dodopay_cancel_url(
            "https://aether.example.com",
            &signed_order.order_no,
            None,
            api_key,
        );
        let signed_query = signed_url
            .split_once('?')
            .map(|(_, query)| query)
            .expect("signed cancel url should contain query");
        let response =
            super::handle_dodopay_cancel(&state, &cancel_request_context(signed_query)).await;
        assert_eq!(response.status(), axum::http::StatusCode::FOUND);
        let signed_after_cancel =
            read_cancel_test_order(&state, "user-po-cancel-signed", signed_order.id).await;
        assert_eq!(signed_after_cancel.status, "cancelled");

        let unsigned_order =
            create_cancel_test_order(&state, "po-cancel-unsigned", "dodopay").await;
        let response = super::handle_dodopay_cancel(
            &state,
            &cancel_request_context(format!("order_no={}", unsigned_order.order_no)),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::FOUND);
        let unsigned_after_cancel =
            read_cancel_test_order(&state, "user-po-cancel-unsigned", unsigned_order.id).await;
        assert_eq!(unsigned_after_cancel.status, "pending");

        let epay_order = create_cancel_test_order(&state, "po-cancel-epay", "epay").await;
        let epay_signed_url = super::dodopay_cancel_url(
            "https://aether.example.com",
            &epay_order.order_no,
            None,
            api_key,
        );
        let epay_signed_query = epay_signed_url
            .split_once('?')
            .map(|(_, query)| query)
            .expect("signed cancel url should contain query");
        let response =
            super::handle_dodopay_cancel(&state, &cancel_request_context(epay_signed_query)).await;
        assert_eq!(response.status(), axum::http::StatusCode::FOUND);
        let epay_after_cancel =
            read_cancel_test_order(&state, "user-po-cancel-epay", epay_order.id).await;
        assert_eq!(epay_after_cancel.status, "pending");
    }

    #[test]
    fn dodopay_callback_gateway_order_id_reads_checkout_session_id() {
        let payload = json!({
            "type": "payment.succeeded",
            "data": {
                "object": {
                    "checkout_session_id": "cs_123"
                }
            }
        });

        assert_eq!(
            super::dodopay_callback_gateway_order_id(&payload).as_deref(),
            Some("cs_123")
        );
    }

    #[test]
    fn dodopay_product_matches_official_product_cart_payload() {
        let payload = json!({
            "type": "payment.succeeded",
            "data": {
                "payment_id": "pay_123",
                "product_cart": [
                    {
                        "product_id": "pdt_123",
                        "quantity": 1
                    }
                ]
            }
        });

        assert!(super::dodopay_product_matches(&payload, "pdt_123"));
        assert!(!super::dodopay_product_matches(&payload, "pdt_other"));
    }

    #[test]
    fn dodopay_callback_currency_reads_official_payload() {
        let payload = json!({
            "type": "payment.succeeded",
            "data": {
                "payment_id": "pay_123",
                "currency": "cny"
            }
        });

        assert_eq!(
            super::dodopay_callback_currency(&payload).as_deref(),
            Some("CNY")
        );
    }

    #[test]
    fn dodopay_callback_pay_amount_excludes_reported_tax_from_total_amount() {
        let payload = json!({
            "type": "payment.succeeded",
            "data": {
                "object": {
                    "total_amount": 1099,
                    "tax": 100
                }
            }
        });

        assert_eq!(super::dodopay_callback_pay_amount(&payload), Some(9.99));
    }

    #[test]
    fn dodopay_callback_pay_amount_prefers_business_amount_over_payable_amount() {
        let payload = json!({
            "event_type": "payment.succeeded",
            "amount": "9.90",
            "payable_amount": "9.91",
            "received_amount": "9.91"
        });

        assert_eq!(super::dodopay_callback_pay_amount(&payload), Some(9.90));
    }

    #[tokio::test]
    async fn dodopay_notify_is_disabled_without_gateway_config() {
        let state = super::AppState::new().expect("state should build");
        let body = axum::body::Bytes::from(
            serde_json::to_vec(&json!({
                "event_id": "evt_1",
                "event_type": "payment.succeeded",
                "app_id": "app_test",
                "order_id": "order_1",
                "merchant_order_id": "po_test",
                "amount": "9.90",
                "payable_amount": "9.91",
                "channel": "ALIPAY",
                "paid_at": "2026-05-26T10:08:00.000Z",
                "metadata": null,
                "timestamp": 1710000000
            }))
            .expect("payload should encode"),
        );

        let response =
            super::handle_dodopay_notify(&state, &axum::http::HeaderMap::new(), Some(&body)).await;

        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
    }
}

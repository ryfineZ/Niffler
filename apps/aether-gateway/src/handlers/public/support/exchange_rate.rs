use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use chrono::Utc;
use serde_json::{json, Value};

use super::{resolve_user_portal, system_config_bool, system_config_string, AppState};

const DEFAULT_RATE_API_URL: &str = "https://api.coinbase.com/v2/exchange-rates?currency={base}";
const DEFAULT_RATE_SOURCE: &str = "coinbase";
const DEFAULT_CACHE_TTL_SECONDS: u64 = 60;
const DEFAULT_MAX_STALE_SECONDS: u64 = 3_600;
const DEFAULT_REQUEST_TIMEOUT_SECONDS: u64 = 5;

#[derive(Debug, Clone)]
pub(crate) struct ResolvedExchangeRate {
    pub(crate) rate: f64,
    pub(crate) source: String,
    pub(crate) as_of: Option<String>,
    pub(crate) live: bool,
}

impl ResolvedExchangeRate {
    pub(crate) fn public_payload(&self) -> Value {
        json!({
            "usd_exchange_rate": self.rate,
            "exchange_rate_source": self.source,
            "exchange_rate_as_of": self.as_of,
            "exchange_rate_live": self.live,
        })
    }

    pub(crate) fn enrich_payload(&self, payload: &mut Value) {
        let Some(target) = payload.as_object_mut() else {
            return;
        };
        let Some(metadata) = self.public_payload().as_object().cloned() else {
            return;
        };
        target.extend(metadata);
    }
}

#[derive(Debug, Clone)]
struct CachedExchangeRate {
    rate: f64,
    as_of: Option<String>,
    fetched_at: Instant,
}

fn exchange_rate_cache() -> &'static Mutex<BTreeMap<String, CachedExchangeRate>> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, CachedExchangeRate>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn config_u64(value: Option<&Value>, default: u64) -> u64 {
    match value {
        Some(Value::Number(value)) => value.as_u64().unwrap_or(default),
        Some(Value::String(value)) => value.trim().parse().unwrap_or(default),
        _ => default,
    }
}

fn valid_rate(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn manual_rate(rate: f64, source: &str) -> Result<ResolvedExchangeRate, String> {
    if !valid_rate(rate) {
        return Err("支付网关汇率配置无效".to_string());
    }
    Ok(ResolvedExchangeRate {
        rate,
        source: source.to_string(),
        as_of: None,
        live: false,
    })
}

fn normalize_quote_currency(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_uppercase();
    if value.len() != 3 || !value.bytes().all(|value| value.is_ascii_alphabetic()) {
        return Err("支付币种无效".to_string());
    }
    Ok(value)
}

fn build_rate_api_url(template: &str, quote: &str) -> Result<String, String> {
    let value = template
        .trim()
        .replace("{base}", "USD")
        .replace("{quote}", quote);
    let parsed = url::Url::parse(&value).map_err(|_| "实时汇率接口地址无效".to_string())?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() || !parsed.username().is_empty() {
        return Err("实时汇率接口必须是无用户信息的 HTTPS 地址".to_string());
    }
    Ok(value)
}

fn parse_rate_payload(payload: &Value, quote: &str) -> Option<(f64, Option<String>)> {
    fn value_as_f64(value: &Value) -> Option<f64> {
        value
            .as_f64()
            .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
    }

    let rate = payload
        .get("rate")
        .and_then(value_as_f64)
        .or_else(|| {
            payload
                .get("rates")
                .and_then(Value::as_object)
                .and_then(|rates| rates.get(quote))
                .and_then(value_as_f64)
        })
        .or_else(|| {
            payload
                .get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("rates"))
                .and_then(Value::as_object)
                .and_then(|rates| rates.get(quote))
                .and_then(value_as_f64)
        })?;
    if !valid_rate(rate) {
        return None;
    }
    let as_of = payload
        .get("date")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Some((rate, as_of))
}

fn cached_rate(
    quote: &str,
    max_age: Duration,
    source: &str,
    live: bool,
) -> Option<ResolvedExchangeRate> {
    let cache = exchange_rate_cache().lock().ok()?;
    let cached = cache.get(quote)?;
    if cached.fetched_at.elapsed() > max_age {
        return None;
    }
    Some(ResolvedExchangeRate {
        rate: cached.rate,
        source: source.to_string(),
        as_of: cached.as_of.clone(),
        live,
    })
}

async fn fetch_live_rate(
    state: &AppState,
    api_url: &str,
    quote: &str,
    timeout: Duration,
    source: &str,
) -> Result<ResolvedExchangeRate, String> {
    let response = tokio::time::timeout(
        timeout,
        state
            .client
            .get(api_url)
            .header(http::header::ACCEPT, "application/json")
            .send(),
    )
    .await
    .map_err(|_| "实时汇率接口请求超时".to_string())?
    .map_err(|err| format!("实时汇率接口请求失败: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("实时汇率接口返回 {}", response.status()));
    }
    let payload = response
        .json::<Value>()
        .await
        .map_err(|_| "实时汇率接口响应无效".to_string())?;
    let (rate, as_of) = parse_rate_payload(&payload, quote)
        .ok_or_else(|| "实时汇率接口未返回有效汇率".to_string())?;
    let as_of = as_of.or_else(|| Some(Utc::now().to_rfc3339()));
    if let Ok(mut cache) = exchange_rate_cache().lock() {
        cache.insert(
            quote.to_string(),
            CachedExchangeRate {
                rate,
                as_of: as_of.clone(),
                fetched_at: Instant::now(),
            },
        );
    }
    Ok(ResolvedExchangeRate {
        rate,
        source: source.to_string(),
        as_of,
        live: true,
    })
}

pub(crate) async fn resolve_payment_exchange_rate(
    state: &AppState,
    user_id: &str,
    pay_currency: &str,
    configured_rate: f64,
) -> Result<ResolvedExchangeRate, String> {
    let portal = resolve_user_portal(state, user_id)
        .await
        .map_err(|err| format!("portal settings lookup failed: {err:?}"))?;
    if !portal.is_official_usd() {
        return manual_rate(configured_rate, "gateway_config");
    }

    let quote = normalize_quote_currency(pay_currency)?;
    if quote == "USD" {
        return Ok(ResolvedExchangeRate {
            rate: 1.0,
            source: "identity".to_string(),
            as_of: None,
            live: true,
        });
    }

    let enabled = state
        .read_system_config_json_value("official_usd_portal_live_exchange_rate_enabled")
        .await
        .map_err(|err| format!("exchange rate settings lookup failed: {err:?}"))?;
    if !system_config_bool(enabled.as_ref(), true) {
        return manual_rate(configured_rate, "gateway_config");
    }
    let api_url = state
        .read_system_config_json_value("official_usd_portal_exchange_rate_api_url")
        .await
        .map_err(|err| format!("exchange rate settings lookup failed: {err:?}"))?;
    let ttl = state
        .read_system_config_json_value("official_usd_portal_exchange_rate_ttl_seconds")
        .await
        .map_err(|err| format!("exchange rate settings lookup failed: {err:?}"))?;
    let max_stale = state
        .read_system_config_json_value("official_usd_portal_exchange_rate_max_stale_seconds")
        .await
        .map_err(|err| format!("exchange rate settings lookup failed: {err:?}"))?;
    let timeout = state
        .read_system_config_json_value("official_usd_portal_exchange_rate_timeout_seconds")
        .await
        .map_err(|err| format!("exchange rate settings lookup failed: {err:?}"))?;
    let allow_manual_fallback = state
        .read_system_config_json_value("official_usd_portal_exchange_rate_allow_manual_fallback")
        .await
        .map_err(|err| format!("exchange rate settings lookup failed: {err:?}"))?;

    let ttl = Duration::from_secs(config_u64(ttl.as_ref(), DEFAULT_CACHE_TTL_SECONDS).max(1));
    if let Some(cached) = cached_rate(&quote, ttl, "live_cache", true) {
        return Ok(cached);
    }

    let template = system_config_string(api_url.as_ref())
        .or_else(|| std::env::var("NIFFLER_OFFICIAL_USD_PORTAL_EXCHANGE_RATE_API_URL").ok())
        .unwrap_or_else(|| DEFAULT_RATE_API_URL.to_string());
    let source = if template.trim() == DEFAULT_RATE_API_URL {
        DEFAULT_RATE_SOURCE
    } else {
        "configured_live_api"
    };
    let api_url = build_rate_api_url(&template, &quote)?;
    let timeout = Duration::from_secs(
        config_u64(timeout.as_ref(), DEFAULT_REQUEST_TIMEOUT_SECONDS).clamp(1, 30),
    );
    match fetch_live_rate(state, &api_url, &quote, timeout, source).await {
        Ok(rate) => Ok(rate),
        Err(live_error) => {
            let max_stale = Duration::from_secs(
                config_u64(max_stale.as_ref(), DEFAULT_MAX_STALE_SECONDS).max(1),
            );
            if let Some(cached) = cached_rate(&quote, max_stale, "live_cache_stale", false) {
                return Ok(cached);
            }
            if system_config_bool(allow_manual_fallback.as_ref(), false) {
                return manual_rate(configured_rate, "gateway_config_fallback");
            }
            Err(live_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_rate_api_url, parse_rate_payload};
    use serde_json::json;

    #[test]
    fn parses_pair_and_rates_payloads() {
        assert_eq!(
            parse_rate_payload(
                &json!({"date": "2026-08-22", "base": "USD", "quote": "CNY", "rate": 6.72}),
                "CNY"
            ),
            Some((6.72, Some("2026-08-22".to_string())))
        );
        assert_eq!(
            parse_rate_payload(&json!({"rates": {"CNY": 6.73}}), "CNY"),
            Some((6.73, None))
        );
        assert_eq!(
            parse_rate_payload(
                &json!({"data": {"currency": "USD", "rates": {"CNY": "6.74"}}}),
                "CNY"
            ),
            Some((6.74, None))
        );
    }

    #[test]
    fn rate_url_requires_https_and_expands_currency_placeholders() {
        assert_eq!(
            build_rate_api_url("https://rates.example/rate/{base}/{quote}", "CNY").unwrap(),
            "https://rates.example/rate/USD/CNY"
        );
        assert!(build_rate_api_url("http://rates.example/USD/CNY", "CNY").is_err());
    }
}

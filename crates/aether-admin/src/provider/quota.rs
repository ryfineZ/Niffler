use aether_contracts::ExecutionResult;
use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey;
use serde_json::json;
use std::collections::BTreeMap;

use super::status as provider_status;

const OAUTH_ACCOUNT_BLOCK_PREFIX: &str = "[ACCOUNT_BLOCK] ";
const OAUTH_REFRESH_FAILED_PREFIX: &str = "[REFRESH_FAILED] ";
const OAUTH_EXPIRED_PREFIX: &str = "[OAUTH_EXPIRED] ";
const OAUTH_REQUEST_FAILED_PREFIX: &str = "[REQUEST_FAILED] ";
const CODEX_SPARK_LIMIT_NAME: &str = "GPT-5.3-Codex-Spark";

pub fn provider_auto_remove_banned_keys(config: Option<&serde_json::Value>) -> bool {
    config
        .and_then(|value| value.get("pool_advanced"))
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.get("auto_remove_banned_keys"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

pub fn should_auto_remove_structured_reason(reason: Option<&str>) -> bool {
    provider_status::should_auto_remove_account_state(&provider_status::resolve_pool_account_state(
        None, None, reason,
    ))
}

fn oauth_reason_has_tag(reason: Option<&str>, tag: &str) -> bool {
    reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|reason| {
            reason
                .lines()
                .map(str::trim)
                .any(|line| line.starts_with(tag))
        })
}

fn oauth_access_token_expired(key: &StoredProviderCatalogKey, now_unix_secs: u64) -> bool {
    let now_unix_secs = if now_unix_secs == 0 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    } else {
        now_unix_secs
    };
    key.expires_at_unix_secs
        .is_none_or(|expires_at| expires_at == 0 || expires_at <= now_unix_secs)
}

pub fn should_auto_remove_oauth_invalid_key(
    key: &StoredProviderCatalogKey,
    candidate_reason: Option<&str>,
    now_unix_secs: u64,
) -> bool {
    if should_auto_remove_structured_reason(candidate_reason)
        || should_auto_remove_structured_reason(key.oauth_invalid_reason.as_deref())
    {
        return true;
    }

    let refresh_token_failed = oauth_reason_has_tag(candidate_reason, OAUTH_REFRESH_FAILED_PREFIX)
        || oauth_reason_has_tag(
            key.oauth_invalid_reason.as_deref(),
            OAUTH_REFRESH_FAILED_PREFIX,
        );
    if !refresh_token_failed {
        return false;
    }

    oauth_reason_has_tag(candidate_reason, OAUTH_EXPIRED_PREFIX)
        || oauth_reason_has_tag(key.oauth_invalid_reason.as_deref(), OAUTH_EXPIRED_PREFIX)
        || oauth_access_token_expired(key, now_unix_secs)
}

pub fn normalize_string_id_list(values: Option<Vec<String>>) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for value in values.into_iter().flatten() {
        let trimmed = value.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        out.push(trimmed.to_string());
    }
    (!out.is_empty()).then_some(out)
}

pub fn coerce_json_u64(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

pub fn coerce_json_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

pub fn coerce_json_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(value) => Some(*value),
        serde_json::Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub fn coerce_json_string(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn extract_execution_error_message(result: &ExecutionResult) -> Option<String> {
    if let Some(body_json) = result
        .body
        .as_ref()
        .and_then(|body| body.json_body.as_ref())
        .and_then(serde_json::Value::as_object)
    {
        if let Some(error) = body_json
            .get("error")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(message) = error.get("message").and_then(serde_json::Value::as_str) {
                let trimmed = message.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        if let Some(message) = body_json.get("message").and_then(serde_json::Value::as_str) {
            let trimmed = message.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    result
        .error
        .as_ref()
        .map(|error| error.message.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn quota_refresh_success_invalid_state(
    key: &StoredProviderCatalogKey,
) -> (Option<u64>, Option<String>) {
    let current_reason = key
        .oauth_invalid_reason
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if current_reason.starts_with(OAUTH_REFRESH_FAILED_PREFIX) {
        return (
            key.oauth_invalid_at_unix_secs,
            (!current_reason.is_empty()).then_some(current_reason.to_string()),
        );
    }
    (None, None)
}

pub fn parse_antigravity_usage_response(
    value: &serde_json::Value,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    let models = value.get("models")?.as_object()?;
    let mut quota_by_model = serde_json::Map::new();

    for (model_id, model_value) in models {
        let mut payload = serde_json::Map::new();
        if let Some(display_name) = coerce_json_string(
            model_value
                .get("displayName")
                .or_else(|| model_value.get("display_name")),
        ) {
            payload.insert("display_name".to_string(), json!(display_name));
        }

        let quota_info = model_value
            .get("quotaInfo")
            .and_then(serde_json::Value::as_object);
        let remaining_fraction = quota_info
            .and_then(|object| object.get("remainingFraction"))
            .and_then(coerce_json_f64);
        if let Some(remaining_fraction) = remaining_fraction {
            let used_percent = ((1.0 - remaining_fraction).max(0.0) * 100.0).min(100.0);
            payload.insert("remaining_fraction".to_string(), json!(remaining_fraction));
            payload.insert("used_percent".to_string(), json!(used_percent));
        }
        if let Some(reset_time) = quota_info
            .and_then(|object| object.get("resetTime"))
            .cloned()
            .filter(|value| !value.is_null())
        {
            payload.insert("reset_time".to_string(), reset_time);
        }
        quota_by_model.insert(model_id.clone(), serde_json::Value::Object(payload));
    }

    Some(json!({
        "updated_at": updated_at_unix_secs,
        "is_forbidden": false,
        "forbidden_reason": serde_json::Value::Null,
        "forbidden_at": serde_json::Value::Null,
        "models": quota_by_model,
    }))
}

fn parse_grok_oauth_billing_number(value: Option<&serde_json::Value>) -> Option<f64> {
    let value = value?;
    let value = value
        .as_object()
        .and_then(|object| object.get("val"))
        .unwrap_or(value);
    coerce_json_f64(value).filter(|value| value.is_finite())
}

fn parse_grok_oauth_billing_timestamp(value: Option<&serde_json::Value>) -> Option<u64> {
    let value = value?;
    if let Some(timestamp) = coerce_json_u64(value) {
        return Some(if timestamp > 1_000_000_000_000 {
            timestamp / 1000
        } else {
            timestamp
        });
    }
    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .and_then(|timestamp| u64::try_from(timestamp.timestamp()).ok())
}

/// Normalizes the authoritative weekly credits window returned by
/// `GET /v1/billing?format=credits` for Grok CLI OAuth accounts.
pub fn parse_grok_oauth_weekly_billing_response(
    value: &serde_json::Value,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    let config = value.get("config")?.as_object()?;
    let used_percent = parse_grok_oauth_billing_number(config.get("creditUsagePercent"));
    let current_period = config
        .get("currentPeriod")
        .and_then(serde_json::Value::as_object);
    let period_type = current_period
        .and_then(|period| period.get("type"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let period_start = current_period
        .and_then(|period| parse_grok_oauth_billing_timestamp(period.get("start")))
        .or_else(|| parse_grok_oauth_billing_timestamp(config.get("billingPeriodStart")));
    let period_end = current_period
        .and_then(|period| parse_grok_oauth_billing_timestamp(period.get("end")))
        .or_else(|| parse_grok_oauth_billing_timestamp(config.get("billingPeriodEnd")));
    let product_usage = config
        .get("productUsage")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let item = item.as_object()?;
                    let product = item
                        .get("product")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())?;
                    Some(json!({
                        "product": product,
                        "used_percent": parse_grok_oauth_billing_number(item.get("usagePercent")),
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if used_percent.is_none()
        && period_type.is_none()
        && period_start.is_none()
        && period_end.is_none()
        && product_usage.is_empty()
    {
        return None;
    }

    let mut metadata = serde_json::Map::new();
    if let Some(value) = used_percent {
        metadata.insert("weekly_used_percent".to_string(), json!(value));
    }
    if let Some(value) = period_type {
        metadata.insert("weekly_period_type".to_string(), json!(value));
    }
    if let Some(value) = period_start {
        metadata.insert("weekly_period_start".to_string(), json!(value));
    }
    if let Some(value) = period_end {
        metadata.insert("weekly_period_end".to_string(), json!(value));
        metadata.insert("weekly_reset_at".to_string(), json!(value));
    }
    if !product_usage.is_empty() {
        metadata.insert("weekly_product_usage".to_string(), json!(product_usage));
    }
    metadata.insert("weekly_updated_at".to_string(), json!(updated_at_unix_secs));
    Some(serde_json::Value::Object(metadata))
}

fn grok_oauth_plan_type_from_monthly_limit(
    monthly_limit_cents: Option<f64>,
) -> Option<&'static str> {
    match monthly_limit_cents?.round() as i64 {
        15_000 => Some("super"),
        150_000 => Some("heavy"),
        _ => None,
    }
}

/// Normalizes the authoritative monthly included-usage window returned by
/// `GET /v1/billing` for Grok CLI OAuth accounts.
pub fn parse_grok_oauth_monthly_billing_response(
    value: &serde_json::Value,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    let config = value.get("config")?.as_object()?;
    let limit_cents = parse_grok_oauth_billing_number(config.get("monthlyLimit"));
    let used_cents = parse_grok_oauth_billing_number(config.get("used"));
    let included_used_cents = used_cents.map(|used| match limit_cents {
        Some(limit) if limit > 0.0 => used.min(limit),
        _ => used,
    });
    let used_percent = included_used_cents
        .zip(limit_cents)
        .and_then(|(used, limit)| {
            (limit > 0.0).then_some((used / limit * 100.0).clamp(0.0, 100.0))
        });
    let period_start = parse_grok_oauth_billing_timestamp(config.get("billingPeriodStart"));
    let period_end = parse_grok_oauth_billing_timestamp(config.get("billingPeriodEnd"));
    let plan_type = grok_oauth_plan_type_from_monthly_limit(limit_cents);

    if limit_cents.is_none()
        && used_cents.is_none()
        && period_start.is_none()
        && period_end.is_none()
    {
        return None;
    }

    let mut metadata = serde_json::Map::new();
    if let Some(value) = limit_cents {
        metadata.insert("monthly_limit_cents".to_string(), json!(value));
    }
    if let Some(value) = used_cents {
        metadata.insert("monthly_used_cents".to_string(), json!(value));
    }
    if let Some(value) = included_used_cents {
        metadata.insert("monthly_included_used_cents".to_string(), json!(value));
    }
    if let Some(value) = used_percent {
        metadata.insert("monthly_used_percent".to_string(), json!(value));
    }
    if let Some(value) = period_start {
        metadata.insert("monthly_period_start".to_string(), json!(value));
    }
    if let Some(value) = period_end {
        metadata.insert("monthly_period_end".to_string(), json!(value));
        metadata.insert("monthly_reset_at".to_string(), json!(value));
    }
    if let Some(value) = plan_type {
        metadata.insert("plan_type".to_string(), json!(value));
    }
    metadata.insert(
        "monthly_updated_at".to_string(),
        json!(updated_at_unix_secs),
    );
    Some(serde_json::Value::Object(metadata))
}

pub fn normalize_codex_plan_type(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

pub fn build_codex_quota_exhausted_fallback_metadata(
    plan_type: Option<&str>,
    updated_at_unix_secs: u64,
) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    if let Some(plan_type) = normalize_codex_plan_type(plan_type) {
        object.insert(
            "plan_type".to_string(),
            serde_json::Value::String(plan_type),
        );
    }
    object.insert("updated_at".to_string(), json!(updated_at_unix_secs));
    object.insert("has_credits".to_string(), json!(false));
    object.insert("credits_balance".to_string(), json!(0.0));
    object.insert("credits_unlimited".to_string(), json!(false));
    serde_json::Value::Object(object)
}

fn codex_write_window(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Map<String, serde_json::Value>,
    target_prefix: &str,
) {
    if let Some(value) = source.get("used_percent").and_then(coerce_json_f64) {
        target.insert(format!("{target_prefix}_used_percent"), json!(value));
    }
    if let Some(value) = source.get("reset_after_seconds").and_then(coerce_json_u64) {
        target.insert(format!("{target_prefix}_reset_after_seconds"), json!(value));
    }
    if let Some(value) = source.get("reset_at").and_then(coerce_json_u64) {
        target.insert(format!("{target_prefix}_reset_at"), json!(value));
    }
    if let Some(value) = source.get("window_minutes").and_then(coerce_json_u64) {
        target.insert(format!("{target_prefix}_window_minutes"), json!(value));
    }
    if let Some(value) = source
        .get("limit_window_seconds")
        .and_then(coerce_json_u64)
        .map(|seconds| seconds / 60)
    {
        target.insert(format!("{target_prefix}_window_minutes"), json!(value));
    }
}

fn codex_window_minutes(source: &serde_json::Map<String, serde_json::Value>) -> Option<u64> {
    source
        .get("window_minutes")
        .and_then(coerce_json_u64)
        .or_else(|| {
            source
                .get("limit_window_seconds")
                .and_then(coerce_json_u64)
                .map(|seconds| seconds / 60)
        })
}

fn codex_window_has_materialized_limit(
    source: &serde_json::Map<String, serde_json::Value>,
    updated_at_unix_secs: u64,
) -> bool {
    if codex_window_minutes(source).is_some_and(|minutes| minutes > 0) {
        return true;
    }
    if source
        .get("reset_after_seconds")
        .and_then(coerce_json_u64)
        .is_some_and(|seconds| seconds > 0)
    {
        return true;
    }
    if source
        .get("reset_at")
        .and_then(coerce_json_u64)
        .is_some_and(|reset_at| reset_at > updated_at_unix_secs)
    {
        return true;
    }
    source
        .get("used_percent")
        .and_then(coerce_json_f64)
        .is_some_and(|used_percent| used_percent > 0.0)
}

fn codex_window_duration_seconds(
    source: &serde_json::Map<String, serde_json::Value>,
) -> Option<u64> {
    source
        .get("limit_window_seconds")
        .and_then(coerce_json_u64)
        .or_else(|| codex_window_minutes(source).and_then(|minutes| minutes.checked_mul(60)))
}

fn codex_window_duration_label(seconds: u64) -> String {
    if seconds == 300 * 60 {
        "5H".to_string()
    } else if seconds == 7 * 24 * 60 * 60 {
        "7D".to_string()
    } else if seconds == 30 * 24 * 60 * 60 {
        "1M".to_string()
    } else {
        let total_minutes = seconds.saturating_add(59) / 60;
        let days = total_minutes / (24 * 60);
        let hours = (total_minutes % (24 * 60)) / 60;
        let minutes = total_minutes % 60;
        let mut parts = Vec::new();
        if days > 0 {
            parts.push(format!("{days}天"));
        }
        if hours > 0 {
            parts.push(format!("{hours}小时"));
        }
        if minutes > 0 || parts.is_empty() {
            parts.push(format!("{minutes}分钟"));
        }
        parts.join("")
    }
}

fn codex_window_duration_code(seconds: u64) -> String {
    if seconds == 300 * 60 {
        "5h".to_string()
    } else if seconds == 7 * 24 * 60 * 60 {
        "weekly".to_string()
    } else if seconds == 30 * 24 * 60 * 60 {
        "1m".to_string()
    } else {
        format!("window_{seconds}s")
    }
}

fn codex_window_feature_slug(value: &str) -> String {
    let slug = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "feature".to_string()
    } else {
        slug.to_string()
    }
}

fn codex_window_metadata(
    source: &serde_json::Map<String, serde_json::Value>,
    namespace: Option<&str>,
    limit_name: Option<&str>,
    role: &str,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    if !codex_window_has_materialized_limit(source, updated_at_unix_secs) {
        return None;
    }

    let duration_seconds = codex_window_duration_seconds(source)?;
    if duration_seconds == 0 {
        return None;
    }
    let duration_code = codex_window_duration_code(duration_seconds);
    let code = namespace
        .filter(|namespace| !namespace.is_empty())
        .map(|namespace| format!("{namespace}:{duration_code}"))
        .unwrap_or_else(|| duration_code.clone());
    let label = match limit_name {
        Some(limit_name) if !limit_name.trim().is_empty() => {
            format!(
                "{} {}",
                limit_name.trim(),
                codex_window_duration_label(duration_seconds)
            )
        }
        _ => codex_window_duration_label(duration_seconds),
    };
    let mut window = serde_json::Map::new();
    window.insert("code".to_string(), json!(code));
    window.insert("label".to_string(), json!(label));
    window.insert(
        "scope".to_string(),
        json!(if namespace.is_some() {
            "feature"
        } else {
            "account"
        }),
    );
    window.insert("source_role".to_string(), json!(role));
    if let Some(limit_name) = limit_name.filter(|value| !value.trim().is_empty()) {
        window.insert("limit_name".to_string(), json!(limit_name.trim()));
    }
    window.insert(
        "used_percent".to_string(),
        source
            .get("used_percent")
            .and_then(coerce_json_f64)
            .map_or(serde_json::Value::Null, |value| json!(value)),
    );
    window.insert(
        "reset_after_seconds".to_string(),
        source
            .get("reset_after_seconds")
            .and_then(coerce_json_u64)
            .map_or(serde_json::Value::Null, |value| json!(value)),
    );
    window.insert(
        "reset_at".to_string(),
        source
            .get("reset_at")
            .and_then(coerce_json_u64)
            .map_or(serde_json::Value::Null, |value| json!(value)),
    );
    window.insert("window_seconds".to_string(), json!(duration_seconds));
    if duration_seconds % 60 == 0 {
        window.insert("window_minutes".to_string(), json!(duration_seconds / 60));
    }
    Some(serde_json::Value::Object(window))
}

fn codex_push_window_metadata(
    windows: &mut Vec<serde_json::Value>,
    source: Option<&serde_json::Map<String, serde_json::Value>>,
    namespace: Option<&str>,
    limit_name: Option<&str>,
    role: &str,
    updated_at_unix_secs: u64,
) {
    let Some(source) = source else {
        return;
    };
    let Some(mut window) =
        codex_window_metadata(source, namespace, limit_name, role, updated_at_unix_secs)
    else {
        return;
    };
    let Some(window_object) = window.as_object_mut() else {
        return;
    };
    let code = window_object
        .get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    if windows.iter().any(|item| {
        item.get("code")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|existing| existing.eq_ignore_ascii_case(&code))
    }) {
        window_object.insert("code".to_string(), json!(format!("{code}:{role}")));
    }
    windows.push(window);
}

fn codex_write_window_if_materialized(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Map<String, serde_json::Value>,
    target_prefix: &str,
    updated_at_unix_secs: u64,
) {
    if codex_window_has_materialized_limit(source, updated_at_unix_secs) {
        codex_write_window(target, source, target_prefix);
    }
}

fn codex_find_spark_rate_limit(
    root: &serde_json::Map<String, serde_json::Value>,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    root.get("additional_rate_limits")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .filter_map(serde_json::Value::as_object)
        .find(|item| {
            item.get("limit_name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.trim() == CODEX_SPARK_LIMIT_NAME)
        })?
        .get("rate_limit")
        .and_then(serde_json::Value::as_object)
}

pub fn parse_codex_wham_usage_response(
    value: &serde_json::Value,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    let root = value.as_object()?;
    if root.is_empty() {
        return None;
    }

    let mut result = serde_json::Map::new();
    let plan_type =
        normalize_codex_plan_type(root.get("plan_type").and_then(serde_json::Value::as_str));
    if let Some(plan_type) = plan_type.as_ref() {
        result.insert("plan_type".to_string(), json!(plan_type));
    }

    let rate_limit = root
        .get("rate_limit")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let primary_window = rate_limit
        .get("primary_window")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let secondary_window = rate_limit
        .get("secondary_window")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut windows = Vec::new();
    codex_push_window_metadata(
        &mut windows,
        Some(&primary_window),
        None,
        None,
        "primary",
        updated_at_unix_secs,
    );
    codex_push_window_metadata(
        &mut windows,
        Some(&secondary_window),
        None,
        None,
        "secondary",
        updated_at_unix_secs,
    );

    let has_paid_plan = plan_type.as_deref() != Some("free");
    let use_paid_windows = has_paid_plan
        && (!secondary_window.is_empty()
            || codex_window_minutes(&primary_window).is_some_and(|minutes| minutes == 300));
    if use_paid_windows {
        codex_write_window_if_materialized(
            &mut result,
            &secondary_window,
            "primary",
            updated_at_unix_secs,
        );
        codex_write_window_if_materialized(
            &mut result,
            &primary_window,
            "secondary",
            updated_at_unix_secs,
        );
    } else {
        codex_write_window_if_materialized(
            &mut result,
            &primary_window,
            "primary",
            updated_at_unix_secs,
        );
    }

    if let Some(spark_rate_limit) = codex_find_spark_rate_limit(root) {
        if let Some(primary_window) = spark_rate_limit
            .get("primary_window")
            .and_then(serde_json::Value::as_object)
        {
            codex_write_window_if_materialized(
                &mut result,
                primary_window,
                "spark_primary",
                updated_at_unix_secs,
            );
        }
        if let Some(secondary_window) = spark_rate_limit
            .get("secondary_window")
            .and_then(serde_json::Value::as_object)
        {
            codex_write_window_if_materialized(
                &mut result,
                secondary_window,
                "spark_secondary",
                updated_at_unix_secs,
            );
        }
    }
    if let Some(additional_rate_limits) = root
        .get("additional_rate_limits")
        .and_then(serde_json::Value::as_array)
    {
        for item in additional_rate_limits
            .iter()
            .filter_map(serde_json::Value::as_object)
        {
            let limit_name = item
                .get("limit_name")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let namespace = limit_name.map(|value| {
                if value == CODEX_SPARK_LIMIT_NAME {
                    "spark".to_string()
                } else {
                    format!("feature:{}", codex_window_feature_slug(value))
                }
            });
            let Some(rate_limit) = item
                .get("rate_limit")
                .and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            for (role, key) in [
                ("primary", "primary_window"),
                ("secondary", "secondary_window"),
            ] {
                codex_push_window_metadata(
                    &mut windows,
                    rate_limit.get(key).and_then(serde_json::Value::as_object),
                    namespace.as_deref(),
                    limit_name,
                    role,
                    updated_at_unix_secs,
                );
            }
        }
    }

    if let Some(credits) = root.get("credits").and_then(serde_json::Value::as_object) {
        if let Some(value) = credits.get("has_credits").and_then(coerce_json_bool) {
            result.insert("has_credits".to_string(), json!(value));
        }
        if let Some(value) = credits.get("balance").and_then(coerce_json_f64) {
            result.insert("credits_balance".to_string(), json!(value));
        }
        if let Some(value) = credits.get("unlimited").and_then(coerce_json_bool) {
            result.insert("credits_unlimited".to_string(), json!(value));
        }
    }

    if let Some(available_count) = root
        .get("rate_limit_reset_credits")
        .and_then(serde_json::Value::as_object)
        .and_then(|credits| credits.get("available_count"))
        .and_then(coerce_json_u64)
    {
        result.insert(
            "rate_limit_reset_credits".to_string(),
            json!({ "available_count": available_count }),
        );
    }

    if !windows.is_empty() {
        result.insert("windows".to_string(), json!(windows));
    }
    if result.is_empty() {
        return None;
    }
    result.insert("updated_at".to_string(), json!(updated_at_unix_secs));
    Some(serde_json::Value::Object(result))
}

fn codex_json_object<'a>(
    root: &'a serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    keys.iter()
        .find_map(|key| root.get(*key).and_then(serde_json::Value::as_object))
}

fn codex_json_string_from_object(
    object: Option<&serde_json::Map<String, serde_json::Value>>,
    keys: &[&str],
) -> Option<String> {
    let object = object?;
    keys.iter()
        .find_map(|key| coerce_json_string(object.get(*key)))
}

fn codex_json_string_from_root(
    root: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .find_map(|key| coerce_json_string(root.get(*key)))
}

fn codex_backend_me_account_object(
    root: &serde_json::Map<String, serde_json::Value>,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    codex_json_object(root, &["account", "current_account", "selected_account"])
        .or_else(|| {
            root.get("accounts")
                .and_then(serde_json::Value::as_array)?
                .iter()
                .filter_map(serde_json::Value::as_object)
                .find(|account| {
                    account
                        .get("is_default")
                        .or_else(|| account.get("selected"))
                        .or_else(|| account.get("current"))
                        .and_then(coerce_json_bool)
                        .unwrap_or(false)
                })
        })
        .or_else(|| {
            root.get("accounts")
                .and_then(serde_json::Value::as_array)?
                .iter()
                .find_map(serde_json::Value::as_object)
        })
}

fn codex_backend_me_plan_object<'a>(
    root: &'a serde_json::Map<String, serde_json::Value>,
    account: Option<&'a serde_json::Map<String, serde_json::Value>>,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    codex_json_object(root, &["plan", "subscription", "workspace_plan"]).or_else(|| {
        account
            .and_then(|account| account.get("plan"))
            .and_then(serde_json::Value::as_object)
    })
}

pub fn parse_codex_backend_me_response(
    value: &serde_json::Value,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    let root = value.as_object()?;
    if root.is_empty() {
        return None;
    }

    let user = codex_json_object(root, &["user", "auth_user", "profile"]);
    let account = codex_backend_me_account_object(root);
    let plan = codex_backend_me_plan_object(root, account);
    let mut result = serde_json::Map::new();

    if let Some(user_id) = codex_json_string_from_object(user, &["id", "user_id"])
        .or_else(|| codex_json_string_from_root(root, &["user_id"]))
    {
        result.insert("user_id".to_string(), json!(user_id));
    }
    if let Some(email) = codex_json_string_from_object(user, &["email"])
        .or_else(|| codex_json_string_from_root(root, &["email"]))
    {
        result.insert("email".to_string(), json!(email));
    }
    if let Some(name) = codex_json_string_from_object(user, &["name", "display_name", "full_name"])
        .or_else(|| codex_json_string_from_root(root, &["name", "display_name", "full_name"]))
    {
        result.insert("user_name".to_string(), json!(name));
    }
    if let Some(account_id) =
        codex_json_string_from_object(account, &["id", "account_id", "accountId", "workspace_id"])
            .or_else(|| {
                codex_json_string_from_root(root, &["account_id", "accountId", "workspace_id"])
            })
    {
        result.insert("account_id".to_string(), json!(account_id));
    }
    if let Some(account_name) =
        codex_json_string_from_object(account, &["name", "title", "display_name"])
    {
        result.insert("account_name".to_string(), json!(account_name));
    }

    let plan_type = codex_json_string_from_object(
        account,
        &["plan_type", "planType", "subscription_plan", "tier"],
    )
    .or_else(|| codex_json_string_from_object(plan, &["type", "plan_type", "name", "tier"]))
    .or_else(|| codex_json_string_from_root(root, &["plan_type", "planType"]));
    if let Some(plan_type) = normalize_codex_plan_type(plan_type.as_deref()) {
        result.insert("plan_type".to_string(), json!(plan_type));
    }
    if let Some(plan_title) =
        codex_json_string_from_object(plan, &["title", "display_name", "label"])
    {
        result.insert("plan_title".to_string(), json!(plan_title));
    }

    if result.is_empty() {
        return None;
    }
    result.insert("updated_at".to_string(), json!(updated_at_unix_secs));
    Some(serde_json::Value::Object(result))
}

pub fn parse_codex_usage_headers(
    headers: &BTreeMap<String, String>,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    let mut result = serde_json::Map::new();
    let normalized = headers
        .iter()
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect::<BTreeMap<_, _>>();
    if !normalized.keys().any(|key| key.starts_with("x-codex-")) {
        return None;
    }

    let plan_type =
        normalize_codex_plan_type(normalized.get("x-codex-plan-type").map(String::as_str));
    if let Some(plan_type) = plan_type.as_ref() {
        result.insert("plan_type".to_string(), json!(plan_type));
    }

    let read_window = |prefix: &str| -> serde_json::Map<String, serde_json::Value> {
        let mut object = serde_json::Map::new();
        let used_key = format!("x-codex-{prefix}-used-percent");
        let reset_after_key = format!("x-codex-{prefix}-reset-after-seconds");
        let reset_at_key = format!("x-codex-{prefix}-reset-at");
        let window_minutes_key = format!("x-codex-{prefix}-window-minutes");
        if let Some(value) = normalized
            .get(&used_key)
            .and_then(|value| value.parse::<f64>().ok())
        {
            object.insert("used_percent".to_string(), json!(value));
        }
        if let Some(value) = normalized
            .get(&reset_after_key)
            .and_then(|value| value.parse::<u64>().ok())
        {
            object.insert("reset_after_seconds".to_string(), json!(value));
        }
        if let Some(value) = normalized
            .get(&reset_at_key)
            .and_then(|value| value.parse::<u64>().ok())
        {
            object.insert("reset_at".to_string(), json!(value));
        }
        if let Some(value) = normalized
            .get(&window_minutes_key)
            .and_then(|value| value.parse::<u64>().ok())
        {
            object.insert("window_minutes".to_string(), json!(value));
        }
        object
    };

    let primary_window = read_window("primary");
    let secondary_window = read_window("secondary");
    let mut windows = Vec::new();
    codex_push_window_metadata(
        &mut windows,
        Some(&primary_window),
        None,
        None,
        "primary",
        updated_at_unix_secs,
    );
    codex_push_window_metadata(
        &mut windows,
        Some(&secondary_window),
        None,
        None,
        "secondary",
        updated_at_unix_secs,
    );
    let has_paid_plan = plan_type.as_deref() != Some("free");
    let use_paid_windows = has_paid_plan
        && (!secondary_window.is_empty()
            || codex_window_minutes(&primary_window).is_some_and(|minutes| minutes == 300));
    if use_paid_windows {
        codex_write_window_if_materialized(
            &mut result,
            &secondary_window,
            "primary",
            updated_at_unix_secs,
        );
        codex_write_window_if_materialized(
            &mut result,
            &primary_window,
            "secondary",
            updated_at_unix_secs,
        );
    } else {
        codex_write_window_if_materialized(
            &mut result,
            &primary_window,
            "primary",
            updated_at_unix_secs,
        );
    }

    if let Some(value) = normalized
        .get("x-codex-primary-over-secondary-limit-percent")
        .and_then(|value| value.parse::<f64>().ok())
    {
        result.insert(
            "primary_over_secondary_limit_percent".to_string(),
            json!(value),
        );
    }
    if let Some(value) = normalized
        .get("x-codex-credits-has-credits")
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        })
    {
        result.insert("has_credits".to_string(), json!(value));
    }
    if let Some(value) = normalized
        .get("x-codex-credits-balance")
        .and_then(|value| value.parse::<f64>().ok())
    {
        result.insert("credits_balance".to_string(), json!(value));
    }
    if let Some(value) = normalized
        .get("x-codex-credits-unlimited")
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        })
    {
        result.insert("credits_unlimited".to_string(), json!(value));
    }

    if !windows.is_empty() {
        result.insert("windows".to_string(), json!(windows));
    }
    if result.is_empty() {
        return None;
    }
    result.insert("updated_at".to_string(), json!(updated_at_unix_secs));
    Some(serde_json::Value::Object(result))
}

fn codex_current_invalid_reason(key: &StoredProviderCatalogKey) -> String {
    key.oauth_invalid_reason
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

fn codex_merge_invalid_reason(current: &str, candidate_reason: &str) -> String {
    if current.is_empty() {
        return candidate_reason.to_string();
    }
    if current.starts_with(OAUTH_ACCOUNT_BLOCK_PREFIX) {
        return current.to_string();
    }
    if current.starts_with(OAUTH_EXPIRED_PREFIX)
        && candidate_reason.starts_with(OAUTH_REFRESH_FAILED_PREFIX)
    {
        if current
            .lines()
            .map(str::trim)
            .any(|line| line.starts_with(OAUTH_REFRESH_FAILED_PREFIX))
        {
            return current.to_string();
        }
        return format!("{current}\n{candidate_reason}");
    }
    if current.starts_with(OAUTH_EXPIRED_PREFIX)
        && candidate_reason.starts_with(OAUTH_REQUEST_FAILED_PREFIX)
    {
        return current.to_string();
    }
    candidate_reason.to_string()
}

pub fn codex_build_invalid_state(
    key: &StoredProviderCatalogKey,
    candidate_reason: String,
    now_unix_secs: u64,
) -> (Option<u64>, Option<String>) {
    let current_reason = codex_current_invalid_reason(key);
    let merged_reason = codex_merge_invalid_reason(&current_reason, &candidate_reason);
    if merged_reason == current_reason {
        return (key.oauth_invalid_at_unix_secs, Some(merged_reason));
    }
    (Some(now_unix_secs), Some(merged_reason))
}

pub fn codex_looks_like_token_invalidated(message: Option<&str>) -> bool {
    let lowered = message.unwrap_or_default().trim().to_ascii_lowercase();
    lowered.contains("token invalid")
        || lowered.contains("token invalidated")
        || lowered.contains("session has expired")
        || lowered.contains("session expired")
}

fn codex_looks_like_account_deactivated(message: Option<&str>) -> bool {
    let lowered = message.unwrap_or_default().trim().to_ascii_lowercase();
    lowered.contains("account has been deactivated") || lowered.contains("account deactivated")
}

pub fn codex_looks_like_workspace_deactivated(message: Option<&str>) -> bool {
    let lowered = message.unwrap_or_default().trim().to_ascii_lowercase();
    lowered.contains("deactivated_workspace")
        || (lowered.contains("workspace") && lowered.contains("deactivated"))
}

pub fn codex_structured_invalid_reason(status_code: u16, upstream_message: Option<&str>) -> String {
    let message = upstream_message.unwrap_or_default().trim();
    if status_code == 402 && codex_looks_like_workspace_deactivated(Some(message)) {
        return format!("{OAUTH_ACCOUNT_BLOCK_PREFIX}工作区已停用 (deactivated_workspace)");
    }
    if codex_looks_like_account_deactivated(Some(message)) {
        let detail = if message.is_empty() {
            "OpenAI 账号已停用"
        } else {
            message
        };
        return format!("{OAUTH_ACCOUNT_BLOCK_PREFIX}{detail}");
    }
    if codex_looks_like_token_invalidated(Some(message)) {
        let detail = if message.is_empty() {
            "Codex Token 无效或已过期"
        } else {
            message
        };
        return format!("{OAUTH_EXPIRED_PREFIX}{detail}");
    }
    if status_code == 401 {
        let detail = if message.is_empty() {
            "Codex Token 无效或已过期 (401)"
        } else {
            message
        };
        return format!("{OAUTH_EXPIRED_PREFIX}{detail}");
    }
    if status_code == 403 {
        let detail = if message.is_empty() {
            "Codex 账户访问受限 (403)"
        } else {
            message
        };
        return format!("{OAUTH_ACCOUNT_BLOCK_PREFIX}{detail}");
    }
    if status_code == 402 {
        let detail = if message.is_empty() {
            "Codex 账户需要付款 (402)"
        } else {
            message
        };
        return format!("{OAUTH_ACCOUNT_BLOCK_PREFIX}{detail}");
    }
    message.to_string()
}

pub fn codex_runtime_invalid_reason(
    status_code: u16,
    upstream_message: Option<&str>,
) -> Option<String> {
    match status_code {
        401 => Some(codex_structured_invalid_reason(401, upstream_message)),
        402 => Some(codex_structured_invalid_reason(402, upstream_message)),
        403 if codex_looks_like_token_invalidated(upstream_message)
            || codex_looks_like_account_deactivated(upstream_message) =>
        {
            Some(codex_structured_invalid_reason(403, upstream_message))
        }
        _ => None,
    }
}

pub fn codex_soft_request_failure_reason(
    status_code: u16,
    upstream_message: Option<&str>,
) -> String {
    let detail = upstream_message
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Codex 请求失败 ({status_code})"));
    format!("{OAUTH_REQUEST_FAILED_PREFIX}{detail}")
}

fn compute_kiro_total_usage_limit(breakdown: &serde_json::Value) -> f64 {
    let mut total = breakdown
        .get("usageLimitWithPrecision")
        .and_then(coerce_json_f64)
        .unwrap_or(0.0);

    if breakdown
        .get("freeTrialInfo")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|free_trial| {
            free_trial
                .get("freeTrialStatus")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .is_some_and(|value| value.eq_ignore_ascii_case("ACTIVE"))
        })
    {
        total += breakdown
            .get("freeTrialInfo")
            .and_then(|value| value.get("usageLimitWithPrecision"))
            .and_then(coerce_json_f64)
            .unwrap_or(0.0);
    }

    if let Some(bonuses) = breakdown
        .get("bonuses")
        .and_then(serde_json::Value::as_array)
    {
        for bonus in bonuses {
            let is_active = bonus
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .is_some_and(|value| value.eq_ignore_ascii_case("ACTIVE"));
            if is_active {
                total += bonus
                    .get("usageLimit")
                    .and_then(coerce_json_f64)
                    .unwrap_or(0.0);
            }
        }
    }

    total
}

fn compute_kiro_current_usage(breakdown: &serde_json::Value) -> f64 {
    let mut total = breakdown
        .get("currentUsageWithPrecision")
        .and_then(coerce_json_f64)
        .unwrap_or(0.0);

    if breakdown
        .get("freeTrialInfo")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|free_trial| {
            free_trial
                .get("freeTrialStatus")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .is_some_and(|value| value.eq_ignore_ascii_case("ACTIVE"))
        })
    {
        total += breakdown
            .get("freeTrialInfo")
            .and_then(|value| value.get("currentUsageWithPrecision"))
            .and_then(coerce_json_f64)
            .unwrap_or(0.0);
    }

    if let Some(bonuses) = breakdown
        .get("bonuses")
        .and_then(serde_json::Value::as_array)
    {
        for bonus in bonuses {
            let is_active = bonus
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .is_some_and(|value| value.eq_ignore_ascii_case("ACTIVE"));
            if is_active {
                total += bonus
                    .get("currentUsage")
                    .and_then(coerce_json_f64)
                    .unwrap_or(0.0);
            }
        }
    }

    total
}

pub fn parse_kiro_usage_response(
    value: &serde_json::Value,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    let root = value.as_object()?;
    let breakdown = root
        .get("usageBreakdownList")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first())?;

    let usage_limit = compute_kiro_total_usage_limit(breakdown);
    let current_usage = compute_kiro_current_usage(breakdown);
    let remaining = (usage_limit - current_usage).max(0.0);
    let usage_percentage = if usage_limit > 0.0 {
        ((current_usage / usage_limit) * 100.0).min(100.0)
    } else {
        0.0
    };

    let mut result = serde_json::Map::new();
    result.insert("current_usage".to_string(), json!(current_usage));
    result.insert("usage_limit".to_string(), json!(usage_limit));
    result.insert("remaining".to_string(), json!(remaining));
    result.insert("usage_percentage".to_string(), json!(usage_percentage));
    result.insert("updated_at".to_string(), json!(updated_at_unix_secs));

    if let Some(subscription_title) = root
        .get("subscriptionInfo")
        .and_then(|value| value.get("subscriptionTitle"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        result.insert("subscription_title".to_string(), json!(subscription_title));
    }

    if let Some(next_reset_at) = root
        .get("nextDateReset")
        .and_then(coerce_json_f64)
        .or_else(|| breakdown.get("nextDateReset").and_then(coerce_json_f64))
    {
        result.insert("next_reset_at".to_string(), json!(next_reset_at));
    }

    let email = root
        .get("desktopUserInfo")
        .and_then(|value| value.get("email"))
        .or_else(|| root.get("userInfo").and_then(|value| value.get("email")))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(email) = email {
        result.insert("email".to_string(), json!(email));
    }

    Some(serde_json::Value::Object(result))
}

fn chatgpt_web_quota_feature_name(value: &serde_json::Value) -> Option<String> {
    coerce_json_string(
        value
            .get("feature_name")
            .or_else(|| value.get("featureName"))
            .or_else(|| value.get("feature"))
            .or_else(|| value.get("name")),
    )
}

fn chatgpt_web_is_image_quota_feature(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "image_gen" | "image_generation" | "image_edit" | "img_gen"
    )
}

fn chatgpt_web_feature_number(feature: &serde_json::Value, fields: &[&str]) -> Option<f64> {
    fields
        .iter()
        .find_map(|field| feature.get(*field).and_then(coerce_json_f64))
}

fn parse_chatgpt_web_reset_timestamp(
    value: Option<&serde_json::Value>,
    observed_at: u64,
) -> Option<u64> {
    let value = value?;
    if let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text) {
            return u64::try_from(parsed.timestamp()).ok();
        }
        if let Ok(parsed) = text.parse::<f64>() {
            return normalize_chatgpt_web_numeric_reset(parsed, observed_at);
        }
        return None;
    }
    value
        .as_f64()
        .and_then(|parsed| normalize_chatgpt_web_numeric_reset(parsed, observed_at))
}

fn normalize_chatgpt_web_numeric_reset(value: f64, observed_at: u64) -> Option<u64> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    if value > 1_000_000_000_000.0 {
        return Some((value / 1000.0).floor() as u64);
    }
    if value > 1_000_000_000.0 {
        return Some(value.floor() as u64);
    }
    Some(observed_at.saturating_add(value.floor() as u64))
}

fn chatgpt_web_blocked_features(value: &serde_json::Value) -> Vec<String> {
    value
        .get("blocked_features")
        .or_else(|| value.get("blockedFeatures"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub fn parse_chatgpt_web_conversation_init_response(
    value: &serde_json::Value,
    updated_at_unix_secs: u64,
) -> Option<serde_json::Value> {
    let root = value.as_object()?;
    let limits_progress = root
        .get("limits_progress")
        .or_else(|| root.get("limitsProgress"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let image_limit = limits_progress
        .iter()
        .find(|item| {
            chatgpt_web_quota_feature_name(item)
                .as_deref()
                .is_some_and(chatgpt_web_is_image_quota_feature)
        })
        .cloned();
    let blocked_features = chatgpt_web_blocked_features(value);
    let image_blocked = blocked_features
        .iter()
        .any(|feature| chatgpt_web_is_image_quota_feature(feature));

    if image_limit.is_none() && !image_blocked {
        return None;
    }

    let mut result = serde_json::Map::new();
    result.insert("updated_at".to_string(), json!(updated_at_unix_secs));

    if let Some(default_model_slug) = coerce_json_string(
        root.get("default_model_slug")
            .or_else(|| root.get("defaultModelSlug")),
    ) {
        result.insert("default_model_slug".to_string(), json!(default_model_slug));
    }
    if let Some(plan_type) = coerce_json_string(
        root.get("plan_type")
            .or_else(|| root.get("planType"))
            .or_else(|| root.get("subscription_plan")),
    ) {
        result.insert(
            "plan_type".to_string(),
            json!(plan_type.to_ascii_lowercase()),
        );
    }
    result.insert("blocked_features".to_string(), json!(blocked_features));
    result.insert(
        "limits_progress".to_string(),
        serde_json::Value::Array(limits_progress),
    );

    if image_blocked {
        result.insert("image_quota_blocked".to_string(), json!(true));
    }

    if let Some(image_limit) = image_limit.as_ref() {
        if let Some(feature_name) = chatgpt_web_quota_feature_name(image_limit) {
            result.insert("image_quota_feature_name".to_string(), json!(feature_name));
        }

        let remaining = chatgpt_web_feature_number(
            image_limit,
            &[
                "remaining",
                "remaining_value",
                "remainingValue",
                "remaining_count",
                "remainingCount",
            ],
        );
        let total = chatgpt_web_feature_number(
            image_limit,
            &[
                "max_value",
                "maxValue",
                "cap",
                "total",
                "limit",
                "quota",
                "usage_limit",
                "usageLimit",
            ],
        );
        let used = chatgpt_web_feature_number(
            image_limit,
            &[
                "used",
                "used_value",
                "usedValue",
                "consumed",
                "current_usage",
                "currentUsage",
            ],
        )
        .or_else(|| {
            total
                .zip(remaining)
                .map(|(total, remaining)| (total - remaining).max(0.0))
        });
        let reset_source = image_limit
            .get("reset_at")
            .or_else(|| image_limit.get("resetAt"))
            .or_else(|| image_limit.get("next_reset_at"))
            .or_else(|| image_limit.get("nextResetAt"))
            .or_else(|| image_limit.get("reset_after"))
            .or_else(|| image_limit.get("resetAfter"));
        let reset_at = parse_chatgpt_web_reset_timestamp(reset_source, updated_at_unix_secs);

        if let Some(remaining) = remaining {
            result.insert("image_quota_remaining".to_string(), json!(remaining));
        } else if image_blocked {
            result.insert("image_quota_remaining".to_string(), json!(0.0));
        }
        if let Some(total) = total {
            result.insert("image_quota_total".to_string(), json!(total));
        }
        if let Some(used) = used {
            result.insert("image_quota_used".to_string(), json!(used));
        }
        if let Some(reset_at) = reset_at {
            result.insert("image_quota_reset_at".to_string(), json!(reset_at));
        }
        if let Some(reset_after) = coerce_json_string(
            image_limit
                .get("reset_after")
                .or_else(|| image_limit.get("resetAfter")),
        ) {
            result.insert("image_quota_reset_after".to_string(), json!(reset_after));
        }
    } else if image_blocked {
        result.insert("image_quota_remaining".to_string(), json!(0.0));
    }

    Some(serde_json::Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::{
        build_codex_quota_exhausted_fallback_metadata, codex_build_invalid_state,
        codex_runtime_invalid_reason, parse_chatgpt_web_conversation_init_response,
        parse_codex_backend_me_response, parse_codex_usage_headers,
        parse_codex_wham_usage_response, parse_grok_oauth_monthly_billing_response,
        parse_grok_oauth_weekly_billing_response, OAUTH_ACCOUNT_BLOCK_PREFIX, OAUTH_EXPIRED_PREFIX,
        OAUTH_REFRESH_FAILED_PREFIX, OAUTH_REQUEST_FAILED_PREFIX,
    };
    use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn parses_grok_oauth_weekly_billing_credits() {
        let parsed = parse_grok_oauth_weekly_billing_response(
            &json!({
                "config": {
                    "creditUsagePercent": 37.5,
                    "currentPeriod": {
                        "type": "weekly",
                        "start": "2026-07-13T00:00:00Z",
                        "end": "2026-07-20T00:00:00Z"
                    },
                    "productUsage": [
                        { "product": "grok", "usagePercent": 25.0 },
                        { "product": "", "usagePercent": 99.0 }
                    ]
                }
            }),
            1_768_000_000,
        )
        .expect("weekly billing should parse");

        assert_eq!(parsed["weekly_used_percent"], json!(37.5));
        assert_eq!(parsed["weekly_period_type"], json!("weekly"));
        assert_eq!(parsed["weekly_period_start"], json!(1_783_900_800u64));
        assert_eq!(parsed["weekly_reset_at"], json!(1_784_505_600u64));
        assert_eq!(
            parsed["weekly_product_usage"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(parsed["weekly_updated_at"], json!(1_768_000_000u64));
    }

    #[test]
    fn parses_grok_oauth_monthly_billing_and_infers_plan() {
        let parsed = parse_grok_oauth_monthly_billing_response(
            &json!({
                "config": {
                    "monthlyLimit": { "val": "15000" },
                    "used": { "val": 3750 },
                    "billingPeriodStart": "2026-07-01T00:00:00Z",
                    "billingPeriodEnd": "2026-08-01T00:00:00Z"
                }
            }),
            1_768_000_000,
        )
        .expect("monthly billing should parse");

        assert_eq!(parsed["monthly_limit_cents"], json!(15_000.0));
        assert_eq!(parsed["monthly_used_cents"], json!(3_750.0));
        assert_eq!(parsed["monthly_included_used_cents"], json!(3_750.0));
        assert_eq!(parsed["monthly_used_percent"], json!(25.0));
        assert_eq!(parsed["monthly_reset_at"], json!(1_785_542_400u64));
        assert_eq!(parsed["plan_type"], json!("super"));
    }

    #[test]
    fn codex_runtime_invalid_reason_marks_401_as_expired() {
        assert_eq!(
            codex_runtime_invalid_reason(401, Some("session expired")),
            Some(format!("{OAUTH_EXPIRED_PREFIX}session expired"))
        );
    }

    #[test]
    fn codex_runtime_invalid_reason_marks_account_deactivated_403() {
        assert_eq!(
            codex_runtime_invalid_reason(403, Some("account has been deactivated")),
            Some(format!(
                "{OAUTH_ACCOUNT_BLOCK_PREFIX}account has been deactivated"
            ))
        );
    }

    #[test]
    fn codex_runtime_invalid_reason_marks_402_as_account_blocked() {
        assert_eq!(
            codex_runtime_invalid_reason(402, Some("payment required")),
            Some(format!("{OAUTH_ACCOUNT_BLOCK_PREFIX}payment required"))
        );
    }

    #[test]
    fn codex_runtime_invalid_reason_ignores_generic_403() {
        assert_eq!(codex_runtime_invalid_reason(403, Some("forbidden")), None);
    }

    #[test]
    fn codex_quota_exhausted_fallback_does_not_fabricate_windows() {
        let metadata = build_codex_quota_exhausted_fallback_metadata(Some("plus"), 1_777_000_000);

        assert_eq!(metadata["has_credits"], json!(false));
        assert_eq!(metadata["credits_balance"], json!(0.0));
        assert_eq!(metadata["credits_unlimited"], json!(false));
        assert!(metadata.get("primary_used_percent").is_none());
        assert!(metadata.get("secondary_used_percent").is_none());
    }

    #[test]
    fn codex_usage_headers_with_only_weekly_window_do_not_create_five_hour_window() {
        let headers = BTreeMap::from([
            ("x-codex-plan-type".to_string(), "team".to_string()),
            (
                "x-codex-primary-used-percent".to_string(),
                "12.5".to_string(),
            ),
            (
                "x-codex-primary-window-minutes".to_string(),
                "10080".to_string(),
            ),
            (
                "x-codex-primary-reset-after-seconds".to_string(),
                "600".to_string(),
            ),
        ]);

        let metadata =
            parse_codex_usage_headers(&headers, 1_777_000_000).expect("headers should parse");

        assert_eq!(metadata["primary_used_percent"], json!(12.5));
        assert_eq!(metadata["primary_window_minutes"], json!(10080u64));
        assert!(metadata.get("secondary_used_percent").is_none());
        assert!(metadata.get("secondary_window_minutes").is_none());
        let windows = metadata["windows"]
            .as_array()
            .expect("generic quota windows should exist");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0]["code"], json!("weekly"));
        assert_eq!(windows[0]["label"], json!("7D"));
        assert_eq!(windows[0]["window_seconds"], json!(604_800u64));
    }

    #[test]
    fn codex_invalid_state_appends_refresh_failure_to_oauth_expired() {
        let mut key = StoredProviderCatalogKey::new(
            "key-1".to_string(),
            "provider-1".to_string(),
            "key-1".to_string(),
            "oauth".to_string(),
            None,
            true,
        )
        .expect("key should build");
        key.oauth_invalid_at_unix_secs = Some(100);
        key.oauth_invalid_reason = Some(format!("{OAUTH_EXPIRED_PREFIX}session expired"));

        assert_eq!(
            codex_build_invalid_state(
                &key,
                format!("{OAUTH_REFRESH_FAILED_PREFIX}Token 续期失败"),
                200,
            ),
            (
                Some(200),
                Some(format!(
                    "{OAUTH_EXPIRED_PREFIX}session expired\n{OAUTH_REFRESH_FAILED_PREFIX}Token 续期失败"
                ))
            )
        );
    }

    #[test]
    fn codex_invalid_state_keeps_oauth_expired_over_request_failure() {
        let mut key = StoredProviderCatalogKey::new(
            "key-1".to_string(),
            "provider-1".to_string(),
            "key-1".to_string(),
            "oauth".to_string(),
            None,
            true,
        )
        .expect("key should build");
        key.oauth_invalid_at_unix_secs = Some(100);
        key.oauth_invalid_reason = Some(format!("{OAUTH_EXPIRED_PREFIX}session expired"));

        assert_eq!(
            codex_build_invalid_state(
                &key,
                format!("{OAUTH_REQUEST_FAILED_PREFIX}账号状态检查失败"),
                200,
            ),
            (
                Some(100),
                Some(format!("{OAUTH_EXPIRED_PREFIX}session expired"))
            )
        );
    }

    #[test]
    fn codex_invalid_state_allows_account_block_to_override_oauth_expired() {
        let mut key = StoredProviderCatalogKey::new(
            "key-1".to_string(),
            "provider-1".to_string(),
            "key-1".to_string(),
            "oauth".to_string(),
            None,
            true,
        )
        .expect("key should build");
        key.oauth_invalid_at_unix_secs = Some(100);
        key.oauth_invalid_reason = Some(format!("{OAUTH_EXPIRED_PREFIX}session expired"));

        assert_eq!(
            codex_build_invalid_state(
                &key,
                format!("{OAUTH_ACCOUNT_BLOCK_PREFIX}account has been deactivated"),
                200,
            ),
            (
                Some(200),
                Some(format!(
                    "{OAUTH_ACCOUNT_BLOCK_PREFIX}account has been deactivated"
                ))
            )
        );
    }

    #[test]
    fn auto_remove_refresh_failed_after_access_token_expiry() {
        let mut key = StoredProviderCatalogKey::new(
            "key-1".to_string(),
            "provider-1".to_string(),
            "key-1".to_string(),
            "oauth".to_string(),
            None,
            true,
        )
        .expect("key should build");
        key.expires_at_unix_secs = Some(1_000);
        key.oauth_invalid_reason = Some(format!("{OAUTH_REFRESH_FAILED_PREFIX}Token 续期失败"));

        assert!(!super::should_auto_remove_oauth_invalid_key(
            &key, None, 999
        ));
        assert!(super::should_auto_remove_oauth_invalid_key(
            &key, None, 1_000
        ));
    }

    #[test]
    fn auto_remove_combined_refresh_and_access_token_failure() {
        let mut key = StoredProviderCatalogKey::new(
            "key-1".to_string(),
            "provider-1".to_string(),
            "key-1".to_string(),
            "oauth".to_string(),
            None,
            true,
        )
        .expect("key should build");
        key.expires_at_unix_secs = Some(2_000);
        key.oauth_invalid_reason = Some(format!("{OAUTH_REFRESH_FAILED_PREFIX}Token 续期失败"));

        assert!(super::should_auto_remove_oauth_invalid_key(
            &key,
            Some("[OAUTH_EXPIRED] access token invalid"),
            1_000,
        ));
    }

    #[test]
    fn does_not_auto_remove_access_token_failure_without_refresh_failure() {
        let mut key = StoredProviderCatalogKey::new(
            "key-1".to_string(),
            "provider-1".to_string(),
            "key-1".to_string(),
            "oauth".to_string(),
            None,
            true,
        )
        .expect("key should build");
        key.expires_at_unix_secs = Some(1_000);
        key.oauth_invalid_reason = Some(format!("{OAUTH_EXPIRED_PREFIX}session expired"));

        assert!(!super::should_auto_remove_oauth_invalid_key(
            &key, None, 1_001
        ));
    }

    #[test]
    fn parses_codex_spark_quota_from_additional_rate_limits() {
        let parsed = parse_codex_wham_usage_response(
            &json!({
                "plan_type": "plus",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 25.0,
                        "reset_after_seconds": 604800,
                        "reset_at": 1_900_000_000u64
                    },
                    "secondary_window": {
                        "used_percent": 10.0,
                        "reset_after_seconds": 18000,
                        "reset_at": 1_800_000_000u64
                    }
                },
                "additional_rate_limits": [{
                    "limit_name": "GPT-5.3-Codex-Spark",
                    "metered_feature": "codex_bengalfox",
                    "rate_limit": {
                        "primary_window": {
                            "used_percent": 40.0,
                            "limit_window_seconds": 18000,
                            "reset_after_seconds": 9000,
                            "reset_at": 1_780_000_000u64
                        },
                        "secondary_window": {
                            "used_percent": 5.0,
                            "limit_window_seconds": 604800,
                            "reset_after_seconds": 300000,
                            "reset_at": 1_790_000_000u64
                        }
                    }
                }]
            }),
            1_777_000_000,
        )
        .expect("codex wham usage should parse");

        assert_eq!(parsed.get("primary_used_percent"), Some(&json!(10.0)));
        assert_eq!(parsed.get("secondary_used_percent"), Some(&json!(25.0)));
        assert_eq!(parsed.get("spark_primary_used_percent"), Some(&json!(40.0)));
        assert_eq!(
            parsed.get("spark_primary_window_minutes"),
            Some(&json!(300u64))
        );
        assert_eq!(
            parsed.get("spark_secondary_used_percent"),
            Some(&json!(5.0))
        );
        assert_eq!(
            parsed.get("spark_secondary_window_minutes"),
            Some(&json!(10_080u64))
        );
    }

    #[test]
    fn parses_codex_rate_limit_reset_credit_count() {
        let parsed = parse_codex_wham_usage_response(
            &json!({
                "plan_type": "pro",
                "rate_limit_reset_credits": {
                    "available_count": 2
                }
            }),
            1_777_000_000,
        )
        .expect("codex reset credit summary should parse");

        assert_eq!(
            parsed.get("rate_limit_reset_credits"),
            Some(&json!({ "available_count": 2u64 }))
        );
    }

    #[test]
    fn preserves_all_codex_windows_by_upstream_duration() {
        let parsed = parse_codex_wham_usage_response(
            &json!({
                "plan_type": "team",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 25.0,
                        "limit_window_seconds": 18_000,
                        "reset_after_seconds": 9000,
                        "reset_at": 1_800_000_000u64
                    },
                    "secondary_window": {
                        "used_percent": 10.0,
                        "limit_window_seconds": 604_800,
                        "reset_after_seconds": 300000,
                        "reset_at": 1_900_000_000u64
                    }
                },
                "additional_rate_limits": [{
                    "limit_name": "Monthly",
                    "rate_limit": {
                        "primary_window": {
                            "used_percent": 5.0,
                            "limit_window_seconds": 2_592_000,
                            "reset_after_seconds": 1_000_000,
                            "reset_at": 2_000_000_000u64
                        }
                    }
                }, {
                    "limit_name": "Experimental",
                    "rate_limit": {
                        "primary_window": {
                            "used_percent": 1.0,
                            "limit_window_seconds": 12_345,
                            "reset_after_seconds": 600,
                            "reset_at": 2_100_000_000u64
                        }
                    }
                }]
            }),
            1_777_000_000,
        )
        .expect("codex wham usage should parse");

        let windows = parsed["windows"]
            .as_array()
            .expect("generic quota windows should exist");
        assert_eq!(windows.len(), 4);
        assert!(windows.iter().any(|window| {
            window["code"] == json!("5h")
                && window["label"] == json!("5H")
                && window["window_seconds"] == json!(18_000u64)
                && window["scope"] == json!("account")
        }));
        assert!(windows.iter().any(|window| {
            window["code"] == json!("weekly")
                && window["label"] == json!("7D")
                && window["window_seconds"] == json!(604_800u64)
                && window["scope"] == json!("account")
        }));
        assert!(windows.iter().any(|window| {
            window["code"] == json!("feature:monthly:1m")
                && window["label"] == json!("Monthly 1M")
                && window["window_seconds"] == json!(2_592_000u64)
                && window["scope"] == json!("feature")
        }));
        assert!(windows.iter().any(|window| {
            window["code"] == json!("feature:experimental:window_12345s")
                && window["label"] == json!("Experimental 3小时26分钟")
                && window["window_seconds"] == json!(12_345u64)
                && window.get("window_minutes").is_none()
                && window["scope"] == json!("feature")
        }));
    }

    #[test]
    fn preserves_codex_monthly_window_when_it_is_the_only_window() {
        let parsed = parse_codex_wham_usage_response(
            &json!({
                "plan_type": "team",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 32.0,
                        "limit_window_seconds": 2_592_000,
                        "reset_after_seconds": 1_000_000,
                        "reset_at": 2_000_000_000u64
                    }
                }
            }),
            1_777_000_000,
        )
        .expect("codex wham usage should parse");

        let windows = parsed["windows"]
            .as_array()
            .expect("generic quota windows should exist");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0]["code"], json!("1m"));
        assert_eq!(windows[0]["label"], json!("1M"));
        assert_eq!(windows[0]["scope"], json!("account"));
        assert_eq!(windows[0]["used_percent"], json!(32.0));
    }

    #[test]
    fn parses_codex_paid_primary_only_window_as_five_hour() {
        let parsed = parse_codex_wham_usage_response(
            &json!({
                "plan_type": "plus",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 12.5,
                        "reset_after_seconds": 18_000,
                        "reset_at": 1_900_000_000u64,
                        "window_minutes": 300
                    }
                }
            }),
            1_777_000_000,
        )
        .expect("codex wham usage should parse");

        assert_eq!(parsed.get("primary_used_percent"), None);
        assert_eq!(parsed.get("secondary_used_percent"), Some(&json!(12.5)));
        assert_eq!(parsed.get("secondary_window_minutes"), Some(&json!(300u64)));
    }

    #[test]
    fn ignores_codex_wham_zero_length_quota_window() {
        let parsed = parse_codex_wham_usage_response(
            &json!({
                "plan_type": "plus",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 0.0,
                        "reset_after_seconds": 0,
                        "reset_at": 1_777_000_000u64,
                        "window_minutes": 0
                    }
                }
            }),
            1_777_000_000,
        )
        .expect("codex wham usage should parse");

        assert_eq!(parsed.get("primary_used_percent"), None);
        assert_eq!(parsed.get("secondary_used_percent"), None);
        assert_eq!(parsed.get("primary_window_minutes"), None);
        assert_eq!(parsed.get("secondary_window_minutes"), None);
        assert_eq!(parsed.get("plan_type"), Some(&json!("plus")));
    }

    #[test]
    fn parses_codex_backend_me_identity_metadata_without_quota_windows() {
        let parsed = parse_codex_backend_me_response(
            &json!({
                "user": {
                    "id": "user-codex-123",
                    "email": "codex@example.com",
                    "name": "Codex User"
                },
                "account": {
                    "id": "acct-codex-123",
                    "name": "Personal",
                    "plan_type": "plus"
                },
                "plan": {
                    "type": "Plus",
                    "title": "ChatGPT Plus"
                }
            }),
            1_777_000_000,
        )
        .expect("codex backend me should parse");

        assert_eq!(parsed.get("user_id"), Some(&json!("user-codex-123")));
        assert_eq!(parsed.get("email"), Some(&json!("codex@example.com")));
        assert_eq!(parsed.get("account_id"), Some(&json!("acct-codex-123")));
        assert_eq!(parsed.get("account_name"), Some(&json!("Personal")));
        assert_eq!(parsed.get("plan_type"), Some(&json!("plus")));
        assert_eq!(parsed.get("plan_title"), Some(&json!("ChatGPT Plus")));
        assert_eq!(parsed.get("updated_at"), Some(&json!(1_777_000_000u64)));
        assert!(parsed.get("primary_used_percent").is_none());
        assert!(parsed.get("secondary_used_percent").is_none());
    }

    #[test]
    fn parses_chatgpt_web_image_quota_from_conversation_init() {
        let parsed = parse_chatgpt_web_conversation_init_response(
            &json!({
                "default_model_slug": "auto",
                "blocked_features": [],
                "limits_progress": [
                    {
                        "feature_name": "image_gen",
                        "remaining": 24,
                        "reset_after": "2026-05-07T12:32:52.826482+00:00"
                    }
                ]
            }),
            1_778_067_246,
        )
        .expect("chatgpt web quota should parse");

        assert_eq!(parsed.get("default_model_slug"), Some(&json!("auto")));
        assert_eq!(parsed.get("image_quota_remaining"), Some(&json!(24.0)));
        assert_eq!(
            parsed.get("image_quota_reset_at"),
            Some(&json!(1_778_157_172u64))
        );
    }

    #[test]
    fn parses_chatgpt_web_blocked_image_feature_as_zero_remaining() {
        let parsed = parse_chatgpt_web_conversation_init_response(
            &json!({
                "blocked_features": ["image_generation"],
                "limits_progress": []
            }),
            1_778_067_246,
        )
        .expect("blocked image feature should produce metadata");

        assert_eq!(parsed.get("image_quota_blocked"), Some(&json!(true)));
        assert_eq!(parsed.get("image_quota_remaining"), Some(&json!(0.0)));
    }
}

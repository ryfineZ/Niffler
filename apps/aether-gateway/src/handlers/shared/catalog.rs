use crate::handlers::shared::{json_string_list, unix_secs_to_rfc3339};
use crate::provider_key_auth::{
    provider_key_auth_semantics, provider_key_can_refresh_oauth,
    provider_key_configured_api_formats, provider_key_inherits_provider_api_formats,
};
use crate::AppState;
use aether_admin::provider::pool as admin_provider_pool_pure;
use aether_admin::provider::quota as admin_provider_quota_pure;
use aether_admin::provider::status as admin_provider_status_pure;
#[cfg(test)]
use aether_crypto::DEVELOPMENT_ENCRYPTION_KEY;
use aether_crypto::{decrypt_python_fernet_ciphertext, encrypt_python_fernet_plaintext};
use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey;
use aether_provider_pool::{
    grok_pool_tier_from_quota_bucket, grok_supported_quota_windows_for_tier,
};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

const OAUTH_ACCOUNT_BLOCK_PREFIX: &str = "[ACCOUNT_BLOCK] ";
const OAUTH_EXPIRED_PREFIX: &str = "[OAUTH_EXPIRED] ";
const OAUTH_REFRESH_FAILED_PREFIX: &str = "[REFRESH_FAILED] ";
const OAUTH_REQUEST_FAILED_PREFIX: &str = "[REQUEST_FAILED] ";

pub(crate) fn provider_catalog_key_supports_format(
    key: &StoredProviderCatalogKey,
    provider_type: &str,
    api_format: &str,
) -> bool {
    if provider_key_inherits_provider_api_formats(key, provider_type) {
        return true;
    }
    let formats = provider_key_configured_api_formats(key);
    if formats.is_empty() {
        return true;
    }
    formats
        .iter()
        .any(|candidate| crate::ai_serving::api_format_alias_matches(candidate, api_format))
}

pub(crate) fn decrypt_catalog_secret_with_fallbacks(
    encryption_key: Option<&str>,
    ciphertext: &str,
) -> Option<String> {
    let encryption_key = encryption_key.map(str::trim).unwrap_or("");
    if !encryption_key.is_empty() {
        if let Ok(value) = decrypt_python_fernet_ciphertext(encryption_key, ciphertext) {
            return Some(value);
        }
    }
    for env_key in ["AETHER_GATEWAY_DATA_ENCRYPTION_KEY", "ENCRYPTION_KEY"] {
        let Ok(fallback) = std::env::var(env_key) else {
            continue;
        };
        let fallback = fallback.trim();
        if fallback.is_empty() || fallback == encryption_key {
            continue;
        }
        if let Ok(value) = decrypt_python_fernet_ciphertext(fallback, ciphertext) {
            return Some(value);
        }
    }
    #[cfg(test)]
    if encryption_key != DEVELOPMENT_ENCRYPTION_KEY {
        if let Ok(value) = decrypt_python_fernet_ciphertext(DEVELOPMENT_ENCRYPTION_KEY, ciphertext)
        {
            return Some(value);
        }
    }
    None
}

pub(crate) fn effective_catalog_encryption_key(state: &AppState) -> Option<Cow<'_, str>> {
    let encryption_key = state.encryption_key().map(str::trim).unwrap_or("");
    if !encryption_key.is_empty() {
        return Some(Cow::Borrowed(encryption_key));
    }
    for env_key in ["AETHER_GATEWAY_DATA_ENCRYPTION_KEY", "ENCRYPTION_KEY"] {
        let Ok(candidate) = std::env::var(env_key) else {
            continue;
        };
        let trimmed = candidate.trim();
        if !trimmed.is_empty() {
            return Some(if trimmed.len() == candidate.len() {
                Cow::Owned(candidate)
            } else {
                Cow::Owned(trimmed.to_string())
            });
        }
    }
    #[cfg(test)]
    {
        return Some(Cow::Borrowed(DEVELOPMENT_ENCRYPTION_KEY));
    }
    #[allow(unreachable_code)]
    None
}

pub(crate) fn encrypt_catalog_secret_with_fallbacks(
    state: &AppState,
    plaintext: &str,
) -> Option<String> {
    let encryption_key = effective_catalog_encryption_key(state)?;
    encrypt_python_fernet_plaintext(encryption_key.as_ref(), plaintext).ok()
}

pub(crate) fn take_secret_prefix(value: &str, prefix_chars: usize) -> &str {
    let end = value
        .char_indices()
        .nth(prefix_chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    &value[..end]
}

pub(crate) fn take_secret_suffix(value: &str, suffix_chars: usize) -> &str {
    if suffix_chars == 0 {
        return &value[value.len()..];
    }

    let start = value
        .char_indices()
        .rev()
        .nth(suffix_chars - 1)
        .map(|(index, _)| index)
        .unwrap_or(0);
    &value[start..]
}

pub(crate) fn masked_catalog_api_key(state: &AppState, key: &StoredProviderCatalogKey) -> String {
    match key.auth_type.trim() {
        "service_account" | "vertex_ai" => "[Service Account]".to_string(),
        "oauth" => "[OAuth Token]".to_string(),
        _ => {
            let Some(ciphertext) = key
                .encrypted_api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                return "[未设置]".to_string();
            };
            decrypt_catalog_secret_with_fallbacks(state.encryption_key(), ciphertext)
                .map(|value| {
                    if value.chars().count() <= 12 {
                        format!("{value}***")
                    } else {
                        format!(
                            "{}***{}",
                            take_secret_prefix(&value, 8),
                            take_secret_suffix(&value, 4)
                        )
                    }
                })
                .unwrap_or_else(|| "***ERROR***".to_string())
        }
    }
}

pub(crate) fn parse_catalog_auth_config_json(
    state: &AppState,
    key: &StoredProviderCatalogKey,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let ciphertext = key.encrypted_auth_config.as_deref()?.trim();
    if ciphertext.is_empty() {
        return None;
    }
    let plaintext = decrypt_catalog_secret_with_fallbacks(state.encryption_key(), ciphertext)?;
    serde_json::from_str::<serde_json::Value>(&plaintext)
        .ok()?
        .as_object()
        .cloned()
}

pub(crate) fn default_provider_key_status_snapshot() -> serde_json::Value {
    json!({
        "oauth": {
            "code": "none",
            "label": serde_json::Value::Null,
            "reason": serde_json::Value::Null,
            "expires_at": serde_json::Value::Null,
            "invalid_at": serde_json::Value::Null,
            "source": serde_json::Value::Null,
            "requires_reauth": false,
            "expiring_soon": false,
        },
        "account": {
            "code": "ok",
            "label": serde_json::Value::Null,
            "reason": serde_json::Value::Null,
            "blocked": false,
            "source": serde_json::Value::Null,
            "recoverable": false,
        },
        "quota": {
            "code": "unknown",
            "label": serde_json::Value::Null,
            "reason": serde_json::Value::Null,
            "exhausted": false,
            "usage_ratio": serde_json::Value::Null,
            "updated_at": serde_json::Value::Null,
            "reset_seconds": serde_json::Value::Null,
            "plan_type": serde_json::Value::Null,
        }
    })
}

fn default_oauth_status_snapshot_value() -> Value {
    default_provider_key_status_snapshot()
        .get("oauth")
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "code": "none",
                "label": Value::Null,
                "reason": Value::Null,
                "expires_at": Value::Null,
                "invalid_at": Value::Null,
                "source": Value::Null,
                "requires_reauth": false,
                "expiring_soon": false,
            })
        })
}

fn trimmed_oauth_invalid_reason(reason: Option<&str>) -> Option<String> {
    reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn tagged_oauth_invalid_reason(reason: Option<&str>, prefix: &str) -> Option<String> {
    reason.and_then(|value| {
        value
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix(prefix))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn oauth_access_token_expired(expires_at_unix_secs: Option<u64>, now_unix_secs: u64) -> bool {
    expires_at_unix_secs.is_none_or(|expires_at| expires_at == 0 || expires_at <= now_unix_secs)
}

fn build_provider_key_oauth_status_snapshot(key: &StoredProviderCatalogKey) -> Value {
    if !key.auth_type.trim().eq_ignore_ascii_case("oauth") {
        return default_oauth_status_snapshot_value();
    }

    let now_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let expires_at_unix_secs = key.expires_at_unix_secs;
    let invalid_at_unix_secs = key.oauth_invalid_at_unix_secs;
    let invalid_reason = trimmed_oauth_invalid_reason(key.oauth_invalid_reason.as_deref());

    if let Some(reason) =
        tagged_oauth_invalid_reason(invalid_reason.as_deref(), OAUTH_EXPIRED_PREFIX)
    {
        return json!({
            "code": "invalid",
            "label": "已失效",
            "reason": reason,
            "expires_at": expires_at_unix_secs,
            "invalid_at": invalid_at_unix_secs,
            "source": "oauth_invalid",
            "requires_reauth": true,
            "expiring_soon": false,
        });
    }
    if let Some(reason) =
        tagged_oauth_invalid_reason(invalid_reason.as_deref(), OAUTH_REFRESH_FAILED_PREFIX)
    {
        let access_token_expired = oauth_access_token_expired(expires_at_unix_secs, now_unix_secs);
        return json!({
            "code": if access_token_expired { "invalid" } else { "reauth_required" },
            "label": if access_token_expired { "已失效" } else { "续期失败" },
            "reason": reason,
            "expires_at": expires_at_unix_secs,
            "invalid_at": invalid_at_unix_secs,
            "source": "oauth_refresh",
            "requires_reauth": true,
            "usable_until_expiry": !access_token_expired,
            "expiring_soon": false,
        });
    }
    if let Some(reason) =
        tagged_oauth_invalid_reason(invalid_reason.as_deref(), OAUTH_REQUEST_FAILED_PREFIX)
    {
        return json!({
            "code": "check_failed",
            "label": "检查失败",
            "reason": reason,
            "expires_at": expires_at_unix_secs,
            "invalid_at": Value::Null,
            "source": "oauth_request",
            "requires_reauth": false,
            "expiring_soon": false,
        });
    }
    if invalid_reason
        .as_deref()
        .is_some_and(|reason| !reason.starts_with(OAUTH_ACCOUNT_BLOCK_PREFIX))
        || invalid_at_unix_secs.is_some()
    {
        return json!({
            "code": "invalid",
            "label": "已失效",
            "reason": invalid_reason,
            "expires_at": expires_at_unix_secs,
            "invalid_at": invalid_at_unix_secs,
            "source": "oauth_invalid",
            "requires_reauth": true,
            "expiring_soon": false,
        });
    }

    let Some(expires_at_unix_secs) = expires_at_unix_secs else {
        return default_oauth_status_snapshot_value();
    };
    if expires_at_unix_secs <= now_unix_secs {
        return json!({
            "code": "expired",
            "label": "已过期",
            "reason": "Access Token 已过期，等待自动续期",
            "expires_at": expires_at_unix_secs,
            "invalid_at": Value::Null,
            "source": "expires_at",
            "requires_reauth": false,
            "expiring_soon": false,
        });
    }

    let expiring_soon = expires_at_unix_secs.saturating_sub(now_unix_secs) < 24 * 60 * 60;
    json!({
        "code": if expiring_soon { "expiring" } else { "valid" },
        "label": if expiring_soon { "即将过期" } else { "有效" },
        "reason": Value::Null,
        "expires_at": expires_at_unix_secs,
        "invalid_at": Value::Null,
        "source": "expires_at",
        "requires_reauth": false,
        "expiring_soon": expiring_soon,
    })
}

pub(crate) fn sync_provider_key_oauth_status_snapshot(
    status_snapshot: Option<&Value>,
    key: &StoredProviderCatalogKey,
) -> Option<Value> {
    let mut snapshot = provider_key_status_snapshot_object(status_snapshot)
        .or_else(|| default_provider_key_status_snapshot().as_object().cloned())
        .unwrap_or_default();
    snapshot.insert(
        "oauth".to_string(),
        build_provider_key_oauth_status_snapshot(key),
    );
    Some(Value::Object(snapshot))
}

fn build_provider_key_account_status_snapshot(
    key: &StoredProviderCatalogKey,
    provider_type: &str,
) -> Value {
    let snapshot = admin_provider_status_pure::resolve_account_status_snapshot(
        Some(provider_type),
        key.upstream_metadata.as_ref(),
        key.oauth_invalid_reason.as_deref(),
    );
    json!({
        "code": snapshot.code,
        "label": snapshot.label,
        "reason": snapshot.reason,
        "blocked": snapshot.blocked,
        "source": snapshot.source,
        "recoverable": snapshot.recoverable,
    })
}

fn provider_key_status_snapshot_object(
    status_snapshot: Option<&Value>,
) -> Option<Map<String, Value>> {
    status_snapshot.and_then(|value| match value {
        Value::Object(object) => Some(object.clone()),
        _ => None,
    })
}

fn provider_quota_metadata_bucket<'a>(
    upstream_metadata: Option<&'a Value>,
    provider_type: &str,
) -> Option<&'a Map<String, Value>> {
    upstream_metadata
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(&provider_type.trim().to_ascii_lowercase()))
        .and_then(Value::as_object)
}

fn is_codex_quota_metadata_key(key: &str) -> bool {
    matches!(
        key,
        "has_credits"
            | "credits_balance"
            | "credits_unlimited"
            | "rate_limit_reset_credits"
            | "primary_over_secondary_limit_percent"
            | "windows"
    ) || [
        "primary_",
        "secondary_",
        "spark_primary_",
        "spark_secondary_",
    ]
    .iter()
    .any(|prefix| key.starts_with(prefix))
}

pub(crate) fn codex_metadata_has_quota_evidence(metadata: &Map<String, Value>) -> bool {
    metadata.keys().any(|key| is_codex_quota_metadata_key(key))
}

pub(crate) fn merge_codex_metadata_bucket(current: Option<&Value>, incoming: &Value) -> Value {
    let mut merged = current
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let Some(incoming_object) = incoming.as_object() else {
        return Value::Object(merged);
    };

    if codex_metadata_has_quota_evidence(incoming_object) {
        merged.retain(|key, _| !is_codex_quota_metadata_key(key));
    }
    for (key, value) in incoming_object {
        merged.insert(key.clone(), value.clone());
    }
    Value::Object(merged)
}

fn codex_quota_window_is_account_scoped(window: &Value) -> bool {
    let Some(window) = window.as_object() else {
        return false;
    };
    let scope = window
        .get("scope")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if !scope.is_empty() {
        return scope.eq_ignore_ascii_case("account");
    }

    let code = window
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    !code.starts_with("spark:") && !code.starts_with("spark_") && !code.starts_with("feature:")
}

fn codex_quota_window_code(window: &Value) -> Option<&str> {
    window
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty())
}

fn rebase_codex_preserved_window_countdown(
    window: &mut Value,
    current_observed_at: Option<u64>,
    incoming_observed_at: Option<u64>,
) {
    let Some(window) = window.as_object_mut() else {
        return;
    };
    let reset_at = window
        .get("reset_at")
        .and_then(admin_provider_quota_pure::coerce_json_u64)
        .or_else(|| {
            current_observed_at
                .zip(
                    window
                        .get("reset_after_seconds")
                        .and_then(admin_provider_quota_pure::coerce_json_u64),
                )
                .and_then(|(observed_at, reset_after)| observed_at.checked_add(reset_after))
        });
    let Some(reset_at) = reset_at else {
        return;
    };
    window.insert("reset_at".to_string(), json!(reset_at));
    if let Some(observed_at) = incoming_observed_at {
        window.insert(
            "reset_after_seconds".to_string(),
            json!(reset_at.saturating_sub(observed_at)),
        );
    }
}

fn merge_codex_response_header_windows(
    current: Option<&Map<String, Value>>,
    incoming: &Map<String, Value>,
) -> Value {
    let current_windows = current
        .and_then(|current| current.get("windows"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let incoming_windows = incoming
        .get("windows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let incoming_has_account_windows = incoming_windows
        .iter()
        .any(codex_quota_window_is_account_scoped);
    let current_observed_at = current
        .and_then(|current| current.get("updated_at"))
        .and_then(admin_provider_quota_pure::coerce_json_u64);
    let incoming_observed_at = incoming
        .get("updated_at")
        .and_then(admin_provider_quota_pure::coerce_json_u64);
    let mut merged = incoming_windows;

    for mut current_window in current_windows {
        if incoming_has_account_windows && codex_quota_window_is_account_scoped(&current_window) {
            continue;
        }
        let duplicate = codex_quota_window_code(&current_window).is_some_and(|current_code| {
            merged.iter().any(|incoming_window| {
                codex_quota_window_code(incoming_window)
                    .is_some_and(|incoming_code| incoming_code.eq_ignore_ascii_case(current_code))
            })
        });
        if !duplicate {
            rebase_codex_preserved_window_countdown(
                &mut current_window,
                current_observed_at,
                incoming_observed_at,
            );
            merged.push(current_window);
        }
    }

    Value::Array(merged)
}

pub(crate) fn merge_codex_response_header_metadata_bucket(
    current: Option<&Value>,
    incoming: &Value,
) -> Value {
    let current_object = current.and_then(Value::as_object);
    let mut merged = current_object.cloned().unwrap_or_default();
    let Some(incoming_object) = incoming.as_object() else {
        return Value::Object(merged);
    };

    let incoming_has_account_quota = incoming_object.keys().any(|key| {
        key.starts_with("primary_")
            || key.starts_with("secondary_")
            || key == "primary_over_secondary_limit_percent"
    }) || incoming_object
        .get("windows")
        .and_then(Value::as_array)
        .is_some_and(|windows| windows.iter().any(codex_quota_window_is_account_scoped));
    if incoming_has_account_quota {
        merged.retain(|key, _| {
            !key.starts_with("primary_")
                && !key.starts_with("secondary_")
                && key != "primary_over_secondary_limit_percent"
        });
    }

    for (key, value) in incoming_object {
        if key == "windows" {
            let next = merge_codex_response_header_windows(current_object, incoming_object);
            merged.insert(key.clone(), next);
            continue;
        }
        merged.insert(key.clone(), value.clone());
    }
    Value::Object(merged)
}

fn provider_quota_timestamp_unix_secs(value: Option<&Value>) -> Option<u64> {
    let mut parsed = match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return None;
    }
    if parsed > 1_000_000_000_000.0 {
        parsed /= 1000.0;
    }
    Some(parsed.floor() as u64)
}

fn provider_quota_model_bucket(metadata: &Map<String, Value>) -> Option<&Map<String, Value>> {
    metadata
        .get("quota_by_model")
        .or_else(|| metadata.get("models"))
        .and_then(Value::as_object)
}

fn quota_window_reset_seconds(
    observed_at_unix_secs: Option<u64>,
    reset_at_unix_secs: Option<u64>,
) -> Option<u64> {
    observed_at_unix_secs
        .zip(reset_at_unix_secs)
        .map(|(observed_at, reset_at)| reset_at.saturating_sub(observed_at))
}

fn chatgpt_web_image_quota_limit(
    metadata: &Map<String, Value>,
    remaining: Option<f64>,
) -> Option<f64> {
    let plan_type = metadata
        .get("plan_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    if plan_type.as_deref() == Some("free") {
        return Some(25.0);
    }

    let explicit_limit = metadata
        .get("image_quota_total")
        .and_then(admin_provider_quota_pure::coerce_json_f64)
        .filter(|value| *value > 0.0);
    if let Some(limit) = explicit_limit {
        return Some(limit);
    }

    remaining.filter(|value| *value > 0.0)
}

fn model_quota_window_snapshot(
    model_name: &str,
    item: &Map<String, Value>,
    observed_at_unix_secs: Option<u64>,
) -> Option<Value> {
    let remaining_value = item
        .get("remaining")
        .or_else(|| item.get("remaining_value"))
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let limit_value = item
        .get("total")
        .or_else(|| item.get("limit_value"))
        .and_then(admin_provider_quota_pure::coerce_json_f64)
        .filter(|value| *value > 0.0);
    let used_value = item
        .get("used")
        .or_else(|| item.get("used_value"))
        .and_then(admin_provider_quota_pure::coerce_json_f64)
        .or_else(|| {
            remaining_value
                .zip(limit_value)
                .map(|(remaining, limit)| (limit - remaining).max(0.0))
        });
    let used_ratio = item
        .get("used_percent")
        .and_then(admin_provider_quota_pure::coerce_json_f64)
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
        .or_else(|| {
            item.get("remaining_fraction")
                .and_then(admin_provider_quota_pure::coerce_json_f64)
                .map(|value| (1.0 - value.clamp(0.0, 1.0)).clamp(0.0, 1.0))
        });
    let remaining_ratio = item
        .get("remaining_fraction")
        .and_then(admin_provider_quota_pure::coerce_json_f64)
        .map(|value| value.clamp(0.0, 1.0))
        .or_else(|| used_ratio.map(|value| (1.0 - value).max(0.0)));
    let reset_at = provider_quota_timestamp_unix_secs(
        item.get("reset_at").or_else(|| item.get("next_reset_at")),
    );
    let reset_seconds = quota_window_reset_seconds(observed_at_unix_secs, reset_at);
    let is_exhausted = item
        .get("is_exhausted")
        .and_then(admin_provider_quota_pure::coerce_json_bool)
        .or_else(|| used_ratio.map(|value| value >= 1.0 - 1e-6));

    if used_ratio.is_none()
        && remaining_ratio.is_none()
        && reset_at.is_none()
        && reset_seconds.is_none()
        && is_exhausted.is_none()
        && remaining_value.is_none()
        && limit_value.is_none()
    {
        return None;
    }

    let mut window = Map::new();
    let label = item
        .get("display_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(model_name);
    window.insert("code".to_string(), json!(format!("model:{model_name}")));
    window.insert("label".to_string(), json!(label));
    window.insert("scope".to_string(), json!("model"));
    window.insert("unit".to_string(), json!("percent"));
    window.insert("model".to_string(), json!(model_name));
    window.insert("used_ratio".to_string(), json!(used_ratio));
    window.insert("remaining_ratio".to_string(), json!(remaining_ratio));
    window.insert("used_value".to_string(), json!(used_value));
    window.insert("remaining_value".to_string(), json!(remaining_value));
    window.insert("limit_value".to_string(), json!(limit_value));
    window.insert("reset_at".to_string(), json!(reset_at));
    window.insert("reset_seconds".to_string(), json!(reset_seconds));
    window.insert("is_exhausted".to_string(), json!(is_exhausted));
    Some(Value::Object(window))
}

fn provider_quota_metadata_string(
    metadata: &Map<String, Value>,
    fields: &[&str],
) -> Option<String> {
    fields.iter().find_map(|field| {
        metadata
            .get(*field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn quota_windows_usage_ratio(windows: &[Value]) -> Option<f64> {
    windows
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|window| window.get("used_ratio"))
        .filter_map(Value::as_f64)
        .max_by(f64::total_cmp)
}

fn quota_windows_min_reset_seconds(windows: &[Value]) -> Option<u64> {
    windows
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|window| window.get("reset_seconds"))
        .filter_map(admin_provider_quota_pure::coerce_json_u64)
        .min()
}

fn quota_windows_min_reset_at(windows: &[Value]) -> Option<u64> {
    windows
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|window| window.get("reset_at"))
        .filter_map(|value| provider_quota_timestamp_unix_secs(Some(value)))
        .min()
}

fn quota_windows_all_exhausted(windows: &[Value]) -> bool {
    let mut total = 0usize;
    let mut exhausted = 0usize;
    for window in windows.iter().filter_map(Value::as_object) {
        total += 1;
        let is_exhausted = window
            .get("is_exhausted")
            .and_then(admin_provider_quota_pure::coerce_json_bool)
            .or_else(|| {
                window
                    .get("used_ratio")
                    .and_then(Value::as_f64)
                    .map(|value| value >= 1.0 - 1e-6)
            })
            .unwrap_or(false);
        if is_exhausted {
            exhausted += 1;
        }
    }
    total > 0 && exhausted == total
}

fn provider_key_status_snapshot_now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn normalize_elapsed_codex_quota_windows_at(quota: &mut Value, now_unix_secs: u64) {
    let Some(windows) = quota.get_mut("windows").and_then(Value::as_array_mut) else {
        return;
    };

    for window in windows.iter_mut().filter_map(Value::as_object_mut) {
        let Some(reset_at) = window
            .get("reset_at")
            .and_then(admin_provider_quota_pure::coerce_json_u64)
        else {
            continue;
        };
        if reset_at > now_unix_secs {
            continue;
        }

        window.insert("used_ratio".to_string(), json!(0.0));
        window.insert("remaining_ratio".to_string(), json!(1.0));
        window.insert("is_exhausted".to_string(), json!(false));
        window.insert("reset_seconds".to_string(), json!(0u64));
        window.remove("usage");
    }
}

fn codex_quota_window_is_primary(window: &Map<String, Value>) -> bool {
    let scope = window
        .get("scope")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("account");
    if scope.eq_ignore_ascii_case("feature")
        || scope.eq_ignore_ascii_case("model")
        || scope.eq_ignore_ascii_case("workspace")
    {
        return false;
    }
    !window
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|code| code.starts_with("spark:") || code.starts_with("spark_"))
}

fn refresh_codex_quota_summary_from_windows(quota: &mut Value) {
    let Some(quota_object) = quota.as_object_mut() else {
        return;
    };
    let Some(windows) = quota_object.get("windows").and_then(Value::as_array) else {
        return;
    };

    let primary_windows = windows
        .iter()
        .filter_map(Value::as_object)
        .filter(|window| codex_quota_window_is_primary(window))
        .collect::<Vec<_>>();
    let usage_ratio = primary_windows
        .iter()
        .filter_map(|window| window.get("used_ratio"))
        .filter_map(Value::as_f64)
        .max_by(f64::total_cmp);
    let credits = quota_object.get("credits").and_then(Value::as_object);
    let credits_has_credits = credits
        .and_then(|credits| credits.get("has_credits"))
        .and_then(admin_provider_quota_pure::coerce_json_bool);
    let credits_unlimited = credits
        .and_then(|credits| credits.get("unlimited"))
        .and_then(admin_provider_quota_pure::coerce_json_bool);
    let exhausted_by_credits = primary_windows.is_empty()
        && credits_unlimited != Some(true)
        && credits_has_credits == Some(false);
    let exhausted_by_window = usage_ratio.is_some_and(|value| value >= 1.0 - 1e-6);
    let exhausted = exhausted_by_credits || exhausted_by_window;

    quota_object.insert("usage_ratio".to_string(), json!(usage_ratio));
    quota_object.insert("exhausted".to_string(), json!(exhausted));
    quota_object.insert(
        "code".to_string(),
        json!(if exhausted { "exhausted" } else { "ok" }),
    );
    quota_object.insert(
        "label".to_string(),
        if exhausted {
            json!("额度耗尽")
        } else {
            Value::Null
        },
    );
    quota_object.insert(
        "reason".to_string(),
        if exhausted_by_credits {
            json!("无可用积分")
        } else if exhausted_by_window {
            json!("额度窗口已耗尽")
        } else {
            Value::Null
        },
    );
}

fn normalize_codex_quota_windows_for_display_at(quota: &mut Value, now_unix_secs: u64) {
    normalize_elapsed_codex_quota_windows_at(quota, now_unix_secs);
    refresh_codex_quota_summary_from_windows(quota);
}

fn preserve_quota_window_duration(current_status_snapshot: Option<&Value>, quota: &mut Value) {
    let Some(current_windows) = current_status_snapshot
        .and_then(Value::as_object)
        .and_then(|snapshot| snapshot.get("quota"))
        .and_then(Value::as_object)
        .and_then(|quota| quota.get("windows"))
        .and_then(Value::as_array)
    else {
        return;
    };
    let Some(next_windows) = quota.get_mut("windows").and_then(Value::as_array_mut) else {
        return;
    };

    for next_window in next_windows.iter_mut().filter_map(Value::as_object_mut) {
        let Some(code) = next_window
            .get("code")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|code| !code.is_empty())
        else {
            continue;
        };
        let current_window =
            current_windows
                .iter()
                .filter_map(Value::as_object)
                .find(|current_window| {
                    current_window
                        .get("code")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .is_some_and(|current_code| current_code.eq_ignore_ascii_case(code))
                });
        let Some(current_window) = current_window else {
            continue;
        };
        let current_reset_at = current_window
            .get("reset_at")
            .and_then(admin_provider_quota_pure::coerce_json_u64);
        let next_reset_at = next_window
            .get("reset_at")
            .and_then(admin_provider_quota_pure::coerce_json_u64);
        if current_reset_at.is_none() || current_reset_at != next_reset_at {
            continue;
        }

        if next_window
            .get("window_minutes")
            .and_then(admin_provider_quota_pure::coerce_json_u64)
            .is_none()
        {
            if let Some(window_minutes) = current_window
                .get("window_minutes")
                .and_then(admin_provider_quota_pure::coerce_json_u64)
                .or_else(|| codex_default_window_minutes(code))
            {
                next_window.insert("window_minutes".to_string(), json!(window_minutes));
            }
        }
    }
}

fn codex_default_window_minutes(code: &str) -> Option<u64> {
    if code.eq_ignore_ascii_case("5h") || code.eq_ignore_ascii_case("spark_5h") {
        Some(300)
    } else if code.eq_ignore_ascii_case("weekly")
        || code.eq_ignore_ascii_case("7d")
        || code.eq_ignore_ascii_case("spark_weekly")
        || code.eq_ignore_ascii_case("spark_7d")
    {
        Some(10_080)
    } else if code.eq_ignore_ascii_case("1m") || code.eq_ignore_ascii_case("monthly") {
        Some(43_200)
    } else {
        None
    }
}

fn codex_quota_window_snapshot_from_metadata(
    item: &Map<String, Value>,
    observed_at_unix_secs: Option<u64>,
) -> Option<Value> {
    let code = item
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let label = item
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(code);
    let used_percent = item
        .get("used_percent")
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let explicit_reset_at = item
        .get("reset_at")
        .and_then(admin_provider_quota_pure::coerce_json_u64);
    let reset_seconds = item
        .get("reset_after_seconds")
        .or_else(|| item.get("reset_seconds"))
        .and_then(admin_provider_quota_pure::coerce_json_u64)
        .or_else(|| {
            observed_at_unix_secs
                .zip(explicit_reset_at)
                .map(|(observed_at, reset_at)| reset_at.saturating_sub(observed_at))
        });
    let reset_at = explicit_reset_at.or_else(|| {
        observed_at_unix_secs
            .zip(reset_seconds)
            .map(|(observed_at, reset_seconds)| observed_at.saturating_add(reset_seconds))
    });
    let window_seconds = item
        .get("window_seconds")
        .and_then(admin_provider_quota_pure::coerce_json_u64);
    let window_minutes = item
        .get("window_minutes")
        .and_then(admin_provider_quota_pure::coerce_json_u64)
        .or_else(|| {
            window_seconds
                .filter(|seconds| seconds % 60 == 0)
                .map(|seconds| seconds / 60)
        })
        .or_else(|| codex_default_window_minutes(code));
    let used_ratio = used_percent.map(|value| (value / 100.0).clamp(0.0, 1.0));
    let remaining_ratio = used_ratio.map(|value| (1.0 - value).max(0.0));
    let is_exhausted = item
        .get("is_exhausted")
        .and_then(admin_provider_quota_pure::coerce_json_bool)
        .or_else(|| used_ratio.map(|value| value >= 1.0 - 1e-6));

    if used_ratio.is_none()
        && reset_at.is_none()
        && window_minutes.is_none()
        && is_exhausted.is_none()
    {
        return None;
    }

    let mut window = Map::new();
    window.insert("code".to_string(), json!(code));
    window.insert("label".to_string(), json!(label));
    window.insert(
        "scope".to_string(),
        item.get("scope")
            .cloned()
            .unwrap_or_else(|| json!("account")),
    );
    window.insert("unit".to_string(), json!("percent"));
    window.insert("used_ratio".to_string(), json!(used_ratio));
    window.insert("remaining_ratio".to_string(), json!(remaining_ratio));
    window.insert("reset_at".to_string(), json!(reset_at));
    window.insert("reset_seconds".to_string(), json!(reset_seconds));
    window.insert(
        "window_seconds".to_string(),
        json!(window_seconds.or_else(|| window_minutes.and_then(|minutes| minutes.checked_mul(60)))),
    );
    window.insert("window_minutes".to_string(), json!(window_minutes));
    window.insert("is_exhausted".to_string(), json!(is_exhausted));
    Some(Value::Object(window))
}

fn codex_quota_window_has_materialized_limit(
    used_percent: Option<f64>,
    reset_at: Option<u64>,
    reset_seconds: Option<u64>,
    explicit_window_minutes: Option<u64>,
    observed_at_unix_secs: Option<u64>,
) -> bool {
    if explicit_window_minutes.is_some_and(|minutes| minutes > 0) {
        return true;
    }
    if reset_seconds.is_some_and(|seconds| seconds > 0) {
        return true;
    }
    if reset_at.is_some_and(|reset_at| {
        observed_at_unix_secs
            .map(|observed_at| reset_at > observed_at)
            .unwrap_or(reset_at > 0)
    }) {
        return true;
    }
    used_percent.is_some_and(|used_percent| used_percent > 0.0)
}

fn codex_quota_window_snapshot(
    metadata: &Map<String, Value>,
    prefix: &str,
    code: &str,
    label: &str,
    observed_at_unix_secs: Option<u64>,
) -> Option<Value> {
    let used_percent_key = format!("{prefix}_used_percent");
    let reset_seconds_key = format!("{prefix}_reset_seconds");
    let reset_after_seconds_key = format!("{prefix}_reset_after_seconds");
    let reset_at_key = format!("{prefix}_reset_at");
    let window_minutes_key = format!("{prefix}_window_minutes");

    let used_percent = metadata
        .get(&used_percent_key)
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let explicit_reset_at = metadata
        .get(&reset_at_key)
        .and_then(admin_provider_quota_pure::coerce_json_u64);
    let reset_seconds = metadata
        .get(&reset_seconds_key)
        .and_then(admin_provider_quota_pure::coerce_json_u64)
        .or_else(|| {
            metadata
                .get(&reset_after_seconds_key)
                .and_then(admin_provider_quota_pure::coerce_json_u64)
        })
        .or_else(|| {
            observed_at_unix_secs
                .zip(explicit_reset_at)
                .map(|(observed_at, reset_at)| reset_at.saturating_sub(observed_at))
        });
    let reset_at = explicit_reset_at.or_else(|| {
        observed_at_unix_secs
            .zip(reset_seconds)
            .map(|(observed_at, reset_seconds)| observed_at.saturating_add(reset_seconds))
    });
    let explicit_window_minutes = metadata
        .get(&window_minutes_key)
        .and_then(admin_provider_quota_pure::coerce_json_u64);

    if !codex_quota_window_has_materialized_limit(
        used_percent,
        reset_at,
        reset_seconds,
        explicit_window_minutes,
        observed_at_unix_secs,
    ) {
        return None;
    }

    let window_minutes = explicit_window_minutes.or_else(|| codex_default_window_minutes(code));
    let used_ratio = used_percent.map(|value| (value / 100.0).clamp(0.0, 1.0));
    let remaining_ratio = used_ratio.map(|value| (1.0 - value).max(0.0));

    let mut window = Map::new();
    window.insert("code".to_string(), json!(code));
    window.insert("label".to_string(), json!(label));
    window.insert("scope".to_string(), json!("account"));
    window.insert("unit".to_string(), json!("percent"));
    window.insert("used_ratio".to_string(), json!(used_ratio));
    window.insert("remaining_ratio".to_string(), json!(remaining_ratio));
    window.insert("reset_at".to_string(), json!(reset_at));
    window.insert("reset_seconds".to_string(), json!(reset_seconds));
    window.insert("window_minutes".to_string(), json!(window_minutes));
    Some(Value::Object(window))
}

fn build_codex_quota_status_snapshot(
    upstream_metadata: Option<&Value>,
    source: &str,
) -> Option<Value> {
    let metadata = provider_quota_metadata_bucket(upstream_metadata, "codex")?;
    let observed_at_unix_secs = metadata
        .get("updated_at")
        .and_then(admin_provider_quota_pure::coerce_json_u64);
    let plan_type = metadata
        .get("plan_type")
        .and_then(Value::as_str)
        .and_then(|value| admin_provider_quota_pure::normalize_codex_plan_type(Some(value)));
    let credits_has_credits = metadata
        .get("has_credits")
        .and_then(admin_provider_quota_pure::coerce_json_bool);
    let credits_balance = metadata
        .get("credits_balance")
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let credits_unlimited = metadata
        .get("credits_unlimited")
        .and_then(admin_provider_quota_pure::coerce_json_bool);
    let reset_credits_available_count = metadata
        .get("rate_limit_reset_credits")
        .and_then(Value::as_object)
        .and_then(|credits| credits.get("available_count"))
        .and_then(admin_provider_quota_pure::coerce_json_u64);

    let windows = metadata
        .get("windows")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_object)
                .filter_map(|item| {
                    codex_quota_window_snapshot_from_metadata(item, observed_at_unix_secs)
                })
                .collect::<Vec<_>>()
        })
        .filter(|windows| !windows.is_empty())
        .unwrap_or_else(|| {
            [
                codex_quota_window_snapshot(
                    metadata,
                    "primary",
                    "weekly",
                    "7D",
                    observed_at_unix_secs,
                ),
                codex_quota_window_snapshot(
                    metadata,
                    "secondary",
                    "5h",
                    "5H",
                    observed_at_unix_secs,
                ),
                codex_quota_window_snapshot(
                    metadata,
                    "spark_primary",
                    "spark_5h",
                    "Spark 5H",
                    observed_at_unix_secs,
                ),
                codex_quota_window_snapshot(
                    metadata,
                    "spark_secondary",
                    "spark_weekly",
                    "Spark 7D",
                    observed_at_unix_secs,
                ),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
        });

    if windows.is_empty()
        && plan_type.is_none()
        && credits_has_credits.is_none()
        && credits_balance.is_none()
        && credits_unlimited.is_none()
        && reset_credits_available_count.is_none()
        && observed_at_unix_secs.is_none()
    {
        return None;
    }

    let primary_windows = windows
        .iter()
        .filter_map(Value::as_object)
        .filter(|window| codex_quota_window_is_primary(window))
        .cloned()
        .map(Value::Object)
        .collect::<Vec<_>>();
    let usage_ratio = primary_windows
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|window| window.get("used_ratio"))
        .filter_map(Value::as_f64)
        .max_by(f64::total_cmp);
    let reset_seconds = primary_windows
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|window| window.get("reset_seconds"))
        .filter_map(admin_provider_quota_pure::coerce_json_u64)
        .min();
    let reset_at = quota_windows_min_reset_at(&primary_windows);
    let exhausted_by_credits = primary_windows.is_empty()
        && credits_unlimited != Some(true)
        && credits_has_credits == Some(false);
    let exhausted_by_window = usage_ratio.is_some_and(|value| value >= 1.0 - 1e-6);
    let exhausted = exhausted_by_credits || exhausted_by_window;

    let mut credits = Map::new();
    if let Some(value) = credits_has_credits {
        credits.insert("has_credits".to_string(), json!(value));
    }
    if let Some(value) = credits_balance {
        credits.insert("balance".to_string(), json!(value));
    }
    if let Some(value) = credits_unlimited {
        credits.insert("unlimited".to_string(), json!(value));
    }

    let reason = if exhausted_by_credits {
        Some("无可用积分")
    } else if exhausted_by_window {
        Some("额度窗口已耗尽")
    } else {
        None
    };

    Some(json!({
        "version": 2,
        "provider_type": "codex",
        "code": if exhausted { "exhausted" } else { "ok" },
        "label": if exhausted { Some("额度耗尽") } else { None::<&str> },
        "reason": reason,
        "freshness": "fresh",
        "source": source,
        "observed_at": observed_at_unix_secs,
        "exhausted": exhausted,
        "usage_ratio": usage_ratio,
        "updated_at": observed_at_unix_secs,
        "reset_at": reset_at,
        "reset_seconds": reset_seconds,
        "plan_type": plan_type,
        "credits": if credits.is_empty() {
            Value::Null
        } else {
            Value::Object(credits)
        },
        "reset_credits": reset_credits_available_count.map(|available_count| json!({
            "available_count": available_count,
        })),
        "windows": windows,
    }))
}

fn build_kiro_quota_status_snapshot(
    upstream_metadata: Option<&Value>,
    source: &str,
) -> Option<Value> {
    let metadata = provider_quota_metadata_bucket(upstream_metadata, "kiro")?;
    let observed_at_unix_secs = provider_quota_timestamp_unix_secs(metadata.get("updated_at"));
    let usage_limit = metadata
        .get("usage_limit")
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let current_usage = metadata
        .get("current_usage")
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let remaining = metadata
        .get("remaining")
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let usage_ratio = metadata
        .get("usage_percentage")
        .and_then(admin_provider_quota_pure::coerce_json_f64)
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
        .or_else(|| {
            current_usage
                .zip(usage_limit)
                .and_then(|(current_usage, usage_limit)| {
                    (usage_limit > 0.0).then_some((current_usage / usage_limit).clamp(0.0, 1.0))
                })
        });
    let remaining_ratio = usage_ratio.map(|value| (1.0 - value).max(0.0));
    let next_reset_at = provider_quota_timestamp_unix_secs(metadata.get("next_reset_at"));
    let reset_seconds = quota_window_reset_seconds(observed_at_unix_secs, next_reset_at);
    let plan_type = metadata
        .get("subscription_title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let is_banned = metadata
        .get("is_banned")
        .and_then(admin_provider_quota_pure::coerce_json_bool)
        == Some(true);
    let ban_reason = metadata
        .get("ban_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let mut windows = Vec::new();
    if usage_ratio.is_some()
        || remaining.is_some()
        || usage_limit.is_some()
        || current_usage.is_some()
        || next_reset_at.is_some()
    {
        windows.push(json!({
            "code": "usage",
            "label": "额度",
            "scope": "account",
            "unit": "count",
            "used_ratio": usage_ratio,
            "remaining_ratio": remaining_ratio,
            "used_value": current_usage,
            "remaining_value": remaining,
            "limit_value": usage_limit,
            "reset_at": next_reset_at,
            "reset_seconds": reset_seconds,
        }));
    }

    if windows.is_empty() && plan_type.is_none() && observed_at_unix_secs.is_none() && !is_banned {
        return None;
    }

    let exhausted = !is_banned
        && (remaining.is_some_and(|value| value <= 0.0)
            || usage_ratio.is_some_and(|value| value >= 1.0 - 1e-6));
    let reason = if is_banned {
        ban_reason
    } else if exhausted {
        Some("额度已耗尽".to_string())
    } else {
        None
    };
    let label = if is_banned {
        Some("账号已封禁")
    } else if exhausted {
        Some("额度耗尽")
    } else {
        None
    };
    let code = if is_banned {
        "banned"
    } else if exhausted {
        "exhausted"
    } else {
        "ok"
    };

    Some(json!({
        "version": 2,
        "provider_type": "kiro",
        "code": code,
        "label": label,
        "reason": reason,
        "freshness": "fresh",
        "source": source,
        "observed_at": observed_at_unix_secs,
        "exhausted": exhausted,
        "usage_ratio": usage_ratio,
        "updated_at": observed_at_unix_secs,
        "reset_at": next_reset_at,
        "reset_seconds": reset_seconds,
        "plan_type": plan_type,
        "windows": windows,
    }))
}

fn build_chatgpt_web_quota_status_snapshot(
    upstream_metadata: Option<&Value>,
    source: &str,
) -> Option<Value> {
    let metadata = provider_quota_metadata_bucket(upstream_metadata, "chatgpt_web")?;
    let observed_at_unix_secs = provider_quota_timestamp_unix_secs(metadata.get("updated_at"));
    let remaining = metadata
        .get("image_quota_remaining")
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let limit = chatgpt_web_image_quota_limit(metadata, remaining);
    let used = metadata
        .get("image_quota_used")
        .and_then(admin_provider_quota_pure::coerce_json_f64)
        .or_else(|| {
            limit
                .zip(remaining)
                .map(|(limit, remaining)| (limit - remaining).max(0.0))
        });
    let reset_at = provider_quota_timestamp_unix_secs(metadata.get("image_quota_reset_at"));
    let reset_seconds = quota_window_reset_seconds(observed_at_unix_secs, reset_at);
    let plan_type = metadata
        .get("plan_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let image_blocked = metadata
        .get("image_quota_blocked")
        .and_then(admin_provider_quota_pure::coerce_json_bool)
        == Some(true);
    let usage_ratio = used
        .zip(limit)
        .and_then(|(used, limit)| (limit > 0.0).then_some((used / limit).clamp(0.0, 1.0)));
    let remaining_ratio = remaining.zip(limit).and_then(|(remaining, limit)| {
        (limit > 0.0).then_some((remaining / limit).clamp(0.0, 1.0))
    });

    let mut windows = Vec::new();
    if remaining.is_some()
        || limit.is_some()
        || used.is_some()
        || reset_at.is_some()
        || image_blocked
    {
        windows.push(json!({
            "code": "image_gen",
            "label": "生图",
            "scope": "account",
            "unit": "count",
            "used_ratio": usage_ratio,
            "remaining_ratio": remaining_ratio,
            "used_value": used,
            "remaining_value": remaining,
            "limit_value": limit,
            "reset_at": reset_at,
            "reset_seconds": reset_seconds,
            "is_exhausted": image_blocked || remaining.is_some_and(|value| value <= 0.0),
        }));
    }

    if windows.is_empty() && plan_type.is_none() && observed_at_unix_secs.is_none() {
        return None;
    }

    let exhausted = image_blocked
        || remaining.is_some_and(|value| value <= 0.0)
        || usage_ratio.is_some_and(|value| value >= 1.0 - 1e-6);
    let reason = if exhausted {
        Some("生图额度已耗尽")
    } else {
        None
    };

    Some(json!({
        "version": 2,
        "provider_type": "chatgpt_web",
        "code": if exhausted { "exhausted" } else { "ok" },
        "label": if exhausted { Some("额度耗尽") } else { None::<&str> },
        "reason": reason,
        "freshness": "fresh",
        "source": source,
        "observed_at": observed_at_unix_secs,
        "exhausted": exhausted,
        "usage_ratio": usage_ratio,
        "updated_at": observed_at_unix_secs,
        "reset_at": reset_at,
        "reset_seconds": reset_seconds,
        "plan_type": plan_type,
        "windows": windows,
    }))
}

fn build_antigravity_quota_status_snapshot(
    upstream_metadata: Option<&Value>,
    source: &str,
) -> Option<Value> {
    let metadata = provider_quota_metadata_bucket(upstream_metadata, "antigravity")?;
    let observed_at_unix_secs = provider_quota_timestamp_unix_secs(metadata.get("updated_at"));
    let is_forbidden = metadata
        .get("is_forbidden")
        .and_then(admin_provider_quota_pure::coerce_json_bool)
        == Some(true);
    let forbidden_reason = metadata
        .get("forbidden_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let windows = provider_quota_model_bucket(metadata)
        .map(|models| {
            models
                .iter()
                .filter_map(|(model_name, item)| {
                    model_quota_window_snapshot(
                        model_name,
                        item.as_object()?,
                        observed_at_unix_secs,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if windows.is_empty() && observed_at_unix_secs.is_none() && !is_forbidden {
        return None;
    }

    let usage_ratio = quota_windows_usage_ratio(&windows);
    let reset_seconds = quota_windows_min_reset_seconds(&windows);
    let reset_at = quota_windows_min_reset_at(&windows);
    let exhausted = !is_forbidden && quota_windows_all_exhausted(&windows);
    let reason = if is_forbidden {
        forbidden_reason
    } else if exhausted {
        Some("所有模型额度已耗尽".to_string())
    } else {
        None
    };
    let label = if is_forbidden {
        Some("访问受限")
    } else if exhausted {
        Some("额度耗尽")
    } else {
        None
    };
    let code = if is_forbidden {
        "forbidden"
    } else if exhausted {
        "exhausted"
    } else {
        "ok"
    };

    Some(json!({
        "version": 2,
        "provider_type": "antigravity",
        "code": code,
        "label": label,
        "reason": reason,
        "freshness": "fresh",
        "source": source,
        "observed_at": observed_at_unix_secs,
        "exhausted": exhausted,
        "usage_ratio": usage_ratio,
        "updated_at": observed_at_unix_secs,
        "reset_at": reset_at,
        "reset_seconds": reset_seconds,
        "plan_type": serde_json::Value::Null,
        "windows": windows,
    }))
}

fn build_grok_quota_status_snapshot(
    upstream_metadata: Option<&Value>,
    source: &str,
) -> Option<Value> {
    let metadata = provider_quota_metadata_bucket(upstream_metadata, "grok")?;
    let observed_at_unix_secs = provider_quota_timestamp_unix_secs(metadata.get("updated_at"));
    let inferred_pool_tier = grok_pool_tier_from_quota_bucket(metadata);
    let pool_tier = provider_quota_metadata_string(metadata, &["pool_tier", "tier"])
        .or_else(|| inferred_pool_tier.map(ToOwned::to_owned));
    let plan_type = provider_quota_metadata_string(metadata, &["plan_type", "plan"])
        .or_else(|| pool_tier.clone());
    let supported_windows = grok_supported_quota_windows_for_tier(pool_tier.as_deref());
    let windows = provider_quota_model_bucket(metadata)
        .map(|models| {
            models
                .iter()
                .filter_map(|(model_name, item)| {
                    if !supported_windows
                        .iter()
                        .any(|(quota_key, _)| *quota_key == model_name.as_str())
                    {
                        return None;
                    }
                    model_quota_window_snapshot(
                        model_name,
                        item.as_object()?,
                        observed_at_unix_secs,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if windows.is_empty() && observed_at_unix_secs.is_none() {
        return None;
    }

    let usage_ratio = quota_windows_usage_ratio(&windows);
    let reset_seconds = quota_windows_min_reset_seconds(&windows);
    let reset_at = quota_windows_min_reset_at(&windows);
    let exhausted = quota_windows_all_exhausted(&windows);

    Some(json!({
        "version": 2,
        "provider_type": "grok",
        "code": if exhausted { "exhausted" } else { "ok" },
        "label": if exhausted { Some("额度耗尽") } else { None::<&str> },
        "reason": if exhausted {
            Some("所有 Grok 模式额度已耗尽")
        } else {
            None::<&str>
        },
        "freshness": "fresh",
        "source": source,
        "observed_at": observed_at_unix_secs,
        "exhausted": exhausted,
        "usage_ratio": usage_ratio,
        "updated_at": observed_at_unix_secs,
        "reset_at": reset_at,
        "reset_seconds": reset_seconds,
        "plan_type": plan_type,
        "pool_tier": pool_tier,
        "windows": windows,
    }))
}

fn grok_oauth_billing_window_snapshot(
    metadata: &Map<String, Value>,
    prefix: &str,
    code: &str,
    label: &str,
    observed_at_unix_secs: Option<u64>,
) -> Option<Value> {
    let used_percent = metadata
        .get(&format!("{prefix}_used_percent"))
        .and_then(admin_provider_quota_pure::coerce_json_f64);
    let reset_at = provider_quota_timestamp_unix_secs(
        metadata
            .get(&format!("{prefix}_reset_at"))
            .or_else(|| metadata.get(&format!("{prefix}_period_end"))),
    );
    let period_start =
        provider_quota_timestamp_unix_secs(metadata.get(&format!("{prefix}_period_start")));
    let window_observed_at =
        provider_quota_timestamp_unix_secs(metadata.get(&format!("{prefix}_updated_at")))
            .or(observed_at_unix_secs);
    let used_ratio = used_percent.map(|value| (value / 100.0).clamp(0.0, 1.0));
    let remaining_ratio = used_ratio.map(|value| (1.0 - value).max(0.0));
    let reset_seconds = quota_window_reset_seconds(window_observed_at, reset_at);

    let (used_value, remaining_value, limit_value, unit) = if prefix == "monthly" {
        let limit_cents = metadata
            .get("monthly_limit_cents")
            .and_then(admin_provider_quota_pure::coerce_json_f64);
        let used_cents = metadata
            .get("monthly_included_used_cents")
            .or_else(|| metadata.get("monthly_used_cents"))
            .and_then(admin_provider_quota_pure::coerce_json_f64);
        let remaining_cents = limit_cents
            .zip(used_cents)
            .map(|(limit, used)| (limit - used).max(0.0));
        (
            used_cents.map(|value| value / 100.0),
            remaining_cents.map(|value| value / 100.0),
            limit_cents.map(|value| value / 100.0),
            "usd",
        )
    } else {
        (None, None, None, "percent")
    };

    if used_ratio.is_none()
        && reset_at.is_none()
        && period_start.is_none()
        && used_value.is_none()
        && limit_value.is_none()
    {
        return None;
    }

    let mut window = Map::new();
    window.insert("code".to_string(), json!(code));
    window.insert("label".to_string(), json!(label));
    window.insert("scope".to_string(), json!("account"));
    window.insert("unit".to_string(), json!(unit));
    window.insert("used_ratio".to_string(), json!(used_ratio));
    window.insert("remaining_ratio".to_string(), json!(remaining_ratio));
    window.insert("used_value".to_string(), json!(used_value));
    window.insert("remaining_value".to_string(), json!(remaining_value));
    window.insert("limit_value".to_string(), json!(limit_value));
    window.insert("period_start".to_string(), json!(period_start));
    window.insert("reset_at".to_string(), json!(reset_at));
    window.insert("reset_seconds".to_string(), json!(reset_seconds));
    window.insert("observed_at".to_string(), json!(window_observed_at));
    window.insert(
        "is_exhausted".to_string(),
        json!(used_ratio.map(|value| value >= 1.0 - 1e-6)),
    );
    if prefix == "weekly" {
        if let Some(product_usage) = metadata.get("weekly_product_usage") {
            window.insert("product_usage".to_string(), product_usage.clone());
        }
    }
    Some(Value::Object(window))
}

fn build_grok_oauth_quota_status_snapshot(
    upstream_metadata: Option<&Value>,
    source: &str,
) -> Option<Value> {
    let metadata = provider_quota_metadata_bucket(upstream_metadata, "grok_oauth")?;
    let observed_at_unix_secs = provider_quota_timestamp_unix_secs(metadata.get("updated_at"));
    let plan_type = provider_quota_metadata_string(metadata, &["plan_type", "plan"]);
    let windows = [
        grok_oauth_billing_window_snapshot(
            metadata,
            "weekly",
            "weekly",
            "周",
            observed_at_unix_secs,
        ),
        grok_oauth_billing_window_snapshot(
            metadata,
            "monthly",
            "monthly",
            "月",
            observed_at_unix_secs,
        ),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    if windows.is_empty() && observed_at_unix_secs.is_none() && plan_type.is_none() {
        return None;
    }

    let usage_ratio = quota_windows_usage_ratio(&windows);
    let reset_seconds = quota_windows_min_reset_seconds(&windows);
    let reset_at = quota_windows_min_reset_at(&windows);
    let exhausted = windows.iter().filter_map(Value::as_object).any(|window| {
        window
            .get("is_exhausted")
            .and_then(admin_provider_quota_pure::coerce_json_bool)
            == Some(true)
    });
    let partial = metadata
        .get("partial")
        .and_then(admin_provider_quota_pure::coerce_json_bool)
        .unwrap_or(false);
    let failed_windows = metadata
        .get("failed_windows")
        .cloned()
        .unwrap_or_else(|| json!([]));

    Some(json!({
        "version": 2,
        "provider_type": "grok_oauth",
        "code": if exhausted { "exhausted" } else { "ok" },
        "label": if exhausted { Some("额度耗尽") } else { None::<&str> },
        "reason": if exhausted {
            Some("Grok 订阅额度窗口已耗尽")
        } else {
            None::<&str>
        },
        "freshness": if partial { "stale" } else { "fresh" },
        "source": source,
        "observed_at": observed_at_unix_secs,
        "exhausted": exhausted,
        "usage_ratio": usage_ratio,
        "updated_at": observed_at_unix_secs,
        "reset_at": reset_at,
        "reset_seconds": reset_seconds,
        "plan_type": plan_type,
        "partial": partial,
        "failed_windows": failed_windows,
        "windows": windows,
    }))
}

fn build_gemini_cli_quota_status_snapshot(
    upstream_metadata: Option<&Value>,
    source: &str,
) -> Option<Value> {
    let metadata = provider_quota_metadata_bucket(upstream_metadata, "gemini_cli")?;
    let observed_at_unix_secs = provider_quota_timestamp_unix_secs(metadata.get("updated_at"));
    let windows = provider_quota_model_bucket(metadata)
        .map(|models| {
            models
                .iter()
                .filter_map(|(model_name, item)| {
                    model_quota_window_snapshot(
                        model_name,
                        item.as_object()?,
                        observed_at_unix_secs,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if windows.is_empty() && observed_at_unix_secs.is_none() {
        return None;
    }

    let usage_ratio = quota_windows_usage_ratio(&windows);
    let active_exhausted_windows = windows
        .iter()
        .filter_map(Value::as_object)
        .filter(|window| {
            window
                .get("is_exhausted")
                .and_then(admin_provider_quota_pure::coerce_json_bool)
                .or_else(|| {
                    window
                        .get("used_ratio")
                        .and_then(Value::as_f64)
                        .map(|value| value >= 1.0 - 1e-6)
                })
                .unwrap_or(false)
        })
        .filter(|window| {
            provider_quota_timestamp_unix_secs(window.get("reset_at"))
                .zip(observed_at_unix_secs)
                .map(|(reset_at, observed_at)| reset_at > observed_at)
                .unwrap_or(true)
        })
        .count();
    let exhausted = !windows.is_empty() && active_exhausted_windows == windows.len();
    let cooling = active_exhausted_windows > 0;
    let reset_seconds = if cooling {
        quota_windows_min_reset_seconds(&windows)
    } else {
        None
    };
    let reset_at = if cooling {
        quota_windows_min_reset_at(&windows)
    } else {
        None
    };

    Some(json!({
        "version": 2,
        "provider_type": "gemini_cli",
        "code": if exhausted {
            "exhausted"
        } else if cooling {
            "cooldown"
        } else {
            "ok"
        },
        "label": if cooling { Some("冷却中") } else { None::<&str> },
        "reason": if exhausted {
            Some("所有模型均处于冷却中")
        } else {
            None::<&str>
        },
        "freshness": "fresh",
        "source": source,
        "observed_at": observed_at_unix_secs,
        "exhausted": exhausted,
        "usage_ratio": usage_ratio,
        "updated_at": observed_at_unix_secs,
        "reset_at": reset_at,
        "reset_seconds": reset_seconds,
        "plan_type": serde_json::Value::Null,
        "windows": windows,
    }))
}

pub(crate) fn sync_provider_key_quota_status_snapshot(
    status_snapshot: Option<&Value>,
    provider_type: &str,
    upstream_metadata: Option<&Value>,
    source: &str,
) -> Option<Value> {
    let normalized_provider_type = provider_type.trim().to_ascii_lowercase();
    let mut quota = match normalized_provider_type.as_str() {
        "codex" => build_codex_quota_status_snapshot(upstream_metadata, source),
        "kiro" => build_kiro_quota_status_snapshot(upstream_metadata, source),
        "chatgpt_web" => build_chatgpt_web_quota_status_snapshot(upstream_metadata, source),
        "antigravity" => build_antigravity_quota_status_snapshot(upstream_metadata, source),
        "grok" => build_grok_quota_status_snapshot(upstream_metadata, source),
        "grok_oauth" => build_grok_oauth_quota_status_snapshot(upstream_metadata, source),
        "gemini_cli" => build_gemini_cli_quota_status_snapshot(upstream_metadata, source),
        _ => None,
    }?;
    if normalized_provider_type == "codex" {
        preserve_quota_window_duration(status_snapshot, &mut quota);
        normalize_codex_quota_windows_for_display_at(
            &mut quota,
            provider_key_status_snapshot_now_unix_secs(),
        );
    }

    let default_snapshot = default_provider_key_status_snapshot();
    let mut snapshot = provider_key_status_snapshot_object(status_snapshot)
        .or_else(|| default_snapshot.as_object().cloned())
        .unwrap_or_default();
    snapshot.insert("quota".to_string(), quota);
    Some(Value::Object(snapshot))
}

fn quota_snapshot_has_materialized_data(
    quota_snapshot: Option<&Map<String, Value>>,
    provider_type: &str,
) -> bool {
    let Some(quota_snapshot) = quota_snapshot else {
        return false;
    };

    let normalized_provider_type = provider_type.trim().to_ascii_lowercase();
    let snapshot_provider_type = quota_snapshot
        .get("provider_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !snapshot_provider_type.is_empty() && snapshot_provider_type != normalized_provider_type {
        return false;
    }

    if quota_snapshot
        .get("windows")
        .and_then(Value::as_array)
        .is_some_and(|windows| !windows.is_empty())
    {
        return true;
    }
    if quota_snapshot
        .get("credits")
        .is_some_and(|credits| !credits.is_null())
    {
        return true;
    }

    quota_snapshot
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|code| {
            !code.is_empty()
                && !code.eq_ignore_ascii_case("unknown")
                && !code.eq_ignore_ascii_case("ok")
        })
}

pub(crate) fn provider_key_status_snapshot_payload(
    key: &StoredProviderCatalogKey,
    provider_type: &str,
) -> serde_json::Value {
    let status_snapshot = key
        .status_snapshot
        .as_ref()
        .filter(|value| value.is_object());
    let quota_snapshot = status_snapshot
        .and_then(Value::as_object)
        .and_then(|snapshot| snapshot.get("quota"))
        .and_then(Value::as_object);

    let mut payload = if quota_snapshot_has_materialized_data(quota_snapshot, provider_type) {
        status_snapshot
            .cloned()
            .unwrap_or_else(default_provider_key_status_snapshot)
    } else {
        sync_provider_key_quota_status_snapshot(
            status_snapshot,
            provider_type,
            key.upstream_metadata.as_ref(),
            "catalog_fallback",
        )
        .or_else(|| status_snapshot.cloned())
        .unwrap_or_else(default_provider_key_status_snapshot)
    };
    if provider_type.trim().eq_ignore_ascii_case("codex") {
        if let Some(quota) = payload
            .as_object_mut()
            .and_then(|snapshot| snapshot.get_mut("quota"))
        {
            normalize_codex_quota_windows_for_display_at(
                quota,
                provider_key_status_snapshot_now_unix_secs(),
            );
        }
    }

    let mut snapshot = provider_key_status_snapshot_object(Some(&payload))
        .or_else(|| default_provider_key_status_snapshot().as_object().cloned())
        .unwrap_or_default();
    snapshot.insert(
        "oauth".to_string(),
        build_provider_key_oauth_status_snapshot(key),
    );
    snapshot.insert(
        "account".to_string(),
        build_provider_key_account_status_snapshot(key, provider_type),
    );
    Value::Object(snapshot)
}

fn status_snapshot_field<'a>(
    status_snapshot: &'a Value,
    section: &str,
    field: &str,
) -> Option<&'a Value> {
    status_snapshot
        .as_object()
        .and_then(|snapshot| snapshot.get(section))
        .and_then(Value::as_object)
        .and_then(|section| section.get(field))
}

fn status_snapshot_string(status_snapshot: &Value, section: &str, field: &str) -> Option<String> {
    status_snapshot_field(status_snapshot, section, field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn status_snapshot_bool(status_snapshot: &Value, section: &str, field: &str) -> bool {
    status_snapshot_field(status_snapshot, section, field)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn provider_key_scheduling_state_payload(
    key: &StoredProviderCatalogKey,
    provider_type: &str,
    status_snapshot: &Value,
    now_unix_secs: u64,
) -> admin_provider_pool_pure::AdminPoolSchedulingStatePayload {
    let account_status_code = status_snapshot_string(status_snapshot, "account", "code");
    let account_status_label = status_snapshot_string(status_snapshot, "account", "label");
    let account_status_reason = status_snapshot_string(status_snapshot, "account", "reason");
    let account_status_source = status_snapshot_string(status_snapshot, "account", "source");
    let quota_code = status_snapshot_string(status_snapshot, "quota", "code");
    let account_quota_exhausted = status_snapshot_bool(status_snapshot, "quota", "exhausted")
        || quota_code
            .as_deref()
            .is_some_and(|code| matches!(code, "exhausted" | "quota_exhausted"));

    let _ = provider_type;
    admin_provider_pool_pure::admin_pool_resolve_scheduling_state(
        admin_provider_pool_pure::AdminPoolSchedulingStateInput {
            key,
            now_unix_secs,
            cooldown_reason: None,
            cooldown_ttl_seconds: None,
            account_blocked: status_snapshot_bool(status_snapshot, "account", "blocked"),
            account_status_code: account_status_code.as_deref(),
            account_status_label: account_status_label.as_deref(),
            account_status_reason: account_status_reason.as_deref(),
            account_status_source: account_status_source.as_deref(),
            account_quota_exhausted,
        },
    )
}

pub(crate) fn provider_key_health_summary(
    key: &StoredProviderCatalogKey,
) -> (
    f64,
    i64,
    Option<String>,
    bool,
    serde_json::Map<String, serde_json::Value>,
) {
    let health_by_format = key
        .health_by_format
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let circuit_by_format = key
        .circuit_breaker_by_format
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut min_health_score = 1.0_f64;
    let mut max_consecutive = 0_i64;
    let mut last_failure_at: Option<String> = None;
    for value in health_by_format.values() {
        let score = value
            .get("health_score")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);
        min_health_score = min_health_score.min(score);
        let consecutive = value
            .get("consecutive_failures")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        max_consecutive = max_consecutive.max(consecutive);
        if let Some(last_failure) = value
            .get("last_failure_at")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
        {
            if last_failure_at
                .as_ref()
                .is_none_or(|current| last_failure > *current)
            {
                last_failure_at = Some(last_failure);
            }
        }
    }

    let any_circuit_open = circuit_by_format.values().any(|value| {
        value
            .get("open")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    });

    (
        if health_by_format.is_empty() {
            1.0
        } else {
            min_health_score
        },
        max_consecutive,
        last_failure_at,
        any_circuit_open,
        circuit_by_format,
    )
}

fn normalize_catalog_oauth_plan_type(value: &str, provider_type: &str) -> Option<String> {
    let mut normalized = value.trim().to_string();
    if normalized.is_empty() {
        return None;
    }

    let provider_type = provider_type.trim().to_ascii_lowercase();
    if !provider_type.is_empty() && normalized.to_ascii_lowercase().starts_with(&provider_type) {
        normalized = normalized[provider_type.len()..]
            .trim_matches(|ch: char| [' ', ':', '-', '_'].contains(&ch))
            .to_string();
    }

    let normalized = normalized.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn catalog_oauth_plan_type_from_source(
    source: &serde_json::Map<String, serde_json::Value>,
    provider_type: &str,
    fields: &[&str],
) -> Option<String> {
    for field in fields {
        let Some(value) = source.get(*field).and_then(serde_json::Value::as_str) else {
            continue;
        };
        if let Some(normalized) = normalize_catalog_oauth_plan_type(value, provider_type) {
            return Some(normalized);
        }
    }
    None
}

fn derive_catalog_oauth_plan_type(
    key: &StoredProviderCatalogKey,
    provider_type: &str,
    auth_config: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<String> {
    if !provider_key_auth_semantics(key, provider_type).oauth_managed() {
        return None;
    }

    let provider_type_key = provider_type.trim().to_ascii_lowercase();
    if let Some(upstream_metadata) = key
        .upstream_metadata
        .as_ref()
        .and_then(serde_json::Value::as_object)
    {
        let provider_bucket = if provider_type_key.is_empty() {
            None
        } else {
            upstream_metadata
                .get(&provider_type_key)
                .and_then(serde_json::Value::as_object)
        };
        for source in provider_bucket
            .into_iter()
            .chain(std::iter::once(upstream_metadata))
        {
            if let Some(plan_type) = catalog_oauth_plan_type_from_source(
                source,
                provider_type,
                &[
                    "plan_type",
                    "tier",
                    "subscription_title",
                    "subscription_plan",
                    "plan",
                ],
            ) {
                return Some(plan_type);
            }
        }
    }

    auth_config.and_then(|source| {
        catalog_oauth_plan_type_from_source(
            source,
            provider_type,
            &["plan_type", "tier", "plan", "subscription_plan"],
        )
    })
}

fn catalog_auth_config_string(
    auth_config: Option<&serde_json::Map<String, serde_json::Value>>,
    fields: &[&str],
) -> serde_json::Value {
    for field in fields {
        let Some(value) = auth_config
            .and_then(|config| config.get(*field))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        return json!(value);
    }
    serde_json::Value::Null
}

pub(crate) fn provider_key_account_label_from_auth_config(
    auth_config: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<String> {
    let mut parts = Vec::new();
    for field in [
        "email",
        "oauth_email",
        "phone",
        "phone_number",
        "mobile",
        "oauth_phone",
    ] {
        let Some(value) = auth_config
            .and_then(|config| config.get(field))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if !parts.iter().any(|part| part == value) {
            parts.push(value.to_string());
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" / "))
    }
}

pub(crate) fn build_admin_provider_key_response(
    state: &AppState,
    key: &StoredProviderCatalogKey,
    provider_type: &str,
    api_formats: &[String],
    now_unix_secs: u64,
) -> serde_json::Value {
    let request_count = u64::from(key.request_count.unwrap_or(0));
    let success_count = u64::from(key.success_count.unwrap_or(0));
    let error_count = u64::from(key.error_count.unwrap_or(0));
    let total_response_time_ms = f64::from(key.total_response_time_ms.unwrap_or(0));
    let success_rate = if request_count > 0 {
        success_count as f64 / request_count as f64
    } else {
        0.0
    };
    let avg_response_time_ms = if success_count > 0 {
        total_response_time_ms / success_count as f64
    } else {
        0.0
    };
    let auth_semantics = provider_key_auth_semantics(key, provider_type);
    let auth_config = parse_catalog_auth_config_json(state, key);
    let oauth_organizations = if auth_semantics.can_show_oauth_metadata() {
        auth_config
            .as_ref()
            .and_then(|config| config.get("organizations"))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let oauth_temporary = auth_semantics.can_show_oauth_metadata()
        && auth_config
            .as_ref()
            .and_then(|config| config.get("access_token_import_temporary"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
    let oauth_plan_type = derive_catalog_oauth_plan_type(key, provider_type, auth_config.as_ref());
    let (
        health_score,
        consecutive_failures,
        last_failure_at,
        circuit_breaker_open,
        circuit_by_format,
    ) = provider_key_health_summary(key);
    let circuit_sample = circuit_by_format
        .values()
        .find(|value| {
            value
                .get("open")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| circuit_by_format.values().next());
    let is_adaptive = key.rpm_limit.is_none();
    let effective_limit = if is_adaptive {
        key.learned_rpm_limit
    } else {
        key.rpm_limit
    };
    let mut payload = serde_json::Map::new();
    payload.insert("id".to_string(), json!(key.id));
    payload.insert("provider_id".to_string(), json!(key.provider_id));
    payload.insert(
        "api_formats".to_string(),
        serde_json::Value::Array(
            api_formats
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    payload.insert(
        "api_key_masked".to_string(),
        json!(masked_catalog_api_key(state, key)),
    );
    payload.insert("api_key_plain".to_string(), serde_json::Value::Null);
    payload.insert("auth_type".to_string(), json!(key.auth_type));
    payload.insert(
        "auth_type_by_format".to_string(),
        json!(key.auth_type_by_format),
    );
    payload.insert(
        "allow_auth_channel_mismatch_formats".to_string(),
        json!(key.allow_auth_channel_mismatch_formats),
    );
    payload.insert(
        "credential_kind".to_string(),
        json!(auth_semantics.credential_kind().as_str()),
    );
    payload.insert(
        "runtime_auth_kind".to_string(),
        json!(auth_semantics.runtime_auth_kind().as_str()),
    );
    payload.insert(
        "oauth_managed".to_string(),
        json!(auth_semantics.oauth_managed()),
    );
    payload.insert(
        "can_refresh_oauth".to_string(),
        json!(provider_key_can_refresh_oauth(
            auth_semantics,
            auth_config.as_ref()
        )),
    );
    payload.insert(
        "can_export_oauth".to_string(),
        json!(auth_semantics.can_export_oauth()),
    );
    payload.insert(
        "can_edit_oauth".to_string(),
        json!(auth_semantics.can_edit_oauth()),
    );
    payload.insert("name".to_string(), json!(key.name));
    payload.insert("rate_multipliers".to_string(), json!(key.rate_multipliers));
    payload.insert(
        "internal_priority".to_string(),
        json!(key.internal_priority),
    );
    payload.insert(
        "global_priority_by_format".to_string(),
        json!(key.global_priority_by_format),
    );
    payload.insert("rpm_limit".to_string(), json!(key.rpm_limit));
    payload.insert("concurrent_limit".to_string(), json!(key.concurrent_limit));
    payload.insert(
        "allowed_models".to_string(),
        serde_json::Value::Array(
            json_string_list(key.allowed_models.as_ref())
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    payload.insert("capabilities".to_string(), json!(key.capabilities));
    payload.insert(
        "oauth_expires_at".to_string(),
        json!(auth_semantics
            .can_show_oauth_metadata()
            .then_some(key.expires_at_unix_secs)
            .flatten()),
    );
    payload.insert(
        "oauth_email".to_string(),
        if auth_semantics.can_show_oauth_metadata() {
            catalog_auth_config_string(auth_config.as_ref(), &["email", "oauth_email"])
        } else {
            serde_json::Value::Null
        },
    );
    payload.insert(
        "oauth_phone".to_string(),
        if auth_semantics.can_show_oauth_metadata() {
            catalog_auth_config_string(
                auth_config.as_ref(),
                &["phone", "phone_number", "mobile", "mobile_phone"],
            )
        } else {
            serde_json::Value::Null
        },
    );
    payload.insert("oauth_plan_type".to_string(), json!(oauth_plan_type));
    payload.insert(
        "oauth_account_id".to_string(),
        if auth_semantics.can_show_oauth_metadata() {
            auth_config
                .as_ref()
                .and_then(|config| config.get("account_id"))
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        },
    );
    payload.insert(
        "oauth_account_name".to_string(),
        if auth_semantics.can_show_oauth_metadata() {
            auth_config
                .as_ref()
                .and_then(|config| config.get("account_name"))
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        },
    );
    payload.insert(
        "oauth_account_user_id".to_string(),
        if auth_semantics.can_show_oauth_metadata() {
            auth_config
                .as_ref()
                .and_then(|config| config.get("account_user_id"))
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        },
    );
    payload.insert(
        "oauth_organizations".to_string(),
        serde_json::Value::Array(oauth_organizations),
    );
    payload.insert("oauth_temporary".to_string(), json!(oauth_temporary));
    payload.insert(
        "oauth_invalid_at".to_string(),
        json!(auth_semantics
            .can_show_oauth_metadata()
            .then_some(key.oauth_invalid_at_unix_secs)
            .flatten()),
    );
    payload.insert(
        "oauth_invalid_reason".to_string(),
        json!(auth_semantics
            .can_show_oauth_metadata()
            .then_some(key.oauth_invalid_reason.clone())
            .flatten()),
    );
    let status_snapshot = provider_key_status_snapshot_payload(key, provider_type);
    payload.insert("status_snapshot".to_string(), status_snapshot.clone());
    payload.insert(
        "cache_ttl_minutes".to_string(),
        json!(key.cache_ttl_minutes),
    );
    payload.insert(
        "max_probe_interval_minutes".to_string(),
        json!(key.max_probe_interval_minutes),
    );
    payload.insert("health_by_format".to_string(), json!(key.health_by_format));
    payload.insert(
        "circuit_breaker_by_format".to_string(),
        json!(key.circuit_breaker_by_format),
    );
    payload.insert("health_score".to_string(), json!(health_score));
    payload.insert(
        "consecutive_failures".to_string(),
        json!(consecutive_failures),
    );
    payload.insert("last_failure_at".to_string(), json!(last_failure_at));
    payload.insert(
        "circuit_breaker_open".to_string(),
        json!(circuit_breaker_open),
    );
    let scheduling =
        provider_key_scheduling_state_payload(key, provider_type, &status_snapshot, now_unix_secs);
    payload.insert(
        "scheduling_state".to_string(),
        json!(scheduling.state.code()),
    );
    payload.insert(
        "scheduling_status".to_string(),
        json!(scheduling.legacy_status),
    );
    payload.insert("scheduling_reason".to_string(), json!(scheduling.reason));
    payload.insert(
        "scheduling_reason_label".to_string(),
        json!(scheduling.reason_label),
    );
    payload.insert("scheduling_label".to_string(), json!(scheduling.label));
    payload.insert(
        "scheduling_blocking".to_string(),
        json!(scheduling.blocking),
    );
    payload.insert(
        "scheduling_ttl_seconds".to_string(),
        json!(scheduling.ttl_seconds),
    );
    payload.insert(
        "circuit_breaker_open_at".to_string(),
        circuit_sample
            .and_then(|value| value.get("open_at"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    );
    payload.insert(
        "next_probe_at".to_string(),
        circuit_sample
            .and_then(|value| value.get("next_probe_at"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    );
    payload.insert(
        "half_open_until".to_string(),
        circuit_sample
            .and_then(|value| value.get("half_open_until"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    );
    payload.insert(
        "half_open_successes".to_string(),
        json!(circuit_sample
            .and_then(|value| value.get("half_open_successes"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)),
    );
    payload.insert(
        "half_open_failures".to_string(),
        json!(circuit_sample
            .and_then(|value| value.get("half_open_failures"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)),
    );
    payload.insert(
        "request_results_window".to_string(),
        circuit_sample
            .and_then(|value| value.get("request_results_window"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    );
    payload.insert("request_count".to_string(), json!(request_count));
    payload.insert("success_count".to_string(), json!(success_count));
    payload.insert("error_count".to_string(), json!(error_count));
    payload.insert("success_rate".to_string(), json!(success_rate));
    payload.insert(
        "avg_response_time_ms".to_string(),
        json!(avg_response_time_ms),
    );
    payload.insert("is_active".to_string(), json!(key.is_active));
    payload.insert("is_adaptive".to_string(), json!(is_adaptive));
    payload.insert(
        "learned_rpm_limit".to_string(),
        json!(key.learned_rpm_limit),
    );
    payload.insert("effective_limit".to_string(), json!(effective_limit));
    payload.insert(
        "utilization_samples".to_string(),
        json!(key.utilization_samples),
    );
    payload.insert(
        "last_probe_increase_at".to_string(),
        json!(key
            .last_probe_increase_at_unix_secs
            .and_then(unix_secs_to_rfc3339)),
    );
    payload.insert(
        "concurrent_429_count".to_string(),
        json!(key.concurrent_429_count),
    );
    payload.insert("rpm_429_count".to_string(), json!(key.rpm_429_count));
    payload.insert(
        "last_429_at".to_string(),
        json!(key.last_429_at_unix_secs.and_then(unix_secs_to_rfc3339)),
    );
    payload.insert("last_429_type".to_string(), json!(key.last_429_type));
    payload.insert("note".to_string(), json!(key.note));
    payload.insert(
        "auto_fetch_models".to_string(),
        json!(key.auto_fetch_models),
    );
    payload.insert(
        "last_models_fetch_at".to_string(),
        json!(key
            .last_models_fetch_at_unix_secs
            .and_then(unix_secs_to_rfc3339)),
    );
    payload.insert(
        "last_models_fetch_error".to_string(),
        json!(key.last_models_fetch_error),
    );
    payload.insert("locked_models".to_string(), json!(key.locked_models));
    payload.insert(
        "model_include_patterns".to_string(),
        json!(key.model_include_patterns),
    );
    payload.insert(
        "model_exclude_patterns".to_string(),
        json!(key.model_exclude_patterns),
    );
    payload.insert(
        "upstream_metadata".to_string(),
        json!(key.upstream_metadata),
    );
    payload.insert("proxy".to_string(), json!(key.proxy));
    payload.insert("fingerprint".to_string(), json!(key.fingerprint));
    payload.insert(
        "last_used_at".to_string(),
        json!(key.last_used_at_unix_secs.and_then(unix_secs_to_rfc3339)),
    );
    payload.insert(
        "created_at".to_string(),
        json!(unix_secs_to_rfc3339(
            key.created_at_unix_ms.unwrap_or(now_unix_secs)
        )),
    );
    payload.insert(
        "updated_at".to_string(),
        json!(unix_secs_to_rfc3339(
            key.updated_at_unix_secs.unwrap_or(now_unix_secs)
        )),
    );
    serde_json::Value::Object(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_catalog_key() -> StoredProviderCatalogKey {
        let encrypted_api_key =
            encrypt_python_fernet_plaintext(DEVELOPMENT_ENCRYPTION_KEY, "sk-test-123")
                .expect("api key ciphertext should build");
        StoredProviderCatalogKey::new(
            "key-test".to_string(),
            "provider-test".to_string(),
            "default".to_string(),
            "api_key".to_string(),
            None,
            true,
        )
        .expect("key should build")
        .with_transport_fields(
            Some(json!(["openai:chat"])),
            encrypted_api_key,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("key transport should build")
    }

    #[test]
    fn masked_catalog_api_key_handles_unicode_plaintext_without_panicking() {
        let state = AppState::new().expect("gateway should build");
        let encrypted_api_key =
            encrypt_python_fernet_plaintext(DEVELOPMENT_ENCRYPTION_KEY, "测试-密钥-1234567890")
                .expect("api key ciphertext should build");
        let key = StoredProviderCatalogKey::new(
            "key-unicode".to_string(),
            "provider-test".to_string(),
            "default".to_string(),
            "api_key".to_string(),
            None,
            true,
        )
        .expect("key should build")
        .with_transport_fields(
            Some(json!(["openai:chat"])),
            encrypted_api_key,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("key transport should build");

        let masked = masked_catalog_api_key(&state, &key);
        assert!(masked.contains("***"));
        assert_ne!(masked, "***ERROR***");
    }

    #[test]
    fn provider_key_status_snapshot_payload_backfills_missing_quota_from_upstream_metadata() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "codex": {
                "updated_at": 1_775_553_285u64,
                "plan_type": "plus",
                "primary_used_percent": 55.0,
                "primary_reset_at": 1_900_000_000u64,
                "secondary_used_percent": 12.5,
                "secondary_reset_at": 1_900_500_000u64,
                "has_credits": true,
                "credits_balance": 42.0
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should be object");

        assert_eq!(quota.get("provider_type"), Some(&json!("codex")));
        assert_eq!(quota.get("plan_type"), Some(&json!("plus")));
        assert_eq!(quota.get("updated_at"), Some(&json!(1_775_553_285u64)));
        assert_eq!(quota.get("reset_at"), Some(&json!(1_900_000_000u64)));
        assert_eq!(
            quota
                .get("credits")
                .and_then(Value::as_object)
                .and_then(|credits| credits.get("balance")),
            Some(&json!(42.0))
        );
        assert_eq!(
            quota.get("windows").and_then(Value::as_array).map(Vec::len),
            Some(2usize)
        );
    }

    #[test]
    fn provider_key_status_snapshot_payload_backfills_codex_spark_windows() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "codex": {
                "updated_at": 1_775_553_285u64,
                "plan_type": "plus",
                "primary_used_percent": 55.0,
                "primary_reset_at": 1_900_000_000u64,
                "secondary_used_percent": 12.5,
                "secondary_reset_at": 1_900_500_000u64,
                "spark_primary_used_percent": 40.0,
                "spark_primary_reset_at": 1_900_100_000u64,
                "spark_primary_window_minutes": 300u64,
                "spark_secondary_used_percent": 5.0,
                "spark_secondary_reset_at": 1_900_600_000u64,
                "spark_secondary_window_minutes": 10_080u64
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let windows = payload["quota"]["windows"]
            .as_array()
            .expect("quota windows should exist");
        let spark_5h = windows
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("spark_5h")))
            .expect("Spark 5H window should exist");
        let spark_weekly = windows
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("spark_weekly")))
            .expect("Spark weekly window should exist");

        assert_eq!(windows.len(), 4);
        assert_eq!(spark_5h.get("label"), Some(&json!("Spark 5H")));
        assert_eq!(spark_5h.get("remaining_ratio"), Some(&json!(0.6)));
        assert_eq!(spark_weekly.get("label"), Some(&json!("Spark 7D")));
        assert_eq!(spark_weekly.get("remaining_ratio"), Some(&json!(0.95)));
    }

    #[test]
    fn provider_key_status_snapshot_uses_generic_codex_windows_without_feature_exhaustion() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "codex": {
                "updated_at": 1_775_553_285u64,
                "plan_type": "team",
                "windows": [{
                    "code": "1m",
                    "label": "1M",
                    "scope": "account",
                    "used_percent": 10.0,
                    "reset_at": 1_900_000_000u64,
                    "window_seconds": 2_592_000u64
                }, {
                    "code": "spark:5h",
                    "label": "GPT-5.3-Codex-Spark 5H",
                    "scope": "feature",
                    "used_percent": 100.0,
                    "reset_at": 1_800_000_000u64,
                    "window_seconds": 18_000u64
                }]
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let quota = payload["quota"]
            .as_object()
            .expect("quota snapshot should exist");
        let windows = quota["windows"]
            .as_array()
            .expect("quota windows should exist");

        assert_eq!(windows.len(), 2);
        assert_eq!(quota.get("usage_ratio"), Some(&json!(0.1)));
        assert_eq!(quota.get("exhausted"), Some(&json!(false)));
        assert!(windows.iter().any(|window| {
            window["code"] == json!("1m")
                && window["window_minutes"] == json!(43_200u64)
                && window["remaining_ratio"] == json!(0.9)
        }));
    }

    #[test]
    fn provider_key_status_snapshot_payload_keeps_codex_free_window_quota_available() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "codex": {
                "updated_at": 1_775_553_285u64,
                "plan_type": "free",
                "primary_used_percent": 64.0,
                "primary_reset_at": 1_900_000_000u64,
                "secondary_used_percent": 3.0,
                "secondary_reset_at": 1_900_500_000u64,
                "has_credits": false,
                "credits_balance": 0.0,
                "credits_unlimited": false
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should be object");

        assert_eq!(quota.get("code"), Some(&json!("ok")));
        assert_eq!(quota.get("exhausted"), Some(&json!(false)));
        assert_eq!(quota.get("usage_ratio"), Some(&json!(0.64)));
        assert_eq!(
            quota
                .get("credits")
                .and_then(Value::as_object)
                .and_then(|credits| credits.get("has_credits")),
            Some(&json!(false))
        );
        assert_eq!(
            quota.get("windows").and_then(Value::as_array).map(Vec::len),
            Some(2usize)
        );
    }

    #[test]
    fn provider_key_status_snapshot_payload_derives_codex_reset_at_from_countdown() {
        let mut key = sample_catalog_key();
        let observed_at = provider_key_status_snapshot_now_unix_secs();
        let reset_at = observed_at + 3_600;
        key.upstream_metadata = Some(json!({
            "codex": {
                "updated_at": observed_at,
                "primary_used_percent": 55.0,
                "primary_reset_after_seconds": 3_600u64
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should be object");
        let window = quota
            .get("windows")
            .and_then(Value::as_array)
            .and_then(|windows| windows.first())
            .and_then(Value::as_object)
            .expect("quota window should exist");

        assert_eq!(quota.get("reset_at"), Some(&json!(reset_at)));
        assert_eq!(quota.get("reset_seconds"), Some(&json!(3_600u64)));
        assert_eq!(window.get("reset_at"), Some(&json!(reset_at)));
        assert_eq!(window.get("reset_seconds"), Some(&json!(3_600u64)));
    }

    #[test]
    fn provider_key_status_snapshot_payload_backfills_chatgpt_web_image_quota() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "chatgpt_web": {
                "updated_at": 1_778_067_246u64,
                "plan_type": "free",
                "image_quota_remaining": 24.0,
                "image_quota_reset_at": 1_778_157_172u64
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "chatgpt_web");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should be object");
        let window = quota
            .get("windows")
            .and_then(Value::as_array)
            .and_then(|windows| windows.first())
            .and_then(Value::as_object)
            .expect("image quota window should exist");

        assert_eq!(quota.get("provider_type"), Some(&json!("chatgpt_web")));
        assert_eq!(quota.get("code"), Some(&json!("ok")));
        assert_eq!(quota.get("plan_type"), Some(&json!("free")));
        assert_eq!(quota.get("reset_at"), Some(&json!(1_778_157_172u64)));
        assert_eq!(quota.get("usage_ratio"), Some(&json!(0.04)));
        assert_eq!(window.get("code"), Some(&json!("image_gen")));
        assert_eq!(window.get("remaining_value"), Some(&json!(24.0)));
        assert_eq!(window.get("limit_value"), Some(&json!(25.0)));
        assert_eq!(window.get("used_value"), Some(&json!(1.0)));
        assert_eq!(window.get("remaining_ratio"), Some(&json!(0.96)));
    }

    #[test]
    fn provider_key_status_snapshot_payload_backfills_grok_model_quota() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "grok": {
                "updated_at": 1_778_067_246u64,
                "pool_tier": "heavy",
                "plan_type": "heavy",
                "quota_by_model": {
                    "quota_auto": {
                        "display_name": "auto",
                        "remaining_fraction": 0.4,
                        "used_percent": 60.0,
                        "remaining": 60.0,
                        "total": 150.0,
                        "reset_at": 1_778_157_172u64,
                        "is_exhausted": false
                    },
                    "quota_heavy": {
                        "display_name": "heavy",
                        "remaining_fraction": 0.0,
                        "used_percent": 100.0,
                        "reset_at": 1_778_157_172u64,
                        "is_exhausted": true
                    }
                }
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "grok");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should be object");
        let windows = quota
            .get("windows")
            .and_then(Value::as_array)
            .expect("grok quota windows should exist");

        assert_eq!(quota.get("provider_type"), Some(&json!("grok")));
        assert_eq!(quota.get("code"), Some(&json!("ok")));
        assert_eq!(quota.get("plan_type"), Some(&json!("heavy")));
        assert_eq!(quota.get("pool_tier"), Some(&json!("heavy")));
        assert_eq!(quota.get("exhausted"), Some(&json!(false)));
        assert_eq!(quota.get("usage_ratio"), Some(&json!(1.0)));
        assert_eq!(quota.get("reset_at"), Some(&json!(1_778_157_172u64)));
        assert_eq!(windows.len(), 2);
        assert!(windows.iter().any(|window| {
            window
                .get("code")
                .and_then(Value::as_str)
                .is_some_and(|code| code == "model:quota_auto")
        }));
        let auto = windows
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("model:quota_auto")))
            .expect("auto quota window should exist");
        assert_eq!(auto.get("remaining_value"), Some(&json!(60.0)));
        assert_eq!(auto.get("limit_value"), Some(&json!(150.0)));
        assert_eq!(auto.get("used_value"), Some(&json!(90.0)));
    }

    #[test]
    fn provider_key_status_snapshot_payload_builds_grok_oauth_billing_windows() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "grok_oauth": {
                "updated_at": 1_800_000_000u64,
                "weekly_updated_at": 1_800_000_000u64,
                "weekly_used_percent": 25.0,
                "weekly_reset_at": 1_900_000_000u64,
                "monthly_updated_at": 1_800_000_000u64,
                "monthly_limit_cents": 15_000.0,
                "monthly_included_used_cents": 4_500.0,
                "monthly_used_percent": 30.0,
                "monthly_reset_at": 1_910_000_000u64,
                "plan_type": "super",
                "partial": false
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "grok_oauth");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should be object");
        let windows = quota
            .get("windows")
            .and_then(Value::as_array)
            .expect("billing windows should exist");

        assert_eq!(quota.get("provider_type"), Some(&json!("grok_oauth")));
        assert_eq!(quota.get("plan_type"), Some(&json!("super")));
        assert_eq!(quota.get("usage_ratio"), Some(&json!(0.3)));
        assert_eq!(quota.get("exhausted"), Some(&json!(false)));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0]["code"], json!("weekly"));
        assert_eq!(windows[0]["remaining_ratio"], json!(0.75));
        assert_eq!(windows[1]["code"], json!("monthly"));
        assert_eq!(windows[1]["unit"], json!("usd"));
        assert_eq!(windows[1]["used_value"], json!(45.0));
        assert_eq!(windows[1]["remaining_value"], json!(105.0));
        assert_eq!(windows[1]["limit_value"], json!(150.0));
    }

    #[test]
    fn provider_key_status_snapshot_payload_preserves_existing_materialized_quota_snapshot() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "codex": {
                "updated_at": 100u64,
                "primary_used_percent": 100.0
            }
        }));
        key.status_snapshot = Some(json!({
            "oauth": {
                "code": "none",
                "label": serde_json::Value::Null,
                "reason": serde_json::Value::Null,
                "expires_at": serde_json::Value::Null,
                "invalid_at": serde_json::Value::Null,
                "source": serde_json::Value::Null,
                "requires_reauth": false,
                "expiring_soon": false
            },
            "account": {
                "code": "ok",
                "label": serde_json::Value::Null,
                "reason": serde_json::Value::Null,
                "blocked": false,
                "source": serde_json::Value::Null,
                "recoverable": false
            },
            "quota": {
                "version": 2,
                "provider_type": "codex",
                "code": "ok",
                "label": serde_json::Value::Null,
                "reason": serde_json::Value::Null,
                "freshness": "fresh",
                "source": "refresh_api",
                "observed_at": 200u64,
                "exhausted": false,
                "usage_ratio": 0.25,
                "updated_at": 200u64,
                "reset_seconds": 3600u64,
                "plan_type": "team",
                "windows": [{
                    "code": "weekly",
                    "label": "周",
                    "scope": "account",
                    "unit": "percent",
                    "used_ratio": 0.25,
                    "remaining_ratio": 0.75,
                    "reset_at": 1_900_000_000u64,
                    "reset_seconds": 3600u64
                }]
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should be object");

        assert_eq!(quota.get("updated_at"), Some(&json!(200u64)));
        assert_eq!(quota.get("plan_type"), Some(&json!("team")));
        assert_eq!(
            quota
                .get("windows")
                .and_then(Value::as_array)
                .and_then(|windows| windows.first())
                .and_then(Value::as_object)
                .and_then(|window| window.get("used_ratio")),
            Some(&json!(0.25))
        );
    }

    #[test]
    fn provider_key_status_snapshot_payload_normalizes_elapsed_codex_window_for_display() {
        let mut key = sample_catalog_key();
        key.status_snapshot = Some(json!({
            "quota": {
                "version": 2,
                "provider_type": "codex",
                "code": "exhausted",
                "exhausted": true,
                "usage_ratio": 1.0,
                "updated_at": 100u64,
                "windows": [
                    {
                        "code": "5h",
                        "label": "5H",
                        "used_ratio": 1.0,
                        "remaining_ratio": 0.0,
                        "is_exhausted": true,
                        "reset_at": 1u64,
                        "reset_seconds": 3600u64,
                        "window_minutes": 300u64,
                        "usage": {
                            "request_count": 12,
                            "total_tokens": 3456,
                            "total_cost_usd": "7.89000000"
                        }
                    }
                ]
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should exist");
        assert_eq!(quota.get("code"), Some(&json!("ok")));
        assert_eq!(quota.get("exhausted"), Some(&json!(false)));
        assert_eq!(quota.get("usage_ratio"), Some(&json!(0.0)));

        let five_h = payload["quota"]["windows"]
            .as_array()
            .expect("quota windows should exist")
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("5h")))
            .expect("5h window should exist");

        assert_eq!(five_h.get("used_ratio"), Some(&json!(0.0)));
        assert_eq!(five_h.get("remaining_ratio"), Some(&json!(1.0)));
        assert_eq!(five_h.get("is_exhausted"), Some(&json!(false)));
        assert_eq!(five_h.get("reset_seconds"), Some(&json!(0u64)));
        assert!(five_h.get("usage").is_none());
    }

    #[test]
    fn sync_provider_key_quota_status_snapshot_drops_legacy_codex_usage_state() {
        let current_status_snapshot = json!({
            "quota": {
                "version": 2,
                "provider_type": "codex",
                "windows": [
                    {
                        "code": "weekly",
                        "usage_reset_at": 1_775_600_000u64,
                        "usage": {
                            "request_count": 3,
                            "total_tokens": 375,
                            "total_cost_usd": "0.60000000"
                        },
                        "reset_at": 1_900_000_000u64,
                        "window_minutes": 10_080u64
                    },
                    {
                        "code": "5h",
                        "usage_reset_at": 1_775_700_000u64,
                        "usage": {
                            "request_count": 2,
                            "total_tokens": 225,
                            "total_cost_usd": "0.30000000"
                        },
                        "reset_at": 1_900_500_000u64,
                        "window_minutes": 300u64
                    }
                ]
            }
        });
        let upstream_metadata = json!({
            "codex": {
                "updated_at": 1_775_800_000u64,
                "plan_type": "plus",
                "primary_used_percent": 5.0,
                "primary_reset_at": 1_900_000_000u64,
                "primary_window_minutes": 10_080u64,
                "secondary_used_percent": 1.0,
                "secondary_reset_at": 1_900_500_000u64,
                "secondary_window_minutes": 300u64
            }
        });

        let payload = sync_provider_key_quota_status_snapshot(
            Some(&current_status_snapshot),
            "codex",
            Some(&upstream_metadata),
            "refresh_api",
        )
        .expect("quota snapshot should sync");
        let windows = payload
            .get("quota")
            .and_then(Value::as_object)
            .and_then(|quota| quota.get("windows"))
            .and_then(Value::as_array)
            .expect("quota windows should exist");
        let weekly = windows
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("weekly")))
            .expect("weekly window should exist");
        let five_h = windows
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("5h")))
            .expect("5h window should exist");

        assert!(weekly.get("usage_reset_at").is_none());
        assert!(weekly.get("usage").is_none());
        assert!(five_h.get("usage_reset_at").is_none());
        assert!(five_h.get("usage").is_none());
    }

    #[test]
    fn sync_provider_key_quota_status_snapshot_does_not_preserve_missing_active_codex_windows() {
        let current_status_snapshot = json!({
            "quota": {
                "version": 2,
                "provider_type": "codex",
                "windows": [
                    {
                        "code": "weekly",
                        "label": "周",
                        "used_ratio": 0.16,
                        "remaining_ratio": 0.84,
                        "reset_at": 1_900_000_000u64,
                        "reset_seconds": 530_000u64,
                        "window_minutes": 10_080u64
                    },
                    {
                        "code": "5h",
                        "label": "5H",
                        "used_ratio": 0.05,
                        "remaining_ratio": 0.95,
                        "reset_at": 1_800_000_000u64,
                        "reset_seconds": 12_000u64,
                        "window_minutes": 300u64
                    }
                ]
            }
        });
        let upstream_metadata = json!({
            "codex": {
                "updated_at": 1_777_000_000u64,
                "plan_type": "plus"
            }
        });

        let payload = sync_provider_key_quota_status_snapshot(
            Some(&current_status_snapshot),
            "codex",
            Some(&upstream_metadata),
            "refresh_api",
        )
        .expect("quota snapshot should sync");
        let windows = payload["quota"]["windows"]
            .as_array()
            .expect("quota windows should exist");

        assert!(windows.is_empty());
    }

    #[test]
    fn sync_provider_key_quota_status_snapshot_removes_stale_codex_five_hour_window() {
        let current_status_snapshot = json!({
            "quota": {
                "version": 2,
                "provider_type": "codex",
                "windows": [
                    {
                        "code": "weekly",
                        "label": "周",
                        "used_ratio": 0.16,
                        "remaining_ratio": 0.84,
                        "reset_at": 1_900_000_000u64,
                        "reset_seconds": 530_000u64,
                        "window_minutes": 10_080u64
                    },
                    {
                        "code": "5h",
                        "label": "5H",
                        "used_ratio": 0.05,
                        "remaining_ratio": 0.95,
                        "reset_at": 1_900_500_000u64,
                        "reset_seconds": 12_000u64,
                        "window_minutes": 300u64
                    }
                ]
            }
        });
        let upstream_metadata = json!({
            "codex": {
                "updated_at": 1_777_000_000u64,
                "plan_type": "team",
                "primary_used_percent": 16.0,
                "primary_reset_after_seconds": 530_000u64,
                "primary_window_minutes": 10_080u64
            }
        });

        let payload = sync_provider_key_quota_status_snapshot(
            Some(&current_status_snapshot),
            "codex",
            Some(&upstream_metadata),
            "response_headers",
        )
        .expect("quota snapshot should sync");
        let windows = payload["quota"]["windows"]
            .as_array()
            .expect("quota windows should exist");

        assert!(windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("weekly"))));
        assert!(!windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("5h"))));
    }

    #[test]
    fn merge_codex_metadata_bucket_removes_stale_quota_fields_when_new_evidence_arrives() {
        let current = json!({
            "plan_type": "team",
            "secondary_used_percent": 7.0,
            "secondary_reset_after_seconds": 1200u64,
            "spark_primary_used_percent": 4.0,
            "rate_limit_reset_credits": { "available_count": 1 },
            "account_id": "account-1"
        });
        let incoming = json!({
            "updated_at": 1_777_000_000u64,
            "primary_used_percent": 12.0,
            "primary_window_minutes": 10_080u64
        });

        let merged = merge_codex_metadata_bucket(Some(&current), &incoming);

        assert_eq!(merged["plan_type"], json!("team"));
        assert_eq!(merged["account_id"], json!("account-1"));
        assert_eq!(merged["primary_used_percent"], json!(12.0));
        assert!(merged.get("secondary_used_percent").is_none());
        assert!(merged.get("secondary_reset_after_seconds").is_none());
        assert!(merged.get("spark_primary_used_percent").is_none());
        assert!(merged.get("rate_limit_reset_credits").is_none());
    }

    #[test]
    fn merge_codex_response_headers_preserves_feature_windows_and_reset_credits() {
        let current = json!({
            "plan_type": "pro",
            "account_id": "account-1",
            "rate_limit_reset_credits": { "available_count": 1 },
            "spark_primary_used_percent": 3.0,
            "windows": [
                {
                    "code": "weekly",
                    "scope": "account",
                    "used_percent": 5.0,
                    "window_minutes": 10_080u64
                },
                {
                    "code": "5h",
                    "scope": "account",
                    "used_percent": 7.0,
                    "window_minutes": 300u64
                },
                {
                    "code": "spark:5h",
                    "scope": "feature",
                    "used_percent": 0.0,
                    "window_minutes": 300u64
                },
                {
                    "code": "spark:weekly",
                    "scope": "feature",
                    "used_percent": 0.0,
                    "window_minutes": 10_080u64
                }
            ]
        });
        let incoming = json!({
            "updated_at": 1_777_000_000u64,
            "plan_type": "pro",
            "primary_used_percent": 12.0,
            "primary_window_minutes": 10_080u64,
            "windows": [{
                "code": "weekly",
                "scope": "account",
                "used_percent": 12.0,
                "window_minutes": 10_080u64
            }]
        });

        let merged = merge_codex_response_header_metadata_bucket(Some(&current), &incoming);
        let windows = merged["windows"]
            .as_array()
            .expect("merged windows should exist");

        assert_eq!(merged["account_id"], json!("account-1"));
        assert_eq!(
            merged["rate_limit_reset_credits"]["available_count"],
            json!(1)
        );
        assert_eq!(merged["spark_primary_used_percent"], json!(3.0));
        assert_eq!(windows.len(), 3);
        assert!(windows.iter().any(|window| {
            window["code"] == json!("weekly") && window["used_percent"] == json!(12.0)
        }));
        assert!(!windows.iter().any(|window| window["code"] == json!("5h")));
        assert!(windows
            .iter()
            .any(|window| window["code"] == json!("spark:5h")));
        assert!(windows
            .iter()
            .any(|window| window["code"] == json!("spark:weekly")));
    }

    #[test]
    fn sync_provider_key_quota_status_snapshot_drops_zero_length_codex_refresh_window() {
        let current_status_snapshot = json!({
            "quota": {
                "version": 2,
                "provider_type": "codex",
                "windows": [
                    {
                        "code": "5h",
                        "label": "5H",
                        "used_ratio": 0.05,
                        "remaining_ratio": 0.95,
                        "reset_at": 1_800_000_000u64,
                        "reset_seconds": 12_000u64,
                        "window_minutes": 300u64
                    }
                ]
            }
        });
        let upstream_metadata = json!({
            "codex": {
                "updated_at": 1_777_000_000u64,
                "plan_type": "plus",
                "primary_used_percent": 0.0,
                "primary_reset_after_seconds": 0u64,
                "primary_reset_at": 1_777_000_000u64,
                "primary_window_minutes": 0u64
            }
        });

        let payload = sync_provider_key_quota_status_snapshot(
            Some(&current_status_snapshot),
            "codex",
            Some(&upstream_metadata),
            "refresh_api",
        )
        .expect("quota snapshot should sync");
        let windows = payload["quota"]["windows"]
            .as_array()
            .expect("quota windows should exist");

        assert!(!windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("weekly"))));
        assert!(!windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("5h"))));
    }

    #[test]
    fn sync_provider_key_quota_status_snapshot_does_not_preserve_expired_missing_codex_windows() {
        let current_status_snapshot = json!({
            "quota": {
                "version": 2,
                "provider_type": "codex",
                "windows": [
                    {
                        "code": "5h",
                        "reset_at": 1_776_999_999u64,
                        "window_minutes": 300u64
                    }
                ]
            }
        });
        let upstream_metadata = json!({
            "codex": {
                "updated_at": 1_777_000_000u64,
                "plan_type": "plus"
            }
        });

        let payload = sync_provider_key_quota_status_snapshot(
            Some(&current_status_snapshot),
            "codex",
            Some(&upstream_metadata),
            "refresh_api",
        )
        .expect("quota snapshot should sync");
        let windows = payload["quota"]["windows"]
            .as_array()
            .expect("quota windows should exist");

        assert!(!windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("5h"))));
    }

    #[test]
    fn sync_provider_key_quota_status_snapshot_drops_codex_usage_state_when_window_resets() {
        let current_status_snapshot = json!({
            "quota": {
                "version": 2,
                "provider_type": "codex",
                "windows": [
                    {
                        "code": "weekly",
                        "usage_reset_at": 1_775_600_000u64,
                        "usage": {
                            "request_count": 3,
                            "total_tokens": 375,
                            "total_cost_usd": "0.60000000"
                        },
                        "reset_at": 1_900_000_000u64,
                        "window_minutes": 10_080u64
                    }
                ]
            }
        });
        let upstream_metadata = json!({
            "codex": {
                "updated_at": 1_900_000_100u64,
                "plan_type": "plus",
                "primary_used_percent": 0.0,
                "primary_reset_at": 1_960_480_100u64,
                "primary_window_minutes": 10_080u64
            }
        });

        let payload = sync_provider_key_quota_status_snapshot(
            Some(&current_status_snapshot),
            "codex",
            Some(&upstream_metadata),
            "refresh_api",
        )
        .expect("quota snapshot should sync");
        let weekly = payload["quota"]["windows"]
            .as_array()
            .expect("quota windows should exist")
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("weekly")))
            .expect("weekly window should exist");

        assert_eq!(weekly.get("reset_at"), Some(&json!(1_960_480_100u64)));
        assert!(weekly.get("usage_reset_at").is_none());
        assert!(weekly.get("usage").is_none());
    }

    #[test]
    fn sync_provider_key_quota_status_snapshot_defaults_codex_window_minutes() {
        let upstream_metadata = json!({
            "codex": {
                "updated_at": 1_775_800_000u64,
                "plan_type": "plus",
                "primary_used_percent": 5.0,
                "primary_reset_at": 1_900_000_000u64,
                "secondary_used_percent": 1.0,
                "secondary_reset_at": 1_900_500_000u64
            }
        });

        let payload = sync_provider_key_quota_status_snapshot(
            None,
            "codex",
            Some(&upstream_metadata),
            "response_headers",
        )
        .expect("quota snapshot should sync");
        let windows = payload["quota"]["windows"]
            .as_array()
            .expect("quota windows should exist");
        let weekly = windows
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("weekly")))
            .expect("weekly window should exist");
        let five_h = windows
            .iter()
            .filter_map(Value::as_object)
            .find(|window| window.get("code") == Some(&json!("5h")))
            .expect("5h window should exist");

        assert_eq!(weekly.get("window_minutes"), Some(&json!(10_080u64)));
        assert_eq!(five_h.get("window_minutes"), Some(&json!(300u64)));
    }

    #[test]
    fn provider_key_status_snapshot_payload_backfills_thin_ok_snapshot_from_upstream_metadata() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "antigravity": {
                "updated_at": 1_775_553_285u64,
                "quota_by_model": {
                    "gemini-2.5-pro": { "used_percent": 0.0 },
                    "gemini-2.5-flash": { "used_percent": 25.0 }
                }
            }
        }));
        key.status_snapshot = Some(json!({
            "oauth": {
                "code": "none",
                "label": serde_json::Value::Null,
                "reason": serde_json::Value::Null,
                "expires_at": serde_json::Value::Null,
                "invalid_at": serde_json::Value::Null,
                "source": serde_json::Value::Null,
                "requires_reauth": false,
                "expiring_soon": false
            },
            "account": {
                "code": "ok",
                "label": serde_json::Value::Null,
                "reason": serde_json::Value::Null,
                "blocked": false,
                "source": serde_json::Value::Null,
                "recoverable": false
            },
            "quota": {
                "version": 2,
                "provider_type": "antigravity",
                "code": "ok",
                "label": serde_json::Value::Null,
                "reason": serde_json::Value::Null,
                "freshness": "fresh",
                "source": "refresh_api",
                "observed_at": 100u64,
                "exhausted": false,
                "usage_ratio": 0.0,
                "updated_at": 100u64,
                "reset_seconds": serde_json::Value::Null,
                "plan_type": serde_json::Value::Null
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "antigravity");
        let quota = payload
            .get("quota")
            .and_then(Value::as_object)
            .expect("quota snapshot should be object");

        assert_eq!(quota.get("provider_type"), Some(&json!("antigravity")));
        assert_eq!(quota.get("updated_at"), Some(&json!(1_775_553_285u64)));
        assert_eq!(
            quota.get("windows").and_then(Value::as_array).map(Vec::len),
            Some(2usize)
        );
    }

    #[test]
    fn provider_key_status_snapshot_payload_backfills_account_block_from_oauth_invalid_reason() {
        let mut key = sample_catalog_key();
        key.oauth_invalid_reason = Some("[ACCOUNT_BLOCK] account has been deactivated".to_string());

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let account = payload
            .get("account")
            .and_then(Value::as_object)
            .expect("account snapshot should be object");

        assert_eq!(account.get("code"), Some(&json!("account_disabled")));
        assert_eq!(account.get("label"), Some(&json!("账号停用")));
        assert_eq!(
            account.get("reason"),
            Some(&json!("account has been deactivated"))
        );
        assert_eq!(account.get("blocked"), Some(&json!(true)));
        assert_eq!(account.get("source"), Some(&json!("oauth_invalid")));
    }

    #[test]
    fn provider_key_status_snapshot_payload_backfills_workspace_deactivated_from_metadata() {
        let mut key = sample_catalog_key();
        key.upstream_metadata = Some(json!({
            "codex": {
                "account_disabled": true,
                "reason": "deactivated_workspace"
            }
        }));

        let payload = provider_key_status_snapshot_payload(&key, "codex");
        let account = payload
            .get("account")
            .and_then(Value::as_object)
            .expect("account snapshot should be object");

        assert_eq!(account.get("code"), Some(&json!("workspace_deactivated")));
        assert_eq!(account.get("label"), Some(&json!("工作区停用")));
        assert_eq!(account.get("blocked"), Some(&json!(true)));
        assert_eq!(account.get("source"), Some(&json!("metadata")));
    }
}

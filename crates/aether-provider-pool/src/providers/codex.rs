use std::collections::BTreeMap;

use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogEndpoint;
use aether_pool_core::PoolSchedulingPreset;
use serde_json::{json, Map, Value};

use crate::capability::ProviderPoolCapabilities;
use crate::provider::{
    provider_pool_endpoint_format_matches, provider_pool_matching_endpoint, ProviderPoolAdapter,
    ProviderPoolMemberInput,
};
use crate::quota::{
    provider_pool_current_unix_secs, provider_pool_json_bool, provider_pool_json_f64,
    provider_pool_metadata_bucket, provider_pool_quota_snapshot_exhausted_decision,
    provider_pool_reset_deadline_elapsed, provider_pool_timestamp_unix_secs,
};
use crate::quota_refresh::ProviderPoolQuotaRequestSpec;

pub const CODEX_WHAM_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
pub const CODEX_WHAM_RESET_CREDITS_CONSUME_URL: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume";
const PLACEHOLDER_API_KEY: &str = "__placeholder__";

#[derive(Debug, Clone, Default)]
pub struct CodexProviderPoolAdapter;

impl ProviderPoolAdapter for CodexProviderPoolAdapter {
    fn provider_type(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> ProviderPoolCapabilities {
        ProviderPoolCapabilities {
            plan_tier: true,
            quota_reset: true,
            quota_refresh: true,
        }
    }

    fn default_scheduling_presets(&self) -> Vec<PoolSchedulingPreset> {
        vec![PoolSchedulingPreset {
            preset: "recent_refresh".to_string(),
            enabled: true,
            mode: None,
        }]
    }

    fn quota_exhausted(&self, input: &ProviderPoolMemberInput<'_>) -> bool {
        if let Some(exhausted) =
            provider_pool_quota_snapshot_exhausted_decision(input.key, input.provider_type)
        {
            return exhausted;
        }
        provider_pool_metadata_bucket(input.key.upstream_metadata.as_ref(), input.provider_type)
            .is_some_and(quota_exhausted_from_bucket)
    }

    fn quota_refresh_endpoint(
        &self,
        endpoints: &[StoredProviderCatalogEndpoint],
        include_inactive: bool,
    ) -> Option<StoredProviderCatalogEndpoint> {
        provider_pool_matching_endpoint(endpoints, include_inactive, |endpoint| {
            provider_pool_endpoint_format_matches(endpoint, "openai:responses")
        })
    }

    fn quota_refresh_missing_endpoint_message(&self) -> String {
        "找不到有效的 openai:responses 端点".to_string()
    }
}

pub fn build_codex_pool_quota_request(
    key_id: &str,
    resolved_oauth_auth: Option<(String, String)>,
    decrypted_api_key: Option<&str>,
    auth_config: Option<&Value>,
) -> Result<ProviderPoolQuotaRequestSpec, String> {
    let headers = build_codex_quota_headers(resolved_oauth_auth, decrypted_api_key, auth_config)?;

    Ok(ProviderPoolQuotaRequestSpec {
        request_id: format!("codex-quota:{key_id}"),
        provider_name: "codex".to_string(),
        quota_kind: "codex".to_string(),
        method: "GET".to_string(),
        url: CODEX_WHAM_USAGE_URL.to_string(),
        headers,
        content_type: None,
        json_body: None,
        client_api_format: "openai:responses".to_string(),
        provider_api_format: "openai:responses".to_string(),
        model_name: Some("codex-wham-usage".to_string()),
        accept_invalid_certs: false,
    })
}

pub fn build_codex_pool_quota_reset_request(
    key_id: &str,
    redeem_request_id: &str,
    resolved_oauth_auth: Option<(String, String)>,
    decrypted_api_key: Option<&str>,
    auth_config: Option<&Value>,
) -> Result<ProviderPoolQuotaRequestSpec, String> {
    let redeem_request_id = redeem_request_id.trim();
    if redeem_request_id.is_empty() {
        return Err("缺少额度重置幂等键".to_string());
    }
    let headers = build_codex_quota_headers(resolved_oauth_auth, decrypted_api_key, auth_config)?;

    Ok(ProviderPoolQuotaRequestSpec {
        request_id: format!("codex-quota-reset:{key_id}:{redeem_request_id}"),
        provider_name: "codex".to_string(),
        quota_kind: "codex_reset_credit".to_string(),
        method: "POST".to_string(),
        url: CODEX_WHAM_RESET_CREDITS_CONSUME_URL.to_string(),
        headers,
        content_type: Some("application/json".to_string()),
        json_body: Some(json!({ "redeem_request_id": redeem_request_id })),
        client_api_format: "openai:responses".to_string(),
        provider_api_format: "openai:responses".to_string(),
        model_name: Some("codex-wham-quota-reset".to_string()),
        accept_invalid_certs: false,
    })
}

fn build_codex_quota_headers(
    resolved_oauth_auth: Option<(String, String)>,
    decrypted_api_key: Option<&str>,
    auth_config: Option<&Value>,
) -> Result<BTreeMap<String, String>, String> {
    let mut headers = BTreeMap::new();
    headers.insert("accept".to_string(), "application/json".to_string());

    if let Some((name, value)) = resolved_oauth_auth {
        headers.insert(name.to_ascii_lowercase(), value);
    } else {
        let decrypted_key = decrypted_api_key.unwrap_or_default().trim();
        if decrypted_key.is_empty() || decrypted_key == PLACEHOLDER_API_KEY {
            return Err("缺少 OAuth 认证信息，请先授权/刷新 Token".to_string());
        }
        headers.insert(
            "authorization".to_string(),
            format!("Bearer {decrypted_key}"),
        );
    }

    let oauth_plan_type = auth_config
        .and_then(|value| value.get("plan_type"))
        .and_then(Value::as_str)
        .and_then(|value| crate::plan::normalize_provider_plan_tier(value, "codex"));
    let oauth_account_id = auth_config
        .and_then(|value| value.get("account_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if oauth_account_id.is_some() && oauth_plan_type.as_deref() != Some("free") {
        headers.insert(
            "chatgpt-account-id".to_string(),
            oauth_account_id.unwrap_or_default().to_string(),
        );
    }

    Ok(headers)
}

fn codex_window_reset_elapsed(bucket: &Map<String, Value>, prefix: &str) -> bool {
    let Some(now_unix_secs) = provider_pool_current_unix_secs() else {
        return false;
    };
    let mut window = Map::new();
    for (target, source) in [
        ("reset_at", format!("{prefix}_reset_at")),
        ("next_reset_at", format!("{prefix}_next_reset_at")),
        ("reset_seconds", format!("{prefix}_reset_seconds")),
        (
            "reset_after_seconds",
            format!("{prefix}_reset_after_seconds"),
        ),
    ] {
        if let Some(value) = bucket.get(source.as_str()) {
            window.insert(target.to_string(), value.clone());
        }
    }
    provider_pool_reset_deadline_elapsed(
        &window,
        provider_pool_timestamp_unix_secs(bucket.get("updated_at")),
        now_unix_secs,
    )
}

fn codex_window_used_percent_exhausted(bucket: &Map<String, Value>, prefix: &str) -> bool {
    let used_percent_key = format!("{prefix}_used_percent");
    provider_pool_json_f64(bucket.get(used_percent_key.as_str()))
        .is_some_and(|value| value >= 100.0 && !codex_window_reset_elapsed(bucket, prefix))
}

fn codex_metadata_window_exhausted(
    bucket: &Map<String, Value>,
    window: &Map<String, Value>,
) -> bool {
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
    let used_percent = provider_pool_json_f64(window.get("used_percent"))
        .or_else(|| provider_pool_json_f64(window.get("used_ratio")).map(|value| value * 100.0));
    let Some(used_percent) = used_percent else {
        return false;
    };
    let snapshot_observed_at = provider_pool_timestamp_unix_secs(
        bucket
            .get("updated_at")
            .or_else(|| bucket.get("observed_at")),
    );
    used_percent >= 100.0 - 1e-6
        && !provider_pool_reset_deadline_elapsed(
            window,
            snapshot_observed_at,
            provider_pool_current_unix_secs().unwrap_or(0),
        )
}

pub(crate) fn quota_exhausted_from_bucket(bucket: &Map<String, Value>) -> bool {
    if provider_pool_json_bool(bucket.get("credits_unlimited")) == Some(true) {
        return false;
    }
    let generic_windows = bucket
        .get("windows")
        .and_then(Value::as_array)
        .map(|windows| {
            windows
                .iter()
                .filter_map(Value::as_object)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !generic_windows.is_empty() {
        return generic_windows
            .iter()
            .any(|window| codex_metadata_window_exhausted(bucket, window));
    }
    let has_window_data = provider_pool_json_f64(bucket.get("primary_used_percent")).is_some()
        || provider_pool_json_f64(bucket.get("secondary_used_percent")).is_some();
    if !has_window_data && provider_pool_json_bool(bucket.get("has_credits")) == Some(false) {
        return true;
    }
    codex_window_used_percent_exhausted(bucket, "primary")
        || codex_window_used_percent_exhausted(bucket, "secondary")
}

#[cfg(test)]
mod tests {
    use super::{build_codex_pool_quota_reset_request, CODEX_WHAM_RESET_CREDITS_CONSUME_URL};
    use serde_json::json;

    #[test]
    fn codex_quota_reset_request_reuses_auth_and_account_headers() {
        let request = build_codex_pool_quota_reset_request(
            "key-codex",
            "f0b53e98-94be-4983-ab4c-d7f24a06d5fa",
            Some(("Authorization".to_string(), "Bearer refreshed".to_string())),
            None,
            Some(&json!({
                "plan_type": "pro",
                "account_id": "account-123"
            })),
        )
        .expect("reset request should build");

        assert_eq!(request.method, "POST");
        assert_eq!(request.url, CODEX_WHAM_RESET_CREDITS_CONSUME_URL);
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer refreshed")
        );
        assert_eq!(
            request
                .headers
                .get("chatgpt-account-id")
                .map(String::as_str),
            Some("account-123")
        );
        assert_eq!(
            request.json_body,
            Some(json!({
                "redeem_request_id": "f0b53e98-94be-4983-ab4c-d7f24a06d5fa"
            }))
        );
    }
}

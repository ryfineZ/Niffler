use super::super::token_import::{import_tokens_from_raw_token, normalize_provider_import_tokens};
use crate::handlers::admin::provider::oauth::errors::build_internal_control_error_response;
use crate::handlers::admin::provider::oauth::state::{
    current_unix_secs, enrich_admin_provider_oauth_auth_config, json_u64_value,
};
use axum::{
    body::{to_bytes, Body, Bytes},
    http,
    response::{IntoResponse, Response},
    Json,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct AdminProviderOAuthBatchImportRequest {
    pub credentials: String,
    pub proxy_node_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct AdminProviderOAuthBatchImportEntry {
    pub key_name: Option<String>,
    pub validation_error: Option<String>,
    pub refresh_token: Option<String>,
    pub access_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_at: Option<u64>,
    pub disabled: bool,
    pub account_id: Option<String>,
    pub account_user_id: Option<String>,
    pub plan_type: Option<String>,
    pub pool_tier: Option<String>,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub account_name: Option<String>,
    pub sso_rw_token: Option<String>,
    pub cf_cookies: Option<String>,
    pub cf_clearance: Option<String>,
    pub user_agent: Option<String>,
    pub browser_profile: Option<String>,
    pub last_refresh: Option<String>,
    pub codex_installation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct AdminProviderOAuthBatchImportOutcome {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub results: Vec<serde_json::Value>,
}

pub(super) fn parse_admin_provider_oauth_batch_import_request(
    request_body: Option<&Bytes>,
) -> Result<AdminProviderOAuthBatchImportRequest, Response<Body>> {
    let Some(request_body) = request_body else {
        return Err(
            crate::handlers::admin::provider::oauth::errors::build_internal_control_error_response(
                http::StatusCode::BAD_REQUEST,
                "请求体必须是合法的 JSON 对象",
            ),
        );
    };
    match serde_json::from_slice::<AdminProviderOAuthBatchImportRequest>(request_body) {
        Ok(payload) if !payload.credentials.trim().is_empty() => Ok(payload),
        _ => Err(
            crate::handlers::admin::provider::oauth::errors::build_internal_control_error_response(
                http::StatusCode::BAD_REQUEST,
                "请求体必须是合法的 JSON 对象",
            ),
        ),
    }
}

fn coerce_admin_provider_oauth_import_str(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn sub2api_oauth_email_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)^[A-Z0-9._%+-]{1,64}@[A-Z0-9.-]{1,253}\.[A-Z]{2,63}$")
            .expect("sub2api email regex should compile")
    })
}

fn sub2api_oauth_email_from_account_name(account_name: Option<String>) -> Option<String> {
    account_name.filter(|value| sub2api_oauth_email_regex().is_match(value))
}

fn admin_provider_oauth_import_expiry_value(value: Option<&serde_json::Value>) -> Option<u64> {
    match value? {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(value) => {
            let normalized = value.trim();
            if normalized.is_empty() {
                return None;
            }
            normalized.parse::<u64>().ok().or_else(|| {
                chrono::DateTime::parse_from_rfc3339(normalized)
                    .ok()
                    .and_then(|value| u64::try_from(value.timestamp()).ok())
            })
        }
        _ => None,
    }
}

fn admin_provider_oauth_import_bool_value(value: Option<&serde_json::Value>) -> Option<bool> {
    match value? {
        serde_json::Value::Bool(value) => Some(*value),
        serde_json::Value::Number(value) => value.as_u64().map(|value| value != 0),
        serde_json::Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" => Some(true),
            "false" | "0" | "no" | "n" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn admin_provider_oauth_import_identity_hints_from_id_token(
    provider_type: &str,
    id_token: Option<&str>,
) -> Map<String, Value> {
    let mut auth_config = Map::new();
    let Some(id_token) = id_token.map(str::trim).filter(|value| !value.is_empty()) else {
        return auth_config;
    };
    enrich_admin_provider_oauth_auth_config(
        provider_type,
        &mut auth_config,
        &json!({ "id_token": id_token }),
    );
    auth_config
}

fn admin_provider_oauth_import_hint_string(
    hints: &Map<String, Value>,
    field: &str,
) -> Option<String> {
    coerce_admin_provider_oauth_import_str(hints.get(field))
}

fn grok_cookie_value(raw: &str, name: &str) -> Option<String> {
    raw.trim()
        .strip_prefix("Cookie:")
        .unwrap_or_else(|| raw.trim())
        .split(';')
        .filter_map(|segment| segment.trim().split_once('='))
        .find_map(|(cookie_name, cookie_value)| {
            cookie_name
                .trim()
                .eq_ignore_ascii_case(name)
                .then(|| cookie_value.trim())
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn grok_cookie_profile(raw: &str) -> Option<String> {
    let raw = raw
        .trim()
        .strip_prefix("Cookie:")
        .unwrap_or_else(|| raw.trim());
    let parts = raw
        .split(';')
        .filter_map(|segment| {
            let (cookie_name, cookie_value) = segment.trim().split_once('=')?;
            let cookie_name = cookie_name.trim();
            let cookie_value = cookie_value.trim();
            if cookie_name.is_empty()
                || cookie_value.is_empty()
                || cookie_name.eq_ignore_ascii_case("sso")
                || cookie_name.eq_ignore_ascii_case("sso-rw")
            {
                return None;
            }
            Some(format!("{cookie_name}={cookie_value}"))
        })
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn grok_cookie_session_token(provider_type: &str, raw: &str) -> Option<String> {
    provider_type
        .trim()
        .eq_ignore_ascii_case("grok")
        .then(|| grok_cookie_value(raw, "sso"))
        .flatten()
}

fn extract_admin_provider_oauth_batch_import_entry(
    provider_type: &str,
    item: &serde_json::Value,
) -> Option<AdminProviderOAuthBatchImportEntry> {
    match item {
        serde_json::Value::String(value) => {
            let raw_token = value.trim();
            if raw_token.is_empty() {
                None
            } else {
                let sso_from_cookie = grok_cookie_session_token(provider_type, raw_token);
                let token_input = sso_from_cookie.as_deref().unwrap_or(raw_token);
                let (refresh_token, access_token) = import_tokens_from_raw_token(token_input);
                let (refresh_token, access_token) = normalize_provider_import_tokens(
                    provider_type,
                    refresh_token.as_deref(),
                    access_token.as_deref(),
                );
                Some(AdminProviderOAuthBatchImportEntry {
                    key_name: None,
                    validation_error: None,
                    refresh_token,
                    access_token,
                    id_token: None,
                    expires_at: None,
                    disabled: false,
                    account_id: None,
                    account_user_id: None,
                    plan_type: None,
                    pool_tier: None,
                    user_id: grok_cookie_value(raw_token, "x-userid"),
                    email: None,
                    account_name: None,
                    sso_rw_token: grok_cookie_value(raw_token, "sso-rw"),
                    cf_cookies: grok_cookie_profile(raw_token),
                    cf_clearance: grok_cookie_value(raw_token, "cf_clearance"),
                    user_agent: None,
                    browser_profile: None,
                    last_refresh: None,
                    codex_installation_id: None,
                })
            }
        }
        serde_json::Value::Object(object) => {
            let refresh_token = coerce_admin_provider_oauth_import_str(
                object
                    .get("refresh_token")
                    .or_else(|| object.get("refreshToken")),
            );
            let access_token = coerce_admin_provider_oauth_import_str(
                object
                    .get("access_token")
                    .or_else(|| object.get("accessToken")),
            );
            let id_token = coerce_admin_provider_oauth_import_str(
                object.get("id_token").or_else(|| object.get("idToken")),
            );
            let grok_token_alias = if provider_type.trim().eq_ignore_ascii_case("grok") {
                object.get("token")
            } else {
                None
            };
            let grok_cookie = if provider_type.trim().eq_ignore_ascii_case("grok") {
                coerce_admin_provider_oauth_import_str(
                    object.get("cookie").or_else(|| object.get("cookieHeader")),
                )
            } else {
                None
            };
            let session_token = coerce_admin_provider_oauth_import_str(
                object
                    .get("sso_token")
                    .or_else(|| object.get("ssoToken"))
                    .or(grok_token_alias),
            )
            .or_else(|| {
                grok_cookie
                    .as_deref()
                    .and_then(|cookie| grok_cookie_value(cookie, "sso"))
            });
            let (refresh_token, access_token) = normalize_provider_import_tokens(
                provider_type,
                refresh_token.as_deref(),
                access_token.as_deref().or(session_token.as_deref()),
            );
            if refresh_token.is_none() && access_token.is_none() {
                return None;
            }
            let id_token_hints = admin_provider_oauth_import_identity_hints_from_id_token(
                provider_type,
                id_token.as_deref(),
            );
            let expires_at = admin_provider_oauth_import_expiry_value(
                object
                    .get("expires_at")
                    .or_else(|| object.get("expiresAt"))
                    .or_else(|| object.get("expired"))
                    .or_else(|| object.get("expire"))
                    .or_else(|| object.get("expiry"))
                    .or_else(|| object.get("expires")),
            );
            let disabled =
                admin_provider_oauth_import_bool_value(object.get("disabled")).unwrap_or(false);
            let account_id = coerce_admin_provider_oauth_import_str(
                object
                    .get("account_id")
                    .or_else(|| object.get("accountId"))
                    .or_else(|| object.get("chatgpt_account_id"))
                    .or_else(|| object.get("chatgptAccountId")),
            )
            .or_else(|| admin_provider_oauth_import_hint_string(&id_token_hints, "account_id"));
            let account_user_id = coerce_admin_provider_oauth_import_str(
                object
                    .get("account_user_id")
                    .or_else(|| object.get("accountUserId"))
                    .or_else(|| object.get("chatgpt_account_user_id"))
                    .or_else(|| object.get("chatgptAccountUserId")),
            )
            .or_else(|| {
                admin_provider_oauth_import_hint_string(&id_token_hints, "account_user_id")
            });
            let plan_type = coerce_admin_provider_oauth_import_str(
                object
                    .get("plan_type")
                    .or_else(|| object.get("planType"))
                    .or_else(|| object.get("chatgpt_plan_type"))
                    .or_else(|| object.get("chatgptPlanType")),
            )
            .or_else(|| admin_provider_oauth_import_hint_string(&id_token_hints, "plan_type"))
            .map(|value| value.to_ascii_lowercase());
            let pool_tier = coerce_admin_provider_oauth_import_str(
                object
                    .get("pool_tier")
                    .or_else(|| object.get("poolTier"))
                    .or_else(|| object.get("tier")),
            )
            .map(|value| value.to_ascii_lowercase());
            let user_id = coerce_admin_provider_oauth_import_str(
                object
                    .get("user_id")
                    .or_else(|| object.get("userId"))
                    .or_else(|| object.get("chatgpt_user_id"))
                    .or_else(|| object.get("chatgptUserId")),
            )
            .or_else(|| admin_provider_oauth_import_hint_string(&id_token_hints, "user_id"))
            .or_else(|| {
                grok_cookie
                    .as_deref()
                    .and_then(|cookie| grok_cookie_value(cookie, "x-userid"))
            });
            let email = coerce_admin_provider_oauth_import_str(object.get("email"))
                .or_else(|| admin_provider_oauth_import_hint_string(&id_token_hints, "email"));
            let account_name = coerce_admin_provider_oauth_import_str(
                object
                    .get("account_name")
                    .or_else(|| object.get("accountName")),
            )
            .or_else(|| admin_provider_oauth_import_hint_string(&id_token_hints, "account_name"));
            let sso_rw_token = coerce_admin_provider_oauth_import_str(
                object
                    .get("sso_rw_token")
                    .or_else(|| object.get("ssoRwToken")),
            )
            .or_else(|| {
                grok_cookie
                    .as_deref()
                    .and_then(|cookie| grok_cookie_value(cookie, "sso-rw"))
            });
            let cf_clearance = coerce_admin_provider_oauth_import_str(
                object
                    .get("cf_clearance")
                    .or_else(|| object.get("cfClearance")),
            )
            .or_else(|| {
                grok_cookie
                    .as_deref()
                    .and_then(|cookie| grok_cookie_value(cookie, "cf_clearance"))
            });
            let cf_cookies = coerce_admin_provider_oauth_import_str(
                object.get("cf_cookies").or_else(|| object.get("cfCookies")),
            )
            .or_else(|| grok_cookie.as_deref().and_then(grok_cookie_profile));
            let user_agent = coerce_admin_provider_oauth_import_str(
                object.get("user_agent").or_else(|| object.get("userAgent")),
            );
            let browser_profile = coerce_admin_provider_oauth_import_str(
                object
                    .get("browser_profile")
                    .or_else(|| object.get("browserProfile"))
                    .or_else(|| object.get("browser"))
                    .or_else(|| object.get("impersonate")),
            );
            let last_refresh = coerce_admin_provider_oauth_import_str(
                object
                    .get("last_refresh")
                    .or_else(|| object.get("lastRefresh"))
                    .or_else(|| object.get("last_refreshed_at"))
                    .or_else(|| object.get("lastRefreshedAt")),
            );
            Some(AdminProviderOAuthBatchImportEntry {
                key_name: None,
                validation_error: None,
                refresh_token,
                access_token,
                id_token,
                expires_at,
                disabled,
                account_id,
                account_user_id,
                plan_type,
                pool_tier,
                user_id,
                email,
                account_name,
                sso_rw_token,
                cf_cookies,
                cf_clearance,
                user_agent,
                browser_profile,
                last_refresh,
                codex_installation_id: None,
            })
        }
        _ => None,
    }
}

fn invalid_sub2api_oauth_import_entry(
    error: impl Into<String>,
) -> AdminProviderOAuthBatchImportEntry {
    AdminProviderOAuthBatchImportEntry {
        key_name: None,
        validation_error: Some(error.into()),
        refresh_token: None,
        access_token: None,
        id_token: None,
        expires_at: None,
        disabled: false,
        account_id: None,
        account_user_id: None,
        plan_type: None,
        pool_tier: None,
        user_id: None,
        email: None,
        account_name: None,
        sso_rw_token: None,
        cf_cookies: None,
        cf_clearance: None,
        user_agent: None,
        browser_profile: None,
        last_refresh: None,
        codex_installation_id: None,
    }
}

fn sub2api_oauth_import_string(object: &Map<String, Value>, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| coerce_admin_provider_oauth_import_str(object.get(*field)))
}

fn sub2api_oauth_key_name(
    email: Option<&str>,
    user_id: Option<&str>,
    account_id: Option<&str>,
    workspace_name: Option<&str>,
) -> Option<String> {
    let account_label = email.or(user_id)?.trim();
    if account_label.is_empty() {
        return None;
    }

    let workspace_label = workspace_name
        .or(account_id)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != account_label);
    Some(match workspace_label {
        Some(workspace_label) => format!("{account_label} · {workspace_label}"),
        None => account_label.to_string(),
    })
}

fn extract_sub2api_oauth_import_entry(
    provider_type: &str,
    item: &Value,
) -> AdminProviderOAuthBatchImportEntry {
    let Value::Object(account) = item else {
        return invalid_sub2api_oauth_import_entry("sub2api 账号条目必须是 JSON 对象");
    };

    if !matches!(
        provider_type.trim().to_ascii_lowercase().as_str(),
        "codex" | "chatgpt_web"
    ) {
        return invalid_sub2api_oauth_import_entry(
            "当前 Provider 不支持 sub2api OpenAI OAuth 账号导入",
        );
    }

    let platform = sub2api_oauth_import_string(account, &["platform"]);
    if !platform
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("openai"))
    {
        return invalid_sub2api_oauth_import_entry(
            "sub2api 账号平台不是 openai，不能导入当前 Provider",
        );
    }
    let account_type = sub2api_oauth_import_string(account, &["type"]);
    if !account_type
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("oauth"))
    {
        return invalid_sub2api_oauth_import_entry(
            "sub2api 账号类型不是 oauth，不能导入当前 Provider",
        );
    }

    let Some(credentials) = account.get("credentials").and_then(Value::as_object) else {
        return invalid_sub2api_oauth_import_entry("sub2api 账号缺少 credentials 凭证对象");
    };
    let Some(mut entry) = extract_admin_provider_oauth_batch_import_entry(
        provider_type,
        &Value::Object(credentials.clone()),
    ) else {
        return invalid_sub2api_oauth_import_entry(
            "sub2api 账号凭证缺少 access_token 或 refresh_token",
        );
    };

    if provider_type.trim().eq_ignore_ascii_case("codex") {
        entry.codex_installation_id = account
            .get("extra")
            .and_then(Value::as_object)
            .and_then(|extra| extra.get("openai_device_id"))
            .and_then(Value::as_str)
            .and_then(valid_sub2api_openai_device_id);
    }

    let account_name_email =
        sub2api_oauth_email_from_account_name(sub2api_oauth_import_string(account, &["name"]));
    let email = entry.email.clone().or(account_name_email);
    if entry.email.is_none() {
        entry.email = email.clone();
    }
    let workspace_name = sub2api_oauth_import_string(
        credentials,
        &[
            "workspace_name",
            "space_name",
            "chatgpt_account_name",
            "organization_name",
            "account_name",
        ],
    );
    if workspace_name.is_some() {
        entry.account_name = workspace_name.clone();
    }
    entry.key_name = sub2api_oauth_key_name(
        email.as_deref(),
        entry.user_id.as_deref(),
        entry.account_id.as_deref(),
        workspace_name.as_deref(),
    );
    if entry.key_name.is_none() {
        return invalid_sub2api_oauth_import_entry(
            "sub2api 账号缺少邮箱或用户 ID，无法确认账号身份",
        );
    }
    entry
}

fn valid_sub2api_openai_device_id(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 128
        && value.is_ascii()
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte)))
    .then(|| value.to_string())
}

fn sub2api_oauth_accounts(
    object: &Map<String, Value>,
) -> Option<Result<&Vec<Value>, &'static str>> {
    let has_sub2api_type = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| matches!(value, "sub2api-data" | "sub2api-bundle"));
    let has_legacy_sub2api_shape = object.contains_key("exported_at")
        && object.contains_key("proxies")
        && object.contains_key("accounts");
    let looks_like_sub2api_export = has_sub2api_type || has_legacy_sub2api_shape;
    looks_like_sub2api_export.then(|| {
        object
            .get("accounts")
            .and_then(Value::as_array)
            .ok_or("sub2api 导出文件缺少 accounts 数组")
    })
}

pub(super) fn parse_admin_provider_oauth_batch_import_entries(
    provider_type: &str,
    raw_credentials: &str,
) -> Vec<AdminProviderOAuthBatchImportEntry> {
    let raw = raw_credentials.trim();
    if raw.is_empty() {
        return Vec::new();
    }

    if raw.starts_with('[') {
        if let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(raw)
        {
            return items
                .iter()
                .filter_map(|item| {
                    extract_admin_provider_oauth_batch_import_entry(provider_type, item)
                })
                .collect();
        }
    }

    if raw.starts_with('{') {
        if let Ok(value @ serde_json::Value::Object(_)) = serde_json::from_str::<Value>(raw) {
            if let Value::Object(object) = &value {
                if let Some(accounts) = sub2api_oauth_accounts(object) {
                    return match accounts {
                        Ok(accounts) => accounts
                            .iter()
                            .map(|item| extract_sub2api_oauth_import_entry(provider_type, item))
                            .collect(),
                        Err(error) => vec![invalid_sub2api_oauth_import_entry(error)],
                    };
                }
            }
            return extract_admin_provider_oauth_batch_import_entry(provider_type, &value)
                .into_iter()
                .collect();
        }
    }

    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            if line.starts_with('{') {
                return serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .and_then(|value| {
                        extract_admin_provider_oauth_batch_import_entry(provider_type, &value)
                    });
            }

            extract_admin_provider_oauth_batch_import_entry(
                provider_type,
                &serde_json::Value::String(line.to_string()),
            )
        })
        .collect()
}

pub(super) fn apply_admin_provider_oauth_batch_import_hints(
    provider_type: &str,
    entry: &AdminProviderOAuthBatchImportEntry,
    auth_config: &mut serde_json::Map<String, serde_json::Value>,
) {
    let provider_type = provider_type.trim().to_ascii_lowercase();
    if !matches!(provider_type.as_str(), "codex" | "chatgpt_web" | "grok") {
        return;
    }
    if let Some(id_token) = entry.id_token.as_ref() {
        enrich_admin_provider_oauth_auth_config(
            &provider_type,
            auth_config,
            &json!({ "id_token": id_token }),
        );
        auth_config
            .entry("id_token".to_string())
            .or_insert_with(|| json!(id_token));
    }
    if let Some(account_id) = entry.account_id.as_ref() {
        auth_config
            .entry("account_id".to_string())
            .or_insert_with(|| json!(account_id));
    }
    if let Some(account_user_id) = entry.account_user_id.as_ref() {
        auth_config
            .entry("account_user_id".to_string())
            .or_insert_with(|| json!(account_user_id));
    }
    if let Some(plan_type) = entry.plan_type.as_ref() {
        auth_config
            .entry("plan_type".to_string())
            .or_insert_with(|| json!(plan_type));
    }
    if let Some(pool_tier) = entry.pool_tier.as_ref() {
        auth_config
            .entry("pool_tier".to_string())
            .or_insert_with(|| json!(pool_tier));
    }
    if let Some(user_id) = entry.user_id.as_ref() {
        auth_config
            .entry("user_id".to_string())
            .or_insert_with(|| json!(user_id));
    }
    if let Some(email) = entry.email.as_ref() {
        auth_config
            .entry("email".to_string())
            .or_insert_with(|| json!(email));
    }
    if let Some(account_name) = entry.account_name.as_ref() {
        auth_config
            .entry("account_name".to_string())
            .or_insert_with(|| json!(account_name));
    }
    if let Some(sso_rw_token) = entry.sso_rw_token.as_ref() {
        auth_config
            .entry("sso_rw_token".to_string())
            .or_insert_with(|| json!(sso_rw_token));
    }
    if let Some(cf_cookies) = entry.cf_cookies.as_ref() {
        auth_config
            .entry("cf_cookies".to_string())
            .or_insert_with(|| json!(cf_cookies));
    }
    if let Some(cf_clearance) = entry.cf_clearance.as_ref() {
        auth_config
            .entry("cf_clearance".to_string())
            .or_insert_with(|| json!(cf_clearance));
    }
    if let Some(user_agent) = entry.user_agent.as_ref() {
        auth_config
            .entry("user_agent".to_string())
            .or_insert_with(|| json!(user_agent));
    }
    if let Some(browser_profile) = entry.browser_profile.as_ref() {
        auth_config
            .entry("browser_profile".to_string())
            .or_insert_with(|| json!(browser_profile));
    }
    if let Some(last_refresh) = entry.last_refresh.as_ref() {
        auth_config
            .entry("last_refresh".to_string())
            .or_insert_with(|| json!(last_refresh));
    }
    if entry.disabled {
        auth_config
            .entry("disabled".to_string())
            .or_insert_with(|| json!(true));
    }
}

pub(super) async fn extract_admin_provider_oauth_batch_error_detail(
    response: Response<Body>,
) -> String {
    let status = response.status();
    let raw_body = to_bytes(response.into_body(), usize::MAX).await.ok();
    if let Some(raw_body) = raw_body {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw_body) {
            if let Some(detail) = value.get("detail").and_then(serde_json::Value::as_str) {
                let normalized = detail.trim();
                if !normalized.is_empty() {
                    return normalized.to_string();
                }
            }
        }
        let normalized = String::from_utf8_lossy(&raw_body).trim().to_string();
        if !normalized.is_empty() {
            return normalized;
        }
    }
    format!("HTTP {}", status.as_u16())
}

pub(super) fn build_admin_provider_oauth_batch_import_response(
    outcome: &AdminProviderOAuthBatchImportOutcome,
) -> Json<serde_json::Value> {
    Json(json!({
        "total": outcome.total,
        "success": outcome.success,
        "failed": outcome.failed,
        "results": outcome.results,
    }))
}

pub(super) fn build_admin_provider_oauth_batch_task_state(
    task_id: &str,
    provider_id: &str,
    provider_type: &str,
    status: &str,
    total: usize,
    processed: usize,
    success: usize,
    failed: usize,
    created_count: usize,
    replaced_count: usize,
    message: Option<&str>,
    error: Option<&str>,
    error_samples: Vec<serde_json::Value>,
    created_at: u64,
    started_at: Option<u64>,
    finished_at: Option<u64>,
) -> serde_json::Value {
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .unwrap_or(created_at);
    let progress_percent = processed
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or(0)
        .min(100) as u64;
    json!({
        "task_id": task_id,
        "provider_id": provider_id,
        "provider_type": provider_type,
        "status": status,
        "total": total,
        "processed": processed,
        "success": success,
        "failed": failed,
        "created_count": created_count,
        "replaced_count": replaced_count,
        "progress_percent": progress_percent,
        "message": message,
        "error": error,
        "error_samples": error_samples,
        "created_at": created_at,
        "started_at": started_at,
        "finished_at": finished_at,
        "updated_at": updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_admin_provider_oauth_batch_import_entries;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serde_json::json;

    fn unsigned_jwt(payload: serde_json::Value) -> String {
        let header = json!({"alg": "none", "typ": "JWT"});
        let encode = |value: serde_json::Value| {
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).expect("jwt json should serialize"))
        };
        format!("{}.{}.signature", encode(header), encode(payload))
    }

    #[test]
    fn parses_access_token_only_entry() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            r#"[{"accessToken":"at_1","expiresAt":2100000000,"accountId":"acc-1","email":"u@example.com"}]"#,
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].refresh_token, None);
        assert_eq!(entries[0].access_token.as_deref(), Some("at_1"));
        assert_eq!(entries[0].expires_at, Some(2_100_000_000));
        assert_eq!(entries[0].account_id.as_deref(), Some("acc-1"));
        assert_eq!(entries[0].email.as_deref(), Some("u@example.com"));
    }

    #[test]
    fn parses_sub2api_export_accounts_with_account_and_workspace_names() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            &json!({
                "exported_at": "2026-07-14T10:36:05Z",
                "proxies": [],
                "accounts": [
                    {
                        "name": "ignored-custom-name",
                        "platform": "openai",
                        "type": "oauth",
                        "extra": {
                            "openai_device_id": "device-from-sub2api",
                            "codex_fingerprint_mode": "full",
                            "untrusted": "ignored"
                        },
                        "credentials": {
                            "access_token": "pat-1",
                            "email": "first@example.com",
                            "chatgpt_account_id": "workspace-1",
                            "chatgpt_user_id": "user-1",
                            "workspace_name": "研发空间",
                            "plan_type": "team"
                        }
                    },
                    {
                        "name": "also-ignored",
                        "platform": "openai",
                        "type": "oauth",
                        "credentials": {
                            "access_token": "pat-2",
                            "email": "second@example.com",
                            "chatgpt_account_id": "workspace-1",
                            "chatgpt_user_id": "user-2",
                            "plan_type": "team"
                        }
                    }
                ]
            })
            .to_string(),
        );

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].access_token.as_deref(), Some("pat-1"));
        assert_eq!(entries[0].account_id.as_deref(), Some("workspace-1"));
        assert_eq!(entries[0].user_id.as_deref(), Some("user-1"));
        assert_eq!(entries[0].plan_type.as_deref(), Some("team"));
        assert_eq!(entries[0].account_name.as_deref(), Some("研发空间"));
        assert_eq!(
            entries[0].codex_installation_id.as_deref(),
            Some("device-from-sub2api")
        );
        assert_eq!(
            entries[0].key_name.as_deref(),
            Some("first@example.com · 研发空间")
        );
        assert_eq!(
            entries[1].key_name.as_deref(),
            Some("second@example.com · workspace-1")
        );
        assert_eq!(entries[0].validation_error, None);
        assert_eq!(entries[1].validation_error, None);
        assert_eq!(entries[1].codex_installation_id, None);
    }

    #[test]
    fn ignores_invalid_sub2api_openai_device_id() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            &json!({
                "type": "sub2api-data",
                "accounts": [{
                    "name": "device@example.com",
                    "platform": "openai",
                    "type": "oauth",
                    "extra": { "openai_device_id": "invalid\ndevice" },
                    "credentials": { "access_token": "token" }
                }]
            })
            .to_string(),
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].codex_installation_id, None);
    }

    #[test]
    fn parses_sub2api_account_name_as_email_when_credentials_email_is_missing() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            &json!({
                "type": "sub2api-data",
                "version": 1,
                "exported_at": "2026-07-31T05:55:38Z",
                "proxies": [],
                "accounts": [{
                    "name": "straw.mana_8o@icloud.com",
                    "platform": "openai",
                    "type": "oauth",
                    "credentials": {
                        "access_token": "sub2api-access-token",
                        "chatgpt_account_id": "workspace-from-sub2api",
                        "plan_type": "team"
                    }
                }]
            })
            .to_string(),
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].access_token.as_deref(),
            Some("sub2api-access-token")
        );
        assert_eq!(
            entries[0].email.as_deref(),
            Some("straw.mana_8o@icloud.com")
        );
        assert_eq!(
            entries[0].account_id.as_deref(),
            Some("workspace-from-sub2api")
        );
        assert_eq!(
            entries[0].key_name.as_deref(),
            Some("straw.mana_8o@icloud.com · workspace-from-sub2api")
        );
        assert_eq!(entries[0].validation_error, None);
    }

    #[test]
    fn keeps_invalid_sub2api_accounts_as_failed_entries_without_token_data() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            &json!({
                "exported_at": "2026-07-14T10:36:05Z",
                "proxies": [],
                "accounts": [
                    {
                        "platform": "anthropic",
                        "type": "oauth",
                        "credentials": { "access_token": "secret-token" }
                    },
                    {
                        "platform": "openai",
                        "type": "oauth",
                        "credentials": {}
                    }
                ]
            })
            .to_string(),
        );

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].access_token, None);
        assert!(entries[0]
            .validation_error
            .as_deref()
            .is_some_and(|error| error.contains("平台")));
        assert!(!entries[0]
            .validation_error
            .as_deref()
            .unwrap_or_default()
            .contains("secret-token"));
        assert!(entries[1]
            .validation_error
            .as_deref()
            .is_some_and(|error| error.contains("凭证")));
    }

    #[test]
    fn reports_sub2api_export_without_accounts_array() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            &json!({
                "exported_at": "2026-07-14T10:36:05Z",
                "proxies": [],
                "accounts": {}
            })
            .to_string(),
        );

        assert_eq!(entries.len(), 1);
        assert!(entries[0]
            .validation_error
            .as_deref()
            .is_some_and(|error| error.contains("accounts 数组")));
    }

    #[test]
    fn keeps_single_account_json_with_exported_at_on_legacy_path() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            &json!({
                "access_token": "legacy-token",
                "email": "legacy@example.com",
                "exported_at": "2026-07-14T10:36:05Z"
            })
            .to_string(),
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].access_token.as_deref(), Some("legacy-token"));
        assert_eq!(entries[0].validation_error, None);
    }

    #[test]
    fn rejects_sub2api_account_with_only_workspace_id() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            &json!({
                "exported_at": "2026-07-14T10:36:05Z",
                "proxies": [],
                "accounts": [{
                    "name": "friendly account",
                    "platform": "openai",
                    "type": "oauth",
                    "credentials": {
                        "access_token": "pat-workspace-only",
                        "chatgpt_account_id": "workspace-1"
                    }
                }]
            })
            .to_string(),
        );

        assert_eq!(entries.len(), 1);
        assert!(entries[0]
            .validation_error
            .as_deref()
            .is_some_and(|error| error.contains("邮箱或用户 ID")));
        assert_eq!(entries[0].access_token, None);
    }

    #[test]
    fn parses_cpa_codex_auth_json() {
        let id_token = unsigned_jwt(json!({
                "email": "cpa@example.com",
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": "acc-cpa",
                    "chatgpt_account_user_id": "account-user-cpa",
                    "chatgpt_user_id": "user-cpa",
                    "chatgpt_plan_type": "plus"
                }
        }));
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "codex",
            &json!({
                "type": "codex",
                "email": "cpa@example.com",
                "account_id": "acc-cpa",
                "chatgpt_account_id": "acc-cpa",
                "chatgpt_account_user_id": "account-user-cpa",
                "chatgpt_plan_type": "plus",
                "id_token": id_token,
                "access_token": "access-cpa",
                "refresh_token": "refresh-cpa",
                "last_refresh": "2026-05-27T20:10:51.855318Z",
                "expired": "2026-06-05T20:17:17Z",
                "disabled": true
            })
            .to_string(),
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].refresh_token.as_deref(), Some("refresh-cpa"));
        assert_eq!(entries[0].access_token.as_deref(), Some("access-cpa"));
        assert_eq!(entries[0].id_token.as_deref(), Some(id_token.as_str()));
        assert_eq!(entries[0].expires_at, Some(1_780_690_637));
        assert!(entries[0].disabled);
        assert_eq!(entries[0].account_id.as_deref(), Some("acc-cpa"));
        assert_eq!(
            entries[0].account_user_id.as_deref(),
            Some("account-user-cpa")
        );
        assert_eq!(entries[0].user_id.as_deref(), Some("user-cpa"));
        assert_eq!(entries[0].plan_type.as_deref(), Some("plus"));
        assert_eq!(entries[0].email.as_deref(), Some("cpa@example.com"));
        assert_eq!(
            entries[0].last_refresh.as_deref(),
            Some("2026-05-27T20:10:51.855318Z")
        );
    }

    #[test]
    fn parses_plain_jwt_line_as_access_token() {
        let token = unsigned_jwt(json!({
            "iss": "https://auth.openai.com",
            "aud": ["https://api.openai.com/v1"],
            "exp": 2_000_000_000u64,
        }));

        let entries = parse_admin_provider_oauth_batch_import_entries("codex", &token);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].refresh_token, None);
        assert_eq!(entries[0].access_token.as_deref(), Some(token.as_str()));
    }

    #[test]
    fn parses_grok_jsonl_session_entries() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "grok",
            r#"{"sso_token":"sso-1","cf_clearance":"cf-1","pool_tier":"heavy","email":"grok@example.com","browser_profile":"chrome136"}"#,
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].refresh_token, None);
        assert_eq!(entries[0].access_token.as_deref(), Some("sso-1"));
        assert_eq!(entries[0].cf_clearance.as_deref(), Some("cf-1"));
        assert_eq!(entries[0].pool_tier.as_deref(), Some("heavy"));
        assert_eq!(entries[0].email.as_deref(), Some("grok@example.com"));
        assert_eq!(entries[0].browser_profile.as_deref(), Some("chrome136"));
    }

    #[test]
    fn parses_grok_token_alias_with_account_traits() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "grok",
            r#"[{"token":"sso-1","planType":"super","tier":"heavy","accountName":"Grok Heavy"}]"#,
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].refresh_token, None);
        assert_eq!(entries[0].access_token.as_deref(), Some("sso-1"));
        assert_eq!(entries[0].plan_type.as_deref(), Some("super"));
        assert_eq!(entries[0].pool_tier.as_deref(), Some("heavy"));
        assert_eq!(entries[0].account_name.as_deref(), Some("Grok Heavy"));
    }

    #[test]
    fn parses_grok_plain_line_as_session_token() {
        let entries = parse_admin_provider_oauth_batch_import_entries("grok", "opaque-sso-token");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].refresh_token, None);
        assert_eq!(entries[0].access_token.as_deref(), Some("opaque-sso-token"));
    }

    #[test]
    fn parses_grok_cookie_line_as_session_metadata() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "grok",
            "i18nextLng=zh; cf_clearance=cf-1; sso-rw=rw-1; sso=sso-1; x-userid=user-1",
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].refresh_token, None);
        assert_eq!(entries[0].access_token.as_deref(), Some("sso-1"));
        assert_eq!(entries[0].sso_rw_token.as_deref(), Some("rw-1"));
        assert_eq!(
            entries[0].cf_cookies.as_deref(),
            Some("i18nextLng=zh; cf_clearance=cf-1; x-userid=user-1")
        );
        assert_eq!(entries[0].cf_clearance.as_deref(), Some("cf-1"));
        assert_eq!(entries[0].user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn parses_grok_cookie_object_as_session_metadata() {
        let entries = parse_admin_provider_oauth_batch_import_entries(
            "grok",
            r#"[{"cookie":"cf_clearance=cf-1; sso-rw=rw-1; sso=sso-1; x-userid=user-1","tier":"heavy"}]"#,
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].refresh_token, None);
        assert_eq!(entries[0].access_token.as_deref(), Some("sso-1"));
        assert_eq!(entries[0].sso_rw_token.as_deref(), Some("rw-1"));
        assert_eq!(
            entries[0].cf_cookies.as_deref(),
            Some("cf_clearance=cf-1; x-userid=user-1")
        );
        assert_eq!(entries[0].cf_clearance.as_deref(), Some("cf-1"));
        assert_eq!(entries[0].user_id.as_deref(), Some("user-1"));
        assert_eq!(entries[0].pool_tier.as_deref(), Some("heavy"));
    }
}

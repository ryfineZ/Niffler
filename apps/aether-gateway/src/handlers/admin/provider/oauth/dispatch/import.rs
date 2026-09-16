use super::super::duplicates::find_duplicate_provider_oauth_key_for_import;
use super::super::errors::build_internal_control_error_response;
use super::super::provisioning::{
    build_provider_oauth_auth_config_from_token_payload, create_provider_oauth_catalog_key,
    provider_oauth_active_api_formats, provider_oauth_key_proxy_value,
    update_existing_provider_oauth_catalog_key,
};
use super::super::runtime::{
    resolve_provider_oauth_runtime_endpoints,
    spawn_provider_oauth_account_state_refresh_after_update,
};
use super::super::state::{
    admin_provider_oauth_template, build_admin_provider_oauth_backend_unavailable_response,
    enrich_admin_provider_oauth_auth_config, exchange_admin_provider_oauth_refresh_token,
    is_fixed_provider_type_for_provider_oauth, json_u64_value, provider_oauth_transport_context,
};
use super::helpers::admin_provider_oauth_key_name_from_auth_config;
use super::token_import::{
    build_provider_access_token_import_auth_config, normalize_provider_import_tokens,
    provider_type_supports_access_token_import,
};
use crate::handlers::admin::provider::shared::paths::admin_provider_oauth_import_provider_id;
use crate::handlers::admin::request::{
    AdminAppState, AdminProviderOAuthTemplate, AdminRequestContext,
};
use crate::GatewayError;
use aether_contracts::ProxySnapshot;
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

struct AdminProviderOAuthSingleImportTokens {
    access_token: String,
    auth_config: serde_json::Map<String, serde_json::Value>,
    expires_at: Option<u64>,
}

fn import_payload_string(
    payload: &serde_json::Map<String, serde_json::Value>,
    snake_case: &str,
    camel_case: &str,
) -> Option<String> {
    import_payload_string_any(payload, &[snake_case, camel_case])
}

fn import_payload_string_any(
    payload: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .find_map(|key| payload.get(*key))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn import_payload_u64(
    payload: &serde_json::Map<String, serde_json::Value>,
    snake_case: &str,
    camel_case: &str,
) -> Option<u64> {
    json_u64_value(payload.get(snake_case).or_else(|| payload.get(camel_case)))
}

fn import_payload_bool_any(
    payload: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<bool> {
    keys.iter()
        .find_map(|key| payload.get(*key))
        .and_then(|value| match value {
            serde_json::Value::Bool(value) => Some(*value),
            serde_json::Value::Number(value) => value.as_u64().map(|value| value != 0),
            serde_json::Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "y" => Some(true),
                "false" | "0" | "no" | "n" => Some(false),
                _ => None,
            },
            _ => None,
        })
}

fn import_payload_expiry_unix_secs(
    payload: &serde_json::Map<String, serde_json::Value>,
) -> Option<u64> {
    let value = payload
        .get("expires_at")
        .or_else(|| payload.get("expiresAt"))
        .or_else(|| payload.get("expired"))
        .or_else(|| payload.get("expire"))
        .or_else(|| payload.get("expiry"))
        .or_else(|| payload.get("expires"))?;
    json_u64_value(Some(value)).or_else(|| {
        value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| {
                chrono::DateTime::parse_from_rfc3339(value)
                    .ok()
                    .and_then(|value| u64::try_from(value.timestamp()).ok())
            })
    })
}

fn apply_single_import_hints(
    provider_type: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
    auth_config: &mut serde_json::Map<String, serde_json::Value>,
) {
    let provider_type = provider_type.trim().to_ascii_lowercase();
    if !matches!(provider_type.as_str(), "codex" | "chatgpt_web" | "grok") {
        return;
    }

    if let Some(id_token) = import_payload_string_any(payload, &["id_token", "idToken"]) {
        enrich_admin_provider_oauth_auth_config(
            &provider_type,
            auth_config,
            &json!({ "id_token": id_token }),
        );
        auth_config
            .entry("id_token".to_string())
            .or_insert_with(|| json!(id_token));
    }

    for (target, keys) in [
        ("email", &["email", "oauth_email"][..]),
        (
            "account_id",
            &[
                "account_id",
                "accountId",
                "chatgpt_account_id",
                "chatgptAccountId",
            ][..],
        ),
        (
            "account_user_id",
            &[
                "account_user_id",
                "accountUserId",
                "chatgpt_account_user_id",
                "chatgptAccountUserId",
            ][..],
        ),
        (
            "plan_type",
            &[
                "plan_type",
                "planType",
                "chatgpt_plan_type",
                "chatgptPlanType",
            ][..],
        ),
        (
            "user_id",
            &["user_id", "userId", "chatgpt_user_id", "chatgptUserId"][..],
        ),
        ("account_name", &["account_name", "accountName"][..]),
        ("sso_rw_token", &["sso_rw_token", "ssoRwToken"][..]),
        (
            "cf_cookies",
            &["cf_cookies", "cfCookies", "cookie", "cookieHeader"][..],
        ),
        ("cf_clearance", &["cf_clearance", "cfClearance"][..]),
        ("user_agent", &["user_agent", "userAgent"][..]),
        (
            "browser_profile",
            &[
                "browser_profile",
                "browserProfile",
                "browser",
                "impersonate",
            ][..],
        ),
        ("pool_tier", &["pool_tier", "poolTier", "tier"][..]),
        (
            "last_refresh",
            &[
                "last_refresh",
                "lastRefresh",
                "last_refreshed_at",
                "lastRefreshedAt",
            ][..],
        ),
    ] {
        let Some(value) = import_payload_string_any(payload, keys) else {
            continue;
        };
        auth_config.entry(target.to_string()).or_insert_with(|| {
            if target == "plan_type" || target == "pool_tier" {
                json!(value.to_ascii_lowercase())
            } else {
                json!(value)
            }
        });
    }
    if let Some(disabled) = import_payload_bool_any(payload, &["disabled"]) {
        auth_config
            .entry("disabled".to_string())
            .or_insert_with(|| json!(disabled));
    }
}

async fn resolve_admin_provider_oauth_single_import_tokens(
    state: &AdminAppState<'_>,
    template: Option<AdminProviderOAuthTemplate>,
    provider_id: &str,
    provider_type: &str,
    provider_config: Option<serde_json::Value>,
    refresh_token: Option<&str>,
    access_token: Option<&str>,
    imported_expires_at: Option<u64>,
    request_proxy: Option<ProxySnapshot>,
) -> Result<AdminProviderOAuthSingleImportTokens, Response<Body>> {
    if let Some(refresh_token) = refresh_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let Some(template) = template else {
            if provider_type_supports_access_token_import(provider_type) {
                if let Some(access_token) = access_token
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    let (auth_config, expires_at) = build_provider_access_token_import_auth_config(
                        provider_type,
                        access_token,
                        Some(refresh_token),
                        imported_expires_at,
                        Some("Provider 不支持 Refresh Token 交换，已回退为 Session Token 导入"),
                    );
                    return Ok(AdminProviderOAuthSingleImportTokens {
                        access_token: access_token.to_string(),
                        auth_config,
                        expires_at,
                    });
                }
            }
            return Err(build_internal_control_error_response(
                http::StatusCode::BAD_REQUEST,
                "该 Provider 不支持 Refresh Token 导入，请提供 sso_token 或 access_token",
            ));
        };

        let exchange_ctx = provider_oauth_transport_context(
            provider_id,
            provider_type,
            None,
            None,
            provider_config,
            None,
            request_proxy.clone(),
        );
        let token_payload = match state
            .exchange_admin_provider_oauth_refresh_token(template, exchange_ctx, refresh_token)
            .await
        {
            Ok(payload) => payload,
            Err(response) => {
                if provider_type_supports_access_token_import(provider_type) {
                    if let Some(access_token) = access_token
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        let (auth_config, expires_at) =
                            build_provider_access_token_import_auth_config(
                                provider_type,
                                access_token,
                                Some(refresh_token),
                                imported_expires_at,
                                Some("Refresh Token 验证失败，已回退为 Access Token 导入"),
                            );
                        return Ok(AdminProviderOAuthSingleImportTokens {
                            access_token: access_token.to_string(),
                            auth_config,
                            expires_at,
                        });
                    }
                }
                return Err(response);
            }
        };

        let (mut auth_config, access_token, returned_refresh_token, expires_at) =
            build_provider_oauth_auth_config_from_token_payload(provider_type, &token_payload);
        let Some(access_token) = access_token else {
            return Err(build_internal_control_error_response(
                http::StatusCode::BAD_REQUEST,
                "token refresh 返回缺少 access_token",
            ));
        };
        let refresh_token = returned_refresh_token
            .or_else(|| Some(refresh_token.to_string()))
            .filter(|value| !value.trim().is_empty());
        if let Some(refresh_token) = refresh_token.as_ref() {
            auth_config.insert("refresh_token".to_string(), json!(refresh_token));
        }
        return Ok(AdminProviderOAuthSingleImportTokens {
            access_token,
            auth_config,
            expires_at,
        });
    }

    let Some(access_token) = access_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(build_internal_control_error_response(
            http::StatusCode::BAD_REQUEST,
            "Refresh Token 或 Access Token 不能为空",
        ));
    };
    if !provider_type_supports_access_token_import(provider_type) {
        return Err(build_internal_control_error_response(
            http::StatusCode::BAD_REQUEST,
            "Access Token 导入仅支持 Codex / ChatGPT Web / Grok Provider",
        ));
    }

    let (auth_config, expires_at) = build_provider_access_token_import_auth_config(
        provider_type,
        access_token,
        None,
        imported_expires_at,
        None,
    );
    Ok(AdminProviderOAuthSingleImportTokens {
        access_token: access_token.to_string(),
        auth_config,
        expires_at,
    })
}

pub(super) async fn handle_admin_provider_oauth_import_refresh_token(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_provider_catalog_data_reader() {
        return Ok(build_admin_provider_oauth_backend_unavailable_response());
    }
    let Some(provider_id) = admin_provider_oauth_import_provider_id(request_context.path()) else {
        return Ok(build_internal_control_error_response(
            http::StatusCode::NOT_FOUND,
            "Provider 不存在",
        ));
    };
    let Some(request_body) = request_body else {
        return Ok(build_internal_control_error_response(
            http::StatusCode::BAD_REQUEST,
            "请求体必须是合法的 JSON 对象",
        ));
    };
    let raw_payload = match serde_json::from_slice::<serde_json::Value>(request_body) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => {
            return Ok(build_internal_control_error_response(
                http::StatusCode::BAD_REQUEST,
                "请求体必须是合法的 JSON 对象",
            ));
        }
    };
    let refresh_token_input = import_payload_string(&raw_payload, "refresh_token", "refreshToken");
    let access_token_input = import_payload_string_any(
        &raw_payload,
        &["access_token", "accessToken", "sso_token", "ssoToken"],
    );
    let imported_expires_at = import_payload_expiry_unix_secs(&raw_payload);
    let import_disabled = import_payload_bool_any(&raw_payload, &["disabled"]).unwrap_or(false);
    let name = raw_payload
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let proxy_node_id = raw_payload
        .get("proxy_node_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let replace_existing =
        import_payload_bool_any(&raw_payload, &["replace_existing", "replaceExisting"])
            .unwrap_or(false);

    let Some(provider) = state
        .read_provider_catalog_providers_by_ids(std::slice::from_ref(&provider_id))
        .await?
        .into_iter()
        .next()
    else {
        return Ok(build_internal_control_error_response(
            http::StatusCode::NOT_FOUND,
            "Provider 不存在",
        ));
    };
    let provider_type = provider.provider_type.trim().to_ascii_lowercase();
    let (refresh_token_input, access_token_input) = normalize_provider_import_tokens(
        &provider_type,
        refresh_token_input.as_deref(),
        access_token_input.as_deref(),
    );
    if refresh_token_input.is_none() && access_token_input.is_none() {
        return Ok(build_internal_control_error_response(
            http::StatusCode::BAD_REQUEST,
            "Refresh Token、Access Token 或 sso_token 不能为空",
        ));
    }
    if !is_fixed_provider_type_for_provider_oauth(&provider_type) {
        return Ok(build_internal_control_error_response(
            http::StatusCode::BAD_REQUEST,
            "该 Provider 不是固定类型，无法使用 provider-oauth",
        ));
    }
    if provider_type == "kiro" {
        return Ok(build_internal_control_error_response(
            http::StatusCode::BAD_REQUEST,
            "Kiro 不支持单条 Refresh Token 导入，请使用批量导入或设备授权。",
        ));
    }
    let template = admin_provider_oauth_template(&provider_type);
    if template.is_none() && !provider_type_supports_access_token_import(&provider_type) {
        return Ok(build_admin_provider_oauth_backend_unavailable_response());
    }
    let endpoint_resolution =
        resolve_provider_oauth_runtime_endpoints(state, &provider, &provider_type).await?;
    let endpoints = endpoint_resolution.endpoints;
    let runtime_endpoint = endpoint_resolution.runtime_endpoint;
    let request_proxy = state
        .resolve_admin_provider_oauth_operation_proxy_snapshot(
            proxy_node_id.as_deref(),
            &[
                runtime_endpoint
                    .as_ref()
                    .and_then(|endpoint| endpoint.proxy.as_ref()),
                provider.proxy.as_ref(),
            ],
        )
        .await;
    let key_proxy = provider_oauth_key_proxy_value(proxy_node_id.as_deref());

    let resolved_import = match resolve_admin_provider_oauth_single_import_tokens(
        state,
        template,
        &provider_id,
        &provider_type,
        provider.config.clone(),
        refresh_token_input.as_deref(),
        access_token_input.as_deref(),
        imported_expires_at,
        request_proxy.clone(),
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return Ok(response),
    };
    let AdminProviderOAuthSingleImportTokens {
        access_token,
        mut auth_config,
        expires_at,
    } = resolved_import;
    apply_single_import_hints(&provider_type, &raw_payload, &mut auth_config);
    let has_refresh_token = auth_config
        .get("refresh_token")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());

    let api_formats = provider_oauth_active_api_formats(&endpoints);
    let duplicate = match find_duplicate_provider_oauth_key_for_import(
        state,
        &provider_id,
        &auth_config,
        None,
        replace_existing,
    )
    .await
    {
        Ok(duplicate) => duplicate,
        Err(detail) => {
            return Ok(build_internal_control_error_response(
                http::StatusCode::BAD_REQUEST,
                detail,
            ));
        }
    };

    let replaced = duplicate.is_some();
    let persisted_key = if let Some(existing_key) = duplicate {
        match state
            .update_existing_provider_oauth_catalog_key(
                &existing_key,
                &provider_type,
                &access_token,
                &auth_config,
                &api_formats,
                key_proxy.clone(),
                expires_at,
                !import_disabled,
            )
            .await?
        {
            Some(key) => key,
            None => {
                return Ok(build_internal_control_error_response(
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    "provider oauth write unavailable",
                ));
            }
        }
    } else {
        let name = name.unwrap_or_else(|| {
            admin_provider_oauth_key_name_from_auth_config(&provider_type, &auth_config, None)
        });
        match state
            .create_provider_oauth_catalog_key(
                &provider_id,
                &provider_type,
                &name,
                &access_token,
                &auth_config,
                &api_formats,
                key_proxy.clone(),
                expires_at,
                !import_disabled,
            )
            .await?
        {
            Some(key) => key,
            None => {
                return Ok(build_internal_control_error_response(
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    "provider oauth write unavailable",
                ));
            }
        }
    };

    if !import_disabled {
        spawn_provider_oauth_account_state_refresh_after_update(
            state.cloned_app(),
            provider.clone(),
            persisted_key.id.clone(),
            request_proxy.clone(),
        );
    }

    Ok(Json(json!({
        "key_id": persisted_key.id,
        "provider_type": provider_type,
        "expires_at": expires_at,
        "has_refresh_token": has_refresh_token,
        "disabled": import_disabled,
        "temporary": auth_config
            .get("access_token_import_temporary")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "email": auth_config.get("email").cloned().unwrap_or(serde_json::Value::Null),
        "replaced": replaced,
    }))
    .into_response())
}

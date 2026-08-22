pub(super) use super::{
    base_url_from_request, build_portal_mismatch_response, build_unhandled_public_support_response,
    decrypt_catalog_secret_with_fallbacks, escape_admin_email_template_html,
    ldap_module_config_is_valid, module_available_from_env, read_admin_email_template_payload,
    render_admin_email_template_html, resolve_request_portal, resolve_user_portal,
    system_config_bool, system_config_string, validate_official_usd_registration_group, AppState,
    GatewayError, GatewayPublicRequestContext, PortalContext,
};
pub(super) use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
pub(super) use regex::Regex;
use serde::Deserialize;
pub(super) use serde_json::json;

#[path = "auth_helpers.rs"]
mod auth_helpers;
pub(crate) use auth_helpers::*;

#[path = "auth_turnstile.rs"]
mod auth_turnstile;
use auth_turnstile::*;

#[path = "auth_email.rs"]
mod auth_email;
use auth_email::*;

#[path = "auth_ldap.rs"]
mod auth_ldap;
use auth_ldap::*;

#[path = "auth_session.rs"]
pub(super) mod auth_session;
use auth_session::*;

#[path = "auth_registration.rs"]
pub(super) mod auth_registration;
use auth_registration::*;

#[path = "auth_password_reset.rs"]
mod auth_password_reset;
use auth_password_reset::*;

#[derive(Debug, Deserialize)]
struct AuthLoginRequest {
    email: String,
    password: String,
    #[serde(default = "default_auth_login_type")]
    auth_type: String,
}

fn default_auth_login_type() -> String {
    "local".to_string()
}

fn system_config_f64(value: Option<&serde_json::Value>, default: f64) -> f64 {
    match value {
        Some(serde_json::Value::Number(value)) => value.as_f64().unwrap_or(default),
        Some(serde_json::Value::String(value)) => value.trim().parse::<f64>().unwrap_or(default),
        _ => default,
    }
}

fn system_config_u16(value: Option<&serde_json::Value>, default: u16) -> u16 {
    match value {
        Some(serde_json::Value::Number(value)) => value
            .as_u64()
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(default),
        Some(serde_json::Value::String(value)) => value.trim().parse::<u16>().unwrap_or(default),
        _ => default,
    }
}

fn system_config_string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .collect(),
        Some(serde_json::Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .collect(),
        _ => Vec::new(),
    }
}
async fn handle_auth_login(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
    request_body: Option<&axum::body::Bytes>,
) -> Response<Body> {
    let Some(request_body) = request_body else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "缺少登录请求体", false);
    };
    let payload = match serde_json::from_slice::<AuthLoginRequest>(request_body) {
        Ok(value) => value,
        Err(_) => {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "无效的登录请求",
                false,
            )
        }
    };
    let identifier = normalize_auth_login_identifier(&payload.email);
    if identifier.is_empty() {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "邮箱或用户名不能为空",
            false,
        );
    }
    if let Err(detail) = validate_auth_login_password(&payload.password) {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
    }
    let client_device_id = match extract_client_device_id(request_context, headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let auth_type = payload.auth_type.trim().to_ascii_lowercase();
    let request_portal = match resolve_request_portal(state, request_context, headers).await {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("portal settings lookup failed: {err:?}"),
                false,
            )
        }
    };
    if request_portal.is_official_usd() && auth_type != "local" {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "专属门户仅支持邮箱密码登录",
            false,
        );
    }
    let user = match auth_type.as_str() {
        "local" => {
            let user = match state.find_user_auth_by_identifier(&identifier).await {
                Ok(Some(user)) => user,
                Ok(None) => {
                    return build_auth_error_response(
                        http::StatusCode::UNAUTHORIZED,
                        "邮箱或密码错误",
                        false,
                    )
                }
                Err(err) => {
                    return build_auth_error_response(
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("auth user lookup failed: {err:?}"),
                        false,
                    )
                }
            };
            if user.is_deleted
                || !user.is_active
                || !user.auth_source.eq_ignore_ascii_case("local")
                || user.password_hash.as_deref().is_none_or(str::is_empty)
            {
                return build_auth_error_response(
                    http::StatusCode::UNAUTHORIZED,
                    "邮箱或密码错误",
                    false,
                );
            }
            match auth_local_login_allowed_for_user(state, &user).await {
                Ok(true) => {}
                Ok(false) => {
                    return build_auth_error_response(
                        http::StatusCode::UNAUTHORIZED,
                        "邮箱或密码错误",
                        false,
                    )
                }
                Err(err) => {
                    return build_auth_error_response(
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("auth settings lookup failed: {err:?}"),
                        false,
                    )
                }
            }
            let password_hash = user
                .password_hash
                .as_deref()
                .expect("validated password hash should exist");
            let password_matches =
                bcrypt::verify(&payload.password, password_hash).unwrap_or(false);
            if !password_matches {
                return build_auth_error_response(
                    http::StatusCode::UNAUTHORIZED,
                    "邮箱或密码错误",
                    false,
                );
            }
            user
        }
        "ldap" => {
            let ldap_user =
                match authenticate_auth_ldap_user(state, &identifier, &payload.password).await {
                    Ok(Some(user)) => user,
                    Ok(None) => {
                        return build_auth_error_response(
                            http::StatusCode::UNAUTHORIZED,
                            "邮箱或密码错误",
                            false,
                        )
                    }
                    Err(err) => {
                        return build_auth_error_response(
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            format!("auth ldap login failed: {err:?}"),
                            false,
                        )
                    }
                };
            let _ = &ldap_user.display_name;
            let initial_gift = match state
                .read_system_config_json_value("default_user_initial_gift_usd")
                .await
            {
                Ok(value) => system_config_f64(value.as_ref(), 10.0),
                Err(err) => {
                    return build_auth_error_response(
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("auth settings lookup failed: {err:?}"),
                        false,
                    )
                }
            };
            match state
                .get_or_create_ldap_auth_user(
                    ldap_user.email,
                    ldap_user.username,
                    Some(ldap_user.ldap_dn),
                    Some(ldap_user.ldap_username),
                    auth_now(),
                    initial_gift,
                    false,
                )
                .await
            {
                Ok(Some(user)) => user,
                Ok(None) => {
                    return build_auth_error_response(
                        http::StatusCode::UNAUTHORIZED,
                        "邮箱或密码错误",
                        false,
                    )
                }
                Err(err) => {
                    return build_auth_error_response(
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("auth ldap user sync failed: {err:?}"),
                        false,
                    )
                }
            }
        }
        _ => {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "不支持的认证类型",
                false,
            )
        }
    };

    let user_portal = match resolve_user_portal(state, &user.id).await {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("user portal lookup failed: {err:?}"),
                false,
            )
        }
    };
    if request_portal.id != user_portal.id {
        return build_portal_mismatch_response(&user_portal);
    }

    build_auth_login_success_response(state, headers, client_device_id, user).await
}

pub(super) async fn maybe_build_local_auth_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
    cf_connecting_ip: Option<&str>,
    request_body: Option<&axum::body::Bytes>,
) -> Option<Response<Body>> {
    let decision = request_context.control_decision.as_ref()?;
    if decision.route_family.as_deref() != Some("auth") {
        return None;
    }

    match decision.route_kind.as_deref() {
        Some("send_verification_code")
            if request_context.request_path == "/api/auth/send-verification-code" =>
        {
            Some(
                handle_auth_send_verification_code(state, headers, cf_connecting_ip, request_body)
                    .await,
            )
        }
        Some("login") if request_context.request_path == "/api/auth/login" => {
            Some(handle_auth_login(state, request_context, headers, request_body).await)
        }
        Some("register") if request_context.request_path == "/api/auth/register" => Some(
            handle_auth_register(
                state,
                request_context,
                headers,
                cf_connecting_ip,
                request_body,
            )
            .await,
        ),
        Some("verify_email") if request_context.request_path == "/api/auth/verify-email" => {
            Some(handle_auth_verify_email(state, request_body).await)
        }
        Some("verification_status")
            if request_context.request_path == "/api/auth/verification-status" =>
        {
            Some(handle_auth_verification_status(state, request_body).await)
        }
        Some("request_password_reset")
            if request_context.request_path == "/api/auth/request-password-reset" =>
        {
            Some(
                handle_auth_request_password_reset(state, request_context, headers, request_body)
                    .await,
            )
        }
        Some("reset_password") if request_context.request_path == "/api/auth/reset-password" => {
            Some(handle_auth_reset_password(state, request_body).await)
        }
        Some("me") if request_context.request_path == "/api/auth/me" => {
            Some(handle_auth_me(state, request_context, headers).await)
        }
        Some("refresh") if request_context.request_path == "/api/auth/refresh" => {
            Some(handle_auth_refresh(state, request_context, headers).await)
        }
        Some("logout") if request_context.request_path == "/api/auth/logout" => {
            Some(handle_auth_logout(state, request_context, headers).await)
        }
        _ => Some(build_unhandled_public_support_response(request_context)),
    }
}

#[cfg(test)]
mod tests {
    use super::{maybe_build_local_auth_response, AppState, GatewayPublicRequestContext};
    use crate::control::GatewayControlDecision;
    use axum::body::to_bytes;
    use axum::http::{HeaderMap, Method, StatusCode, Uri};

    fn request_context(method: Method, uri: &str, route_kind: &str) -> GatewayPublicRequestContext {
        GatewayPublicRequestContext::from_request_parts(
            "trace-auth-unhandled",
            &method,
            &uri.parse::<Uri>().expect("uri should parse"),
            &HeaderMap::new(),
            Some(GatewayControlDecision::synthetic(
                uri,
                Some("public_support".to_string()),
                Some("auth".to_string()),
                Some(route_kind.to_string()),
                Some("user:auth".to_string()),
            )),
        )
    }

    #[tokio::test]
    async fn auth_unhandled_route_returns_local_not_implemented_response() {
        let state = AppState::new().expect("gateway should build");
        let request_context = request_context(Method::POST, "/api/auth/login/history", "login");
        let response = maybe_build_local_auth_response(
            &state,
            &request_context,
            &HeaderMap::new(),
            None,
            None,
        )
        .await
        .expect("auth handler should return response");

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("json body should parse");
        assert_eq!(
            payload["detail"],
            "public support route not implemented in rust frontdoor"
        );
        assert_eq!(payload["route_family"], "auth");
        assert_eq!(payload["route_kind"], "login");
        assert_eq!(payload["request_path"], "/api/auth/login/history");
    }
}

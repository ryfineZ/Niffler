use aether_data::repository::users::StoredUserGroup;
use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{AppState, GatewayError};

use super::GatewayPublicRequestContext;

pub(crate) const DEFAULT_PORTAL_ID: &str = "default";
pub(crate) const OFFICIAL_USD_PORTAL_ID: &str = "official_usd";

const OFFICIAL_USD_PORTAL_HOSTS_KEY: &str = "official_usd_portal_hosts";
const OFFICIAL_USD_PORTAL_GROUP_ID_KEY: &str = "official_usd_portal_group_id";
const OFFICIAL_USD_PORTAL_CANONICAL_URL_KEY: &str = "official_usd_portal_canonical_url";
const DEFAULT_PORTAL_CANONICAL_URL_KEY: &str = "default_portal_canonical_url";
const OFFICIAL_USD_PORTAL_SITE_NAME_KEY: &str = "official_usd_portal_site_name";
const OFFICIAL_USD_PORTAL_SITE_SUBTITLE_KEY: &str = "official_usd_portal_site_subtitle";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PortalContext {
    pub(crate) id: &'static str,
    pub(crate) display_currency: &'static str,
    pub(crate) canonical_url: Option<String>,
    pub(crate) group_id: Option<String>,
    pub(crate) site_name: Option<String>,
    pub(crate) site_subtitle: Option<String>,
    pub(crate) discount: Option<f64>,
    pub(crate) model_discounts: Option<Value>,
}

impl PortalContext {
    pub(crate) fn is_official_usd(&self) -> bool {
        self.id == OFFICIAL_USD_PORTAL_ID
    }

    pub(crate) fn public_payload(&self) -> Value {
        json!({
            "id": self.id,
            "display_currency": self.display_currency,
            "pricing_mode": if self.is_official_usd() { "official_api" } else { "default" },
            "discount": self.discount,
            "model_discounts": self.model_discounts,
            "canonical_url": self.canonical_url,
        })
    }
}

pub(crate) fn build_portal_mismatch_response(portal: &PortalContext) -> Response<Body> {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "detail": "请从账号所属门户登录",
            "portal_mismatch": true,
            "portal": portal.public_payload(),
            "canonical_url": portal.canonical_url,
        })),
    )
        .into_response()
}

#[derive(Debug, Clone)]
struct PortalSettings {
    official_hosts: Vec<String>,
    official_group_id: Option<String>,
    official_canonical_url: Option<String>,
    default_canonical_url: Option<String>,
    official_site_name: Option<String>,
    official_site_subtitle: Option<String>,
}

fn value_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn value_string_list(value: Option<&Value>) -> Vec<String> {
    let values = match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>(),
        Some(Value::String(value)) => value.split(',').map(ToOwned::to_owned).collect(),
        _ => Vec::new(),
    };
    values
        .into_iter()
        .filter_map(|value| normalize_portal_host(&value))
        .collect()
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn read_portal_settings(state: &AppState) -> Result<PortalSettings, GatewayError> {
    let hosts = state
        .read_system_config_json_value(OFFICIAL_USD_PORTAL_HOSTS_KEY)
        .await?;
    let group_id = state
        .read_system_config_json_value(OFFICIAL_USD_PORTAL_GROUP_ID_KEY)
        .await?;
    let official_canonical_url = state
        .read_system_config_json_value(OFFICIAL_USD_PORTAL_CANONICAL_URL_KEY)
        .await?;
    let default_canonical_url = state
        .read_system_config_json_value(DEFAULT_PORTAL_CANONICAL_URL_KEY)
        .await?;
    let official_site_name = state
        .read_system_config_json_value(OFFICIAL_USD_PORTAL_SITE_NAME_KEY)
        .await?;
    let official_site_subtitle = state
        .read_system_config_json_value(OFFICIAL_USD_PORTAL_SITE_SUBTITLE_KEY)
        .await?;

    let official_hosts = {
        let configured = value_string_list(hosts.as_ref());
        if configured.is_empty() {
            optional_env("NIFFLER_OFFICIAL_USD_PORTAL_HOSTS")
                .map(|value| value_string_list(Some(&Value::String(value))))
                .unwrap_or_default()
        } else {
            configured
        }
    };

    Ok(PortalSettings {
        official_hosts,
        official_group_id: value_string(group_id.as_ref())
            .or_else(|| optional_env("NIFFLER_OFFICIAL_USD_PORTAL_GROUP_ID")),
        official_canonical_url: value_string(official_canonical_url.as_ref())
            .or_else(|| optional_env("NIFFLER_OFFICIAL_USD_PORTAL_CANONICAL_URL")),
        default_canonical_url: value_string(default_canonical_url.as_ref())
            .or_else(|| optional_env("NIFFLER_DEFAULT_PORTAL_CANONICAL_URL")),
        official_site_name: value_string(official_site_name.as_ref()),
        official_site_subtitle: value_string(official_site_subtitle.as_ref()),
    })
}

pub(crate) fn normalize_portal_host(value: &str) -> Option<String> {
    let value = value.split(',').next()?.trim();
    if value.is_empty() || value.contains('/') || value.contains('@') {
        return None;
    }
    let authority = value.parse::<http::uri::Authority>().ok()?;
    let host = authority
        .host()
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn request_portal_host(request_context: &GatewayPublicRequestContext) -> Option<String> {
    // Portal selection is security-sensitive because it controls registration and pricing.
    // Only use the Host value captured by the gateway listener; an arbitrary client must not
    // be able to switch portals by supplying X-Forwarded-Host.
    request_context
        .host_header
        .as_deref()
        .and_then(normalize_portal_host)
}

fn default_portal(settings: &PortalSettings) -> PortalContext {
    PortalContext {
        id: DEFAULT_PORTAL_ID,
        display_currency: "USD",
        canonical_url: settings.default_canonical_url.clone(),
        group_id: None,
        site_name: None,
        site_subtitle: None,
        discount: None,
        model_discounts: None,
    }
}

async fn official_usd_portal(
    state: &AppState,
    settings: &PortalSettings,
) -> Result<PortalContext, GatewayError> {
    let group =
        load_validated_official_usd_group(state, settings.official_group_id.as_deref()).await?;
    Ok(PortalContext {
        id: OFFICIAL_USD_PORTAL_ID,
        display_currency: "USD",
        canonical_url: settings.official_canonical_url.clone(),
        group_id: Some(group.id),
        site_name: settings.official_site_name.clone(),
        site_subtitle: settings.official_site_subtitle.clone(),
        discount: Some(group.sales_multiplier),
        model_discounts: group.model_sales_multipliers,
    })
}

pub(crate) async fn resolve_request_portal(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    _headers: &http::HeaderMap,
) -> Result<PortalContext, GatewayError> {
    let settings = read_portal_settings(state).await?;
    let host = request_portal_host(request_context);
    if host.is_some_and(|host| settings.official_hosts.iter().any(|value| value == &host)) {
        official_usd_portal(state, &settings).await
    } else {
        Ok(default_portal(&settings))
    }
}

pub(crate) async fn resolve_user_portal(
    state: &AppState,
    user_id: &str,
) -> Result<PortalContext, GatewayError> {
    let settings = read_portal_settings(state).await?;
    let Some(group_id) = settings.official_group_id.as_deref() else {
        return Ok(default_portal(&settings));
    };
    let is_member = state
        .list_user_groups_for_user(user_id)
        .await?
        .iter()
        .any(|group| group.id == group_id);
    if is_member {
        official_usd_portal(state, &settings).await
    } else {
        Ok(default_portal(&settings))
    }
}

fn is_valid_discount(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn validate_model_discounts(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Object(values)) => values.iter().all(|(model_id, value)| {
            !model_id.trim().is_empty() && value.as_f64().is_some_and(is_valid_discount)
        }),
        Some(_) => false,
    }
}

async fn load_validated_official_usd_group(
    state: &AppState,
    group_id: Option<&str>,
) -> Result<StoredUserGroup, GatewayError> {
    let Some(group_id) = group_id else {
        return Err(GatewayError::Internal(
            "official USD portal group is not configured".to_string(),
        ));
    };
    let Some(group) = state.find_user_group_by_id(group_id).await? else {
        return Err(GatewayError::Internal(format!(
            "official USD portal group does not exist: {group_id}"
        )));
    };
    if group.visibility.trim().eq_ignore_ascii_case("public") {
        return Err(GatewayError::Internal(
            "official USD portal group must be internal".to_string(),
        ));
    }
    if !is_valid_discount(group.sales_multiplier) {
        return Err(GatewayError::Internal(
            "official USD portal discount must be a non-negative finite number".to_string(),
        ));
    }
    if !validate_model_discounts(group.model_sales_multipliers.as_ref()) {
        return Err(GatewayError::Internal(
            "official USD portal model discounts must be a non-negative numeric object".to_string(),
        ));
    }
    Ok(group)
}

pub(crate) async fn validate_official_usd_registration_group(
    state: &AppState,
    portal: &PortalContext,
) -> Result<String, GatewayError> {
    let group = load_validated_official_usd_group(state, portal.group_id.as_deref()).await?;
    Ok(group.id)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, Method, Uri};
    use serde_json::json;

    use super::{
        is_valid_discount, normalize_portal_host, request_portal_host, validate_model_discounts,
        GatewayPublicRequestContext,
    };

    #[test]
    fn portal_host_normalization_removes_port_and_case() {
        assert_eq!(
            normalize_portal_host("OFFICIAL.Example.COM:443"),
            Some("official.example.com".to_string())
        );
        assert_eq!(
            normalize_portal_host("official.example.com."),
            Some("official.example.com".to_string())
        );
    }

    #[test]
    fn portal_host_normalization_rejects_urls_and_userinfo() {
        assert_eq!(normalize_portal_host("https://official.example.com"), None);
        assert_eq!(normalize_portal_host("user@official.example.com"), None);
    }

    #[test]
    fn portal_selection_ignores_client_forwarded_host() {
        let mut headers = HeaderMap::new();
        headers.insert(http::header::HOST, "app.example.com".parse().unwrap());
        headers.insert(
            crate::constants::FORWARDED_HOST_HEADER,
            "partner.example.com".parse().unwrap(),
        );
        let context = GatewayPublicRequestContext::from_request_parts(
            "trace-1",
            &Method::GET,
            &Uri::from_static("/api/public/site-info"),
            &headers,
            None,
        );

        assert_eq!(
            request_portal_host(&context).as_deref(),
            Some("app.example.com")
        );
    }

    #[test]
    fn official_portal_accepts_adjustable_non_negative_discounts() {
        for discount in [0.0, 0.9, 1.0, 1.25] {
            assert!(is_valid_discount(discount));
        }
        assert!(!is_valid_discount(-0.01));
        assert!(!is_valid_discount(f64::NAN));
        assert!(!is_valid_discount(f64::INFINITY));

        assert!(validate_model_discounts(Some(&json!({
            "gpt-5": 1.2,
            "claude-sonnet": 0.95
        }))));
        assert!(!validate_model_discounts(Some(&json!({"gpt-5": -1}))));
        assert!(!validate_model_discounts(Some(&json!([1.0]))));
    }
}

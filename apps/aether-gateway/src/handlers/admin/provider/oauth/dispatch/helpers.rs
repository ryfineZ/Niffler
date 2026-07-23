use crate::handlers::admin::shared::attach_admin_audit_response;
use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn admin_provider_oauth_single_import_audit_taxonomy(
    request_body: Option<&axum::body::Bytes>,
) -> (&'static str, &'static str) {
    let creates_agent_identity = request_body
        .and_then(|body| serde_json::from_slice::<Value>(body).ok())
        .and_then(|value| value.as_object().cloned())
        .is_some_and(|payload| {
            payload
                .get("create_agent_identity")
                .and_then(Value::as_bool)
                == Some(true)
        });
    if creates_agent_identity {
        (
            "admin_provider_oauth_agent_identity_created",
            "create_provider_agent_identity",
        )
    } else {
        (
            "admin_provider_oauth_refresh_token_imported",
            "import_provider_oauth_refresh_token",
        )
    }
}

pub(super) fn attach_admin_provider_oauth_audit_response(
    response: Response<Body>,
    event_name: &'static str,
    action: &'static str,
    target_type: &'static str,
    target_id: Option<String>,
) -> Response<Body> {
    if !response.status().is_success() {
        return response;
    }
    let Some(target_id) = target_id else {
        return response;
    };
    attach_admin_audit_response(response, event_name, action, target_type, &target_id)
}

pub(super) fn admin_provider_oauth_key_name_from_auth_config(
    provider_type: &str,
    auth_config: &Map<String, Value>,
    batch_index: Option<usize>,
) -> String {
    let provider_type = provider_type.trim();
    if let Some(email) = trimmed_auth_config_string(auth_config, "email") {
        return format!("{provider_type}_{email}");
    }
    if provider_type.eq_ignore_ascii_case("grok") {
        if let Some(user_id) = trimmed_auth_config_string(auth_config, "user_id") {
            return format!("grok_{user_id}");
        }
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    match batch_index {
        Some(index) => format!("{provider_type}_{timestamp}_{index}"),
        None => format!("账号_{timestamp}"),
    }
}

fn trimmed_auth_config_string(auth_config: &Map<String, Value>, key: &str) -> Option<String> {
    auth_config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Map};

    #[test]
    fn grok_default_key_name_uses_full_user_id() {
        let mut auth_config = Map::new();
        auth_config.insert(
            "user_id".to_string(),
            json!("1619039a-0191-4e0a-a490-8f4ad21262c9"),
        );

        assert_eq!(
            admin_provider_oauth_key_name_from_auth_config("grok", &auth_config, None),
            "grok_1619039a-0191-4e0a-a490-8f4ad21262c9"
        );
    }

    #[test]
    fn default_key_name_prefers_email_over_grok_user_id() {
        let mut auth_config = Map::new();
        auth_config.insert("email".to_string(), json!("grok@example.com"));
        auth_config.insert("user_id".to_string(), json!("user-1"));

        assert_eq!(
            admin_provider_oauth_key_name_from_auth_config("grok", &auth_config, None),
            "grok_grok@example.com"
        );
    }

    #[test]
    fn batch_default_key_name_keeps_existing_timestamp_shape() {
        let auth_config = Map::new();
        let name = admin_provider_oauth_key_name_from_auth_config("codex", &auth_config, Some(3));

        assert!(name.starts_with("codex_"));
        assert!(name.ends_with("_3"));
    }

    #[test]
    fn single_import_audit_distinguishes_agent_identity_creation_without_exposing_input() {
        let body = axum::body::Bytes::from(
            json!({
                "create_agent_identity": true,
                "access_token": "secret-access-token"
            })
            .to_string(),
        );
        assert_eq!(
            admin_provider_oauth_single_import_audit_taxonomy(Some(&body)),
            (
                "admin_provider_oauth_agent_identity_created",
                "create_provider_agent_identity",
            )
        );
    }

    #[test]
    fn single_import_audit_rejects_removed_session_token_creation_alias() {
        let body = axum::body::Bytes::from(
            json!({
                "create_agent_identity_from_session_token": true,
                "access_token": "secret-access-token"
            })
            .to_string(),
        );
        assert_eq!(
            admin_provider_oauth_single_import_audit_taxonomy(Some(&body)),
            (
                "admin_provider_oauth_refresh_token_imported",
                "import_provider_oauth_refresh_token",
            )
        );
    }

    #[test]
    fn single_import_audit_keeps_standard_import_taxonomy() {
        let body =
            axum::body::Bytes::from(json!({ "refresh_token": "secret-refresh-token" }).to_string());
        assert_eq!(
            admin_provider_oauth_single_import_audit_taxonomy(Some(&body)),
            (
                "admin_provider_oauth_refresh_token_imported",
                "import_provider_oauth_refresh_token",
            )
        );
    }
}

use super::super::super::errors::normalize_provider_oauth_refresh_error_message;
use super::super::super::runtime::refresh_provider_oauth_account_state_after_update;
use super::helpers::{self, RefreshDispatch, RefreshRequestContext, RefreshSuccessContext};
use super::response;
use crate::handlers::admin::request::{AdminAppState, AdminLocalOAuthRefreshError};
use crate::provider_transport::provider_types::provider_type_retains_oauth_forbidden;
use crate::GatewayError;
use axum::http;

pub(super) async fn execute_admin_provider_oauth_refresh(
    state: &AdminAppState<'_>,
    request: RefreshRequestContext,
) -> Result<RefreshDispatch<RefreshSuccessContext>, GatewayError> {
    let RefreshRequestContext {
        key_id,
        key,
        provider,
        provider_type,
        trace_id,
        transport,
    } = request;

    let refreshed_entry = match state.force_local_oauth_refresh_entry(&transport).await {
        Ok(Some(entry)) => Some(entry),
        Ok(None) => {
            tracing::warn!(
                trace_id = %trace_id,
                key_id = %key_id,
                provider_id = %provider.id,
                provider_type = %provider_type,
                "gateway manual provider oauth refresh did not run"
            );
            return Ok(RefreshDispatch::Respond(response::control_error_response(
                http::StatusCode::BAD_REQUEST,
                "Token 刷新未执行，请检查授权配置",
            )));
        }
        Err(AdminLocalOAuthRefreshError::HttpStatus {
            status_code,
            body_excerpt,
            ..
        }) => {
            let error_reason = normalize_provider_oauth_refresh_error_message(
                Some(status_code),
                Some(body_excerpt.as_str()),
            );
            tracing::warn!(
                trace_id = %trace_id,
                key_id = %key_id,
                provider_id = %provider.id,
                provider_type = %provider_type,
                status_code,
                reason = %error_reason,
                "gateway manual provider oauth refresh failed"
            );
            if manual_oauth_refresh_failure_must_be_retained(&provider_type, status_code) {
                tracing::warn!(
                    trace_id = %trace_id,
                    key_id = %key_id,
                    provider_id = %provider.id,
                    provider_type = %provider_type,
                    status_code,
                    event_name = "manual_grok_oauth_refresh_forbidden_retained",
                    "gateway retained grok oauth key after manual refresh forbidden response"
                );
            } else if matches!(status_code, 400 | 401 | 403) {
                let auto_removed = state
                    .app()
                    .persist_local_oauth_refresh_failure_state(
                        &transport,
                        status_code,
                        body_excerpt.as_str(),
                        false,
                    )
                    .await?;
                if auto_removed {
                    tracing::info!(
                        trace_id = %trace_id,
                        key_id = %key_id,
                        provider_id = %provider.id,
                        provider_type = %provider_type,
                        event_name = "auto_removed_oauth_refresh_failed",
                        "gateway manual provider oauth refresh auto-removed unusable key"
                    );
                    return Ok(RefreshDispatch::Respond(
                        response::oauth_refresh_auto_removed_response(&error_reason),
                    ));
                }
                tracing::info!(
                    trace_id = %trace_id,
                    key_id = %key_id,
                    provider_id = %provider.id,
                    provider_type = %provider_type,
                    event_name = "refresh_failed_retained",
                    "gateway manual provider oauth refresh failure retained key"
                );
            }
            return Ok(RefreshDispatch::Respond(
                response::oauth_refresh_failed_bad_request_response(&error_reason),
            ));
        }
        Err(AdminLocalOAuthRefreshError::Transport { source, .. }) => {
            tracing::warn!(
                trace_id = %trace_id,
                key_id = %key_id,
                provider_id = %provider.id,
                provider_type = %provider_type,
                error = %source,
                "gateway manual provider oauth refresh transport failed"
            );
            return Ok(RefreshDispatch::Respond(
                response::oauth_refresh_failed_service_unavailable_response(source.to_string()),
            ));
        }
        Err(AdminLocalOAuthRefreshError::TransportMessage { message, .. }) => {
            tracing::warn!(
                trace_id = %trace_id,
                key_id = %key_id,
                provider_id = %provider.id,
                provider_type = %provider_type,
                error = %message,
                "gateway manual provider oauth refresh transport failed"
            );
            return Ok(RefreshDispatch::Respond(
                response::oauth_refresh_failed_service_unavailable_response(message),
            ));
        }
        Err(AdminLocalOAuthRefreshError::InvalidResponse { message, .. }) => {
            tracing::warn!(
                trace_id = %trace_id,
                key_id = %key_id,
                provider_id = %provider.id,
                provider_type = %provider_type,
                reason = %message,
                "gateway manual provider oauth refresh returned invalid response"
            );
            return Ok(RefreshDispatch::Respond(
                response::oauth_refresh_failed_bad_request_response(&message),
            ));
        }
    };

    let refreshed_key = state
        .read_provider_catalog_keys_by_ids(std::slice::from_ref(&key_id))
        .await?
        .into_iter()
        .next()
        .unwrap_or(key);
    let refreshed_auth_config = refreshed_entry
        .as_ref()
        .and_then(|entry| entry.metadata.as_ref())
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_else(|| {
            helpers::refreshed_auth_config_object(
                state,
                refreshed_key.encrypted_auth_config.as_deref(),
            )
        });
    let refreshed_expires_at_unix_secs = refreshed_entry
        .as_ref()
        .and_then(|entry| entry.expires_at_unix_secs)
        .or_else(|| {
            refreshed_auth_config
                .get("expires_at")
                .and_then(serde_json::Value::as_u64)
        });
    let (account_state_recheck_attempted, account_state_recheck_error) = state
        .refresh_provider_oauth_account_state_after_update(&provider, &key_id, None)
        .await?;

    Ok(RefreshDispatch::Continue(RefreshSuccessContext {
        provider_type,
        refreshed_auth_config,
        refreshed_expires_at_unix_secs,
        account_state_recheck_attempted,
        account_state_recheck_error,
    }))
}

fn manual_oauth_refresh_failure_must_be_retained(provider_type: &str, status_code: u16) -> bool {
    status_code == 403 && provider_type_retains_oauth_forbidden(provider_type)
}

#[cfg(test)]
mod tests {
    use super::manual_oauth_refresh_failure_must_be_retained;

    #[test]
    fn manual_grok_oauth_403_is_retained_without_changing_401() {
        assert!(manual_oauth_refresh_failure_must_be_retained(
            "grok_oauth",
            403
        ));
        assert!(!manual_oauth_refresh_failure_must_be_retained(
            "grok_oauth",
            401
        ));
        assert!(!manual_oauth_refresh_failure_must_be_retained("codex", 403));
    }

    #[test]
    fn manual_refresh_uses_fenced_state_persistence_without_redundant_clear() {
        let source = include_str!("execution.rs");
        assert!(source.contains("persist_local_oauth_refresh_failure_state"));
        let redundant_clear = ["clear_provider_catalog_key_", "oauth_invalid_marker"].concat();
        let unfenced_persistence = ["persist_provider_quota_", "refresh_state"].concat();
        assert!(!source.contains(&redundant_clear));
        assert!(!source.contains(&unfenced_persistence));
    }
}

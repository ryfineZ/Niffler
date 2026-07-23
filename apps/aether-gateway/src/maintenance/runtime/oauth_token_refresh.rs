use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use serde_json::Value;
use tracing::{info, warn};

use crate::admin_api::provider_oauth_maintenance_endpoint_for_provider;
use crate::provider_key_auth::provider_key_is_oauth_managed;
use crate::{AppState, GatewayError};

use super::system_config_bool;

const OAUTH_TOKEN_REFRESH_LOOKAHEAD_SECS: u64 = 120;
const OAUTH_REFRESH_FAILED_PREFIX: &str = "[REFRESH_FAILED] ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub(crate) struct OAuthTokenRefreshRunSummary {
    pub(crate) scanned: usize,
    pub(crate) eligible: usize,
    pub(crate) refreshed: usize,
    pub(crate) resolved: usize,
    pub(crate) skipped: usize,
    pub(crate) failed: usize,
}

pub(crate) async fn perform_oauth_token_refresh_once(
    state: &AppState,
) -> Result<OAuthTokenRefreshRunSummary, GatewayError> {
    if !state.has_provider_catalog_data_reader() || !state.has_provider_catalog_data_writer() {
        return Ok(OAuthTokenRefreshRunSummary::default());
    }
    if !system_config_bool(&state.data, "enable_oauth_token_refresh", true)
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))?
    {
        return Ok(OAuthTokenRefreshRunSummary::default());
    }

    let providers = state.list_provider_catalog_providers(true).await?;
    let provider_ids = providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<Vec<_>>();
    if provider_ids.is_empty() {
        return Ok(OAuthTokenRefreshRunSummary::default());
    }

    let endpoints = state
        .list_provider_catalog_endpoints_by_provider_ids(&provider_ids)
        .await?;
    let keys = state
        .list_provider_catalog_keys_by_provider_ids(&provider_ids)
        .await?;
    let endpoints_by_provider = group_endpoints_by_provider(endpoints);
    let keys_by_provider = group_keys_by_provider(keys);
    let mut summary = OAuthTokenRefreshRunSummary::default();
    let refresh_cutoff_unix_secs =
        now_unix_secs().saturating_add(OAUTH_TOKEN_REFRESH_LOOKAHEAD_SECS);

    for provider in providers {
        let provider_keys = keys_by_provider
            .get(provider.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let provider_endpoints = endpoints_by_provider
            .get(provider.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for key in provider_keys {
            summary.scanned = summary.scanned.saturating_add(1);
            if !oauth_refresh_candidate(&provider, key, refresh_cutoff_unix_secs) {
                summary.skipped = summary.skipped.saturating_add(1);
                continue;
            }
            summary.eligible = summary.eligible.saturating_add(1);

            let Some(endpoint) = provider_oauth_maintenance_endpoint_for_provider(
                &provider.provider_type,
                provider_endpoints,
            ) else {
                summary.skipped = summary.skipped.saturating_add(1);
                continue;
            };

            let Some(transport) = state
                .read_provider_transport_snapshot(&provider.id, &endpoint.id, &key.id)
                .await?
            else {
                summary.skipped = summary.skipped.saturating_add(1);
                continue;
            };
            let is_agent_identity =
                crate::provider_transport::is_codex_agent_identity_transport(&transport);
            let needs_agent_task_recovery = is_agent_identity
                && agent_identity_needs_task_recovery(
                    transport.key.decrypted_auth_config.as_deref(),
                    key.oauth_invalid_reason.as_deref(),
                );
            if !needs_agent_task_recovery
                && !auth_config_has_refresh_token(transport.key.decrypted_auth_config.as_deref())
            {
                summary.skipped = summary.skipped.saturating_add(1);
                continue;
            }

            let refresh_result = if needs_agent_task_recovery {
                state
                    .force_local_oauth_refresh_entry(&transport)
                    .await
                    .map(|entry| entry.map(|_| ()))
                    .map_err(|err| GatewayError::Internal(err.to_string()))
            } else {
                state
                    .resolve_local_oauth_request_auth(&transport)
                    .await
                    .map(|auth| auth.map(|_| ()))
            };
            match refresh_result {
                Ok(Some(())) => {
                    summary.resolved = summary.resolved.saturating_add(1);
                    if provider_key_credentials_changed(state, key).await? {
                        summary.refreshed = summary.refreshed.saturating_add(1);
                    }
                }
                Ok(None) => {
                    summary.skipped = summary.skipped.saturating_add(1);
                }
                Err(err) => {
                    summary.failed = summary.failed.saturating_add(1);
                    warn!(
                        event_name = "oauth_token_refresh_failed",
                        log_type = "ops",
                        worker = "oauth_token_refresh",
                        provider_id = %provider.id,
                        key_id = %key.id,
                        error = ?err,
                        "gateway oauth token auto refresh failed"
                    );
                }
            }
        }
    }

    if summary.eligible > 0 || summary.refreshed > 0 || summary.failed > 0 {
        info!(
            event_name = "oauth_token_refresh_completed",
            log_type = "ops",
            worker = "oauth_token_refresh",
            scanned = summary.scanned,
            eligible = summary.eligible,
            refreshed = summary.refreshed,
            resolved = summary.resolved,
            skipped = summary.skipped,
            failed = summary.failed,
            "gateway completed oauth token auto refresh scan"
        );
    }

    Ok(summary)
}

fn group_endpoints_by_provider(
    endpoints: Vec<StoredProviderCatalogEndpoint>,
) -> BTreeMap<String, Vec<StoredProviderCatalogEndpoint>> {
    let mut grouped = BTreeMap::new();
    for endpoint in endpoints {
        grouped
            .entry(endpoint.provider_id.clone())
            .or_insert_with(Vec::new)
            .push(endpoint);
    }
    grouped
}

fn group_keys_by_provider(
    keys: Vec<StoredProviderCatalogKey>,
) -> BTreeMap<String, Vec<StoredProviderCatalogKey>> {
    let mut grouped = BTreeMap::new();
    for key in keys {
        grouped
            .entry(key.provider_id.clone())
            .or_insert_with(Vec::new)
            .push(key);
    }
    grouped
}

fn oauth_refresh_candidate(
    provider: &StoredProviderCatalogProvider,
    key: &StoredProviderCatalogKey,
    refresh_cutoff_unix_secs: u64,
) -> bool {
    let has_auth_config = key
        .encrypted_auth_config
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let regular_oauth_candidate = key.oauth_invalid_at_unix_secs.is_none()
        && key
            .expires_at_unix_secs
            .is_some_and(|expires_at| expires_at <= refresh_cutoff_unix_secs);
    // The catalog row is encrypted here, so exact Agent Identity validation is
    // deferred until the transport snapshot has decrypted auth_config.
    let possible_agent_candidate = provider.provider_type.trim().eq_ignore_ascii_case("codex")
        && key.auth_type.trim().eq_ignore_ascii_case("oauth")
        && (key.expires_at_unix_secs.is_none()
            || key
                .oauth_invalid_reason
                .as_deref()
                .is_some_and(|reason| reason.contains(OAUTH_REFRESH_FAILED_PREFIX)));
    key.is_active
        && has_auth_config
        && (regular_oauth_candidate || possible_agent_candidate)
        && provider_key_is_oauth_managed(key, provider.provider_type.as_str())
}

fn agent_identity_needs_task_recovery(
    auth_config: Option<&str>,
    oauth_invalid_reason: Option<&str>,
) -> bool {
    if oauth_invalid_reason.is_some_and(|reason| reason.contains(OAUTH_REFRESH_FAILED_PREFIX)) {
        return true;
    }
    auth_config
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .is_some_and(|config| {
            crate::provider_transport::is_codex_agent_identity_auth_config_value(&config)
                && !crate::provider_transport::codex_agent_identity_auth_config_has_task_id(&config)
        })
}

async fn provider_key_credentials_changed(
    state: &AppState,
    before: &StoredProviderCatalogKey,
) -> Result<bool, GatewayError> {
    let Some(after) = state
        .list_provider_catalog_keys_by_ids(std::slice::from_ref(&before.id))
        .await?
        .into_iter()
        .next()
    else {
        return Ok(false);
    };
    Ok(after.encrypted_api_key != before.encrypted_api_key
        || after.encrypted_auth_config != before.encrypted_auth_config
        || after.expires_at_unix_secs != before.expires_at_unix_secs)
}

fn auth_config_has_refresh_token(auth_config: Option<&str>) -> bool {
    let Some(auth_config) = auth_config.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(auth_config) else {
        return false;
    };
    value
        .as_object()
        .and_then(|object| object.get("refresh_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::agent_identity_needs_task_recovery;

    #[test]
    fn pending_agent_identity_without_task_is_recoverable() {
        let config = serde_json::json!({
            "auth_mode": "agentIdentity",
            "agent_runtime_id": "runtime-1",
            "agent_private_key": "private-key-present",
        });
        assert!(agent_identity_needs_task_recovery(
            Some(&config.to_string()),
            None,
        ));
    }

    #[test]
    fn refresh_failure_marker_forces_agent_task_recovery() {
        assert!(agent_identity_needs_task_recovery(
            Some("{}"),
            Some("[REFRESH_FAILED] temporary"),
        ));
    }
}

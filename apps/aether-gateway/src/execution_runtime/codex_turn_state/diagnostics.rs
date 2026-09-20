//! 仅保存诊断元数据；真实可用性在管理员读取时重新校验。
use super::*;
use crate::provider_transport::GatewayProviderTransportSnapshot;

const KEEP: Duration = Duration::from_secs(86400);
const MAX_SCOPES: usize = 24;

#[derive(Serialize, Deserialize)]
struct Scope {
    scope: String,
    endpoint_id: String,
    configuration: String,
    guard: String,
    credential: String,
    blocks: usize,
    model: String,
    egress: String,
    node_id: Option<String>,
    instance: Option<String>,
    at: u64,
}

fn index_key(provider_id: &str, key_id: &str) -> String {
    format!(
        "codex-state:diagnostics:{}",
        digest(json!([provider_id, key_id]).to_string())
    )
}

async fn best_effort(task: impl Future<Output = Result<(), StateError>>) {
    if !matches!(
        tokio::time::timeout(Duration::from_millis(250), task).await,
        Ok(Ok(()))
    ) {
        tracing::warn!(
            event_name = "codex_state_diagnostics_write_failed",
            "无法更新 state 诊断，正式请求继续执行"
        );
    }
}

pub(super) async fn register(
    state: &AppState,
    plan: &ExecutionPlan,
    transport: &GatewayProviderTransportSnapshot,
) {
    if !background::current_credential_matches(plan, transport) {
        return;
    }
    best_effort(async {
        let key = index_key(&plan.provider_id, &plan.key_id);
        let configuration = background::configuration(state, transport).await?;
        let Some(lease) = state
            .runtime_state
            .lock_try_acquire(
                &format!("{key}:lock"),
                state.tunnel.local_instance_id(),
                Duration::from_secs(3),
            )
            .await
            .map_err(|_| StateError::runtime())?
        else {
            return Ok(());
        };
        let result = async {
            let mut entries: Vec<Scope> = state
                .runtime_state
                .kv_get(&key)
                .await
                .map_err(|_| StateError::runtime())?
                .map(|raw| serde_json::from_str(&raw))
                .transpose()
                .map_err(|_| StateError::runtime())?
                .unwrap_or_default();
            let scope = state_scope(state, plan);
            entries.retain(|entry| {
                entry.scope != scope && entry.at.saturating_add(KEEP.as_secs()) > now()
            });
            let proxy = plan.proxy.as_ref().filter(|p| p.enabled != Some(false));
            let local = route_scope(plan, "instance-a") != route_scope(plan, "instance-b");
            entries.insert(
                0,
                Scope {
                    scope,
                    endpoint_id: plan.endpoint_id.clone(),
                    configuration,
                    guard: format!(
                        "codex-state:guard:{}",
                        digest(
                            json!([
                                plan.provider_id,
                                plan.key_id,
                                header(&plan.headers, "chatgpt-account-id")
                            ])
                            .to_string()
                        )
                    ),
                    credential: digest(header(&plan.headers, "authorization")),
                    blocks: policy::expected_blocks(
                        header(&plan.headers, "authorization"),
                        header(&plan.headers, "chatgpt-account-id"),
                    ),
                    model: plan
                        .body
                        .json_body
                        .as_ref()
                        .and_then(|b| b.get("model"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    egress: if proxy.is_none() {
                        "direct"
                    } else if local {
                        "local_proxy"
                    } else {
                        "shared_proxy"
                    }
                    .into(),
                    node_id: proxy.and_then(|p| p.node_id.clone()),
                    instance: local.then(|| state.tunnel.local_instance_id().to_string()),
                    at: now(),
                },
            );
            entries.truncate(MAX_SCOPES);
            let saved = state
                .runtime_state
                .kv_set_if_lock_owned(
                    &lease,
                    &key,
                    &serde_json::to_string(&entries).map_err(|_| StateError::runtime())?,
                    KEEP,
                )
                .await
                .map_err(|_| StateError::runtime())?;
            if !saved {
                return Err(StateError::runtime());
            }
            Ok(())
        }
        .await;
        state
            .runtime_state
            .lock_release(&lease)
            .await
            .map_err(|_| StateError::runtime())?;
        result
    })
    .await;
}

pub(super) async fn record_use(
    state: &AppState,
    prepared: &Prepared,
    status: u16,
    invalidated: bool,
) {
    best_effort(async {
        state.runtime_state.kv_set(
            &format!("codex-state:last-use:{}", state_scope(state, &prepared.plan)),
            json!({"at": now(), "mode": if invalidated {"invalidated"} else if prepared.injected {"injected"} else {"passthrough"}, "http_status": status}).to_string(), Some(KEEP),
        ).await.map_err(|_| StateError::runtime())
    }).await;
}

pub(crate) async fn read(
    state: &AppState,
    provider_id: &str,
    key_id: &str,
) -> Result<Value, StateError> {
    let enabled = enabled(state).await?;
    let entries: Vec<Scope> = state
        .runtime_state
        .kv_get(&index_key(provider_id, key_id))
        .await
        .map_err(|_| StateError::runtime())?
        .map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(|_| StateError::runtime())?
        .unwrap_or_default();
    let mut items = Vec::new();
    for entry in entries
        .into_iter()
        .filter(|e| e.at.saturating_add(KEEP.as_secs()) > now())
        .take(MAX_SCOPES)
    {
        let transport = state
            .read_provider_transport_snapshot(provider_id, &entry.endpoint_id, key_id)
            .await
            .map_err(|_| StateError::runtime())?;
        let current = match transport.as_ref() {
            Some(transport)
                if transport.provider.is_active
                    && transport.endpoint.is_active
                    && transport.key.is_active =>
            {
                background::configuration(state, transport).await? == entry.configuration
            }
            _ => false,
        };
        let runtime = &state.runtime_state;
        let keys = [
            format!("codex-state:active:{}", entry.scope),
            format!("{}:rate", entry.guard),
            format!("{}:auth:{}", entry.guard, entry.credential),
            format!("codex-state:last-probe:{}", entry.scope),
            format!("codex-state:last-use:{}", entry.scope),
        ];
        let values = runtime
            .kv_get_many(&keys)
            .await
            .map_err(|_| StateError::runtime())?;
        let mut expires_at = None;
        if let Some(raw) = values[0].as_deref() {
            let plain = decrypt_python_fernet_ciphertext(
                state.encryption_key().ok_or_else(StateError::runtime)?,
                raw,
            )
            .map_err(|_| StateError::runtime())?;
            let cached: Cached = serde_json::from_str(&plain).map_err(|_| StateError::runtime())?;
            if policy::accepts(&cached.token, entry.blocks, now()) {
                expires_at = policy::parse(&cached.token)
                    .map(|(issued, _)| issued + policy::TTL_SECONDS - 30);
            }
        }
        let rate: Option<RateLimit> = values[1]
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| StateError::runtime())?;
        let auth: Option<AuthRejection> = values[2]
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| StateError::runtime())?;
        let retry_until = rate
            .filter(|r| r.retry_until > now())
            .map(|r| r.retry_until);
        let cooldown = runtime
            .kv_ttl_seconds(&format!("codex-state:cooldown:v2:{}", entry.scope))
            .await
            .map_err(|_| StateError::runtime())?
            .unwrap_or(0)
            .max(0);
        let status = if !enabled {
            "disabled"
        } else if !current {
            "configuration_changed"
        } else if retry_until.is_some() {
            "rate_limited"
        } else if auth.is_some() {
            "auth_rejected"
        } else if expires_at.is_some() {
            "ready"
        } else if cooldown > 0 {
            "cooldown"
        } else {
            "unavailable"
        };
        let probe: Option<Value> = values[3]
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| StateError::runtime())?;
        let last_use: Option<Value> = values[4]
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| StateError::runtime())?;
        items.push(json!({"model":entry.model,"egress":entry.egress,"node_id":entry.node_id,"instance":entry.instance,"last_seen_at":entry.at,
            "status":status,"expires_at":expires_at.filter(|_|current && enabled),"retry_until":retry_until.filter(|_|current),"auth_status":auth.filter(|_|current).map(|a|a.status),"cooldown_seconds":cooldown,
            "last_probe":probe.map(|p| json!({"at":p["at"],"status":p["status"],"accepted":p["accepted"],"reason":p["reason"]})),"last_use":last_use}));
    }
    Ok(json!({"enabled":enabled,"observed_at":now(),"items":items}))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{configured_state, plan, success};
    use super::*;

    async fn snapshot(state: &AppState) -> Value {
        read(state, "provider", "account-a").await.ok().unwrap()
    }

    #[tokio::test]
    async fn distinguishes_qualified_cache_usage_and_revocation_without_exposing_secrets() {
        let state = configured_state("codex", "oauth");
        let prepared = prepare_with_probe(&state, &plan(), |_| async { success() })
            .await
            .ok()
            .flatten()
            .unwrap();
        let result = snapshot(&state).await;
        assert_eq!(result["items"][0]["status"], "ready");
        assert_eq!(result["items"][0]["last_probe"]["reason"], "accepted");
        assert!(
            result["items"][0]["last_use"].is_null(),
            "collection alone is not injection"
        );
        prepared
            .observe(&state, 200, &mut BTreeMap::new())
            .await
            .ok()
            .unwrap();
        let result = snapshot(&state).await;
        assert_eq!(result["items"][0]["last_use"]["mode"], "injected");
        for secret in [
            "Bearer",
            "secret",
            "workspace",
            "private prompt",
            header(&prepared.plan.headers, HEADER),
        ] {
            assert!(!result.to_string().contains(secret));
        }
        let mut headers = BTreeMap::from([(HEADER.into(), "invalid".into())]);
        assert!(prepared.observe(&state, 200, &mut headers).await.is_err());
        let result = snapshot(&state).await;
        assert_ne!(result["items"][0]["status"], "ready");
        assert_eq!(result["items"][0]["last_use"]["mode"], "invalidated");
    }

    #[tokio::test]
    async fn missing_state_cooldown_and_passthrough_do_not_mean_auth_failure() {
        let state = configured_state("codex", "oauth");
        let prepared = prepare_with_fallback_probe(&state, &plan(), |_| async {
            let (status, _, body) = success().unwrap();
            Ok((status, BTreeMap::new(), body))
        })
        .await
        .ok()
        .flatten()
        .unwrap();
        prepared
            .observe(&state, 200, &mut BTreeMap::new())
            .await
            .ok()
            .unwrap();
        let result = snapshot(&state).await;
        assert_eq!(result["items"][0]["status"], "cooldown");
        assert_eq!(result["items"][0]["last_probe"]["reason"], "missing_state");
        assert_eq!(result["items"][0]["last_use"]["mode"], "passthrough");
        assert!(result["items"][0]["auth_status"].is_null());
        assert!(result["items"][0]["cooldown_seconds"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn guard_configuration_and_global_switch_override_cached_state() {
        let state = configured_state("codex", "oauth");
        let prepared = prepare_with_probe(&state, &plan(), |_| async { success() })
            .await
            .ok()
            .flatten()
            .unwrap();
        reject(
            &state,
            &prepared.guard_key,
            &prepared.credential,
            401,
            &BTreeMap::new(),
        )
        .await
        .ok()
        .unwrap();
        assert_eq!(
            snapshot(&state).await["items"][0]["status"],
            "auth_rejected"
        );
        reject(
            &state,
            &prepared.guard_key,
            &prepared.credential,
            429,
            &BTreeMap::new(),
        )
        .await
        .ok()
        .unwrap();
        assert_eq!(snapshot(&state).await["items"][0]["status"], "rate_limited");
        state
            .upsert_system_config_json_value("system_proxy_node_id", &json!("changed-node"), None)
            .await
            .unwrap();
        assert_eq!(
            snapshot(&state).await["items"][0]["status"],
            "configuration_changed"
        );
        state
            .upsert_system_config_json_value(CONFIG_KEY, &json!(false), None)
            .await
            .unwrap();
        assert_eq!(snapshot(&state).await["items"][0]["status"], "disabled");
    }

    #[tokio::test]
    async fn diagnostic_read_is_read_only_and_scope_history_is_bounded() {
        let state = configured_state("codex", "oauth");
        assert_eq!(snapshot(&state).await["items"], json!([]));
        let transport = state
            .read_provider_transport_snapshot("provider", "endpoint", "account-a")
            .await
            .unwrap()
            .unwrap();
        for index in 0..30 {
            let mut plan = plan();
            plan.proxy = Some(
                serde_json::from_value(json!({"node_id":format!("node-{index}"),"mode":"tunnel"}))
                    .unwrap(),
            );
            register(&state, &plan, &transport).await;
        }
        let result = snapshot(&state).await;
        assert_eq!(result["items"].as_array().unwrap().len(), MAX_SCOPES);
        assert_eq!(result["items"][0]["node_id"], "node-29");
        assert!(result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["last_probe"].is_null() && v["instance"].is_null()));
        // Two app instances can read the same registry without collecting state.
        let mut other = configured_state("codex", "oauth");
        other.runtime_state = state.runtime_state.clone();
        assert_eq!(snapshot(&other).await["items"], result["items"]);
    }

    #[tokio::test]
    async fn admin_diagnostic_read_checks_account_ownership_and_auth_type() {
        let state = configured_state("codex", "oauth");
        let admin = crate::admin_api::AdminAppState::new(&state);
        assert!(admin
            .read_codex_turn_state_diagnostics("other-provider", "account-a")
            .await
            .unwrap()
            .is_none());
        assert!(admin
            .read_codex_turn_state_diagnostics("provider", "unknown-account")
            .await
            .unwrap()
            .is_none());
        let response = admin
            .read_codex_turn_state_diagnostics("provider", "account-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response["items"], json!([]));
        let state = configured_state("codex", "api_key");
        let admin = crate::admin_api::AdminAppState::new(&state);
        assert!(admin
            .read_codex_turn_state_diagnostics("provider", "account-a")
            .await
            .unwrap()
            .is_none());
    }
}

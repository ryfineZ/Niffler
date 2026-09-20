//! 仅保存账号提示；调度前必须按当前配置重新验证真实缓存。
use super::*;
use crate::provider_transport::GatewayProviderTransportSnapshot;

fn index(provider: &str, endpoint: &str, model: &str) -> String {
    format!(
        "codex-state:priority:{}",
        digest(json!([provider, endpoint, model]).to_string())
    )
}

pub(super) async fn remember(state: &AppState, plan: &ExecutionPlan, cache: &Cached) {
    let Some((issued, _)) = policy::parse(&cache.token) else {
        return;
    };
    let model = plan
        .body
        .json_body
        .as_ref()
        .and_then(|b| b["model"].as_str())
        .unwrap_or_default();
    let key = index(&plan.provider_id, &plan.endpoint_id, model);
    // 同一账号的不同实例直连/出口可以有不同有效期，不能互相覆盖提示。
    let member = json!([plan.key_id, state_scope(state, plan)]).to_string();
    let result = tokio::time::timeout(Duration::from_millis(100), async {
        state
            .runtime_state
            .score_set(&key, &member, (issued + policy::TTL_SECONDS - 30) as f64)
            .await?;
        state
            .runtime_state
            .score_remove_by_score(&key, now() as f64)
            .await?;
        state
            .runtime_state
            .key_expire(&key, Duration::from_secs(policy::TTL_SECONDS))
            .await?;
        Ok::<_, aether_data_contracts::DataLayerError>(())
    })
    .await;
    if !matches!(result, Ok(Ok(()))) {
        tracing::warn!(
            event_name = "codex_state_priority_index_failed",
            "State 索引暂不可用，保留正常转发"
        );
    }
}

pub(crate) async fn candidates(
    state: &AppState,
    provider: &str,
    endpoint: &str,
    model: &str,
) -> Vec<String> {
    if !MODELS.contains(&model) {
        return Vec::new();
    }
    let result = tokio::time::timeout(Duration::from_millis(100), async {
        if !enabled(state).await? {
            return Ok(Vec::new());
        }
        state
            .runtime_state
            .score_range_by_min(&index(provider, endpoint, model), (now() + 1) as f64)
            .await
            .map_err(|_| StateError::runtime())
    })
    .await;
    match result {
        Ok(Ok(members)) => members
            .into_iter()
            .filter_map(|member| serde_json::from_str::<[String; 2]>(&member).ok())
            .map(|[key, _]| key)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        _ => {
            tracing::warn!(event_name = "codex_state_priority_read_failed");
            Vec::new()
        }
    }
}

pub(crate) async fn ready(
    state: &AppState,
    transport: &GatewayProviderTransportSnapshot,
    model: &str,
) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_millis(250), async {
            if !enabled(state).await? {
                return Ok(false);
            }
            let Some(plan) = Box::pin(catalog::plan(state, transport, model)).await? else {
                return Ok(false);
            };
            ready_plan(state, &plan).await
        })
        .await,
        Ok(Ok(true))
    )
}

pub(super) async fn ready_plan(state: &AppState, plan: &ExecutionPlan) -> Result<bool, StateError> {
    let account = header(&plan.headers, "chatgpt-account-id");
    let auth = header(&plan.headers, "authorization");
    let guard = format!(
        "codex-state:guard:{}",
        digest(json!([plan.provider_id, plan.key_id, account]).to_string())
    );
    if check_guard(state, &guard, &digest(auth)).await.is_err() {
        return Ok(false);
    }
    let Some(raw) = state
        .runtime_state
        .kv_get(&format!("codex-state:active:{}", state_scope(state, plan)))
        .await
        .map_err(|_| StateError::runtime())?
    else {
        return Ok(false);
    };
    let plaintext = decrypt_python_fernet_ciphertext(
        state.encryption_key().ok_or_else(StateError::runtime)?,
        &raw,
    )
    .map_err(|_| StateError::runtime())?;
    let cached: Cached = serde_json::from_str(&plaintext).map_err(|_| StateError::runtime())?;
    Ok(policy::accepts(
        &cached.token,
        policy::expected_blocks(auth, account),
        now(),
    ))
}

#[cfg(test)]
pub(crate) fn scope_for_tests(state: &AppState, plan: &ExecutionPlan) -> String {
    state_scope(state, plan)
}

#[cfg(test)]
pub(crate) async fn seed_for_tests(
    state: &AppState,
    transport: &GatewayProviderTransportSnapshot,
    model: &str,
) -> ExecutionPlan {
    let plan = catalog::plan(state, transport, model)
        .await
        .ok()
        .flatten()
        .expect("eligible plan");
    let cached = Cached {
        token: super::tests::success().ok().unwrap().headers[HEADER].clone(),
        version: "test-version".into(),
        source: Some("probe".into()),
    };
    let lease = state
        .runtime_state
        .lock_try_acquire("test-state-publish", "test", Duration::from_secs(5))
        .await
        .unwrap()
        .unwrap();
    publish_cache(
        state,
        &lease,
        &format!("codex-state:active:{}", state_scope(state, &plan)),
        &cached,
    )
    .await
    .ok()
    .unwrap();
    state.runtime_state.lock_release(&lease).await.unwrap();
    remember(state, &plan, &cached).await;
    plan
}

#[cfg(test)]
mod tests {
    use super::super::tests::{configured_state, plan, success, unexpected_probe};
    use super::*;

    #[tokio::test]
    async fn priority_keeps_instance_egress_and_auth_rate_protection() {
        for shared in [false, true] {
            let a = configured_state("codex", "oauth").with_tunnel_identity_for_tests("a", None);
            let mut b =
                configured_state("codex", "oauth").with_tunnel_identity_for_tests("b", None);
            b.runtime_state = a.runtime_state.clone();
            let mut p = plan();
            if shared {
                p.proxy = Some(
                    serde_json::from_value(
                        json!({"enabled":true,"url":"http://shared.example:18080"}),
                    )
                    .unwrap(),
                );
            }
            let prepared = prepare_with_probe(&a, &p, |_| async { success() })
                .await
                .ok()
                .flatten()
                .unwrap();
            assert_eq!(ready_plan(&b, &p).await.ok().unwrap(), shared);
            for status in [401, 429] {
                prepared
                    .observe(&a, status, &mut BTreeMap::new())
                    .await
                    .ok();
                assert!(!ready_plan(&a, &p).await.ok().unwrap());
            }
        }
    }

    #[tokio::test]
    async fn stale_hint_cannot_promote_invalidated_other_model_or_other_egress_state() {
        let state = configured_state("codex", "oauth");
        let p = plan();
        prepare_with_probe(&state, &p, |_| async { success() })
            .await
            .ok()
            .unwrap();
        assert!(ready_plan(&state, &p).await.ok().unwrap());
        assert_eq!(
            candidates(&state, "provider", "endpoint", "gpt-5.6-sol").await,
            ["account-a"]
        );
        for field in ["auth", "egress", "model"] {
            let mut changed = p.clone();
            match field {
                "auth" => {
                    changed
                        .headers
                        .insert("authorization".into(), "Bearer newer".into());
                }
                "egress" => {
                    changed.proxy = Some(
                        serde_json::from_value(
                            json!({"enabled":true,"url":"http://proxy.example:18080"}),
                        )
                        .unwrap(),
                    )
                }
                _ => changed.body.json_body.as_mut().unwrap()["model"] = json!("gpt-6-astra"),
            }
            assert!(!ready_plan(&state, &changed).await.ok().unwrap(), "{field}");
        }
        let prepared = prepare_with_probe(&state, &p, unexpected_probe)
            .await
            .ok()
            .flatten()
            .unwrap();
        let mut headers = BTreeMap::from([(HEADER.into(), "invalid".into())]);
        assert!(prepared.observe(&state, 200, &mut headers).await.is_err());
        assert!(!ready_plan(&state, &p).await.ok().unwrap());
        assert!(
            !candidates(&state, "provider", "endpoint", "gpt-5.6-sol")
                .await
                .is_empty(),
            "hint may be stale but cannot promote"
        );
        state
            .upsert_system_config_json_value(CONFIG_KEY, &json!(false), None)
            .await
            .unwrap();
        assert!(candidates(&state, "provider", "endpoint", "gpt-5.6-sol")
            .await
            .is_empty());
    }
}

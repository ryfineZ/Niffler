use super::*;

fn compact(original: &ExecutionPlan) -> ExecutionPlan {
    let mut p = original.clone();
    p.url.push_str("/compact");
    p.provider_api_format = "openai:responses:compact".into();
    p.client_api_format = "openai:responses:compact".into();
    p
}

#[tokio::test]
async fn compact_follows_valid_state_egress_without_injecting_and_shares_remote_proxy_across_instances(
) {
    let state =
        configured_state("codex", "oauth").with_tunnel_identity_for_tests("frontdoor-a", None);
    let mut generation = plan();
    generation.proxy = Some(
        serde_json::from_value(json!({"url":"http://proxy.example:18090","enabled":true})).unwrap(),
    );
    let prepared = prepare_with_probe(&state, &generation, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    let mut p = compact(&generation);
    p.proxy = None;
    let other = state
        .clone()
        .with_tunnel_identity_for_tests("frontdoor-b", None);
    let route = compact_route::prepare(&other, &p).await.ok().unwrap();
    assert!(route.bound);
    assert_eq!(route.plan.proxy, generation.proxy);
    assert_eq!(header(&route.plan.headers, HEADER), "client-state");
    assert_ne!(
        header(&route.plan.headers, HEADER),
        header(&prepared.plan.headers, HEADER)
    );
    let mut headers = BTreeMap::from([(HEADER.into(), "wrong-shape".into())]);
    assert!(route.observe(&other, 200, &headers).await.is_ok());
    assert!(other
        .runtime_state
        .kv_get(&prepared.cache_key)
        .await
        .unwrap()
        .is_some());
    headers.insert("retry-after".into(), "180".into());
    route.observe(&other, 429, &headers).await.ok().unwrap();
    assert_eq!(
        compact_route::prepare(&other, &p)
            .await
            .err()
            .unwrap()
            .status,
        429
    );
}

#[tokio::test]
async fn missing_stale_or_replaced_state_does_not_force_an_old_route() {
    let state = configured_state("codex", "oauth");
    let generation = plan();
    let p = compact(&generation);
    assert!(!compact_route::prepare(&state, &p).await.ok().unwrap().bound);
    let prepared = prepare_with_probe(&state, &generation, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    assert!(compact_route::prepare(&state, &p).await.ok().unwrap().bound);
    let mut cached: Cached = serde_json::from_str(
        &decrypt_python_fernet_ciphertext(state.encryption_key().unwrap(), &prepared.cached_value)
            .unwrap(),
    )
    .unwrap();
    cached.version = "replacement".into();
    let new = encrypt_python_fernet_plaintext(
        state.encryption_key().unwrap(),
        &serde_json::to_string(&cached).unwrap(),
    )
    .unwrap();
    state
        .runtime_state
        .kv_set(&prepared.cache_key, new, Some(Duration::from_secs(60)))
        .await
        .unwrap();
    assert!(!compact_route::prepare(&state, &p).await.ok().unwrap().bound);
    state
        .runtime_state
        .kv_delete(&prepared.cache_key)
        .await
        .unwrap();
    assert!(!compact_route::prepare(&state, &p).await.ok().unwrap().bound);
}

#[tokio::test]
async fn compact_never_crosses_model_workspace_credentials_or_account() {
    let state = configured_state("codex", "oauth");
    let original = plan();
    prepare_with_probe(&state, &original, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    for change in ["model", "workspace", "credential", "account"] {
        let mut p = compact(&original);
        match change {
            "model" => p.body.json_body.as_mut().unwrap()["model"] = json!("gpt-6-astra"),
            "workspace" => {
                p.headers
                    .insert("chatgpt-account-id".into(), "other".into());
            }
            "credential" => {
                p.headers
                    .insert("authorization".into(), "Bearer refreshed".into());
            }
            "account" => p.key_id = "other-account".into(),
            _ => unreachable!(),
        }
        assert!(!compact_route::prepare(&state, &p).await.ok().unwrap().bound);
    }
}

#[tokio::test]
async fn direct_egress_on_another_instance_is_an_explicit_error() {
    let state = configured_state("codex", "oauth").with_tunnel_identity_for_tests("a", None);
    let original = plan();
    prepare_with_probe(&state, &original, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    let other = state.clone().with_tunnel_identity_for_tests("b", None);
    assert_eq!(
        compact_route::prepare(&other, &compact(&original))
            .await
            .err()
            .unwrap()
            .code,
        "codex_turn_state_egress_unavailable"
    );
}

#[tokio::test]
async fn disabling_or_changing_configuration_stops_using_cached_egress() {
    let state = configured_state("codex", "oauth");
    let original = plan();
    prepare_with_probe(&state, &original, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    state
        .upsert_system_config_json_value("system_proxy_node_id", &json!("new-node"), None)
        .await
        .unwrap();
    assert!(
        !compact_route::prepare(&state, &compact(&original))
            .await
            .ok()
            .unwrap()
            .bound
    );
    state
        .upsert_system_config_json_value(CONFIG_KEY, &json!(false), None)
        .await
        .unwrap();
    assert!(
        !compact_route::prepare(&state, &compact(&original))
            .await
            .ok()
            .unwrap()
            .bound
    );
}

#[tokio::test]
async fn bound_node_must_still_be_available_and_old_auth_cannot_clear_compact_guard() {
    let state = configured_state("codex", "oauth");
    let mut original = plan();
    original.proxy = Some(
        serde_json::from_value(json!({"enabled":true,"mode":"tunnel","node_id":"removed"}))
            .unwrap(),
    );
    let prepared = prepare_with_probe(&state, &original, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    assert_eq!(
        compact_route::prepare(&state, &compact(&original))
            .await
            .err()
            .unwrap()
            .code,
        "codex_turn_state_egress_unavailable"
    );
    reject(
        &state,
        &prepared.guard_key,
        &prepared.credential,
        403,
        &BTreeMap::new(),
    )
    .await
    .ok()
    .unwrap();
    reject(&state, &prepared.guard_key, "old", 401, &BTreeMap::new())
        .await
        .ok()
        .unwrap();
    assert_eq!(
        compact_route::prepare(&state, &compact(&original))
            .await
            .err()
            .unwrap()
            .status,
        403
    );
}

#[tokio::test]
async fn shared_tunnel_binding_resolves_current_owner_on_the_other_frontdoor() {
    use aether_data::repository::proxy_nodes::{InMemoryProxyNodeRepository, StoredProxyNode};
    let repository = std::sync::Arc::new(InMemoryProxyNodeRepository::seed(vec![
        StoredProxyNode::new(
            "shared-node".into(),
            "Shared".into(),
            "127.0.0.1".into(),
            0,
            false,
            "online".into(),
            15,
            1,
            0,
            0,
            0,
            0,
            true,
            true,
            1,
        )
        .unwrap(),
    ]));
    let data = configured_data("codex", "oauth").attach_proxy_node_repository_for_tests(repository)
        .with_system_config_values_for_tests([
            (CONFIG_KEY.into(), json!(true)),
            ("tunnel.attachments.shared-node".into(), json!({"gateway_instance_id":"current-owner", "relay_base_url":"http://current-owner:18080", "conn_count":1, "observed_at_unix_secs":4102444800u64})),
        ]);
    let state = AppState::new()
        .unwrap()
        .with_data_state_for_tests(data)
        .with_tunnel_identity_for_tests("a", None);
    let mut original = plan();
    original.proxy = Some(
        serde_json::from_value(
            json!({"mode":"tunnel", "node_id":"shared-node", "enabled":true,
        "extra":{"tunnel_base_url":"http://old-owner:18080"}}),
        )
        .unwrap(),
    );
    prepare_with_probe(&state, &original, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    let other = state.clone().with_tunnel_identity_for_tests("b", None);
    let mut p = compact(&original);
    p.proxy = None;
    let route = compact_route::prepare(&other, &p).await.ok().unwrap();
    assert!(route.bound);
    let proxy = route.plan.proxy.unwrap();
    assert_eq!(proxy.node_id.as_deref(), Some("shared-node"));
    assert_eq!(
        proxy.extra.unwrap()["tunnel_base_url"],
        "http://current-owner:18080"
    );
}

#[tokio::test]
async fn expired_token_in_a_live_cache_does_not_pin_compact_to_stale_egress() {
    let state = configured_state("codex", "oauth");
    let original = plan();
    let prepared = prepare_with_probe(&state, &original, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    let mut cache: Cached = serde_json::from_str(
        &decrypt_python_fernet_ciphertext(state.encryption_key().unwrap(), &prepared.cached_value)
            .unwrap(),
    )
    .unwrap();
    let mut raw = base64::engine::general_purpose::URL_SAFE
        .decode(&cache.token)
        .unwrap();
    raw[1..9].copy_from_slice(&(now() - policy::TTL_SECONDS).to_be_bytes());
    cache.token = base64::engine::general_purpose::URL_SAFE.encode(raw);
    let encrypted = encrypt_python_fernet_plaintext(
        state.encryption_key().unwrap(),
        &serde_json::to_string(&cache).unwrap(),
    )
    .unwrap();
    state
        .runtime_state
        .kv_set(
            &prepared.cache_key,
            encrypted,
            Some(Duration::from_secs(60)),
        )
        .await
        .unwrap();
    assert!(
        !compact_route::prepare(&state, &compact(&original))
            .await
            .ok()
            .unwrap()
            .bound
    );
}

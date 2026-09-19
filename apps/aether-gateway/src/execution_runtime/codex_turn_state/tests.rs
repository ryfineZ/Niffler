use super::*;
use base64::Engine as _;

#[path = "compact_route_tests.rs"]
mod compact_route_tests;
#[path = "sync_tests.rs"]
mod sync_tests;

fn plan() -> ExecutionPlan {
    serde_json::from_value(json!({"request_id":"request", "provider_id":"provider", "endpoint_id":"endpoint", "key_id":"account-a",
        "method":"POST", "url":"https://chatgpt.com/backend-api/codex/responses",
        "headers":{"authorization":"Bearer secret", "chatgpt-account-id":"workspace", "x-codex-turn-state":"client-state", "cookie":"private"},
        "body":{"json_body":{"model":"gpt-5.6-sol","input":"private prompt"}},
        "client_api_format":"openai:responses", "provider_api_format":"openai:responses"})).unwrap()
}

#[test]
fn direct_and_loopback_routes_are_instance_local_but_tunnel_is_shared() {
    let mut p = plan();
    assert_ne!(route_scope(&p, "a"), route_scope(&p, "b"));
    p.proxy = Some(
        serde_json::from_value(json!({"enabled":true,"url":"http://127.0.0.1:18090"})).unwrap(),
    );
    assert_ne!(route_scope(&p, "a"), route_scope(&p, "b"));
    p.proxy.as_mut().unwrap().node_id = Some("manual-local-node".into());
    assert_ne!(route_scope(&p, "a"), route_scope(&p, "b"));
    p.proxy.as_mut().unwrap().url = Some("http://[::1]:18090".into());
    assert_ne!(route_scope(&p, "a"), route_scope(&p, "b"));
    p.proxy = Some(
        serde_json::from_value(json!({"enabled":true,"mode":"tunnel","node_id":"fixed-node"}))
            .unwrap(),
    );
    assert_eq!(route_scope(&p, "a"), route_scope(&p, "b"));
    let old = route_scope(&p, "a");
    p.proxy.as_mut().unwrap().node_id = Some("replacement-node".into());
    assert_ne!(old, route_scope(&p, "a"));
}

#[test]
fn probe_preserves_egress_and_auth_but_never_user_content_or_state() {
    let mut p = plan();
    p.proxy = Some(
        serde_json::from_value(json!({"enabled":true,"mode":"tunnel","node_id":"fixed-node"}))
            .unwrap(),
    );
    let probe = probe_plan(&p);
    assert_eq!(probe.proxy, p.proxy);
    assert_eq!(header(&probe.headers, "authorization"), "Bearer secret");
    assert_eq!(header(&probe.headers, HEADER), "");
    assert_eq!(header(&probe.headers, "cookie"), "");
    assert!(!probe
        .body
        .json_body
        .unwrap()
        .to_string()
        .contains("private prompt"));
}

#[test]
fn only_official_supported_generation_is_eligible() {
    let mut p = plan();
    assert!(eligible(&p));
    p.url = "https://relay.example/responses".into();
    assert!(!eligible(&p));
    p = plan();
    p.body.json_body.as_mut().unwrap()["compaction_trigger"] = json!({});
    assert!(!eligible(&p));
    p = plan();
    p.client_api_format = "openai:image".into();
    assert!(!eligible(&p));
    p = plan();
    p.body.json_body.as_mut().unwrap()["model"] = json!("another-model");
    assert!(!eligible(&p));
}

#[tokio::test]
async fn feature_defaults_off_without_probing_or_mutating_plan() {
    let state = AppState::new().unwrap();
    let p = plan();
    assert!(prepare(&state, &p).await.ok().flatten().is_none());
    assert_eq!(header(&p.headers, HEADER), "client-state");
}

#[tokio::test]
async fn native_compaction_never_collects_or_injects_generation_state() {
    let state = configured_state("codex", "oauth");
    let mut p = plan();
    p.body.json_body.as_mut().unwrap()["input"] = json!([
        {"role":"user","content":"history"}, {"type":"compaction_trigger"}
    ]);
    assert!(!eligible(&p), "V2 compression is not a normal generation");
    assert!(prepare_with_probe(&state, &p, unexpected_probe)
        .await
        .ok()
        .flatten()
        .is_none());

    for input in [
        json!([{"role":"user","content":"compaction_trigger"}]),
        json!([{"type":"compaction","encrypted_content":"opaque"}]),
        json!([{"type":"compaction_trigger","text":"not a protocol item"}]),
        json!([{"type":"compaction_trigger"},{"role":"user","content":"continue"}]),
    ] {
        p.body.json_body.as_mut().unwrap()["input"] = input;
        assert!(
            eligible(&p),
            "ordinary conversations must retain state handling"
        );
    }
}

#[tokio::test]
async fn one_bad_response_rejects_only_its_own_version() {
    let state = AppState::new().unwrap();
    let p = Prepared {
        plan: plan(),
        cache_key: "state".into(),
        cached_value: "old-version".into(),
        guard_key: "guard".into(),
        credential: "credential".into(),
        blocks: 10,
    };
    state
        .runtime_state
        .kv_set("state", "new-version", Some(Duration::from_secs(60)))
        .await
        .unwrap();
    let mut raw = vec![0; 57 + 11 * 16];
    raw[0] = 0x80;
    raw[1..9].copy_from_slice(&now().to_be_bytes());
    let mut headers = BTreeMap::from([(
        HEADER.into(),
        base64::engine::general_purpose::URL_SAFE.encode(raw),
    )]);
    let error = p.observe(&state, 200, &mut headers).await.err().unwrap();
    assert!(error.sent);
    assert!(terminal_error(Some(&error.body().to_string())));
    assert!(!headers.contains_key(HEADER));
    assert_eq!(
        state
            .runtime_state
            .kv_get("state")
            .await
            .unwrap()
            .as_deref(),
        Some("new-version")
    );
    state
        .runtime_state
        .kv_set("state", "old-version", Some(Duration::from_secs(60)))
        .await
        .unwrap();
    assert!(p.observe(&state, 200, &mut BTreeMap::new()).await.is_ok());
    let mut bad = BTreeMap::from([(HEADER.into(), "bad-envelope".into())]);
    assert!(p.observe(&state, 200, &mut bad).await.is_err());
    assert!(state.runtime_state.kv_get("state").await.unwrap().is_none());
}

#[tokio::test]
async fn account_rejection_retains_auth_and_rate_limits_across_racing_models() {
    let state = AppState::new().unwrap();
    let empty = BTreeMap::new();
    let delay = BTreeMap::from([("retry-after".into(), "3600".into())]);
    let (a, b, c) = tokio::join!(
        reject(&state, "guard", "old-auth", 401, &empty),
        reject(&state, "guard", "old-auth", 429, &delay),
        reject(&state, "guard", "new-auth", 403, &empty)
    );
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    assert_eq!(
        check_guard(&state, "guard", "refreshed-auth")
            .await
            .err()
            .unwrap()
            .status,
        429
    );
    let raw = state
        .runtime_state
        .kv_get("guard:rate")
        .await
        .unwrap()
        .unwrap();
    let record: RateLimit = serde_json::from_str(&raw).unwrap();
    assert!(record.retry_until >= now() + 3599);
    // 限流结束后，各凭据自己的认证拒绝仍然保留。
    state.runtime_state.kv_delete("guard:rate").await.unwrap();
    assert_eq!(
        check_guard(&state, "guard", "old-auth")
            .await
            .err()
            .map(|e| e.status),
        Some(401)
    );
    assert_eq!(
        check_guard(&state, "guard", "new-auth")
            .await
            .err()
            .map(|e| e.status),
        Some(403)
    );
    assert!(check_guard(&state, "guard", "refreshed-auth").await.is_ok());
}

#[tokio::test]
async fn concurrent_rate_limits_keep_longest_account_cooldown() {
    let state = AppState::new().unwrap();
    let long = BTreeMap::from([("retry-after".into(), "3600".into())]);
    let short = BTreeMap::from([("retry-after".into(), "180".into())]);
    let (a, b) = tokio::join!(
        reject(&state, "guard", "old-auth", 429, &long),
        reject(&state, "guard", "new-auth", 429, &short)
    );
    assert!(a.is_ok() && b.is_ok());
    let raw = state
        .runtime_state
        .kv_get("guard:rate")
        .await
        .unwrap()
        .unwrap();
    let record: RateLimit = serde_json::from_str(&raw).unwrap();
    assert!(record.retry_until >= now() + 3599);
    assert_eq!(
        check_guard(&state, "guard", "next-auth")
            .await
            .err()
            .map(|e| e.status),
        Some(429)
    );
    assert!(check_guard(&state, "different-account", "old-auth")
        .await
        .is_ok());
}

#[tokio::test]
async fn late_old_credential_rejection_cannot_clear_new_credential_rejection() {
    let state = AppState::new().unwrap();
    let headers = BTreeMap::new();
    for (credential, status) in [("new-auth", 403), ("old-auth", 401)] {
        assert!(reject(&state, "guard", credential, status, &headers)
            .await
            .is_ok());
    }
    assert_eq!(
        check_guard(&state, "guard", "new-auth")
            .await
            .err()
            .map(|e| e.status),
        Some(403)
    );
    assert_eq!(
        check_guard(&state, "guard", "old-auth")
            .await
            .err()
            .map(|e| e.status),
        Some(401)
    );
    assert!(check_guard(&state, "guard", "next-auth").await.is_ok());
}

#[tokio::test]
async fn concurrent_credential_rejections_remain_independent() {
    let state = AppState::new().unwrap();
    let headers = BTreeMap::new();
    let (old, new) = tokio::join!(
        reject(&state, "guard", "old-auth", 401, &headers),
        reject(&state, "guard", "new-auth", 403, &headers)
    );
    assert!(old.is_ok() && new.is_ok());
    assert_eq!(
        check_guard(&state, "guard", "old-auth")
            .await
            .err()
            .map(|e| e.status),
        Some(401)
    );
    assert_eq!(
        check_guard(&state, "guard", "new-auth")
            .await
            .err()
            .map(|e| e.status),
        Some(403)
    );
}

#[tokio::test]
async fn state_errors_stop_both_sync_and_stream_candidate_failover() {
    let state = AppState::new().unwrap();
    let p = plan();
    let error = StateError::shape();
    let body = error.body().to_string();
    let sync = crate::execution_runtime::fallback::analyze_local_candidate_failover_sync(
        &state,
        &p,
        "openai_responses_sync",
        None,
        &error.sync(&p),
        Some(&body),
    )
    .await;
    let stream =
        crate::execution_runtime::fallback::resolve_local_candidate_failover_analysis_stream(
            &state,
            &p,
            None,
            503,
            Some(&body),
        )
        .await;
    assert!(matches!(
        sync.decision,
        crate::orchestration::LocalFailoverDecision::StopLocalFailover
    ));
    assert!(matches!(
        stream.decision,
        crate::orchestration::LocalFailoverDecision::StopLocalFailover
    ));
}
#[test]
fn only_known_state_failures_stop_failover() {
    assert!(terminal_error(Some(
        &StateError::shape().body().to_string()
    )));
    assert!(!terminal_error(Some(
        r#"{"error":{"code":"server_overloaded"}}"#
    )));
}
#[test]
fn removes_opaque_state_from_nested_report_headers() {
    let mut value =
        json!({"provider_response_headers":{"X-Codex-Turn-State":"secret","retry-after":"180"}});
    scrub_context(&mut value);
    assert_eq!(
        value,
        json!({"provider_response_headers":{"retry-after":"180"}})
    );
}

fn configured_state(provider_type: &str, auth_type: &str) -> AppState {
    AppState::new()
        .unwrap()
        .with_data_state_for_tests(configured_data(provider_type, auth_type))
}

fn configured_data(provider_type: &str, auth_type: &str) -> crate::data::GatewayDataState {
    use aether_data::repository::provider_catalog::InMemoryProviderCatalogReadRepository;
    use aether_data_contracts::repository::provider_catalog::{
        StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
    };
    let provider = StoredProviderCatalogProvider::new(
        "provider".into(),
        "Provider".into(),
        None,
        provider_type.into(),
    )
    .unwrap();
    let endpoint = StoredProviderCatalogEndpoint::new(
        "endpoint".into(),
        "provider".into(),
        "openai:responses".into(),
        Some("openai".into()),
        Some("responses".into()),
        true,
    )
    .unwrap();
    let key = StoredProviderCatalogKey::new(
        "account-a".into(),
        "provider".into(),
        "Key".into(),
        auth_type.into(),
        None,
        true,
    )
    .unwrap();
    let catalog =
        InMemoryProviderCatalogReadRepository::seed(vec![provider], vec![endpoint], vec![key]);
    crate::data::GatewayDataState::with_provider_transport_reader_for_tests(
        std::sync::Arc::new(catalog),
        "test-encryption-key",
    )
    .with_system_config_values_for_tests([(CONFIG_KEY.to_string(), json!(true))])
}

fn success() -> ProbeResult {
    let mut raw = vec![0; 57 + 10 * 16];
    raw[0] = 0x80;
    raw[1..9].copy_from_slice(&now().to_be_bytes());
    Ok((200, BTreeMap::from([(HEADER.into(), base64::engine::general_purpose::URL_SAFE.encode(raw))]),
        b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\n".to_vec()))
}

async fn unexpected_probe(_: ExecutionPlan) -> ProbeResult {
    panic!("unexpected upstream probe")
}

#[tokio::test]
async fn concurrent_requests_share_one_probe_and_encrypted_cache() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let state = configured_state("codex", "oauth");
    let count = AtomicUsize::new(0);
    let mut original = plan();
    original.headers.insert(
        aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER.to_uppercase(),
        "true".into(),
    );
    let count_ref = &count;
    let probe = |dispatched: ExecutionPlan| async move {
        count_ref.fetch_add(1, Ordering::SeqCst);
        assert_eq!(header(&dispatched.headers, HEADER), "");
        assert!(!dispatched
            .body
            .json_body
            .unwrap()
            .to_string()
            .contains("private prompt"));
        tokio::time::sleep(Duration::from_millis(30)).await;
        success()
    };
    let (a, b) = tokio::join!(
        prepare_with_probe(&state, &original, &probe),
        prepare_with_probe(&state, &original, &probe)
    );
    let a = a.ok().flatten().expect("first prepared plan");
    let b = b.ok().flatten().expect("second prepared plan");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(a.cache_key, b.cache_key);
    assert_eq!(
        header(&a.plan.headers, HEADER),
        header(&b.plan.headers, HEADER)
    );
    assert_ne!(header(&a.plan.headers, HEADER), "client-state");
    assert_eq!(
        header(
            &a.plan.headers,
            aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER
        ),
        "false"
    );
    assert!(!a.cached_value.contains(header(&a.plan.headers, HEADER)));
    assert_eq!(header(&original.headers, HEADER), "client-state");
    assert!(prepare_with_probe(&state, &original, unexpected_probe)
        .await
        .ok()
        .flatten()
        .is_some());

    // 新凭据、模型或出口不可使用旧缓存；账号冷却期间也不可另起采集绕开预算。
    for changed in ["credential", "model", "egress", "workspace"] {
        let mut changed_plan = original.clone();
        match changed {
            "credential" => {
                changed_plan
                    .headers
                    .insert("authorization".into(), "Bearer refreshed".into());
            }
            "model" => {
                changed_plan.body.json_body.as_mut().unwrap()["model"] = json!("gpt-6-astra")
            }
            "egress" => {
                changed_plan.proxy = Some(
                    serde_json::from_value(json!({"mode":"tunnel","node_id":"other-route"}))
                        .unwrap(),
                )
            }
            "workspace" => {
                changed_plan
                    .headers
                    .insert("chatgpt-account-id".into(), "other-workspace".into());
            }
            _ => unreachable!(),
        }
        if changed == "workspace" {
            let prepared = prepare_with_probe(&state, &changed_plan, |_| async { success() })
                .await
                .ok()
                .flatten()
                .unwrap();
            assert_ne!(prepared.cache_key, a.cache_key);
        } else {
            assert!(prepare_with_probe(&state, &changed_plan, unexpected_probe)
                .await
                .is_err());
        }
    }
}

#[tokio::test]
async fn rejects_incomplete_probe_and_enforces_cooldown() {
    let state = configured_state("codex", "oauth");
    let result = prepare_with_probe(&state, &plan(), |_| async {
        let (status, headers, _) = success().unwrap();
        Ok((
            status,
            headers,
            b"data: {\"type\":\"response.created\"}\n\n".to_vec(),
        ))
    })
    .await;
    assert!(result.is_err());
    assert!(prepare_with_probe(&state, &plan(), unexpected_probe)
        .await
        .is_err());
}

#[tokio::test]
async fn probe_rate_limit_is_shared_across_models_and_refreshed_credentials() {
    let state = configured_state("codex", "oauth");
    let mut original = plan();
    let error = prepare_with_probe(&state, &original, |_| async {
        Ok((429, BTreeMap::new(), vec![]))
    })
    .await
    .err()
    .unwrap();
    assert_eq!(error.status, 429);
    assert!(!error.sent);
    original
        .headers
        .insert("authorization".into(), "Bearer refreshed".into());
    original.body.json_body.as_mut().unwrap()["model"] = json!("gpt-6-astra");
    assert_eq!(
        prepare_with_probe(&state, &original, unexpected_probe)
            .await
            .err()
            .unwrap()
            .status,
        429
    );
}

#[tokio::test]
async fn excludes_non_codex_and_api_key_accounts_even_on_official_url() {
    for (provider, auth) in [("custom", "oauth"), ("codex", "api_key")] {
        let state = configured_state(provider, auth);
        assert!(prepare_with_probe(&state, &plan(), unexpected_probe)
            .await
            .ok()
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn disabling_during_collection_prevents_publication() {
    let state = configured_state("codex", "oauth");
    let result = prepare_with_probe(&state, &plan(), |_| async {
        state
            .upsert_system_config_json_value(CONFIG_KEY, &json!(false), None)
            .await
            .unwrap();
        success()
    })
    .await;
    assert!(result.is_err());
    assert!(prepare_with_probe(&state, &plan(), unexpected_probe)
        .await
        .ok()
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn frontdoors_share_tunnel_state_but_keep_direct_state_separate() {
    let a = configured_state("codex", "oauth").with_tunnel_identity_for_tests("frontdoor-a", None);
    let b = a
        .clone()
        .with_tunnel_identity_for_tests("frontdoor-b", None);
    let mut original = plan();
    original.proxy =
        Some(serde_json::from_value(json!({"mode":"tunnel","node_id":"same-egress"})).unwrap());
    let first = prepare_with_probe(&a, &original, |_| async { success() })
        .await
        .ok()
        .flatten()
        .unwrap();
    let second = prepare_with_probe(&b, &original, unexpected_probe)
        .await
        .ok()
        .flatten()
        .unwrap();
    assert_eq!(first.cache_key, second.cache_key);
    original.proxy = None;
    assert!(prepare_with_probe(&b, &original, unexpected_probe)
        .await
        .is_err());
}

#[tokio::test]
async fn buffered_state_error_survives_stream_framing() {
    let execution = StateError::shape().stream(&plan());
    let frames =
        crate::execution_runtime::stream_pump::build_direct_execution_frame_stream(execution)
            .collect::<Vec<_>>()
            .await;
    let frames: Vec<Value> = frames
        .into_iter()
        .map(|frame| serde_json::from_slice(&frame.unwrap()).unwrap())
        .collect();
    assert_eq!(frames[0]["payload"]["status_code"], 503);
    let data = frames.iter().find(|frame| frame["type"] == "data").unwrap();
    let payload = base64::engine::general_purpose::STANDARD
        .decode(data["payload"]["chunk_b64"].as_str().unwrap())
        .unwrap();
    let body: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(body["error"]["upstream_request_sent"], true);
    assert!(terminal_error(std::str::from_utf8(&payload).ok()));
    assert_eq!(frames.last().unwrap()["type"], "eof");
}

#[tokio::test]
async fn excessive_retry_after_does_not_overflow_runtime_ttl() {
    let state = AppState::new().unwrap();
    let headers = BTreeMap::from([("retry-after".into(), u64::MAX.to_string())]);
    assert!(reject(&state, "guard", "credential", 429, &headers)
        .await
        .is_ok());
    assert_eq!(
        check_guard(&state, "guard", "credential")
            .await
            .err()
            .unwrap()
            .status,
        429
    );
}

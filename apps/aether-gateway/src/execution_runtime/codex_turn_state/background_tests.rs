use super::super::tests::{configured_state, plan, success, unexpected_probe};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn background_collects_active_missing_state_and_skips_healthy_cache() {
    let state = configured_state("codex", "oauth");
    remember(&state, &plan()).await.ok().unwrap();
    let calls = AtomicUsize::new(0);
    tick_with_probe(&state, |probe| {
        calls.fetch_add(1, Ordering::SeqCst);
        assert!(!serde_json::to_string(&probe)
            .unwrap()
            .contains("private prompt"));
        assert_eq!(header(&probe.headers, HEADER), "");
        async { success() }
    })
    .await
    .ok()
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tick_with_probe(&state, unexpected_probe)
        .await
        .ok()
        .unwrap();
    assert!(
        prepare_with_probe(&state, &plan(), unexpected_probe)
            .await
            .ok()
            .flatten()
            .unwrap()
            .injected
    );
}

#[tokio::test]
async fn background_retries_after_cooldown_without_another_user_request() {
    let state = configured_state("codex", "oauth");
    remember(&state, &plan()).await.ok().unwrap();
    tick_with_probe(&state, |_| async { Err(()) })
        .await
        .ok()
        .unwrap();
    tick_with_probe(&state, unexpected_probe)
        .await
        .ok()
        .unwrap();
    state
        .runtime_state
        .kv_delete(&format!(
            "codex-state:cooldown:v2:{}",
            state_scope(&state, &plan())
        ))
        .await
        .unwrap();
    tick_with_probe(&state, |_| async { success() })
        .await
        .ok()
        .unwrap();
    assert!(
        prepare_with_probe(&state, &plan(), unexpected_probe)
            .await
            .ok()
            .flatten()
            .unwrap()
            .injected
    );
}

#[tokio::test]
async fn background_drops_idle_changed_or_disabled_sessions() {
    for change in ["idle", "configuration", "disabled"] {
        let state = configured_state("codex", "oauth");
        remember(&state, &plan()).await.ok().unwrap();
        match change {
            "idle" => {
                for session in state
                    .codex_turn_state_sessions
                    .0
                    .lock()
                    .unwrap()
                    .values_mut()
                {
                    session.last_seen -= ACTIVE_FOR + Duration::from_secs(1);
                }
            }
            "configuration" => {
                state
                    .upsert_system_config_json_value(
                        "system_proxy_node_id",
                        &json!("new-node"),
                        None,
                    )
                    .await
                    .unwrap();
            }
            _ => {
                state
                    .upsert_system_config_json_value(CONFIG_KEY, &json!(false), None)
                    .await
                    .unwrap();
            }
        }
        tick_with_probe(&state, unexpected_probe)
            .await
            .ok()
            .unwrap();
        assert!(
            state.codex_turn_state_sessions.0.lock().unwrap().is_empty(),
            "{change}"
        );
    }
}

#[tokio::test]
async fn background_respects_protection_and_does_not_reactivate_idle_sessions() {
    let state = configured_state("codex", "oauth");
    remember(&state, &plan()).await.ok().unwrap();
    let seen = state
        .codex_turn_state_sessions
        .0
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .last_seen;
    tick_with_probe(&state, |_| async { Ok((401, BTreeMap::new(), vec![])) })
        .await
        .ok()
        .unwrap();
    tick_with_probe(&state, unexpected_probe)
        .await
        .ok()
        .unwrap();
    assert_eq!(
        state
            .codex_turn_state_sessions
            .0
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .last_seen,
        seen
    );
}

#[tokio::test]
async fn sessions_keep_only_encrypted_probe_content() {
    let state = configured_state("codex", "oauth");
    remember(&state, &plan()).await.ok().unwrap();
    let sessions = state.codex_turn_state_sessions.0.lock().unwrap();
    let session = sessions.values().next().unwrap();
    assert!(!session.encrypted_plan.contains("Bearer secret"));
    let raw =
        decrypt_python_fernet_ciphertext(state.encryption_key().unwrap(), &session.encrypted_plan)
            .unwrap();
    assert!(!raw.contains("private prompt"));
    assert!(!raw.contains("client-state"));
    assert!(!raw.contains("cookie"));
}

#[tokio::test]
async fn two_frontdoors_and_foreground_share_one_collection_for_shared_egress() {
    let a = configured_state("codex", "oauth").with_tunnel_identity_for_tests("a", None);
    let mut b = configured_state("codex", "oauth").with_tunnel_identity_for_tests("b", None);
    b.runtime_state = a.runtime_state.clone();
    let mut p = plan();
    p.proxy = Some(
        serde_json::from_value(json!({"enabled":true,"url":"http://shared.example:18080"}))
            .unwrap(),
    );
    remember(&a, &p).await.ok().unwrap();
    remember(&b, &p).await.ok().unwrap();
    let calls = AtomicUsize::new(0);
    let probe = |_: ExecutionPlan| async {
        calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        success()
    };
    let (first, second, foreground) = tokio::join!(
        tick_with_probe(&a, &probe),
        tick_with_probe(&b, &probe),
        prepare_with_probe(&a, &p, &probe)
    );
    assert!(first.is_ok() && second.is_ok());
    assert!(foreground.ok().flatten().unwrap().injected);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn changed_provider_configuration_discards_old_background_plan() {
    use super::super::tests::configured_data;
    let state = configured_state("codex", "oauth");
    remember(&state, &plan()).await.ok().unwrap();
    let changed = state
        .clone()
        .with_data_state_for_tests(configured_data("other", "oauth"));
    tick_with_probe(&changed, unexpected_probe)
        .await
        .ok()
        .unwrap();
    assert!(changed
        .codex_turn_state_sessions
        .0
        .lock()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn registry_evicts_oldest_when_full() {
    let state = configured_state("codex", "oauth");
    remember(&state, &plan()).await.ok().unwrap();
    {
        let mut sessions = state.codex_turn_state_sessions.0.lock().unwrap();
        let mut template = sessions.values().next().unwrap().clone();
        template.last_seen -= Duration::from_secs(10);
        sessions.clear();
        sessions.insert("oldest".into(), template.clone());
        template.last_seen += Duration::from_secs(1);
        for id in 1..MAX_SESSIONS {
            sessions.insert(id.to_string(), template.clone());
        }
    }
    remember(&state, &plan()).await.ok().unwrap();
    let sessions = state.codex_turn_state_sessions.0.lock().unwrap();
    assert_eq!(sessions.len(), MAX_SESSIONS);
    assert!(!sessions.contains_key("oldest"));
}

#[tokio::test]
async fn background_does_not_change_compact_egress_before_a_real_request_uses_state() {
    let state = configured_state("codex", "oauth");
    let p = plan();
    remember(&state, &p).await.ok().unwrap();
    tick_with_probe(&state, |_| async { success() })
        .await
        .ok()
        .unwrap();
    assert!(!compact_route::prepare(&state, &p).await.ok().unwrap().bound);
    prepare_with_probe(&state, &p, unexpected_probe)
        .await
        .ok()
        .unwrap();
    assert!(compact_route::prepare(&state, &p).await.ok().unwrap().bound);
}

#[tokio::test]
async fn stale_request_cannot_register_old_credentials_after_refresh() {
    let state = configured_state("codex", "oauth");
    let mut p = plan();
    p.headers
        .insert("authorization".into(), "Bearer old".into());
    remember(&state, &p).await.ok().unwrap();
    assert!(state.codex_turn_state_sessions.0.lock().unwrap().is_empty());
    tick_with_probe(&state, unexpected_probe)
        .await
        .ok()
        .unwrap();
}

#[tokio::test]
async fn unavailable_node_keeps_session_for_later_recovery() {
    let state = configured_state("codex", "oauth");
    let mut p = plan();
    p.proxy = Some(
        serde_json::from_value(
            json!({"enabled":true,"mode":"tunnel","node_id":"temporarily-unavailable"}),
        )
        .unwrap(),
    );
    remember(&state, &p).await.ok().unwrap();
    tick_with_probe(&state, unexpected_probe)
        .await
        .ok()
        .unwrap();
    assert_eq!(state.codex_turn_state_sessions.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn collection_worker_is_supervised_and_stops_on_shutdown() {
    let state = AppState::new().unwrap();
    let supervisor = state.spawn_turn_state_background_tasks();
    assert_eq!(supervisor.task_count(), 1);
    tokio::time::timeout(Duration::from_secs(1), supervisor.shutdown())
        .await
        .unwrap();
}

#[tokio::test]
async fn configuration_change_between_attempts_stops_background_dispatch() {
    let state = configured_state("codex", "oauth");
    remember(&state, &plan()).await.ok().unwrap();
    let calls = AtomicUsize::new(0);
    tick_with_probe(&state, |_| async {
        calls.fetch_add(1, Ordering::SeqCst);
        state
            .upsert_system_config_json_value("system_proxy_node_id", &json!("changed"), None)
            .await
            .unwrap();
        Err(())
    })
    .await
    .ok()
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tick_with_probe(&state, unexpected_probe)
        .await
        .ok()
        .unwrap();
    assert!(state.codex_turn_state_sessions.0.lock().unwrap().is_empty());
}

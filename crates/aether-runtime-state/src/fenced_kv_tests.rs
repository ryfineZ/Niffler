use std::time::Duration;

use crate::{MemoryRuntimeStateConfig, RuntimeState};

#[tokio::test]
#[ignore = "requires an isolated Redis via AETHER_FENCED_KV_TEST_REDIS_URL"]
async fn fenced_kv_redis_enforces_ownership_across_independent_clients() {
    let config = crate::redis::RedisClientConfig {
        url: std::env::var("AETHER_FENCED_KV_TEST_REDIS_URL").expect("isolated test Redis URL"),
        key_prefix: Some(format!("fenced-test-{}", uuid::Uuid::new_v4())),
    };
    let a = RuntimeState::redis(config.clone(), Some(1000))
        .await
        .unwrap();
    let b = RuntimeState::redis(config, Some(1000)).await.unwrap();
    let old = a
        .lock_try_acquire("collector", "a", Duration::from_millis(20))
        .await
        .unwrap()
        .unwrap();
    assert!(b
        .lock_try_acquire("collector", "b", Duration::from_secs(10))
        .await
        .unwrap()
        .is_none());
    tokio::time::sleep(Duration::from_millis(40)).await;
    let current = b
        .lock_try_acquire("collector", "b", Duration::from_secs(10))
        .await
        .unwrap()
        .unwrap();
    assert!(b
        .kv_set_if_lock_owned(&current, "active", "new", Duration::from_secs(10))
        .await
        .unwrap());
    assert!(!a
        .kv_set_if_lock_owned(&old, "active", "stale", Duration::from_secs(10))
        .await
        .unwrap());
    assert!(!a.kv_delete_if_value("active", "stale").await.unwrap());
    assert_eq!(a.kv_get("active").await.unwrap().as_deref(), Some("new"));
    assert!(b.lock_release(&current).await.unwrap());
    assert!(!b
        .kv_set_if_lock_owned(&current, "active", "released", Duration::from_secs(10))
        .await
        .unwrap());
    assert!(a.kv_delete_if_value("active", "new").await.unwrap());
    assert!(b.kv_get("active").await.unwrap().is_none());
}

#[tokio::test]
async fn fenced_kv_rejects_expired_publisher_and_preserves_new_state() {
    let state = RuntimeState::memory(MemoryRuntimeStateConfig::default());
    let old = state
        .lock_try_acquire("collector", "old", Duration::from_millis(1))
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let current = state
        .lock_try_acquire("collector", "new", Duration::from_secs(10))
        .await
        .unwrap()
        .unwrap();
    assert!(state
        .kv_set_if_lock_owned(&current, "active", "new", Duration::from_secs(60))
        .await
        .unwrap());
    assert!(!state
        .kv_set_if_lock_owned(&old, "active", "old", Duration::from_secs(60))
        .await
        .unwrap());
    assert_eq!(
        state.kv_get("active").await.unwrap().as_deref(),
        Some("new")
    );
}

#[tokio::test]
async fn fenced_kv_late_invalidation_cannot_delete_replacement() {
    let state = RuntimeState::memory(MemoryRuntimeStateConfig::default());
    state
        .kv_set("active", "new", Some(Duration::from_secs(60)))
        .await
        .unwrap();
    assert!(!state.kv_delete_if_value("active", "old").await.unwrap());
    assert_eq!(
        state.kv_get("active").await.unwrap().as_deref(),
        Some("new")
    );
    assert!(state.kv_delete_if_value("active", "new").await.unwrap());
    assert_eq!(state.kv_get("active").await.unwrap(), None);
}

#[tokio::test]
async fn fenced_kv_released_lease_cannot_publish() {
    let state = RuntimeState::memory(MemoryRuntimeStateConfig::default());
    let lease = state
        .lock_try_acquire("collector", "owner", Duration::from_secs(10))
        .await
        .unwrap()
        .unwrap();
    assert!(state.lock_release(&lease).await.unwrap());
    assert!(!state
        .kv_set_if_lock_owned(&lease, "active", "secret", Duration::from_secs(60))
        .await
        .unwrap());
    assert!(state.kv_get("active").await.unwrap().is_none());
}

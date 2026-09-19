//! 租约所有者发布与按版本删除，在共享后端中保持原子性。
use std::time::Duration;

use crate::error::RedisResultExt;
use crate::{
    run_redis_with_timeout, DataLayerError, RuntimeLockLease, RuntimeState, RuntimeStateBackend,
};

impl RuntimeState {
    /// 只有仍持有有效租约的采集者才能发布，过期工作者不能覆盖新值。
    pub async fn kv_set_if_lock_owned(
        &self,
        lease: &RuntimeLockLease,
        key: &str,
        value: &str,
        ttl: Duration,
    ) -> Result<bool, DataLayerError> {
        if key.is_empty() || ttl.is_zero() {
            return Err(DataLayerError::InvalidInput(
                "发布键和有效期不能为空".into(),
            ));
        }
        match self.backend.as_ref() {
            RuntimeStateBackend::Memory(memory) => Ok(memory.kv_set_if_lock_owned(lease, key, value, ttl).await),
            RuntimeStateBackend::Redis(redis) => {
                run_redis_with_timeout(redis.command_timeout_ms, "fenced kv publish", async {
                    let mut connection = redis.client.get_multiplexed_async_connection().await.map_redis_err()?;
                    let result: i64 = redis::Script::new(
                        "if redis.call('GET', KEYS[1]) ~= ARGV[1] then return 0 end redis.call('SET', KEYS[2], ARGV[2], 'PX', ARGV[3]); return 1"
                    ).key(redis.keyspace.lock_key(&lease.key).0)
                        .key(redis.keyspace.key(key)).arg(&lease.token).arg(value)
                        .arg(ttl.as_millis().max(1).min(u64::MAX as u128) as u64)
                        .invoke_async(&mut connection).await.map_redis_err()?;
                    Ok(result == 1)
                }).await
            }
        }
    }

    /// 迟到的旧响应只可删除它实际使用过的版本。
    pub async fn kv_delete_if_value(
        &self,
        key: &str,
        expected: &str,
    ) -> Result<bool, DataLayerError> {
        match self.backend.as_ref() {
            RuntimeStateBackend::Memory(memory) => Ok(memory.kv_delete_if_value(key, expected).await),
            RuntimeStateBackend::Redis(redis) => {
                run_redis_with_timeout(redis.command_timeout_ms, "conditional kv delete", async {
                    let mut connection = redis.client.get_multiplexed_async_connection().await.map_redis_err()?;
                    let result: i64 = redis::Script::new(
                        "if redis.call('GET', KEYS[1]) ~= ARGV[1] then return 0 end return redis.call('DEL', KEYS[1])"
                    ).key(redis.keyspace.key(key)).arg(expected)
                        .invoke_async(&mut connection).await.map_redis_err()?;
                    Ok(result == 1)
                }).await
            }
        }
    }
}

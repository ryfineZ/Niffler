mod keys;
mod leases;
mod mutations;
mod reads;
mod status;
mod writes;

pub(crate) use self::leases::release_admin_provider_pool_key_lease;
pub(crate) use self::mutations::{
    clear_admin_provider_pool_cooldown, reset_admin_provider_pool_cost,
};
pub(crate) use self::reads::{
    read_admin_provider_pool_cooldown_count, read_admin_provider_pool_cooldown_counts,
    read_admin_provider_pool_cooldown_key_ids, read_admin_provider_pool_key_cooldown_reason,
    read_admin_provider_pool_key_model_cooldown_reason, read_admin_provider_pool_runtime_state,
};
pub(crate) use self::status::build_admin_provider_pool_status_payload;
pub(crate) use self::writes::{
    admin_provider_pool_key_hard_error_reason, record_admin_provider_pool_error,
    record_admin_provider_pool_model_cooldown, record_admin_provider_pool_stream_timeout,
    record_admin_provider_pool_success,
};

pub(crate) async fn read_capacity_model_cooldown_seconds(
    runtime: &aether_runtime_state::RuntimeState,
    provider: &str,
    key: &str,
    model: &str,
) -> Result<u64, crate::GatewayError> {
    runtime
        .kv_ttl_seconds(&keys::pool_model_cooldown_key(provider, key, model))
        .await
        .map(|ttl| ttl.unwrap_or(0).max(0) as u64)
        .map_err(|error| {
            crate::GatewayError::Internal(format!("capacity cooldown read failed: {error}"))
        })
}

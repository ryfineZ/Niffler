use super::*;

fn production_source(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or(source)
}

#[test]
fn handlers_do_not_inline_sql_queries() {
    assert_no_sqlx_queries("src/handlers");
}

#[test]
fn gateway_runtime_does_not_inline_sql_queries() {
    assert_no_sqlx_queries("src/state/runtime");
}

#[test]
fn aether_data_bootstrap_snapshot_is_built_from_schema_sources() {
    let build_rs = read_workspace_file("crates/aether-data/build.rs");
    assert!(
        build_rs.contains("schema/bootstrap/postgres/manifest.txt"),
        "build.rs should source the bootstrap snapshot from schema/bootstrap/postgres"
    );

    let compose_schema = read_workspace_file("crates/aether-data/schema/compose_schema.sh");
    assert!(
        compose_schema.contains("check_bootstrap_sources"),
        "compose_schema.sh should still validate bootstrap source fragments"
    );
    assert!(
        !compose_schema.contains("bootstrap/postgres/20260413020000_empty_database_snapshot.sql"),
        "compose_schema.sh should not depend on the outer bootstrap artifact anymore"
    );

    let bootstrap = read_workspace_file("crates/aether-data/src/lifecycle/bootstrap/postgres.rs");
    assert!(
        bootstrap.contains("include_str!(concat!(env!(\"OUT_DIR\"), \"/empty_database_snapshot.sql\"))"),
        "lifecycle/bootstrap/postgres.rs should embed the generated bootstrap snapshot from OUT_DIR"
    );
    assert!(
        !bootstrap
            .contains("../../../bootstrap/postgres/20260413020000_empty_database_snapshot.sql"),
        "lifecycle/bootstrap/postgres.rs should not read the outer bootstrap artifact directly"
    );

    let provider_catalog =
        read_workspace_file("crates/aether-data/src/repository/provider_catalog/postgres.rs");
    assert!(
        !provider_catalog.contains("../../../bootstrap/postgres/20260413020000_empty_database_snapshot.sql"),
        "provider_catalog tests should use the shared bootstrap snapshot constant instead of the outer bootstrap artifact"
    );
}

#[test]
fn aether_data_backend_pool_modules_do_not_own_maintenance_sql() {
    for path in ["crates/aether-data/src/backend/postgres.rs"] {
        let source = read_workspace_file(path);
        let production = production_source(&source);
        for forbidden in [
            "run_table_maintenance(",
            "aggregate_wallet_daily_usage(",
            "aggregate_stats_hourly(",
            "aggregate_stats_daily(",
            "find_system_config_value(",
            "list_system_config_entries(",
            "upsert_system_config_entry(",
            "read_admin_system_stats(",
            "sqlx::query(",
            "sqlx::query_scalar",
            "sqlx::raw_sql(",
        ] {
            assert!(
                !production.contains(forbidden),
                "{path} should stay focused on pool and repository construction instead of owning maintenance SQL via {forbidden}"
            );
        }
    }

    let maintenance = read_workspace_file("crates/aether-data/src/backend/maintenance.rs");
    for pattern in [
        "self.postgres().run_table_maintenance(table_names).await",
        "self.postgres().aggregate_wallet_daily_usage(input).await",
        "self.postgres().aggregate_stats_hourly(input).await",
        "self.postgres().aggregate_stats_daily(input).await",
    ] {
        assert!(
            maintenance.contains(pattern),
            "backend/maintenance.rs should own SQL-driver maintenance dispatch {pattern}"
        );
    }
}

#[test]
fn referral_postgres_queries_cast_numeric_amount_columns_before_f64_decoding() {
    let source = read_workspace_file("apps/aether-gateway/src/data/state/referrals.rs");
    let production = production_source(&source);

    for required in [
        "SELECT id, user_id, CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd, payment_method, status, order_kind",
        "CAST(refunded_amount_usd AS DOUBLE PRECISION) AS refunded_amount_usd",
        "CAST(rw.amount_usd AS DOUBLE PRECISION) AS amount_usd",
        "CAST(balance AS DOUBLE PRECISION) AS balance",
        "CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance",
        "CAST(reversed_amount_usd AS DOUBLE PRECISION) AS reversed_amount_usd",
        "CAST(pending_reversal_amount_usd AS DOUBLE PRECISION) AS pending_reversal_amount_usd",
    ] {
        assert!(
            production.contains(required),
            "PostgreSQL referral SQL should cast numeric values before f64 decoding: {required}"
        );
    }
}

#[test]
fn usage_cost_statistics_use_settled_billing_instead_of_success_state() {
    let daily_stats_sql =
        read_workspace_file("crates/aether-data/src/backend/stats/postgres_daily/sql.rs");
    let usage_postgres =
        read_workspace_file("crates/aether-data/src/repository/usage/postgres/mod.rs");

    for source in [&daily_stats_sql, &usage_postgres] {
        assert!(
            source.contains("WHEN billing_status = 'settled'"),
            "usage cost statistics should key user charges off settled billing status"
        );
        assert!(
            !source.contains("CASE WHEN success_flag = 1 THEN total_cost_usd ELSE 0 END"),
            "usage cost statistics must not drop settled charges only because the request was not technically successful"
        );
        assert!(
            !source.contains("CASE WHEN success_flag = 1 THEN actual_total_cost_usd ELSE 0 END"),
            "actual cost statistics must not be tied to technical success in usage cost aggregation"
        );
        assert!(
            !source.contains("CASE WHEN billing_status = 'settled' THEN actual_total_cost_usd ELSE 0 END"),
            "actual cost statistics must keep upstream cost even when the user charge is not settled"
        );
    }

    assert!(
        !daily_stats_sql.contains(
            "WHEN status IN ('completed', 'success', 'ok', 'billed', 'settled')\n                 AND (status_code IS NULL OR status_code < 400)\n                 AND (error_message IS NULL OR BTRIM(error_message) = '')\n            THEN COALESCE(CAST(total_cost_usd AS DOUBLE PRECISION), 0)"
        ),
        "daily stats should not calculate user charges from technical success state"
    );
    assert!(
        !daily_stats_sql.contains(
            "WHEN billing_status = 'settled'\n            THEN COALESCE(CAST(actual_total_cost_usd AS DOUBLE PRECISION), 0)"
        ),
        "daily stats should not hide upstream cost only because the user charge is not settled"
    );
}

#[test]
fn testkit_does_not_copy_aether_business_schema_sql() {
    let owner_relay_baseline =
        read_workspace_file("crates/aether-testkit/src/bin/multi_instance_owner_relay_baseline.rs");
    for forbidden in [
        "CREATE TYPE proxynodestatus",
        "CREATE TABLE IF NOT EXISTS system_configs",
        "CREATE TABLE IF NOT EXISTS proxy_nodes",
        "CREATE TABLE IF NOT EXISTS proxy_node_events",
        "PgConnection::connect",
        "sqlx::{Connection, Executor, PgConnection}",
    ] {
        assert!(
            !owner_relay_baseline.contains(forbidden),
            "owner relay baseline should use aether-data schema bootstrap instead of copying business schema SQL via {forbidden}"
        );
    }
    assert!(
        owner_relay_baseline.contains("prepare_aether_postgres_schema(&postgres_url).await?"),
        "owner relay baseline should prepare business schema through aether-testkit's aether-data helper"
    );

    let postgres_testkit = read_workspace_file("crates/aether-testkit/src/postgres.rs");
    for required in [
        "pub async fn prepare_aether_postgres_schema",
        "DataBackends::from_config",
        ".prepare_database_for_startup()",
        ".run_database_migrations()",
    ] {
        assert!(
            postgres_testkit.contains(required),
            "testkit Postgres helper should delegate Aether schema setup to aether-data via {required}"
        );
    }
}

#[test]
fn gateway_main_keeps_database_export_import_driver_selection_in_data_layer() {
    let main_rs = read_workspace_file("apps/aether-gateway/src/main.rs");
    for forbidden in ["PostgresPoolFactory", "to_postgres_config()"] {
        assert!(
            !main_rs.contains(forbidden),
            "main.rs should delegate database export/import driver selection to aether-data instead of {forbidden}"
        );
    }
    for required in ["export_database_jsonl", "import_database_jsonl"] {
        assert!(
            main_rs.contains(required),
            "main.rs should use aether-data {required}"
        );
    }
}

#[test]
fn wallet_repository_does_not_reexport_settlement_types() {
    let wallet_mod = read_workspace_file("crates/aether-data/src/repository/wallet/mod.rs");
    let wallet_types = read_workspace_file("crates/aether-data/src/repository/wallet/types.rs");
    let wallet_sql = read_workspace_file("crates/aether-data/src/repository/wallet/postgres.rs");
    let wallet_memory = read_workspace_file("crates/aether-data/src/repository/wallet/memory.rs");

    assert!(
        !wallet_mod.contains("StoredUsageSettlement"),
        "wallet/mod.rs should not export StoredUsageSettlement"
    );
    assert!(
        !wallet_mod.contains("UsageSettlementInput"),
        "wallet/mod.rs should not export UsageSettlementInput"
    );
    assert!(
        !wallet_types.contains("pub use crate::repository::settlement"),
        "wallet/types.rs should not re-export settlement types"
    );
    assert!(
        !wallet_types.contains("async fn settle_usage("),
        "wallet/types.rs should not own settlement entrypoints"
    );
    assert!(
        !wallet_sql.contains("impl SettlementWriteRepository"),
        "wallet/postgres.rs should not implement SettlementWriteRepository"
    );
    assert!(
        !wallet_memory.contains("impl SettlementWriteRepository"),
        "wallet/memory.rs should not implement SettlementWriteRepository"
    );
}

#[test]
fn gateway_system_config_types_are_owned_by_aether_data() {
    let state_mod = read_workspace_file("apps/aether-gateway/src/data/state/mod.rs");
    assert!(
        state_mod.contains("aether_data::repository::system"),
        "data/state/mod.rs should depend on aether-data system types"
    );
    assert!(
        !state_mod.contains("pub(crate) struct StoredSystemConfigEntry"),
        "data/state/mod.rs should not define StoredSystemConfigEntry locally"
    );

    let state_core = read_workspace_file("apps/aether-gateway/src/data/state/core.rs");
    for pattern in [
        "backends.list_system_config_entries().await",
        ".upsert_system_config_entry(key, value, description)",
        "backends.read_admin_system_stats().await",
        "AdminSystemStats::default()",
    ] {
        assert!(
            state_core.contains(pattern),
            "data/state/core.rs should use shared system DTO path {pattern}"
        );
    }
    let data_backends = read_workspace_file("crates/aether-data/src/backend/maintenance.rs");
    let pattern = "self.postgres().list_system_config_entries().await";
    assert!(
        data_backends.contains(pattern),
        "aether-data backends should own driver-specific system config dispatch {pattern}"
    );
    for pattern in [
        "|(key, value, description, updated_at_unix_secs)|",
        "Ok((0, 0, 0, 0))",
    ] {
        assert!(
            !state_core.contains(pattern),
            "data/state/core.rs should not own local system DTO projection {pattern}"
        );
    }

    let system_types = read_workspace_file("crates/aether-data/src/repository/system.rs");
    for pattern in [
        "pub struct StoredSystemConfigEntry",
        "pub struct AdminSystemStats",
        "pub struct AdminSecurityBlacklistEntry",
    ] {
        assert!(
            system_types.contains(pattern),
            "aether-data system module should own {pattern}"
        );
    }

    let admin_types = read_workspace_file("apps/aether-gateway/src/state/admin_types.rs");
    assert!(
        admin_types.contains("aether_data::repository::system::AdminSecurityBlacklistEntry"),
        "state/admin_types.rs should re-export AdminSecurityBlacklistEntry from aether-data"
    );
    assert!(
        !admin_types.contains("struct AdminSecurityBlacklistEntry"),
        "state/admin_types.rs should not define AdminSecurityBlacklistEntry locally"
    );

    let runtime_mod = read_workspace_file("apps/aether-gateway/src/state/runtime/mod.rs");
    assert!(
        !runtime_mod.contains("AdminSecurityBlacklistEntryPayload"),
        "state/runtime/mod.rs should not keep the unused blacklist payload wrapper"
    );
}

#[test]
fn gateway_auth_snapshot_type_is_owned_by_aether_data() {
    let gateway_auth = read_workspace_file("apps/aether-gateway/src/data/auth.rs");
    let runtime_mod = read_workspace_file("apps/aether-gateway/src/state/runtime/mod.rs");
    let auth_api_keys =
        read_workspace_file("apps/aether-gateway/src/state/runtime/auth/api_keys.rs");
    assert!(
        gateway_auth.contains("aether_data::repository::auth"),
        "data/auth.rs should depend on aether-data auth snapshot types"
    );
    assert!(
        gateway_auth.contains("ResolvedAuthApiKeySnapshot as GatewayAuthApiKeySnapshot"),
        "data/auth.rs should expose the shared resolved auth snapshot type under the gateway-facing name"
    );
    for pattern in [
        "pub(crate) struct GatewayAuthApiKeySnapshot",
        "pub(crate) async fn read_auth_api_key_snapshot(",
        "pub(crate) async fn read_auth_api_key_snapshot_by_key_hash(",
        "fn effective_allowed_providers(",
        "fn effective_allowed_api_formats(",
        "fn effective_allowed_models(",
    ] {
        assert!(
            !gateway_auth.contains(pattern),
            "data/auth.rs should not own local auth snapshot logic {pattern}"
        );
    }
    for pattern in [
        "pub(crate) async fn read_auth_api_key_snapshot(",
        "pub(crate) async fn read_auth_api_key_snapshots_by_ids(",
    ] {
        assert!(
            !auth_api_keys.contains(pattern),
            "state/runtime/auth/api_keys.rs should not keep auth snapshot read wrapper {pattern}"
        );
    }
    assert!(
        !runtime_mod.contains("mod audit;"),
        "state/runtime/mod.rs should not keep the obsolete audit runtime module"
    );
    assert!(
        auth_api_keys.contains("touch_auth_api_key_last_used_best_effort"),
        "state/runtime/auth/api_keys.rs should own auth api key last_used touch helper"
    );
    assert!(
        !auth_api_keys.contains("fn has_auth_api_key_writer("),
        "state/runtime/auth/api_keys.rs should not keep auth api key writer passthrough"
    );

    let auth_types = read_workspace_file("crates/aether-data/src/repository/auth/types.rs");
    for pattern in [
        "pub struct ResolvedAuthApiKeySnapshot",
        "pub trait ResolvedAuthApiKeySnapshotReader",
        "pub async fn read_resolved_auth_api_key_snapshot(",
        "pub async fn read_resolved_auth_api_key_snapshot_by_key_hash(",
        "pub async fn read_resolved_auth_api_key_snapshot_by_user_api_key_ids(",
        "pub fn effective_allowed_providers(&self)",
        "pub fn effective_allowed_api_formats(&self)",
        "pub fn effective_allowed_models(&self)",
    ] {
        assert!(
            auth_types.contains(pattern),
            "aether-data auth types should own {pattern}"
        );
    }
}

#[test]
fn gateway_auth_data_layer_does_not_keep_ldap_row_wrapper() {
    let gateway_auth_state = read_workspace_file("apps/aether-gateway/src/data/state/auth.rs");
    for pattern in [
        "struct StoredLdapAuthUserRow",
        "fn map_ldap_user_auth_row(",
        "Result<Option<StoredLdapAuthUserRow>, DataLayerError>",
        "existing.user.",
        "map_user_auth_row(row)",
    ] {
        assert!(
            !gateway_auth_state.contains(pattern),
            "data/state/auth.rs should not keep ldap row wrapper {pattern}"
        );
    }

    let user_sql = read_workspace_file("crates/aether-data/src/repository/users/postgres.rs");
    for pattern in [
        "Result<Option<StoredUserAuthRecord>, DataLayerError>",
        "return map_user_auth_row(row).map(Some);",
    ] {
        assert!(
            user_sql.contains(pattern),
            "aether-data user repository should use shared user auth record directly via {pattern}"
        );
    }
}

#[test]
fn gateway_provider_oauth_storage_types_are_owned_by_aether_data() {
    let provider_oauth_storage = read_workspace_file(
        "apps/aether-gateway/src/handlers/admin/provider/oauth/state/storage.rs",
    );
    let request_provider_oauth =
        read_workspace_file("apps/aether-gateway/src/handlers/admin/request/provider/oauth.rs");
    assert!(
        request_provider_oauth.contains("aether_data::repository::provider_oauth"),
        "request/provider/oauth.rs should depend on aether-data provider oauth storage types"
    );
    for pattern in [
        "pub(crate) struct StoredAdminProviderOAuthDeviceSession",
        "pub(crate) struct StoredAdminProviderOAuthState",
        "const KIRO_DEVICE_AUTH_SESSION_PREFIX",
        "fn provider_oauth_device_session_key(",
        "fn build_provider_oauth_batch_task_status_payload(",
        "fn provider_oauth_batch_task_key(",
        "const PROVIDER_OAUTH_BATCH_TASK_TTL_SECS",
        "format!(\"provider_oauth_state:{nonce}\")",
    ] {
        assert!(
            !provider_oauth_storage.contains(pattern),
            "provider_oauth/state/storage.rs should not own local storage helper {pattern}"
        );
    }
    for pattern in [
        "StoredAdminProviderOAuthDeviceSession",
        "StoredAdminProviderOAuthState",
        "provider_oauth_batch_task_storage_key",
        "build_provider_oauth_batch_task_status_payload",
        "PROVIDER_OAUTH_BATCH_TASK_TTL_SECS",
    ] {
        assert!(
            request_provider_oauth.contains(pattern),
            "request/provider/oauth.rs should own aether-data provider oauth storage boundary {pattern}"
        );
    }

    assert!(
        !workspace_file_exists("apps/aether-gateway/src/handlers/admin/provider/oauth/state.rs"),
        "provider_oauth/state.rs should not exist after oauth storage helpers move under state/storage.rs"
    );

    let dispatch_device_authorize = read_workspace_file(
        "apps/aether-gateway/src/handlers/admin/provider/oauth/dispatch/device/authorize.rs",
    );
    assert!(
        dispatch_device_authorize.contains("aether_data::repository::provider_oauth"),
        "provider_oauth/dispatch/device/authorize.rs should use shared provider oauth storage DTOs"
    );
    assert!(
        !workspace_file_exists("apps/aether-gateway/src/handlers/admin/provider/oauth/dispatch/device.rs"),
        "provider_oauth/dispatch/device.rs should be removed once device flows move under dispatch/device/"
    );

    let shared_provider_oauth =
        read_workspace_file("crates/aether-data/src/repository/provider_oauth.rs");
    for pattern in [
        "pub struct StoredAdminProviderOAuthDeviceSession",
        "pub struct StoredAdminProviderOAuthState",
        "pub fn provider_oauth_device_session_storage_key(",
        "pub fn provider_oauth_state_storage_key(",
        "pub fn provider_oauth_batch_task_storage_key(",
        "pub fn build_provider_oauth_batch_task_status_payload(",
        "pub const KIRO_DEVICE_AUTH_SESSION_TTL_BUFFER_SECS: u64 = 60;",
        "pub const PROVIDER_OAUTH_BATCH_TASK_TTL_SECS: u64 = 24 * 60 * 60;",
        "pub const PROVIDER_OAUTH_STATE_TTL_SECS: u64 = 600;",
    ] {
        assert!(
            shared_provider_oauth.contains(pattern),
            "aether-data provider oauth storage module should own {pattern}"
        );
    }
}

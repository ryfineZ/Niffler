use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sqlx::{migrate::AppliedMigration, query, query_scalar, Connection, PgConnection, PgPool, Row};

use super::{
    all_up_migrations, pending_migrations_from_applied, prepare_database_for_startup,
    run_migrations, POSTGRES_MIGRATOR,
};
use crate::lifecycle::bootstrap::postgres::{
    snapshot_migrations as empty_database_snapshot_migrations, EMPTY_DATABASE_SNAPSHOT_SQL,
};

#[derive(Debug)]
struct ManagedPostgresServer {
    child: Option<Child>,
    workdir: PathBuf,
    database_url: String,
}

impl ManagedPostgresServer {
    async fn try_start() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let initdb_bin = std::env::var("AETHER_INITDB_BIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "initdb".to_string());
        let postgres_bin = std::env::var("AETHER_POSTGRES_BIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "postgres".to_string());

        if !command_exists(&initdb_bin) || !command_exists(&postgres_bin) {
            eprintln!(
                    "skipping postgres integration test because required binaries are unavailable: initdb={}, postgres={}",
                    initdb_bin, postgres_bin
                );
            return Ok(None);
        }

        match Self::start(initdb_bin, postgres_bin).await {
            Ok(server) => Ok(Some(server)),
            Err(err) if postgres_local_startup_unavailable(err.to_string().as_str()) => {
                eprintln!(
                        "skipping postgres integration test because local postgres could not start in this environment: {err}"
                    );
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    async fn start(
        initdb_bin: String,
        postgres_bin: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let port = reserve_local_port()?;
        let workdir = std::env::temp_dir().join(format!(
            "aether-migrate-tests-{}-{}",
            std::process::id(),
            port
        ));
        let data_dir = workdir.join("data");
        std::fs::create_dir_all(&workdir)?;

        let init_output = Command::new(&initdb_bin)
            .arg("-D")
            .arg(&data_dir)
            .arg("-U")
            .arg("aether")
            .arg("--auth=trust")
            .arg("--encoding=UTF8")
            .arg("--no-instructions")
            .output()?;
        if !init_output.status.success() {
            return Err(std::io::Error::other(format!(
                "initdb failed: {}",
                String::from_utf8_lossy(&init_output.stderr)
            ))
            .into());
        }

        let database_url = format!("postgres://aether@127.0.0.1:{port}/postgres");
        let log_path = workdir.join("postgres.log");
        let stdout = std::fs::File::create(&log_path)?;
        let stderr = stdout.try_clone()?;
        let mut child = Command::new(&postgres_bin)
            .arg("-D")
            .arg(&data_dir)
            .arg("-h")
            .arg("127.0.0.1")
            .arg("-p")
            .arg(port.to_string())
            .arg("-F")
            .arg("-c")
            .arg("fsync=off")
            .arg("-c")
            .arg("synchronous_commit=off")
            .arg("-c")
            .arg("full_page_writes=off")
            .arg("-c")
            .arg("shared_buffers=8MB")
            .arg("-c")
            .arg("max_connections=8")
            .arg("-c")
            .arg("dynamic_shared_memory_type=mmap")
            .arg("-c")
            .arg("autovacuum=off")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()?;

        if let Err(err) = wait_for_postgres(&database_url).await {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }

        Ok(Self {
            child: Some(child),
            workdir,
            database_url,
        })
    }

    fn database_url(&self) -> &str {
        &self.database_url
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ManagedPostgresServer {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.workdir);
    }
}

fn command_exists(bin: &str) -> bool {
    if bin.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(bin).exists();
    }

    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&paths).any(|path| path.join(bin).exists())
}

fn reserve_local_port() -> Result<u16, std::io::Error> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn postgres_shared_memory_unavailable(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("shared memory")
        && (message.contains("could not create shared memory segment")
            || message.contains("shmget")
            || message.contains("no space left on device"))
}

fn postgres_local_startup_unavailable(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    postgres_shared_memory_unavailable(&message)
        || (message.contains("timed out waiting for local postgres")
            && (message.contains("connection refused")
                || message.contains("os error 61")
                || message.contains("os error 111")))
}

async fn wait_for_postgres(database_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match PgConnection::connect(database_url).await {
            Ok(connection) => {
                connection.close().await?;
                return Ok(());
            }
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            Err(err) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("timed out waiting for local postgres: {err}"),
                )
                .into())
            }
        }
    }
}

async fn table_exists(pool: &PgPool, table_name: &str) -> Result<bool, sqlx::Error> {
    query_scalar::<_, bool>("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("public.{table_name}"))
        .fetch_one(pool)
        .await
}

async fn column_exists(
    pool: &PgPool,
    table_name: &str,
    column_name: &str,
) -> Result<bool, sqlx::Error> {
    query_scalar::<_, bool>(
        r#"
SELECT EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = 'public'
      AND table_name = $1
      AND column_name = $2
)
"#,
    )
    .bind(table_name)
    .bind(column_name)
    .fetch_one(pool)
    .await
}

async fn sync_provider_contributions(
    pool: &PgPool,
    request_ids: &[&str],
) -> Result<(), crate::DataLayerError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(crate::DataLayerError::postgres)?;
    for request_id in request_ids {
        crate::repository::usage::postgres::provider_contribution::
            sync_provider_api_key_usage_contribution_for_request_in_tx(&mut tx, request_id)
            .await?;
    }
    tx.commit().await.map_err(crate::DataLayerError::postgres)
}

#[test]
fn baseline_migration_restores_search_path_for_sqlx_bookkeeping() {
    let baseline = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260403000000)
        .expect("baseline migration should be embedded");
    let first_empty_search_path = baseline
        .sql
        .find("SELECT pg_catalog.set_config('search_path', '', true);")
        .expect("baseline migration should clear search_path transaction-local");
    let restore_public_search_path = baseline
        .sql
        .rfind("SELECT pg_catalog.set_config('search_path', 'public', true);")
        .expect("baseline migration should restore search_path before sqlx bookkeeping");

    assert!(
        first_empty_search_path < restore_public_search_path,
        "baseline migration must restore search_path after clearing it",
    );
    assert!(
        !baseline
            .sql
            .contains("SELECT pg_catalog.set_config('search_path', '', false);"),
        "baseline migration must not persist an empty search_path at session scope",
    );
    assert!(
        !baseline
            .sql
            .contains("SELECT pg_catalog.set_config('search_path', 'public', false);"),
        "baseline migration must not persist a restored search_path at session scope",
    );
}

#[test]
fn empty_database_snapshot_covers_current_cutoff_versions() {
    let versions = empty_database_snapshot_migrations(&POSTGRES_MIGRATOR)
        .expect("empty database snapshot migrations should resolve")
        .into_iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();

    assert_eq!(
        versions,
        vec![
            20260403000000,
            20260406000000,
            20260410000000,
            20260413020000,
            20260413030000,
            20260415000000,
            20260418000000,
            20260421000000,
            20260422110000,
            20260422120000,
            20260423000000,
            20260424000000,
            20260428000000,
            20260502000000,
            20260505000000,
            20260505130000,
            20260507000000,
            20260507120000,
            20260508000000,
            20260509000000,
            20260509120000,
            20260510000000,
            20260510120000,
            20260511000000,
            20260511120000,
            20260511130000,
            20260512000000,
            20260512090000,
            20260512110000,
            20260515000000,
            20260516000000,
            20260518000000,
            20260519000000,
            20260519120000,
            20260519130000,
            20260527120000,
            20260528120000,
            20260530120000,
            20260531120000,
            20260601120000,
            20260606120000,
        ]
    );
}

#[test]
fn empty_database_snapshot_sql_includes_usage_body_blobs_and_audit_admin_role() {
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("'audit_admin'"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.usage_body_objects"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("ix_usage_body_objects_request_id"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("CREATE TABLE IF NOT EXISTS public.usage_body_blobs")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("ix_usage_body_blobs_request_id"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("CREATE TABLE IF NOT EXISTS public.usage_http_audits")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("request_body_state character varying(32)"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("provider_request_body_state character varying(32)")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("response_body_state character varying(32)"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("client_response_body_state character varying(32)")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.niffler_upstream_services"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.niffler_api_key_pauses"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.usage_routing_snapshots"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.usage_settlement_snapshots"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("billing_snapshot_schema_version"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("price_per_request"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("settlement_snapshot_schema_version"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("billing_effective_input_tokens"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("CREATE OR REPLACE VIEW public.usage_billing_facts")
    );
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("usage_settlement_snapshots.billing_total_cost_usd")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("candidate_index integer"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.stats_user_summary"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.stats_user_daily_model"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.stats_hourly_user_model"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("CREATE TABLE IF NOT EXISTS public.schema_backfills")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("idx_schema_backfills_applied_at"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("ALTER TABLE public.stats_hourly"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("response_time_sum_ms double precision"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("CREATE TABLE IF NOT EXISTS public.api_keys"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("total_tokens bigint DEFAULT '0'::bigint NOT NULL")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.stats_user_daily_provider"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.stats_user_daily_api_format"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.stats_daily_cost_savings_model_provider"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains(
        "CREATE TABLE IF NOT EXISTS public.stats_user_daily_cost_savings_model_provider"
    ));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("CREATE TABLE IF NOT EXISTS public.routing_groups")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.routing_group_bindings"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.routing_group_versions"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("routing_groups_system_default_idx"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("successful_response_time_sum_ms double precision")
    );
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("cache_hit_total_requests bigint DEFAULT 0 NOT NULL")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains(
            "ALTER TABLE public.stats_daily_model\n    ADD COLUMN IF NOT EXISTS cache_creation_ephemeral_5m_tokens bigint DEFAULT '0'::bigint NOT NULL,"
        ));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.usage_counter_deltas"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("ix_usage_counter_deltas_ready"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("idx_entitlement_usage_entitlement_date"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("idx_provider_api_keys_provider_default_sort"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("idx_video_tasks_due_poll"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("request_count bigint DEFAULT 0"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("usage_count bigint DEFAULT 0 NOT NULL"));
}

#[test]
fn empty_database_snapshot_sql_includes_payment_gateway_and_plans() {
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("payment_provider character varying(64)"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.payment_gateway_configs"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("CREATE TABLE IF NOT EXISTS public.billing_plans"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("purchase_limit_scope character varying(32) DEFAULT 'active_period'"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.user_plan_entitlements"));
}

#[test]
fn provider_api_keys_api_formats_remains_nullable_in_baselines() {
    let baseline_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260403000000)
        .expect("baseline migration should be embedded");

    assert!(baseline_migration.sql.contains("api_formats json,"));
    assert!(!baseline_migration
        .sql
        .contains("api_formats json DEFAULT '[]'::json NOT NULL"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("api_formats json,"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("concurrent_limit integer,"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("concurrent_limit_mode text DEFAULT 'inherit'"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("allow_auth_channel_mismatch_formats json,"));
    assert!(!EMPTY_DATABASE_SNAPSHOT_SQL.contains("api_formats json DEFAULT '[]'::json NOT NULL"));

    let auth_mismatch_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260502000000)
        .expect("auth mismatch migration should be embedded");
    assert!(auth_mismatch_migration
        .sql
        .contains("allow_auth_channel_mismatch_formats = rebuilt.api_formats"));
    assert!(auth_mismatch_migration
        .sql
        .contains("pak.allow_auth_channel_mismatch_formats IS NULL"));
}

#[test]
fn management_tokens_json_columns_are_normalized_to_jsonb_in_postgres_schema_paths() {
    let normalization_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260510000000)
        .expect("management token jsonb normalization migration should be embedded");
    assert!(normalization_migration
        .sql
        .contains("ALTER COLUMN allowed_ips TYPE jsonb USING allowed_ips::jsonb"));
    assert!(normalization_migration
        .sql
        .contains("ALTER COLUMN permissions TYPE jsonb USING permissions::jsonb"));
    assert!(normalization_migration
        .sql
        .contains("jsonb_array_length(allowed_ips) > 0"));

    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("allowed_ips jsonb,"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("permissions jsonb,"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("jsonb_array_length(allowed_ips)"));

    let bootstrap_schema =
        include_str!("../../../schema/bootstrap/postgres/001_types_and_tables.sql");
    assert!(bootstrap_schema.contains("allowed_ips jsonb,"));
    assert!(bootstrap_schema.contains("permissions jsonb,"));
    assert!(bootstrap_schema.contains("jsonb_array_length(allowed_ips)"));

    let driver_schema =
        include_str!("../../../schema/drivers/postgres/baseline/001_types_and_tables.sql");
    assert!(driver_schema.contains("allowed_ips jsonb,"));
    assert!(driver_schema.contains("permissions jsonb,"));
    assert!(driver_schema.contains("jsonb_array_length(allowed_ips)"));

    let generated_identity =
        include_str!("../../../schema/generated/postgres/baseline/001_identity.sql");
    assert!(generated_identity.contains("allowed_ips jsonb,"));
    assert!(generated_identity.contains("permissions jsonb,"));
}

#[test]
fn provider_api_keys_api_key_is_nullable() {
    let baseline_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260403000000)
        .expect("baseline migration should be embedded");
    let normalization_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260428000000)
        .expect("api format normalization migration should be embedded");

    assert!(baseline_migration.sql.contains("api_key text,"));
    assert!(!baseline_migration.sql.contains("api_key text NOT NULL"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("api_key text,"));
    assert!(!EMPTY_DATABASE_SNAPSHOT_SQL.contains("api_key text NOT NULL"));
    assert!(normalization_migration
        .sql
        .contains("ALTER COLUMN api_key DROP NOT NULL"));
}

#[test]
fn provider_api_key_window_usage_migration_defines_required_tables_and_queue_kind() {
    let migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260723120000)
        .expect("provider key window usage migration should be embedded");

    assert!(migration
        .sql
        .contains("provider_api_key_window_usage_counters"));
    assert!(migration
        .sql
        .contains("provider_api_key_window_usage_applications"));
    assert!(migration
        .sql
        .contains("provider_api_key_window_usage_resets"));
    assert!(migration.sql.contains("'provider_api_key_window'"));
    assert!(migration.sql.contains("available_at"));
    assert!(migration.sql.contains("ON DELETE CASCADE"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("provider_api_key_window_usage_counters"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("provider_api_key_window_usage_applications"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("provider_api_key_window_usage_resets"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("'provider_api_key_window'"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("available_at"));
}

#[test]
fn provider_api_key_usage_contribution_migration_defines_fact_and_revision_table() {
    let migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260801190000)
        .expect("provider usage contribution migration should be embedded");

    assert!(migration
        .sql
        .contains("provider_api_key_usage_contributions"));
    assert!(migration.sql.contains("revision bigint NOT NULL DEFAULT 0"));
    assert!(migration
        .sql
        .contains("provider_api_key_usage_contribution_backfill_state"));
    assert!(migration
        .sql
        .contains("provider_api_key_usage_contribution_backfills"));
    assert!(migration
        .sql
        .contains("provider_api_key_usage_projection_repairs"));
    assert!(migration
        .sql
        .contains("ON CONFLICT (provider_api_key_id) DO NOTHING"));
    assert!(!migration.sql.contains("LOCK TABLE public.\"usage\""));
    assert!(!migration.sql.contains("FROM public.usage_billing_facts"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("CREATE TABLE IF NOT EXISTS public.provider_api_key_usage_contributions"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("ix_provider_api_key_usage_contributions_key"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("provider_api_key_usage_contributions_key_fkey"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("provider_api_key_usage_contribution_backfill_state")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("provider_api_key_usage_projection_repairs"));
}

#[test]
fn provider_api_key_window_usage_index_migrations_are_concurrent_and_single_statement() {
    let create_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260723121000)
        .expect("provider key window ready-index migration should be embedded");
    let drop_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260723122000)
        .expect("provider key window old-index cleanup migration should be embedded");

    assert!(create_migration.no_tx);
    assert!(create_migration
        .sql
        .contains("CREATE INDEX CONCURRENTLY IF NOT EXISTS ix_usage_counter_deltas_ready"));
    assert!(!create_migration.sql.contains("DROP INDEX CONCURRENTLY"));
    assert!(drop_migration.no_tx);
    assert!(drop_migration
        .sql
        .contains("DROP INDEX CONCURRENTLY IF EXISTS public.ix_usage_counter_deltas_unprocessed"));
    assert!(!drop_migration.sql.contains("CREATE INDEX CONCURRENTLY"));
}

#[tokio::test]
async fn provider_api_key_window_usage_migration_runs_on_existing_schema() {
    let Some(server) = ManagedPostgresServer::try_start()
        .await
        .expect("postgres migration test should start or skip")
    else {
        return;
    };
    let pool = PgPool::connect(server.database_url())
        .await
        .expect("pool should connect");
    let migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260723120000)
        .expect("provider key window usage migration should be embedded");

    sqlx::raw_sql(
        r#"
CREATE TABLE public.provider_api_keys (
  id text PRIMARY KEY
);
CREATE TABLE public.usage_counter_deltas (
  id character varying(36) PRIMARY KEY,
  kind character varying(64) NOT NULL,
  created_at timestamp with time zone DEFAULT now() NOT NULL,
  processed_at timestamp with time zone,
  CONSTRAINT usage_counter_deltas_kind_check CHECK (
    kind IN ('api_key', 'provider_api_key')
  )
);
"#,
    )
    .execute(&pool)
    .await
    .expect("pre-migration schema should build");

    sqlx::raw_sql(migration.sql.as_ref())
        .execute(&pool)
        .await
        .expect("provider key window usage migration should run");

    assert!(
        table_exists(&pool, "provider_api_key_window_usage_counters")
            .await
            .expect("counter table lookup should succeed")
    );
    assert!(
        table_exists(&pool, "provider_api_key_window_usage_applications")
            .await
            .expect("application table lookup should succeed")
    );
    assert!(table_exists(&pool, "provider_api_key_window_usage_resets")
        .await
        .expect("reset table lookup should succeed"));
    query(
        "INSERT INTO public.usage_counter_deltas (id, kind) VALUES ('delta-1', 'provider_api_key_window')",
    )
    .execute(&pool)
    .await
    .expect("new queue kind should satisfy migrated constraint");
}

#[tokio::test]
async fn provider_api_key_window_usage_sql_handles_refresh_and_zero_cost_rebuild() {
    let Some(server) = ManagedPostgresServer::try_start()
        .await
        .expect("postgres window usage test should start or skip")
    else {
        return;
    };
    let pool = PgPool::connect(server.database_url())
        .await
        .expect("pool should connect");
    run_migrations(&pool)
        .await
        .expect("clean database migrations should succeed");

    query(
        r#"
INSERT INTO public.providers (id, name, provider_type)
VALUES ('provider-window-test', 'Window test', 'codex')
"#,
    )
    .execute(&pool)
    .await
    .expect("provider should insert");
    query(
        r#"
INSERT INTO public.provider_api_keys (
  id,
  name,
  provider_id,
  total_tokens,
  total_cost_usd,
  status_snapshot
) VALUES (
  'provider-key-window-test',
  'Window key',
  'provider-window-test',
  0,
  0,
  $1
)
"#,
    )
    .bind(serde_json::json!({
        "quota": {
            "windows": [
                {
                    "code": "5h",
                    "scope": "account",
                    "window_seconds": 1_000,
                    "reset_at": 2_000
                },
                {
                    "code": "weekly",
                    "scope": "account",
                    "window_seconds": 3_000,
                    "reset_at": 3_000
                }
            ]
        }
    }))
    .execute(&pool)
    .await
    .expect("provider key should insert");

    let apply_sql = include_str!(
        "../../repository/usage/postgres/queries/apply_provider_api_key_codex_window_usage_delta_sql.sql"
    );
    let apply_delta = |delta_id: &str, provider_api_key_id: &str, created_at: i64| {
        sqlx::query(apply_sql)
            .bind(vec![delta_id.to_string()])
            .bind(vec![provider_api_key_id.to_string()])
            .bind(vec![1_i64])
            .bind(vec![100_i64])
            .bind(vec![0.25_f64])
            .bind(vec![created_at])
    };

    query(
        r#"
INSERT INTO public.usage_counter_deltas (
  id,
  request_id,
  kind,
  target_id,
  window_request_count_delta,
  window_total_tokens_delta,
  window_total_cost_usd_delta,
  usage_created_at_unix_secs
) VALUES (
  'delta-window-1',
  'request-window-1',
  'provider_api_key_window',
  'provider-key-window-test',
  1,
  100,
  0.25,
  1500
)
"#,
    )
    .execute(&pool)
    .await
    .expect("first window delta should insert");

    let rows = apply_delta("delta-window-1", "provider-key-window-test", 1_500)
        .fetch_all(&pool)
        .await
        .expect("first window delta should apply");
    assert!(rows
        .iter()
        .all(|row| row.get::<bool, _>("ready_to_complete")));

    apply_delta("delta-window-1", "provider-key-window-test", 1_500)
        .fetch_all(&pool)
        .await
        .expect("repeated window delta should remain idempotent");
    let first_counts: Vec<(String, i64)> = query(
        r#"
SELECT window_code, request_count
FROM public.provider_api_key_window_usage_counters
WHERE provider_api_key_id = 'provider-key-window-test'
ORDER BY window_code
"#,
    )
    .fetch_all(&pool)
    .await
    .expect("window counters should read")
    .into_iter()
    .map(|row| (row.get("window_code"), row.get("request_count")))
    .collect();
    assert_eq!(
        first_counts,
        vec![("5h".to_string(), 1), ("weekly".to_string(), 1)]
    );

    query(
        r#"
UPDATE public.provider_api_keys
SET status_snapshot = $1
WHERE id = 'provider-key-window-test'
"#,
    )
    .bind(serde_json::json!({
        "quota": {
            "windows": [
                {
                    "code": "5h",
                    "scope": "account",
                    "window_seconds": 1_000,
                    "reset_at": 1_600
                },
                {
                    "code": "weekly",
                    "scope": "account",
                    "window_seconds": 3_000,
                    "reset_at": 3_000
                }
            ]
        }
    }))
    .execute(&pool)
    .await
    .expect("stale short window should update");
    query(
        r#"
INSERT INTO public.usage_counter_deltas (
  id,
  request_id,
  kind,
  target_id,
  window_request_count_delta,
  window_total_tokens_delta,
  window_total_cost_usd_delta,
  usage_created_at_unix_secs
) VALUES (
  'delta-window-2',
  'request-window-2',
  'provider_api_key_window',
  'provider-key-window-test',
  1,
  100,
  0.25,
  1700
)
"#,
    )
    .execute(&pool)
    .await
    .expect("second window delta should insert");
    let rows = apply_delta("delta-window-2", "provider-key-window-test", 1_700)
        .fetch_all(&pool)
        .await
        .expect("second window delta should partially apply");
    assert!(rows
        .iter()
        .all(|row| !row.get::<bool, _>("ready_to_complete")));

    query(
        r#"
UPDATE public.provider_api_keys
SET status_snapshot = $1
WHERE id = 'provider-key-window-test'
"#,
    )
    .bind(serde_json::json!({
        "quota": {
            "windows": [
                {
                    "code": "5h",
                    "scope": "account",
                    "window_seconds": 1_000,
                    "reset_at": 2_600
                },
                {
                    "code": "weekly",
                    "scope": "account",
                    "window_seconds": 3_000,
                    "reset_at": 3_000
                }
            ]
        }
    }))
    .execute(&pool)
    .await
    .expect("refreshed short window should update");
    let rows = apply_delta("delta-window-2", "provider-key-window-test", 1_700)
        .fetch_all(&pool)
        .await
        .expect("second window delta should finish after refresh");
    assert!(rows
        .iter()
        .all(|row| row.get::<bool, _>("ready_to_complete")));

    let weekly_request_count: i64 = query_scalar(
        r#"
SELECT request_count
FROM public.provider_api_key_window_usage_counters
WHERE provider_api_key_id = 'provider-key-window-test'
  AND window_code = 'weekly'
  AND window_end_unix_secs = 3000
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("weekly counter should read");
    assert_eq!(weekly_request_count, 2);

    query(
        r#"
UPDATE public.provider_api_keys
SET status_snapshot = '{"quota":{"windows":[]}}'::json
WHERE id = 'provider-key-window-test'
"#,
    )
    .execute(&pool)
    .await
    .expect("empty windows should update");
    let rows = apply_delta("delta-window-2", "provider-key-window-test", 1_700)
        .fetch_all(&pool)
        .await
        .expect("window delta without active windows should complete");
    assert!(rows
        .iter()
        .all(|row| row.get::<bool, _>("ready_to_complete")));

    query(
        r#"
INSERT INTO public.provider_api_keys (
  id,
  name,
  provider_id,
  total_tokens,
  total_cost_usd,
  status_snapshot
) VALUES (
  'provider-key-rebuild-test',
  'Rebuild key',
  'provider-window-test',
  0,
  0,
  '{"quota":{"windows":[{"code":"weekly","scope":"account","window_seconds":1000,"reset_at":2000}]}}'::json
)
"#,
    )
    .execute(&pool)
    .await
    .expect("rebuild provider key should insert");
    query(
        r#"
INSERT INTO public.usage (
  id,
  request_id,
  provider_name,
  model,
  provider_id,
  provider_api_key_id,
  input_tokens,
  output_tokens,
  total_tokens,
  total_cost_usd,
  request_metadata,
  status,
  billing_status,
  created_at
) VALUES
  (
    'usage-zero-cost-window',
    'request-zero-cost-window',
    'Window test',
    'gpt-test',
    'provider-window-test',
    'provider-key-rebuild-test',
    10,
    20,
    30,
    0,
    '{"settlement_snapshot":{"base_cost_usd":0}}'::jsonb,
    'completed',
    'settled',
    to_timestamp(1500)
  ),
  (
    'usage-multiplier-window',
    'request-multiplier-window',
    'Window test',
    'gpt-test',
    'provider-window-test',
    'provider-key-rebuild-test',
    10,
    10,
    20,
    0.5,
    '{"sales_multiplier":0.25}'::jsonb,
    'completed',
    'settled',
    to_timestamp(1600)
  )
"#,
    )
    .execute(&pool)
    .await
    .expect("zero cost usage should insert");
    sync_provider_contributions(
        &pool,
        &["request-zero-cost-window", "request-multiplier-window"],
    )
    .await
    .expect("window rebuild contributions should sync");
    query(include_str!(
        "../../repository/usage/postgres/queries/rebuild_provider_api_key_codex_window_usage_for_key_sql.sql"
    ))
    .bind("provider-key-rebuild-test")
    .execute(&pool)
    .await
    .expect("zero cost window usage should rebuild");

    let rebuilt = query(
        r#"
SELECT request_count, total_tokens, CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd
FROM public.provider_api_key_window_usage_counters
WHERE provider_api_key_id = 'provider-key-rebuild-test'
  AND window_code = 'weekly'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("rebuilt counter should read");
    assert_eq!(rebuilt.get::<i64, _>("request_count"), 2);
    assert_eq!(rebuilt.get::<i64, _>("total_tokens"), 50);
    assert_eq!(rebuilt.get::<f64, _>("total_cost_usd"), 2.0);

    let reset_windows: i64 = sqlx::query_scalar(include_str!(
        "../../repository/usage/postgres/queries/reset_provider_api_key_codex_window_usage_sql.sql"
    ))
    .bind(vec!["provider-key-rebuild-test".to_string()])
    .bind(vec!["account".to_string()])
    .bind(vec![1_000_i64])
    .bind(vec![2_000_i64])
    .bind(1_550_i64)
    .fetch_one(&pool)
    .await
    .expect("window reset boundary should persist");
    assert_eq!(reset_windows, 1);
    query(include_str!(
        "../../repository/usage/postgres/queries/rebuild_provider_api_key_codex_window_usage_for_key_sql.sql"
    ))
    .bind("provider-key-rebuild-test")
    .execute(&pool)
    .await
    .expect("reset window usage should rebuild");

    let reset_rebuilt = query(
        r#"
SELECT request_count, total_tokens, CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd
FROM public.provider_api_key_window_usage_counters
WHERE provider_api_key_id = 'provider-key-rebuild-test'
  AND window_code = 'weekly'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("reset counter should read");
    assert_eq!(reset_rebuilt.get::<i64, _>("request_count"), 1);
    assert_eq!(reset_rebuilt.get::<i64, _>("total_tokens"), 20);
    assert_eq!(reset_rebuilt.get::<f64, _>("total_cost_usd"), 2.0);

    query(
        r#"
UPDATE public.provider_api_keys
SET status_snapshot = '{"quota":{"windows":[]}}'::json
WHERE id = 'provider-key-rebuild-test'
"#,
    )
    .execute(&pool)
    .await
    .expect("rebuild window should disappear");
    query(include_str!(
        "../../repository/usage/postgres/queries/rebuild_provider_api_key_codex_window_usage_for_key_sql.sql"
    ))
    .bind("provider-key-rebuild-test")
    .execute(&pool)
    .await
    .expect("missing window should invalidate counters");

    let counters_after_disappear: i64 = query_scalar(
        "SELECT COUNT(*)::BIGINT FROM provider_api_key_window_usage_counters WHERE provider_api_key_id = 'provider-key-rebuild-test'",
    )
    .fetch_one(&pool)
    .await
    .expect("stale counter count should read");
    assert_eq!(counters_after_disappear, 0);
    let resets_after_disappear: i64 = query_scalar(
        "SELECT COUNT(*)::BIGINT FROM provider_api_key_window_usage_resets WHERE provider_api_key_id = 'provider-key-rebuild-test'",
    )
    .fetch_one(&pool)
    .await
    .expect("reset boundary count should read");
    assert_eq!(resets_after_disappear, 1);

    query(
        r#"
UPDATE public.provider_api_keys
SET status_snapshot = '{"quota":{"windows":[{"code":"7d","scope":"account","window_seconds":1000,"reset_at":2000}]}}'::json
WHERE id = 'provider-key-rebuild-test'
"#,
    )
    .execute(&pool)
    .await
    .expect("renamed window should restore");
    let restored_missing: bool = query_scalar(include_str!(
        "../../repository/usage/postgres/queries/provider_api_key_codex_window_usage_missing_sql.sql"
    ))
    .bind("provider-key-rebuild-test")
    .fetch_one(&pool)
    .await
    .expect("restored window readiness should read");
    assert!(restored_missing);
    query(include_str!(
        "../../repository/usage/postgres/queries/rebuild_provider_api_key_codex_window_usage_for_key_sql.sql"
    ))
    .bind("provider-key-rebuild-test")
    .execute(&pool)
    .await
    .expect("restored window should rebuild");

    let restored = query(
        r#"
SELECT window_code, request_count, total_tokens,
       CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd
FROM public.provider_api_key_window_usage_counters
WHERE provider_api_key_id = 'provider-key-rebuild-test'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("restored counter should read");
    assert_eq!(restored.get::<String, _>("window_code"), "7d");
    assert_eq!(restored.get::<i64, _>("request_count"), 1);
    assert_eq!(restored.get::<i64, _>("total_tokens"), 20);
    assert_eq!(restored.get::<f64, _>("total_cost_usd"), 2.0);

    query(
        r#"
INSERT INTO public.provider_api_keys (
  id,
  name,
  provider_id,
  total_tokens,
  total_cost_usd,
  status_snapshot
) VALUES (
  'provider-key-increment-first',
  'Increment first key',
  'provider-window-test',
  0,
  0,
  '{"quota":{"windows":[{"code":"weekly","scope":"account","window_seconds":1000,"reset_at":2000}]}}'::json
)
"#,
    )
    .execute(&pool)
    .await
    .expect("increment-first provider key should insert");
    query(
        r#"
INSERT INTO public.usage (
  id,
  request_id,
  provider_name,
  model,
  provider_id,
  provider_api_key_id,
  input_tokens,
  output_tokens,
  total_tokens,
  total_cost_usd,
  request_metadata,
  status,
  billing_status,
  created_at
) VALUES
  (
    'usage-increment-first-history',
    'request-increment-first-history',
    'Window test',
    'gpt-test',
    'provider-window-test',
    'provider-key-increment-first',
    10,
    10,
    20,
    0.4,
    '{"base_cost_usd":0.4}'::jsonb,
    'completed',
    'settled',
    to_timestamp(1200)
  ),
  (
    'usage-increment-first-current',
    'request-increment-first-current',
    'Window test',
    'gpt-test',
    'provider-window-test',
    'provider-key-increment-first',
    10,
    10,
    20,
    0.6,
    '{"base_cost_usd":0.6}'::jsonb,
    'completed',
    'settled',
    to_timestamp(1500)
  )
"#,
    )
    .execute(&pool)
    .await
    .expect("increment-first usage should insert");
    sync_provider_contributions(
        &pool,
        &[
            "request-increment-first-history",
            "request-increment-first-current",
        ],
    )
    .await
    .expect("increment-first contributions should sync");
    query(
        r#"
INSERT INTO public.usage_counter_deltas (
  id,
  request_id,
  kind,
  target_id,
  window_request_count_delta,
  window_total_tokens_delta,
  window_total_cost_usd_delta,
  usage_created_at_unix_secs
) VALUES (
  'delta-increment-first',
  'request-increment-first-current',
  'provider_api_key_window',
  'provider-key-increment-first',
  1,
  20,
  0.6,
  1500
)
"#,
    )
    .execute(&pool)
    .await
    .expect("increment-first delta should insert");
    apply_delta(
        "delta-increment-first",
        "provider-key-increment-first",
        1_500,
    )
    .fetch_all(&pool)
    .await
    .expect("increment-first delta should apply");

    let incremental_counter = query(
        r#"
SELECT request_count, rebuilt_at IS NULL AS needs_rebuild
FROM public.provider_api_key_window_usage_counters
WHERE provider_api_key_id = 'provider-key-increment-first'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("incremental counter should read");
    assert_eq!(incremental_counter.get::<i64, _>("request_count"), 1);
    assert!(incremental_counter.get::<bool, _>("needs_rebuild"));

    let increment_first_missing: bool = query_scalar(include_str!(
        "../../repository/usage/postgres/queries/provider_api_key_codex_window_usage_missing_sql.sql"
    ))
    .bind("provider-key-increment-first")
    .fetch_one(&pool)
    .await
    .expect("increment-first readiness should read");
    assert!(increment_first_missing);
    query(include_str!(
        "../../repository/usage/postgres/queries/rebuild_provider_api_key_codex_window_usage_for_key_sql.sql"
    ))
    .bind("provider-key-increment-first")
    .execute(&pool)
    .await
    .expect("increment-first counter should rebuild");

    let increment_first_rebuilt = query(
        r#"
SELECT request_count, total_tokens,
       CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
       rebuilt_at IS NOT NULL AS rebuilt
FROM public.provider_api_key_window_usage_counters
WHERE provider_api_key_id = 'provider-key-increment-first'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("rebuilt increment-first counter should read");
    assert_eq!(increment_first_rebuilt.get::<i64, _>("request_count"), 2);
    assert_eq!(increment_first_rebuilt.get::<i64, _>("total_tokens"), 40);
    assert_eq!(increment_first_rebuilt.get::<f64, _>("total_cost_usd"), 1.0);
    assert!(increment_first_rebuilt.get::<bool, _>("rebuilt"));
}

#[tokio::test]
async fn provider_usage_contribution_transitions_from_pending_to_settled_once() {
    let Some(server) = ManagedPostgresServer::try_start()
        .await
        .expect("postgres provider contribution test should start or skip")
    else {
        return;
    };
    let pool = PgPool::connect(server.database_url())
        .await
        .expect("pool should connect");
    run_migrations(&pool)
        .await
        .expect("clean database migrations should succeed");

    query(
        r#"
INSERT INTO public.providers (id, name, provider_type)
VALUES ('provider-contribution-test', 'Contribution test', 'codex')
"#,
    )
    .execute(&pool)
    .await
    .expect("provider should insert");
    query(
        r#"
INSERT INTO public.provider_api_keys (
  id,
  name,
  provider_id,
  total_tokens,
  total_cost_usd,
  status_snapshot
) VALUES (
  'provider-key-contribution-test',
  'Contribution key',
  'provider-contribution-test',
  0,
  0,
  '{"quota":{"windows":[{"code":"weekly","scope":"account","window_seconds":1000,"reset_at":2000}]}}'::jsonb
)
"#,
    )
    .execute(&pool)
    .await
    .expect("provider key should insert");
    query(
        r#"
INSERT INTO public.usage (
  id,
  request_id,
  provider_name,
  model,
  provider_id,
  provider_api_key_id,
  input_tokens,
  output_tokens,
  total_tokens,
  total_cost_usd,
  response_time_ms,
  request_metadata,
  status,
  billing_status,
  created_at
) VALUES (
  'usage-contribution-test',
  'request-contribution-test',
  'Contribution test',
  'gpt-test',
  'provider-contribution-test',
  'provider-key-contribution-test',
  40,
  60,
  100,
  0.25,
  25,
  '{"base_cost_usd":0.25}'::jsonb,
  'completed',
  'pending',
  to_timestamp(1500)
)
"#,
    )
    .execute(&pool)
    .await
    .expect("pending usage should insert");

    let mut tx = pool
        .begin()
        .await
        .expect("pending transaction should begin");
    crate::repository::usage::postgres::provider_contribution::
        sync_provider_api_key_usage_contribution_for_request_in_tx(
            &mut tx,
            "request-contribution-test",
        )
        .await
        .expect("pending contribution should sync");
    tx.commit()
        .await
        .expect("pending contribution should commit");

    let pending_contribution = query(
        r#"
SELECT provider_api_key_id, request_count, total_tokens,
       CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
       window_request_count, window_total_tokens,
       CAST(window_total_cost_usd AS DOUBLE PRECISION) AS window_total_cost_usd,
       revision
FROM public.provider_api_key_usage_contributions
WHERE request_id = 'request-contribution-test'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("pending contribution should read");
    assert_eq!(
        pending_contribution.get::<String, _>("provider_api_key_id"),
        "provider-key-contribution-test"
    );
    assert_eq!(pending_contribution.get::<i64, _>("request_count"), 1);
    assert_eq!(pending_contribution.get::<i64, _>("total_tokens"), 100);
    assert_eq!(pending_contribution.get::<f64, _>("total_cost_usd"), 0.0);
    assert_eq!(
        pending_contribution.get::<i64, _>("window_request_count"),
        0
    );
    assert_eq!(pending_contribution.get::<i64, _>("revision"), 1);

    query(
        r#"
UPDATE public.usage
SET billing_status = 'settled'
WHERE request_id = 'request-contribution-test'
"#,
    )
    .execute(&pool)
    .await
    .expect("usage should settle");

    let mut tx = pool
        .begin()
        .await
        .expect("settlement transaction should begin");
    crate::repository::usage::postgres::provider_contribution::
        sync_provider_api_key_usage_contribution_for_request_in_tx(
            &mut tx,
            "request-contribution-test",
        )
        .await
        .expect("settled contribution should sync");
    crate::repository::usage::postgres::provider_contribution::
        sync_provider_api_key_usage_contribution_for_request_in_tx(
            &mut tx,
            "request-contribution-test",
        )
        .await
        .expect("repeated settled contribution should be idempotent");
    tx.commit()
        .await
        .expect("settled contribution should commit");

    let settled_contribution = query(
        r#"
SELECT CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
       window_request_count, window_total_tokens,
       CAST(window_total_cost_usd AS DOUBLE PRECISION) AS window_total_cost_usd,
       revision
FROM public.provider_api_key_usage_contributions
WHERE request_id = 'request-contribution-test'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("settled contribution should read");
    assert_eq!(settled_contribution.get::<f64, _>("total_cost_usd"), 0.25);
    assert_eq!(
        settled_contribution.get::<i64, _>("window_request_count"),
        1
    );
    assert_eq!(
        settled_contribution.get::<i64, _>("window_total_tokens"),
        100
    );
    assert_eq!(
        settled_contribution.get::<f64, _>("window_total_cost_usd"),
        0.25
    );
    assert_eq!(settled_contribution.get::<i64, _>("revision"), 2);

    let main_delta = query(
        r#"
SELECT COUNT(*)::BIGINT AS rows,
       COALESCE(SUM(request_count_delta), 0)::BIGINT AS requests,
       COALESCE(SUM(total_tokens_delta), 0)::BIGINT AS tokens,
       COALESCE(SUM(total_cost_usd_delta), 0)::DOUBLE PRECISION AS cost
FROM public.usage_counter_deltas
WHERE request_id = 'request-contribution-test'
  AND kind = 'provider_api_key'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("main provider deltas should read");
    assert_eq!(main_delta.get::<i64, _>("rows"), 2);
    assert_eq!(main_delta.get::<i64, _>("requests"), 1);
    assert_eq!(main_delta.get::<i64, _>("tokens"), 100);
    assert_eq!(main_delta.get::<f64, _>("cost"), 0.25);

    let window_delta = query(
        r#"
SELECT COUNT(*)::BIGINT AS rows,
       COALESCE(SUM(window_request_count_delta), 0)::BIGINT AS requests,
       COALESCE(SUM(window_total_tokens_delta), 0)::BIGINT AS tokens,
       COALESCE(SUM(window_total_cost_usd_delta), 0)::DOUBLE PRECISION AS cost
FROM public.usage_counter_deltas
WHERE request_id = 'request-contribution-test'
  AND kind = 'provider_api_key_window'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("window provider deltas should read");
    assert_eq!(window_delta.get::<i64, _>("rows"), 1);
    assert_eq!(window_delta.get::<i64, _>("requests"), 1);
    assert_eq!(window_delta.get::<i64, _>("tokens"), 100);
    assert_eq!(window_delta.get::<f64, _>("cost"), 0.25);

    query(
        r#"
INSERT INTO public.provider_api_key_window_usage_counters (
  provider_api_key_id,
  window_scope,
  window_code,
  window_start_unix_secs,
  window_end_unix_secs,
  request_count,
  total_tokens,
  total_cost_usd,
  rebuilt_at
) VALUES (
  'provider-key-contribution-test',
  'account',
  'weekly',
  1000,
  2000,
  0,
  0,
  0,
  NOW()
)
"#,
    )
    .execute(&pool)
    .await
    .expect("zero window projection should insert");
    let missing: bool = query_scalar(include_str!(
        "../../repository/usage/postgres/queries/provider_api_key_codex_window_usage_missing_sql.sql"
    ))
    .bind("provider-key-contribution-test")
    .fetch_one(&pool)
    .await
    .expect("window drift should read");
    assert!(missing);

    query(include_str!(
        "../../repository/usage/postgres/queries/rebuild_provider_api_key_codex_window_usage_for_key_sql.sql"
    ))
    .bind("provider-key-contribution-test")
    .execute(&pool)
    .await
    .expect("drifted zero window should rebuild");
    let rebuilt_window = query(
        r#"
SELECT request_count, total_tokens,
       CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
       rebuilt_at IS NOT NULL AS rebuilt
FROM public.provider_api_key_window_usage_counters
WHERE provider_api_key_id = 'provider-key-contribution-test'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("rebuilt window should read");
    assert_eq!(rebuilt_window.get::<i64, _>("request_count"), 1);
    assert_eq!(rebuilt_window.get::<i64, _>("total_tokens"), 100);
    assert_eq!(rebuilt_window.get::<f64, _>("total_cost_usd"), 0.25);
    assert!(rebuilt_window.get::<bool, _>("rebuilt"));

    let missing_after_rebuild: bool = query_scalar(include_str!(
        "../../repository/usage/postgres/queries/provider_api_key_codex_window_usage_missing_sql.sql"
    ))
    .bind("provider-key-contribution-test")
    .fetch_one(&pool)
    .await
    .expect("rebuilt window drift should read");
    assert!(!missing_after_rebuild);
}

#[tokio::test]
async fn provider_usage_contribution_migration_repairs_legacy_projection_without_replaying_queue() {
    let Some(server) = ManagedPostgresServer::try_start()
        .await
        .expect("postgres provider contribution migration test should start or skip")
    else {
        return;
    };
    let pool = PgPool::connect(server.database_url())
        .await
        .expect("pool should connect");
    run_migrations(&pool)
        .await
        .expect("clean database migrations should succeed");

    query(
        r#"
INSERT INTO public.providers (id, name, provider_type)
VALUES ('provider-migration-repair', 'Migration repair', 'codex')
"#,
    )
    .execute(&pool)
    .await
    .expect("provider should insert");
    query(
        r#"
INSERT INTO public.provider_api_keys (
  id,
  name,
  provider_id,
  request_count,
  success_count,
  error_count,
  total_tokens,
  total_cost_usd,
  total_response_time_ms
) VALUES (
  'provider-key-migration-repair',
  'Migration repair key',
  'provider-migration-repair',
  80,
  70,
  10,
  8000,
  0,
  9000
)
"#,
    )
    .execute(&pool)
    .await
    .expect("provider key should insert");
    query(
        r#"
INSERT INTO public.usage (
  id,
  request_id,
  provider_name,
  model,
  provider_id,
  provider_api_key_id,
  input_tokens,
  output_tokens,
  total_tokens,
  total_cost_usd,
  response_time_ms,
  request_metadata,
  status,
  status_code,
  billing_status,
  created_at
) VALUES (
  'usage-migration-repair',
  'request-migration-repair',
  'Migration repair',
  'gpt-test',
  'provider-migration-repair',
  'provider-key-migration-repair',
  40,
  60,
  100,
  2,
  25,
  '{"base_cost_usd":0.25}'::jsonb,
  'completed',
  200,
  'settled',
  to_timestamp(1500)
)
"#,
    )
    .execute(&pool)
    .await
    .expect("settled usage should insert");
    query(
        r#"
INSERT INTO public.usage_counter_deltas (
  id,
  request_id,
  kind,
  target_id,
  request_count_delta,
  success_count_delta,
  total_tokens_delta
) VALUES (
  'legacy-provider-delta',
  'request-migration-repair',
  'provider_api_key',
  'provider-key-migration-repair',
  1,
  1,
  100
)
"#,
    )
    .execute(&pool)
    .await
    .expect("legacy provider delta should insert");
    query("TRUNCATE TABLE public.provider_api_key_usage_contributions")
        .execute(&pool)
        .await
        .expect("provider contribution fixture should reset");

    let migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260801190000)
        .expect("provider usage contribution migration should be embedded");
    sqlx::raw_sql(migration.sql.as_ref())
        .execute(&pool)
        .await
        .expect("provider contribution migration should rerun for backfill fixture");

    let repository = crate::repository::usage::postgres::SqlxUsageReadRepository::new(pool.clone());
    let first_maintenance = repository
        .run_provider_api_key_usage_projection_maintenance(1)
        .await
        .expect("provider contribution backfill should run");
    assert_eq!(first_maintenance.backfill_requests, 1);
    assert_eq!(first_maintenance.completed_backfill_keys, 0);

    let maintenance = repository
        .run_provider_api_key_usage_projection_maintenance(1)
        .await
        .expect("provider contribution backfill should finish");
    assert_eq!(maintenance.backfill_requests, 0);
    assert_eq!(maintenance.completed_backfill_keys, 1);

    let contribution = query(
        r#"
SELECT request_count, success_count, total_tokens,
       CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
       window_request_count, window_total_tokens,
       CAST(window_total_cost_usd AS DOUBLE PRECISION) AS window_total_cost_usd
FROM public.provider_api_key_usage_contributions
WHERE request_id = 'request-migration-repair'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("repaired contribution should read");
    assert_eq!(contribution.get::<i64, _>("request_count"), 1);
    assert_eq!(contribution.get::<i64, _>("success_count"), 1);
    assert_eq!(contribution.get::<i64, _>("total_tokens"), 100);
    assert_eq!(contribution.get::<f64, _>("total_cost_usd"), 0.25);
    assert_eq!(contribution.get::<i64, _>("window_request_count"), 1);
    assert_eq!(contribution.get::<i64, _>("window_total_tokens"), 100);
    assert_eq!(contribution.get::<f64, _>("window_total_cost_usd"), 0.25);

    let provider_key = query(
        r#"
SELECT request_count, success_count, error_count, total_tokens,
       CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
       total_response_time_ms
FROM public.provider_api_keys
WHERE id = 'provider-key-migration-repair'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("repaired provider key should read");
    assert_eq!(provider_key.get::<i64, _>("request_count"), 1);
    assert_eq!(provider_key.get::<i64, _>("success_count"), 1);
    assert_eq!(provider_key.get::<i64, _>("error_count"), 0);
    assert_eq!(provider_key.get::<i64, _>("total_tokens"), 100);
    assert_eq!(provider_key.get::<f64, _>("total_cost_usd"), 0.25);
    assert_eq!(provider_key.get::<i64, _>("total_response_time_ms"), 25);

    let legacy_delta_processed: bool = query_scalar(
        r#"
SELECT processed_at IS NOT NULL
FROM public.usage_counter_deltas
WHERE id = 'legacy-provider-delta'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("legacy provider delta should read");
    assert!(legacy_delta_processed);

    query(
        r#"
INSERT INTO public.usage (
  id,
  request_id,
  provider_name,
  model,
  provider_id,
  provider_api_key_id,
  input_tokens,
  output_tokens,
  total_tokens,
  total_cost_usd,
  response_time_ms,
  request_metadata,
  status,
  status_code,
  billing_status,
  created_at
) VALUES (
  'usage-migration-repair-concurrent',
  'request-migration-repair-concurrent',
  'Migration repair',
  'gpt-test',
  'provider-migration-repair',
  'provider-key-migration-repair',
  80,
  120,
  200,
  0.5,
  30,
  '{"settlement_snapshot":{"base_cost_usd":0.5}}'::jsonb,
  'completed',
  200,
  'settled',
  to_timestamp(1600)
)
"#,
    )
    .execute(&pool)
    .await
    .expect("concurrent usage should insert");
    sync_provider_contributions(&pool, &["request-migration-repair-concurrent"])
        .await
        .expect("concurrent contribution should sync");
    query(
        r#"
UPDATE public.provider_api_keys
SET request_count = 0,
    success_count = 0,
    error_count = 0,
    total_tokens = 0,
    total_cost_usd = 0,
    total_response_time_ms = 0
WHERE id = 'provider-key-migration-repair'
"#,
    )
    .execute(&pool)
    .await
    .expect("provider projection should reset for concurrency test");

    let (rebuild_result, flush_result) = tokio::join!(
        repository.rebuild_provider_api_key_usage_stats(),
        repository.flush_usage_counter_deltas(100)
    );
    rebuild_result.expect("concurrent provider rebuild should succeed");
    flush_result.expect("concurrent provider delta flush should succeed");

    let concurrent_projection = query(
        r#"
SELECT request_count, success_count, total_tokens,
       CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
       total_response_time_ms
FROM public.provider_api_keys
WHERE id = 'provider-key-migration-repair'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("concurrent provider projection should read");
    assert_eq!(concurrent_projection.get::<i64, _>("request_count"), 2);
    assert_eq!(concurrent_projection.get::<i64, _>("success_count"), 2);
    assert_eq!(concurrent_projection.get::<i64, _>("total_tokens"), 300);
    assert_eq!(concurrent_projection.get::<f64, _>("total_cost_usd"), 0.75);
    assert_eq!(
        concurrent_projection.get::<i64, _>("total_response_time_ms"),
        55
    );
}

#[tokio::test]
async fn provider_usage_projection_repair_records_delayed_retry_after_failure() {
    let Some(server) = ManagedPostgresServer::try_start()
        .await
        .expect("postgres provider projection retry test should start or skip")
    else {
        return;
    };
    let pool = PgPool::connect(server.database_url())
        .await
        .expect("pool should connect");
    run_migrations(&pool)
        .await
        .expect("clean database migrations should succeed");

    query(
        r#"
INSERT INTO public.providers (id, name, provider_type)
VALUES ('provider-retry-test', 'Retry test', 'codex')
"#,
    )
    .execute(&pool)
    .await
    .expect("provider should insert");
    query(
        r#"
INSERT INTO public.provider_api_keys (
  id,
  name,
  provider_id,
  status_snapshot
) VALUES (
  'provider-key-retry-test',
  'Retry key',
  'provider-retry-test',
  '{"quota":{"windows":[]}}'::jsonb
)
"#,
    )
    .execute(&pool)
    .await
    .expect("provider key should insert");

    let repository = crate::repository::usage::postgres::SqlxUsageReadRepository::new(pool.clone());
    let initial = repository
        .run_provider_api_key_usage_projection_maintenance(1)
        .await
        .expect("empty retry key should finish historical maintenance");
    assert_eq!(initial.completed_backfill_keys, 1);

    query(
        r#"
UPDATE public.provider_api_keys
SET status_snapshot = $1
WHERE id = 'provider-key-retry-test'
"#,
    )
    .bind(serde_json::json!({
        "quota": {
            "windows": [
                {
                    "code": "weekly",
                    "scope": "account",
                    "window_minutes": "9999999999999999",
                    "reset_at": 2_000
                }
            ]
        }
    }))
    .execute(&pool)
    .await
    .expect("invalid retry window should update");
    repository
        .enqueue_provider_api_key_usage_projection_repair("provider-key-retry-test", false, true)
        .await
        .expect("retry repair should enqueue");

    let summary = repository
        .run_provider_api_key_usage_projection_maintenance(1)
        .await
        .expect("failed repair should be recorded");
    assert_eq!(summary.failed_repair_keys, 1);

    let retry = query(
        r#"
SELECT attempts, last_error, available_at > NOW() AS delayed
FROM public.provider_api_key_usage_projection_repairs
WHERE provider_api_key_id = 'provider-key-retry-test'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("failed repair should remain queued");
    assert_eq!(retry.get::<i32, _>("attempts"), 1);
    assert!(retry
        .get::<Option<String>, _>("last_error")
        .is_some_and(|value| !value.trim().is_empty()));
    assert!(retry.get::<bool, _>("delayed"));
}

#[test]
fn normalized_endpoint_formats_do_not_require_unique_provider_format_pairs() {
    let normalization_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260428000000)
        .expect("api format normalization migration should be embedded");

    assert!(normalization_migration
        .sql
        .contains("DROP CONSTRAINT IF EXISTS uq_provider_api_format"));
    assert!(normalization_migration
        .sql
        .contains("idx_provider_endpoints_provider_api_format"));
    assert!(!EMPTY_DATABASE_SNAPSHOT_SQL.contains("uq_provider_api_format"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("idx_provider_endpoints_provider_api_format"));
}

#[test]
fn split_baseline_sources_match_executable_migrations() {
    fn schema_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema")
    }

    fn compose_manifest(relative_manifest: &str) -> String {
        let root = schema_root();
        let manifest_path = root.join(relative_manifest);
        let manifest = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|err| panic!("failed to read {manifest_path:?}: {err}"));
        let manifest_dir = manifest_path
            .parent()
            .expect("schema manifest should have a parent directory");

        let mut output = String::new();
        for line in manifest.lines() {
            let part = line.trim();
            if part.is_empty() || part.starts_with('#') {
                continue;
            }
            let part_path = manifest_dir.join(part);
            output.push_str(
                &fs::read_to_string(&part_path)
                    .unwrap_or_else(|err| panic!("failed to read {part_path:?}: {err}")),
            );
        }
        output
    }

    assert_eq!(
        include_str!("../../../migrations/postgres/20260403000000_baseline.sql"),
        compose_manifest("drivers/postgres/baseline/manifest.txt")
    );
    assert_eq!(
        EMPTY_DATABASE_SNAPSHOT_SQL,
        compose_manifest("bootstrap/postgres/manifest.txt")
    );
}
#[test]
fn fresh_usage_schema_projects_upstream_stream_mode() {
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("upstream_is_stream boolean"));
}

#[tokio::test]
async fn api_format_normalization_migration_preserves_duplicate_endpoint_transports() {
    let Some(server) = ManagedPostgresServer::try_start()
        .await
        .expect("postgres migration test should start or skip")
    else {
        return;
    };
    let pool = PgPool::connect(server.database_url())
        .await
        .expect("pool should connect");
    let normalization_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260428000000)
        .expect("api format normalization migration should be embedded");

    sqlx::raw_sql(
        r#"
CREATE TABLE public.providers (
  id text PRIMARY KEY,
  provider_type text NOT NULL
);

CREATE TABLE public.provider_endpoints (
  id text PRIMARY KEY,
  provider_id text NOT NULL,
  api_format text NOT NULL,
  api_family text,
  endpoint_kind text,
  base_url text NOT NULL,
  max_retries integer,
  is_active boolean DEFAULT true NOT NULL,
  custom_path text,
  config json,
  created_at timestamp with time zone DEFAULT now() NOT NULL,
  updated_at timestamp with time zone DEFAULT now() NOT NULL,
  proxy jsonb,
  header_rules json,
  format_acceptance_config json,
  body_rules json
);

ALTER TABLE ONLY public.provider_endpoints
  ADD CONSTRAINT uq_provider_api_format UNIQUE (provider_id, api_format);

CREATE TABLE public.provider_api_keys (
  id text PRIMARY KEY,
  provider_id text NOT NULL,
  api_key text,
  auth_type text DEFAULT 'api_key' NOT NULL,
  auth_type_by_format json,
  api_formats json,
  updated_at timestamp with time zone DEFAULT now() NOT NULL,
  rate_multipliers json,
  global_priority_by_format json,
  health_by_format jsonb,
  circuit_breaker_by_format jsonb
);

CREATE TABLE public.api_keys (
  id text PRIMARY KEY,
  allowed_api_formats json,
  updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.users (
  id text PRIMARY KEY,
  allowed_api_formats json,
  updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.models (
  id text PRIMARY KEY,
  provider_model_mappings jsonb,
  updated_at timestamp with time zone DEFAULT now() NOT NULL
);

INSERT INTO public.providers (id, provider_type)
VALUES
  ('provider-conflict', 'custom'),
  ('provider-claude-code', 'claude_code'),
  ('provider-gemini-cli', 'gemini_cli');

INSERT INTO public.provider_endpoints (
  id,
  provider_id,
  api_format,
  api_family,
  endpoint_kind,
  base_url,
  custom_path,
  max_retries,
  header_rules,
  body_rules,
  config,
  proxy,
  format_acceptance_config
) VALUES
  (
    'endpoint-claude-chat',
    'provider-conflict',
    'claude:chat',
    'claude',
    'chat',
    'https://claude-chat.example',
    '/v1/messages',
    2,
    '{"x-channel":"chat"}'::json,
    '{"mode":"chat"}'::json,
    '{"transport":"chat"}'::json,
    '{"url":"http://proxy-chat"}'::jsonb,
    '{"accept":"chat"}'::json
  ),
  (
    'endpoint-claude-cli',
    'provider-conflict',
    'claude:cli',
    'claude',
    'cli',
    'https://claude-cli.example',
    '/v1/messages',
    3,
    '{"x-channel":"cli"}'::json,
    '{"mode":"cli"}'::json,
    '{"transport":"cli"}'::json,
    '{"url":"http://proxy-cli"}'::jsonb,
    '{"accept":"cli"}'::json
  ),
  (
    'endpoint-claude-oauth-cli',
    'provider-claude-code',
    'claude:cli',
    'claude',
    'cli',
    'https://claude-oauth.example',
    '/v1/messages',
    3,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL
  ),
  (
    'endpoint-gemini-oauth-cli',
    'provider-gemini-cli',
    'gemini:cli',
    'gemini',
    'cli',
    'https://gemini-oauth.example',
    '/v1beta/models',
    3,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL
  );

INSERT INTO public.provider_api_keys (
  id,
  provider_id,
  auth_type,
  auth_type_by_format,
  api_formats,
  rate_multipliers,
  global_priority_by_format,
  health_by_format,
  circuit_breaker_by_format
) VALUES
  (
    'provider-key',
    'provider-conflict',
    'api_key',
    '{"claude:cli":"bearer","gemini:chat":"api-key"}'::json,
    '["claude:chat","claude:cli","openai:cli","openai:responses"]'::json,
    '{"claude:chat":1,"openai:compact":2}'::json,
    '{"gemini:cli":3}'::json,
    '{"openai:cli":{"health_score":0.9}}'::jsonb,
    '{"openai:compact":{"open":false}}'::jsonb
  ),
  (
    'provider-raw-cli-key',
    'provider-conflict',
    'api_key',
    NULL,
    '["claude:cli"]'::json,
    NULL,
    NULL,
    NULL,
    NULL
  ),
  (
    'provider-chat-key',
    'provider-conflict',
    'bearer',
    NULL,
    '["gemini:chat"]'::json,
    NULL,
    NULL,
    NULL,
    NULL
  ),
  (
    'provider-claude-oauth-key',
    'provider-claude-code',
    'api_key',
    NULL,
    '["claude:cli"]'::json,
    NULL,
    NULL,
    NULL,
    NULL
  ),
  (
    'provider-claude-oauth-null-key',
    'provider-claude-code',
    'api_key',
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL
  ),
  (
    'provider-gemini-oauth-key',
    'provider-gemini-cli',
    'api_key',
    NULL,
    '["gemini:cli"]'::json,
    NULL,
    NULL,
    NULL,
    NULL
  ),
  (
    'provider-gemini-oauth-null-key',
    'provider-gemini-cli',
    'api_key',
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL
  );

INSERT INTO public.api_keys (id, allowed_api_formats)
VALUES ('api-key', '["gemini:chat","gemini:cli"]'::json);

INSERT INTO public.users (id, allowed_api_formats)
VALUES ('user', '["openai:compact","openai:responses:compact"]'::json);

INSERT INTO public.models (id, provider_model_mappings)
VALUES (
  'model',
  '[{"api_formats":["claude:chat","claude:cli","gemini:chat"]}]'::jsonb
);
"#,
    )
    .execute(&pool)
    .await
    .expect("fixture schema should be created");

    sqlx::raw_sql(&normalization_migration.sql)
        .execute(&pool)
        .await
        .expect("api format normalization migration should preserve duplicate endpoints");

    let endpoint_rows = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
        r#"
SELECT id, api_format, api_family, endpoint_kind
FROM public.provider_endpoints
WHERE provider_id = 'provider-conflict'
ORDER BY id
"#,
    )
    .fetch_all(&pool)
    .await
    .expect("endpoint rows should be readable");
    assert_eq!(
        endpoint_rows,
        vec![
            (
                "endpoint-claude-chat".to_string(),
                "claude:messages".to_string(),
                Some("claude".to_string()),
                Some("messages".to_string())
            ),
            (
                "endpoint-claude-cli".to_string(),
                "claude:messages".to_string(),
                Some("claude".to_string()),
                Some("messages".to_string())
            ),
        ]
    );

    let base_urls = sqlx::query_as::<_, (String,)>(
        r#"
SELECT base_url
FROM public.provider_endpoints
WHERE provider_id = 'provider-conflict'
ORDER BY id
"#,
    )
    .fetch_all(&pool)
    .await
    .expect("endpoint transport rows should be readable")
    .into_iter()
    .map(|(base_url,)| base_url)
    .collect::<Vec<_>>();
    assert_eq!(
        base_urls,
        vec![
            "https://claude-chat.example".to_string(),
            "https://claude-cli.example".to_string()
        ]
    );

    let provider_key_formats: serde_json::Value = query_scalar(
        "SELECT api_formats::jsonb FROM public.provider_api_keys WHERE id = 'provider-key'",
    )
    .fetch_one(&pool)
    .await
    .expect("provider key formats should be readable");
    assert_eq!(
        provider_key_formats,
        serde_json::json!(["claude:messages", "openai:responses"])
    );

    let provider_key_auth_rows = sqlx::query_as::<_, (String, String, Option<serde_json::Value>)>(
        r#"
SELECT id, auth_type, auth_type_by_format::jsonb
FROM public.provider_api_keys
WHERE id IN (
  'provider-chat-key',
  'provider-claude-oauth-key',
  'provider-claude-oauth-null-key',
  'provider-gemini-oauth-key',
  'provider-gemini-oauth-null-key',
  'provider-key',
  'provider-raw-cli-key'
)
ORDER BY id
"#,
    )
    .fetch_all(&pool)
    .await
    .expect("provider key auth rows should be readable");
    assert_eq!(
        provider_key_auth_rows,
        vec![
            ("provider-chat-key".to_string(), "api_key".to_string(), None),
            (
                "provider-claude-oauth-key".to_string(),
                "oauth".to_string(),
                None
            ),
            (
                "provider-claude-oauth-null-key".to_string(),
                "oauth".to_string(),
                None
            ),
            (
                "provider-gemini-oauth-key".to_string(),
                "oauth".to_string(),
                None
            ),
            (
                "provider-gemini-oauth-null-key".to_string(),
                "oauth".to_string(),
                None
            ),
            (
                "provider-key".to_string(),
                "api_key".to_string(),
                Some(serde_json::json!({
                    "claude:messages": "bearer",
                    "gemini:generate_content": "api_key"
                }))
            ),
            (
                "provider-raw-cli-key".to_string(),
                "api_key".to_string(),
                Some(serde_json::json!({"claude:messages": "bearer"}))
            ),
        ]
    );

    let provider_format_constraint_count: i64 = query_scalar(
        "SELECT COUNT(*)::BIGINT FROM pg_constraint WHERE conname = 'uq_provider_api_format'",
    )
    .fetch_one(&pool)
    .await
    .expect("constraint count should be readable");
    assert_eq!(provider_format_constraint_count, 0);
}

#[test]
fn deprecation_migration_and_baseline_mark_legacy_usage_columns() {
    let settlement_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260413020000)
        .expect("deprecation migration should be embedded");
    let http_migration = POSTGRES_MIGRATOR
        .iter()
        .find(|migration| migration.version == 20260413030000)
        .expect("http/body deprecation migration should be embedded");

    assert!(settlement_migration
        .sql
        .contains("COMMENT ON COLUMN public.usage.output_price_per_1m"));
    assert!(settlement_migration
        .sql
        .contains("COMMENT ON COLUMN public.usage.wallet_id"));
    assert!(settlement_migration
        .sql
        .contains("COMMENT ON COLUMN public.usage.username"));
    assert!(settlement_migration
        .sql
        .contains("COMMENT ON COLUMN public.usage.api_key_name"));
    assert!(http_migration
        .sql
        .contains("COMMENT ON COLUMN public.usage.request_headers"));
    assert!(http_migration
        .sql
        .contains("COMMENT ON COLUMN public.usage.request_body"));
    assert!(http_migration
        .sql
        .contains("COMMENT ON COLUMN public.usage.billing_status"));
    assert!(http_migration
        .sql
        .contains("COMMENT ON COLUMN public.usage.finalized_at"));
    assert!(
        EMPTY_DATABASE_SNAPSHOT_SQL.contains("COMMENT ON COLUMN public.usage.output_price_per_1m")
    );
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("COMMENT ON COLUMN public.usage.wallet_id"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("COMMENT ON COLUMN public.usage.username"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("COMMENT ON COLUMN public.usage.api_key_name"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("COMMENT ON COLUMN public.usage.request_headers"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("COMMENT ON COLUMN public.usage.request_body"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("COMMENT ON COLUMN public.usage.billing_status"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("COMMENT ON COLUMN public.usage.finalized_at"));
}

#[test]
fn pending_migrations_from_applied_returns_all_versions_when_none_applied() {
    let pending = pending_migrations_from_applied(&[]);
    assert_eq!(pending, all_up_migrations());
}

#[test]
fn embedded_postgres_manifest_contains_latest_production_migrations() {
    let versions = super::embedded_postgres_migration_versions();

    assert!(versions.contains(&20260723120000));
    assert!(versions.contains(&20260723121000));
    assert!(versions.contains(&20260723122000));
    assert!(versions.contains(&20260810180000));
    assert!(versions.contains(&20260824120000));
    assert!(versions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn pending_migrations_from_applied_skips_versions_already_applied() {
    let applied = vec![
        AppliedMigration {
            version: 20260403000000,
            checksum: Cow::Borrowed(&[]),
        },
        AppliedMigration {
            version: 20260406000000,
            checksum: Cow::Borrowed(&[]),
        },
    ];

    let pending_versions = pending_migrations_from_applied(&applied)
        .into_iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();

    assert_eq!(
        pending_versions,
        vec![
            20260410000000,
            20260413020000,
            20260413030000,
            20260415000000,
            20260418000000,
            20260421000000,
            20260422110000,
            20260422120000,
            20260423000000,
            20260424000000,
            20260428000000,
            20260502000000,
            20260505000000,
            20260505130000,
            20260507000000,
            20260507120000,
            20260508000000,
            20260509000000,
            20260509120000,
            20260510000000,
            20260510120000,
            20260511000000,
            20260511120000,
            20260511130000,
            20260512000000,
            20260512090000,
            20260512110000,
            20260515000000,
            20260516000000,
            20260518000000,
            20260519000000,
            20260519120000,
            20260519130000,
            20260527120000,
            20260528120000,
            20260530120000,
            20260531120000,
            20260601120000,
            20260606120000,
            20260607120000,
            20260608120000,
            20260608130000,
            20260608140000,
            20260608150000,
            20260608160000,
            20260609120000,
            20260610120000,
            20260610143000,
            20260615120000,
            20260620090000,
            20260622100000,
            20260723120000,
            20260723121000,
            20260723122000,
            20260725100000,
            20260731190000,
            20260801190000,
            20260804120000,
            20260809130000,
            20260810180000,
            20260824120000,
        ]
    );
}

#[test]
fn pending_migrations_from_applied_after_empty_database_snapshot_stamp_returns_post_snapshot_incrementals(
) {
    let applied = empty_database_snapshot_migrations(&POSTGRES_MIGRATOR)
        .expect("empty database snapshot migrations should resolve")
        .into_iter()
        .map(|migration| AppliedMigration {
            version: migration.version,
            checksum: migration.checksum.clone(),
        })
        .collect::<Vec<_>>();

    let pending = pending_migrations_from_applied(&applied);
    let pending_versions = pending
        .into_iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();

    assert_eq!(
        pending_versions,
        vec![
            20260607120000,
            20260608120000,
            20260608130000,
            20260608140000,
            20260608150000,
            20260608160000,
            20260609120000,
            20260610120000,
            20260610143000,
            20260615120000,
            20260620090000,
            20260622100000,
            20260723120000,
            20260723121000,
            20260723122000,
            20260725100000,
            20260731190000,
            20260801190000,
            20260804120000,
            20260809130000,
            20260810180000,
            20260824120000,
        ],
        "empty database snapshot-stamped databases should run only post-snapshot incrementals on first startup"
    );
}

#[tokio::test]
async fn postgres_migrations_create_core_config_tables_when_url_is_set() {
    let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        eprintln!(
            "skipping postgres migration smoke test because AETHER_TEST_POSTGRES_URL is unset"
        );
        return;
    };

    let pool = PgPool::connect(&database_url)
        .await
        .expect("postgres test pool should connect");

    super::run_migrations(&pool)
        .await
        .expect("postgres migrations should run");

    for table_name in [
        "users",
        "user_preferences",
        "user_sessions",
        "api_keys",
        "management_tokens",
        "billing_rules",
        "dimension_collectors",
        "providers",
        "provider_api_keys",
        "provider_endpoints",
        "models",
        "global_models",
        "system_configs",
        "auth_modules",
        "oauth_providers",
        "proxy_nodes",
        "usage",
        "usage_settlement_snapshots",
        "provider_api_key_window_usage_counters",
        "provider_api_key_window_usage_resets",
        "provider_api_key_window_usage_applications",
        "wallets",
        "wallet_transactions",
        "wallet_daily_usage_ledgers",
        "payment_orders",
        "payment_callbacks",
        "refund_requests",
        "redeem_code_batches",
        "redeem_codes",
    ] {
        let exists: i64 = query_scalar(
            r#"
SELECT COUNT(*)
FROM information_schema.tables
WHERE table_schema = 'public'
  AND table_name = $1
"#,
        )
        .bind(table_name)
        .fetch_one(&pool)
        .await
        .expect("postgres information_schema query should succeed");
        assert_eq!(exists, 1, "missing postgres table {table_name}");
    }

    let total_adjusted_exists: i64 = query_scalar(
        r#"
SELECT COUNT(*)
FROM information_schema.columns
WHERE table_schema = 'public'
  AND table_name = 'wallets'
  AND column_name = 'total_adjusted'
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("postgres information_schema column query should succeed");
    assert_eq!(
        total_adjusted_exists, 1,
        "missing postgres wallets.total_adjusted"
    );
}

#[tokio::test]
async fn prepare_database_for_startup_bootstraps_clean_database() {
    let Some(server) = ManagedPostgresServer::try_start()
        .await
        .expect("postgres bootstrap test should start or skip")
    else {
        return;
    };

    let pool = PgPool::connect(server.database_url())
        .await
        .expect("pool should connect");
    let pending = prepare_database_for_startup(&pool)
        .await
        .expect("clean database bootstrap should succeed");

    let snapshot_applied = empty_database_snapshot_migrations(&POSTGRES_MIGRATOR)
        .expect("baseline migrations should resolve")
        .into_iter()
        .map(|migration| AppliedMigration {
            version: migration.version,
            checksum: migration.checksum.clone(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        pending,
        pending_migrations_from_applied(&snapshot_applied),
        "startup preparation should report post-snapshot migrations for the migration runner"
    );
    assert!(table_exists(&pool, "users")
        .await
        .expect("users lookup should succeed"));
    assert!(table_exists(&pool, "usage")
        .await
        .expect("usage lookup should succeed"));
    assert!(column_exists(&pool, "api_keys", "total_tokens")
        .await
        .expect("api_keys.total_tokens lookup should succeed"));

    let audit_admin_exists: bool = query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_enum WHERE enumtypid = 'public.userrole'::regtype AND enumlabel = 'audit_admin')",
    )
    .fetch_one(&pool)
    .await
    .expect("public.userrole audit_admin lookup should succeed");
    assert!(
        audit_admin_exists,
        "fresh database snapshot should include public.userrole.audit_admin"
    );

    run_migrations(&pool)
        .await
        .expect("post-snapshot migrations should succeed");
    let pending_after_migration = prepare_database_for_startup(&pool)
        .await
        .expect("migrated database startup preparation should succeed");
    assert!(
        pending_after_migration.is_empty(),
        "fresh databases should have no pending migrations after the migration runner completes"
    );
    assert!(table_exists(&pool, "provider_api_key_usage_contributions")
        .await
        .expect("provider contribution table lookup should succeed"));

    let applied_count: i64 = query_scalar("SELECT COUNT(*)::BIGINT FROM public._sqlx_migrations")
        .fetch_one(&pool)
        .await
        .expect("migration count query should succeed");
    assert_eq!(applied_count, all_up_migrations().len() as i64);
}

#[tokio::test]
async fn prepare_database_for_startup_bootstraps_when_only_unrelated_public_tables_exist() {
    let Some(server) = ManagedPostgresServer::try_start()
        .await
        .expect("postgres bootstrap test should start or skip")
    else {
        return;
    };

    let pool = PgPool::connect(server.database_url())
        .await
        .expect("pool should connect");
    query("CREATE TABLE public.vendor_bootstrap_marker (id integer PRIMARY KEY)")
        .execute(&pool)
        .await
        .expect("fixture table should be created");

    let pending = prepare_database_for_startup(&pool)
        .await
        .expect("startup preparation should tolerate unrelated public tables");

    let snapshot_applied = empty_database_snapshot_migrations(&POSTGRES_MIGRATOR)
        .expect("baseline migrations should resolve")
        .into_iter()
        .map(|migration| AppliedMigration {
            version: migration.version,
            checksum: migration.checksum.clone(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        pending,
        pending_migrations_from_applied(&snapshot_applied),
        "unrelated public tables should not block snapshot bootstrap or pending migration reporting"
    );
    assert!(table_exists(&pool, "vendor_bootstrap_marker")
        .await
        .expect("fixture table lookup should succeed"));
    assert!(table_exists(&pool, "oauth_providers")
        .await
        .expect("oauth_providers lookup should succeed"));

    run_migrations(&pool)
        .await
        .expect("post-snapshot migrations should succeed");
    let pending_after_migration = prepare_database_for_startup(&pool)
        .await
        .expect("migrated database startup preparation should succeed");
    assert!(pending_after_migration.is_empty());
}

#[test]
fn billing_overdraft_root_fix_migrations_define_provider_scopes_and_admissions() {
    let migration = include_str!(
        "../../../migrations/postgres/20260809130000_add_billing_admissions_and_plan_providers.sql"
    );

    assert!(migration.contains("billing_plan_providers"));
    assert!(migration.contains("user_entitlement_providers"));
    assert!(migration.contains("billing_request_admissions"));
    assert!(migration.contains("wallet_payment_allowed"));
    assert!(migration.contains("wallet_overage_allowed"));
    assert!(migration.contains("entitlement_provider_scopes"));
    assert!(!migration.contains("selected_provider_id"));
}

#[test]
fn plan_purchase_debt_repayment_migrations_add_a_non_null_zero_default() {
    let migration = include_str!(
        "../../../migrations/postgres/20260810180000_add_plan_purchase_debt_repayment.sql"
    );

    assert!(migration.contains("debt_repayment_usd"));
    assert!(migration
        .to_ascii_uppercase()
        .contains("NOT NULL DEFAULT 0"));
}

#[test]
fn announcement_portal_migration_preserves_existing_rows_as_main_portal() {
    let migration = include_str!(
        "../../../migrations/postgres/20260824120000_scope_announcements_by_portal.sql"
    );

    assert!(migration.contains("ADD COLUMN IF NOT EXISTS portal_id"));
    assert!(migration.contains("SET portal_id = 'default'"));
    assert!(migration.contains("ALTER COLUMN portal_id SET NOT NULL"));
    assert!(migration.contains("announcements_portal_active_created_idx"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL
        .contains("portal_id character varying(32) DEFAULT 'default'::character varying NOT NULL"));
    assert!(EMPTY_DATABASE_SNAPSHOT_SQL.contains("announcements_portal_active_created_idx"));
}

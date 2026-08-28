//! Backend composition layer.
//!
//! `DataBackends` chooses the configured SQL driver, builds low-level pools,
//! instantiates concrete repositories, and exposes app-facing read/write,
//! lease, transaction, and maintenance handles. Request-path repository SQL
//! belongs in `repository/*`; backend-owned maintenance SQL lives in focused
//! modules such as `stats`, `wallet`, and `system`. Pool/client primitives
//! belong in `driver/*`.

mod leases;
mod maintenance;
mod postgres;
mod read;
mod stats;
mod system;
mod transactions;
mod wallet;
mod write;

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::maintenance::DatabasePoolSummary;
use crate::UsageObjectStore;
pub use leases::DataLeaseBackends;
pub use postgres::PostgresBackend;
pub use read::DataReadRepositories;
pub use transactions::DataTransactionBackends;
pub use write::DataWriteRepositories;

use crate::database::DatabaseDriver;
use crate::{DataLayerConfig, DataLayerError};

#[derive(Clone, Copy)]
enum SqlBackendRef<'a> {
    Postgres(&'a PostgresBackend),
}
#[derive(Debug, Clone, Default)]
pub struct DataBackends {
    config: DataLayerConfig,
    postgres: Option<PostgresBackend>,
    leases: DataLeaseBackends,
    read: DataReadRepositories,
    transactions: DataTransactionBackends,
    write: DataWriteRepositories,
}

fn summarize_pool(
    driver: DatabaseDriver,
    pool_size: usize,
    idle: usize,
    max_connections: u32,
) -> DatabasePoolSummary {
    let max_connections = max_connections.max(1);
    let checked_out = pool_size.saturating_sub(idle);
    let usage_rate = checked_out as f64 / f64::from(max_connections) * 100.0;

    DatabasePoolSummary {
        driver,
        checked_out,
        pool_size,
        idle,
        max_connections,
        usage_rate,
    }
}

impl DataBackends {
    fn sql_backend(&self) -> Option<SqlBackendRef<'_>> {
        self.postgres.as_ref().map(SqlBackendRef::Postgres)
    }

    pub fn from_config(config: DataLayerConfig) -> Result<Self, DataLayerError> {
        config.validate()?;

        let usage_object_store = config
            .usage_object_store
            .as_ref()
            .map(UsageObjectStore::from_config)
            .transpose()?
            .map(Arc::new);
        let postgres = config
            .effective_database()
            .map(|database| {
                PostgresBackend::from_config_with_usage_object_store(
                    database.to_postgres_config()?,
                    usage_object_store,
                )
            })
            .transpose()?;
        let leases = DataLeaseBackends::from_postgres(postgres.as_ref())?;
        let read = DataReadRepositories::from_postgres(postgres.as_ref());
        let transactions = DataTransactionBackends::from_postgres(postgres.as_ref());
        let write = DataWriteRepositories::from_postgres(postgres.as_ref());

        Ok(Self {
            config,
            postgres,
            leases,
            read,
            transactions,
            write,
        })
    }

    pub fn config(&self) -> &DataLayerConfig {
        &self.config
    }

    pub fn postgres(&self) -> Option<&PostgresBackend> {
        self.postgres.as_ref()
    }

    pub fn database_driver(&self) -> Option<DatabaseDriver> {
        self.config
            .effective_database()
            .map(|database| database.driver)
    }

    pub fn read(&self) -> &DataReadRepositories {
        &self.read
    }

    pub fn leases(&self) -> &DataLeaseBackends {
        &self.leases
    }

    pub fn transactions(&self) -> &DataTransactionBackends {
        &self.transactions
    }

    pub fn write(&self) -> &DataWriteRepositories {
        &self.write
    }

    pub fn usage_object_store(&self) -> Option<Arc<UsageObjectStore>> {
        self.postgres
            .as_ref()
            .and_then(PostgresBackend::usage_object_store)
    }

    pub fn has_runtime_backends(&self) -> bool {
        self.postgres.is_some()
            || self.leases.has_any()
            || self.read.has_any()
            || self.transactions.has_any()
            || self.write.has_any()
    }

    pub async fn check_table_existence(
        &self,
        table_names: &[&str],
    ) -> Result<Vec<(String, bool)>, DataLayerError> {
        let existing = match self.sql_backend() {
            Some(SqlBackendRef::Postgres(backend)) => sqlx::query_scalar::<_, String>(
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name LIKE 'niffler_%'",
            )
            .fetch_all(backend.pool())
            .await
            .map_err(DataLayerError::sql)?,
            None => Vec::new(),
        }
        .into_iter()
        .collect::<BTreeSet<_>>();

        Ok(table_names
            .iter()
            .map(|table_name| ((*table_name).to_string(), existing.contains(*table_name)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::DataBackends;
    use crate::{driver::postgres::PostgresPoolConfig, DataLayerConfig};

    #[test]
    fn builds_empty_backends_from_default_config() {
        let backends = DataBackends::from_config(DataLayerConfig::default())
            .expect("empty config should be accepted");

        assert!(!backends.has_runtime_backends());
        assert!(backends.postgres().is_none());
        assert!(backends.leases().postgres().is_none());
        assert!(backends.read().auth_api_keys().is_none());
        assert!(backends.read().auth_modules().is_none());
        assert!(backends.read().billing().is_none());
        assert!(backends.read().gemini_file_mappings().is_none());
        assert!(backends.read().global_models().is_none());
        assert!(backends.read().management_tokens().is_none());
        assert!(backends.read().oauth_providers().is_none());
        assert!(backends.read().proxy_nodes().is_none());
        assert!(backends.read().minimal_candidate_selection().is_none());
        assert!(backends.read().request_candidates().is_none());
        assert!(backends.read().provider_catalog().is_none());
        assert!(backends.read().usage().is_none());
        assert!(backends.read().video_tasks().is_none());
        assert!(backends.transactions().postgres().is_none());
        assert!(backends.write().settlement().is_none());
        assert!(backends.write().usage().is_none());
    }

    #[tokio::test]
    async fn builds_postgres_backend_from_config() {
        let backends = DataBackends::from_config(DataLayerConfig {
            database: None,
            postgres: Some(PostgresPoolConfig {
                database_url: "postgres://localhost/aether".to_string(),
                min_connections: 1,
                max_connections: 4,
                acquire_timeout_ms: 1_000,
                idle_timeout_ms: 5_000,
                max_lifetime_ms: 30_000,
                statement_cache_capacity: 64,
                require_ssl: false,
            }),
            usage_object_store: None,
        })
        .expect("postgres backend should build");

        assert!(backends.has_runtime_backends());
        assert!(backends.postgres().is_some());
        assert!(backends.leases().postgres().is_some());
        assert!(backends.read().auth_api_keys().is_some());
        assert!(backends.read().auth_modules().is_some());
        assert!(backends.read().billing().is_some());
        assert!(backends.read().gemini_file_mappings().is_some());
        assert!(backends.read().global_models().is_some());
        assert!(backends.read().management_tokens().is_some());
        assert!(backends.read().minimal_candidate_selection().is_some());
        assert!(backends.read().oauth_providers().is_some());
        assert!(backends.read().proxy_nodes().is_some());
        assert!(backends.read().minimal_candidate_selection().is_some());
        assert!(backends.read().request_candidates().is_some());
        assert!(backends.read().provider_catalog().is_some());
        assert!(backends.read().provider_quotas().is_some());
        assert!(backends.read().usage().is_some());
        assert!(backends.read().video_tasks().is_some());
        assert!(backends.read().wallets().is_some());
        assert!(backends.transactions().postgres().is_some());
        assert!(backends.write().auth_modules().is_some());
        assert!(backends.write().gemini_file_mappings().is_some());
        assert!(backends.write().management_tokens().is_some());
        assert!(backends.write().oauth_providers().is_some());
        assert!(backends.write().proxy_nodes().is_some());
        assert!(backends.write().provider_catalog().is_some());
        assert!(backends.write().provider_quotas().is_some());
        assert!(backends.write().settlement().is_some());
        assert!(backends.write().usage().is_some());
        assert!(backends.write().wallets().is_some());
        assert!(backends.config().effective_database().is_some());
    }
}

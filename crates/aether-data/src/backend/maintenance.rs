use super::{summarize_pool, DataBackends, PostgresBackend, SqlBackendRef};
use crate::error::SqlxResultExt;
use crate::maintenance::{
    DatabaseMaintenanceSummary, DatabasePoolSummary, StatsDailyAggregationInput,
    StatsDailyAggregationSummary, StatsHourlyAggregationInput, StatsHourlyAggregationSummary,
    WalletDailyUsageAggregationInput, WalletDailyUsageAggregationResult,
};
use crate::repository::system::{
    AdminSystemPurgeSummary, AdminSystemPurgeTarget, AdminSystemStats, StoredSystemConfigEntry,
};
use crate::DataLayerError;
use sqlx::migrate::MigrateError;

fn maintenance_identifier(value: &str) -> Result<&str, DataLayerError> {
    let valid = !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if valid {
        Ok(value)
    } else {
        Err(DataLayerError::InvalidInput(format!(
            "invalid maintenance table name: {value}"
        )))
    }
}
impl DataBackends {
    pub fn has_database_maintenance_backend(&self) -> bool {
        self.sql_backend().is_some()
    }

    pub fn has_database_pool_summary(&self) -> bool {
        self.sql_backend().is_some()
    }

    pub fn has_system_config_backend(&self) -> bool {
        self.sql_backend().is_some()
    }

    pub fn has_wallet_daily_usage_aggregation_backend(&self) -> bool {
        self.sql_backend().is_some()
    }

    pub fn has_stats_hourly_aggregation_backend(&self) -> bool {
        self.sql_backend().is_some()
    }

    pub fn has_stats_daily_aggregation_backend(&self) -> bool {
        self.sql_backend().is_some()
    }

    pub async fn run_database_maintenance(
        &self,
        table_names: &[&str],
    ) -> Result<DatabaseMaintenanceSummary, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.run_database_maintenance(table_names).await,
            None => Ok(DatabaseMaintenanceSummary::default()),
        }
    }

    pub async fn run_database_migrations(&self) -> Result<bool, MigrateError> {
        match self.sql_backend() {
            Some(backend) => backend.run_database_migrations().await,
            None => Ok(false),
        }
    }

    pub async fn run_database_backfills(&self) -> Result<bool, MigrateError> {
        match self.sql_backend() {
            Some(backend) => backend.run_database_backfills().await,
            None => Ok(false),
        }
    }

    pub async fn pending_database_migrations(
        &self,
    ) -> Result<Option<Vec<crate::lifecycle::migrate::PendingMigrationInfo>>, MigrateError> {
        match self.sql_backend() {
            Some(backend) => backend.pending_database_migrations().await,
            None => Ok(None),
        }
    }

    pub async fn prepare_database_for_startup(
        &self,
    ) -> Result<Option<Vec<crate::lifecycle::migrate::PendingMigrationInfo>>, MigrateError> {
        match self.sql_backend() {
            Some(backend) => backend.prepare_database_for_startup().await,
            None => Ok(None),
        }
    }

    pub async fn pending_database_backfills(
        &self,
    ) -> Result<Option<Vec<crate::lifecycle::backfill::PendingBackfillInfo>>, MigrateError> {
        match self.sql_backend() {
            Some(backend) => backend.pending_database_backfills().await,
            None => Ok(None),
        }
    }

    pub fn database_pool_summary(&self) -> Option<DatabasePoolSummary> {
        self.sql_backend().map(SqlBackendRef::database_pool_summary)
    }

    pub async fn aggregate_wallet_daily_usage(
        &self,
        input: &WalletDailyUsageAggregationInput,
    ) -> Result<WalletDailyUsageAggregationResult, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.aggregate_wallet_daily_usage(input).await,
            None => Ok(WalletDailyUsageAggregationResult::default()),
        }
    }

    pub async fn aggregate_stats_hourly(
        &self,
        input: &StatsHourlyAggregationInput,
    ) -> Result<Option<StatsHourlyAggregationSummary>, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.aggregate_stats_hourly(input).await,
            None => Ok(None),
        }
    }

    pub async fn aggregate_stats_daily(
        &self,
        input: &StatsDailyAggregationInput,
    ) -> Result<Option<StatsDailyAggregationSummary>, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.aggregate_stats_daily(input).await,
            None => Ok(None),
        }
    }

    pub async fn find_system_config_value(
        &self,
        key: &str,
    ) -> Result<Option<serde_json::Value>, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.find_system_config_value(key).await,
            None => Ok(None),
        }
    }

    pub async fn list_system_config_entries(
        &self,
    ) -> Result<Vec<StoredSystemConfigEntry>, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.list_system_config_entries().await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn upsert_system_config_entry(
        &self,
        key: &str,
        value: &serde_json::Value,
        description: Option<&str>,
    ) -> Result<Option<StoredSystemConfigEntry>, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend
                .upsert_system_config_entry(key, value, description)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    pub async fn delete_system_config_value(&self, key: &str) -> Result<bool, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.delete_system_config_value(key).await,
            None => Ok(false),
        }
    }

    pub async fn read_admin_system_stats(&self) -> Result<AdminSystemStats, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.read_admin_system_stats().await,
            None => Ok(AdminSystemStats::default()),
        }
    }

    pub async fn purge_admin_system_data(
        &self,
        target: AdminSystemPurgeTarget,
    ) -> Result<AdminSystemPurgeSummary, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.purge_admin_system_data(target).await,
            None => Ok(AdminSystemPurgeSummary::default()),
        }
    }

    pub async fn purge_admin_request_bodies_batch(
        &self,
        batch_size: usize,
    ) -> Result<AdminSystemPurgeSummary, DataLayerError> {
        match self.sql_backend() {
            Some(backend) => backend.purge_admin_request_bodies_batch(batch_size).await,
            None => Ok(AdminSystemPurgeSummary::default()),
        }
    }
}

impl PostgresBackend {
    pub async fn run_table_maintenance(
        &self,
        table_names: &[&str],
    ) -> Result<DatabaseMaintenanceSummary, DataLayerError> {
        let mut summary = DatabaseMaintenanceSummary::default();
        for table_name in table_names {
            let table_name = maintenance_identifier(table_name)?;
            summary.attempted += 1;
            let statement = format!("VACUUM ANALYZE \"{table_name}\"");
            if sqlx::raw_sql(&statement)
                .execute(self.pool())
                .await
                .map_postgres_err()
                .is_ok()
            {
                summary.succeeded += 1;
            }
        }
        Ok(summary)
    }
}

impl<'a> SqlBackendRef<'a> {
    fn postgres(self) -> &'a PostgresBackend {
        let Self::Postgres(postgres) = self;
        postgres
    }

    async fn run_database_maintenance(
        self,
        table_names: &[&str],
    ) -> Result<DatabaseMaintenanceSummary, DataLayerError> {
        self.postgres().run_table_maintenance(table_names).await
    }

    async fn run_database_migrations(self) -> Result<bool, MigrateError> {
        crate::lifecycle::migrate::run_migrations(self.postgres().pool()).await?;
        Ok(true)
    }

    async fn run_database_backfills(self) -> Result<bool, MigrateError> {
        crate::lifecycle::backfill::run_backfills(self.postgres().pool()).await?;
        Ok(true)
    }

    async fn pending_database_migrations(
        self,
    ) -> Result<Option<Vec<crate::lifecycle::migrate::PendingMigrationInfo>>, MigrateError> {
        Ok(Some(
            crate::lifecycle::migrate::pending_migrations(self.postgres().pool()).await?,
        ))
    }

    async fn prepare_database_for_startup(
        self,
    ) -> Result<Option<Vec<crate::lifecycle::migrate::PendingMigrationInfo>>, MigrateError> {
        Ok(Some(
            crate::lifecycle::migrate::prepare_database_for_startup(self.postgres().pool()).await?,
        ))
    }

    async fn pending_database_backfills(
        self,
    ) -> Result<Option<Vec<crate::lifecycle::backfill::PendingBackfillInfo>>, MigrateError> {
        Ok(Some(
            crate::lifecycle::backfill::pending_backfills(self.postgres().pool()).await?,
        ))
    }

    fn database_pool_summary(self) -> DatabasePoolSummary {
        let postgres = self.postgres();
        summarize_pool(
            crate::database::DatabaseDriver::Postgres,
            usize::try_from(postgres.pool().size()).unwrap_or(usize::MAX),
            postgres.pool().num_idle(),
            postgres.config().max_connections,
        )
    }

    async fn aggregate_wallet_daily_usage(
        self,
        input: &WalletDailyUsageAggregationInput,
    ) -> Result<WalletDailyUsageAggregationResult, DataLayerError> {
        self.postgres().aggregate_wallet_daily_usage(input).await
    }

    async fn aggregate_stats_hourly(
        self,
        input: &StatsHourlyAggregationInput,
    ) -> Result<Option<StatsHourlyAggregationSummary>, DataLayerError> {
        self.postgres().aggregate_stats_hourly(input).await
    }

    async fn aggregate_stats_daily(
        self,
        input: &StatsDailyAggregationInput,
    ) -> Result<Option<StatsDailyAggregationSummary>, DataLayerError> {
        self.postgres().aggregate_stats_daily(input).await
    }

    async fn find_system_config_value(
        self,
        key: &str,
    ) -> Result<Option<serde_json::Value>, DataLayerError> {
        self.postgres().find_system_config_value(key).await
    }

    async fn list_system_config_entries(
        self,
    ) -> Result<Vec<StoredSystemConfigEntry>, DataLayerError> {
        self.postgres().list_system_config_entries().await
    }

    async fn upsert_system_config_entry(
        self,
        key: &str,
        value: &serde_json::Value,
        description: Option<&str>,
    ) -> Result<StoredSystemConfigEntry, DataLayerError> {
        self.postgres()
            .upsert_system_config_entry(key, value, description)
            .await
    }

    async fn delete_system_config_value(self, key: &str) -> Result<bool, DataLayerError> {
        self.postgres().delete_system_config_value(key).await
    }

    async fn read_admin_system_stats(self) -> Result<AdminSystemStats, DataLayerError> {
        self.postgres().read_admin_system_stats().await
    }

    async fn purge_admin_system_data(
        self,
        target: AdminSystemPurgeTarget,
    ) -> Result<AdminSystemPurgeSummary, DataLayerError> {
        self.postgres().purge_admin_system_data(target).await
    }

    async fn purge_admin_request_bodies_batch(
        self,
        batch_size: usize,
    ) -> Result<AdminSystemPurgeSummary, DataLayerError> {
        self.postgres()
            .purge_admin_request_bodies_batch(batch_size)
            .await
    }
}

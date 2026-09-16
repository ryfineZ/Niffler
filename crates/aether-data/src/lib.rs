//! Runtime data access for Aether.
//!
//! This crate contains concrete database clients, repository
//! implementations, migration/backfill/export workflows, and the backend
//! composition layer. Shared repository contracts that other crates compile
//! against live in `aether-data-contracts`.
//!
//! See `crates/aether-data/README.md` for the layer map and SQL driver policy.

pub mod backend;
mod config;
mod database;
pub mod driver;
mod error;
pub mod lifecycle;
pub mod maintenance;
pub mod repository;
mod usage_object_store;

pub use backend::{
    DataBackends, DataLeaseBackends, DataReadRepositories, DataTransactionBackends,
    DataWriteRepositories, PostgresBackend,
};
pub use config::DataLayerConfig;
pub use database::{DatabaseDriver, SqlDatabaseConfig, SqlPoolConfig};
pub use error::DataLayerError;
pub use maintenance::{
    DatabaseMaintenanceSummary, DatabasePoolSummary, StatsDailyAggregationInput,
    StatsDailyAggregationSummary, StatsHourlyAggregationInput, StatsHourlyAggregationSummary,
    WalletDailyUsageAggregationInput, WalletDailyUsageAggregationResult,
};
pub use usage_object_store::{
    StoredUsageObject, UsageObjectStore, UsageObjectStoreConfig, UsageObjectStoreWriter,
};

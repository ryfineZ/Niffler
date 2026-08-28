use crate::database::SqlDatabaseConfig;
use crate::driver::postgres::PostgresPoolConfig;
use crate::{DataLayerError, UsageObjectStoreConfig};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DataLayerConfig {
    pub database: Option<SqlDatabaseConfig>,
    pub postgres: Option<PostgresPoolConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_object_store: Option<UsageObjectStoreConfig>,
}

impl DataLayerConfig {
    pub fn from_database(database: SqlDatabaseConfig) -> Self {
        Self {
            database: Some(database),
            postgres: None,
            usage_object_store: None,
        }
    }

    pub fn from_postgres(postgres: PostgresPoolConfig) -> Self {
        Self {
            database: Some(SqlDatabaseConfig::from_postgres_config(postgres)),
            postgres: None,
            usage_object_store: None,
        }
    }

    pub fn effective_database(&self) -> Option<SqlDatabaseConfig> {
        self.database.clone().or_else(|| {
            self.postgres
                .clone()
                .map(SqlDatabaseConfig::from_postgres_config)
        })
    }

    pub fn validate(&self) -> Result<(), DataLayerError> {
        if let Some(database) = &self.database {
            database.validate()?;
        }
        if let Some(postgres) = &self.postgres {
            postgres.validate()?;
        }
        if let Some(object_store) = &self.usage_object_store {
            object_store.validate()?;
        }
        Ok(())
    }

    pub fn has_persistent_backends(&self) -> bool {
        self.effective_database().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::DataLayerConfig;
    use crate::database::{DatabaseDriver, SqlDatabaseConfig, SqlPoolConfig};
    use crate::driver::postgres::PostgresPoolConfig;

    #[test]
    fn validates_nested_backend_configs() {
        let config = DataLayerConfig {
            database: None,
            postgres: Some(PostgresPoolConfig {
                database_url: "postgres://localhost/aether".to_string(),
                min_connections: 2,
                max_connections: 8,
                acquire_timeout_ms: 1_500,
                idle_timeout_ms: 5_000,
                max_lifetime_ms: 30_000,
                statement_cache_capacity: 64,
                require_ssl: false,
            }),
            usage_object_store: None,
        };

        assert!(config.validate().is_ok());
        assert!(config.has_persistent_backends());
    }

    #[test]
    fn rejects_invalid_nested_backend_configs() {
        let config = DataLayerConfig {
            database: None,
            postgres: Some(PostgresPoolConfig {
                database_url: String::new(),
                min_connections: 4,
                max_connections: 2,
                acquire_timeout_ms: 1_500,
                idle_timeout_ms: 5_000,
                max_lifetime_ms: 30_000,
                statement_cache_capacity: 64,
                require_ssl: false,
            }),
            usage_object_store: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn new_database_config_takes_priority_over_legacy_postgres_config() {
        let config = DataLayerConfig {
            database: Some(SqlDatabaseConfig {
                driver: DatabaseDriver::Postgres,
                url: "postgresql://primary.example/aether".to_string(),
                pool: SqlPoolConfig::default(),
            }),
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
        };

        let effective = config
            .effective_database()
            .expect("database config should exist");
        assert_eq!(effective.driver, DatabaseDriver::Postgres);
        assert_eq!(effective.url, "postgresql://primary.example/aether");
    }
}

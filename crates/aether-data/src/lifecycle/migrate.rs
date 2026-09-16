//! Runtime database migration entry points.
//!
//! Postgres uses the empty-database snapshot bootstrap before checking normal
//! migrations. The snapshot logic lives under `lifecycle::bootstrap` so this
//! module remains focused on migration execution and pending-migration
//! reporting.

use std::collections::{HashMap, HashSet};

use sqlx::{
    migrate::{Migrate, MigrateError, Migrator},
    query_scalar, PgConnection, PgPool,
};
use tracing::{error, info, warn};

#[cfg(test)]
mod tests;

static POSTGRES_MIGRATOR: Migrator = sqlx::migrate!("./migrations/postgres");
const MIGRATIONS_TABLE_EXISTS_SQL: &str =
    "SELECT to_regclass('public._sqlx_migrations') IS NOT NULL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMigrationInfo {
    pub version: i64,
    pub description: String,
}

/// Return every embedded PostgreSQL up-migration version in execution order.
///
/// Production release tooling uses this read-only manifest before replacing
/// running containers. Keeping the manifest in the compiled binary guarantees
/// the check describes the exact image that will be started.
pub fn embedded_postgres_migration_versions() -> Vec<i64> {
    POSTGRES_MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| migration.version)
        .collect()
}

/// Run all pending Postgres migrations embedded at compile time from `migrations/postgres/`.
pub async fn run_migrations(pool: &PgPool) -> Result<(), MigrateError> {
    let mut conn = pool.acquire().await?;

    if POSTGRES_MIGRATOR.locking {
        conn.lock().await?;
    }

    let result = run_migrations_locked(&mut conn).await;

    if POSTGRES_MIGRATOR.locking {
        match conn.unlock().await {
            Ok(()) => {}
            Err(unlock_error) if result.is_ok() => return Err(unlock_error),
            Err(unlock_error) => {
                warn!(
                    error = %unlock_error,
                    "database migration lock release failed after migration error"
                );
            }
        }
    }

    result
}

pub async fn pending_migrations(pool: &PgPool) -> Result<Vec<PendingMigrationInfo>, MigrateError> {
    let mut conn = pool.acquire().await?;
    pending_migrations_locked(&mut conn).await
}

pub async fn prepare_database_for_startup(
    pool: &PgPool,
) -> Result<Vec<PendingMigrationInfo>, MigrateError> {
    let mut conn = pool.acquire().await?;

    if POSTGRES_MIGRATOR.locking {
        conn.lock().await?;
    }

    let result = prepare_database_for_startup_locked(&mut conn).await;

    if POSTGRES_MIGRATOR.locking {
        match conn.unlock().await {
            Ok(()) => {}
            Err(unlock_error) if result.is_ok() => return Err(unlock_error),
            Err(unlock_error) => {
                warn!(
                    error = %unlock_error,
                    "database migration lock release failed after startup preparation error"
                );
            }
        }
    }

    result
}

async fn run_migrations_locked(conn: &mut PgConnection) -> Result<(), MigrateError> {
    conn.ensure_migrations_table().await?;
    crate::lifecycle::bootstrap::postgres::apply_snapshot_if_empty(conn, &POSTGRES_MIGRATOR)
        .await?;

    if let Some(version) = conn.dirty_version().await? {
        error!(version, "database migration state is dirty");
        return Err(MigrateError::Dirty(version));
    }

    let applied_migrations = conn.list_applied_migrations().await?;
    validate_applied_migrations(&applied_migrations)?;

    let known_versions: HashSet<_> = POSTGRES_MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| migration.version)
        .collect();
    let applied_migrations_by_version: HashMap<_, _> = applied_migrations
        .into_iter()
        .map(|migration| (migration.version, migration))
        .collect();

    let pending_migrations: Vec<_> = POSTGRES_MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .filter(|migration| !applied_migrations_by_version.contains_key(&migration.version))
        .collect();

    let total_migrations = known_versions.len();
    let applied_count = total_migrations.saturating_sub(pending_migrations.len());

    if pending_migrations.is_empty() {
        info!(
            total_migrations,
            applied_migrations = applied_count,
            pending_migrations = 0,
            "database migrations already up to date"
        );
        return Ok(());
    }

    info!(
        total_migrations,
        applied_migrations = applied_count,
        pending_migrations = pending_migrations.len(),
        "database migrations pending"
    );

    for (index, migration) in pending_migrations.iter().enumerate() {
        let current = index + 1;
        let total = pending_migrations.len();

        info!(
            current,
            total,
            version = migration.version,
            description = %migration.description,
            "applying database migration"
        );

        let elapsed = conn.apply(migration).await?;

        info!(
            current,
            total,
            version = migration.version,
            description = %migration.description,
            elapsed_ms = elapsed.as_millis() as u64,
            "applied database migration"
        );
    }

    info!(
        total_migrations,
        applied_migrations = total_migrations,
        pending_migrations = 0,
        "database migrations complete"
    );

    Ok(())
}

async fn prepare_database_for_startup_locked(
    conn: &mut PgConnection,
) -> Result<Vec<PendingMigrationInfo>, MigrateError> {
    conn.ensure_migrations_table().await?;
    crate::lifecycle::bootstrap::postgres::apply_snapshot_if_empty(conn, &POSTGRES_MIGRATOR)
        .await?;
    pending_migrations_locked(conn).await
}

async fn pending_migrations_locked(
    conn: &mut PgConnection,
) -> Result<Vec<PendingMigrationInfo>, MigrateError> {
    if !migrations_table_exists(conn).await? {
        return Ok(all_up_migrations());
    }

    if let Some(version) = conn.dirty_version().await? {
        error!(version, "database migration state is dirty");
        return Err(MigrateError::Dirty(version));
    }

    let applied_migrations = conn.list_applied_migrations().await?;
    validate_applied_migrations(&applied_migrations)?;
    Ok(pending_migrations_from_applied(&applied_migrations))
}

async fn migrations_table_exists(conn: &mut PgConnection) -> Result<bool, MigrateError> {
    let exists: bool = query_scalar(MIGRATIONS_TABLE_EXISTS_SQL)
        .fetch_one(&mut *conn)
        .await?;
    Ok(exists)
}

fn all_up_migrations() -> Vec<PendingMigrationInfo> {
    POSTGRES_MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| PendingMigrationInfo {
            version: migration.version,
            description: migration.description.to_string(),
        })
        .collect()
}

fn pending_migrations_from_applied(
    applied_migrations: &[sqlx::migrate::AppliedMigration],
) -> Vec<PendingMigrationInfo> {
    pending_migrations_from_applied_for(&POSTGRES_MIGRATOR, applied_migrations)
}

fn pending_migrations_from_applied_for(
    migrator: &'static Migrator,
    applied_migrations: &[sqlx::migrate::AppliedMigration],
) -> Vec<PendingMigrationInfo> {
    let applied_versions: HashSet<_> = applied_migrations
        .iter()
        .map(|migration| migration.version)
        .collect();

    migrator
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .filter(|migration| !applied_versions.contains(&migration.version))
        .map(|migration| PendingMigrationInfo {
            version: migration.version,
            description: migration.description.to_string(),
        })
        .collect()
}

fn validate_applied_migrations(
    applied_migrations: &[sqlx::migrate::AppliedMigration],
) -> Result<(), MigrateError> {
    if POSTGRES_MIGRATOR.ignore_missing {
        return Ok(());
    }

    let known_versions: HashSet<_> = POSTGRES_MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .collect();

    for applied_migration in applied_migrations {
        if !known_versions.contains(&applied_migration.version) {
            error!(
                version = applied_migration.version,
                "applied database migration is missing from embedded migrations"
            );
            return Err(MigrateError::VersionMissing(applied_migration.version));
        }
    }

    // Checksum drift is reported as a warning only. Strict enforcement makes
    // harmless edits (comment tweaks, whitespace, metadata fixes) impossible
    // without also touching every environment's migration history table. We
    // match by version alone — sqlx still skips already-applied migrations so
    // edited files will not re-run, they are merely allowed to exist.
    for migration in POSTGRES_MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
    {
        if let Some(applied_migration) = applied_migrations
            .iter()
            .find(|applied_migration| applied_migration.version == migration.version)
        {
            if migration.checksum != applied_migration.checksum {
                warn!(
                    version = migration.version,
                    description = %migration.description,
                    "database migration checksum mismatch (ignored: version-only validation)"
                );
            }
        }
    }

    Ok(())
}

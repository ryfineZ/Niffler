use futures_util::TryStreamExt;
use sqlx::Row;

use super::PostgresBackend;
use crate::error::SqlxResultExt;
use crate::repository::system::{
    AdminSystemPurgeSummary, AdminSystemPurgeTarget, AdminSystemStats, StoredSystemConfigEntry,
};
use crate::{DataLayerError, UsageObjectStore};

const POSTGRES_FIND_SYSTEM_CONFIG_VALUE_SQL: &str = r#"
SELECT value
FROM system_configs
WHERE key = $1
LIMIT 1
"#;

const POSTGRES_UPSERT_SYSTEM_CONFIG_VALUE_SQL: &str = r#"
INSERT INTO system_configs (id, key, value, description, created_at, updated_at)
VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value,
    description = COALESCE(EXCLUDED.description, system_configs.description),
    updated_at = NOW()
RETURNING value
"#;

const POSTGRES_LIST_SYSTEM_CONFIG_ENTRIES_SQL: &str = r#"
SELECT
    key,
    value,
    description,
    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix_secs
FROM system_configs
ORDER BY key ASC
"#;

const POSTGRES_UPSERT_SYSTEM_CONFIG_ENTRY_SQL: &str = r#"
INSERT INTO system_configs (id, key, value, description, created_at, updated_at)
VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value,
    description = COALESCE(EXCLUDED.description, system_configs.description),
    updated_at = NOW()
RETURNING
    key,
    value,
    description,
    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix_secs
"#;

const POSTGRES_DELETE_SYSTEM_CONFIG_VALUE_SQL: &str = r#"
DELETE FROM system_configs
WHERE key = $1
"#;

const POSTGRES_READ_ADMIN_SYSTEM_STATS_SQL: &str = r#"
SELECT
    (SELECT COUNT(*) FROM users) AS total_users,
    (SELECT COUNT(*) FROM users WHERE is_active IS TRUE) AS active_users,
    (SELECT COUNT(*) FROM api_keys) AS total_api_keys,
    (SELECT COUNT(*) FROM usage) AS total_requests
"#;

impl PostgresBackend {
    pub async fn purge_admin_system_data(
        &self,
        target: AdminSystemPurgeTarget,
    ) -> Result<AdminSystemPurgeSummary, DataLayerError> {
        let mut tx = self.pool().begin().await.map_postgres_err()?;
        let mut summary = AdminSystemPurgeSummary::default();
        let object_store = self.usage_object_store();
        purge_postgres_admin_system_data(&mut tx, target, object_store.as_deref(), &mut summary)
            .await?;
        tx.commit().await.map_postgres_err()?;
        Ok(summary)
    }

    pub async fn purge_admin_request_bodies_batch(
        &self,
        batch_size: usize,
    ) -> Result<AdminSystemPurgeSummary, DataLayerError> {
        if batch_size == 0 {
            return Ok(AdminSystemPurgeSummary::default());
        }
        let mut tx = self.pool().begin().await.map_postgres_err()?;
        let mut summary = AdminSystemPurgeSummary::default();
        let object_store = self.usage_object_store();
        purge_postgres_request_bodies_batch(
            &mut tx,
            batch_size,
            object_store.as_deref(),
            &mut summary,
        )
        .await?;
        tx.commit().await.map_postgres_err()?;
        Ok(summary)
    }

    pub async fn find_system_config_value(
        &self,
        key: &str,
    ) -> Result<Option<serde_json::Value>, DataLayerError> {
        let row = sqlx::query(POSTGRES_FIND_SYSTEM_CONFIG_VALUE_SQL)
            .bind(key)
            .fetch_optional(self.pool())
            .await
            .map_postgres_err()?;
        row.map(|row| row.try_get("value"))
            .transpose()
            .map_postgres_err()
    }

    pub async fn upsert_system_config_value(
        &self,
        key: &str,
        value: &serde_json::Value,
        description: Option<&str>,
    ) -> Result<serde_json::Value, DataLayerError> {
        let row = sqlx::query(POSTGRES_UPSERT_SYSTEM_CONFIG_VALUE_SQL)
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(key)
            .bind(value)
            .bind(description)
            .fetch_one(self.pool())
            .await
            .map_postgres_err()?;
        row.try_get("value").map_postgres_err()
    }

    pub async fn list_system_config_entries(
        &self,
    ) -> Result<Vec<StoredSystemConfigEntry>, DataLayerError> {
        let mut rows = sqlx::query(POSTGRES_LIST_SYSTEM_CONFIG_ENTRIES_SQL).fetch(self.pool());
        let mut entries = Vec::new();
        while let Some(row) = rows.try_next().await.map_postgres_err()? {
            entries.push(StoredSystemConfigEntry {
                key: row.try_get("key").map_postgres_err()?,
                value: row.try_get("value").map_postgres_err()?,
                description: row.try_get("description").map_postgres_err()?,
                updated_at_unix_secs: row
                    .try_get::<Option<i64>, _>("updated_at_unix_secs")
                    .map_postgres_err()?
                    .map(|value| value.max(0) as u64),
            });
        }
        Ok(entries)
    }

    pub async fn upsert_system_config_entry(
        &self,
        key: &str,
        value: &serde_json::Value,
        description: Option<&str>,
    ) -> Result<StoredSystemConfigEntry, DataLayerError> {
        let row = sqlx::query(POSTGRES_UPSERT_SYSTEM_CONFIG_ENTRY_SQL)
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(key)
            .bind(value)
            .bind(description)
            .fetch_one(self.pool())
            .await
            .map_postgres_err()?;
        Ok(StoredSystemConfigEntry {
            key: row.try_get("key").map_postgres_err()?,
            value: row.try_get("value").map_postgres_err()?,
            description: row.try_get("description").map_postgres_err()?,
            updated_at_unix_secs: row
                .try_get::<Option<i64>, _>("updated_at_unix_secs")
                .map_postgres_err()?
                .map(|value| value.max(0) as u64),
        })
    }

    pub async fn delete_system_config_value(&self, key: &str) -> Result<bool, DataLayerError> {
        let result = sqlx::query(POSTGRES_DELETE_SYSTEM_CONFIG_VALUE_SQL)
            .bind(key)
            .execute(self.pool())
            .await
            .map_postgres_err()?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn read_admin_system_stats(&self) -> Result<AdminSystemStats, DataLayerError> {
        let row = sqlx::query(POSTGRES_READ_ADMIN_SYSTEM_STATS_SQL)
            .fetch_one(self.pool())
            .await
            .map_postgres_err()?;
        postgres_admin_system_stats(row)
    }
}
const ADMIN_CONFIG_PURGE_TABLES: &[&str] = &[
    "api_key_provider_mappings",
    "gemini_file_mappings",
    "provider_usage_tracking",
    "billing_rules",
    "dimension_collectors",
    "models",
    "provider_endpoints",
    "provider_api_keys",
    "providers",
    "global_models",
    "proxy_node_events",
    "proxy_nodes",
    "user_oauth_links",
    "ldap_configs",
    "oauth_providers",
    "auth_modules",
    "system_configs",
];

const ADMIN_STATS_PURGE_TABLES: &[&str] = &[
    "stats_user_daily_cost_savings_model_provider",
    "stats_user_daily_cost_savings_model",
    "stats_user_daily_cost_savings_provider",
    "stats_user_daily_cost_savings",
    "stats_daily_cost_savings_model_provider",
    "stats_daily_cost_savings_model",
    "stats_daily_cost_savings_provider",
    "stats_daily_cost_savings",
    "stats_user_daily_model_provider",
    "stats_daily_model_provider",
    "stats_user_daily_api_format",
    "stats_user_daily_provider",
    "stats_hourly_user_model",
    "stats_user_daily_model",
    "stats_user_summary",
    "stats_hourly_user",
    "stats_user_daily",
    "stats_daily_api_key",
    "stats_daily_error",
    "stats_daily_model",
    "stats_daily_provider",
    "stats_hourly_model",
    "stats_hourly_provider",
    "stats_summary",
    "stats_hourly",
    "stats_daily",
];

const ADMIN_USAGE_CHILD_TABLES: &[&str] = &[
    "usage_body_objects",
    "usage_body_blobs",
    "usage_http_audits",
    "usage_routing_snapshots",
    "usage_settlement_snapshots",
];

const USAGE_BODY_FIELD_COLUMNS: &[&str] = &[
    "request_body",
    "response_body",
    "provider_request_body",
    "client_response_body",
    "request_body_compressed",
    "response_body_compressed",
    "provider_request_body_compressed",
    "client_response_body_compressed",
];

const ADMIN_USER_SCOPED_TABLES: &[&str] = &[
    "stats_user_daily_cost_savings_model_provider",
    "stats_user_daily_cost_savings_model",
    "stats_user_daily_cost_savings_provider",
    "stats_user_daily_cost_savings",
    "stats_user_daily_model_provider",
    "stats_user_daily_api_format",
    "stats_user_daily_provider",
    "stats_hourly_user_model",
    "stats_user_daily_model",
    "stats_user_summary",
    "stats_hourly_user",
    "stats_user_daily",
    "user_model_usage_counts",
    "announcement_reads",
    "management_tokens",
    "user_preferences",
    "user_sessions",
    "user_oauth_links",
];

fn checked_sql_identifier(value: &str) -> Result<&str, DataLayerError> {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        Ok(value)
    } else {
        Err(DataLayerError::InvalidInput(format!(
            "invalid SQL identifier: {value}"
        )))
    }
}

async fn purge_postgres_admin_system_data(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    target: AdminSystemPurgeTarget,
    object_store: Option<&UsageObjectStore>,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<(), DataLayerError> {
    match target {
        AdminSystemPurgeTarget::Config => {
            pg_execute_if_table(
                tx,
                "usage",
                "usage_provider_refs_cleared",
                r#"
UPDATE public.usage
SET provider_id = NULL,
    provider_endpoint_id = NULL,
    provider_api_key_id = NULL
WHERE provider_id IS NOT NULL
   OR provider_endpoint_id IS NOT NULL
   OR provider_api_key_id IS NOT NULL
"#,
                summary,
            )
            .await?;
            pg_execute_if_table(
                tx,
                "request_candidates",
                "request_candidate_provider_refs_cleared",
                r#"
UPDATE public.request_candidates
SET provider_id = NULL,
    endpoint_id = NULL,
    key_id = NULL
WHERE provider_id IS NOT NULL
   OR endpoint_id IS NOT NULL
   OR key_id IS NOT NULL
"#,
                summary,
            )
            .await?;
            pg_execute_if_table(
                tx,
                "video_tasks",
                "video_task_provider_refs_cleared",
                r#"
UPDATE public.video_tasks
SET provider_id = NULL,
    endpoint_id = NULL,
    key_id = NULL
WHERE provider_id IS NOT NULL
   OR endpoint_id IS NOT NULL
   OR key_id IS NOT NULL
"#,
                summary,
            )
            .await?;
            pg_execute_if_table(
                tx,
                "user_preferences",
                "user_default_provider_refs_cleared",
                "UPDATE public.user_preferences SET default_provider_id = NULL WHERE default_provider_id IS NOT NULL",
                summary,
            )
            .await?;
            for table in ADMIN_CONFIG_PURGE_TABLES {
                pg_delete_table(tx, table, summary).await?;
            }
        }
        AdminSystemPurgeTarget::Users => {
            purge_postgres_non_admin_users(tx, summary).await?;
        }
        AdminSystemPurgeTarget::Usage => {
            while purge_postgres_usage_body_objects_batch(tx, 500, object_store, summary).await?
                == 500
            {}
            pg_delete_table(tx, "request_candidates", summary).await?;
            for table in ADMIN_USAGE_CHILD_TABLES {
                pg_delete_table(tx, table, summary).await?;
            }
            pg_delete_table(tx, "usage", summary).await?;
            pg_execute_if_table(
                tx,
                "api_keys",
                "api_key_usage_stats_reset",
                r#"
UPDATE public.api_keys
SET total_requests = 0,
    total_tokens = 0,
    total_cost_usd = 0,
    last_used_at = NULL
WHERE total_requests <> 0
   OR total_tokens <> 0
   OR total_cost_usd <> 0
   OR last_used_at IS NOT NULL
"#,
                summary,
            )
            .await?;
            pg_execute_if_table(
                tx,
                "provider_api_keys",
                "provider_key_usage_stats_reset",
                r#"
UPDATE public.provider_api_keys
SET request_count = 0,
    success_count = 0,
    error_count = 0,
    total_tokens = 0,
    total_cost_usd = 0,
    total_response_time_ms = 0,
    last_used_at = NULL
WHERE request_count <> 0
   OR success_count <> 0
   OR error_count <> 0
   OR total_tokens <> 0
   OR total_cost_usd <> 0
   OR total_response_time_ms <> 0
   OR last_used_at IS NOT NULL
"#,
                summary,
            )
            .await?;
        }
        AdminSystemPurgeTarget::AuditLogs => {
            pg_delete_table(tx, "audit_logs", summary).await?;
        }
        AdminSystemPurgeTarget::RequestBodies => {
            while purge_postgres_usage_body_objects_batch(tx, 500, object_store, summary).await?
                == 500
            {}
            pg_delete_table(tx, "usage_body_blobs", summary).await?;
            pg_execute_if_table_has_columns(
                tx,
                "usage",
                USAGE_BODY_FIELD_COLUMNS,
                "usage_body_fields_cleaned",
                r#"
UPDATE public.usage
SET request_body = NULL,
    response_body = NULL,
    provider_request_body = NULL,
    client_response_body = NULL,
    request_body_compressed = NULL,
    response_body_compressed = NULL,
    provider_request_body_compressed = NULL,
    client_response_body_compressed = NULL
WHERE request_body IS NOT NULL
   OR response_body IS NOT NULL
   OR provider_request_body IS NOT NULL
   OR client_response_body IS NOT NULL
   OR request_body_compressed IS NOT NULL
   OR response_body_compressed IS NOT NULL
   OR provider_request_body_compressed IS NOT NULL
   OR client_response_body_compressed IS NOT NULL
"#,
                summary,
            )
            .await?;
            pg_execute_if_table(
                tx,
                "usage_http_audits",
                "usage_http_audit_body_refs_cleaned",
                r#"
UPDATE public.usage_http_audits
SET request_body_ref = NULL,
    provider_request_body_ref = NULL,
    response_body_ref = NULL,
    client_response_body_ref = NULL,
    request_body_state = NULL,
    provider_request_body_state = NULL,
    response_body_state = NULL,
    client_response_body_state = NULL,
    body_capture_mode = 'none',
    updated_at = NOW()
WHERE request_body_ref IS NOT NULL
   OR provider_request_body_ref IS NOT NULL
   OR response_body_ref IS NOT NULL
   OR client_response_body_ref IS NOT NULL
   OR request_body_state IS NOT NULL
   OR provider_request_body_state IS NOT NULL
   OR response_body_state IS NOT NULL
   OR client_response_body_state IS NOT NULL
   OR body_capture_mode <> 'none'
"#,
                summary,
            )
            .await?;
        }
        AdminSystemPurgeTarget::Stats => {
            for table in ADMIN_STATS_PURGE_TABLES {
                pg_delete_table(tx, table, summary).await?;
            }
        }
    }
    Ok(())
}

async fn purge_postgres_non_admin_users(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<(), DataLayerError> {
    let non_admin_users = "SELECT id FROM public.users WHERE COALESCE(role::text, '') <> 'admin'";
    let non_admin_keys = "SELECT id FROM public.api_keys WHERE user_id IN (SELECT id FROM public.users WHERE COALESCE(role::text, '') <> 'admin')";

    pg_execute_if_table(
        tx,
        "api_key_provider_mappings",
        "api_key_provider_mappings",
        &format!(
            "DELETE FROM public.api_key_provider_mappings WHERE api_key_id IN ({non_admin_keys})"
        ),
        summary,
    )
    .await?;
    pg_execute_if_table(
        tx,
        "usage",
        "usage_user_refs_cleared",
        &format!(
            r#"
UPDATE public.usage
SET user_id = CASE WHEN user_id IN ({non_admin_users}) THEN NULL ELSE user_id END,
    api_key_id = CASE WHEN api_key_id IN ({non_admin_keys}) THEN NULL ELSE api_key_id END
WHERE user_id IN ({non_admin_users})
   OR api_key_id IN ({non_admin_keys})
"#
        ),
        summary,
    )
    .await?;
    pg_execute_if_table(
        tx,
        "request_candidates",
        "request_candidate_user_refs_cleared",
        &format!(
            r#"
UPDATE public.request_candidates
SET user_id = CASE WHEN user_id IN ({non_admin_users}) THEN NULL ELSE user_id END,
    api_key_id = CASE WHEN api_key_id IN ({non_admin_keys}) THEN NULL ELSE api_key_id END
WHERE user_id IN ({non_admin_users})
   OR api_key_id IN ({non_admin_keys})
"#
        ),
        summary,
    )
    .await?;
    pg_execute_if_table(
        tx,
        "audit_logs",
        "audit_log_user_refs_cleared",
        &format!(
            r#"
UPDATE public.audit_logs
SET user_id = CASE WHEN user_id IN ({non_admin_users}) THEN NULL ELSE user_id END,
    api_key_id = CASE WHEN api_key_id IN ({non_admin_keys}) THEN NULL ELSE api_key_id END
WHERE user_id IN ({non_admin_users})
   OR api_key_id IN ({non_admin_keys})
"#
        ),
        summary,
    )
    .await?;
    pg_execute_if_table(
        tx,
        "video_tasks",
        "video_task_user_refs_cleared",
        &format!(
            r#"
UPDATE public.video_tasks
SET user_id = CASE WHEN user_id IN ({non_admin_users}) THEN NULL ELSE user_id END,
    api_key_id = CASE WHEN api_key_id IN ({non_admin_keys}) THEN NULL ELSE api_key_id END
WHERE user_id IN ({non_admin_users})
   OR api_key_id IN ({non_admin_keys})
"#
        ),
        summary,
    )
    .await?;
    pg_execute_if_table(
        tx,
        "wallets",
        "wallet_user_refs_cleared",
        &format!(
            r#"
UPDATE public.wallets
SET user_id = CASE WHEN user_id IN ({non_admin_users}) THEN NULL ELSE user_id END,
    api_key_id = CASE WHEN api_key_id IN ({non_admin_keys}) THEN NULL ELSE api_key_id END
WHERE user_id IN ({non_admin_users})
   OR api_key_id IN ({non_admin_keys})
"#
        ),
        summary,
    )
    .await?;
    for (table, column, key) in [
        (
            "wallet_transactions",
            "operator_id",
            "wallet_transaction_operator_refs_cleared",
        ),
        (
            "payment_orders",
            "user_id",
            "payment_order_user_refs_cleared",
        ),
        (
            "announcements",
            "author_id",
            "announcement_author_refs_cleared",
        ),
        (
            "proxy_nodes",
            "registered_by",
            "proxy_node_registrant_refs_cleared",
        ),
    ] {
        pg_execute_if_table(
            tx,
            table,
            key,
            &format!(
                "UPDATE public.\"{table}\" SET {column} = NULL WHERE {column} IN ({non_admin_users})"
            ),
            summary,
        )
        .await?;
    }
    pg_execute_if_table(
        tx,
        "refund_requests",
        "refund_request_user_refs_cleared",
        &format!(
            r#"
UPDATE public.refund_requests
SET user_id = CASE WHEN user_id IN ({non_admin_users}) THEN NULL ELSE user_id END,
    requested_by = CASE WHEN requested_by IN ({non_admin_users}) THEN NULL ELSE requested_by END,
    approved_by = CASE WHEN approved_by IN ({non_admin_users}) THEN NULL ELSE approved_by END,
    processed_by = CASE WHEN processed_by IN ({non_admin_users}) THEN NULL ELSE processed_by END
WHERE user_id IN ({non_admin_users})
   OR requested_by IN ({non_admin_users})
   OR approved_by IN ({non_admin_users})
   OR processed_by IN ({non_admin_users})
"#
        ),
        summary,
    )
    .await?;
    pg_delete_non_admin_api_key_rows(tx, "stats_daily_api_key", summary).await?;
    for table in ADMIN_USER_SCOPED_TABLES {
        pg_delete_non_admin_user_rows(tx, table, summary).await?;
    }
    pg_execute_if_table(
        tx,
        "api_keys",
        "api_keys",
        &format!("DELETE FROM public.api_keys WHERE user_id IN ({non_admin_users})"),
        summary,
    )
    .await?;
    pg_execute_if_table(
        tx,
        "users",
        "users",
        "DELETE FROM public.users WHERE COALESCE(role::text, '') <> 'admin'",
        summary,
    )
    .await?;
    Ok(())
}

async fn purge_postgres_request_bodies_batch(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    batch_size: usize,
    object_store: Option<&UsageObjectStore>,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<(), DataLayerError> {
    let limit = i64::try_from(batch_size).unwrap_or(i64::MAX);
    purge_postgres_usage_body_objects_batch(tx, batch_size, object_store, summary).await?;
    pg_execute_batch_if_table(
        tx,
        "usage_body_blobs",
        "usage_body_blobs",
        r#"
WITH doomed AS (
    SELECT body_ref
    FROM public.usage_body_blobs
    ORDER BY body_ref ASC
    LIMIT $1
)
DELETE FROM public.usage_body_blobs AS blobs
USING doomed
WHERE blobs.body_ref = doomed.body_ref
"#,
        summary,
        limit,
    )
    .await?;
    pg_execute_batch_if_table_has_columns(
        tx,
        "usage",
        USAGE_BODY_FIELD_COLUMNS,
        "usage_body_fields_cleaned",
        r#"
WITH batch AS (
    SELECT request_id
    FROM public.usage
    WHERE request_body IS NOT NULL
       OR response_body IS NOT NULL
       OR provider_request_body IS NOT NULL
       OR client_response_body IS NOT NULL
       OR request_body_compressed IS NOT NULL
       OR response_body_compressed IS NOT NULL
       OR provider_request_body_compressed IS NOT NULL
       OR client_response_body_compressed IS NOT NULL
    ORDER BY created_at_unix_ms ASC, request_id ASC
    LIMIT $1
)
UPDATE public.usage AS usage_rows
SET request_body = NULL,
    response_body = NULL,
    provider_request_body = NULL,
    client_response_body = NULL,
    request_body_compressed = NULL,
    response_body_compressed = NULL,
    provider_request_body_compressed = NULL,
    client_response_body_compressed = NULL
FROM batch
WHERE usage_rows.request_id = batch.request_id
"#,
        summary,
        limit,
    )
    .await?;
    pg_execute_batch_if_table(
        tx,
        "usage_http_audits",
        "usage_http_audit_body_refs_cleaned",
        r#"
WITH batch AS (
    SELECT request_id
    FROM public.usage_http_audits
    WHERE request_body_ref IS NOT NULL
       OR provider_request_body_ref IS NOT NULL
       OR response_body_ref IS NOT NULL
       OR client_response_body_ref IS NOT NULL
       OR request_body_state IS NOT NULL
       OR provider_request_body_state IS NOT NULL
       OR response_body_state IS NOT NULL
       OR client_response_body_state IS NOT NULL
       OR body_capture_mode <> 'none'
    ORDER BY request_id ASC
    LIMIT $1
)
UPDATE public.usage_http_audits AS audits
SET request_body_ref = NULL,
    provider_request_body_ref = NULL,
    response_body_ref = NULL,
    client_response_body_ref = NULL,
    request_body_state = NULL,
    provider_request_body_state = NULL,
    response_body_state = NULL,
    client_response_body_state = NULL,
    body_capture_mode = 'none',
    updated_at = NOW()
FROM batch
WHERE audits.request_id = batch.request_id
"#,
        summary,
        limit,
    )
    .await?;
    Ok(())
}

async fn purge_postgres_usage_body_objects_batch(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    batch_size: usize,
    object_store: Option<&UsageObjectStore>,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<usize, DataLayerError> {
    if batch_size == 0 || !pg_table_exists(tx, "usage_body_objects").await? {
        return Ok(0);
    }

    let rows = sqlx::query(
        r#"
SELECT body_ref, object_key, storage_status
FROM public.usage_body_objects
ORDER BY body_ref ASC
LIMIT $1
"#,
    )
    .bind(i64::try_from(batch_size).unwrap_or(i64::MAX))
    .fetch_all(&mut **tx)
    .await
    .map_postgres_err()?;
    if rows.is_empty() {
        return Ok(0);
    }

    let mut body_refs = Vec::with_capacity(rows.len());
    for row in rows {
        let body_ref = row.try_get::<String, _>("body_ref").map_postgres_err()?;
        let object_key = row
            .try_get::<Option<String>, _>("object_key")
            .map_postgres_err()?
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let storage_status = row
            .try_get::<String, _>("storage_status")
            .map_postgres_err()?;
        if let Some(object_key) = object_key {
            let object_store = object_store.ok_or_else(|| {
                DataLayerError::UnexpectedValue(format!(
                    "cannot purge usage body object {body_ref}: usage object store is not configured"
                ))
            })?;
            object_store.delete(&object_key).await?;
        } else if storage_status == "available" {
            return Err(DataLayerError::UnexpectedValue(format!(
                "cannot purge usage body object {body_ref}: available object has no object key"
            )));
        }
        body_refs.push(body_ref);
    }

    let deleted = sqlx::query("DELETE FROM public.usage_body_objects WHERE body_ref = ANY($1)")
        .bind(&body_refs)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?
        .rows_affected();
    summary.add("usage_body_objects", deleted);
    Ok(usize::try_from(deleted).unwrap_or(usize::MAX))
}

async fn pg_delete_table(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<(), DataLayerError> {
    let table = checked_sql_identifier(table)?;
    if !pg_table_exists(tx, table).await? {
        return Ok(());
    }
    let sql = format!("DELETE FROM public.\"{table}\"");
    let rows = sqlx::query(&sql)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?
        .rows_affected();
    summary.add(table, rows);
    Ok(())
}

async fn pg_delete_non_admin_user_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<(), DataLayerError> {
    let table = checked_sql_identifier(table)?;
    if !pg_table_exists(tx, table).await? {
        return Ok(());
    }
    let sql = format!(
        "DELETE FROM public.\"{table}\" WHERE user_id IN (SELECT id FROM public.users WHERE COALESCE(role::text, '') <> 'admin')"
    );
    let rows = sqlx::query(&sql)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?
        .rows_affected();
    summary.add(table, rows);
    Ok(())
}

async fn pg_delete_non_admin_api_key_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<(), DataLayerError> {
    let table = checked_sql_identifier(table)?;
    if !pg_table_exists(tx, table).await? {
        return Ok(());
    }
    let sql = format!(
        "DELETE FROM public.\"{table}\" WHERE api_key_id IN (SELECT id FROM public.api_keys WHERE user_id IN (SELECT id FROM public.users WHERE COALESCE(role::text, '') <> 'admin'))"
    );
    let rows = sqlx::query(&sql)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?
        .rows_affected();
    summary.add(table, rows);
    Ok(())
}

async fn pg_execute_if_table(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    key: &str,
    sql: &str,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<(), DataLayerError> {
    if !pg_table_exists(tx, checked_sql_identifier(table)?).await? {
        return Ok(());
    }
    let rows = sqlx::query(sql)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?
        .rows_affected();
    summary.add(key, rows);
    Ok(())
}

async fn pg_execute_if_table_has_columns(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    columns: &[&str],
    key: &str,
    sql: &str,
    summary: &mut AdminSystemPurgeSummary,
) -> Result<(), DataLayerError> {
    if !pg_table_has_columns(tx, checked_sql_identifier(table)?, columns).await? {
        return Ok(());
    }
    let rows = sqlx::query(sql)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?
        .rows_affected();
    summary.add(key, rows);
    Ok(())
}

async fn pg_execute_batch_if_table(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    key: &str,
    sql: &str,
    summary: &mut AdminSystemPurgeSummary,
    limit: i64,
) -> Result<(), DataLayerError> {
    if !pg_table_exists(tx, checked_sql_identifier(table)?).await? {
        return Ok(());
    }
    let rows = sqlx::query(sql)
        .bind(limit)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?
        .rows_affected();
    summary.add(key, rows);
    Ok(())
}

async fn pg_execute_batch_if_table_has_columns(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    columns: &[&str],
    key: &str,
    sql: &str,
    summary: &mut AdminSystemPurgeSummary,
    limit: i64,
) -> Result<(), DataLayerError> {
    if !pg_table_has_columns(tx, checked_sql_identifier(table)?, columns).await? {
        return Ok(());
    }
    let rows = sqlx::query(sql)
        .bind(limit)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?
        .rows_affected();
    summary.add(key, rows);
    Ok(())
}

async fn pg_table_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
) -> Result<bool, DataLayerError> {
    let table = checked_sql_identifier(table)?;
    sqlx::query_scalar::<_, bool>("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("public.{table}"))
        .fetch_one(&mut **tx)
        .await
        .map_postgres_err()
}

async fn pg_table_has_columns(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    columns: &[&str],
) -> Result<bool, DataLayerError> {
    let table = checked_sql_identifier(table)?;
    if !pg_table_exists(tx, table).await? {
        return Ok(false);
    }
    for column in columns {
        let column = checked_sql_identifier(column)?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = 'public'
                  AND table_name = $1
                  AND column_name = $2
            )",
        )
        .bind(table)
        .bind(column)
        .fetch_one(&mut **tx)
        .await
        .map_postgres_err()?;
        if !exists {
            return Ok(false);
        }
    }
    Ok(true)
}

fn postgres_admin_system_stats(
    row: sqlx::postgres::PgRow,
) -> Result<AdminSystemStats, DataLayerError> {
    Ok(AdminSystemStats {
        total_users: row
            .try_get::<i64, _>("total_users")
            .map_postgres_err()?
            .max(0) as u64,
        active_users: row
            .try_get::<i64, _>("active_users")
            .map_postgres_err()?
            .max(0) as u64,
        total_api_keys: row
            .try_get::<i64, _>("total_api_keys")
            .map_postgres_err()?
            .max(0) as u64,
        total_requests: row
            .try_get::<i64, _>("total_requests")
            .map_postgres_err()?
            .max(0) as u64,
    })
}

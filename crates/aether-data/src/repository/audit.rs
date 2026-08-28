use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use serde_json::Value;
use sqlx::{postgres::PgRow, Row};

use crate::driver::postgres::PostgresPool;
use crate::error::SqlxResultExt;
use crate::repository::auth::ResolvedAuthApiKeySnapshot;
use crate::repository::candidates::DecisionTrace;
use crate::repository::usage::StoredRequestUsageAudit;
use crate::DataLayerError;

const SUSPICIOUS_EVENT_TYPES: &[&str] = &[
    "suspicious_activity",
    "unauthorized_access",
    "login_failed",
    "request_rate_limited",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditLogListQuery {
    pub cutoff_unix_secs: u64,
    pub username_pattern: Option<String>,
    pub event_type: Option<String>,
    pub limit: usize,
    pub offset: usize,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredAdminAuditLog {
    pub id: String,
    pub event_type: String,
    pub user_id: Option<String>,
    pub user_email: Option<String>,
    pub user_username: Option<String>,
    pub description: Option<String>,
    pub ip_address: Option<String>,
    pub status_code: Option<i32>,
    pub error_message: Option<String>,
    pub metadata: Option<Value>,
    pub created_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredSuspiciousActivity {
    pub id: String,
    pub event_type: String,
    pub user_id: Option<String>,
    pub description: Option<String>,
    pub ip_address: Option<String>,
    pub metadata: Option<Value>,
    pub created_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredUserAuditLog {
    pub id: String,
    pub event_type: String,
    pub description: Option<String>,
    pub ip_address: Option<String>,
    pub status_code: Option<i32>,
    pub created_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredAdminAuditLogPage {
    pub items: Vec<StoredAdminAuditLog>,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredUserAuditLogPage {
    pub items: Vec<StoredUserAuditLog>,
    pub total: u64,
}

#[async_trait]
pub trait AuditLogReadRepository: Send + Sync {
    async fn list_admin_audit_logs(
        &self,
        query: &AuditLogListQuery,
    ) -> Result<StoredAdminAuditLogPage, DataLayerError>;

    async fn list_admin_suspicious_activities(
        &self,
        cutoff_unix_secs: u64,
    ) -> Result<Vec<StoredSuspiciousActivity>, DataLayerError>;

    async fn read_admin_user_behavior_event_counts(
        &self,
        user_id: &str,
        cutoff_unix_secs: u64,
    ) -> Result<std::collections::BTreeMap<String, u64>, DataLayerError>;

    async fn list_user_audit_logs(
        &self,
        user_id: &str,
        query: &AuditLogListQuery,
    ) -> Result<StoredUserAuditLogPage, DataLayerError>;

    async fn delete_audit_logs_before(
        &self,
        cutoff_unix_secs: u64,
        limit: usize,
    ) -> Result<usize, DataLayerError>;
}

#[derive(Debug, Clone)]
pub struct PostgresAuditLogReadRepository {
    pool: PostgresPool,
}

impl PostgresAuditLogReadRepository {
    pub fn new(pool: PostgresPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuditLogReadRepository for PostgresAuditLogReadRepository {
    async fn list_admin_audit_logs(
        &self,
        query: &AuditLogListQuery,
    ) -> Result<StoredAdminAuditLogPage, DataLayerError> {
        let cutoff_time = postgres_cutoff_time(query.cutoff_unix_secs);
        let total = sqlx::query_scalar::<_, i64>(
            r#"
SELECT COUNT(*)
FROM audit_logs AS a
LEFT JOIN users AS u ON a.user_id = u.id
WHERE a.created_at >= $1
  AND ($2::text IS NULL OR u.username ILIKE $2 ESCAPE '\')
  AND ($3::text IS NULL OR a.event_type = $3)
"#,
        )
        .bind(cutoff_time)
        .bind(query.username_pattern.as_deref())
        .bind(query.event_type.as_deref())
        .fetch_one(&self.pool)
        .await
        .map_postgres_err()?;

        let mut rows = sqlx::query(
            r#"
SELECT
  a.id,
  a.event_type,
  a.user_id,
  u.email AS user_email,
  u.username AS user_username,
  a.description,
  a.ip_address,
  a.status_code,
  a.error_message,
  a.event_metadata AS metadata,
  a.created_at
FROM audit_logs AS a
LEFT JOIN users AS u ON a.user_id = u.id
WHERE a.created_at >= $1
  AND ($2::text IS NULL OR u.username ILIKE $2 ESCAPE '\')
  AND ($3::text IS NULL OR a.event_type = $3)
ORDER BY a.created_at DESC
LIMIT $4 OFFSET $5
"#,
        )
        .bind(cutoff_time)
        .bind(query.username_pattern.as_deref())
        .bind(query.event_type.as_deref())
        .bind(i64::try_from(query.limit).unwrap_or(i64::MAX))
        .bind(i64::try_from(query.offset).unwrap_or(i64::MAX))
        .fetch(&self.pool);

        let mut items = Vec::new();
        while let Some(row) = rows.try_next().await.map_postgres_err()? {
            items.push(map_postgres_admin_audit_log_row(&row)?);
        }

        Ok(StoredAdminAuditLogPage {
            items,
            total: total.max(0) as u64,
        })
    }

    async fn list_admin_suspicious_activities(
        &self,
        cutoff_unix_secs: u64,
    ) -> Result<Vec<StoredSuspiciousActivity>, DataLayerError> {
        let cutoff_time = postgres_cutoff_time(cutoff_unix_secs);
        let mut rows = sqlx::query(
            r#"
SELECT
  id,
  event_type,
  user_id,
  description,
  ip_address,
  event_metadata AS metadata,
  created_at
FROM audit_logs
WHERE created_at >= $1
  AND event_type = ANY($2)
ORDER BY created_at DESC
LIMIT 100
"#,
        )
        .bind(cutoff_time)
        .bind(SUSPICIOUS_EVENT_TYPES.to_vec())
        .fetch(&self.pool);

        let mut items = Vec::new();
        while let Some(row) = rows.try_next().await.map_postgres_err()? {
            items.push(map_postgres_suspicious_activity_row(&row)?);
        }
        Ok(items)
    }

    async fn read_admin_user_behavior_event_counts(
        &self,
        user_id: &str,
        cutoff_unix_secs: u64,
    ) -> Result<std::collections::BTreeMap<String, u64>, DataLayerError> {
        let cutoff_time = postgres_cutoff_time(cutoff_unix_secs);
        let mut rows = sqlx::query(
            r#"
SELECT event_type, COUNT(*)::bigint AS count
FROM audit_logs
WHERE user_id = $1
  AND created_at >= $2
GROUP BY event_type
"#,
        )
        .bind(user_id)
        .bind(cutoff_time)
        .fetch(&self.pool);

        let mut counts = std::collections::BTreeMap::new();
        while let Some(row) = rows.try_next().await.map_postgres_err()? {
            if let Ok((event_type, count)) = event_count_from_postgres_row(&row) {
                counts.insert(event_type, count);
            }
        }
        Ok(counts)
    }

    async fn list_user_audit_logs(
        &self,
        user_id: &str,
        query: &AuditLogListQuery,
    ) -> Result<StoredUserAuditLogPage, DataLayerError> {
        let cutoff_time = postgres_cutoff_time(query.cutoff_unix_secs);
        let total = sqlx::query_scalar::<_, i64>(
            r#"
SELECT COUNT(*)
FROM audit_logs
WHERE user_id = $1
  AND created_at >= $2
  AND ($3::text IS NULL OR event_type = $3)
"#,
        )
        .bind(user_id)
        .bind(cutoff_time)
        .bind(query.event_type.as_deref())
        .fetch_one(&self.pool)
        .await
        .map_postgres_err()?;

        let mut rows = sqlx::query(
            r#"
SELECT id, event_type, description, ip_address, status_code, created_at
FROM audit_logs
WHERE user_id = $1
  AND created_at >= $2
  AND ($3::text IS NULL OR event_type = $3)
ORDER BY created_at DESC
LIMIT $4 OFFSET $5
"#,
        )
        .bind(user_id)
        .bind(cutoff_time)
        .bind(query.event_type.as_deref())
        .bind(i64::try_from(query.limit).unwrap_or(i64::MAX))
        .bind(i64::try_from(query.offset).unwrap_or(i64::MAX))
        .fetch(&self.pool);

        let mut items = Vec::new();
        while let Some(row) = rows.try_next().await.map_postgres_err()? {
            items.push(map_postgres_user_audit_log_row(&row)?);
        }

        Ok(StoredUserAuditLogPage {
            items,
            total: total.max(0) as u64,
        })
    }

    async fn delete_audit_logs_before(
        &self,
        cutoff_unix_secs: u64,
        limit: usize,
    ) -> Result<usize, DataLayerError> {
        let deleted = sqlx::query(
            r#"
WITH doomed AS (
    SELECT id
    FROM audit_logs
    WHERE created_at < $1
    ORDER BY created_at ASC, id ASC
    LIMIT $2
)
DELETE FROM audit_logs AS audit
USING doomed
WHERE audit.id = doomed.id
"#,
        )
        .bind(postgres_cutoff_time(cutoff_unix_secs))
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await
        .map_postgres_err()?
        .rows_affected();
        Ok(usize::try_from(deleted).unwrap_or(usize::MAX))
    }
}

fn postgres_cutoff_time(cutoff_unix_secs: u64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(cutoff_unix_secs.min(i64::MAX as u64) as i64, 0)
        .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).expect("unix epoch is valid"))
}

fn unix_secs_to_rfc3339(secs: u64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(secs.min(i64::MAX as u64) as i64, 0)
        .map(|value| value.to_rfc3339())
}

impl StoredAdminAuditLog {
    pub fn created_at_rfc3339(&self) -> Option<String> {
        unix_secs_to_rfc3339(self.created_at_unix_secs)
    }
}

impl StoredSuspiciousActivity {
    pub fn created_at_rfc3339(&self) -> Option<String> {
        unix_secs_to_rfc3339(self.created_at_unix_secs)
    }
}

impl StoredUserAuditLog {
    pub fn created_at_rfc3339(&self) -> Option<String> {
        unix_secs_to_rfc3339(self.created_at_unix_secs)
    }
}

fn postgres_created_at_unix_secs(row: &PgRow) -> Result<u64, DataLayerError> {
    let value = row
        .try_get::<DateTime<Utc>, _>("created_at")
        .map_postgres_err()?;
    Ok(value.timestamp().max(0) as u64)
}

fn map_postgres_admin_audit_log_row(row: &PgRow) -> Result<StoredAdminAuditLog, DataLayerError> {
    Ok(StoredAdminAuditLog {
        id: row.try_get("id").map_postgres_err()?,
        event_type: row.try_get("event_type").map_postgres_err()?,
        user_id: row.try_get("user_id").map_postgres_err()?,
        user_email: row.try_get("user_email").map_postgres_err()?,
        user_username: row.try_get("user_username").map_postgres_err()?,
        description: row.try_get("description").map_postgres_err()?,
        ip_address: row.try_get("ip_address").map_postgres_err()?,
        status_code: row.try_get("status_code").map_postgres_err()?,
        error_message: row.try_get("error_message").map_postgres_err()?,
        metadata: row.try_get("metadata").map_postgres_err()?,
        created_at_unix_secs: postgres_created_at_unix_secs(row)?,
    })
}

fn map_postgres_suspicious_activity_row(
    row: &PgRow,
) -> Result<StoredSuspiciousActivity, DataLayerError> {
    Ok(StoredSuspiciousActivity {
        id: row.try_get("id").map_postgres_err()?,
        event_type: row.try_get("event_type").map_postgres_err()?,
        user_id: row.try_get("user_id").map_postgres_err()?,
        description: row.try_get("description").map_postgres_err()?,
        ip_address: row.try_get("ip_address").map_postgres_err()?,
        metadata: row.try_get("metadata").map_postgres_err()?,
        created_at_unix_secs: postgres_created_at_unix_secs(row)?,
    })
}

fn map_postgres_user_audit_log_row(row: &PgRow) -> Result<StoredUserAuditLog, DataLayerError> {
    Ok(StoredUserAuditLog {
        id: row.try_get("id").map_postgres_err()?,
        event_type: row.try_get("event_type").map_postgres_err()?,
        description: row.try_get("description").map_postgres_err()?,
        ip_address: row.try_get("ip_address").map_postgres_err()?,
        status_code: row.try_get("status_code").map_postgres_err()?,
        created_at_unix_secs: postgres_created_at_unix_secs(row)?,
    })
}

fn event_count_from_postgres_row(row: &PgRow) -> Result<(String, u64), DataLayerError> {
    let event_type = row.try_get("event_type").map_postgres_err()?;
    let count = row.try_get::<i64, _>("count").map_postgres_err()?.max(0) as u64;
    Ok((event_type, count))
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequestAuditBundle {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<StoredRequestUsageAudit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_trace: Option<DecisionTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_snapshot: Option<ResolvedAuthApiKeySnapshot>,
}

#[async_trait]
pub trait RequestAuditReader {
    async fn find_request_usage_audit_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError>;

    async fn read_request_decision_trace(
        &self,
        request_id: &str,
        attempted_only: bool,
    ) -> Result<Option<DecisionTrace>, DataLayerError>;

    async fn read_resolved_auth_api_key_snapshot(
        &self,
        user_id: &str,
        api_key_id: &str,
        now_unix_secs: u64,
    ) -> Result<Option<ResolvedAuthApiKeySnapshot>, DataLayerError>;
}

pub async fn read_request_audit_bundle(
    state: &impl RequestAuditReader,
    request_id: &str,
    attempted_only: bool,
    now_unix_secs: u64,
) -> Result<Option<RequestAuditBundle>, DataLayerError> {
    let usage = state
        .find_request_usage_audit_by_request_id(request_id)
        .await?;
    let decision_trace = state
        .read_request_decision_trace(request_id, attempted_only)
        .await?;

    let auth_snapshot = if let Some(usage) = usage.as_ref() {
        match (usage.user_id.as_deref(), usage.api_key_id.as_deref()) {
            (Some(user_id), Some(api_key_id)) => {
                state
                    .read_resolved_auth_api_key_snapshot(user_id, api_key_id, now_unix_secs)
                    .await?
            }
            _ => None,
        }
    } else {
        None
    };

    if usage.is_none() && decision_trace.is_none() && auth_snapshot.is_none() {
        return Ok(None);
    }

    Ok(Some(RequestAuditBundle {
        request_id: request_id.to_string(),
        usage,
        decision_trace,
        auth_snapshot,
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::{read_request_audit_bundle, RequestAuditReader};
    use crate::repository::auth::{ResolvedAuthApiKeySnapshot, StoredAuthApiKeySnapshot};
    use crate::repository::candidates::{
        DecisionTrace, DecisionTraceCandidate, RequestCandidateFinalStatus, RequestCandidateStatus,
        StoredRequestCandidate,
    };
    use crate::repository::usage::StoredRequestUsageAudit;
    use crate::DataLayerError;

    #[derive(Default)]
    struct FakeRequestAuditReader {
        usage: Option<StoredRequestUsageAudit>,
        decision_trace: Option<DecisionTrace>,
        auth_snapshot: Option<ResolvedAuthApiKeySnapshot>,
        auth_snapshot_reads: AtomicUsize,
    }

    #[async_trait]
    impl RequestAuditReader for FakeRequestAuditReader {
        async fn find_request_usage_audit_by_request_id(
            &self,
            _request_id: &str,
        ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
            Ok(self.usage.clone())
        }

        async fn read_request_decision_trace(
            &self,
            _request_id: &str,
            _attempted_only: bool,
        ) -> Result<Option<DecisionTrace>, DataLayerError> {
            Ok(self.decision_trace.clone())
        }

        async fn read_resolved_auth_api_key_snapshot(
            &self,
            _user_id: &str,
            _api_key_id: &str,
            _now_unix_secs: u64,
        ) -> Result<Option<ResolvedAuthApiKeySnapshot>, DataLayerError> {
            self.auth_snapshot_reads.fetch_add(1, Ordering::Relaxed);
            Ok(self.auth_snapshot.clone())
        }
    }

    #[tokio::test]
    async fn read_request_audit_bundle_resolves_usage_trace_and_auth_snapshot() {
        let state = FakeRequestAuditReader {
            usage: Some(sample_usage("req-audit-1")),
            decision_trace: Some(sample_decision_trace("req-audit-1")),
            auth_snapshot: Some(sample_resolved_auth_snapshot("user-1", "api-key-1")),
            auth_snapshot_reads: AtomicUsize::new(0),
        };

        let bundle = read_request_audit_bundle(&state, "req-audit-1", true, 123)
            .await
            .expect("bundle should read")
            .expect("bundle should exist");

        assert_eq!(bundle.request_id, "req-audit-1");
        assert_eq!(
            bundle
                .usage
                .as_ref()
                .map(|usage| usage.provider_name.as_str()),
            Some("OpenAI")
        );
        assert_eq!(
            bundle
                .decision_trace
                .as_ref()
                .map(|trace| trace.total_candidates),
            Some(1)
        );
        assert_eq!(
            bundle
                .auth_snapshot
                .as_ref()
                .map(|snapshot| snapshot.api_key_id.as_str()),
            Some("api-key-1")
        );
        assert_eq!(state.auth_snapshot_reads.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn read_request_audit_bundle_returns_none_when_all_sources_are_empty() {
        let state = FakeRequestAuditReader::default();

        let bundle = read_request_audit_bundle(&state, "req-audit-empty", false, 123)
            .await
            .expect("bundle should read");

        assert!(bundle.is_none());
        assert_eq!(state.auth_snapshot_reads.load(Ordering::Relaxed), 0);
    }

    fn sample_usage(request_id: &str) -> StoredRequestUsageAudit {
        StoredRequestUsageAudit::new(
            "usage-1".to_string(),
            request_id.to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            "OpenAI".to_string(),
            "gpt-4.1".to_string(),
            None,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            false,
            false,
            120,
            40,
            160,
            0.24,
            0.36,
            Some(200),
            None,
            None,
            Some(450),
            Some(120),
            "completed".to_string(),
            "settled".to_string(),
            100,
            101,
            Some(102),
        )
        .expect("usage should build")
    }

    fn sample_decision_trace(request_id: &str) -> DecisionTrace {
        let candidate = StoredRequestCandidate::new(
            "cand-1".to_string(),
            request_id.to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            0,
            0,
            Some("provider-1".to_string()),
            Some("endpoint-1".to_string()),
            Some("provider-key-1".to_string()),
            RequestCandidateStatus::Success,
            None,
            false,
            Some(200),
            None,
            None,
            Some(37),
            None,
            None,
            None,
            100,
            Some(101),
            Some(102),
        )
        .expect("candidate should build");
        DecisionTrace {
            request_id: request_id.to_string(),
            total_candidates: 1,
            final_status: RequestCandidateFinalStatus::Success,
            total_latency_ms: 37,
            candidates: vec![DecisionTraceCandidate {
                candidate,
                provider_name: Some("OpenAI".to_string()),
                provider_website: None,
                provider_type: Some("custom".to_string()),
                provider_priority: Some(0),
                provider_keep_priority_on_conversion: Some(false),
                provider_enable_format_conversion: Some(false),
                endpoint_api_format: Some("openai:chat".to_string()),
                endpoint_api_family: Some("openai".to_string()),
                endpoint_kind: Some("chat".to_string()),
                endpoint_format_acceptance_config: None,
                provider_key_name: Some("prod".to_string()),
                provider_key_auth_type: Some("api_key".to_string()),
                provider_key_api_formats: None,
                provider_key_internal_priority: Some(10),
                provider_key_global_priority_by_format: None,
                provider_key_capabilities: None,
                provider_key_is_active: Some(true),
            }],
        }
    }

    fn sample_resolved_auth_snapshot(
        user_id: &str,
        api_key_id: &str,
    ) -> ResolvedAuthApiKeySnapshot {
        let stored = StoredAuthApiKeySnapshot::new(
            user_id.to_string(),
            "alice".to_string(),
            Some("alice@example.com".to_string()),
            "user".to_string(),
            "local".to_string(),
            true,
            false,
            Some(serde_json::json!(["openai"])),
            Some(serde_json::json!(["openai:chat"])),
            Some(serde_json::json!(["gpt-4.1"])),
            api_key_id.to_string(),
            Some("default".to_string()),
            true,
            false,
            false,
            Some(60),
            Some(5),
            Some(4_102_444_800),
            Some(serde_json::json!(["openai"])),
            Some(serde_json::json!(["openai:chat"])),
            Some(serde_json::json!(["gpt-4.1"])),
        )
        .expect("auth snapshot should build");
        ResolvedAuthApiKeySnapshot::from_stored(stored, 123)
    }
}

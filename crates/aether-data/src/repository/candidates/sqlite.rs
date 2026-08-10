use std::collections::{BTreeMap, BTreeSet};

use aether_data_contracts::repository::billing::{
    BillingFundingSource, BillingRequestAdmissionInput, BillingRequestAdmissionRecord,
};
use async_trait::async_trait;
use sqlx::{sqlite::SqliteRow, QueryBuilder, Row, Sqlite, SqliteConnection};

use super::{
    PublicHealthStatusCount, PublicHealthTimelineBucket, RequestCandidateReadRepository,
    RequestCandidateStatus, RequestCandidateWriteRepository, StoredRequestCandidate,
    UpsertRequestCandidateRecord,
};
use crate::driver::sqlite::SqlitePool;
use crate::error::SqlResultExt;
use crate::DataLayerError;
use aether_data_query::{push_in, WhereClause};

const CANDIDATE_COLUMNS: &str = r#"
SELECT
  id,
  request_id,
  user_id,
  api_key_id,
  username,
  api_key_name,
  candidate_index,
  retry_index,
  provider_id,
  endpoint_id,
  key_id,
  status,
  skip_reason,
  is_cached,
  status_code,
  error_type,
  error_message,
  latency_ms,
  concurrent_requests,
  extra_data,
  required_capabilities,
  created_at AS created_at_unix_ms,
  started_at AS started_at_unix_ms,
  finished_at AS finished_at_unix_ms
FROM request_candidates
"#;

const DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL: &str = r#"
DELETE FROM billing_request_admissions
WHERE request_id IN (
  SELECT admission.request_id
  FROM billing_request_admissions admission
  LEFT JOIN "usage" usage_record
    ON usage_record.request_id = admission.request_id
  LEFT JOIN usage_settlement_snapshots settlement
    ON settlement.request_id = admission.request_id
  WHERE admission.created_at < ?
    AND NOT EXISTS (
      SELECT 1
      FROM request_candidates candidate
      WHERE candidate.request_id = admission.request_id
    )
    AND COALESCE(settlement.billing_status, usage_record.billing_status, 'settled') <> 'pending'
  ORDER BY admission.created_at ASC, admission.request_id ASC
  LIMIT ?
)
"#;

#[derive(Debug, Clone)]
pub struct SqliteRequestCandidateRepository {
    pool: SqlitePool,
}

impl SqliteRequestCandidateRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn find_by_unique(
        &self,
        request_id: &str,
        candidate_index: u32,
        retry_index: u32,
    ) -> Result<Option<StoredRequestCandidate>, DataLayerError> {
        let row = sqlx::query(&format!(
            "{CANDIDATE_COLUMNS} WHERE request_id = ? AND candidate_index = ? AND retry_index = ? LIMIT 1"
        ))
        .bind(request_id)
        .bind(to_i32(candidate_index)?)
        .bind(to_i32(retry_index)?)
        .fetch_optional(&self.pool)
        .await
        .map_sql_err()?;
        row.as_ref().map(map_candidate_row).transpose()
    }
}

#[async_trait]
impl RequestCandidateReadRepository for SqliteRequestCandidateRepository {
    async fn find_billing_admission(
        &self,
        request_id: &str,
    ) -> Result<Option<BillingRequestAdmissionRecord>, DataLayerError> {
        let row = sqlx::query(
            r#"
SELECT request_id, user_id, api_key_id, wallet_id, global_model_id, funding_source,
       wallet_balance_at_admission, wallet_payment_allowed, wallet_overage_allowed,
       entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
       billing_admitted, status, rejection_reason, schema_version, created_at, updated_at
FROM billing_request_admissions
WHERE request_id = ?
LIMIT 1
            "#,
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await
        .map_sql_err()?;
        row.as_ref().map(map_billing_admission_row).transpose()
    }

    async fn list_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        let rows = sqlx::query(&format!(
            "{CANDIDATE_COLUMNS} WHERE request_id = ? ORDER BY candidate_index ASC, retry_index ASC, created_at ASC"
        ))
        .bind(request_id)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        rows.iter().map(map_candidate_row).collect()
    }

    async fn list_recent(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(&format!(
            "{CANDIDATE_COLUMNS} ORDER BY created_at DESC LIMIT ?"
        ))
        .bind(limit_i64(limit, "recent request candidate limit")?)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        rows.iter().map(map_candidate_row).collect()
    }

    async fn list_by_provider_id(
        &self,
        provider_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(&format!(
            "{CANDIDATE_COLUMNS} WHERE provider_id = ? ORDER BY created_at DESC LIMIT ?"
        ))
        .bind(provider_id)
        .bind(limit_i64(limit, "provider request candidate limit")?)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        rows.iter().map(map_candidate_row).collect()
    }

    async fn count_attempted_with_unknown_upstream_in_window(
        &self,
        window_start_unix_ms: u64,
        window_end_unix_ms: u64,
    ) -> Result<u64, DataLayerError> {
        if window_end_unix_ms <= window_start_unix_ms {
            return Ok(0);
        }

        let row = sqlx::query(
            r#"
SELECT COUNT(*) AS count
FROM request_candidates
WHERE created_at >= ?
  AND created_at < ?
  AND (
    status IN ('streaming', 'success', 'failed', 'cancelled')
    OR (status = 'pending' AND started_at IS NOT NULL)
  )
  AND (
    TRIM(COALESCE(provider_id, '')) = ''
    OR LOWER(TRIM(COALESCE(provider_id, ''))) IN ('unknown', 'unknow', 'pending')
    OR TRIM(COALESCE(key_id, '')) = ''
    OR LOWER(TRIM(COALESCE(key_id, ''))) IN ('unknown', 'unknow', 'pending')
  )
"#,
        )
        .bind(u64_to_i64(
            window_start_unix_ms,
            "request candidate window start",
        )?)
        .bind(u64_to_i64(
            window_end_unix_ms,
            "request candidate window end",
        )?)
        .fetch_one(&self.pool)
        .await
        .map_sql_err()?;
        Ok(
            u64::try_from(row.try_get::<i64, _>("count").map_sql_err()?).map_err(|_| {
                DataLayerError::UnexpectedValue("request candidate count out of range".to_string())
            })?,
        )
    }

    async fn list_finalized_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        if endpoint_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut builder = QueryBuilder::<Sqlite>::new(CANDIDATE_COLUMNS);
        let mut where_clause = WhereClause::new();
        push_in(&mut builder, &mut where_clause, "endpoint_id", endpoint_ids);
        builder
            .push(" AND created_at >= ")
            .push_bind(unix_secs_to_ms_i64(since_unix_secs)?)
            .push(" AND status IN ('success', 'failed', 'skipped')")
            .push(" ORDER BY created_at DESC LIMIT ")
            .push_bind(limit_i64(limit, "finalized request candidate limit")?);
        let rows = builder.build().fetch_all(&self.pool).await.map_sql_err()?;
        rows.iter().map(map_candidate_row).collect()
    }

    async fn count_finalized_statuses_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
    ) -> Result<Vec<PublicHealthStatusCount>, DataLayerError> {
        if endpoint_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT endpoint_id, status, COUNT(id) AS count FROM request_candidates",
        );
        let mut where_clause = WhereClause::new();
        push_in(&mut builder, &mut where_clause, "endpoint_id", endpoint_ids);
        builder
            .push(" AND created_at >= ")
            .push_bind(unix_secs_to_ms_i64(since_unix_secs)?)
            .push(" AND status IN ('success', 'failed', 'skipped')")
            .push(" GROUP BY endpoint_id, status");
        let rows = builder.build().fetch_all(&self.pool).await.map_sql_err()?;
        rows.iter()
            .map(|row| {
                Ok(PublicHealthStatusCount {
                    endpoint_id: row.try_get("endpoint_id").map_sql_err()?,
                    status: RequestCandidateStatus::from_database(
                        row.try_get::<String, _>("status").map_sql_err()?.as_str(),
                    )?,
                    count: u64::try_from(row.try_get::<i64, _>("count").map_sql_err()?).map_err(
                        |_| {
                            DataLayerError::UnexpectedValue(
                                "public health status count out of range".to_string(),
                            )
                        },
                    )?,
                })
            })
            .collect()
    }

    async fn aggregate_finalized_timeline_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
        until_unix_secs: u64,
        segments: u32,
    ) -> Result<Vec<PublicHealthTimelineBucket>, DataLayerError> {
        if endpoint_ids.is_empty() || segments == 0 || until_unix_secs < since_unix_secs {
            return Ok(Vec::new());
        }
        let since_ms = unix_secs_to_ms_i64(since_unix_secs)?;
        let until_ms = unix_secs_to_ms_i64(until_unix_secs)?;
        let mut builder = QueryBuilder::<Sqlite>::new(CANDIDATE_COLUMNS);
        let mut where_clause = WhereClause::new();
        push_in(&mut builder, &mut where_clause, "endpoint_id", endpoint_ids);
        builder
            .push(" AND created_at >= ")
            .push_bind(since_ms)
            .push(" AND created_at <= ")
            .push_bind(until_ms)
            .push(" AND status IN ('success', 'failed', 'skipped')");
        let rows = builder.build().fetch_all(&self.pool).await.map_sql_err()?;
        aggregate_timeline(
            rows.iter()
                .map(map_candidate_row)
                .collect::<Result<Vec<_>, _>>()?,
            since_unix_secs,
            until_unix_secs,
            segments,
        )
    }
}

#[async_trait]
impl RequestCandidateWriteRepository for SqliteRequestCandidateRepository {
    async fn upsert(
        &self,
        candidate: UpsertRequestCandidateRecord,
    ) -> Result<StoredRequestCandidate, DataLayerError> {
        candidate.validate()?;
        let existing = self
            .find_by_unique(
                &candidate.request_id,
                candidate.candidate_index,
                candidate.retry_index,
            )
            .await?;
        let merged = merge_candidate(candidate, existing)?;
        let mut connection = self.pool.acquire().await.map_sql_err()?;
        upsert_merged_candidate(&mut connection, &merged).await?;
        Ok(merged)
    }

    async fn upsert_with_billing_admission(
        &self,
        candidate: UpsertRequestCandidateRecord,
        admission: BillingRequestAdmissionInput,
    ) -> Result<(StoredRequestCandidate, BillingRequestAdmissionRecord), DataLayerError> {
        super::admission::validate_candidate_admission(&candidate, &admission)?;
        let mut tx = self.pool.begin().await.map_sql_err()?;
        let existing = sqlx::query(&format!(
            "{CANDIDATE_COLUMNS} WHERE request_id = ? AND candidate_index = ? AND retry_index = ? LIMIT 1"
        ))
        .bind(&candidate.request_id)
        .bind(to_i32(candidate.candidate_index)?)
        .bind(to_i32(candidate.retry_index)?)
        .fetch_optional(&mut *tx)
        .await
        .map_sql_err()?
        .as_ref()
        .map(map_candidate_row)
        .transpose()?;
        let merged = merge_candidate(candidate, existing)?;
        let now = super::admission::current_unix_ms();
        sqlx::query(
            r#"
INSERT OR IGNORE INTO billing_request_admissions (
  request_id, user_id, api_key_id, wallet_id, global_model_id, funding_source,
  wallet_balance_at_admission, wallet_payment_allowed, wallet_overage_allowed,
  entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
  billing_admitted, status, rejection_reason, schema_version, created_at, updated_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 'admitted', NULL, ?, ?, ?)
            "#,
        )
        .bind(&admission.request_id)
        .bind(&admission.user_id)
        .bind(&admission.api_key_id)
        .bind(&admission.wallet_id)
        .bind(&admission.global_model_id)
        .bind(admission.funding_source.as_str())
        .bind(admission.wallet_balance_at_admission)
        .bind(i64::from(admission.wallet_payment_allowed))
        .bind(i64::from(admission.wallet_overage_allowed))
        .bind(
            serde_json::to_string(&admission.entitlement_ids).map_err(|error| {
                DataLayerError::UnexpectedValue(format!(
                    "billing entitlement ids encode failed: {error}"
                ))
            })?,
        )
        .bind(
            serde_json::to_string(&admission.entitlement_provider_scopes).map_err(|error| {
                DataLayerError::UnexpectedValue(format!(
                    "billing entitlement provider scopes encode failed: {error}"
                ))
            })?,
        )
        .bind(
            serde_json::to_string(&admission.allowed_provider_ids).map_err(|error| {
                DataLayerError::UnexpectedValue(format!(
                    "billing provider ids encode failed: {error}"
                ))
            })?,
        )
        .bind(i64::from(admission.schema_version))
        .bind(u64_to_i64(now, "billing admission created_at")?)
        .bind(u64_to_i64(now, "billing admission updated_at")?)
        .execute(&mut *tx)
        .await
        .map_sql_err()?;
        let stored_admission = sqlx::query(
            r#"
SELECT request_id, user_id, api_key_id, wallet_id, global_model_id, funding_source,
       wallet_balance_at_admission, wallet_payment_allowed, wallet_overage_allowed,
       entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
       billing_admitted, status, rejection_reason, schema_version, created_at, updated_at
FROM billing_request_admissions
WHERE request_id = ?
LIMIT 1
            "#,
        )
        .bind(&admission.request_id)
        .fetch_one(&mut *tx)
        .await
        .map_sql_err()
        .and_then(|row| map_billing_admission_row(&row))?;
        super::admission::validate_stored_admission_matches_input(&stored_admission, &admission)?;
        upsert_merged_candidate(&mut tx, &merged).await?;
        tx.commit().await.map_sql_err()?;
        Ok((merged, stored_admission))
    }

    async fn delete_created_before(
        &self,
        created_before_unix_secs: u64,
        limit: usize,
    ) -> Result<usize, DataLayerError> {
        if limit == 0 {
            return Ok(0);
        }
        let rows_affected = sqlx::query(
            r#"
DELETE FROM request_candidates
WHERE id IN (
  SELECT id
  FROM request_candidates
  WHERE created_at < ?
  ORDER BY created_at ASC, id ASC
  LIMIT ?
)
"#,
        )
        .bind(unix_secs_to_ms_i64(created_before_unix_secs)?)
        .bind(limit_i64(limit, "request candidate delete limit")?)
        .execute(&self.pool)
        .await
        .map_sql_err()?
        .rows_affected();
        sqlx::query(DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL)
            .bind(unix_secs_to_ms_i64(created_before_unix_secs)?)
            .bind(limit_i64(limit, "billing admission delete limit")?)
            .execute(&self.pool)
            .await
            .map_sql_err()?;
        Ok(usize::try_from(rows_affected).unwrap_or_default())
    }
}

async fn upsert_merged_candidate(
    connection: &mut SqliteConnection,
    candidate: &StoredRequestCandidate,
) -> Result<(), DataLayerError> {
    sqlx::query(
        r#"
INSERT INTO request_candidates (
  id, request_id, user_id, api_key_id, username, api_key_name,
  candidate_index, retry_index, provider_id, endpoint_id, key_id, status,
  skip_reason, is_cached, status_code, error_type, error_message, latency_ms,
  concurrent_requests, extra_data, required_capabilities, created_at, started_at, finished_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(request_id, candidate_index, retry_index) DO UPDATE SET
  user_id = excluded.user_id,
  api_key_id = excluded.api_key_id,
  username = excluded.username,
  api_key_name = excluded.api_key_name,
  provider_id = excluded.provider_id,
  endpoint_id = excluded.endpoint_id,
  key_id = excluded.key_id,
  status = excluded.status,
  skip_reason = excluded.skip_reason,
  is_cached = excluded.is_cached,
  status_code = excluded.status_code,
  error_type = excluded.error_type,
  error_message = excluded.error_message,
  latency_ms = excluded.latency_ms,
  concurrent_requests = excluded.concurrent_requests,
  extra_data = excluded.extra_data,
  required_capabilities = excluded.required_capabilities,
  created_at = excluded.created_at,
  started_at = excluded.started_at,
  finished_at = excluded.finished_at
"#,
    )
    .bind(&candidate.id)
    .bind(&candidate.request_id)
    .bind(&candidate.user_id)
    .bind(&candidate.api_key_id)
    .bind(&candidate.username)
    .bind(&candidate.api_key_name)
    .bind(to_i32(candidate.candidate_index)?)
    .bind(to_i32(candidate.retry_index)?)
    .bind(&candidate.provider_id)
    .bind(&candidate.endpoint_id)
    .bind(&candidate.key_id)
    .bind(status_to_database(candidate.status))
    .bind(&candidate.skip_reason)
    .bind(candidate.is_cached)
    .bind(candidate.status_code.map(i32::from))
    .bind(&candidate.error_type)
    .bind(&candidate.error_message)
    .bind(candidate.latency_ms.map(to_i32_u64).transpose()?)
    .bind(candidate.concurrent_requests.map(to_i32).transpose()?)
    .bind(json_to_string(&candidate.extra_data)?)
    .bind(json_to_string(&candidate.required_capabilities)?)
    .bind(u64_to_i64(
        candidate.created_at_unix_ms,
        "request candidate created_at",
    )?)
    .bind(optional_u64_to_i64(
        candidate.started_at_unix_ms,
        "request candidate started_at",
    )?)
    .bind(optional_u64_to_i64(
        candidate.finished_at_unix_ms,
        "request candidate finished_at",
    )?)
    .execute(connection)
    .await
    .map_sql_err()?;
    Ok(())
}

fn merge_candidate(
    candidate: UpsertRequestCandidateRecord,
    existing: Option<StoredRequestCandidate>,
) -> Result<StoredRequestCandidate, DataLayerError> {
    let created_at_unix_ms = candidate
        .created_at_unix_ms
        .filter(|value| *value > 1000)
        .or_else(|| {
            existing
                .as_ref()
                .map(|value| value.created_at_unix_ms)
                .filter(|value| *value > 1000)
        })
        .or(candidate.started_at_unix_ms)
        .or(candidate.finished_at_unix_ms)
        .unwrap_or_else(current_unix_ms);
    let id = existing
        .as_ref()
        .map(|value| value.id.clone())
        .unwrap_or(candidate.id);
    let extra_data = merge_json_objects(
        existing.as_ref().and_then(|value| value.extra_data.clone()),
        candidate.extra_data,
    );
    StoredRequestCandidate::new(
        id,
        candidate.request_id,
        candidate
            .user_id
            .or_else(|| existing.as_ref().and_then(|value| value.user_id.clone())),
        candidate
            .api_key_id
            .or_else(|| existing.as_ref().and_then(|value| value.api_key_id.clone())),
        candidate
            .username
            .or_else(|| existing.as_ref().and_then(|value| value.username.clone())),
        candidate.api_key_name.or_else(|| {
            existing
                .as_ref()
                .and_then(|value| value.api_key_name.clone())
        }),
        to_i32(candidate.candidate_index)?,
        to_i32(candidate.retry_index)?,
        candidate.provider_id.or_else(|| {
            existing
                .as_ref()
                .and_then(|value| value.provider_id.clone())
        }),
        candidate.endpoint_id.or_else(|| {
            existing
                .as_ref()
                .and_then(|value| value.endpoint_id.clone())
        }),
        candidate
            .key_id
            .or_else(|| existing.as_ref().and_then(|value| value.key_id.clone())),
        candidate.status,
        candidate.skip_reason.or_else(|| {
            existing
                .as_ref()
                .and_then(|value| value.skip_reason.clone())
        }),
        candidate
            .is_cached
            .unwrap_or_else(|| existing.as_ref().is_some_and(|value| value.is_cached)),
        candidate.status_code.map(i32::from).or_else(|| {
            existing
                .as_ref()
                .and_then(|value| value.status_code.map(i32::from))
        }),
        candidate
            .error_type
            .or_else(|| existing.as_ref().and_then(|value| value.error_type.clone())),
        candidate.error_message.or_else(|| {
            existing
                .as_ref()
                .and_then(|value| value.error_message.clone())
        }),
        candidate.latency_ms.map(to_i32_u64).transpose()?.or(
            match existing.as_ref().and_then(|value| value.latency_ms) {
                Some(value) => Some(to_i32_u64(value)?),
                None => None,
            },
        ),
        candidate.concurrent_requests.map(to_i32).transpose()?.or(
            match existing
                .as_ref()
                .and_then(|value| value.concurrent_requests)
            {
                Some(value) => Some(to_i32(value)?),
                None => None,
            },
        ),
        extra_data,
        candidate.required_capabilities.or_else(|| {
            existing
                .as_ref()
                .and_then(|value| value.required_capabilities.clone())
        }),
        u64_to_i64(created_at_unix_ms, "request candidate created_at")?,
        candidate
            .started_at_unix_ms
            .or_else(|| existing.as_ref().and_then(|value| value.started_at_unix_ms))
            .map(|value| u64_to_i64(value, "request candidate started_at"))
            .transpose()?,
        candidate
            .finished_at_unix_ms
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|value| value.finished_at_unix_ms)
            })
            .map(|value| u64_to_i64(value, "request candidate finished_at"))
            .transpose()?,
    )
}

fn aggregate_timeline(
    candidates: Vec<StoredRequestCandidate>,
    since_unix_secs: u64,
    until_unix_secs: u64,
    segments: u32,
) -> Result<Vec<PublicHealthTimelineBucket>, DataLayerError> {
    let endpoint_ids = candidates
        .iter()
        .filter_map(|candidate| candidate.endpoint_id.clone())
        .collect::<BTreeSet<_>>();
    let span_ms = until_unix_secs
        .saturating_sub(since_unix_secs)
        .saturating_mul(1000)
        .max(1);
    let since_ms = since_unix_secs.saturating_mul(1000);
    let mut buckets = BTreeMap::<(String, u32), PublicHealthTimelineBucket>::new();
    for candidate in candidates {
        let Some(endpoint_id) = candidate.endpoint_id.clone() else {
            continue;
        };
        let offset = candidate.created_at_unix_ms.saturating_sub(since_ms);
        let segment_idx = ((offset.saturating_mul(u64::from(segments))) / span_ms)
            .min(u64::from(segments.saturating_sub(1))) as u32;
        let bucket = buckets.entry((endpoint_id.clone(), segment_idx)).or_insert(
            PublicHealthTimelineBucket {
                endpoint_id,
                segment_idx,
                total_count: 0,
                success_count: 0,
                failed_count: 0,
                min_created_at_unix_ms: Some(candidate.created_at_unix_ms),
                max_created_at_unix_ms: Some(candidate.created_at_unix_ms),
            },
        );
        bucket.total_count += 1;
        if candidate.status == RequestCandidateStatus::Success {
            bucket.success_count += 1;
        }
        if candidate.status == RequestCandidateStatus::Failed {
            bucket.failed_count += 1;
        }
        bucket.min_created_at_unix_ms = bucket
            .min_created_at_unix_ms
            .map(|value| value.min(candidate.created_at_unix_ms));
        bucket.max_created_at_unix_ms = bucket
            .max_created_at_unix_ms
            .map(|value| value.max(candidate.created_at_unix_ms));
    }
    for endpoint_id in endpoint_ids {
        for segment_idx in 0..segments {
            buckets.entry((endpoint_id.clone(), segment_idx)).or_insert(
                PublicHealthTimelineBucket {
                    endpoint_id: endpoint_id.clone(),
                    segment_idx,
                    total_count: 0,
                    success_count: 0,
                    failed_count: 0,
                    min_created_at_unix_ms: None,
                    max_created_at_unix_ms: None,
                },
            );
        }
    }
    Ok(buckets.into_values().collect())
}

fn map_candidate_row(row: &SqliteRow) -> Result<StoredRequestCandidate, DataLayerError> {
    StoredRequestCandidate::new(
        row.try_get("id").map_sql_err()?,
        row.try_get("request_id").map_sql_err()?,
        row.try_get("user_id").map_sql_err()?,
        row.try_get("api_key_id").map_sql_err()?,
        row.try_get("username").map_sql_err()?,
        row.try_get("api_key_name").map_sql_err()?,
        row.try_get("candidate_index").map_sql_err()?,
        row.try_get("retry_index").map_sql_err()?,
        row.try_get("provider_id").map_sql_err()?,
        row.try_get("endpoint_id").map_sql_err()?,
        row.try_get("key_id").map_sql_err()?,
        RequestCandidateStatus::from_database(
            row.try_get::<String, _>("status").map_sql_err()?.as_str(),
        )?,
        row.try_get("skip_reason").map_sql_err()?,
        row.try_get("is_cached").map_sql_err()?,
        row.try_get("status_code").map_sql_err()?,
        row.try_get("error_type").map_sql_err()?,
        row.try_get("error_message").map_sql_err()?,
        row.try_get("latency_ms").map_sql_err()?,
        row.try_get("concurrent_requests").map_sql_err()?,
        parse_json(row.try_get("extra_data").ok().flatten())?,
        parse_json(row.try_get("required_capabilities").ok().flatten())?,
        row.try_get("created_at_unix_ms").map_sql_err()?,
        row.try_get("started_at_unix_ms").map_sql_err()?,
        row.try_get("finished_at_unix_ms").map_sql_err()?,
    )
}

fn map_billing_admission_row(
    row: &SqliteRow,
) -> Result<BillingRequestAdmissionRecord, DataLayerError> {
    let entitlement_ids = serde_json::from_str::<Vec<String>>(
        &row.try_get::<String, _>("entitlement_ids").map_sql_err()?,
    )
    .map_err(|error| {
        DataLayerError::UnexpectedValue(format!(
            "billing admission entitlement ids are invalid: {error}"
        ))
    })?;
    let allowed_provider_ids = serde_json::from_str::<Vec<String>>(
        &row.try_get::<String, _>("allowed_provider_ids")
            .map_sql_err()?,
    )
    .map_err(|error| {
        DataLayerError::UnexpectedValue(format!(
            "billing admission provider ids are invalid: {error}"
        ))
    })?;
    let entitlement_provider_scopes = serde_json::from_str(
        &row.try_get::<String, _>("entitlement_provider_scopes")
            .map_sql_err()?,
    )
    .map_err(|error| {
        DataLayerError::UnexpectedValue(format!(
            "billing admission entitlement provider scopes are invalid: {error}"
        ))
    })?;
    let schema_version = row.try_get::<i64, _>("schema_version").map_sql_err()?;
    let created_at = row.try_get::<i64, _>("created_at").map_sql_err()?;
    let updated_at = row.try_get::<i64, _>("updated_at").map_sql_err()?;
    Ok(BillingRequestAdmissionRecord {
        request_id: row.try_get("request_id").map_sql_err()?,
        user_id: row.try_get("user_id").map_sql_err()?,
        api_key_id: row.try_get("api_key_id").map_sql_err()?,
        wallet_id: row.try_get("wallet_id").map_sql_err()?,
        global_model_id: row.try_get("global_model_id").map_sql_err()?,
        funding_source: BillingFundingSource::from_database(
            &row.try_get::<String, _>("funding_source").map_sql_err()?,
        )?,
        wallet_balance_at_admission: row.try_get("wallet_balance_at_admission").map_sql_err()?,
        wallet_payment_allowed: row
            .try_get::<i64, _>("wallet_payment_allowed")
            .map_sql_err()?
            != 0,
        wallet_overage_allowed: row
            .try_get::<i64, _>("wallet_overage_allowed")
            .map_sql_err()?
            != 0,
        entitlement_ids,
        entitlement_provider_scopes,
        allowed_provider_ids,
        billing_admitted: row.try_get::<i64, _>("billing_admitted").map_sql_err()? != 0,
        status: row.try_get("status").map_sql_err()?,
        rejection_reason: row.try_get("rejection_reason").map_sql_err()?,
        schema_version: u16::try_from(schema_version).map_err(|_| {
            DataLayerError::UnexpectedValue(
                "billing admission schema_version is invalid".to_string(),
            )
        })?,
        created_at_unix_ms: u64::try_from(created_at).map_err(|_| {
            DataLayerError::UnexpectedValue("billing admission created_at is invalid".to_string())
        })?,
        updated_at_unix_ms: u64::try_from(updated_at).map_err(|_| {
            DataLayerError::UnexpectedValue("billing admission updated_at is invalid".to_string())
        })?,
    })
}

fn parse_json(value: Option<String>) -> Result<Option<serde_json::Value>, DataLayerError> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            serde_json::from_str(&value).map_err(|err| {
                DataLayerError::UnexpectedValue(format!(
                    "request_candidates JSON field is invalid: {err}"
                ))
            })
        })
        .transpose()
}

fn json_to_string(value: &Option<serde_json::Value>) -> Result<Option<String>, DataLayerError> {
    value
        .as_ref()
        .map(|value| {
            serde_json::to_string(value).map_err(|err| {
                DataLayerError::UnexpectedValue(format!(
                    "request_candidates JSON field is unserializable: {err}"
                ))
            })
        })
        .transpose()
}

fn merge_json_objects(
    existing: Option<serde_json::Value>,
    overlay: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match (existing, overlay) {
        (
            Some(serde_json::Value::Object(mut existing_object)),
            Some(serde_json::Value::Object(overlay_object)),
        ) => {
            existing_object.extend(overlay_object);
            Some(serde_json::Value::Object(existing_object))
        }
        (_existing, Some(overlay)) => Some(overlay),
        (existing, None) => existing,
    }
}

fn status_to_database(status: RequestCandidateStatus) -> &'static str {
    match status {
        RequestCandidateStatus::Available => "available",
        RequestCandidateStatus::Unused => "unused",
        RequestCandidateStatus::Pending => "pending",
        RequestCandidateStatus::Streaming => "streaming",
        RequestCandidateStatus::Success => "success",
        RequestCandidateStatus::Failed => "failed",
        RequestCandidateStatus::Cancelled => "cancelled",
        RequestCandidateStatus::Skipped => "skipped",
    }
}

fn current_unix_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn unix_secs_to_ms_i64(value: u64) -> Result<i64, DataLayerError> {
    let value = value.checked_mul(1000).ok_or_else(|| {
        DataLayerError::UnexpectedValue("request candidate timestamp overflow".to_string())
    })?;
    i64::try_from(value).map_err(|_| {
        DataLayerError::UnexpectedValue("request candidate timestamp overflow".to_string())
    })
}

fn limit_i64(value: usize, name: &str) -> Result<i64, DataLayerError> {
    i64::try_from(value)
        .map_err(|_| DataLayerError::UnexpectedValue(format!("invalid {name}: {value}")))
}

fn to_i32(value: u32) -> Result<i32, DataLayerError> {
    i32::try_from(value).map_err(|_| {
        DataLayerError::UnexpectedValue(format!("request candidate value out of range: {value}"))
    })
}

fn to_i32_u64(value: u64) -> Result<i32, DataLayerError> {
    i32::try_from(value).map_err(|_| {
        DataLayerError::UnexpectedValue(format!("request candidate value out of range: {value}"))
    })
}

fn u64_to_i64(value: u64, name: &str) -> Result<i64, DataLayerError> {
    i64::try_from(value).map_err(|_| DataLayerError::UnexpectedValue(format!("{name} overflow")))
}

fn optional_u64_to_i64(value: Option<u64>, name: &str) -> Result<Option<i64>, DataLayerError> {
    value.map(|value| u64_to_i64(value, name)).transpose()
}

#[cfg(test)]
mod tests {
    use super::{
        SqliteRequestCandidateRepository, DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL,
    };
    use crate::lifecycle::migrate::run_sqlite_migrations;
    use crate::repository::candidates::{
        RequestCandidateReadRepository, RequestCandidateStatus, RequestCandidateWriteRepository,
        UpsertRequestCandidateRecord,
    };
    use aether_data_contracts::repository::billing::{
        BillingFundingSource, BillingRequestAdmissionInput,
    };
    use serde_json::json;

    #[test]
    fn billing_admission_cleanup_sql_keeps_pending_costs_and_live_candidates() {
        assert!(DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL.contains(
            "COALESCE(settlement.billing_status, usage_record.billing_status, 'settled') <> 'pending'"
        ));
        assert!(DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL
            .contains("FROM request_candidates candidate"));
    }

    #[tokio::test]
    async fn sqlite_cleanup_deletes_only_finalized_billing_admissions() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        sqlx::query(
            r#"
INSERT INTO "usage" (
  request_id, provider_name, model, status, billing_status,
  total_cost_usd, actual_total_cost_usd
) VALUES
  ('request-pending-cost', 'test', 'gpt-test', 'completed', 'pending', 3, 2),
  ('request-settled-cost', 'test', 'gpt-test', 'completed', 'settled', 3, 2);

INSERT INTO billing_request_admissions (
  request_id, funding_source, entitlement_ids, entitlement_provider_scopes,
  allowed_provider_ids, created_at, updated_at
) VALUES
  ('request-pending-cost', 'wallet', '[]', '{}', '[]', 1, 1),
  ('request-settled-cost', 'wallet', '[]', '{}', '[]', 1, 1);
            "#,
        )
        .execute(&pool)
        .await
        .expect("cleanup rows should seed");
        let repository = SqliteRequestCandidateRepository::new(pool.clone());

        repository
            .delete_created_before(2, 10)
            .await
            .expect("cleanup should run");

        let pending_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_request_admissions WHERE request_id = 'request-pending-cost'",
        )
        .fetch_one(&pool)
        .await
        .expect("pending admission should query");
        let settled_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_request_admissions WHERE request_id = 'request-settled-cost'",
        )
        .fetch_one(&pool)
        .await
        .expect("settled admission should query");
        assert_eq!(pending_count, 1);
        assert_eq!(settled_count, 0);
    }

    #[tokio::test]
    async fn sqlite_repository_writes_and_reads_request_candidates() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");

        let repository = SqliteRequestCandidateRepository::new(pool);
        let created = repository
            .upsert(sample_upsert(
                "candidate-1",
                RequestCandidateStatus::Pending,
                Some(json!({"a": 1})),
                1_000_000,
            ))
            .await
            .expect("candidate should insert");
        assert_eq!(created.request_id, "request-1");

        let updated = repository
            .upsert(sample_upsert(
                "candidate-replacement",
                RequestCandidateStatus::Success,
                Some(json!({"b": 2})),
                1_000_500,
            ))
            .await
            .expect("candidate should update");
        assert_eq!(updated.id, "candidate-1");
        assert_eq!(updated.extra_data, Some(json!({"a": 1, "b": 2})));

        assert_eq!(
            repository
                .list_by_request_id("request-1")
                .await
                .expect("request list should load")
                .len(),
            1
        );
        assert_eq!(
            repository
                .count_finalized_statuses_by_endpoint_ids_since(&["endpoint-1".to_string()], 900)
                .await
                .expect("status counts should load")[0]
                .count,
            1
        );
        assert_eq!(
            repository
                .aggregate_finalized_timeline_by_endpoint_ids_since(
                    &["endpoint-1".to_string()],
                    900,
                    1200,
                    3,
                )
                .await
                .expect("timeline should load")
                .len(),
            3
        );
        assert_eq!(
            repository
                .delete_created_before(2_000, 10)
                .await
                .expect("old candidates should delete"),
            1
        );
    }

    #[tokio::test]
    async fn sqlite_candidate_and_billing_admission_are_written_together() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        let repository = SqliteRequestCandidateRepository::new(pool.clone());
        let candidate = sample_upsert(
            "candidate-admitted-1",
            RequestCandidateStatus::Pending,
            None,
            1_000_000,
        );
        let admission = BillingRequestAdmissionInput {
            request_id: "request-1".to_string(),
            user_id: None,
            api_key_id: None,
            wallet_id: None,
            global_model_id: None,
            funding_source: BillingFundingSource::Wallet,
            wallet_balance_at_admission: Some(1.0),
            wallet_payment_allowed: true,
            wallet_overage_allowed: false,
            entitlement_ids: Vec::new(),
            entitlement_provider_scopes: std::collections::BTreeMap::new(),
            allowed_provider_ids: vec!["provider-1".to_string()],
            schema_version: 1,
        };

        let (stored, stored_admission) = repository
            .upsert_with_billing_admission(candidate, admission)
            .await
            .expect("candidate admission transaction should commit");

        assert_eq!(stored.id, "candidate-admitted-1");
        assert_eq!(
            stored_admission.funding_source,
            BillingFundingSource::Wallet
        );
        let admission_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_request_admissions WHERE request_id = ?",
        )
        .bind("request-1")
        .fetch_one(&pool)
        .await
        .expect("admission count should query");
        assert_eq!(admission_count, 1);
        let candidate_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM request_candidates WHERE request_id = ?")
                .bind("request-1")
                .fetch_one(&pool)
                .await
                .expect("candidate count should query");
        assert_eq!(candidate_count, 1);
    }

    #[tokio::test]
    async fn sqlite_request_retries_reuse_admission_and_reject_identity_changes() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        let repository = SqliteRequestCandidateRepository::new(pool.clone());
        let first_admission = BillingRequestAdmissionInput {
            request_id: "request-1".to_string(),
            user_id: None,
            api_key_id: None,
            wallet_id: None,
            global_model_id: Some("global-1".to_string()),
            funding_source: BillingFundingSource::Plan,
            wallet_balance_at_admission: Some(1.0),
            wallet_payment_allowed: true,
            wallet_overage_allowed: true,
            entitlement_ids: vec!["entitlement-1".to_string()],
            entitlement_provider_scopes: std::collections::BTreeMap::from([(
                "entitlement-1".to_string(),
                vec!["provider-1".to_string(), "provider-2".to_string()],
            )]),
            allowed_provider_ids: vec!["provider-1".to_string(), "provider-2".to_string()],
            schema_version: 1,
        };
        repository
            .upsert_with_billing_admission(
                sample_upsert(
                    "candidate-admission-first",
                    RequestCandidateStatus::Pending,
                    None,
                    1_000_000,
                ),
                first_admission,
            )
            .await
            .expect("first admission should commit");

        let stored_admission = repository
            .find_billing_admission("request-1")
            .await
            .expect("admission lookup should succeed")
            .expect("admission should exist");
        let mut retry_candidate = sample_upsert(
            "candidate-admission-retry",
            RequestCandidateStatus::Pending,
            None,
            1_000_100,
        );
        retry_candidate.retry_index = 1;
        retry_candidate.provider_id = Some("provider-2".to_string());
        retry_candidate.endpoint_id = Some("endpoint-2".to_string());
        repository
            .upsert_with_billing_admission(retry_candidate, stored_admission.to_input())
            .await
            .expect("plan-funded retry within the provider scope should reuse admission");

        let mut outside_scope_candidate = sample_upsert(
            "candidate-admission-outside-scope",
            RequestCandidateStatus::Pending,
            None,
            1_000_150,
        );
        outside_scope_candidate.retry_index = 2;
        outside_scope_candidate.provider_id = Some("provider-3".to_string());
        outside_scope_candidate.endpoint_id = Some("endpoint-3".to_string());
        let outside_scope = repository
            .upsert_with_billing_admission(outside_scope_candidate, stored_admission.to_input())
            .await;
        assert!(outside_scope.is_err());

        let mut conflicting = stored_admission.to_input();
        conflicting.global_model_id = Some("global-other".to_string());
        let mut conflicting_candidate = sample_upsert(
            "candidate-admission-conflict",
            RequestCandidateStatus::Pending,
            None,
            1_000_200,
        );
        conflicting_candidate.retry_index = 3;
        let conflict = repository
            .upsert_with_billing_admission(conflicting_candidate, conflicting)
            .await;
        assert!(conflict.is_err());

        let candidate_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM request_candidates WHERE request_id = ?")
                .bind("request-1")
                .fetch_one(&pool)
                .await
                .expect("candidate count should query");
        assert_eq!(candidate_count, 2);
    }

    fn sample_upsert(
        id: &str,
        status: RequestCandidateStatus,
        extra_data: Option<serde_json::Value>,
        created_at_unix_ms: u64,
    ) -> UpsertRequestCandidateRecord {
        UpsertRequestCandidateRecord {
            id: id.to_string(),
            request_id: "request-1".to_string(),
            user_id: Some("user-1".to_string()),
            api_key_id: Some("key-1".to_string()),
            username: Some("user".to_string()),
            api_key_name: Some("Key".to_string()),
            candidate_index: 0,
            retry_index: 0,
            provider_id: Some("provider-1".to_string()),
            endpoint_id: Some("endpoint-1".to_string()),
            key_id: Some("provider-key-1".to_string()),
            status,
            skip_reason: None,
            is_cached: Some(false),
            status_code: Some(200),
            error_type: None,
            error_message: None,
            latency_ms: Some(123),
            concurrent_requests: Some(2),
            extra_data,
            required_capabilities: Some(json!({"streaming": true})),
            created_at_unix_ms: Some(created_at_unix_ms),
            started_at_unix_ms: Some(created_at_unix_ms + 1),
            finished_at_unix_ms: Some(created_at_unix_ms + 2),
        }
    }
}

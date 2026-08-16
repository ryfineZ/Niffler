use aether_data_contracts::repository::billing::{
    BillingFundingSource, BillingRequestAdmissionInput, BillingRequestAdmissionRecord,
};
use async_trait::async_trait;
use futures_util::{future::BoxFuture, stream::TryStream, TryStreamExt};
use sqlx::{postgres::PgRow, PgPool, Postgres, QueryBuilder, Row};
use uuid::Uuid;

use super::{
    PublicHealthStatusCount, PublicHealthTimelineBucket, RequestCandidateReadRepository,
    RequestCandidateStatus, RequestCandidateWriteRepository, StoredRequestCandidate,
    UpsertRequestCandidateRecord,
};
use crate::driver::postgres::PostgresTransactionRunner;
use crate::{error::SqlxResultExt, DataLayerError};
use aether_data_query::{push_eq, push_in, push_limit, WhereClause};

const LIST_BY_REQUEST_ID_SQL: &str = r#"
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
  CAST(EXTRACT(EPOCH FROM created_at) * 1000 AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM started_at) * 1000 AS BIGINT) AS started_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM finished_at) * 1000 AS BIGINT) AS finished_at_unix_ms
FROM request_candidates
WHERE request_id = $1
ORDER BY candidate_index ASC, retry_index ASC, created_at ASC
"#;

const AGGREGATE_FINALIZED_TIMELINE_BY_ENDPOINT_IDS_SINCE_SQL: &str = r#"
SELECT
  endpoint_id,
  FLOOR(EXTRACT(EPOCH FROM (created_at - TO_TIMESTAMP($2))) / $4)::BIGINT AS segment_idx,
  COUNT(id) AS total_count,
  SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS success_count,
  SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed_count,
  CAST(EXTRACT(EPOCH FROM MIN(created_at)) * 1000 AS BIGINT) AS min_created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM MAX(created_at)) * 1000 AS BIGINT) AS max_created_at_unix_ms
FROM request_candidates
WHERE endpoint_id = ANY($1)
  AND created_at >= TO_TIMESTAMP($2)
  AND created_at <= TO_TIMESTAMP($3)
  AND status IN ('success', 'failed', 'skipped')
GROUP BY
  endpoint_id,
  FLOOR(EXTRACT(EPOCH FROM (created_at - TO_TIMESTAMP($2))) / $4)::BIGINT
"#;

const UPSERT_SQL: &str = r#"
INSERT INTO request_candidates (
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
  created_at,
  started_at,
  finished_at
)
VALUES (
  $1,
  $2,
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $9,
  $10,
  $11,
  $12,
  $13,
  COALESCE($14, false),
  $15,
  $16,
  $17,
  $18,
  $19,
  $20,
  $21,
  COALESCE(
    CASE
      WHEN $22 IS NOT NULL AND $22 > 1000.0 THEN TO_TIMESTAMP($22 / 1000.0)
    END,
    TO_TIMESTAMP($23 / 1000.0),
    TO_TIMESTAMP($24 / 1000.0),
    NOW()
  ),
  TO_TIMESTAMP($23 / 1000.0),
  TO_TIMESTAMP($24 / 1000.0)
)
ON CONFLICT (request_id, candidate_index, retry_index)
DO UPDATE SET
  user_id = COALESCE(EXCLUDED.user_id, request_candidates.user_id),
  api_key_id = COALESCE(EXCLUDED.api_key_id, request_candidates.api_key_id),
  username = COALESCE(EXCLUDED.username, request_candidates.username),
  api_key_name = COALESCE(EXCLUDED.api_key_name, request_candidates.api_key_name),
  provider_id = COALESCE(EXCLUDED.provider_id, request_candidates.provider_id),
  endpoint_id = COALESCE(EXCLUDED.endpoint_id, request_candidates.endpoint_id),
  key_id = COALESCE(EXCLUDED.key_id, request_candidates.key_id),
  status = EXCLUDED.status,
  skip_reason = COALESCE(EXCLUDED.skip_reason, request_candidates.skip_reason),
  is_cached = COALESCE($14, request_candidates.is_cached),
  status_code = COALESCE(EXCLUDED.status_code, request_candidates.status_code),
  error_type = COALESCE(EXCLUDED.error_type, request_candidates.error_type),
  error_message = COALESCE(EXCLUDED.error_message, request_candidates.error_message),
  latency_ms = COALESCE(EXCLUDED.latency_ms, request_candidates.latency_ms),
  concurrent_requests = COALESCE(EXCLUDED.concurrent_requests, request_candidates.concurrent_requests),
  extra_data = CASE
    WHEN request_candidates.extra_data IS NULL THEN EXCLUDED.extra_data
    WHEN EXCLUDED.extra_data IS NULL THEN request_candidates.extra_data
    WHEN json_typeof(request_candidates.extra_data) = 'object'
      AND json_typeof(EXCLUDED.extra_data) = 'object'
      THEN (request_candidates.extra_data::jsonb || EXCLUDED.extra_data::jsonb)::json
    ELSE EXCLUDED.extra_data
  END,
  required_capabilities = COALESCE(EXCLUDED.required_capabilities, request_candidates.required_capabilities),
  created_at = CASE
    WHEN request_candidates.created_at <= TO_TIMESTAMP(1)
      THEN EXCLUDED.created_at
    ELSE request_candidates.created_at
  END,
  started_at = COALESCE(EXCLUDED.started_at, request_candidates.started_at),
  finished_at = COALESCE(EXCLUDED.finished_at, request_candidates.finished_at)
RETURNING
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
  CAST(EXTRACT(EPOCH FROM created_at) * 1000 AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM started_at) * 1000 AS BIGINT) AS started_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM finished_at) * 1000 AS BIGINT) AS finished_at_unix_ms
"#;

const DELETE_CREATED_BEFORE_SQL: &str = r#"
DELETE FROM request_candidates
WHERE id IN (
  SELECT id
  FROM request_candidates
  WHERE created_at < TO_TIMESTAMP($1)
  ORDER BY created_at ASC, id ASC
  LIMIT $2
)
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
  WHERE admission.created_at < TO_TIMESTAMP($1)
    AND NOT EXISTS (
      SELECT 1
      FROM request_candidates candidate
      WHERE candidate.request_id = admission.request_id
    )
    AND COALESCE(settlement.billing_status, usage_record.billing_status, 'settled') <> 'pending'
  ORDER BY admission.created_at ASC, admission.request_id ASC
  LIMIT $2
)
"#;

#[derive(Debug, Clone)]
pub struct SqlxRequestCandidateReadRepository {
    pool: PgPool,
    tx_runner: PostgresTransactionRunner,
}

impl SqlxRequestCandidateReadRepository {
    pub fn new(pool: PgPool) -> Self {
        let tx_runner = PostgresTransactionRunner::new(pool.clone());
        Self { pool, tx_runner }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn transaction_runner(&self) -> &PostgresTransactionRunner {
        &self.tx_runner
    }

    pub async fn list_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        let mut builder = QueryBuilder::<Postgres>::new(candidate_columns());
        let mut where_clause = WhereClause::new();
        push_eq(
            &mut builder,
            &mut where_clause,
            "request_id",
            request_id.to_string(),
        );
        builder.push(" ORDER BY candidate_index ASC, retry_index ASC, created_at ASC");
        collect_query_rows(builder.build().fetch(&self.pool), map_request_candidate_row).await
    }

    pub async fn list_recent(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut builder = QueryBuilder::<Postgres>::new(candidate_columns());
        builder.push(" ORDER BY created_at DESC");
        push_limit(
            &mut builder,
            i64::try_from(limit).map_err(|_| {
                DataLayerError::UnexpectedValue(format!(
                    "invalid recent request candidate limit: {limit}"
                ))
            })?,
        );
        collect_query_rows(builder.build().fetch(&self.pool), map_request_candidate_row).await
    }

    pub async fn list_by_provider_id(
        &self,
        provider_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut builder = QueryBuilder::<Postgres>::new(candidate_columns());
        let mut where_clause = WhereClause::new();
        push_eq(
            &mut builder,
            &mut where_clause,
            "provider_id",
            provider_id.to_string(),
        );
        builder.push(" ORDER BY created_at DESC");
        push_limit(
            &mut builder,
            i64::try_from(limit).map_err(|_| {
                DataLayerError::UnexpectedValue(format!(
                    "invalid provider request candidate limit: {limit}"
                ))
            })?,
        );
        collect_query_rows(builder.build().fetch(&self.pool), map_request_candidate_row).await
    }

    pub async fn list_finalized_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        if endpoint_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let mut builder = QueryBuilder::<Postgres>::new(candidate_columns());
        let mut where_clause = WhereClause::new();
        push_in(&mut builder, &mut where_clause, "endpoint_id", endpoint_ids);
        builder
            .push(" AND created_at >= TO_TIMESTAMP(")
            .push_bind(since_unix_secs as f64)
            .push(") AND status IN ('success', 'failed', 'skipped') ORDER BY created_at DESC");
        push_limit(
            &mut builder,
            i64::try_from(limit).map_err(|_| {
                DataLayerError::UnexpectedValue(format!(
                    "invalid finalized request candidate limit: {limit}"
                ))
            })?,
        );
        collect_query_rows(builder.build().fetch(&self.pool), map_request_candidate_row).await
    }

    pub async fn count_attempted_with_unknown_upstream_in_window(
        &self,
        window_start_unix_ms: u64,
        window_end_unix_ms: u64,
    ) -> Result<u64, DataLayerError> {
        if window_end_unix_ms <= window_start_unix_ms {
            return Ok(0);
        }

        let row = sqlx::query(
            r#"
SELECT COUNT(*)::BIGINT AS count
FROM request_candidates
WHERE created_at >= TO_TIMESTAMP($1::double precision / 1000.0)
  AND created_at < TO_TIMESTAMP($2::double precision / 1000.0)
  AND (
    status IN ('streaming', 'success', 'failed', 'cancelled')
    OR (status = 'pending' AND started_at IS NOT NULL)
  )
  AND (
    BTRIM(COALESCE(provider_id, '')) = ''
    OR lower(BTRIM(COALESCE(provider_id, ''))) IN ('unknown', 'unknow', 'pending')
    OR BTRIM(COALESCE(key_id, '')) = ''
    OR lower(BTRIM(COALESCE(key_id, ''))) IN ('unknown', 'unknow', 'pending')
  )
"#,
        )
        .bind(unix_ms_to_i64(
            window_start_unix_ms,
            "request candidate window start",
        )?)
        .bind(unix_ms_to_i64(
            window_end_unix_ms,
            "request candidate window end",
        )?)
        .fetch_one(&self.pool)
        .await
        .map_postgres_err()?;
        Ok(row_get::<i64>(&row, "count")?.max(0) as u64)
    }

    pub async fn count_finalized_statuses_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
    ) -> Result<Vec<PublicHealthStatusCount>, DataLayerError> {
        if endpoint_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT endpoint_id, status, COUNT(id) AS count FROM request_candidates",
        );
        let mut where_clause = WhereClause::new();
        push_in(&mut builder, &mut where_clause, "endpoint_id", endpoint_ids);
        builder
            .push(" AND created_at >= TO_TIMESTAMP(")
            .push_bind(since_unix_secs as f64)
            .push(") AND status IN ('success', 'failed', 'skipped') GROUP BY endpoint_id, status");
        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_postgres_err()?;
        rows.iter()
            .map(|row| {
                Ok(PublicHealthStatusCount {
                    endpoint_id: row_get(row, "endpoint_id")?,
                    status: RequestCandidateStatus::from_database(
                        row_get::<String>(row, "status")?.as_str(),
                    )?,
                    count: u64::try_from(row_get::<i64>(row, "count")?).map_err(|_| {
                        DataLayerError::UnexpectedValue(
                            "public health status count out of range".to_string(),
                        )
                    })?,
                })
            })
            .collect()
    }

    pub async fn aggregate_finalized_timeline_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
        until_unix_secs: u64,
        segments: u32,
    ) -> Result<Vec<PublicHealthTimelineBucket>, DataLayerError> {
        if endpoint_ids.is_empty() || segments == 0 || until_unix_secs < since_unix_secs {
            return Ok(Vec::new());
        }

        let span_seconds = until_unix_secs.saturating_sub(since_unix_secs);
        let segment_seconds = if span_seconds == 0 {
            1.0
        } else {
            (span_seconds as f64) / (segments as f64)
        };

        let mut rows = sqlx::query(AGGREGATE_FINALIZED_TIMELINE_BY_ENDPOINT_IDS_SINCE_SQL)
            .bind(endpoint_ids)
            .bind(since_unix_secs as f64)
            .bind(until_unix_secs as f64)
            .bind(segment_seconds)
            .fetch(&self.pool);
        let mut buckets = Vec::new();
        while let Some(row) = rows.try_next().await.map_postgres_err()? {
            let bucket = {
                let raw_segment_idx = row_get::<i64>(&row, "segment_idx")?;
                let segment_idx = if raw_segment_idx < 0 {
                    0
                } else {
                    u32::try_from(raw_segment_idx).map_err(|_| {
                        DataLayerError::UnexpectedValue(format!(
                            "public health segment idx out of range: {raw_segment_idx}"
                        ))
                    })?
                }
                .min(segments.saturating_sub(1));

                PublicHealthTimelineBucket {
                    endpoint_id: row_get(&row, "endpoint_id")?,
                    segment_idx,
                    total_count: u64::try_from(row_get::<i64>(&row, "total_count")?).map_err(
                        |_| {
                            DataLayerError::UnexpectedValue(
                                "public health total_count out of range".to_string(),
                            )
                        },
                    )?,
                    success_count: u64::try_from(row_get::<i64>(&row, "success_count")?).map_err(
                        |_| {
                            DataLayerError::UnexpectedValue(
                                "public health success_count out of range".to_string(),
                            )
                        },
                    )?,
                    failed_count: u64::try_from(row_get::<i64>(&row, "failed_count")?).map_err(
                        |_| {
                            DataLayerError::UnexpectedValue(
                                "public health failed_count out of range".to_string(),
                            )
                        },
                    )?,
                    min_created_at_unix_ms: row_get::<Option<i64>>(&row, "min_created_at_unix_ms")?
                        .map(|value| {
                            u64::try_from(value).map_err(|_| {
                                DataLayerError::UnexpectedValue(format!(
                                    "public health min_created_at_unix_ms out of range: {value}"
                                ))
                            })
                        })
                        .transpose()?,
                    max_created_at_unix_ms: row_get::<Option<i64>>(&row, "max_created_at_unix_ms")?
                        .map(|value| {
                            u64::try_from(value).map_err(|_| {
                                DataLayerError::UnexpectedValue(format!(
                                    "public health max_created_at_unix_ms out of range: {value}"
                                ))
                            })
                        })
                        .transpose()?,
                }
            };
            buckets.push(bucket);
        }
        Ok(buckets)
    }

    pub async fn upsert(
        &self,
        candidate: UpsertRequestCandidateRecord,
    ) -> Result<StoredRequestCandidate, DataLayerError> {
        candidate.validate()?;
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move { upsert_candidate_postgres(tx, candidate).await })
                    as BoxFuture<'_, Result<StoredRequestCandidate, DataLayerError>>
            })
            .await
    }

    pub async fn upsert_with_billing_admission(
        &self,
        candidate: UpsertRequestCandidateRecord,
        admission: BillingRequestAdmissionInput,
    ) -> Result<(StoredRequestCandidate, BillingRequestAdmissionRecord), DataLayerError> {
        super::admission::validate_candidate_admission_identity(&candidate, &admission)?;
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let stored_admission =
                        insert_billing_admission_postgres(tx, &admission).await?;
                    super::admission::validate_candidate_provider(
                        &candidate,
                        &stored_admission.to_input(),
                    )?;
                    let stored = upsert_candidate_postgres(tx, candidate).await?;
                    Ok((stored, stored_admission))
                })
            })
            .await
    }

    pub async fn delete_created_before(
        &self,
        created_before_unix_secs: u64,
        limit: usize,
    ) -> Result<usize, DataLayerError> {
        if limit == 0 {
            return Ok(0);
        }

        let result = sqlx::query(DELETE_CREATED_BEFORE_SQL)
            .bind(created_before_unix_secs as f64)
            .bind(i64::try_from(limit).map_err(|_| {
                DataLayerError::UnexpectedValue(format!(
                    "invalid request candidate delete limit: {limit}"
                ))
            })?)
            .execute(&self.pool)
            .await
            .map_postgres_err()?;
        sqlx::query(DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL)
            .bind(created_before_unix_secs as f64)
            .bind(i64::try_from(limit).map_err(|_| {
                DataLayerError::UnexpectedValue(format!(
                    "invalid billing admission delete limit: {limit}"
                ))
            })?)
            .execute(&self.pool)
            .await
            .map_postgres_err()?;
        Ok(result.rows_affected() as usize)
    }
}

#[async_trait]
impl RequestCandidateReadRepository for SqlxRequestCandidateReadRepository {
    async fn find_billing_admission(
        &self,
        request_id: &str,
    ) -> Result<Option<BillingRequestAdmissionRecord>, DataLayerError> {
        let row = sqlx::query(
            r#"
SELECT request_id, user_id, api_key_id, wallet_id, global_model_id, funding_source,
       CAST(wallet_balance_at_admission AS DOUBLE PRECISION) AS wallet_balance_at_admission,
       wallet_payment_allowed, wallet_overage_allowed,
       entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
       billing_admitted, status, rejection_reason, schema_version, created_at, updated_at
FROM billing_request_admissions
WHERE request_id = $1
LIMIT 1
            "#,
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await
        .map_postgres_err()?;
        row.as_ref().map(map_billing_admission_row).transpose()
    }

    async fn list_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        Self::list_by_request_id(self, request_id).await
    }

    async fn list_recent(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        Self::list_recent(self, limit).await
    }

    async fn list_finalized_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        Self::list_finalized_by_endpoint_ids_since(self, endpoint_ids, since_unix_secs, limit).await
    }

    async fn list_by_provider_id(
        &self,
        provider_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, DataLayerError> {
        Self::list_by_provider_id(self, provider_id, limit).await
    }

    async fn count_attempted_with_unknown_upstream_in_window(
        &self,
        window_start_unix_ms: u64,
        window_end_unix_ms: u64,
    ) -> Result<u64, DataLayerError> {
        Self::count_attempted_with_unknown_upstream_in_window(
            self,
            window_start_unix_ms,
            window_end_unix_ms,
        )
        .await
    }

    async fn count_finalized_statuses_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
    ) -> Result<Vec<PublicHealthStatusCount>, DataLayerError> {
        Self::count_finalized_statuses_by_endpoint_ids_since(self, endpoint_ids, since_unix_secs)
            .await
    }

    async fn aggregate_finalized_timeline_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
        until_unix_secs: u64,
        segments: u32,
    ) -> Result<Vec<PublicHealthTimelineBucket>, DataLayerError> {
        Self::aggregate_finalized_timeline_by_endpoint_ids_since(
            self,
            endpoint_ids,
            since_unix_secs,
            until_unix_secs,
            segments,
        )
        .await
    }
}

#[async_trait]
impl RequestCandidateWriteRepository for SqlxRequestCandidateReadRepository {
    async fn upsert(
        &self,
        candidate: UpsertRequestCandidateRecord,
    ) -> Result<StoredRequestCandidate, DataLayerError> {
        Self::upsert(self, candidate).await
    }

    async fn upsert_with_billing_admission(
        &self,
        candidate: UpsertRequestCandidateRecord,
        admission: BillingRequestAdmissionInput,
    ) -> Result<(StoredRequestCandidate, BillingRequestAdmissionRecord), DataLayerError> {
        Self::upsert_with_billing_admission(self, candidate, admission).await
    }

    async fn delete_created_before(
        &self,
        created_before_unix_secs: u64,
        limit: usize,
    ) -> Result<usize, DataLayerError> {
        Self::delete_created_before(self, created_before_unix_secs, limit).await
    }
}

async fn upsert_candidate_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    candidate: UpsertRequestCandidateRecord,
) -> Result<StoredRequestCandidate, DataLayerError> {
    let row = sqlx::query(UPSERT_SQL)
        .bind(if candidate.id.trim().is_empty() {
            Uuid::new_v4().to_string()
        } else {
            candidate.id.clone()
        })
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
        .bind(&candidate.extra_data)
        .bind(&candidate.required_capabilities)
        .bind(candidate.created_at_unix_ms.map(|value| value as f64))
        .bind(candidate.started_at_unix_ms.map(|value| value as f64))
        .bind(candidate.finished_at_unix_ms.map(|value| value as f64))
        .fetch_one(&mut **tx)
        .await
        .map_postgres_err()?;
    map_request_candidate_row(&row)
}

async fn insert_billing_admission_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    admission: &BillingRequestAdmissionInput,
) -> Result<BillingRequestAdmissionRecord, DataLayerError> {
    let row = sqlx::query(
        r#"
INSERT INTO billing_request_admissions (
  request_id, user_id, api_key_id, wallet_id, global_model_id, funding_source,
  wallet_balance_at_admission, wallet_payment_allowed, wallet_overage_allowed,
  entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
  billing_admitted, status, rejection_reason, schema_version, created_at, updated_at
)
VALUES (
  $1, $2, $3, $4, $5, $6, CAST($7 AS NUMERIC(20,8)), $8, $9, $10, $11, $12,
  TRUE, 'admitted', NULL, $13, NOW(), NOW()
)
ON CONFLICT (request_id) DO UPDATE
SET request_id = EXCLUDED.request_id
RETURNING request_id, user_id, api_key_id, wallet_id, global_model_id, funding_source,
          CAST(wallet_balance_at_admission AS DOUBLE PRECISION) AS wallet_balance_at_admission,
          wallet_payment_allowed, wallet_overage_allowed,
          entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
          billing_admitted, status, rejection_reason, schema_version, created_at, updated_at,
          CAST(CAST($7 AS NUMERIC(20,8)) AS DOUBLE PRECISION)
            AS expected_wallet_balance_at_admission
        "#,
    )
    .bind(&admission.request_id)
    .bind(&admission.user_id)
    .bind(&admission.api_key_id)
    .bind(&admission.wallet_id)
    .bind(&admission.global_model_id)
    .bind(admission.funding_source.as_str())
    .bind(admission.wallet_balance_at_admission)
    .bind(admission.wallet_payment_allowed)
    .bind(admission.wallet_overage_allowed)
    .bind(
        serde_json::to_value(&admission.entitlement_ids).map_err(|error| {
            DataLayerError::UnexpectedValue(format!(
                "billing entitlement ids encode failed: {error}"
            ))
        })?,
    )
    .bind(
        serde_json::to_value(&admission.entitlement_provider_scopes).map_err(|error| {
            DataLayerError::UnexpectedValue(format!(
                "billing entitlement provider scopes encode failed: {error}"
            ))
        })?,
    )
    .bind(
        serde_json::to_value(&admission.allowed_provider_ids).map_err(|error| {
            DataLayerError::UnexpectedValue(format!("billing provider ids encode failed: {error}"))
        })?,
    )
    .bind(i16::try_from(admission.schema_version).map_err(|_| {
        DataLayerError::InvalidInput("billing admission schema_version overflow".to_string())
    })?)
    .fetch_one(&mut **tx)
    .await
    .map_postgres_err()?;
    let stored = map_billing_admission_row(&row)?;
    let mut persisted_admission = admission.clone();
    persisted_admission.wallet_balance_at_admission =
        row_get(&row, "expected_wallet_balance_at_admission")?;
    super::admission::validate_stored_admission_matches_input(&stored, &persisted_admission)?;
    Ok(stored)
}

async fn collect_query_rows<T, S>(
    mut rows: S,
    map_row: fn(&PgRow) -> Result<T, DataLayerError>,
) -> Result<Vec<T>, DataLayerError>
where
    S: TryStream<Ok = PgRow, Error = sqlx::Error> + Unpin,
{
    let mut items = Vec::new();
    while let Some(row) = rows.try_next().await.map_postgres_err()? {
        items.push(map_row(&row)?);
    }
    Ok(items)
}

fn map_request_candidate_row(row: &PgRow) -> Result<StoredRequestCandidate, DataLayerError> {
    let status = RequestCandidateStatus::from_database(row_get::<String>(row, "status")?.as_str())?;
    StoredRequestCandidate::new(
        row_get(row, "id")?,
        row_get(row, "request_id")?,
        row_get(row, "user_id")?,
        row_get(row, "api_key_id")?,
        row_get(row, "username")?,
        row_get(row, "api_key_name")?,
        row_get(row, "candidate_index")?,
        row_get(row, "retry_index")?,
        row_get(row, "provider_id")?,
        row_get(row, "endpoint_id")?,
        row_get(row, "key_id")?,
        status,
        row_get(row, "skip_reason")?,
        row_get(row, "is_cached")?,
        row_get(row, "status_code")?,
        row_get(row, "error_type")?,
        row_get(row, "error_message")?,
        row_get(row, "latency_ms")?,
        row_get(row, "concurrent_requests")?,
        row_get(row, "extra_data")?,
        row_get(row, "required_capabilities")?,
        row_get(row, "created_at_unix_ms")?,
        row_get(row, "started_at_unix_ms")?,
        row_get(row, "finished_at_unix_ms")?,
    )
}

fn map_billing_admission_row(row: &PgRow) -> Result<BillingRequestAdmissionRecord, DataLayerError> {
    let entitlement_ids = serde_json::from_value::<Vec<String>>(row_get(row, "entitlement_ids")?)
        .map_err(|error| {
        DataLayerError::UnexpectedValue(format!(
            "billing admission entitlement ids are invalid: {error}"
        ))
    })?;
    let allowed_provider_ids =
        serde_json::from_value::<Vec<String>>(row_get(row, "allowed_provider_ids")?).map_err(
            |error| {
                DataLayerError::UnexpectedValue(format!(
                    "billing admission provider ids are invalid: {error}"
                ))
            },
        )?;
    let entitlement_provider_scopes =
        serde_json::from_value(row_get(row, "entitlement_provider_scopes")?).map_err(|error| {
            DataLayerError::UnexpectedValue(format!(
                "billing admission entitlement provider scopes are invalid: {error}"
            ))
        })?;
    let schema_version: i16 = row_get(row, "schema_version")?;
    let created_at: chrono::DateTime<chrono::Utc> = row_get(row, "created_at")?;
    let updated_at: chrono::DateTime<chrono::Utc> = row_get(row, "updated_at")?;
    Ok(BillingRequestAdmissionRecord {
        request_id: row_get(row, "request_id")?,
        user_id: row_get(row, "user_id")?,
        api_key_id: row_get(row, "api_key_id")?,
        wallet_id: row_get(row, "wallet_id")?,
        global_model_id: row_get(row, "global_model_id")?,
        funding_source: BillingFundingSource::from_database(&row_get::<String>(
            row,
            "funding_source",
        )?)?,
        wallet_balance_at_admission: row_get(row, "wallet_balance_at_admission")?,
        wallet_payment_allowed: row_get(row, "wallet_payment_allowed")?,
        wallet_overage_allowed: row_get(row, "wallet_overage_allowed")?,
        entitlement_ids,
        entitlement_provider_scopes,
        allowed_provider_ids,
        billing_admitted: row_get(row, "billing_admitted")?,
        status: row_get(row, "status")?,
        rejection_reason: row_get(row, "rejection_reason")?,
        schema_version: u16::try_from(schema_version).map_err(|_| {
            DataLayerError::UnexpectedValue(
                "billing admission schema_version is invalid".to_string(),
            )
        })?,
        created_at_unix_ms: u64::try_from(created_at.timestamp_millis()).map_err(|_| {
            DataLayerError::UnexpectedValue("billing admission created_at is invalid".to_string())
        })?,
        updated_at_unix_ms: u64::try_from(updated_at.timestamp_millis()).map_err(|_| {
            DataLayerError::UnexpectedValue("billing admission updated_at is invalid".to_string())
        })?,
    })
}

fn row_get<T>(row: &PgRow, column: &str) -> Result<T, DataLayerError>
where
    for<'r> T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column).map_postgres_err()
}

fn candidate_columns() -> &'static str {
    LIST_BY_REQUEST_ID_SQL
        .split_once("WHERE request_id = $1")
        .map(|(prefix, _)| prefix)
        .unwrap_or(LIST_BY_REQUEST_ID_SQL)
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

fn unix_ms_to_i64(value: u64, name: &str) -> Result<i64, DataLayerError> {
    i64::try_from(value)
        .map_err(|_| DataLayerError::UnexpectedValue(format!("{name} exceeds i64: {value}")))
}

#[cfg(test)]
mod tests {
    use super::{
        SqlxRequestCandidateReadRepository, DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL,
        UPSERT_SQL,
    };
    use crate::driver::postgres::{PostgresPoolConfig, PostgresPoolFactory};
    use crate::repository::candidates::{
        RequestCandidateReadRepository, RequestCandidateStatus, UpsertRequestCandidateRecord,
    };
    use aether_data_contracts::repository::billing::{
        BillingFundingSource, BillingRequestAdmissionInput,
    };

    #[test]
    fn upsert_sql_does_not_default_missing_or_epoch_created_at_to_epoch() {
        assert!(!UPSERT_SQL.contains("COALESCE($22, 0)"));
        assert!(UPSERT_SQL.contains("WHEN $22 IS NOT NULL AND $22 > 1000.0"));
        assert!(UPSERT_SQL.contains("TO_TIMESTAMP($22 / 1000.0)"));
        assert!(UPSERT_SQL.contains("TO_TIMESTAMP($23 / 1000.0)"));
        assert!(UPSERT_SQL.contains("TO_TIMESTAMP($24 / 1000.0)"));
        assert!(UPSERT_SQL.contains("NOW()"));
        assert!(UPSERT_SQL.contains("request_candidates.created_at <= TO_TIMESTAMP(1)"));
        assert!(UPSERT_SQL.contains("THEN EXCLUDED.created_at"));
    }

    #[test]
    fn billing_admission_cleanup_keeps_pending_costs_and_live_candidates() {
        assert!(DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL.contains(
            "COALESCE(settlement.billing_status, usage_record.billing_status, 'settled') <> 'pending'"
        ));
        assert!(DELETE_SETTLED_BILLING_ADMISSIONS_CREATED_BEFORE_SQL
            .contains("FROM request_candidates candidate"));
    }

    #[tokio::test]
    async fn repository_constructs_from_lazy_pool() {
        let factory = PostgresPoolFactory::new(PostgresPoolConfig {
            database_url: "postgres://localhost/aether".to_string(),
            min_connections: 1,
            max_connections: 4,
            acquire_timeout_ms: 1_000,
            idle_timeout_ms: 5_000,
            max_lifetime_ms: 30_000,
            statement_cache_capacity: 64,
            require_ssl: false,
        })
        .expect("factory should build");

        let pool = factory.connect_lazy().expect("pool should build");
        let repository = SqlxRequestCandidateReadRepository::new(pool);
        let _ = repository.pool();
        let _ = repository.transaction_runner();
    }

    #[tokio::test]
    async fn postgres_candidate_admission_round_trips_numeric_balance_when_url_is_set() {
        let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            eprintln!(
                "skipping postgres candidate admission test because AETHER_TEST_POSTGRES_URL is unset"
            );
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("postgres test pool should connect");
        crate::lifecycle::migrate::run_migrations(&pool)
            .await
            .expect("postgres migrations should run");

        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let request_id = format!("pg-admission-{suffix}");
        let repository = SqlxRequestCandidateReadRepository::new(pool.clone());
        let candidate = UpsertRequestCandidateRecord {
            id: uuid::Uuid::new_v4().to_string(),
            request_id: request_id.clone(),
            user_id: None,
            api_key_id: None,
            username: None,
            api_key_name: None,
            candidate_index: 0,
            retry_index: 0,
            provider_id: None,
            endpoint_id: None,
            key_id: None,
            status: RequestCandidateStatus::Pending,
            skip_reason: None,
            is_cached: Some(false),
            status_code: None,
            error_type: None,
            error_message: None,
            latency_ms: None,
            concurrent_requests: None,
            extra_data: None,
            required_capabilities: None,
            created_at_unix_ms: Some(1_000_000),
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
        };
        let admission = BillingRequestAdmissionInput {
            request_id: request_id.clone(),
            user_id: None,
            api_key_id: None,
            wallet_id: None,
            global_model_id: Some("global-model-1".to_string()),
            funding_source: BillingFundingSource::Wallet,
            wallet_balance_at_admission: Some(-626.123_456_789),
            wallet_payment_allowed: true,
            wallet_overage_allowed: false,
            entitlement_ids: Vec::new(),
            entitlement_provider_scopes: Default::default(),
            allowed_provider_ids: Vec::new(),
            schema_version: 1,
        };

        let (_, inserted) = repository
            .upsert_with_billing_admission(candidate, admission)
            .await
            .expect("postgres should return the inserted numeric billing balance");
        assert_eq!(inserted.wallet_balance_at_admission, Some(-626.123_456_79));

        let loaded = repository
            .find_billing_admission(&request_id)
            .await
            .expect("postgres should decode the stored numeric billing balance")
            .expect("billing admission should exist");
        assert_eq!(loaded.wallet_balance_at_admission, Some(-626.123_456_79));

        sqlx::query("DELETE FROM request_candidates WHERE request_id = $1")
            .bind(&request_id)
            .execute(&pool)
            .await
            .expect("candidate should clean up");
        sqlx::query("DELETE FROM billing_request_admissions WHERE request_id = $1")
            .bind(&request_id)
            .execute(&pool)
            .await
            .expect("billing admission should clean up");
    }
}

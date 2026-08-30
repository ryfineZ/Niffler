use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::{
    finite_wallet_available_usd, plan_finite_wallet_debit,
    settlement_billing_status_for_usage_status, settlement_wallet_charge_multiplier,
    SettlementBillingAdmission, SettlementWriteRepository, StoredUsageSettlement,
    UsageSettlementInput, SETTLEMENT_EPSILON_USD,
};
use crate::driver::postgres::PostgresTransactionRunner;
use crate::error::SqlxResultExt;
use crate::repository::billing::quota::{
    entitlement_allows_global_model, entitlements_snapshot_has_usage_quota_for_global_model,
    quota_base_amount, quota_debit_amount, usage_quota_grants_from_entitlement,
    StoredUsageQuotaWindow, StoredUsageQuotaWindows, UsageQuotaGrant,
};
use crate::repository::usage::postgres::provider_contribution::sync_provider_api_key_usage_contribution_for_request_in_tx;
use crate::DataLayerError;

const FIND_USAGE_FOR_SETTLEMENT_SQL: &str = r#"
SELECT
  usage_record.request_id,
  COALESCE(usage_settlement_snapshots.wallet_id, usage_record.wallet_id) AS wallet_id,
  COALESCE(usage_settlement_snapshots.billing_status, usage_record.billing_status) AS billing_status,
  COALESCE(
    CAST(usage_settlement_snapshots.wallet_balance_before AS DOUBLE PRECISION),
    CAST(usage_record.wallet_balance_before AS DOUBLE PRECISION)
  ) AS wallet_balance_before,
  COALESCE(
    CAST(usage_settlement_snapshots.wallet_balance_after AS DOUBLE PRECISION),
    CAST(usage_record.wallet_balance_after AS DOUBLE PRECISION)
  ) AS wallet_balance_after,
  COALESCE(
    CAST(usage_settlement_snapshots.wallet_recharge_balance_before AS DOUBLE PRECISION),
    CAST(usage_record.wallet_recharge_balance_before AS DOUBLE PRECISION)
  ) AS wallet_recharge_balance_before,
  COALESCE(
    CAST(usage_settlement_snapshots.wallet_recharge_balance_after AS DOUBLE PRECISION),
    CAST(usage_record.wallet_recharge_balance_after AS DOUBLE PRECISION)
  ) AS wallet_recharge_balance_after,
  COALESCE(
    CAST(usage_settlement_snapshots.wallet_gift_balance_before AS DOUBLE PRECISION),
    CAST(usage_record.wallet_gift_balance_before AS DOUBLE PRECISION)
  ) AS wallet_gift_balance_before,
  COALESCE(
    CAST(usage_settlement_snapshots.wallet_gift_balance_after AS DOUBLE PRECISION),
    CAST(usage_record.wallet_gift_balance_after AS DOUBLE PRECISION)
  ) AS wallet_gift_balance_after,
  CAST(usage_settlement_snapshots.provider_monthly_used_usd AS DOUBLE PRECISION) AS provider_monthly_used_usd,
  usage_record.provider_id,
  CAST(
    EXTRACT(
      EPOCH FROM COALESCE(usage_settlement_snapshots.finalized_at, usage_record.finalized_at)
    ) AS BIGINT
  ) AS finalized_at_unix_secs
FROM "usage" AS usage_record
LEFT JOIN usage_settlement_snapshots
  ON usage_settlement_snapshots.request_id = usage_record.request_id
WHERE usage_record.request_id = $1
FOR UPDATE OF usage_record
"#;

const FINALIZE_USAGE_BILLING_SQL: &str = r#"
UPDATE "usage"
SET
  billing_status = $2,
  finalized_at = COALESCE(finalized_at, to_timestamp($3))
WHERE request_id = $1
"#;

const UPSERT_USAGE_SETTLEMENT_SNAPSHOT_SQL: &str = r#"
INSERT INTO usage_settlement_snapshots (
  request_id,
  billing_status,
  wallet_id,
  wallet_balance_before,
  wallet_balance_after,
  wallet_recharge_balance_before,
  wallet_recharge_balance_after,
  wallet_gift_balance_before,
  wallet_gift_balance_after,
  provider_monthly_used_usd,
  finalized_at
) VALUES (
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
  CASE
    WHEN $11 IS NULL THEN NULL
    ELSE TO_TIMESTAMP($11::double precision)
  END
)
ON CONFLICT (request_id)
DO UPDATE SET
  billing_status = EXCLUDED.billing_status,
  wallet_id = COALESCE(EXCLUDED.wallet_id, usage_settlement_snapshots.wallet_id),
  wallet_balance_before = COALESCE(
    EXCLUDED.wallet_balance_before,
    usage_settlement_snapshots.wallet_balance_before
  ),
  wallet_balance_after = COALESCE(
    EXCLUDED.wallet_balance_after,
    usage_settlement_snapshots.wallet_balance_after
  ),
  wallet_recharge_balance_before = COALESCE(
    EXCLUDED.wallet_recharge_balance_before,
    usage_settlement_snapshots.wallet_recharge_balance_before
  ),
  wallet_recharge_balance_after = COALESCE(
    EXCLUDED.wallet_recharge_balance_after,
    usage_settlement_snapshots.wallet_recharge_balance_after
  ),
  wallet_gift_balance_before = COALESCE(
    EXCLUDED.wallet_gift_balance_before,
    usage_settlement_snapshots.wallet_gift_balance_before
  ),
  wallet_gift_balance_after = COALESCE(
    EXCLUDED.wallet_gift_balance_after,
    usage_settlement_snapshots.wallet_gift_balance_after
  ),
  provider_monthly_used_usd = COALESCE(
    EXCLUDED.provider_monthly_used_usd,
    usage_settlement_snapshots.provider_monthly_used_usd
  ),
  finalized_at = COALESCE(EXCLUDED.finalized_at, usage_settlement_snapshots.finalized_at),
  updated_at = NOW()
"#;

const ENQUEUE_PROVIDER_MONTHLY_USAGE_DELTA_SQL: &str = r#"
INSERT INTO usage_counter_deltas (
  id,
  request_id,
  kind,
  target_id,
  total_cost_usd_delta
) VALUES (
  $1,
  $2,
  'provider_monthly',
  $3,
  $4
)
"#;

#[derive(Debug, Clone)]
pub struct SqlxSettlementRepository {
    tx_runner: PostgresTransactionRunner,
}

impl SqlxSettlementRepository {
    pub fn new(pool: PgPool) -> Self {
        let tx_runner = PostgresTransactionRunner::new(pool);
        Self { tx_runner }
    }
}

fn settlement_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredUsageSettlement, DataLayerError> {
    Ok(StoredUsageSettlement {
        request_id: row.try_get("request_id").map_postgres_err()?,
        wallet_id: row.try_get("wallet_id").map_postgres_err()?,
        billing_status: row.try_get("billing_status").map_postgres_err()?,
        wallet_balance_before: row.try_get("wallet_balance_before").map_postgres_err()?,
        wallet_balance_after: row.try_get("wallet_balance_after").map_postgres_err()?,
        wallet_recharge_balance_before: row
            .try_get("wallet_recharge_balance_before")
            .map_postgres_err()?,
        wallet_recharge_balance_after: row
            .try_get("wallet_recharge_balance_after")
            .map_postgres_err()?,
        wallet_gift_balance_before: row
            .try_get("wallet_gift_balance_before")
            .map_postgres_err()?,
        wallet_gift_balance_after: row
            .try_get("wallet_gift_balance_after")
            .map_postgres_err()?,
        provider_monthly_used_usd: row
            .try_get("provider_monthly_used_usd")
            .map_postgres_err()?,
        finalized_at_unix_secs: row
            .try_get::<Option<i64>, _>("finalized_at_unix_secs")
            .map_postgres_err()?
            .map(|value| value as u64),
    })
}

async fn refresh_finalized_settlement_after_lock_wait(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request_id: &str,
) -> Result<StoredUsageSettlement, DataLayerError> {
    let sql = FIND_USAGE_FOR_SETTLEMENT_SQL
        .trim_end()
        .strip_suffix("FOR UPDATE OF usage_record")
        .ok_or_else(|| {
            DataLayerError::UnexpectedValue(
                "settlement query is missing the expected usage row lock".to_string(),
            )
        })?;
    let row = sqlx::query(sql)
        .bind(request_id)
        .fetch_one(&mut **tx)
        .await
        .map_postgres_err()?;
    settlement_from_row(&row)
}

async fn find_billing_admission_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    request_id: &str,
) -> Result<Option<SettlementBillingAdmission>, DataLayerError> {
    let row = sqlx::query(
        r#"
SELECT funding_source, wallet_id, wallet_payment_allowed, wallet_overage_allowed,
       entitlement_ids, entitlement_provider_scopes, allowed_provider_ids
FROM billing_request_admissions
WHERE request_id = $1 AND billing_admitted = TRUE AND status = 'admitted'
FOR UPDATE
        "#,
    )
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_postgres_err()?;
    row.map(|row| {
        let funding_source: String = row.try_get("funding_source").map_postgres_err()?;
        let entitlement_ids: serde_json::Value =
            row.try_get("entitlement_ids").map_postgres_err()?;
        let allowed_provider_ids: serde_json::Value =
            row.try_get("allowed_provider_ids").map_postgres_err()?;
        let entitlement_provider_scopes = serde_json::from_value(
            row.try_get("entitlement_provider_scopes")
                .map_postgres_err()?,
        )
        .map_err(|error| {
            DataLayerError::UnexpectedValue(format!(
                "billing_request_admissions.entitlement_provider_scopes invalid json: {error}"
            ))
        })?;
        Ok(SettlementBillingAdmission {
            funding_source:
                aether_data_contracts::repository::billing::BillingFundingSource::from_database(
                    &funding_source,
                )?,
            wallet_id: row.try_get("wallet_id").map_postgres_err()?,
            wallet_payment_allowed: row.try_get("wallet_payment_allowed").map_postgres_err()?,
            wallet_overage_allowed: row.try_get("wallet_overage_allowed").map_postgres_err()?,
            entitlement_ids: json_string_vec(&entitlement_ids),
            entitlement_provider_scopes,
            allowed_provider_ids: json_string_vec(&allowed_provider_ids),
        })
    })
    .transpose()
}

fn json_string_vec(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

async fn sync_usage_settlement_snapshot<'e, E>(
    executor: E,
    settlement: &StoredUsageSettlement,
) -> Result<(), DataLayerError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(UPSERT_USAGE_SETTLEMENT_SNAPSHOT_SQL)
        .bind(&settlement.request_id)
        .bind(&settlement.billing_status)
        .bind(settlement.wallet_id.as_deref())
        .bind(settlement.wallet_balance_before)
        .bind(settlement.wallet_balance_after)
        .bind(settlement.wallet_recharge_balance_before)
        .bind(settlement.wallet_recharge_balance_after)
        .bind(settlement.wallet_gift_balance_before)
        .bind(settlement.wallet_gift_balance_after)
        .bind(settlement.provider_monthly_used_usd)
        .bind(settlement.finalized_at_unix_secs.map(|value| value as f64))
        .execute(executor)
        .await
        .map_postgres_err()?;
    Ok(())
}

async fn finalize_usage_billing_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    settlement: &StoredUsageSettlement,
    final_billing_status: &str,
    finalized_at: i64,
) -> Result<(), DataLayerError> {
    sync_usage_settlement_snapshot(&mut **tx, settlement).await?;
    sqlx::query(FINALIZE_USAGE_BILLING_SQL)
        .bind(&settlement.request_id)
        .bind(final_billing_status)
        .bind(finalized_at)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?;
    sync_provider_api_key_usage_contribution_for_request_in_tx(tx, &settlement.request_id).await
}

async fn enqueue_provider_monthly_usage_delta<'e, E>(
    executor: E,
    request_id: &str,
    provider_id: &str,
    total_cost_usd_delta: f64,
) -> Result<(), DataLayerError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let request_id = request_id.trim();
    let provider_id = provider_id.trim();
    if request_id.is_empty() || provider_id.is_empty() || total_cost_usd_delta == 0.0 {
        return Ok(());
    }
    if !total_cost_usd_delta.is_finite() {
        return Err(DataLayerError::UnexpectedValue(format!(
            "provider monthly usage delta is not finite for {provider_id}"
        )));
    }

    sqlx::query(ENQUEUE_PROVIDER_MONTHLY_USAGE_DELTA_SQL)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(request_id)
        .bind(provider_id)
        .bind(total_cost_usd_delta)
        .execute(executor)
        .await
        .map_postgres_err()?;
    Ok(())
}

#[derive(Debug, Default)]
struct DailyQuotaDebitResult {
    covered_base_usd: f64,
    insufficient: bool,
}

struct DailyQuotaDebitInput<'a> {
    user_id: &'a str,
    request_id: &'a str,
    total_cost_usd: f64,
    wallet_available_usd: Option<f64>,
    wallet_can_overdraft: bool,
    wallet_charge_multiplier: f64,
    request_global_model_id: Option<&'a str>,
    admitted_entitlement_ids: Option<&'a [String]>,
    force_wallet_overage: bool,
}

async fn consume_daily_quota_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    input: DailyQuotaDebitInput<'_>,
) -> Result<DailyQuotaDebitResult, DataLayerError> {
    if input.total_cost_usd <= 0.0 {
        return Ok(DailyQuotaDebitResult::default());
    }
    let now = chrono::Utc::now();
    let entitlement_rows = sqlx::query(
        r#"
SELECT id, starts_at, entitlements_snapshot
FROM user_plan_entitlements
WHERE user_id = $1
  AND status = 'active'
  AND starts_at <= NOW()
  AND expires_at > NOW()
ORDER BY expires_at ASC, created_at ASC, id ASC
FOR UPDATE
        "#,
    )
    .bind(input.user_id)
    .fetch_all(&mut **tx)
    .await
    .map_postgres_err()?;
    let mut grants = Vec::new();
    for row in entitlement_rows {
        let entitlement_id: String = row.try_get("id").map_postgres_err()?;
        if input
            .admitted_entitlement_ids
            .is_some_and(|ids| !ids.iter().any(|admitted_id| admitted_id == &entitlement_id))
        {
            continue;
        }
        let entitlement_started_at: chrono::DateTime<chrono::Utc> =
            row.try_get("starts_at").map_postgres_err()?;
        let entitlements: serde_json::Value =
            row.try_get("entitlements_snapshot").map_postgres_err()?;
        if input.admitted_entitlement_ids.is_none()
            && input.request_global_model_id.is_some()
            && !entitlements_snapshot_has_usage_quota_for_global_model(
                &entitlements,
                input.request_global_model_id,
            )
        {
            continue;
        }
        let stored_windows = find_usage_quota_windows_postgres(tx, &entitlement_id).await?;
        grants.extend(usage_quota_grants_from_entitlement(
            &entitlement_id,
            &entitlements,
            now,
            entitlement_started_at,
            Some(&stored_windows),
        )?);
    }
    if input.admitted_entitlement_ids.is_none() {
        grants.retain(|grant| {
            entitlement_allows_global_model(
                grant.allowed_global_model_ids.as_deref(),
                input.request_global_model_id,
            )
        });
    }
    if grants.is_empty() {
        return Ok(DailyQuotaDebitResult::default());
    }

    let mut grants_by_entitlement: std::collections::BTreeMap<String, Vec<(UsageQuotaGrant, f64)>> =
        std::collections::BTreeMap::new();
    let mut total_base_remaining = 0.0;
    let mut allow_wallet_overage = true;
    for grant in grants {
        allow_wallet_overage &= grant.allow_wallet_overage;
        let used = upsert_usage_quota_window_postgres(tx, input.user_id, &grant).await?;
        let remaining = (grant.limit_usd - used).max(0.0);
        grants_by_entitlement
            .entry(grant.entitlement_id.clone())
            .or_default()
            .push((grant, remaining));
    }
    let mut entitlement_remaining = Vec::new();
    for grants in grants_by_entitlement.into_values() {
        let remaining = grants
            .iter()
            .map(|(_, remaining)| *remaining)
            .fold(f64::INFINITY, f64::min);
        if remaining.is_finite() {
            let remaining = remaining.max(0.0);
            let quota_multiplier = grants
                .first()
                .map(|(grant, _)| grant.quota_multiplier)
                .unwrap_or(1.0);
            total_base_remaining += quota_base_amount(remaining, quota_multiplier);
            entitlement_remaining.push((grants, remaining));
        }
    }

    let allow_wallet_overage = input.force_wallet_overage || allow_wallet_overage;
    if !allow_wallet_overage && total_base_remaining + 0.000_000_01 < input.total_cost_usd {
        return Ok(DailyQuotaDebitResult {
            covered_base_usd: 0.0,
            insufficient: true,
        });
    }
    if allow_wallet_overage
        && !input.wallet_can_overdraft
        && input.wallet_available_usd.is_some_and(|available| {
            available + SETTLEMENT_EPSILON_USD
                < (input.total_cost_usd - total_base_remaining).max(0.0)
                    * input.wallet_charge_multiplier
        })
    {
        return Ok(DailyQuotaDebitResult {
            covered_base_usd: 0.0,
            insufficient: true,
        });
    }

    let mut remaining_base_cost = input.total_cost_usd;
    let mut covered_base = 0.0;
    for (grants, balance_before) in entitlement_remaining {
        if remaining_base_cost <= 0.000_000_01 || balance_before <= 0.0 {
            continue;
        }
        let quota_multiplier = grants[0].0.quota_multiplier;
        let coverable_base = quota_base_amount(balance_before, quota_multiplier);
        let base_amount = remaining_base_cost.min(coverable_base);
        let amount = quota_debit_amount(base_amount, quota_multiplier).min(balance_before);
        let balance_after = balance_before - amount;
        for (grant, _) in &grants {
            increment_usage_quota_window_postgres(tx, grant, amount).await?;
        }
        let primary_grant = &grants[0].0;
        sqlx::query(
            r#"
INSERT INTO entitlement_usage_ledgers (
  id, user_entitlement_id, user_id, request_id, amount_usd,
  balance_before, balance_after, usage_date, created_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
ON CONFLICT (user_entitlement_id, request_id) DO NOTHING
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&primary_grant.entitlement_id)
        .bind(input.user_id)
        .bind(input.request_id)
        .bind(amount)
        .bind(balance_before)
        .bind(balance_after)
        .bind(&primary_grant.window_key)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?;
        remaining_base_cost -= base_amount;
        covered_base += base_amount;
    }
    Ok(DailyQuotaDebitResult {
        covered_base_usd: covered_base,
        insufficient: false,
    })
}

async fn find_usage_quota_windows_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    entitlement_id: &str,
) -> Result<StoredUsageQuotaWindows, DataLayerError> {
    let rows = sqlx::query(
        r#"
SELECT
  window_scope,
  window_key,
  window_started_at,
  window_ends_at
FROM entitlement_usage_windows
WHERE user_entitlement_id = $1
        "#,
    )
    .bind(entitlement_id)
    .fetch_all(&mut **tx)
    .await
    .map_postgres_err()?;
    let mut windows = StoredUsageQuotaWindows::new();
    for row in rows {
        windows.insert(
            row.try_get("window_scope").map_postgres_err()?,
            StoredUsageQuotaWindow {
                window_key: row.try_get("window_key").map_postgres_err()?,
                window_started_at: row.try_get("window_started_at").map_postgres_err()?,
                window_ends_at: row.try_get("window_ends_at").map_postgres_err()?,
            },
        );
    }
    Ok(windows)
}

async fn upsert_usage_quota_window_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    user_id: &str,
    grant: &crate::repository::billing::quota::UsageQuotaGrant,
) -> Result<f64, DataLayerError> {
    let row = sqlx::query(
        r#"
INSERT INTO entitlement_usage_windows (
  id, user_entitlement_id, user_id, window_scope, window_key,
  window_started_at, window_ends_at, used_usd, created_at, updated_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, 0, NOW(), NOW())
ON CONFLICT (user_entitlement_id, window_scope)
DO UPDATE SET
  window_key = CASE
    WHEN entitlement_usage_windows.window_ends_at <= NOW()
      THEN EXCLUDED.window_key
    ELSE entitlement_usage_windows.window_key
  END,
  window_started_at = CASE
    WHEN entitlement_usage_windows.window_ends_at <= NOW()
      THEN EXCLUDED.window_started_at
    ELSE entitlement_usage_windows.window_started_at
  END,
  window_ends_at = CASE
    WHEN entitlement_usage_windows.window_ends_at <= NOW()
      THEN EXCLUDED.window_ends_at
    ELSE entitlement_usage_windows.window_ends_at
  END,
  used_usd = CASE
    WHEN entitlement_usage_windows.window_ends_at <= NOW()
      THEN 0
    ELSE entitlement_usage_windows.used_usd
  END,
  updated_at = NOW()
RETURNING CAST(used_usd AS DOUBLE PRECISION) AS used_usd
        "#,
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&grant.entitlement_id)
    .bind(user_id)
    .bind(grant.scope)
    .bind(&grant.window_key)
    .bind(grant.window_started_at)
    .bind(grant.window_ends_at)
    .fetch_one(&mut **tx)
    .await
    .map_postgres_err()?;
    row.try_get("used_usd").map_postgres_err()
}

async fn increment_usage_quota_window_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    grant: &crate::repository::billing::quota::UsageQuotaGrant,
    amount: f64,
) -> Result<(), DataLayerError> {
    sqlx::query(
        r#"
UPDATE entitlement_usage_windows
SET used_usd = used_usd + $3,
    updated_at = NOW()
WHERE user_entitlement_id = $1
  AND window_scope = $2
        "#,
    )
    .bind(&grant.entitlement_id)
    .bind(grant.scope)
    .bind(amount)
    .execute(&mut **tx)
    .await
    .map_postgres_err()?;
    Ok(())
}

#[async_trait]
impl SettlementWriteRepository for SqlxSettlementRepository {
    async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
        input.validate()?;
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let row = sqlx::query(FIND_USAGE_FOR_SETTLEMENT_SQL)
                        .bind(&input.request_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?;

                    let Some(usage_row) = row else {
                        return Ok(None);
                    };

                    let current_billing_status: String =
                        usage_row.try_get("billing_status").map_postgres_err()?;
                    if matches!(
                        current_billing_status.as_str(),
                        "settled" | "void" | "insufficient_quota"
                    ) {
                        sync_provider_api_key_usage_contribution_for_request_in_tx(
                            tx,
                            &input.request_id,
                        )
                        .await?;
                        return refresh_finalized_settlement_after_lock_wait(
                            tx,
                            &input.request_id,
                        )
                        .await
                        .map(Some);
                    }

                    let mut final_billing_status =
                        settlement_billing_status_for_usage_status(&input.status).to_string();
                    let finalized_at =
                        i64::try_from(input.finalized_at_unix_secs.unwrap_or_else(|| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                        }))
                        .map_err(|_| {
                            DataLayerError::InvalidInput("finalized_at overflow".to_string())
                        })?;

                    let mut settlement = StoredUsageSettlement {
                        request_id: input.request_id.clone(),
                        wallet_id: None,
                        billing_status: final_billing_status.to_string(),
                        wallet_balance_before: None,
                        wallet_balance_after: None,
                        wallet_recharge_balance_before: None,
                        wallet_recharge_balance_after: None,
                        wallet_gift_balance_before: None,
                        wallet_gift_balance_after: None,
                        provider_monthly_used_usd: None,
                        finalized_at_unix_secs: Some(finalized_at as u64),
                    };

                    if final_billing_status == "settled" {
                        let billing_admission = find_billing_admission_postgres(
                            tx,
                            &input.request_id,
                        )
                        .await?
                        .ok_or_else(|| {
                            DataLayerError::UnexpectedValue(format!(
                                "billing admission missing for request {}",
                                input.request_id
                            ))
                        })?;
                        if !billing_admission.plan_allows_provider(input.provider_id.as_deref()) {
                            return Err(DataLayerError::UnexpectedValue(format!(
                                "billing admission provider mismatch for request {}",
                                input.request_id
                            )));
                        }
                        let api_key_is_standalone = input.api_key_is_standalone;

                        let wallet_row = if let Some(wallet_id) =
                            billing_admission.wallet_id.as_deref()
                        {
                            sqlx::query(
                                r#"
SELECT
  id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode
FROM wallets
WHERE id = $1
FOR UPDATE
LIMIT 1
                                "#,
                            )
                            .bind(wallet_id)
                            .fetch_optional(&mut **tx)
                            .await
                            .map_postgres_err()?
                        } else {
                            None
                        };

                        let wallet_can_overdraft = billing_admission.wallet_can_overdraft();
                        let wallet_available_usd = match wallet_row.as_ref() {
                            Some(row) => {
                                let limit_mode: String =
                                    row.try_get("limit_mode").map_postgres_err()?;
                                if limit_mode.eq_ignore_ascii_case("unlimited") {
                                    None
                                } else {
                                    Some(finite_wallet_available_usd(
                                        row.try_get("balance").map_postgres_err()?,
                                        row.try_get("gift_balance").map_postgres_err()?,
                                    ))
                                }
                            }
                            None => Some(0.0),
                        };
                        if let Some(row) = wallet_row.as_ref() {
                            let wallet_id: String = row.try_get("id").map_postgres_err()?;
                            let before_recharge: f64 = row.try_get("balance").map_postgres_err()?;
                            let before_gift: f64 =
                                row.try_get("gift_balance").map_postgres_err()?;
                            let before_total = before_recharge + before_gift;
                            settlement.wallet_id = Some(wallet_id);
                            settlement.wallet_balance_before = Some(before_total);
                            settlement.wallet_balance_after = Some(before_total);
                            settlement.wallet_recharge_balance_before = Some(before_recharge);
                            settlement.wallet_recharge_balance_after = Some(before_recharge);
                            settlement.wallet_gift_balance_before = Some(before_gift);
                            settlement.wallet_gift_balance_after = Some(before_gift);
                        }

                        let admitted_funding_source = Some(billing_admission.funding_source);
                        let wallet_debit_cost_usd = if matches!(
                            admitted_funding_source,
                            Some(
                                aether_data_contracts::repository::billing::BillingFundingSource::Unlimited
                                    | aether_data_contracts::repository::billing::BillingFundingSource::Free
                            )
                        ) {
                            0.0
                        } else if admitted_funding_source
                            == Some(aether_data_contracts::repository::billing::BillingFundingSource::Wallet)
                        {
                            input.total_cost_usd
                        } else if !api_key_is_standalone {
                            if let Some(user_id) =
                                input.user_id.as_deref().filter(|value| !value.is_empty())
                            {
                                let sales_multiplier = settlement_wallet_charge_multiplier(&input);
                                let admitted_entitlement_ids = billing_admission
                                    .uses_plan_for_provider(input.provider_id.as_deref())
                                    .then(|| {
                                        billing_admission.entitlement_ids_for_provider(
                                            input.provider_id.as_deref(),
                                        )
                                    });
                                let quota = consume_daily_quota_postgres(
                                    tx,
                                    DailyQuotaDebitInput {
                                        user_id,
                                        request_id: &input.request_id,
                                        total_cost_usd: input.base_cost_usd,
                                        wallet_available_usd,
                                        wallet_can_overdraft,
                                        wallet_charge_multiplier: sales_multiplier,
                                        request_global_model_id: input.global_model_id.as_deref(),
                                        admitted_entitlement_ids:
                                            admitted_entitlement_ids.as_deref(),
                                        force_wallet_overage: admitted_entitlement_ids.is_some(),
                                    },
                                )
                                .await?;
                                if quota.insufficient {
                                    final_billing_status = "insufficient_quota".to_string();
                                    settlement.billing_status = final_billing_status.clone();
                                    0.0
                                } else {
                                    (input.base_cost_usd - quota.covered_base_usd).max(0.0)
                                        * sales_multiplier
                                }
                            } else {
                                input.total_cost_usd
                            }
                        } else {
                            input.total_cost_usd
                        };
                        if final_billing_status != "settled" {
                            finalize_usage_billing_in_tx(
                                tx,
                                &settlement,
                                &final_billing_status,
                                finalized_at,
                            )
                            .await?;
                            return Ok(Some(settlement));
                        }

                        if wallet_debit_cost_usd > SETTLEMENT_EPSILON_USD {
                            if let Some(wallet_row) = wallet_row {
                                let wallet_id: String =
                                    wallet_row.try_get("id").map_postgres_err()?;
                                let before_recharge: f64 =
                                    wallet_row.try_get("balance").map_postgres_err()?;
                                let before_gift: f64 =
                                    wallet_row.try_get("gift_balance").map_postgres_err()?;
                                let limit_mode: String =
                                    wallet_row.try_get("limit_mode").map_postgres_err()?;
                                let before_total = before_recharge + before_gift;
                                let mut after_recharge = before_recharge;
                                let mut after_gift = before_gift;
                                if !limit_mode.eq_ignore_ascii_case("unlimited") {
                                    let debit_plan = plan_finite_wallet_debit(
                                        before_recharge,
                                        before_gift,
                                        wallet_debit_cost_usd,
                                    );
                                    (after_recharge, after_gift) =
                                        debit_plan.after_balances(before_recharge, before_gift);
                                }
                                if final_billing_status == "settled" {
                                    sqlx::query(
                                        r#"
UPDATE wallets
SET
  balance = $2,
  gift_balance = $3,
  total_consumed = CAST(total_consumed AS DOUBLE PRECISION) + $4,
  updated_at = NOW()
WHERE id = $1
                                "#,
                                    )
                                    .bind(&wallet_id)
                                    .bind(after_recharge)
                                    .bind(after_gift)
                                    .bind(wallet_debit_cost_usd)
                                    .execute(&mut **tx)
                                    .await
                                    .map_postgres_err()?;
                                }

                                settlement.wallet_id = Some(wallet_id.clone());
                                settlement.wallet_balance_before = Some(before_total);
                                settlement.wallet_balance_after = Some(after_recharge + after_gift);
                                settlement.wallet_recharge_balance_before = Some(before_recharge);
                                settlement.wallet_recharge_balance_after = Some(after_recharge);
                                settlement.wallet_gift_balance_before = Some(before_gift);
                                settlement.wallet_gift_balance_after = Some(after_gift);
                            } else {
                                final_billing_status = "insufficient_quota".to_string();
                                settlement.billing_status = final_billing_status.clone();
                            }
                        }

                        if final_billing_status != "settled" {
                            finalize_usage_billing_in_tx(
                                tx,
                                &settlement,
                                &final_billing_status,
                                finalized_at,
                            )
                            .await?;
                            return Ok(Some(settlement));
                        }

                        if let Some(provider_id) = input
                            .provider_id
                            .as_deref()
                            .filter(|value| !value.is_empty())
                        {
                            enqueue_provider_monthly_usage_delta(
                                &mut **tx,
                                &input.request_id,
                                provider_id,
                                input.actual_total_cost_usd,
                            )
                            .await?;
                        }
                    }

                    finalize_usage_billing_in_tx(
                        tx,
                        &settlement,
                        &final_billing_status,
                        finalized_at,
                    )
                    .await?;

                    Ok(Some(settlement))
                })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{SettlementWriteRepository, SqlxSettlementRepository, UsageSettlementInput};
    use aether_data_contracts::repository::billing::BillingReadRepository;

    #[test]
    fn finalize_usage_billing_sql_does_not_require_usage_updated_at_column() {
        assert!(!super::FINALIZE_USAGE_BILLING_SQL.contains("updated_at"));
    }

    #[test]
    fn settlement_sql_reads_settlement_snapshots_before_legacy_usage_columns() {
        assert!(
            super::FIND_USAGE_FOR_SETTLEMENT_SQL.contains("LEFT JOIN usage_settlement_snapshots")
        );
        assert!(super::FIND_USAGE_FOR_SETTLEMENT_SQL.contains(
            "COALESCE(usage_settlement_snapshots.billing_status, usage_record.billing_status)"
        ));
        assert!(super::FIND_USAGE_FOR_SETTLEMENT_SQL.contains("FOR UPDATE OF usage_record"));
    }

    #[test]
    fn settlement_sql_dual_writes_usage_settlement_snapshots() {
        assert!(super::UPSERT_USAGE_SETTLEMENT_SNAPSHOT_SQL
            .contains("INSERT INTO usage_settlement_snapshots"));
        assert!(super::UPSERT_USAGE_SETTLEMENT_SNAPSHOT_SQL.contains("provider_monthly_used_usd"));
        assert!(super::UPSERT_USAGE_SETTLEMENT_SNAPSHOT_SQL
            .contains("TO_TIMESTAMP($11::double precision)"));
    }

    #[test]
    fn settlement_sql_no_longer_dual_writes_wallet_snapshots_to_usage_rows() {
        let source = include_str!("postgres.rs");
        assert!(!source.contains("UPDATE \"usage\"\nSET\n  wallet_id = $2"));
    }

    #[test]
    fn settlement_sql_enqueues_provider_monthly_usage_delta() {
        let source = include_str!("postgres.rs");
        assert!(super::ENQUEUE_PROVIDER_MONTHLY_USAGE_DELTA_SQL.contains("usage_counter_deltas"));
        assert!(super::ENQUEUE_PROVIDER_MONTHLY_USAGE_DELTA_SQL.contains("'provider_monthly'"));
        assert!(!source.contains("UPDATE providers\nSET\n  monthly_used_usd"));
    }

    #[test]
    fn settlement_sql_syncs_provider_usage_after_finalizing_billing() {
        let source = include_str!("postgres.rs");
        assert!(source.contains("finalize_usage_billing_in_tx("));
        assert!(source.contains("sync_provider_api_key_usage_contribution_for_request_in_tx"));
        assert!(source.contains("sync_usage_settlement_snapshot"));
        assert!(source.contains("FINALIZE_USAGE_BILLING_SQL"));
    }

    #[test]
    fn settlement_sql_requires_the_saved_billing_admission() {
        let source = include_str!("postgres.rs");
        assert!(source.contains("billing admission missing for request"));
        assert!(source.contains("let api_key_is_standalone = input.api_key_is_standalone;"));
        assert!(source.contains("billing_admission.wallet_overage_allowed"));
    }

    #[tokio::test]
    async fn postgres_wallet_settlement_requires_admission_and_is_idempotent_when_url_is_set() {
        let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            eprintln!("skipping postgres billing settlement test because AETHER_TEST_POSTGRES_URL is unset");
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
        let user_id = format!("u-{}", &suffix[..32]);
        let wallet_id = format!("w-{}", &suffix[..32]);
        let missing_request_id = format!("pg-missing-{suffix}");
        let admitted_request_id = format!("pg-admitted-{suffix}");
        let plan_request_id = format!("pg-plan-{suffix}");
        let provider_id = format!("p-{}", &suffix[..32]);
        let plan_id = format!("plan-{}", &suffix[..30]);
        let order_id = format!("order-{}", &suffix[..30]);
        let entitlement_id = format!("ent-{suffix}");
        sqlx::query("INSERT INTO users (id, username, email_verified) VALUES ($1, $2, FALSE)")
            .bind(&user_id)
            .bind(format!("billing-{suffix}"))
            .execute(&pool)
            .await
            .expect("user should seed");
        sqlx::query(
            "INSERT INTO wallets (id, user_id, balance, gift_balance, created_at, updated_at) VALUES ($1, $2, 12, 0, NOW(), NOW())",
        )
        .bind(&wallet_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("wallet should seed");
        for request_id in [&missing_request_id, &admitted_request_id, &plan_request_id] {
            sqlx::query(
                "INSERT INTO usage (id, user_id, request_id, provider_name, model, status, billing_status, total_cost_usd, actual_total_cost_usd) VALUES ($1, $2, $3, 'test', 'gpt-test', 'completed', 'pending', 15, 7.5)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&user_id)
            .bind(request_id)
            .execute(&pool)
            .await
            .expect("usage should seed");
        }

        let repository = SqlxSettlementRepository::new(pool.clone());
        let missing_error = repository
            .settle_usage(sample_wallet_settlement_input(
                &missing_request_id,
                &user_id,
            ))
            .await
            .expect_err("missing admission must stop settlement");
        assert!(missing_error
            .to_string()
            .contains("billing admission missing"));
        let unchanged_balance: f64 = sqlx::query_scalar(
            "SELECT CAST(balance + gift_balance AS DOUBLE PRECISION) FROM wallets WHERE id = $1",
        )
        .bind(&wallet_id)
        .fetch_one(&pool)
        .await
        .expect("wallet should read");
        assert_eq!(unchanged_balance, 12.0);
        let pending_usage: (String, f64) = sqlx::query_as(
            "SELECT billing_status, CAST(total_cost_usd AS DOUBLE PRECISION) FROM usage WHERE request_id = $1",
        )
        .bind(&missing_request_id)
        .fetch_one(&pool)
        .await
        .expect("usage should read");
        assert_eq!(pending_usage, ("pending".to_string(), 15.0));

        sqlx::query(
            r#"
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, funding_source, wallet_balance_at_admission,
  wallet_payment_allowed, wallet_overage_allowed, entitlement_ids,
  entitlement_provider_scopes, allowed_provider_ids
)
VALUES ($1, $2, $3, 'wallet', 12, TRUE, FALSE, '[]'::jsonb, '{}'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(&admitted_request_id)
        .bind(&user_id)
        .bind(&wallet_id)
        .execute(&pool)
        .await
        .expect("billing admission should seed");
        let first_repository = repository.clone();
        let second_repository = repository.clone();
        let first_input = sample_wallet_settlement_input(&admitted_request_id, &user_id);
        let second_input = sample_wallet_settlement_input(&admitted_request_id, &user_id);
        let (first, second) = tokio::join!(
            first_repository.settle_usage(first_input),
            second_repository.settle_usage(second_input),
        );
        let first = first
            .expect("settlement should succeed")
            .expect("usage should exist");
        assert_eq!(first.wallet_balance_after, Some(-3.0));
        let second = second
            .expect("duplicate settlement should succeed")
            .expect("usage should exist");
        assert_eq!(second.wallet_balance_after, Some(-3.0));
        let wallet: (f64, f64) = sqlx::query_as(
            "SELECT CAST(balance + gift_balance AS DOUBLE PRECISION), CAST(total_consumed AS DOUBLE PRECISION) FROM wallets WHERE id = $1",
        )
        .bind(&wallet_id)
        .fetch_one(&pool)
        .await
        .expect("wallet should read");
        assert_eq!(wallet, (-3.0, 15.0));

        sqlx::query(
            "UPDATE wallets SET balance = 10, gift_balance = 0, total_consumed = 0 WHERE id = $1",
        )
        .bind(&wallet_id)
        .execute(&pool)
        .await
        .expect("wallet should reset for plan split test");
        sqlx::query("INSERT INTO providers (id, name) VALUES ($1, $2)")
            .bind(&provider_id)
            .bind(format!("provider-{suffix}"))
            .execute(&pool)
            .await
            .expect("provider should seed");
        sqlx::query(
            "INSERT INTO billing_plans (id, title, price_amount, duration_unit, duration_value, entitlements_json, created_at, updated_at) VALUES ($1, 'Test plan', 1, 'day', 1, $2, NOW(), NOW())",
        )
        .bind(&plan_id)
        .bind(serde_json::json!([{
            "type": "daily_quota",
            "daily_quota_usd": 2.0,
            "quota_multiplier": 1.0,
            "allow_wallet_overage": true
        }]))
        .execute(&pool)
        .await
        .expect("plan should seed");
        sqlx::query(
            "INSERT INTO payment_orders (id, order_no, wallet_id, user_id, amount_usd, payment_method, status, created_at) VALUES ($1, $2, $3, $4, 1, 'admin_grant', 'credited', NOW())",
        )
        .bind(&order_id)
        .bind(format!("NO-{suffix}"))
        .bind(&wallet_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("payment order should seed");
        let entitlement_snapshot = serde_json::json!([{
            "type": "daily_quota",
            "daily_quota_usd": 2.0,
            "quota_multiplier": 1.0,
            "allow_wallet_overage": true
        }]);
        sqlx::query(
            "INSERT INTO user_plan_entitlements (id, user_id, plan_id, payment_order_id, starts_at, expires_at, entitlements_snapshot, created_at, updated_at) VALUES ($1, $2, $3, $4, NOW() - INTERVAL '1 minute', NOW() + INTERVAL '1 day', $5, NOW(), NOW())",
        )
        .bind(&entitlement_id)
        .bind(&user_id)
        .bind(&plan_id)
        .bind(&order_id)
        .bind(entitlement_snapshot)
        .execute(&pool)
        .await
        .expect("entitlement should seed");
        sqlx::query(
            "INSERT INTO user_entitlement_providers (user_entitlement_id, provider_id) VALUES ($1, $2)",
        )
        .bind(&entitlement_id)
        .bind(&provider_id)
        .execute(&pool)
        .await
        .expect("entitlement provider should seed");
        sqlx::query("INSERT INTO billing_plan_providers (plan_id, provider_id) VALUES ($1, $2)")
            .bind(&plan_id)
            .bind(&provider_id)
            .execute(&pool)
            .await
            .expect("plan provider should seed");
        let billing_repository =
            crate::repository::billing::SqlxBillingReadRepository::new(pool.clone());
        let availability_before = billing_repository
            .find_user_daily_quota_availability(&user_id)
            .await
            .expect("plan availability should query")
            .expect("active plan should exist");
        assert_eq!(availability_before.total_quota_usd, 2.0);
        assert_eq!(availability_before.remaining_usd, 2.0);
        assert_eq!(
            availability_before.allowed_provider_ids,
            vec![provider_id.clone()]
        );
        sqlx::query(
            r#"
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, funding_source, wallet_balance_at_admission,
  wallet_payment_allowed, wallet_overage_allowed, entitlement_ids,
  entitlement_provider_scopes, allowed_provider_ids
)
VALUES (
  $1, $2, $3, 'plan', 10, TRUE, TRUE,
  jsonb_build_array($4::text),
  jsonb_build_object($4::text, jsonb_build_array($5::text)),
  jsonb_build_array($5::text)
)
            "#,
        )
        .bind(&plan_request_id)
        .bind(&user_id)
        .bind(&wallet_id)
        .bind(&entitlement_id)
        .bind(&provider_id)
        .execute(&pool)
        .await
        .expect("plan billing admission should seed");
        let mut plan_input = sample_wallet_settlement_input(&plan_request_id, &user_id);
        plan_input.provider_id = Some(provider_id.clone());
        plan_input.base_cost_usd = 8.0;
        plan_input.total_cost_usd = 8.0;
        plan_input.actual_total_cost_usd = 4.0;
        let plan_settlement = repository
            .settle_usage(plan_input)
            .await
            .expect("plan settlement should succeed")
            .expect("plan usage should exist");
        assert_eq!(plan_settlement.wallet_balance_after, Some(4.0));
        let plan_wallet: (f64, f64) = sqlx::query_as(
            "SELECT CAST(balance + gift_balance AS DOUBLE PRECISION), CAST(total_consumed AS DOUBLE PRECISION) FROM wallets WHERE id = $1",
        )
        .bind(&wallet_id)
        .fetch_one(&pool)
        .await
        .expect("plan wallet should read");
        assert_eq!(plan_wallet, (4.0, 6.0));
        let plan_quota_used: f64 = sqlx::query_scalar(
            "SELECT CAST(SUM(amount_usd) AS DOUBLE PRECISION) FROM entitlement_usage_ledgers WHERE request_id = $1",
        )
        .bind(&plan_request_id)
        .fetch_one(&pool)
        .await
        .expect("plan quota ledger should read");
        assert_eq!(plan_quota_used, 2.0);
        let availability_after = billing_repository
            .find_user_daily_quota_availability(&user_id)
            .await
            .expect("plan availability should query after settlement")
            .expect("active plan should exist");
        assert_eq!(availability_after.used_usd, 2.0);
        assert_eq!(availability_after.remaining_usd, 0.0);

        sqlx::query("DELETE FROM billing_request_admissions WHERE request_id = ANY($1)")
            .bind(vec![admitted_request_id.clone(), plan_request_id.clone()])
            .execute(&pool)
            .await
            .expect("admissions should clean up");
        sqlx::query("DELETE FROM usage_counter_deltas WHERE request_id = $1")
            .bind(&plan_request_id)
            .execute(&pool)
            .await
            .expect("provider delta should clean up");
        sqlx::query("DELETE FROM usage WHERE request_id = ANY($1)")
            .bind(vec![
                missing_request_id,
                admitted_request_id,
                plan_request_id,
            ])
            .execute(&pool)
            .await
            .expect("usage should clean up");
        sqlx::query("DELETE FROM user_plan_entitlements WHERE id = $1")
            .bind(&entitlement_id)
            .execute(&pool)
            .await
            .expect("entitlement should clean up");
        sqlx::query("DELETE FROM payment_orders WHERE id = $1")
            .bind(&order_id)
            .execute(&pool)
            .await
            .expect("payment order should clean up");
        sqlx::query("DELETE FROM billing_plans WHERE id = $1")
            .bind(&plan_id)
            .execute(&pool)
            .await
            .expect("plan should clean up");
        sqlx::query("DELETE FROM providers WHERE id = $1")
            .bind(&provider_id)
            .execute(&pool)
            .await
            .expect("provider should clean up");
        sqlx::query("DELETE FROM wallets WHERE id = $1")
            .bind(&wallet_id)
            .execute(&pool)
            .await
            .expect("wallet should clean up");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(&user_id)
            .execute(&pool)
            .await
            .expect("user should clean up");
    }

    #[tokio::test]
    async fn postgres_plan_multipliers_and_provider_scope_are_enforced_when_url_is_set() {
        let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            eprintln!(
                "skipping postgres plan multiplier and provider scope test because AETHER_TEST_POSTGRES_URL is unset"
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
        let key = &suffix[..20];
        let user_id = format!("u-mult-{key}");
        let wallet_id = format!("w-mult-{key}");
        let provider_id = format!("p-mult-{key}");
        let plan_id = format!("plan-mult-{key}");
        let single_entitlement_id = format!("ent-single-{key}");
        let fast_entitlement_id = format!("ent-fast-{key}");
        let slow_entitlement_id = format!("ent-slow-{key}");
        let single_request_id = format!("request-single-{suffix}");
        let combined_request_id = format!("request-combined-{suffix}");
        let mismatch_request_id = format!("request-mismatch-{suffix}");

        sqlx::query("INSERT INTO users (id, username, email_verified) VALUES ($1, $2, FALSE)")
            .bind(&user_id)
            .bind(format!("multiplier-{suffix}"))
            .execute(&pool)
            .await
            .expect("user should seed");
        sqlx::query(
            "INSERT INTO wallets (id, user_id, balance, gift_balance, created_at, updated_at) VALUES ($1, $2, 10, 0, NOW(), NOW())",
        )
        .bind(&wallet_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("wallet should seed");
        sqlx::query("INSERT INTO providers (id, name) VALUES ($1, $2)")
            .bind(&provider_id)
            .bind(format!("multiplier-provider-{suffix}"))
            .execute(&pool)
            .await
            .expect("provider should seed");
        sqlx::query(
            "INSERT INTO billing_plans (id, title, price_amount, duration_unit, duration_value, entitlements_json, created_at, updated_at) VALUES ($1, 'Multiplier plan', 1, 'day', 1, '[]'::jsonb, NOW(), NOW())",
        )
        .bind(&plan_id)
        .execute(&pool)
        .await
        .expect("plan should seed");

        for (entitlement_id, quota, multiplier) in [
            (&single_entitlement_id, 10.0, 0.5),
            (&fast_entitlement_id, 2.0, 0.5),
            (&slow_entitlement_id, 2.0, 2.0),
        ] {
            let order_id = format!("order-{entitlement_id}");
            sqlx::query(
                "INSERT INTO payment_orders (id, order_no, wallet_id, user_id, amount_usd, payment_method, status, created_at) VALUES ($1, $2, $3, $4, 0, 'admin_grant', 'credited', NOW())",
            )
            .bind(&order_id)
            .bind(format!("NO-{order_id}"))
            .bind(&wallet_id)
            .bind(&user_id)
            .execute(&pool)
            .await
            .expect("payment order should seed");
            sqlx::query(
                "INSERT INTO user_plan_entitlements (id, user_id, plan_id, payment_order_id, starts_at, expires_at, entitlements_snapshot, created_at, updated_at) VALUES ($1, $2, $3, $4, NOW() - INTERVAL '1 minute', NOW() + INTERVAL '1 day', $5, NOW(), NOW())",
            )
            .bind(entitlement_id)
            .bind(&user_id)
            .bind(&plan_id)
            .bind(&order_id)
            .bind(serde_json::json!([{
                "type": "daily_quota",
                "daily_quota_usd": quota,
                "quota_multiplier": multiplier,
                "reset_timezone": "Asia/Shanghai",
                "allow_wallet_overage": true
            }]))
            .execute(&pool)
            .await
            .expect("entitlement should seed");
            sqlx::query(
                "INSERT INTO user_entitlement_providers (user_entitlement_id, provider_id) VALUES ($1, $2)",
            )
            .bind(entitlement_id)
            .bind(&provider_id)
            .execute(&pool)
            .await
            .expect("entitlement provider should seed");
        }

        for request_id in [
            &single_request_id,
            &combined_request_id,
            &mismatch_request_id,
        ] {
            sqlx::query(
                "INSERT INTO usage (id, user_id, request_id, provider_name, model, status, billing_status, total_cost_usd, actual_total_cost_usd) VALUES ($1, $2, $3, 'test', 'gpt-test', 'completed', 'pending', 3, 2)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&user_id)
            .bind(request_id)
            .execute(&pool)
            .await
            .expect("usage should seed");
        }

        sqlx::query(
            r#"
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, funding_source, wallet_balance_at_admission,
  wallet_payment_allowed, wallet_overage_allowed, entitlement_ids,
  entitlement_provider_scopes, allowed_provider_ids
)
VALUES (
  $1, $2, $3, 'plan', 10, FALSE, FALSE,
  jsonb_build_array($4::text),
  jsonb_build_object($4::text, jsonb_build_array($5::text)),
  jsonb_build_array($5::text)
)
            "#,
        )
        .bind(&single_request_id)
        .bind(&user_id)
        .bind(&wallet_id)
        .bind(&single_entitlement_id)
        .bind(&provider_id)
        .execute(&pool)
        .await
        .expect("single multiplier admission should seed");

        let repository = SqlxSettlementRepository::new(pool.clone());
        let mut single_input = sample_wallet_settlement_input(&single_request_id, &user_id);
        single_input.provider_id = Some(provider_id.clone());
        single_input.base_cost_usd = 3.0;
        single_input.total_cost_usd = 3.0;
        single_input.actual_total_cost_usd = 2.0;
        let single = repository
            .settle_usage(single_input)
            .await
            .expect("single multiplier settlement should succeed")
            .expect("single multiplier usage should exist");
        assert_eq!(single.billing_status, "settled");
        let single_quota_used: f64 = sqlx::query_scalar(
            "SELECT CAST(SUM(amount_usd) AS DOUBLE PRECISION) FROM entitlement_usage_ledgers WHERE request_id = $1",
        )
        .bind(&single_request_id)
        .fetch_one(&pool)
        .await
        .expect("single multiplier ledger should read");
        assert_eq!(single_quota_used, 1.5);

        sqlx::query(
            r#"
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, funding_source, wallet_balance_at_admission,
  wallet_payment_allowed, wallet_overage_allowed, entitlement_ids,
  entitlement_provider_scopes, allowed_provider_ids
)
VALUES (
  $1, $2, $3, 'plan', 10, TRUE, TRUE,
  jsonb_build_array($4::text, $5::text),
  jsonb_build_object(
    $4::text, jsonb_build_array($6::text),
    $5::text, jsonb_build_array($6::text)
  ),
  jsonb_build_array($6::text)
)
            "#,
        )
        .bind(&combined_request_id)
        .bind(&user_id)
        .bind(&wallet_id)
        .bind(&fast_entitlement_id)
        .bind(&slow_entitlement_id)
        .bind(&provider_id)
        .execute(&pool)
        .await
        .expect("combined multiplier admission should seed");
        let mut combined_input = sample_wallet_settlement_input(&combined_request_id, &user_id);
        combined_input.provider_id = Some(provider_id.clone());
        combined_input.base_cost_usd = 6.0;
        combined_input.total_cost_usd = 12.0;
        combined_input.actual_total_cost_usd = 3.0;
        let combined = repository
            .settle_usage(combined_input)
            .await
            .expect("combined multiplier settlement should succeed")
            .expect("combined multiplier usage should exist");
        assert_eq!(combined.billing_status, "settled");
        for entitlement_id in [&fast_entitlement_id, &slow_entitlement_id] {
            let quota_used: f64 = sqlx::query_scalar(
                "SELECT CAST(amount_usd AS DOUBLE PRECISION) FROM entitlement_usage_ledgers WHERE request_id = $1 AND user_entitlement_id = $2",
            )
            .bind(&combined_request_id)
            .bind(entitlement_id)
            .fetch_one(&pool)
            .await
            .expect("combined multiplier ledger should read");
            assert_eq!(quota_used, 2.0);
        }
        let wallet_after_combined: f64 = sqlx::query_scalar(
            "SELECT CAST(balance + gift_balance AS DOUBLE PRECISION) FROM wallets WHERE id = $1",
        )
        .bind(&wallet_id)
        .fetch_one(&pool)
        .await
        .expect("wallet should read after combined multiplier settlement");
        assert_eq!(wallet_after_combined, 8.0);

        sqlx::query("UPDATE wallets SET balance = -1, gift_balance = 0 WHERE id = $1")
            .bind(&wallet_id)
            .execute(&pool)
            .await
            .expect("wallet should enter debt for provider scope test");
        sqlx::query(
            r#"
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, funding_source, wallet_balance_at_admission,
  wallet_payment_allowed, wallet_overage_allowed, entitlement_ids,
  entitlement_provider_scopes, allowed_provider_ids
)
VALUES (
  $1, $2, $3, 'plan', -1, FALSE, TRUE,
  jsonb_build_array($4::text),
  jsonb_build_object($4::text, jsonb_build_array($5::text)),
  jsonb_build_array($5::text)
)
            "#,
        )
        .bind(&mismatch_request_id)
        .bind(&user_id)
        .bind(&wallet_id)
        .bind(&fast_entitlement_id)
        .bind(&provider_id)
        .execute(&pool)
        .await
        .expect("provider scope admission should seed");
        let mut mismatch_input = sample_wallet_settlement_input(&mismatch_request_id, &user_id);
        mismatch_input.provider_id = Some(format!("outside-{provider_id}"));
        mismatch_input.base_cost_usd = 3.0;
        mismatch_input.total_cost_usd = 3.0;
        mismatch_input.actual_total_cost_usd = 2.0;
        assert!(repository.settle_usage(mismatch_input).await.is_err());
        let wallet_after_mismatch: f64 = sqlx::query_scalar(
            "SELECT CAST(balance + gift_balance AS DOUBLE PRECISION) FROM wallets WHERE id = $1",
        )
        .bind(&wallet_id)
        .fetch_one(&pool)
        .await
        .expect("wallet should read after provider mismatch");
        assert_eq!(wallet_after_mismatch, -1.0);
        let mismatch_status: String =
            sqlx::query_scalar("SELECT billing_status FROM usage WHERE request_id = $1")
                .bind(&mismatch_request_id)
                .fetch_one(&pool)
                .await
                .expect("provider mismatch usage should read");
        assert_eq!(mismatch_status, "pending");
    }

    fn sample_wallet_settlement_input(request_id: &str, user_id: &str) -> UsageSettlementInput {
        UsageSettlementInput {
            request_id: request_id.to_string(),
            user_id: Some(user_id.to_string()),
            api_key_id: None,
            api_key_is_standalone: false,
            provider_id: None,
            global_model_id: None,
            global_model_name: None,
            model: Some("gpt-test".to_string()),
            status: "completed".to_string(),
            billing_status: "pending".to_string(),
            base_cost_usd: 15.0,
            total_cost_usd: 15.0,
            actual_total_cost_usd: 7.5,
            finalized_at_unix_secs: Some(1_700_000_000),
        }
    }
}

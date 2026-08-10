use async_trait::async_trait;
use sqlx::{sqlite::SqliteRow, Row};

use super::{
    finite_wallet_available_usd, plan_finite_wallet_debit,
    settlement_billing_status_for_usage_status, settlement_wallet_charge_multiplier,
    SettlementBillingAdmission, SettlementWriteRepository, StoredUsageSettlement,
    UsageSettlementInput, SETTLEMENT_EPSILON_USD,
};
use crate::driver::sqlite::{sqlite_optional_real, sqlite_real, SqlitePool};
use crate::error::SqlResultExt;
use crate::repository::billing::quota::{
    entitlement_allows_global_model, entitlements_snapshot_has_usage_quota_for_global_model,
    quota_base_amount, quota_debit_amount, usage_quota_grants_from_entitlement,
    StoredUsageQuotaWindow, UsageQuotaGrant, QUOTA_SCOPE_FIVE_HOUR,
};
use crate::DataLayerError;

const FIND_USAGE_FOR_SETTLEMENT_SQL: &str = r#"
SELECT
  usage_record.request_id,
  COALESCE(usage_settlement_snapshots.wallet_id, usage_record.wallet_id) AS wallet_id,
  COALESCE(usage_settlement_snapshots.billing_status, usage_record.billing_status) AS billing_status,
  COALESCE(
    usage_settlement_snapshots.wallet_balance_before,
    usage_record.wallet_balance_before
  ) AS wallet_balance_before,
  COALESCE(
    usage_settlement_snapshots.wallet_balance_after,
    usage_record.wallet_balance_after
  ) AS wallet_balance_after,
  COALESCE(
    usage_settlement_snapshots.wallet_recharge_balance_before,
    usage_record.wallet_recharge_balance_before
  ) AS wallet_recharge_balance_before,
  COALESCE(
    usage_settlement_snapshots.wallet_recharge_balance_after,
    usage_record.wallet_recharge_balance_after
  ) AS wallet_recharge_balance_after,
  COALESCE(
    usage_settlement_snapshots.wallet_gift_balance_before,
    usage_record.wallet_gift_balance_before
  ) AS wallet_gift_balance_before,
  COALESCE(
    usage_settlement_snapshots.wallet_gift_balance_after,
    usage_record.wallet_gift_balance_after
  ) AS wallet_gift_balance_after,
  CAST(usage_settlement_snapshots.provider_monthly_used_usd AS REAL) AS provider_monthly_used_usd,
  usage_record.provider_id,
  COALESCE(usage_settlement_snapshots.finalized_at, usage_record.finalized_at) AS finalized_at_unix_secs
FROM "usage" AS usage_record
LEFT JOIN usage_settlement_snapshots
  ON usage_settlement_snapshots.request_id = usage_record.request_id
WHERE usage_record.request_id = ?
"#;

const FINALIZE_USAGE_BILLING_SQL: &str = r#"
UPDATE "usage"
SET
  billing_status = ?,
  finalized_at = COALESCE(finalized_at, ?)
WHERE request_id = ?
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
  finalized_at,
  created_at,
  updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT (request_id)
DO UPDATE SET
  billing_status = excluded.billing_status,
  wallet_id = COALESCE(excluded.wallet_id, usage_settlement_snapshots.wallet_id),
  wallet_balance_before = COALESCE(
    excluded.wallet_balance_before,
    usage_settlement_snapshots.wallet_balance_before
  ),
  wallet_balance_after = COALESCE(
    excluded.wallet_balance_after,
    usage_settlement_snapshots.wallet_balance_after
  ),
  wallet_recharge_balance_before = COALESCE(
    excluded.wallet_recharge_balance_before,
    usage_settlement_snapshots.wallet_recharge_balance_before
  ),
  wallet_recharge_balance_after = COALESCE(
    excluded.wallet_recharge_balance_after,
    usage_settlement_snapshots.wallet_recharge_balance_after
  ),
  wallet_gift_balance_before = COALESCE(
    excluded.wallet_gift_balance_before,
    usage_settlement_snapshots.wallet_gift_balance_before
  ),
  wallet_gift_balance_after = COALESCE(
    excluded.wallet_gift_balance_after,
    usage_settlement_snapshots.wallet_gift_balance_after
  ),
  provider_monthly_used_usd = COALESCE(
    excluded.provider_monthly_used_usd,
    usage_settlement_snapshots.provider_monthly_used_usd
  ),
  finalized_at = COALESCE(excluded.finalized_at, usage_settlement_snapshots.finalized_at),
  updated_at = excluded.updated_at
"#;

#[derive(Debug, Clone)]
pub struct SqliteSettlementRepository {
    pool: SqlitePool,
}

impl SqliteSettlementRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn settlement_from_row(row: &SqliteRow) -> Result<StoredUsageSettlement, DataLayerError> {
    Ok(StoredUsageSettlement {
        request_id: row.try_get("request_id").map_sql_err()?,
        wallet_id: row.try_get("wallet_id").map_sql_err()?,
        billing_status: row.try_get("billing_status").map_sql_err()?,
        wallet_balance_before: sqlite_optional_real(row, "wallet_balance_before")?,
        wallet_balance_after: sqlite_optional_real(row, "wallet_balance_after")?,
        wallet_recharge_balance_before: sqlite_optional_real(
            row,
            "wallet_recharge_balance_before",
        )?,
        wallet_recharge_balance_after: sqlite_optional_real(row, "wallet_recharge_balance_after")?,
        wallet_gift_balance_before: sqlite_optional_real(row, "wallet_gift_balance_before")?,
        wallet_gift_balance_after: sqlite_optional_real(row, "wallet_gift_balance_after")?,
        provider_monthly_used_usd: sqlite_optional_real(row, "provider_monthly_used_usd")?,
        finalized_at_unix_secs: row
            .try_get::<Option<i64>, _>("finalized_at_unix_secs")
            .map_sql_err()?
            .map(|value| value as u64),
    })
}

async fn find_billing_admission_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request_id: &str,
) -> Result<Option<SettlementBillingAdmission>, DataLayerError> {
    let row = sqlx::query(
        r#"
SELECT funding_source, wallet_id, wallet_payment_allowed, wallet_overage_allowed,
       entitlement_ids, entitlement_provider_scopes, allowed_provider_ids
FROM billing_request_admissions
WHERE request_id = ? AND billing_admitted = 1 AND status = 'admitted'
        "#,
    )
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_sql_err()?;
    row.map(|row| {
        let funding_source: String = row.try_get("funding_source").map_sql_err()?;
        Ok(SettlementBillingAdmission {
            funding_source:
                aether_data_contracts::repository::billing::BillingFundingSource::from_database(
                    &funding_source,
                )?,
            wallet_id: row.try_get("wallet_id").map_sql_err()?,
            wallet_payment_allowed: row
                .try_get::<i64, _>("wallet_payment_allowed")
                .map_sql_err()?
                != 0,
            wallet_overage_allowed: row
                .try_get::<i64, _>("wallet_overage_allowed")
                .map_sql_err()?
                != 0,
            entitlement_ids: parse_json_string_vec(
                row.try_get("entitlement_ids").map_sql_err()?,
                "billing_request_admissions.entitlement_ids",
            )?,
            entitlement_provider_scopes: parse_json_provider_scopes(
                row.try_get("entitlement_provider_scopes").map_sql_err()?,
                "billing_request_admissions.entitlement_provider_scopes",
            )?,
            allowed_provider_ids: parse_json_string_vec(
                row.try_get("allowed_provider_ids").map_sql_err()?,
                "billing_request_admissions.allowed_provider_ids",
            )?,
        })
    })
    .transpose()
}

fn parse_json_string_vec(raw: String, field: &str) -> Result<Vec<String>, DataLayerError> {
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        DataLayerError::UnexpectedValue(format!("{field} invalid json: {error}"))
    })?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn parse_json_provider_scopes(
    raw: String,
    field: &str,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, DataLayerError> {
    serde_json::from_str(&raw)
        .map_err(|error| DataLayerError::UnexpectedValue(format!("{field} invalid json: {error}")))
}

fn now_unix_secs() -> Result<i64, DataLayerError> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .map_err(|_| DataLayerError::InvalidInput("timestamp overflow".to_string()))
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
    now_unix_secs: i64,
    request_global_model_id: Option<&'a str>,
    admitted_entitlement_ids: Option<&'a [String]>,
    force_wallet_overage: bool,
}

async fn consume_daily_quota_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    input: DailyQuotaDebitInput<'_>,
) -> Result<DailyQuotaDebitResult, DataLayerError> {
    if input.total_cost_usd <= 0.0 {
        return Ok(DailyQuotaDebitResult::default());
    }
    let rows = sqlx::query(
        r#"
SELECT id, starts_at, entitlements_snapshot
FROM user_plan_entitlements
WHERE user_id = ?
  AND status = 'active'
  AND starts_at <= ?
  AND expires_at > ?
ORDER BY expires_at ASC, created_at ASC, id ASC
"#,
    )
    .bind(input.user_id)
    .bind(input.now_unix_secs)
    .bind(input.now_unix_secs)
    .fetch_all(&mut **tx)
    .await
    .map_sql_err()?;
    let now = chrono::Utc::now();
    let mut grants = Vec::new();
    for row in rows {
        let entitlement_id: String = row.try_get("id").map_sql_err()?;
        if input
            .admitted_entitlement_ids
            .is_some_and(|ids| !ids.iter().any(|admitted_id| admitted_id == &entitlement_id))
        {
            continue;
        }
        let entitlement_started_at =
            chrono::DateTime::from_timestamp(row.try_get::<i64, _>("starts_at").map_sql_err()?, 0)
                .ok_or_else(|| {
                    DataLayerError::UnexpectedValue("invalid entitlement start".to_string())
                })?;
        let entitlements_raw: String = row.try_get("entitlements_snapshot").map_sql_err()?;
        let entitlements =
            serde_json::from_str::<serde_json::Value>(&entitlements_raw).map_err(|err| {
                DataLayerError::UnexpectedValue(format!(
                    "user_plan_entitlements.entitlements_snapshot invalid json: {err}"
                ))
            })?;
        if input.admitted_entitlement_ids.is_none()
            && input.request_global_model_id.is_some()
            && !entitlements_snapshot_has_usage_quota_for_global_model(
                &entitlements,
                input.request_global_model_id,
            )
        {
            continue;
        }
        let stored_five_hour =
            find_usage_quota_window_sqlite(tx, &entitlement_id, QUOTA_SCOPE_FIVE_HOUR).await?;
        grants.extend(usage_quota_grants_from_entitlement(
            &entitlement_id,
            &entitlements,
            now,
            entitlement_started_at,
            stored_five_hour.as_ref(),
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
        let used = upsert_usage_quota_window_sqlite(tx, input.user_id, &grant, input.now_unix_secs)
            .await?;
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
            increment_usage_quota_window_sqlite(tx, grant, amount, input.now_unix_secs).await?;
        }
        let primary_grant = &grants[0].0;
        sqlx::query(
            r#"
INSERT OR IGNORE INTO entitlement_usage_ledgers (
  id, user_entitlement_id, user_id, request_id, amount_usd,
  balance_before, balance_after, usage_date, created_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(input.now_unix_secs)
        .execute(&mut **tx)
        .await
        .map_sql_err()?;
        remaining_base_cost -= base_amount;
        covered_base += base_amount;
    }
    Ok(DailyQuotaDebitResult {
        covered_base_usd: covered_base,
        insufficient: false,
    })
}

async fn find_usage_quota_window_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    entitlement_id: &str,
    scope: &str,
) -> Result<Option<StoredUsageQuotaWindow>, DataLayerError> {
    let row = sqlx::query(
        r#"
SELECT window_key, window_started_at, window_ends_at, used_usd
FROM entitlement_usage_windows
WHERE user_entitlement_id = ?
  AND window_scope = ?
LIMIT 1
        "#,
    )
    .bind(entitlement_id)
    .bind(scope)
    .fetch_optional(&mut **tx)
    .await
    .map_sql_err()?;
    row.map(|row| {
        Ok(StoredUsageQuotaWindow {
            window_key: row.try_get("window_key").map_sql_err()?,
            window_started_at: chrono::DateTime::from_timestamp(
                row.try_get::<i64, _>("window_started_at").map_sql_err()?,
                0,
            )
            .ok_or_else(|| {
                DataLayerError::UnexpectedValue("invalid quota window start".to_string())
            })?,
            window_ends_at: chrono::DateTime::from_timestamp(
                row.try_get::<i64, _>("window_ends_at").map_sql_err()?,
                0,
            )
            .ok_or_else(|| {
                DataLayerError::UnexpectedValue("invalid quota window end".to_string())
            })?,
        })
    })
    .transpose()
}

async fn upsert_usage_quota_window_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    grant: &UsageQuotaGrant,
    now_unix_secs: i64,
) -> Result<f64, DataLayerError> {
    let existing = sqlx::query(
        r#"
SELECT window_key, window_ends_at, used_usd
FROM entitlement_usage_windows
WHERE user_entitlement_id = ?
  AND window_scope = ?
LIMIT 1
        "#,
    )
    .bind(&grant.entitlement_id)
    .bind(grant.scope)
    .fetch_optional(&mut **tx)
    .await
    .map_sql_err()?;

    if let Some(row) = existing {
        let ends_at: i64 = row.try_get("window_ends_at").map_sql_err()?;
        if ends_at > now_unix_secs {
            return row.try_get("used_usd").map_sql_err();
        }
        sqlx::query(
            r#"
UPDATE entitlement_usage_windows
SET window_key = ?,
    window_started_at = ?,
    window_ends_at = ?,
    used_usd = 0,
    updated_at = ?
WHERE user_entitlement_id = ?
  AND window_scope = ?
            "#,
        )
        .bind(&grant.window_key)
        .bind(grant.window_started_at.timestamp())
        .bind(grant.window_ends_at.timestamp())
        .bind(now_unix_secs)
        .bind(&grant.entitlement_id)
        .bind(grant.scope)
        .execute(&mut **tx)
        .await
        .map_sql_err()?;
        return Ok(0.0);
    }

    sqlx::query(
        r#"
INSERT INTO entitlement_usage_windows (
  id, user_entitlement_id, user_id, window_scope, window_key,
  window_started_at, window_ends_at, used_usd, created_at, updated_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
        "#,
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&grant.entitlement_id)
    .bind(user_id)
    .bind(grant.scope)
    .bind(&grant.window_key)
    .bind(grant.window_started_at.timestamp())
    .bind(grant.window_ends_at.timestamp())
    .bind(now_unix_secs)
    .bind(now_unix_secs)
    .execute(&mut **tx)
    .await
    .map_sql_err()?;
    Ok(0.0)
}

async fn increment_usage_quota_window_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    grant: &UsageQuotaGrant,
    amount: f64,
    now_unix_secs: i64,
) -> Result<(), DataLayerError> {
    sqlx::query(
        r#"
UPDATE entitlement_usage_windows
SET used_usd = used_usd + ?,
    updated_at = ?
WHERE user_entitlement_id = ?
  AND window_scope = ?
        "#,
    )
    .bind(amount)
    .bind(now_unix_secs)
    .bind(&grant.entitlement_id)
    .bind(grant.scope)
    .execute(&mut **tx)
    .await
    .map_sql_err()?;
    Ok(())
}

#[async_trait]
impl SettlementWriteRepository for SqliteSettlementRepository {
    async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
        input.validate()?;
        let finalized_at = i64::try_from(
            input
                .finalized_at_unix_secs
                .unwrap_or(now_unix_secs()? as u64),
        )
        .map_err(|_| DataLayerError::InvalidInput("finalized_at overflow".to_string()))?;
        let updated_at = now_unix_secs()?;

        let mut tx = self.pool.begin().await.map_sql_err()?;
        let row = sqlx::query(FIND_USAGE_FOR_SETTLEMENT_SQL)
            .bind(&input.request_id)
            .fetch_optional(&mut *tx)
            .await
            .map_sql_err()?;

        let Some(usage_row) = row else {
            tx.commit().await.map_sql_err()?;
            return Ok(None);
        };

        let current_billing_status: String = usage_row.try_get("billing_status").map_sql_err()?;
        if matches!(
            current_billing_status.as_str(),
            "settled" | "void" | "insufficient_quota"
        ) {
            let settlement = settlement_from_row(&usage_row)?;
            tx.commit().await.map_sql_err()?;
            return Ok(Some(settlement));
        }

        let mut final_billing_status =
            settlement_billing_status_for_usage_status(&input.status).to_string();
        let mut settlement = StoredUsageSettlement {
            request_id: input.request_id.clone(),
            wallet_id: None,
            billing_status: final_billing_status.clone(),
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
            let billing_admission = find_billing_admission_sqlite(&mut tx, &input.request_id)
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

            let wallet_row = if let Some(wallet_id) = billing_admission.wallet_id.as_deref() {
                sqlx::query(
                    r#"
SELECT id, balance, gift_balance, limit_mode
FROM wallets
WHERE id = ?
LIMIT 1
"#,
                )
                .bind(wallet_id)
                .fetch_optional(&mut *tx)
                .await
                .map_sql_err()?
            } else {
                None
            };

            let wallet_can_overdraft = billing_admission.wallet_can_overdraft();
            let wallet_available_usd = match wallet_row.as_ref() {
                Some(row) => {
                    let limit_mode: String = row.try_get("limit_mode").map_sql_err()?;
                    if limit_mode.eq_ignore_ascii_case("unlimited") {
                        None
                    } else {
                        Some(finite_wallet_available_usd(
                            sqlite_real(row, "balance")?,
                            sqlite_real(row, "gift_balance")?,
                        ))
                    }
                }
                None => Some(0.0),
            };
            if let Some(row) = wallet_row.as_ref() {
                let wallet_id: String = row.try_get("id").map_sql_err()?;
                let before_recharge = sqlite_real(row, "balance")?;
                let before_gift = sqlite_real(row, "gift_balance")?;
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
                if let Some(user_id) = input.user_id.as_deref().filter(|value| !value.is_empty()) {
                    let sales_multiplier = settlement_wallet_charge_multiplier(&input);
                    let admitted_entitlement_ids = billing_admission
                        .uses_plan_for_provider(input.provider_id.as_deref())
                        .then(|| {
                            billing_admission
                                .entitlement_ids_for_provider(input.provider_id.as_deref())
                        });
                    let quota = consume_daily_quota_sqlite(
                        &mut tx,
                        DailyQuotaDebitInput {
                            user_id,
                            request_id: &input.request_id,
                            total_cost_usd: input.base_cost_usd,
                            wallet_available_usd,
                            wallet_can_overdraft,
                            wallet_charge_multiplier: sales_multiplier,
                            now_unix_secs: updated_at,
                            request_global_model_id: input.global_model_id.as_deref(),
                            admitted_entitlement_ids: admitted_entitlement_ids.as_deref(),
                            force_wallet_overage: admitted_entitlement_ids.is_some(),
                        },
                    )
                    .await?;
                    if quota.insufficient {
                        final_billing_status = "insufficient_quota".to_string();
                        settlement.billing_status = final_billing_status.clone();
                        0.0
                    } else {
                        (input.base_cost_usd - quota.covered_base_usd).max(0.0) * sales_multiplier
                    }
                } else {
                    input.total_cost_usd
                }
            } else {
                input.total_cost_usd
            };
            if final_billing_status != "settled" {
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
                    .bind(settlement.finalized_at_unix_secs.map(|value| value as i64))
                    .bind(updated_at)
                    .bind(updated_at)
                    .execute(&mut *tx)
                    .await
                    .map_sql_err()?;
                sqlx::query(FINALIZE_USAGE_BILLING_SQL)
                    .bind(&final_billing_status)
                    .bind(finalized_at)
                    .bind(&input.request_id)
                    .execute(&mut *tx)
                    .await
                    .map_sql_err()?;
                tx.commit().await.map_sql_err()?;
                return Ok(Some(settlement));
            }

            if wallet_debit_cost_usd > SETTLEMENT_EPSILON_USD {
                if let Some(wallet_row) = wallet_row {
                    let wallet_id: String = wallet_row.try_get("id").map_sql_err()?;
                    let before_recharge = sqlite_real(&wallet_row, "balance")?;
                    let before_gift = sqlite_real(&wallet_row, "gift_balance")?;
                    let limit_mode: String = wallet_row.try_get("limit_mode").map_sql_err()?;
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
  balance = ?,
  gift_balance = ?,
  total_consumed = COALESCE(total_consumed, 0) + ?,
  updated_at = ?
WHERE id = ?
"#,
                        )
                        .bind(after_recharge)
                        .bind(after_gift)
                        .bind(wallet_debit_cost_usd)
                        .bind(updated_at)
                        .bind(&wallet_id)
                        .execute(&mut *tx)
                        .await
                        .map_sql_err()?;
                    }

                    settlement.wallet_id = Some(wallet_id);
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
                    .bind(settlement.finalized_at_unix_secs.map(|value| value as i64))
                    .bind(updated_at)
                    .bind(updated_at)
                    .execute(&mut *tx)
                    .await
                    .map_sql_err()?;
                sqlx::query(FINALIZE_USAGE_BILLING_SQL)
                    .bind(&final_billing_status)
                    .bind(finalized_at)
                    .bind(&input.request_id)
                    .execute(&mut *tx)
                    .await
                    .map_sql_err()?;
                tx.commit().await.map_sql_err()?;
                return Ok(Some(settlement));
            }

            if let Some(provider_id) = input
                .provider_id
                .as_deref()
                .filter(|value| !value.is_empty())
            {
                sqlx::query(
                    r#"
UPDATE providers
SET
  monthly_used_usd = CAST(COALESCE(monthly_used_usd, 0) AS REAL) + ?,
  updated_at = ?
WHERE id = ?
"#,
                )
                .bind(input.actual_total_cost_usd)
                .bind(updated_at)
                .bind(provider_id)
                .execute(&mut *tx)
                .await
                .map_sql_err()?;

                settlement.provider_monthly_used_usd = sqlx::query(
                    "SELECT CAST(monthly_used_usd AS REAL) AS monthly_used_usd FROM providers WHERE id = ? LIMIT 1",
                )
                .bind(provider_id)
                .fetch_optional(&mut *tx)
                .await
                .map_sql_err()?
                .map(|row| sqlite_real(&row, "monthly_used_usd"))
                .transpose()?;
            }
        }

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
            .bind(settlement.finalized_at_unix_secs.map(|value| value as i64))
            .bind(updated_at)
            .bind(updated_at)
            .execute(&mut *tx)
            .await
            .map_sql_err()?;

        sqlx::query(FINALIZE_USAGE_BILLING_SQL)
            .bind(&final_billing_status)
            .bind(finalized_at)
            .bind(&input.request_id)
            .execute(&mut *tx)
            .await
            .map_sql_err()?;

        tx.commit().await.map_sql_err()?;
        Ok(Some(settlement))
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteSettlementRepository;
    use crate::lifecycle::migrate::run_sqlite_migrations;
    use crate::repository::settlement::{SettlementWriteRepository, UsageSettlementInput};
    use sqlx::Row;

    #[tokio::test]
    async fn sqlite_repository_settles_usage_once() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_settlement_rows(&pool).await;

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-1".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: None,
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_234),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "settled");
        assert_eq!(settlement.wallet_id.as_deref(), Some("wallet-1"));
        assert_eq!(settlement.wallet_balance_before, Some(12.0));
        assert_eq!(settlement.wallet_balance_after, Some(9.0));
        assert_eq!(settlement.wallet_recharge_balance_after, Some(7.0));
        assert_eq!(settlement.wallet_gift_balance_after, Some(2.0));
        assert_eq!(settlement.provider_monthly_used_usd, Some(7.0));

        let wallet = sqlx::query(
            "SELECT balance, gift_balance, total_consumed FROM wallets WHERE id = 'wallet-1'",
        )
        .fetch_one(&pool)
        .await
        .expect("wallet should load");
        assert_eq!(wallet.try_get::<f64, _>("balance").unwrap(), 7.0);
        assert_eq!(wallet.try_get::<f64, _>("gift_balance").unwrap(), 2.0);
        assert_eq!(wallet.try_get::<f64, _>("total_consumed").unwrap(), 3.0);

        let second = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-1".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: None,
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(9_999),
            })
            .await
            .expect("second settlement should run")
            .expect("usage should exist");
        assert_eq!(second.finalized_at_unix_secs, Some(1_234));

        let provider_used: f64 =
            sqlx::query_scalar("SELECT monthly_used_usd FROM providers WHERE id = 'provider-1'")
                .fetch_one(&pool)
                .await
                .expect("provider should load");
        assert_eq!(provider_used, 7.0);
    }

    #[tokio::test]
    async fn sqlite_repository_voids_failed_usage_without_wallet_mutation() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_settlement_rows(&pool).await;

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-2".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: None,
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "failed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_235),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "void");
        assert_eq!(settlement.wallet_id, None);
        let wallet_total: f64 =
            sqlx::query_scalar("SELECT balance + gift_balance FROM wallets WHERE id = 'wallet-1'")
                .fetch_one(&pool)
                .await
                .expect("wallet should load");
        assert_eq!(wallet_total, 12.0);
    }

    #[tokio::test]
    async fn sqlite_repository_preserves_pending_cost_when_billing_admission_is_missing() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_settlement_rows(&pool).await;

        let repository = SqliteSettlementRepository::new(pool.clone());
        let error = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-overdraw".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: None,
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 15.0,
                total_cost_usd: 15.0,
                actual_total_cost_usd: 7.5,
                finalized_at_unix_secs: Some(1_236),
            })
            .await
            .expect_err("missing billing admission must stop settlement");
        assert!(error
            .to_string()
            .contains("billing admission missing for request request-overdraw"));

        let wallet = sqlx::query(
            "SELECT balance, gift_balance, total_consumed FROM wallets WHERE id = 'wallet-1'",
        )
        .fetch_one(&pool)
        .await
        .expect("wallet should load");
        assert_eq!(wallet.try_get::<f64, _>("balance").unwrap(), 10.0);
        assert_eq!(wallet.try_get::<f64, _>("gift_balance").unwrap(), 2.0);
        assert_eq!(wallet.try_get::<f64, _>("total_consumed").unwrap(), 0.0);

        let usage = sqlx::query(
            "SELECT billing_status, total_cost_usd FROM usage WHERE request_id = 'request-overdraw'",
        )
        .fetch_one(&pool)
        .await
        .expect("usage should remain available for settlement retry");
        assert_eq!(
            usage.try_get::<String, _>("billing_status").unwrap(),
            "pending"
        );
        assert_eq!(usage.try_get::<f64, _>("total_cost_usd").unwrap(), 15.0);
    }

    #[tokio::test]
    async fn sqlite_repository_records_wallet_for_quota_covered_user_usage() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        seed_billing_admission(
            &pool,
            "request-quota-covered",
            "plan",
            0.0,
            false,
            false,
            &["entitlement-quota"],
            "provider-plan",
        )
        .await;

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-plan".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_260),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "settled");
        assert_eq!(settlement.wallet_id.as_deref(), Some("wallet-quota"));
        assert_eq!(settlement.wallet_balance_before, Some(0.0));
        assert_eq!(settlement.wallet_balance_after, Some(0.0));

        let wallet_total: f64 = sqlx::query_scalar(
            "SELECT balance + gift_balance FROM wallets WHERE id = 'wallet-quota'",
        )
        .fetch_one(&pool)
        .await
        .expect("wallet should load");
        assert_eq!(wallet_total, 0.0);

        let quota_used: f64 = sqlx::query_scalar(
            "SELECT CAST(COALESCE(SUM(amount_usd), 0) AS REAL) FROM entitlement_usage_ledgers WHERE request_id = 'request-quota-covered'",
        )
        .fetch_one(&pool)
        .await
        .expect("quota ledger should load");
        assert_eq!(quota_used, 3.0);
    }

    #[tokio::test]
    async fn sqlite_repository_applies_five_hour_and_weekly_package_limits() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        seed_billing_admission(
            &pool,
            "request-quota-covered",
            "plan",
            0.0,
            false,
            false,
            &["entitlement-quota"],
            "provider-plan",
        )
        .await;
        sqlx::query(
            r#"
UPDATE user_plan_entitlements
SET entitlements_snapshot =
  '[{"type":"daily_quota","daily_quota_usd":10.0,"five_hour_quota_usd":4.0,"weekly_quota_usd":5.0,"reset_timezone":"Asia/Shanghai","allow_wallet_overage":false}]'
WHERE id = 'entitlement-quota'
            "#,
        )
        .execute(&pool)
        .await
        .expect("entitlement should update");

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-plan".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_260),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "settled");
        let five_hour_used: f64 = sqlx::query_scalar(
            "SELECT used_usd FROM entitlement_usage_windows WHERE user_entitlement_id = 'entitlement-quota' AND window_scope = 'five_hour'",
        )
        .fetch_one(&pool)
        .await
        .expect("five hour window should load");
        assert_eq!(five_hour_used, 3.0);
        let weekly_used: f64 = sqlx::query_scalar(
            "SELECT used_usd FROM entitlement_usage_windows WHERE user_entitlement_id = 'entitlement-quota' AND window_scope = 'weekly'",
        )
        .fetch_one(&pool)
        .await
        .expect("weekly window should load");
        assert_eq!(weekly_used, 3.0);

        sqlx::query(
            r#"
INSERT INTO "usage" (
  request_id, user_id, api_key_id, status, billing_status,
  total_cost_usd, actual_total_cost_usd
) VALUES (
  'request-quota-over-weekly', 'user-quota', 'key-quota', 'completed',
  'pending', 3.0, 2.0
)
            "#,
        )
        .execute(&pool)
        .await
        .expect("second usage should seed");
        seed_billing_admission(
            &pool,
            "request-quota-over-weekly",
            "plan",
            0.0,
            false,
            false,
            &["entitlement-quota"],
            "provider-plan",
        )
        .await;
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-over-weekly".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-plan".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_320),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "insufficient_quota");
    }

    #[tokio::test]
    async fn sqlite_repository_applies_package_quota_multiplier() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        seed_billing_admission(
            &pool,
            "request-quota-covered",
            "plan",
            0.0,
            false,
            false,
            &["entitlement-quota"],
            "provider-plan",
        )
        .await;
        sqlx::query(
            r#"
UPDATE user_plan_entitlements
SET entitlements_snapshot =
  '[{"type":"daily_quota","daily_quota_usd":10.0,"quota_multiplier":0.5,"reset_timezone":"Asia/Shanghai","allow_wallet_overage":false}]'
WHERE id = 'entitlement-quota'
            "#,
        )
        .execute(&pool)
        .await
        .expect("entitlement should update");

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-plan".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_260),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "settled");
        let quota_used: f64 = sqlx::query_scalar(
            "SELECT CAST(COALESCE(SUM(amount_usd), 0) AS REAL) FROM entitlement_usage_ledgers WHERE request_id = 'request-quota-covered'",
        )
        .fetch_one(&pool)
        .await
        .expect("quota ledger should load");
        assert_eq!(quota_used, 1.5);
    }

    #[tokio::test]
    async fn sqlite_repository_charges_wallet_for_base_cost_not_covered_by_quota_multiplier() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        sqlx::query("UPDATE wallets SET balance = 10.0 WHERE id = 'wallet-quota'")
            .execute(&pool)
            .await
            .expect("wallet should update");
        sqlx::query(
            r#"
UPDATE user_plan_entitlements
SET entitlements_snapshot =
  '[{"type":"daily_quota","daily_quota_usd":2.0,"quota_multiplier":2.0,"reset_timezone":"Asia/Shanghai","allow_wallet_overage":true}]'
WHERE id = 'entitlement-quota'
            "#,
        )
        .execute(&pool)
        .await
        .expect("entitlement should update");
        seed_billing_admission(
            &pool,
            "request-quota-covered",
            "plan",
            10.0,
            true,
            true,
            &["entitlement-quota"],
            "provider-plan",
        )
        .await;

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-plan".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 6.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_260),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "settled");
        let quota_used: f64 = sqlx::query_scalar(
            "SELECT CAST(COALESCE(SUM(amount_usd), 0) AS REAL) FROM entitlement_usage_ledgers WHERE request_id = 'request-quota-covered'",
        )
        .fetch_one(&pool)
        .await
        .expect("quota ledger should load");
        assert_eq!(quota_used, 2.0);
        let wallet_total: f64 = sqlx::query_scalar(
            "SELECT balance + gift_balance FROM wallets WHERE id = 'wallet-quota'",
        )
        .fetch_one(&pool)
        .await
        .expect("wallet should load");
        assert_eq!(wallet_total, 6.0);
    }

    #[tokio::test]
    async fn sqlite_repository_combines_multiple_package_quota_multipliers() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        sqlx::query("UPDATE wallets SET balance = 10.0 WHERE id = 'wallet-quota'")
            .execute(&pool)
            .await
            .expect("wallet should update");
        sqlx::query(
            r#"
UPDATE user_plan_entitlements
SET entitlements_snapshot =
  '[{"type":"daily_quota","daily_quota_usd":2.0,"quota_multiplier":0.5,"reset_timezone":"Asia/Shanghai","allow_wallet_overage":true}]'
WHERE id = 'entitlement-quota';

INSERT INTO payment_orders (
  id, order_no, wallet_id, user_id, amount_usd, refunded_amount_usd,
  refundable_amount_usd, payment_method, gateway_response, status, created_at
) VALUES (
  'order-quota-2', 'order-quota-2', 'wallet-quota', 'user-quota', 0.0, 0.0,
  0.0, 'admin_manual', '{}', 'credited', 1
);

INSERT INTO user_plan_entitlements (
  id, user_id, plan_id, payment_order_id, status, starts_at, expires_at,
  entitlements_snapshot, created_at, updated_at
) VALUES (
  'entitlement-quota-slow', 'user-quota', 'plan-quota', 'order-quota-2',
  'active', 1, 9999999999,
  '[{"type":"daily_quota","daily_quota_usd":2.0,"quota_multiplier":2.0,"reset_timezone":"Asia/Shanghai","allow_wallet_overage":true}]',
  2, 2
);
            "#,
        )
        .execute(&pool)
        .await
        .expect("second entitlement should seed");
        seed_billing_admission(
            &pool,
            "request-quota-covered",
            "plan",
            10.0,
            true,
            true,
            &["entitlement-quota", "entitlement-quota-slow"],
            "provider-plan",
        )
        .await;

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-plan".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 6.0,
                total_cost_usd: 12.0,
                actual_total_cost_usd: 3.0,
                finalized_at_unix_secs: Some(1_260),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "settled");
        let fast_quota_used: f64 = sqlx::query_scalar(
            "SELECT amount_usd FROM entitlement_usage_ledgers WHERE request_id = 'request-quota-covered' AND user_entitlement_id = 'entitlement-quota'",
        )
        .fetch_one(&pool)
        .await
        .expect("fast quota ledger should load");
        assert_eq!(fast_quota_used, 2.0);
        let slow_quota_used: f64 = sqlx::query_scalar(
            "SELECT amount_usd FROM entitlement_usage_ledgers WHERE request_id = 'request-quota-covered' AND user_entitlement_id = 'entitlement-quota-slow'",
        )
        .fetch_one(&pool)
        .await
        .expect("slow quota ledger should load");
        assert_eq!(slow_quota_used, 2.0);
        let wallet_total: f64 = sqlx::query_scalar(
            "SELECT balance + gift_balance FROM wallets WHERE id = 'wallet-quota'",
        )
        .fetch_one(&pool)
        .await
        .expect("wallet should load");
        assert_eq!(wallet_total, 8.0);
    }

    #[tokio::test]
    async fn sqlite_admitted_plan_exhausts_quota_then_overdraws_wallet() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        sqlx::query(
            r#"
UPDATE wallets SET balance = -1.0, gift_balance = 0.0 WHERE id = 'wallet-quota';
UPDATE user_plan_entitlements
SET entitlements_snapshot =
  '[{"type":"daily_quota","daily_quota_usd":2.0,"allowed_global_model_ids":["legacy-model"],"allow_wallet_overage":false}]'
WHERE id = 'entitlement-quota';
UPDATE "usage"
SET total_cost_usd = 8.0, actual_total_cost_usd = 8.0
WHERE request_id = 'request-quota-covered';
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, global_model_id, funding_source,
  wallet_balance_at_admission, wallet_payment_allowed, wallet_overage_allowed,
  entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
  billing_admitted, status, schema_version, created_at, updated_at
) VALUES (
  'request-quota-covered', 'user-quota', 'wallet-quota', 'global-current', 'plan',
  -1.0, 0, 1, '["entitlement-quota"]', '{"entitlement-quota":["provider-plan"]}', '["provider-plan"]',
  1, 'admitted', 1, 1, 1
);
            "#,
        )
        .execute(&pool)
        .await
        .expect("admitted plan settlement should seed");

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-plan".to_string()),
                global_model_id: Some("global-current".to_string()),
                global_model_name: Some("current-model".to_string()),
                model: Some("current-model".to_string()),
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 8.0,
                total_cost_usd: 8.0,
                actual_total_cost_usd: 8.0,
                finalized_at_unix_secs: Some(1_260),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "settled");
        assert_eq!(settlement.wallet_balance_before, Some(-1.0));
        assert_eq!(settlement.wallet_balance_after, Some(-7.0));
        let quota_used: f64 = sqlx::query_scalar(
            "SELECT CAST(COALESCE(SUM(amount_usd), 0) AS REAL) FROM entitlement_usage_ledgers WHERE request_id = 'request-quota-covered'",
        )
        .fetch_one(&pool)
        .await
        .expect("quota usage should load");
        assert_eq!(quota_used, 2.0);
    }

    #[tokio::test]
    async fn sqlite_admitted_wallet_request_can_overdraw_once_then_settles_idempotently() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_settlement_rows(&pool).await;
        sqlx::query(
            r#"
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, global_model_id, funding_source,
  wallet_balance_at_admission, wallet_payment_allowed, wallet_overage_allowed,
  entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
  billing_admitted, status, schema_version, created_at, updated_at
) VALUES (
  'request-overdraw', NULL, 'wallet-1', 'global-1', 'wallet',
  12.0, 1, 0, '[]', '{}', '[]',
  1, 'admitted', 1, 1, 1
)
            "#,
        )
        .execute(&pool)
        .await
        .expect("wallet admission should seed");

        let repository = SqliteSettlementRepository::new(pool.clone());
        let input = UsageSettlementInput {
            request_id: "request-overdraw".to_string(),
            user_id: Some("user-1".to_string()),
            api_key_id: None,
            api_key_is_standalone: false,
            provider_id: Some("provider-1".to_string()),
            global_model_id: Some("global-1".to_string()),
            global_model_name: Some("model-1".to_string()),
            model: Some("model-1".to_string()),
            status: "completed".to_string(),
            billing_status: "pending".to_string(),
            base_cost_usd: 15.0,
            total_cost_usd: 15.0,
            actual_total_cost_usd: 7.5,
            finalized_at_unix_secs: Some(1_300),
        };
        let first = repository
            .settle_usage(input.clone())
            .await
            .expect("settlement should run")
            .expect("usage should exist");
        assert_eq!(first.wallet_balance_before, Some(12.0));
        assert_eq!(first.wallet_balance_after, Some(-3.0));

        let second = repository
            .settle_usage(input)
            .await
            .expect("duplicate settlement should resolve")
            .expect("usage should exist");
        assert_eq!(second.wallet_balance_after, Some(-3.0));
        let wallet_total: f64 =
            sqlx::query_scalar("SELECT balance + gift_balance FROM wallets WHERE id = 'wallet-1'")
                .fetch_one(&pool)
                .await
                .expect("wallet should load");
        assert_eq!(wallet_total, -3.0);
    }

    #[tokio::test]
    async fn sqlite_wallet_admission_charges_wallet_for_a_model_outside_the_plan() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        sqlx::query(
            r#"
UPDATE wallets SET balance = 5.0 WHERE id = 'wallet-quota';
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, global_model_id, funding_source,
  wallet_balance_at_admission, wallet_payment_allowed, wallet_overage_allowed,
  entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
  billing_admitted, status, schema_version, created_at, updated_at
) VALUES (
  'request-quota-covered', 'user-quota', 'wallet-quota', 'global-current', 'wallet',
  5.0, 1, 0, '[]', '{}', '[]',
  1, 'admitted', 1, 1, 1
)
            "#,
        )
        .execute(&pool)
        .await
        .expect("plan admission should seed");

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-wallet".to_string()),
                global_model_id: Some("global-current".to_string()),
                global_model_name: Some("current-model".to_string()),
                model: Some("current-model".to_string()),
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_300),
            })
            .await
            .expect("wallet-funded request should settle")
            .expect("usage should exist");

        assert_eq!(settlement.wallet_balance_before, Some(5.0));
        assert_eq!(settlement.wallet_balance_after, Some(2.0));
        let quota_used: f64 = sqlx::query_scalar(
            "SELECT CAST(COALESCE(SUM(amount_usd), 0) AS REAL) FROM entitlement_usage_ledgers WHERE request_id = 'request-quota-covered'",
        )
        .fetch_one(&pool)
        .await
        .expect("quota ledger should load");
        assert_eq!(quota_used, 0.0);
    }

    #[tokio::test]
    async fn sqlite_plan_admission_rejects_provider_mismatch_when_wallet_was_in_debt() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        sqlx::query(
            r#"
UPDATE wallets SET balance = -1.0 WHERE id = 'wallet-quota';
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, global_model_id, funding_source,
  wallet_balance_at_admission, wallet_payment_allowed, wallet_overage_allowed,
  entitlement_ids, entitlement_provider_scopes, allowed_provider_ids,
  billing_admitted, status, schema_version, created_at, updated_at
) VALUES (
  'request-quota-covered', 'user-quota', 'wallet-quota', 'global-current', 'plan',
  -1.0, 0, 1, '["entitlement-quota"]', '{"entitlement-quota":["provider-plan"]}', '["provider-plan"]',
  1, 'admitted', 1, 1, 1
)
            "#,
        )
        .execute(&pool)
        .await
        .expect("plan admission should seed");

        let repository = SqliteSettlementRepository::new(pool.clone());
        let result = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-outside-plan".to_string()),
                global_model_id: Some("global-current".to_string()),
                global_model_name: Some("current-model".to_string()),
                model: Some("current-model".to_string()),
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_300),
            })
            .await;

        assert!(result.is_err());
        let wallet_total: f64 = sqlx::query_scalar(
            "SELECT balance + gift_balance FROM wallets WHERE id = 'wallet-quota'",
        )
        .fetch_one(&pool)
        .await
        .expect("wallet should load");
        assert_eq!(wallet_total, -1.0);
        let billing_status: String = sqlx::query_scalar(
            "SELECT billing_status FROM \"usage\" WHERE request_id = 'request-quota-covered'",
        )
        .fetch_one(&pool)
        .await
        .expect("usage should load");
        assert_eq!(billing_status, "pending");
    }

    #[tokio::test]
    async fn sqlite_repository_does_not_consume_plan_quota_for_other_global_model() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect");
        run_sqlite_migrations(&pool)
            .await
            .expect("sqlite migrations should run");
        seed_quota_covered_settlement_rows(&pool).await;
        sqlx::query("UPDATE wallets SET balance = 10.0 WHERE id = 'wallet-quota'")
            .execute(&pool)
            .await
            .expect("wallet should update");
        seed_billing_admission(
            &pool,
            "request-quota-covered",
            "wallet",
            10.0,
            true,
            false,
            &[],
            "provider-wallet",
        )
        .await;
        sqlx::query(
            r#"
UPDATE user_plan_entitlements
SET entitlements_snapshot =
  '[{"type":"daily_quota","daily_quota_usd":10.0,"allowed_global_model_ids":["global-codex"],"reset_timezone":"Asia/Shanghai","allow_wallet_overage":false}]'
WHERE id = 'entitlement-quota'
            "#,
        )
        .execute(&pool)
        .await
        .expect("entitlement should update");

        let repository = SqliteSettlementRepository::new(pool.clone());
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "request-quota-covered".to_string(),
                user_id: Some("user-quota".to_string()),
                api_key_id: Some("key-quota".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-wallet".to_string()),
                global_model_id: Some("global-claude".to_string()),
                global_model_name: Some("claude-sonnet".to_string()),
                model: Some("claude-sonnet".to_string()),
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 2.0,
                finalized_at_unix_secs: Some(1_260),
            })
            .await
            .expect("settlement should run")
            .expect("usage should exist");

        assert_eq!(settlement.billing_status, "settled");
        let quota_used: f64 = sqlx::query_scalar(
            "SELECT CAST(COALESCE(SUM(amount_usd), 0) AS REAL) FROM entitlement_usage_ledgers WHERE request_id = 'request-quota-covered'",
        )
        .fetch_one(&pool)
        .await
        .expect("quota ledger should load");
        assert_eq!(quota_used, 0.0);
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_billing_admission(
        pool: &sqlx::SqlitePool,
        request_id: &str,
        funding_source: &str,
        wallet_balance: f64,
        wallet_payment_allowed: bool,
        wallet_overage_allowed: bool,
        entitlement_ids: &[&str],
        provider_id: &str,
    ) {
        let entitlement_provider_scopes = entitlement_ids
            .iter()
            .map(|entitlement_id| ((*entitlement_id).to_string(), vec![provider_id.to_string()]))
            .collect::<std::collections::BTreeMap<_, _>>();
        sqlx::query(
            r#"
INSERT INTO billing_request_admissions (
  request_id, user_id, wallet_id, funding_source, wallet_balance_at_admission,
  wallet_payment_allowed, wallet_overage_allowed, entitlement_ids,
  entitlement_provider_scopes, allowed_provider_ids,
  billing_admitted, status, schema_version, created_at, updated_at
) VALUES (?, 'user-quota', 'wallet-quota', ?, ?, ?, ?, ?, ?, ?, 1, 'admitted', 1, 1, 1)
            "#,
        )
        .bind(request_id)
        .bind(funding_source)
        .bind(wallet_balance)
        .bind(if wallet_payment_allowed { 1_i64 } else { 0_i64 })
        .bind(if wallet_overage_allowed { 1_i64 } else { 0_i64 })
        .bind(serde_json::to_string(entitlement_ids).expect("entitlement ids should serialize"))
        .bind(
            serde_json::to_string(&entitlement_provider_scopes)
                .expect("provider scopes should serialize"),
        )
        .bind(serde_json::to_string(&[provider_id]).expect("provider ids should serialize"))
        .execute(pool)
        .await
        .expect("billing admission should seed");
    }

    async fn seed_settlement_rows(pool: &sqlx::SqlitePool) {
        sqlx::query(
            r#"
INSERT INTO providers (
  id, name, provider_type, monthly_used_usd, created_at, updated_at
)
VALUES ('provider-1', 'Provider One', 'openai', 5.0, 1, 1);

INSERT INTO wallets (
  id, user_id, balance, gift_balance, limit_mode, created_at, updated_at
)
VALUES ('wallet-1', 'user-1', 10.0, 2.0, 'finite', 1, 1);

INSERT INTO "usage" (
  request_id, user_id, provider_id, status, billing_status, total_cost_usd, actual_total_cost_usd
)
VALUES
  ('request-1', 'user-1', 'provider-1', 'completed', 'pending', 3.0, 2.0),
  ('request-2', 'user-1', 'provider-1', 'failed', 'pending', 3.0, 2.0),
  ('request-overdraw', 'user-1', 'provider-1', 'completed', 'pending', 15.0, 7.5);

INSERT INTO billing_request_admissions (
  request_id, wallet_id, funding_source, wallet_balance_at_admission,
  wallet_payment_allowed, wallet_overage_allowed, entitlement_ids,
  entitlement_provider_scopes, allowed_provider_ids,
  billing_admitted, status, schema_version, created_at, updated_at
) VALUES (
  'request-1', 'wallet-1', 'wallet', 12.0,
  1, 0, '[]', '{}', '["provider-1"]',
  1, 'admitted', 1, 1, 1
);
"#,
        )
        .execute(pool)
        .await
        .expect("settlement rows should seed");
    }

    async fn seed_quota_covered_settlement_rows(pool: &sqlx::SqlitePool) {
        sqlx::query(
            r#"
INSERT INTO users (
  id, username, email, role, auth_source, password_hash, is_active,
  is_deleted, created_at, updated_at
) VALUES (
  'user-quota', 'quota-user', 'quota@example.com', 'user', 'local',
  'hash', 1, 0, 1, 1
);

INSERT INTO wallets (
  id, user_id, balance, gift_balance, limit_mode, created_at, updated_at
) VALUES (
  'wallet-quota', 'user-quota', 0.0, 0.0, 'finite', 1, 1
);

INSERT INTO "usage" (
  request_id, user_id, api_key_id, status, billing_status,
  total_cost_usd, actual_total_cost_usd
) VALUES (
  'request-quota-covered', 'user-quota', 'key-quota', 'completed',
  'pending', 3.0, 2.0
);

INSERT INTO billing_plans (
  id, title, price_amount, price_currency, duration_unit,
  duration_value, entitlements_json, created_at, updated_at
) VALUES (
  'plan-quota', 'Quota Plan', 0.0, 'USD', 'month', 1,
  '[{"type":"daily_quota","daily_quota_usd":10.0,"reset_timezone":"Asia/Shanghai","allow_wallet_overage":false}]',
  1, 1
);

INSERT INTO payment_orders (
  id, order_no, wallet_id, user_id, amount_usd, refunded_amount_usd,
  refundable_amount_usd, payment_method, gateway_response, status, created_at
) VALUES (
  'order-quota', 'order-quota', 'wallet-quota', 'user-quota', 0.0, 0.0,
  0.0, 'admin_manual', '{}', 'credited', 1
);

INSERT INTO user_plan_entitlements (
  id, user_id, plan_id, payment_order_id, status, starts_at, expires_at,
  entitlements_snapshot, created_at, updated_at
) VALUES (
  'entitlement-quota', 'user-quota', 'plan-quota', 'order-quota',
  'active', 1, 9999999999,
  '[{"type":"daily_quota","daily_quota_usd":10.0,"reset_timezone":"Asia/Shanghai","allow_wallet_overage":false}]',
  1, 1
);
"#,
        )
        .execute(pool)
        .await
        .expect("quota settlement rows should seed");
    }
}

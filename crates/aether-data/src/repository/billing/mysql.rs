use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use sqlx::{mysql::MySqlRow, MySql, QueryBuilder, Row};

use super::{
    quota::{
        usage_quota_grants_from_entitlement, LoadedUserPlanQuotaSummary, StoredUsageQuotaWindow,
        StoredUsageQuotaWindows,
    },
    AdminBillingCollectorRecord, AdminBillingCollectorWriteInput, AdminBillingMutationOutcome,
    AdminBillingPresetApplyResult, AdminBillingRuleRecord, AdminBillingRuleWriteInput,
    BillingPlanRecord, BillingPlanWriteInput, BillingReadRepository, PaymentGatewayConfigRecord,
    PaymentGatewayConfigWriteInput, StoredBillingModelContext, UserDailyQuotaAvailabilityRecord,
    UserPlanEntitlementRecord, UserPlanEntitlementUpdateInput, UserPlanQuotaSummaryRecord,
};
use crate::driver::mysql::MysqlPool;
use crate::error::SqlResultExt;
use crate::DataLayerError;

const MODEL_CONTEXT_COLUMNS: &str = r#"
SELECT
  p.id AS provider_id,
  p.billing_type AS provider_billing_type,
  p.config AS provider_config,
  pak.id AS provider_api_key_id,
  pak.rate_multipliers AS provider_api_key_rate_multipliers,
  pak.cache_ttl_minutes AS provider_api_key_cache_ttl_minutes,
  gm.id AS global_model_id,
  gm.name AS global_model_name,
  gm.config AS global_model_config,
  gm.default_price_per_request AS default_price_per_request,
  gm.default_tiered_pricing AS default_tiered_pricing,
  m.id AS model_id,
  m.provider_model_name AS model_provider_model_name,
  m.config AS model_config,
  m.price_per_request AS model_price_per_request,
  m.tiered_pricing AS model_tiered_pricing,
  m.provider_model_mappings AS provider_model_mappings,
  m.is_available AS model_is_available,
  m.created_at AS model_created_at
FROM providers p
"#;

#[derive(Debug, Clone)]
pub struct MysqlBillingReadRepository {
    pool: MysqlPool,
}

impl MysqlBillingReadRepository {
    pub fn new(pool: MysqlPool) -> Self {
        Self { pool }
    }

    async fn load_all_plan_provider_ids(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, DataLayerError> {
        let rows = sqlx::query(
            "SELECT plan_id, provider_id FROM billing_plan_providers ORDER BY plan_id, provider_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        let mut scopes = BTreeMap::<String, Vec<String>>::new();
        for row in rows {
            scopes
                .entry(row.try_get("plan_id").map_sql_err()?)
                .or_default()
                .push(row.try_get("provider_id").map_sql_err()?);
        }
        Ok(scopes)
    }

    async fn load_plan_provider_ids(&self, plan_id: &str) -> Result<Vec<String>, DataLayerError> {
        sqlx::query_scalar::<_, String>(
            "SELECT provider_id FROM billing_plan_providers WHERE plan_id = ? ORDER BY provider_id",
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()
    }
}

async fn replace_plan_provider_ids_mysql(
    transaction: &mut sqlx::Transaction<'_, sqlx::MySql>,
    plan_id: &str,
    provider_ids: &[String],
    now: i64,
) -> Result<(), DataLayerError> {
    sqlx::query("DELETE FROM billing_plan_providers WHERE plan_id = ?")
        .bind(plan_id)
        .execute(&mut **transaction)
        .await
        .map_sql_err()?;
    for provider_id in provider_ids {
        sqlx::query(
            "INSERT INTO billing_plan_providers (plan_id, provider_id, created_at) VALUES (?, ?, ?)",
        )
        .bind(plan_id)
        .bind(provider_id)
        .bind(now)
        .execute(&mut **transaction)
        .await
        .map_sql_err()?;
    }
    Ok(())
}

#[async_trait]
impl BillingReadRepository for MysqlBillingReadRepository {
    async fn find_model_context(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        global_model_name: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        let rows = sqlx::query(&format!(
            r#"
{MODEL_CONTEXT_COLUMNS}
INNER JOIN global_models gm
  ON gm.is_active = 1
LEFT JOIN models m
  ON m.global_model_id = gm.id
 AND m.provider_id = p.id
 AND m.is_active = 1
LEFT JOIN provider_api_keys pak
  ON pak.id = ?
 AND pak.provider_id = p.id
WHERE p.id = ?
  AND (
    gm.name = ?
    OR m.provider_model_name = ?
    OR m.provider_model_mappings IS NOT NULL
  )
"#
        ))
        .bind(provider_api_key_id)
        .bind(provider_id)
        .bind(global_model_name)
        .bind(global_model_name)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;

        rows.iter()
            .filter_map(|row| match_rank(row, global_model_name).transpose())
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .min_by_key(|candidate| {
                (
                    candidate.rank,
                    !candidate.is_available,
                    candidate.pricing_rank,
                    candidate.created_at,
                )
            })
            .map(|candidate| candidate.context)
            .transpose()
    }

    async fn find_model_context_by_model_id(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        model_id: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        let row = sqlx::query(&format!(
            r#"
{MODEL_CONTEXT_COLUMNS}
INNER JOIN models m
  ON m.id = ?
 AND m.provider_id = p.id
 AND m.is_active = 1
INNER JOIN global_models gm
  ON gm.id = m.global_model_id
 AND gm.is_active = 1
LEFT JOIN provider_api_keys pak
  ON pak.id = ?
 AND pak.provider_id = p.id
WHERE p.id = ?
LIMIT 1
"#
        ))
        .bind(model_id)
        .bind(provider_api_key_id)
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .map_sql_err()?;
        row.as_ref().map(map_row).transpose()
    }

    async fn admin_billing_enabled_default_value_exists(
        &self,
        api_format: &str,
        task_type: &str,
        dimension_name: &str,
        existing_id: Option<&str>,
    ) -> Result<Option<bool>, DataLayerError> {
        let row = sqlx::query(
            r#"
SELECT COUNT(*) AS total
FROM dimension_collectors
WHERE api_format = ?
  AND task_type = ?
  AND dimension_name = ?
  AND is_enabled = 1
  AND default_value IS NOT NULL
  AND (? IS NULL OR id <> ?)
            "#,
        )
        .bind(api_format)
        .bind(task_type)
        .bind(dimension_name)
        .bind(existing_id)
        .bind(existing_id)
        .fetch_one(&self.pool)
        .await
        .map_sql_err()?;
        Ok(Some(read_count_mysql(&row)? > 0))
    }

    async fn create_admin_billing_rule(
        &self,
        input: &AdminBillingRuleWriteInput,
    ) -> Result<AdminBillingMutationOutcome<AdminBillingRuleRecord>, DataLayerError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = current_unix_secs_i64();
        let result = sqlx::query(
            r#"
INSERT INTO billing_rules (
  id, name, task_type, global_model_id, model_id, expression, variables,
  dimension_mappings, is_enabled, created_at, updated_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.task_type)
        .bind(input.global_model_id.as_deref())
        .bind(input.model_id.as_deref())
        .bind(&input.expression)
        .bind(json_to_string(&input.variables)?)
        .bind(json_to_string(&input.dimension_mappings)?)
        .bind(input.is_enabled)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await;
        if let Err(err) = result {
            return Ok(AdminBillingMutationOutcome::Invalid(format!(
                "Integrity error: {err}"
            )));
        }
        match find_admin_billing_rule_mysql(&self.pool, &id).await? {
            Some(record) => Ok(AdminBillingMutationOutcome::Applied(record)),
            None => Err(DataLayerError::UnexpectedValue(
                "created billing rule missing".to_string(),
            )),
        }
    }

    async fn list_admin_billing_rules(
        &self,
        task_type: Option<&str>,
        is_enabled: Option<bool>,
        page: u32,
        page_size: u32,
    ) -> Result<Option<(Vec<AdminBillingRuleRecord>, u64)>, DataLayerError> {
        let total_row = sqlx::query(
            r#"
SELECT COUNT(*) AS total
FROM billing_rules
WHERE (? IS NULL OR task_type = ?)
  AND (? IS NULL OR is_enabled = ?)
            "#,
        )
        .bind(task_type)
        .bind(task_type)
        .bind(is_enabled)
        .bind(is_enabled)
        .fetch_one(&self.pool)
        .await
        .map_sql_err()?;
        let total = read_count_mysql(&total_row)?;
        let offset = u64::from(page.saturating_sub(1) * page_size);
        let rows = sqlx::query(
            r#"
SELECT
  id, name, task_type, global_model_id, model_id, expression, variables,
  dimension_mappings, is_enabled, created_at AS created_at_unix_ms,
  updated_at AS updated_at_unix_secs
FROM billing_rules
WHERE (? IS NULL OR task_type = ?)
  AND (? IS NULL OR is_enabled = ?)
ORDER BY updated_at DESC, id DESC
LIMIT ? OFFSET ?
            "#,
        )
        .bind(task_type)
        .bind(task_type)
        .bind(is_enabled)
        .bind(is_enabled)
        .bind(i64::from(page_size))
        .bind(
            i64::try_from(offset)
                .map_err(|err| DataLayerError::UnexpectedValue(err.to_string()))?,
        )
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        let items = rows
            .iter()
            .map(map_admin_billing_rule_mysql)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some((items, total)))
    }

    async fn find_admin_billing_rule(
        &self,
        rule_id: &str,
    ) -> Result<Option<AdminBillingRuleRecord>, DataLayerError> {
        find_admin_billing_rule_mysql(&self.pool, rule_id).await
    }

    async fn update_admin_billing_rule(
        &self,
        rule_id: &str,
        input: &AdminBillingRuleWriteInput,
    ) -> Result<AdminBillingMutationOutcome<AdminBillingRuleRecord>, DataLayerError> {
        let result = sqlx::query(
            r#"
UPDATE billing_rules
SET name = ?,
    task_type = ?,
    global_model_id = ?,
    model_id = ?,
    expression = ?,
    variables = ?,
    dimension_mappings = ?,
    is_enabled = ?,
    updated_at = ?
WHERE id = ?
            "#,
        )
        .bind(&input.name)
        .bind(&input.task_type)
        .bind(input.global_model_id.as_deref())
        .bind(input.model_id.as_deref())
        .bind(&input.expression)
        .bind(json_to_string(&input.variables)?)
        .bind(json_to_string(&input.dimension_mappings)?)
        .bind(input.is_enabled)
        .bind(current_unix_secs_i64())
        .bind(rule_id)
        .execute(&self.pool)
        .await;
        let affected = match result {
            Ok(result) => result.rows_affected(),
            Err(err) => {
                return Ok(AdminBillingMutationOutcome::Invalid(format!(
                    "Integrity error: {err}"
                )))
            }
        };
        if affected == 0 {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        match find_admin_billing_rule_mysql(&self.pool, rule_id).await? {
            Some(record) => Ok(AdminBillingMutationOutcome::Applied(record)),
            None => Ok(AdminBillingMutationOutcome::NotFound),
        }
    }

    async fn create_admin_billing_collector(
        &self,
        input: &AdminBillingCollectorWriteInput,
    ) -> Result<AdminBillingMutationOutcome<AdminBillingCollectorRecord>, DataLayerError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = current_unix_secs_i64();
        let result = sqlx::query(
            r#"
INSERT INTO dimension_collectors (
  id, api_format, task_type, dimension_name, source_type, source_path, value_type,
  transform_expression, default_value, priority, is_enabled, created_at, updated_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&input.api_format)
        .bind(&input.task_type)
        .bind(&input.dimension_name)
        .bind(&input.source_type)
        .bind(input.source_path.as_deref())
        .bind(&input.value_type)
        .bind(input.transform_expression.as_deref())
        .bind(input.default_value.as_deref())
        .bind(input.priority)
        .bind(input.is_enabled)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await;
        if let Err(err) = result {
            return Ok(AdminBillingMutationOutcome::Invalid(format!(
                "Integrity error: {err}"
            )));
        }
        match find_admin_billing_collector_mysql(&self.pool, &id).await? {
            Some(record) => Ok(AdminBillingMutationOutcome::Applied(record)),
            None => Err(DataLayerError::UnexpectedValue(
                "created billing collector missing".to_string(),
            )),
        }
    }

    async fn list_admin_billing_collectors(
        &self,
        api_format: Option<&str>,
        task_type: Option<&str>,
        dimension_name: Option<&str>,
        is_enabled: Option<bool>,
        page: u32,
        page_size: u32,
    ) -> Result<Option<(Vec<AdminBillingCollectorRecord>, u64)>, DataLayerError> {
        let total_row = sqlx::query(
            r#"
SELECT COUNT(*) AS total
FROM dimension_collectors
WHERE (? IS NULL OR api_format = ?)
  AND (? IS NULL OR task_type = ?)
  AND (? IS NULL OR dimension_name = ?)
  AND (? IS NULL OR is_enabled = ?)
            "#,
        )
        .bind(api_format)
        .bind(api_format)
        .bind(task_type)
        .bind(task_type)
        .bind(dimension_name)
        .bind(dimension_name)
        .bind(is_enabled)
        .bind(is_enabled)
        .fetch_one(&self.pool)
        .await
        .map_sql_err()?;
        let total = read_count_mysql(&total_row)?;
        let offset = u64::from(page.saturating_sub(1) * page_size);
        let rows = sqlx::query(
            r#"
SELECT
  id, api_format, task_type, dimension_name, source_type, source_path, value_type,
  transform_expression, default_value, priority, is_enabled,
  created_at AS created_at_unix_ms, updated_at AS updated_at_unix_secs
FROM dimension_collectors
WHERE (? IS NULL OR api_format = ?)
  AND (? IS NULL OR task_type = ?)
  AND (? IS NULL OR dimension_name = ?)
  AND (? IS NULL OR is_enabled = ?)
ORDER BY updated_at DESC, priority DESC, id ASC
LIMIT ? OFFSET ?
            "#,
        )
        .bind(api_format)
        .bind(api_format)
        .bind(task_type)
        .bind(task_type)
        .bind(dimension_name)
        .bind(dimension_name)
        .bind(is_enabled)
        .bind(is_enabled)
        .bind(i64::from(page_size))
        .bind(
            i64::try_from(offset)
                .map_err(|err| DataLayerError::UnexpectedValue(err.to_string()))?,
        )
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        let items = rows
            .iter()
            .map(map_admin_billing_collector_mysql)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some((items, total)))
    }

    async fn find_admin_billing_collector(
        &self,
        collector_id: &str,
    ) -> Result<Option<AdminBillingCollectorRecord>, DataLayerError> {
        find_admin_billing_collector_mysql(&self.pool, collector_id).await
    }

    async fn update_admin_billing_collector(
        &self,
        collector_id: &str,
        input: &AdminBillingCollectorWriteInput,
    ) -> Result<AdminBillingMutationOutcome<AdminBillingCollectorRecord>, DataLayerError> {
        let result = sqlx::query(
            r#"
UPDATE dimension_collectors
SET api_format = ?,
    task_type = ?,
    dimension_name = ?,
    source_type = ?,
    source_path = ?,
    value_type = ?,
    transform_expression = ?,
    default_value = ?,
    priority = ?,
    is_enabled = ?,
    updated_at = ?
WHERE id = ?
            "#,
        )
        .bind(&input.api_format)
        .bind(&input.task_type)
        .bind(&input.dimension_name)
        .bind(&input.source_type)
        .bind(input.source_path.as_deref())
        .bind(&input.value_type)
        .bind(input.transform_expression.as_deref())
        .bind(input.default_value.as_deref())
        .bind(input.priority)
        .bind(input.is_enabled)
        .bind(current_unix_secs_i64())
        .bind(collector_id)
        .execute(&self.pool)
        .await;
        let affected = match result {
            Ok(result) => result.rows_affected(),
            Err(err) => {
                return Ok(AdminBillingMutationOutcome::Invalid(format!(
                    "Integrity error: {err}"
                )))
            }
        };
        if affected == 0 {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        match find_admin_billing_collector_mysql(&self.pool, collector_id).await? {
            Some(record) => Ok(AdminBillingMutationOutcome::Applied(record)),
            None => Ok(AdminBillingMutationOutcome::NotFound),
        }
    }

    async fn apply_admin_billing_preset(
        &self,
        preset: &str,
        mode: &str,
        collectors: &[AdminBillingCollectorWriteInput],
    ) -> Result<AdminBillingMutationOutcome<AdminBillingPresetApplyResult>, DataLayerError> {
        let mut created = 0_u64;
        let mut updated = 0_u64;
        let mut skipped = 0_u64;
        let mut errors = Vec::new();

        for collector in collectors {
            let existing_id = match sqlx::query_scalar::<_, String>(
                r#"
SELECT id
FROM dimension_collectors
WHERE api_format = ?
  AND task_type = ?
  AND dimension_name = ?
  AND priority = ?
  AND is_enabled = 1
LIMIT 1
                "#,
            )
            .bind(&collector.api_format)
            .bind(&collector.task_type)
            .bind(&collector.dimension_name)
            .bind(collector.priority)
            .fetch_optional(&self.pool)
            .await
            {
                Ok(value) => value,
                Err(err) => {
                    errors.push(format!(
                        "Failed to query collector: api_format={} task_type={} dim={}: {}",
                        collector.api_format, collector.task_type, collector.dimension_name, err
                    ));
                    continue;
                }
            };

            if let Some(existing_id) = existing_id {
                if mode == "overwrite" {
                    match sqlx::query(
                        r#"
UPDATE dimension_collectors
SET source_type = ?,
    source_path = ?,
    value_type = ?,
    transform_expression = ?,
    default_value = ?,
    is_enabled = ?,
    updated_at = ?
WHERE id = ?
                        "#,
                    )
                    .bind(&collector.source_type)
                    .bind(collector.source_path.as_deref())
                    .bind(&collector.value_type)
                    .bind(collector.transform_expression.as_deref())
                    .bind(collector.default_value.as_deref())
                    .bind(collector.is_enabled)
                    .bind(current_unix_secs_i64())
                    .bind(&existing_id)
                    .execute(&self.pool)
                    .await
                    {
                        Ok(_) => updated += 1,
                        Err(err) => errors.push(format!(
                            "Failed to update collector {}: {}",
                            existing_id, err
                        )),
                    }
                } else {
                    skipped += 1;
                }
                continue;
            }

            let id = uuid::Uuid::new_v4().to_string();
            let now = current_unix_secs_i64();
            match sqlx::query(
                r#"
INSERT INTO dimension_collectors (
  id, api_format, task_type, dimension_name, source_type, source_path, value_type,
  transform_expression, default_value, priority, is_enabled, created_at, updated_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(id)
            .bind(&collector.api_format)
            .bind(&collector.task_type)
            .bind(&collector.dimension_name)
            .bind(&collector.source_type)
            .bind(collector.source_path.as_deref())
            .bind(&collector.value_type)
            .bind(collector.transform_expression.as_deref())
            .bind(collector.default_value.as_deref())
            .bind(collector.priority)
            .bind(collector.is_enabled)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            {
                Ok(_) => created += 1,
                Err(err) => errors.push(format!(
                    "Failed to create collector: api_format={} task_type={} dim={}: {}",
                    collector.api_format, collector.task_type, collector.dimension_name, err
                )),
            }
        }

        Ok(AdminBillingMutationOutcome::Applied(
            AdminBillingPresetApplyResult {
                preset: preset.to_string(),
                mode: mode.to_string(),
                created,
                updated,
                skipped,
                errors,
            },
        ))
    }

    async fn find_payment_gateway_config(
        &self,
        provider: &str,
    ) -> Result<Option<PaymentGatewayConfigRecord>, DataLayerError> {
        let row = sqlx::query(
            r#"
SELECT
  provider, enabled, endpoint_url, callback_base_url, merchant_id,
  merchant_key_encrypted, webhook_secret_encrypted, pay_currency, usd_exchange_rate, min_recharge_usd,
  channels_json, created_at AS created_at_unix_secs, updated_at AS updated_at_unix_secs
FROM payment_gateway_configs
WHERE provider = ?
LIMIT 1
            "#,
        )
        .bind(provider.trim().to_ascii_lowercase())
        .fetch_optional(&self.pool)
        .await
        .map_sql_err()?;
        row.as_ref()
            .map(map_payment_gateway_config_mysql)
            .transpose()
    }

    async fn upsert_payment_gateway_config(
        &self,
        input: &PaymentGatewayConfigWriteInput,
    ) -> Result<AdminBillingMutationOutcome<PaymentGatewayConfigRecord>, DataLayerError> {
        let provider = input.provider.trim().to_ascii_lowercase();
        let existing_secret = if input.preserve_existing_secret {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT merchant_key_encrypted FROM payment_gateway_configs WHERE provider = ?",
            )
            .bind(&provider)
            .fetch_optional(&self.pool)
            .await
            .map_sql_err()?
            .flatten()
        } else {
            None
        };
        let existing_webhook_secret = if input.preserve_existing_webhook_secret {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT webhook_secret_encrypted FROM payment_gateway_configs WHERE provider = ?",
            )
            .bind(&provider)
            .fetch_optional(&self.pool)
            .await
            .map_sql_err()?
            .flatten()
        } else {
            None
        };
        let secret = if input.preserve_existing_secret {
            existing_secret
        } else {
            input.merchant_key_encrypted.clone()
        };
        let webhook_secret = if input.preserve_existing_webhook_secret {
            existing_webhook_secret
        } else {
            input.webhook_secret_encrypted.clone()
        };
        let now = current_unix_secs_i64();
        sqlx::query(
            r#"
INSERT INTO payment_gateway_configs (
  provider, enabled, endpoint_url, callback_base_url, merchant_id,
  merchant_key_encrypted, webhook_secret_encrypted, pay_currency, usd_exchange_rate,
  min_recharge_usd, channels_json, created_at, updated_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON DUPLICATE KEY UPDATE
  enabled = VALUES(enabled),
  endpoint_url = VALUES(endpoint_url),
  callback_base_url = VALUES(callback_base_url),
  merchant_id = VALUES(merchant_id),
  merchant_key_encrypted = VALUES(merchant_key_encrypted),
  webhook_secret_encrypted = VALUES(webhook_secret_encrypted),
  pay_currency = VALUES(pay_currency),
  usd_exchange_rate = VALUES(usd_exchange_rate),
  min_recharge_usd = VALUES(min_recharge_usd),
  channels_json = VALUES(channels_json),
  updated_at = VALUES(updated_at)
            "#,
        )
        .bind(&provider)
        .bind(input.enabled)
        .bind(&input.endpoint_url)
        .bind(input.callback_base_url.as_deref())
        .bind(&input.merchant_id)
        .bind(secret.as_deref())
        .bind(webhook_secret.as_deref())
        .bind(&input.pay_currency)
        .bind(input.usd_exchange_rate)
        .bind(input.min_recharge_usd)
        .bind(json_to_string(&input.channels_json)?)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_sql_err()?;
        match self.find_payment_gateway_config(&provider).await? {
            Some(record) => Ok(AdminBillingMutationOutcome::Applied(record)),
            None => Err(DataLayerError::UnexpectedValue(
                "upserted payment gateway config missing".to_string(),
            )),
        }
    }

    async fn list_billing_plans(
        &self,
        include_disabled: bool,
    ) -> Result<Option<Vec<BillingPlanRecord>>, DataLayerError> {
        let rows = sqlx::query(
            r#"
SELECT
  id, title, description, price_amount, price_currency, duration_unit,
  duration_value, enabled, sort_order, max_active_per_user, purchase_limit_scope,
  entitlements_json,
  created_at AS created_at_unix_secs, updated_at AS updated_at_unix_secs
FROM billing_plans
WHERE (? = TRUE OR enabled = TRUE)
ORDER BY sort_order ASC, price_amount ASC, id ASC
            "#,
        )
        .bind(include_disabled)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        let mut plans = rows
            .iter()
            .map(map_billing_plan_mysql)
            .collect::<Result<Vec<_>, _>>()?;
        let mut scopes = self.load_all_plan_provider_ids().await?;
        for plan in &mut plans {
            plan.allowed_provider_ids = scopes.remove(&plan.id).unwrap_or_default();
        }
        Ok(Some(plans))
    }

    async fn find_billing_plan(
        &self,
        plan_id: &str,
    ) -> Result<Option<BillingPlanRecord>, DataLayerError> {
        let row = sqlx::query(
            r#"
SELECT
  id, title, description, price_amount, price_currency, duration_unit,
  duration_value, enabled, sort_order, max_active_per_user, purchase_limit_scope,
  entitlements_json,
  created_at AS created_at_unix_secs, updated_at AS updated_at_unix_secs
FROM billing_plans
WHERE id = ?
LIMIT 1
            "#,
        )
        .bind(plan_id)
        .fetch_optional(&self.pool)
        .await
        .map_sql_err()?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut plan = map_billing_plan_mysql(&row)?;
        plan.allowed_provider_ids = self.load_plan_provider_ids(plan_id).await?;
        Ok(Some(plan))
    }

    async fn create_billing_plan(
        &self,
        input: &BillingPlanWriteInput,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, DataLayerError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = current_unix_secs_i64();
        let mut transaction = self.pool.begin().await.map_sql_err()?;
        sqlx::query(BILLING_PLAN_INSERT_MYSQL)
            .bind(&id)
            .bind(&input.title)
            .bind(input.description.as_deref())
            .bind(input.price_amount)
            .bind(&input.price_currency)
            .bind(&input.duration_unit)
            .bind(input.duration_value)
            .bind(input.enabled)
            .bind(input.sort_order)
            .bind(input.max_active_per_user)
            .bind(&input.purchase_limit_scope)
            .bind(json_to_string(&input.entitlements_json)?)
            .bind(now)
            .bind(now)
            .execute(&mut *transaction)
            .await
            .map_sql_err()?;
        replace_plan_provider_ids_mysql(&mut transaction, &id, &input.allowed_provider_ids, now)
            .await?;
        transaction.commit().await.map_sql_err()?;
        match self.find_billing_plan(&id).await? {
            Some(record) => Ok(AdminBillingMutationOutcome::Applied(record)),
            None => Err(DataLayerError::UnexpectedValue(
                "created billing plan missing".to_string(),
            )),
        }
    }

    async fn update_billing_plan(
        &self,
        plan_id: &str,
        input: &BillingPlanWriteInput,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, DataLayerError> {
        let now = current_unix_secs_i64();
        let mut transaction = self.pool.begin().await.map_sql_err()?;
        let result = sqlx::query(BILLING_PLAN_UPDATE_MYSQL)
            .bind(&input.title)
            .bind(input.description.as_deref())
            .bind(input.price_amount)
            .bind(&input.price_currency)
            .bind(&input.duration_unit)
            .bind(input.duration_value)
            .bind(input.enabled)
            .bind(input.sort_order)
            .bind(input.max_active_per_user)
            .bind(&input.purchase_limit_scope)
            .bind(json_to_string(&input.entitlements_json)?)
            .bind(now)
            .bind(plan_id)
            .execute(&mut *transaction)
            .await
            .map_sql_err()?;
        if result.rows_affected() == 0 {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        replace_plan_provider_ids_mysql(
            &mut transaction,
            plan_id,
            &input.allowed_provider_ids,
            now,
        )
        .await?;
        transaction.commit().await.map_sql_err()?;
        match self.find_billing_plan(plan_id).await? {
            Some(record) => Ok(AdminBillingMutationOutcome::Applied(record)),
            None => Ok(AdminBillingMutationOutcome::NotFound),
        }
    }

    async fn set_billing_plan_enabled(
        &self,
        plan_id: &str,
        enabled: bool,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, DataLayerError> {
        let result =
            sqlx::query("UPDATE billing_plans SET enabled = ?, updated_at = ? WHERE id = ?")
                .bind(enabled)
                .bind(current_unix_secs_i64())
                .bind(plan_id)
                .execute(&self.pool)
                .await
                .map_sql_err()?;
        if result.rows_affected() == 0 {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        match self.find_billing_plan(plan_id).await? {
            Some(record) => Ok(AdminBillingMutationOutcome::Applied(record)),
            None => Ok(AdminBillingMutationOutcome::NotFound),
        }
    }

    async fn delete_billing_plan(
        &self,
        plan_id: &str,
    ) -> Result<AdminBillingMutationOutcome<()>, DataLayerError> {
        let exists =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM billing_plans WHERE id = ?")
                .bind(plan_id)
                .fetch_one(&self.pool)
                .await
                .map_sql_err()?;
        if exists == 0 {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }

        let order_count = sqlx::query_scalar::<_, i64>(
            r#"
SELECT COUNT(*)
FROM payment_orders
WHERE product_id = ?
  AND order_kind = 'plan_purchase'
            "#,
        )
        .bind(plan_id)
        .fetch_one(&self.pool)
        .await
        .map_sql_err()?;
        let entitlement_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_plan_entitlements WHERE plan_id = ?",
        )
        .bind(plan_id)
        .fetch_one(&self.pool)
        .await
        .map_sql_err()?;
        if order_count > 0 || entitlement_count > 0 {
            return Ok(AdminBillingMutationOutcome::Invalid(
                "套餐已有订单或权益，不能删除，请停用该套餐".to_string(),
            ));
        }

        let result = sqlx::query("DELETE FROM billing_plans WHERE id = ?")
            .bind(plan_id)
            .execute(&self.pool)
            .await
            .map_sql_err()?;
        if result.rows_affected() == 0 {
            Ok(AdminBillingMutationOutcome::NotFound)
        } else {
            Ok(AdminBillingMutationOutcome::Applied(()))
        }
    }

    async fn list_user_plan_entitlements(
        &self,
        user_id: &str,
    ) -> Result<Option<Vec<UserPlanEntitlementRecord>>, DataLayerError> {
        let rows = sqlx::query(
            r#"
SELECT
  id, user_id, plan_id, payment_order_id, status,
  starts_at AS starts_at_unix_secs, expires_at AS expires_at_unix_secs,
  entitlements_snapshot,
  COALESCE((
    SELECT JSON_ARRAYAGG(bpp.provider_id)
    FROM billing_plan_providers bpp
    WHERE bpp.plan_id = user_plan_entitlements.plan_id
  ), JSON_ARRAY()) AS allowed_provider_ids,
  created_at AS created_at_unix_secs,
  updated_at AS updated_at_unix_secs
FROM user_plan_entitlements
WHERE user_id = ?
ORDER BY created_at DESC, expires_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        let entitlements = rows
            .iter()
            .map(map_user_plan_entitlement_mysql)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(entitlements))
    }

    async fn cancel_user_plan_entitlement(
        &self,
        user_id: &str,
        entitlement_id: &str,
    ) -> Result<AdminBillingMutationOutcome<UserPlanEntitlementRecord>, DataLayerError> {
        let now = current_unix_secs_i64();
        let result = sqlx::query(
            r#"
UPDATE user_plan_entitlements
SET status = 'cancelled', expires_at = LEAST(expires_at, ?), updated_at = ?
WHERE id = ?
  AND user_id = ?
  AND status = 'active'
  AND expires_at > ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(entitlement_id)
        .bind(user_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_sql_err()?;
        if result.rows_affected() == 0 {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        let row = sqlx::query(
            r#"
SELECT
  id, user_id, plan_id, payment_order_id, status,
  starts_at AS starts_at_unix_secs, expires_at AS expires_at_unix_secs,
  entitlements_snapshot,
  COALESCE((
    SELECT JSON_ARRAYAGG(bpp.provider_id)
    FROM billing_plan_providers bpp
    WHERE bpp.plan_id = user_plan_entitlements.plan_id
  ), JSON_ARRAY()) AS allowed_provider_ids,
  created_at AS created_at_unix_secs,
  updated_at AS updated_at_unix_secs
FROM user_plan_entitlements
WHERE id = ? AND user_id = ?
            "#,
        )
        .bind(entitlement_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_sql_err()?;
        match row {
            Some(row) => {
                let entitlement = map_user_plan_entitlement_mysql(&row)?;
                Ok(AdminBillingMutationOutcome::Applied(entitlement))
            }
            None => Ok(AdminBillingMutationOutcome::NotFound),
        }
    }

    async fn update_user_plan_entitlement(
        &self,
        user_id: &str,
        entitlement_id: &str,
        input: &UserPlanEntitlementUpdateInput,
    ) -> Result<AdminBillingMutationOutcome<UserPlanEntitlementRecord>, DataLayerError> {
        if input.starts_at_unix_secs >= input.expires_at_unix_secs {
            return Ok(AdminBillingMutationOutcome::Invalid(
                "套餐开始时间必须早于结束时间".to_string(),
            ));
        }
        let now = current_unix_secs_i64();
        let entitlements_snapshot = input
            .entitlements_snapshot
            .as_ref()
            .map(json_to_string)
            .transpose()?;
        let mut transaction = self.pool.begin().await.map_sql_err()?;
        sqlx::query("SELECT id FROM wallets WHERE user_id = ? LIMIT 1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_sql_err()?;
        let target_exists = sqlx::query_scalar::<_, String>(
            r#"
SELECT id
FROM user_plan_entitlements
WHERE id = ? AND user_id = ? AND status = 'active' AND expires_at > ?
LIMIT 1
FOR UPDATE
            "#,
        )
        .bind(entitlement_id)
        .bind(user_id)
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_sql_err()?
        .is_some();
        if !target_exists {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        let overlap = sqlx::query_scalar::<_, i64>(
            r#"
SELECT COUNT(*)
FROM user_plan_entitlements
WHERE user_id = ?
  AND id <> ?
  AND status = 'active'
  AND starts_at < ?
  AND expires_at > ?
            "#,
        )
        .bind(user_id)
        .bind(entitlement_id)
        .bind(input.expires_at_unix_secs as i64)
        .bind(input.starts_at_unix_secs as i64)
        .fetch_one(&mut *transaction)
        .await
        .map_sql_err()?;
        if overlap > 0 {
            return Ok(AdminBillingMutationOutcome::Invalid(
                "同一时间只能有一个生效套餐".to_string(),
            ));
        }
        let result = sqlx::query(
            r#"
UPDATE user_plan_entitlements
SET starts_at = ?, expires_at = ?, entitlements_snapshot = COALESCE(?, entitlements_snapshot), updated_at = ?
WHERE id = ?
  AND user_id = ?
  AND status = 'active'
  AND expires_at > ?
            "#,
        )
        .bind(input.starts_at_unix_secs as i64)
        .bind(input.expires_at_unix_secs as i64)
        .bind(entitlements_snapshot)
        .bind(now)
        .bind(entitlement_id)
        .bind(user_id)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_sql_err()?;
        if result.rows_affected() == 0 {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        transaction.commit().await.map_sql_err()?;
        let entitlements = self
            .list_user_plan_entitlements(user_id)
            .await?
            .unwrap_or_default();
        entitlements
            .into_iter()
            .find(|entitlement| entitlement.id == entitlement_id)
            .map(AdminBillingMutationOutcome::Applied)
            .ok_or_else(|| {
                DataLayerError::UnexpectedValue("updated user entitlement missing".to_string())
            })
    }

    async fn find_user_daily_quota_availability(
        &self,
        user_id: &str,
    ) -> Result<Option<UserDailyQuotaAvailabilityRecord>, DataLayerError> {
        self.find_user_daily_quota_availability_for_global_model(user_id, None)
            .await
    }

    async fn list_active_user_plan_quota_summaries(
        &self,
        user_ids: &[String],
    ) -> Result<Option<Vec<UserPlanQuotaSummaryRecord>>, DataLayerError> {
        if user_ids.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let now_unix_secs = current_unix_secs_i64();
        let mut entitlements_query = QueryBuilder::<MySql>::new(
            r#"
SELECT
  upe.user_id,
  upe.id AS entitlement_id,
  upe.plan_id,
  bp.title AS plan_title,
  upe.starts_at,
  upe.expires_at,
  upe.entitlements_snapshot
FROM user_plan_entitlements upe
JOIN billing_plans bp ON bp.id = upe.plan_id
WHERE upe.user_id IN (
            "#,
        );
        {
            let mut separated = entitlements_query.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        entitlements_query
            .push(") AND upe.status = 'active' AND upe.starts_at <= ")
            .push_bind(now_unix_secs)
            .push(" AND upe.expires_at > ")
            .push_bind(now_unix_secs)
            .push(" ORDER BY upe.user_id ASC, upe.expires_at ASC, upe.created_at ASC, upe.id ASC");
        let entitlement_rows = entitlements_query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_sql_err()?;
        let entitlement_ids = entitlement_rows
            .iter()
            .map(|row| row.try_get::<String, _>("entitlement_id"))
            .collect::<Result<Vec<_>, _>>()
            .map_sql_err()?;
        if entitlement_ids.is_empty() {
            return Ok(Some(Vec::new()));
        }

        let mut usage_query = QueryBuilder::<MySql>::new(
            r#"
SELECT
  user_entitlement_id,
  window_scope,
  window_key,
  used_usd,
  window_started_at,
  window_ends_at
FROM entitlement_usage_windows
WHERE user_entitlement_id IN (
            "#,
        );
        {
            let mut separated = usage_query.separated(", ");
            for entitlement_id in &entitlement_ids {
                separated.push_bind(entitlement_id);
            }
        }
        usage_query.push(")");
        let usage_rows = usage_query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_sql_err()?;
        let mut usage_by_entitlement = BTreeMap::<String, BTreeMap<(String, String), f64>>::new();
        let mut stored_windows_by_entitlement = BTreeMap::<String, StoredUsageQuotaWindows>::new();
        for row in usage_rows {
            let entitlement_id: String = row.try_get("user_entitlement_id").map_sql_err()?;
            let scope: String = row.try_get("window_scope").map_sql_err()?;
            let window_key: String = row.try_get("window_key").map_sql_err()?;
            let used_usd: f64 = row.try_get("used_usd").map_sql_err()?;
            usage_by_entitlement
                .entry(entitlement_id.clone())
                .or_default()
                .insert((scope.clone(), window_key.clone()), used_usd);
            let started_at_unix_secs: i64 = row.try_get("window_started_at").map_sql_err()?;
            let ends_at_unix_secs: i64 = row.try_get("window_ends_at").map_sql_err()?;
            if let (Some(window_started_at), Some(window_ends_at)) = (
                chrono::DateTime::from_timestamp(started_at_unix_secs, 0),
                chrono::DateTime::from_timestamp(ends_at_unix_secs, 0),
            ) {
                stored_windows_by_entitlement
                    .entry(entitlement_id)
                    .or_default()
                    .insert(
                        scope,
                        StoredUsageQuotaWindow {
                            window_key,
                            window_started_at,
                            window_ends_at,
                        },
                    );
            }
        }

        let now = chrono::DateTime::from_timestamp(now_unix_secs, 0).ok_or_else(|| {
            DataLayerError::UnexpectedValue("invalid current timestamp".to_string())
        })?;
        let mut loaded_summaries = Vec::with_capacity(entitlement_rows.len());
        for row in entitlement_rows {
            let user_id: String = row.try_get("user_id").map_sql_err()?;
            let entitlement_id: String = row.try_get("entitlement_id").map_sql_err()?;
            let plan_id: String = row.try_get("plan_id").map_sql_err()?;
            let plan_title: String = row.try_get("plan_title").map_sql_err()?;
            let starts_at_unix_secs: i64 = row.try_get("starts_at").map_sql_err()?;
            let expires_at_unix_secs: i64 = row.try_get("expires_at").map_sql_err()?;
            let starts_at =
                chrono::DateTime::from_timestamp(starts_at_unix_secs, 0).ok_or_else(|| {
                    DataLayerError::UnexpectedValue("invalid entitlement start".to_string())
                })?;
            let entitlements = parse_json(
                row.try_get::<Option<String>, _>("entitlements_snapshot")
                    .map_sql_err()?,
            )?
            .unwrap_or_else(|| serde_json::json!([]));
            let grants = usage_quota_grants_from_entitlement(
                &entitlement_id,
                &entitlements,
                now,
                starts_at,
                stored_windows_by_entitlement.get(&entitlement_id),
            )?;
            loaded_summaries.push(LoadedUserPlanQuotaSummary {
                user_id,
                usage_by_window: usage_by_entitlement
                    .remove(&entitlement_id)
                    .unwrap_or_default(),
                entitlement_id,
                plan_id,
                plan_title,
                starts_at_unix_secs: starts_at_unix_secs.max(0) as u64,
                expires_at_unix_secs: expires_at_unix_secs.max(0) as u64,
                grants,
            });
        }
        let summaries = loaded_summaries
            .into_iter()
            .map(LoadedUserPlanQuotaSummary::into_record)
            .collect();
        Ok(Some(summaries))
    }

    async fn find_user_daily_quota_availability_for_global_model(
        &self,
        user_id: &str,
        global_model_id: Option<&str>,
    ) -> Result<Option<UserDailyQuotaAvailabilityRecord>, DataLayerError> {
        let now_unix_secs = current_unix_secs_i64();
        let rows = sqlx::query(
            r#"
SELECT
  upe.id,
  upe.starts_at,
  upe.entitlements_snapshot,
  COALESCE((
    SELECT JSON_ARRAYAGG(bpp.provider_id)
    FROM billing_plan_providers bpp
    JOIN providers p ON p.id = bpp.provider_id AND p.is_active = 1
    WHERE bpp.plan_id = upe.plan_id
      AND (
        ? IS NULL OR EXISTS (
          SELECT 1
          FROM models m
          JOIN global_models gm ON gm.id = m.global_model_id AND gm.is_active = 1
          WHERE m.provider_id = bpp.provider_id
            AND m.global_model_id = ?
            AND m.is_active = 1
            AND COALESCE(m.is_available, 1) = 1
        )
      )
  ), JSON_ARRAY()) AS provider_ids
FROM user_plan_entitlements upe
WHERE upe.user_id = ?
  AND upe.status = 'active'
  AND upe.starts_at <= ?
  AND upe.expires_at > ?
ORDER BY upe.expires_at ASC, upe.created_at ASC, upe.id ASC
            "#,
        )
        .bind(global_model_id)
        .bind(global_model_id)
        .bind(user_id)
        .bind(now_unix_secs)
        .bind(now_unix_secs)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        let now = chrono::Utc::now();
        let mut grants = Vec::new();
        let mut provider_ids_by_entitlement = BTreeMap::<String, Vec<String>>::new();
        for row in rows {
            let entitlement_id: String = row.try_get("id").map_sql_err()?;
            let entitlement_started_at = chrono::DateTime::from_timestamp(
                row.try_get::<i64, _>("starts_at").map_sql_err()?,
                0,
            )
            .ok_or_else(|| {
                DataLayerError::UnexpectedValue("invalid entitlement start".to_string())
            })?;
            let entitlements = parse_json(row.try_get("entitlements_snapshot").ok().flatten())?
                .unwrap_or_else(|| serde_json::json!([]));
            let provider_ids = parse_string_vec_json(
                row.try_get::<Option<String>, _>("provider_ids")
                    .map_sql_err()?,
            )?;
            provider_ids_by_entitlement.insert(entitlement_id.clone(), provider_ids);
            if provider_ids_by_entitlement
                .get(&entitlement_id)
                .is_none_or(Vec::is_empty)
            {
                continue;
            }
            let stored_windows =
                find_usage_quota_windows_mysql(&self.pool, &entitlement_id).await?;
            grants.extend(usage_quota_grants_from_entitlement(
                &entitlement_id,
                &entitlements,
                now,
                entitlement_started_at,
                Some(&stored_windows),
            )?);
        }
        let mut grouped_limits: BTreeMap<String, (Option<f64>, Option<f64>, f64)> = BTreeMap::new();
        let mut allow_wallet_overage = true;
        for grant in &grants {
            allow_wallet_overage &= grant.allow_wallet_overage;
            let used = sqlx::query_scalar::<_, f64>(
                r#"
SELECT used_usd
FROM entitlement_usage_windows
WHERE user_entitlement_id = ?
  AND window_scope = ?
  AND window_key = ?
LIMIT 1
                "#,
            )
            .bind(&grant.entitlement_id)
            .bind(grant.scope)
            .bind(&grant.window_key)
            .fetch_optional(&self.pool)
            .await
            .map_sql_err()?
            .unwrap_or(0.0);
            let remaining = (grant.limit_usd - used).max(0.0);
            let entry = grouped_limits
                .entry(grant.entitlement_id.clone())
                .or_insert((None, None, grant.quota_multiplier));
            entry.0 = Some(
                entry
                    .0
                    .map_or(grant.limit_usd, |value| value.min(grant.limit_usd)),
            );
            entry.1 = Some(entry.1.map_or(remaining, |value| value.min(remaining)));
        }
        let mut total_quota_usd = 0.0;
        let mut used_usd = 0.0;
        let mut remaining_usd = 0.0;
        let mut base_remaining_usd = 0.0;
        let mut eligible_entitlement_ids = Vec::new();
        let mut allowed_provider_ids = BTreeSet::new();
        let mut eligible_provider_ids_by_entitlement = BTreeMap::new();
        for (entitlement_id, (limit, remaining, quota_multiplier)) in &grouped_limits {
            let limit = limit.unwrap_or(0.0);
            let remaining = remaining.unwrap_or(0.0);
            total_quota_usd += limit;
            used_usd += (limit - remaining).max(0.0);
            remaining_usd += remaining;
            base_remaining_usd += super::quota::quota_base_amount(remaining, *quota_multiplier);
            if remaining > 0.000_000_01 {
                eligible_entitlement_ids.push(entitlement_id.clone());
                if let Some(provider_ids) = provider_ids_by_entitlement.get(entitlement_id) {
                    allowed_provider_ids.extend(provider_ids.iter().cloned());
                    eligible_provider_ids_by_entitlement
                        .insert(entitlement_id.clone(), provider_ids.clone());
                }
            }
        }
        let has_active_daily_quota = !grants.is_empty();
        Ok(Some(UserDailyQuotaAvailabilityRecord {
            has_active_daily_quota,
            total_quota_usd,
            used_usd,
            remaining_usd,
            base_remaining_usd,
            allow_wallet_overage: has_active_daily_quota && allow_wallet_overage,
            eligible_entitlement_ids,
            allowed_provider_ids: allowed_provider_ids.into_iter().collect(),
            provider_ids_by_entitlement: eligible_provider_ids_by_entitlement,
        }))
    }
}

const BILLING_PLAN_INSERT_MYSQL: &str = r#"
INSERT INTO billing_plans (
  id, title, description, price_amount, price_currency, duration_unit,
  duration_value, enabled, sort_order, max_active_per_user, purchase_limit_scope,
  entitlements_json,
  created_at, updated_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const BILLING_PLAN_UPDATE_MYSQL: &str = r#"
UPDATE billing_plans
SET title = ?,
    description = ?,
    price_amount = ?,
    price_currency = ?,
    duration_unit = ?,
    duration_value = ?,
    enabled = ?,
    sort_order = ?,
    max_active_per_user = ?,
    purchase_limit_scope = ?,
    entitlements_json = ?,
    updated_at = ?
WHERE id = ?
"#;

struct RankedContext {
    rank: u8,
    is_available: bool,
    pricing_rank: u8,
    created_at: i64,
    context: Result<StoredBillingModelContext, DataLayerError>,
}

fn match_rank(
    row: &MySqlRow,
    requested_model: &str,
) -> Result<Option<RankedContext>, DataLayerError> {
    let provider_model_name: Option<String> =
        row.try_get("model_provider_model_name").map_sql_err()?;
    let global_model_name: String = row.try_get("global_model_name").map_sql_err()?;
    let mappings: Option<String> = row.try_get("provider_model_mappings").ok().flatten();

    let rank = if provider_model_name.as_deref() == Some(requested_model) {
        0
    } else if mappings
        .as_deref()
        .is_some_and(|mappings| provider_model_mappings_match(mappings, requested_model))
    {
        1
    } else if global_model_name == requested_model {
        2
    } else {
        return Ok(None);
    };

    let has_model_price = row
        .try_get::<Option<f64>, _>("model_price_per_request")
        .map_sql_err()?
        .is_some()
        || row
            .try_get::<Option<String>, _>("model_tiered_pricing")
            .ok()
            .flatten()
            .is_some();
    let has_default_price = row
        .try_get::<Option<f64>, _>("default_price_per_request")
        .map_sql_err()?
        .is_some()
        || row
            .try_get::<Option<String>, _>("default_tiered_pricing")
            .ok()
            .flatten()
            .is_some();
    let pricing_rank = if has_model_price {
        0
    } else if has_default_price {
        1
    } else {
        2
    };

    Ok(Some(RankedContext {
        rank,
        is_available: row
            .try_get::<Option<bool>, _>("model_is_available")
            .map_sql_err()?
            .unwrap_or(false),
        pricing_rank,
        created_at: row
            .try_get::<Option<i64>, _>("model_created_at")
            .map_sql_err()?
            .unwrap_or(i64::MAX),
        context: map_row(row),
    }))
}

fn provider_model_mappings_match(raw: &str, requested_model: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return raw == requested_model;
    };
    json_mapping_matches(&value, requested_model)
}

fn json_mapping_matches(value: &serde_json::Value, requested_model: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value == requested_model,
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| json_mapping_matches(value, requested_model)),
        serde_json::Value::Object(map) => map
            .get("name")
            .is_some_and(|value| json_mapping_matches(value, requested_model)),
        _ => false,
    }
}

fn map_row(row: &MySqlRow) -> Result<StoredBillingModelContext, DataLayerError> {
    StoredBillingModelContext::new(
        row.try_get("provider_id").map_sql_err()?,
        row.try_get("provider_billing_type").map_sql_err()?,
        parse_json(row.try_get("provider_config").ok().flatten())?,
        row.try_get("provider_api_key_id").map_sql_err()?,
        parse_json(
            row.try_get("provider_api_key_rate_multipliers")
                .ok()
                .flatten(),
        )?,
        row.try_get::<Option<i64>, _>("provider_api_key_cache_ttl_minutes")
            .map_sql_err()?,
        row.try_get("global_model_id").map_sql_err()?,
        row.try_get("global_model_name").map_sql_err()?,
        parse_json(row.try_get("global_model_config").ok().flatten())?,
        row.try_get("default_price_per_request").map_sql_err()?,
        parse_json(row.try_get("default_tiered_pricing").ok().flatten())?,
        row.try_get("model_id").map_sql_err()?,
        row.try_get("model_provider_model_name").map_sql_err()?,
        parse_json(row.try_get("model_config").ok().flatten())?,
        row.try_get("model_price_per_request").map_sql_err()?,
        parse_json(row.try_get("model_tiered_pricing").ok().flatten())?,
    )
}

fn parse_json(value: Option<String>) -> Result<Option<serde_json::Value>, DataLayerError> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            serde_json::from_str(&value).map_err(|err| {
                DataLayerError::UnexpectedValue(format!("billing JSON field is invalid: {err}"))
            })
        })
        .transpose()
}

fn parse_string_vec_json(value: Option<String>) -> Result<Vec<String>, DataLayerError> {
    let mut values = parse_json(value)?
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::trim).map(ToOwned::to_owned))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    Ok(values)
}

fn current_unix_secs_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn json_to_string(value: &serde_json::Value) -> Result<String, DataLayerError> {
    serde_json::to_string(value).map_err(|err| {
        DataLayerError::UnexpectedValue(format!("billing JSON encode failed: {err}"))
    })
}

async fn find_usage_quota_windows_mysql(
    pool: &MysqlPool,
    entitlement_id: &str,
) -> Result<StoredUsageQuotaWindows, DataLayerError> {
    let rows = sqlx::query(
        r#"
SELECT window_scope, window_key, window_started_at, window_ends_at
FROM entitlement_usage_windows
WHERE user_entitlement_id = ?
        "#,
    )
    .bind(entitlement_id)
    .fetch_all(pool)
    .await
    .map_sql_err()?;
    let mut windows = StoredUsageQuotaWindows::new();
    for row in rows {
        let scope: String = row.try_get("window_scope").map_sql_err()?;
        windows.insert(
            scope,
            StoredUsageQuotaWindow {
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
            },
        );
    }
    Ok(windows)
}

fn read_count_mysql(row: &MySqlRow) -> Result<u64, DataLayerError> {
    Ok(row.try_get::<i64, _>("total").map_sql_err()?.max(0) as u64)
}

fn map_payment_gateway_config_mysql(
    row: &MySqlRow,
) -> Result<PaymentGatewayConfigRecord, DataLayerError> {
    Ok(PaymentGatewayConfigRecord {
        provider: row.try_get("provider").map_sql_err()?,
        enabled: row.try_get("enabled").map_sql_err()?,
        endpoint_url: row.try_get("endpoint_url").map_sql_err()?,
        callback_base_url: row.try_get("callback_base_url").map_sql_err()?,
        merchant_id: row.try_get("merchant_id").map_sql_err()?,
        merchant_key_encrypted: row.try_get("merchant_key_encrypted").map_sql_err()?,
        webhook_secret_encrypted: row.try_get("webhook_secret_encrypted").map_sql_err()?,
        pay_currency: row.try_get("pay_currency").map_sql_err()?,
        usd_exchange_rate: row.try_get("usd_exchange_rate").map_sql_err()?,
        min_recharge_usd: row.try_get("min_recharge_usd").map_sql_err()?,
        channels_json: parse_json(row.try_get("channels_json").ok().flatten())?
            .unwrap_or_else(|| serde_json::json!([])),
        created_at_unix_secs: row
            .try_get::<i64, _>("created_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
        updated_at_unix_secs: row
            .try_get::<i64, _>("updated_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
    })
}

fn map_billing_plan_mysql(row: &MySqlRow) -> Result<BillingPlanRecord, DataLayerError> {
    Ok(BillingPlanRecord {
        id: row.try_get("id").map_sql_err()?,
        title: row.try_get("title").map_sql_err()?,
        description: row.try_get("description").map_sql_err()?,
        price_amount: row.try_get("price_amount").map_sql_err()?,
        price_currency: row.try_get("price_currency").map_sql_err()?,
        duration_unit: row.try_get("duration_unit").map_sql_err()?,
        duration_value: row.try_get("duration_value").map_sql_err()?,
        enabled: row.try_get("enabled").map_sql_err()?,
        sort_order: row.try_get("sort_order").map_sql_err()?,
        max_active_per_user: row.try_get("max_active_per_user").map_sql_err()?,
        purchase_limit_scope: row
            .try_get::<Option<String>, _>("purchase_limit_scope")
            .map_sql_err()?
            .unwrap_or_else(|| "active_period".to_string()),
        allowed_provider_ids: Vec::new(),
        entitlements_json: parse_json(row.try_get("entitlements_json").ok().flatten())?
            .unwrap_or_else(|| serde_json::json!([])),
        created_at_unix_secs: row
            .try_get::<i64, _>("created_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
        updated_at_unix_secs: row
            .try_get::<i64, _>("updated_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
    })
}

fn map_user_plan_entitlement_mysql(
    row: &MySqlRow,
) -> Result<UserPlanEntitlementRecord, DataLayerError> {
    Ok(UserPlanEntitlementRecord {
        id: row.try_get("id").map_sql_err()?,
        user_id: row.try_get("user_id").map_sql_err()?,
        plan_id: row.try_get("plan_id").map_sql_err()?,
        payment_order_id: row.try_get("payment_order_id").map_sql_err()?,
        status: row.try_get("status").map_sql_err()?,
        starts_at_unix_secs: row
            .try_get::<i64, _>("starts_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
        expires_at_unix_secs: row
            .try_get::<i64, _>("expires_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
        allowed_provider_ids: parse_string_vec_json(
            row.try_get::<Option<String>, _>("allowed_provider_ids")
                .map_sql_err()?,
        )?,
        entitlements_snapshot: parse_json(row.try_get("entitlements_snapshot").ok().flatten())?
            .unwrap_or_else(|| serde_json::json!([])),
        created_at_unix_secs: row
            .try_get::<i64, _>("created_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
        updated_at_unix_secs: row
            .try_get::<i64, _>("updated_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
    })
}

async fn find_admin_billing_rule_mysql(
    pool: &MysqlPool,
    rule_id: &str,
) -> Result<Option<AdminBillingRuleRecord>, DataLayerError> {
    let row = sqlx::query(
        r#"
SELECT
  id, name, task_type, global_model_id, model_id, expression, variables,
  dimension_mappings, is_enabled, created_at AS created_at_unix_ms,
  updated_at AS updated_at_unix_secs
FROM billing_rules
WHERE id = ?
        "#,
    )
    .bind(rule_id)
    .fetch_optional(pool)
    .await
    .map_sql_err()?;
    row.as_ref().map(map_admin_billing_rule_mysql).transpose()
}

fn map_admin_billing_rule_mysql(row: &MySqlRow) -> Result<AdminBillingRuleRecord, DataLayerError> {
    Ok(AdminBillingRuleRecord {
        id: row.try_get("id").map_sql_err()?,
        name: row.try_get("name").map_sql_err()?,
        task_type: row.try_get("task_type").map_sql_err()?,
        global_model_id: row.try_get("global_model_id").map_sql_err()?,
        model_id: row.try_get("model_id").map_sql_err()?,
        expression: row.try_get("expression").map_sql_err()?,
        variables: parse_required_json(row.try_get("variables").map_sql_err()?)?,
        dimension_mappings: parse_required_json(row.try_get("dimension_mappings").map_sql_err()?)?,
        is_enabled: row.try_get("is_enabled").map_sql_err()?,
        created_at_unix_ms: row
            .try_get::<i64, _>("created_at_unix_ms")
            .map_sql_err()?
            .max(0) as u64,
        updated_at_unix_secs: row
            .try_get::<i64, _>("updated_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
    })
}

async fn find_admin_billing_collector_mysql(
    pool: &MysqlPool,
    collector_id: &str,
) -> Result<Option<AdminBillingCollectorRecord>, DataLayerError> {
    let row = sqlx::query(
        r#"
SELECT
  id, api_format, task_type, dimension_name, source_type, source_path, value_type,
  transform_expression, default_value, priority, is_enabled,
  created_at AS created_at_unix_ms, updated_at AS updated_at_unix_secs
FROM dimension_collectors
WHERE id = ?
        "#,
    )
    .bind(collector_id)
    .fetch_optional(pool)
    .await
    .map_sql_err()?;
    row.as_ref()
        .map(map_admin_billing_collector_mysql)
        .transpose()
}

fn map_admin_billing_collector_mysql(
    row: &MySqlRow,
) -> Result<AdminBillingCollectorRecord, DataLayerError> {
    Ok(AdminBillingCollectorRecord {
        id: row.try_get("id").map_sql_err()?,
        api_format: row.try_get("api_format").map_sql_err()?,
        task_type: row.try_get("task_type").map_sql_err()?,
        dimension_name: row.try_get("dimension_name").map_sql_err()?,
        source_type: row.try_get("source_type").map_sql_err()?,
        source_path: row.try_get("source_path").map_sql_err()?,
        value_type: row.try_get("value_type").map_sql_err()?,
        transform_expression: row.try_get("transform_expression").map_sql_err()?,
        default_value: row.try_get("default_value").map_sql_err()?,
        priority: row.try_get("priority").map_sql_err()?,
        is_enabled: row.try_get("is_enabled").map_sql_err()?,
        created_at_unix_ms: row
            .try_get::<i64, _>("created_at_unix_ms")
            .map_sql_err()?
            .max(0) as u64,
        updated_at_unix_secs: row
            .try_get::<i64, _>("updated_at_unix_secs")
            .map_sql_err()?
            .max(0) as u64,
    })
}

fn parse_required_json(raw: String) -> Result<serde_json::Value, DataLayerError> {
    serde_json::from_str(&raw).map_err(|err| {
        DataLayerError::UnexpectedValue(format!("billing JSON field is invalid: {err}"))
    })
}

#[cfg(test)]
mod tests {
    use super::MysqlBillingReadRepository;

    #[tokio::test]
    async fn repository_builds_from_lazy_pool() {
        let pool = sqlx::mysql::MySqlPoolOptions::new().connect_lazy_with(
            "mysql://user:pass@localhost:3306/aether"
                .parse()
                .expect("mysql options should parse"),
        );

        let _repository = MysqlBillingReadRepository::new(pool);
    }
}

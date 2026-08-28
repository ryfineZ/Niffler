use aether_data::DataLayerError;
use aether_data_contracts::repository::niffler_core::StoredNifflerReferralRewardLedger;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashSet;

use super::GatewayDataState;

const REFERRAL_FETCH_LIMIT: usize = 5_000;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReferralUserDashboard {
    pub(crate) invite_code: String,
    pub(crate) total_invites: u64,
    pub(crate) effective_invites: u64,
    pub(crate) paid_reward_usd: f64,
    pub(crate) pending_reward_usd: f64,
    pub(crate) reversed_reward_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReferralRelationshipRecord {
    pub(crate) id: String,
    pub(crate) inviter_user_id: String,
    pub(crate) inviter_username: Option<String>,
    pub(crate) invitee_user_id: String,
    pub(crate) invitee_username: Option<String>,
    pub(crate) invite_code_snapshot: String,
    pub(crate) first_paid_order_id: Option<String>,
    pub(crate) first_paid_at_unix_secs: Option<u64>,
    pub(crate) source: Option<serde_json::Value>,
    pub(crate) created_at_unix_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReferralRewardRecord {
    pub(crate) id: String,
    pub(crate) referral_id: String,
    pub(crate) inviter_user_id: String,
    pub(crate) invitee_user_id: String,
    pub(crate) reward_type: String,
    pub(crate) source_order_id: Option<String>,
    pub(crate) trigger_point: String,
    pub(crate) amount_usd: f64,
    pub(crate) status: String,
    pub(crate) wallet_transaction_id: Option<String>,
    pub(crate) idempotency_key: String,
    pub(crate) reversed_amount_usd: f64,
    pub(crate) pending_reversal_amount_usd: f64,
    pub(crate) admin_operator_id: Option<String>,
    pub(crate) admin_note: Option<String>,
    pub(crate) created_at_unix_secs: u64,
    pub(crate) updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct ReferralAdminStats {
    pub(crate) total_invites: u64,
    pub(crate) effective_invites: u64,
    pub(crate) paid_reward_usd: f64,
    pub(crate) pending_reward_usd: f64,
    pub(crate) reversed_reward_usd: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ReferralRelationshipListQuery {
    pub(crate) inviter: Option<String>,
    pub(crate) invitee: Option<String>,
    pub(crate) invite_code: Option<String>,
    pub(crate) first_paid: Option<bool>,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ReferralRewardListQuery {
    pub(crate) order_id: Option<String>,
    pub(crate) reward_type: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ReferralRewardConfig {
    pub(crate) percent_enabled: bool,
    pub(crate) percent_rate: f64,
    pub(crate) headcount_enabled: bool,
    pub(crate) headcount_amount_usd: f64,
    pub(crate) headcount_trigger: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReferralMutationStatus {
    Applied,
    NotFound,
    Invalid,
    Unavailable,
}

#[derive(Debug, Clone)]
struct ReferralPaymentOrderContext {
    id: String,
    user_id: String,
    amount_usd: f64,
    payment_method: String,
    status: String,
    order_kind: String,
}

#[derive(Debug, Clone)]
struct ReferralPaymentOrderRefundContext {
    amount_usd: f64,
    refunded_amount_usd: f64,
}

#[derive(Debug, Clone)]
struct ReferralCreditTarget {
    id: String,
    wallet_id: String,
    inviter_user_id: String,
    invitee_user_id: String,
    amount_usd: f64,
    reward_type: String,
    trigger_point: String,
}

#[derive(Debug, Clone)]
struct NifflerReferralLedgerPaymentTarget {
    id: String,
    order_id: String,
    inviter_user_id: String,
    invitee_user_id: String,
    reward_amount_usd: f64,
    status: String,
}

impl NifflerReferralLedgerPaymentTarget {
    fn to_stored_ledger(&self) -> StoredNifflerReferralRewardLedger {
        StoredNifflerReferralRewardLedger {
            id: self.id.clone(),
            order_id: self.order_id.clone(),
            idempotency_key: format!("niffler-referral-ledger:order:{}", self.order_id),
            inviter_user_id: self.inviter_user_id.clone(),
            invitee_user_id: self.invitee_user_id.clone(),
            rule_id: None,
            reward_amount_usd: self.reward_amount_usd,
            rule_snapshot: serde_json::Value::Null,
            status: aether_data_contracts::repository::niffler_core::NifflerReferralRewardLedgerStatus::Pending,
            failure_reason: None,
            retry_count: 0,
            paid_at_unix_ms: None,
            cancelled_at_unix_ms: None,
            created_at_unix_ms: 0,
            updated_at_unix_ms: 0,
        }
    }
}

macro_rules! row_string {
    ($row:expr, $col:expr) => {
        $row.try_get::<String, _>($col)
            .map_err(DataLayerError::sql)?
    };
}

macro_rules! row_optional_string {
    ($row:expr, $col:expr) => {
        $row.try_get::<Option<String>, _>($col)
            .map_err(DataLayerError::sql)?
    };
}

macro_rules! row_f64 {
    ($row:expr, $col:expr) => {
        $row.try_get::<f64, _>($col).map_err(DataLayerError::sql)?
    };
}

macro_rules! relationship_from_row {
    ($row:expr) => {{
        let source_text = row_optional_string!($row, "source_json");
        Ok(ReferralRelationshipRecord {
            id: row_string!($row, "id"),
            inviter_user_id: row_string!($row, "inviter_user_id"),
            inviter_username: row_optional_string!($row, "inviter_username"),
            invitee_user_id: row_string!($row, "invitee_user_id"),
            invitee_username: row_optional_string!($row, "invitee_username"),
            invite_code_snapshot: row_string!($row, "invite_code_snapshot"),
            first_paid_order_id: row_optional_string!($row, "first_paid_order_id"),
            first_paid_at_unix_secs: row_optional_unix_secs($row, "first_paid_at_unix_secs")?,
            source: parse_optional_json(source_text)?,
            created_at_unix_secs: row_unix_secs($row, "created_at_unix_secs")?,
        })
    }};
}

macro_rules! reward_from_row {
    ($row:expr) => {{
        Ok(ReferralRewardRecord {
            id: row_string!($row, "id"),
            referral_id: row_string!($row, "referral_id"),
            inviter_user_id: row_string!($row, "inviter_user_id"),
            invitee_user_id: row_string!($row, "invitee_user_id"),
            reward_type: row_string!($row, "reward_type"),
            source_order_id: row_optional_string!($row, "source_order_id"),
            trigger_point: row_string!($row, "trigger_point"),
            amount_usd: row_f64!($row, "amount_usd"),
            status: row_string!($row, "status"),
            wallet_transaction_id: row_optional_string!($row, "wallet_transaction_id"),
            idempotency_key: row_string!($row, "idempotency_key"),
            reversed_amount_usd: row_f64!($row, "reversed_amount_usd"),
            pending_reversal_amount_usd: row_f64!($row, "pending_reversal_amount_usd"),
            admin_operator_id: row_optional_string!($row, "admin_operator_id"),
            admin_note: row_optional_string!($row, "admin_note"),
            created_at_unix_secs: row_unix_secs($row, "created_at_unix_secs")?,
            updated_at_unix_secs: row_unix_secs($row, "updated_at_unix_secs")?,
        })
    }};
}

fn now_unix_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn row_unix_secs<R>(row: &R, column: &str) -> Result<u64, DataLayerError>
where
    R: Row,
    for<'c> &'c str: sqlx::ColumnIndex<R>,
    for<'r> i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    let value = row.try_get::<i64, _>(column).map_err(DataLayerError::sql)?;
    Ok(value.max(0) as u64)
}

fn row_optional_unix_secs<R>(row: &R, column: &str) -> Result<Option<u64>, DataLayerError>
where
    R: Row,
    for<'c> &'c str: sqlx::ColumnIndex<R>,
    for<'r> Option<i64>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    let value = row
        .try_get::<Option<i64>, _>(column)
        .map_err(DataLayerError::sql)?;
    Ok(value.map(|value| value.max(0) as u64))
}

fn parse_optional_json(value: Option<String>) -> Result<Option<serde_json::Value>, DataLayerError> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| serde_json::from_str(&value).map_err(DataLayerError::sql))
        .transpose()
}

fn generate_invite_code() -> String {
    format!(
        "AE{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10].to_ascii_uppercase()
    )
}

fn referral_text_matches(value: Option<&str>, needle: Option<&str>) -> bool {
    let Some(needle) = needle.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let Some(value) = value else {
        return false;
    };
    value
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn referral_list_window<T: Clone>(items: &[T], limit: usize, offset: usize) -> Vec<T> {
    let limit = limit.clamp(1, 200);
    items.iter().skip(offset).take(limit).cloned().collect()
}

fn referral_admin_stats(
    relationships: &[ReferralRelationshipRecord],
    rewards: &[ReferralRewardRecord],
) -> ReferralAdminStats {
    ReferralAdminStats {
        total_invites: relationships.len() as u64,
        effective_invites: relationships
            .iter()
            .filter(|item| item.first_paid_order_id.is_some())
            .count() as u64,
        paid_reward_usd: rewards
            .iter()
            .filter(|item| item.status == "applied")
            .map(|item| item.amount_usd)
            .sum(),
        pending_reward_usd: rewards
            .iter()
            .filter(|item| matches!(item.status.as_str(), "pending" | "failed"))
            .map(|item| item.amount_usd)
            .sum(),
        reversed_reward_usd: rewards.iter().map(|item| item.reversed_amount_usd).sum(),
    }
}

fn normalize_referral_code(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_uppercase();
    (!value.is_empty() && value.len() <= 64).then_some(value)
}

fn reward_description(target: &ReferralCreditTarget) -> String {
    match target.reward_type.as_str() {
        "percent" => "邀请充值比例返利".to_string(),
        "headcount" => "邀请人头返利".to_string(),
        _ => "邀请返利".to_string(),
    }
}

fn referral_retry_allowed(status: &str) -> bool {
    status == "failed"
}

fn referral_void_allowed(status: &str) -> bool {
    matches!(status, "pending" | "failed")
}

fn referral_reversal_delta(
    reward_amount_usd: f64,
    order_amount_usd: f64,
    refunded_amount_usd: f64,
    reversed_amount_usd: f64,
    pending_reversal_amount_usd: f64,
) -> f64 {
    let target_reversal =
        referral_reversal_target(reward_amount_usd, order_amount_usd, refunded_amount_usd);
    (target_reversal - reversed_amount_usd - pending_reversal_amount_usd).max(0.0)
}

fn referral_reversal_target(
    reward_amount_usd: f64,
    order_amount_usd: f64,
    refunded_amount_usd: f64,
) -> f64 {
    if reward_amount_usd <= 0.0 || order_amount_usd <= 0.0 || refunded_amount_usd <= 0.0 {
        return 0.0;
    }
    reward_amount_usd * (refunded_amount_usd / order_amount_usd).clamp(0.0, 1.0)
}

impl GatewayDataState {
    pub(crate) fn has_referral_data_backend(&self) -> bool {
        self.backends.is_some()
    }

    pub(crate) async fn record_user_privacy_policy_acceptance(
        &self,
        user_id: &str,
        version: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(false);
        };
        if let Some(backend) = backends.postgres() {
            let affected = sqlx::query(
                r#"
UPDATE users
SET privacy_policy_accepted_version = $2,
    privacy_policy_accepted_at = NOW()
WHERE id = $1
"#,
            )
            .bind(user_id)
            .bind(version)
            .execute(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?
            .rows_affected();
            return Ok(affected > 0);
        }
        Ok(false)
    }

    pub(crate) async fn referral_dashboard(
        &self,
        user_id: &str,
    ) -> Result<Option<ReferralUserDashboard>, DataLayerError> {
        let Some(invite_code) = self.ensure_referral_invite_code(user_id).await? else {
            return Ok(None);
        };
        let relationships = self
            .list_referral_relationships_raw(Some(user_id), None)
            .await?;
        let rewards = self.list_referral_rewards_raw(Some(user_id)).await?;
        let stats = referral_admin_stats(&relationships, &rewards);
        Ok(Some(ReferralUserDashboard {
            invite_code,
            total_invites: stats.total_invites,
            effective_invites: stats.effective_invites,
            paid_reward_usd: stats.paid_reward_usd,
            pending_reward_usd: stats.pending_reward_usd,
            reversed_reward_usd: stats.reversed_reward_usd,
        }))
    }

    pub(crate) async fn list_admin_referral_relationships(
        &self,
        query: ReferralRelationshipListQuery,
    ) -> Result<Option<(Vec<ReferralRelationshipRecord>, u64, ReferralAdminStats)>, DataLayerError>
    {
        if self.backends.is_none() {
            return Ok(None);
        }
        let all_relationships = self.list_referral_relationships_raw(None, None).await?;
        let all_rewards = self.list_referral_rewards_raw(None).await?;
        let filtered = all_relationships
            .into_iter()
            .filter(|item| {
                referral_text_matches(item.inviter_username.as_deref(), query.inviter.as_deref())
                    || referral_text_matches(
                        Some(item.inviter_user_id.as_str()),
                        query.inviter.as_deref(),
                    )
            })
            .filter(|item| {
                referral_text_matches(item.invitee_username.as_deref(), query.invitee.as_deref())
                    || referral_text_matches(
                        Some(item.invitee_user_id.as_str()),
                        query.invitee.as_deref(),
                    )
            })
            .filter(|item| {
                referral_text_matches(
                    Some(item.invite_code_snapshot.as_str()),
                    query.invite_code.as_deref(),
                )
            })
            .filter(|item| {
                query
                    .first_paid
                    .map(|expected| item.first_paid_order_id.is_some() == expected)
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        let total = filtered.len() as u64;
        let filtered_referral_ids = filtered
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        let filtered_rewards = all_rewards
            .into_iter()
            .filter(|item| filtered_referral_ids.contains(item.referral_id.as_str()))
            .collect::<Vec<_>>();
        let stats = referral_admin_stats(&filtered, &filtered_rewards);
        Ok(Some((
            referral_list_window(&filtered, query.limit, query.offset),
            total,
            stats,
        )))
    }

    pub(crate) async fn list_admin_referral_rewards(
        &self,
        query: ReferralRewardListQuery,
    ) -> Result<Option<(Vec<ReferralRewardRecord>, u64, ReferralAdminStats)>, DataLayerError> {
        if self.backends.is_none() {
            return Ok(None);
        }
        let relationships = self.list_referral_relationships_raw(None, None).await?;
        let rewards = self.list_referral_rewards_raw(None).await?;
        let filtered = rewards
            .iter()
            .filter(|item| {
                referral_text_matches(item.source_order_id.as_deref(), query.order_id.as_deref())
            })
            .filter(|item| {
                referral_text_matches(
                    Some(item.reward_type.as_str()),
                    query.reward_type.as_deref(),
                )
            })
            .filter(|item| {
                referral_text_matches(Some(item.status.as_str()), query.status.as_deref())
            })
            .cloned()
            .collect::<Vec<_>>();
        let total = filtered.len() as u64;
        let filtered_referral_ids = filtered
            .iter()
            .map(|item| item.referral_id.as_str())
            .collect::<HashSet<_>>();
        let filtered_relationships = relationships
            .into_iter()
            .filter(|item| filtered_referral_ids.contains(item.id.as_str()))
            .collect::<Vec<_>>();
        let stats = referral_admin_stats(&filtered_relationships, &filtered);
        Ok(Some((
            referral_list_window(&filtered, query.limit, query.offset),
            total,
            stats,
        )))
    }

    pub(crate) async fn bind_referral_invite_code(
        &self,
        invitee_user_id: &str,
        invite_code: Option<&str>,
        source: Option<serde_json::Value>,
    ) -> Result<Option<ReferralRelationshipRecord>, DataLayerError> {
        let Some(code) = invite_code.and_then(normalize_referral_code) else {
            return Ok(None);
        };
        let Some(inviter_user_id) = self.find_referral_inviter_by_code(&code).await? else {
            return Err(DataLayerError::InvalidInput("邀请码无效".to_string()));
        };
        if inviter_user_id == invitee_user_id {
            return Err(DataLayerError::InvalidInput(
                "不能使用自己的邀请码注册".to_string(),
            ));
        }
        let referral_id = uuid::Uuid::new_v4().to_string();
        let source_json = source.map(|value| value.to_string());
        let inserted = self
            .insert_referral_relationship(
                &referral_id,
                &inviter_user_id,
                invitee_user_id,
                &code,
                source_json.as_deref(),
            )
            .await?;
        if !inserted {
            return Ok(None);
        }
        self.find_referral_relationship(&referral_id).await
    }

    pub(crate) async fn apply_registration_referral_reward(
        &self,
        invitee_user_id: &str,
        amount_usd: f64,
        trigger_point: &str,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        if amount_usd <= 0.0 {
            return Ok(Vec::new());
        }
        let Some(relationship) = self
            .find_referral_relationship_by_invitee(invitee_user_id)
            .await?
        else {
            return Ok(Vec::new());
        };
        let idempotency_key = format!("referral:{}:headcount:{trigger_point}", relationship.id);
        self.insert_referral_reward(
            &relationship,
            "headcount",
            None,
            trigger_point,
            amount_usd,
            &idempotency_key,
            "pending",
        )
        .await?;
        self.credit_pending_referral_rewards(&[idempotency_key], None, None)
            .await
    }

    pub(crate) async fn apply_paid_order_referral_rewards(
        &self,
        order_id: &str,
        config: ReferralRewardConfig,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        self.create_paid_order_referral_rewards(order_id, config, "pending", true)
            .await
    }

    pub(crate) async fn create_paid_order_referral_rewards_for_ledger(
        &self,
        order_id: &str,
        config: ReferralRewardConfig,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        self.create_paid_order_referral_rewards(order_id, config, "ledger_pending", false)
            .await
    }

    async fn create_paid_order_referral_rewards(
        &self,
        order_id: &str,
        config: ReferralRewardConfig,
        initial_status: &str,
        credit_legacy_rewards: bool,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        if self.backends.is_none() {
            return Ok(Vec::new());
        }
        if !config.percent_enabled && !config.headcount_enabled {
            return Ok(Vec::new());
        }
        let Some(context) = self.find_referral_payment_order_context(order_id).await? else {
            return Ok(Vec::new());
        };
        if context.status != "credited" {
            return Ok(Vec::new());
        }
        if !matches!(
            context.order_kind.as_str(),
            "wallet_recharge" | "plan_purchase"
        ) {
            return Ok(Vec::new());
        }
        if matches!(
            context.payment_method.as_str(),
            "manual" | "admin_manual" | "redeem_code" | "gift"
        ) {
            return Ok(Vec::new());
        }
        let Some(relationship) = self
            .find_referral_relationship_by_invitee(&context.user_id)
            .await?
        else {
            return Ok(Vec::new());
        };
        let marked_first_paid = self
            .mark_referral_first_paid_order(&relationship.id, &context.id)
            .await?;

        let mut idempotency_keys = Vec::new();
        if config.percent_enabled && config.percent_rate > 0.0 {
            let amount_usd = (context.amount_usd * config.percent_rate / 100.0).max(0.0);
            if amount_usd > 0.0 {
                let idempotency_key =
                    format!("referral:{}:percent:{}", relationship.id, context.id);
                self.insert_referral_reward(
                    &relationship,
                    "percent",
                    Some(&context.id),
                    "paid_order",
                    amount_usd,
                    &idempotency_key,
                    initial_status,
                )
                .await?;
                idempotency_keys.push(idempotency_key);
            }
        }
        if config.headcount_enabled
            && config.headcount_amount_usd > 0.0
            && config.headcount_trigger == "first_paid_order"
            && marked_first_paid
        {
            let idempotency_key =
                format!("referral:{}:headcount:first_paid_order", relationship.id);
            self.insert_referral_reward(
                &relationship,
                "headcount",
                Some(&context.id),
                "first_paid_order",
                config.headcount_amount_usd,
                &idempotency_key,
                initial_status,
            )
            .await?;
            idempotency_keys.push(idempotency_key);
        }

        if idempotency_keys.is_empty() {
            return Ok(Vec::new());
        }
        if credit_legacy_rewards {
            return self
                .credit_pending_referral_rewards(&idempotency_keys, None, None)
                .await;
        }
        let mut rewards = Vec::new();
        for key in idempotency_keys {
            if let Some(reward) = self.find_referral_reward_by_idempotency_key(&key).await? {
                if reward.status == initial_status {
                    rewards.push(reward);
                }
            }
        }
        Ok(rewards)
    }

    pub(crate) async fn retry_referral_reward(
        &self,
        reward_id: &str,
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<Option<ReferralRewardRecord>, DataLayerError> {
        let Some(reward) = self.find_referral_reward(reward_id).await? else {
            return Ok(None);
        };
        if !referral_retry_allowed(&reward.status) {
            return Err(DataLayerError::InvalidInput(
                "仅失败返利可以补发".to_string(),
            ));
        }
        let rewards = self
            .credit_pending_referral_rewards(&[reward.idempotency_key], operator_id, note)
            .await?;
        Ok(rewards.into_iter().next())
    }

    pub(crate) async fn void_referral_reward(
        &self,
        reward_id: &str,
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<Option<ReferralRewardRecord>, DataLayerError> {
        let Some(reward) = self.find_referral_reward(reward_id).await? else {
            return Ok(None);
        };
        if !referral_void_allowed(&reward.status) {
            return Err(DataLayerError::InvalidInput(
                "仅待发或失败返利可以作废".to_string(),
            ));
        }
        self.update_referral_reward_status(reward_id, "voided", operator_id, note)
            .await?;
        self.find_referral_reward(reward_id).await
    }

    pub(crate) async fn retry_niffler_referral_reward_ledger(
        &self,
        ledger_id: &str,
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<ReferralMutationStatus, DataLayerError> {
        let Some(target) = self
            .find_niffler_referral_ledger_payment_target(ledger_id)
            .await?
        else {
            return Ok(ReferralMutationStatus::NotFound);
        };
        if !matches!(target.status.as_str(), "pending" | "failed") {
            return Ok(ReferralMutationStatus::Invalid);
        }
        let rewards = self
            .find_niffler_ledger_managed_referral_rewards_by_order(&target.order_id)
            .await?;
        if rewards.is_empty() {
            return Ok(ReferralMutationStatus::Invalid);
        }
        let ledger = target.to_stored_ledger();
        match self
            .pay_niffler_referral_reward_ledger(&ledger, &rewards, operator_id, note)
            .await?
        {
            Some(_) => Ok(ReferralMutationStatus::Applied),
            None => Ok(ReferralMutationStatus::Invalid),
        }
    }

    pub(crate) async fn cancel_niffler_referral_reward_ledger(
        &self,
        ledger_id: &str,
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<ReferralMutationStatus, DataLayerError> {
        let Some(target) = self
            .find_niffler_referral_ledger_payment_target(ledger_id)
            .await?
        else {
            return Ok(ReferralMutationStatus::NotFound);
        };
        if !matches!(target.status.as_str(), "pending" | "failed") {
            return Ok(ReferralMutationStatus::Invalid);
        }
        self.cancel_niffler_referral_reward_ledger_target(&target, operator_id, note)
            .await
    }

    pub(crate) async fn reverse_referral_rewards_for_order(
        &self,
        order_id: &str,
        amount_usd: f64,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        if amount_usd <= 0.0 {
            return Ok(Vec::new());
        }
        let Some(refund_context) = self
            .find_referral_payment_order_refund_context(order_id)
            .await?
        else {
            return Ok(Vec::new());
        };
        let rewards = self
            .find_applied_referral_rewards_by_order(order_id)
            .await?;
        let mut reversed = Vec::new();
        for reward in rewards {
            let reversal_amount = referral_reversal_delta(
                reward.amount_usd,
                refund_context.amount_usd,
                refund_context.refunded_amount_usd,
                reward.reversed_amount_usd,
                reward.pending_reversal_amount_usd,
            );
            if reversal_amount <= 0.0 {
                continue;
            }
            let target_reversal = referral_reversal_target(
                reward.amount_usd,
                refund_context.amount_usd,
                refund_context.refunded_amount_usd,
            );
            self.apply_referral_reward_reversal(&reward, target_reversal)
                .await?;
            if let Some(updated) = self.find_referral_reward(&reward.id).await? {
                reversed.push(updated);
            }
        }
        Ok(reversed)
    }
}

impl GatewayDataState {
    async fn ensure_referral_invite_code(
        &self,
        user_id: &str,
    ) -> Result<Option<String>, DataLayerError> {
        if let Some(existing) = self.find_referral_invite_code(user_id).await? {
            return Ok(Some(existing));
        }
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        for _ in 0..5 {
            let code = generate_invite_code();
            let inserted = if let Some(backend) = backends.postgres() {
                sqlx::query(
                    r#"
INSERT INTO user_invite_codes (user_id, invite_code, active, created_at, updated_at)
VALUES ($1, $2, TRUE, NOW(), NOW())
ON CONFLICT DO NOTHING
"#,
                )
                .bind(user_id)
                .bind(&code)
                .execute(&backend.pool_clone())
                .await
                .map_err(DataLayerError::postgres)?
                .rows_affected()
            } else {
                0
            };
            if inserted > 0 {
                return Ok(Some(code));
            }
            if let Some(existing) = self.find_referral_invite_code(user_id).await? {
                return Ok(Some(existing));
            }
        }
        self.find_referral_invite_code(user_id).await
    }

    async fn find_referral_invite_code(
        &self,
        user_id: &str,
    ) -> Result<Option<String>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                "SELECT invite_code FROM user_invite_codes WHERE user_id = $1 AND active = TRUE",
            )
            .bind(user_id)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row
                .map(|row| {
                    row.try_get::<String, _>("invite_code")
                        .map_err(DataLayerError::sql)
                })
                .transpose();
        }
        Ok(None)
    }

    async fn find_referral_inviter_by_code(
        &self,
        invite_code: &str,
    ) -> Result<Option<String>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                "SELECT user_id FROM user_invite_codes WHERE invite_code = $1 AND active = TRUE",
            )
            .bind(invite_code)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row
                .map(|row| {
                    row.try_get::<String, _>("user_id")
                        .map_err(DataLayerError::sql)
                })
                .transpose();
        }
        Ok(None)
    }

    async fn insert_referral_relationship(
        &self,
        referral_id: &str,
        inviter_user_id: &str,
        invitee_user_id: &str,
        invite_code: &str,
        source_json: Option<&str>,
    ) -> Result<bool, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(false);
        };
        if let Some(backend) = backends.postgres() {
            let affected = sqlx::query(
                r#"
INSERT INTO user_referrals (
  id, inviter_user_id, invitee_user_id, invite_code_snapshot, source_json, created_at, updated_at
)
VALUES ($1, $2, $3, $4, $5::jsonb, NOW(), NOW())
ON CONFLICT (invitee_user_id) DO NOTHING
"#,
            )
            .bind(referral_id)
            .bind(inviter_user_id)
            .bind(invitee_user_id)
            .bind(invite_code)
            .bind(source_json)
            .execute(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?
            .rows_affected();
            return Ok(affected > 0);
        }
        Ok(false)
    }

    async fn list_referral_relationships_raw(
        &self,
        inviter_user_id: Option<&str>,
        invitee_user_id: Option<&str>,
    ) -> Result<Vec<ReferralRelationshipRecord>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(Vec::new());
        };
        if let Some(backend) = backends.postgres() {
            let rows = sqlx::query(
                r#"
SELECT
  r.id, r.inviter_user_id, inviter.username AS inviter_username,
  r.invitee_user_id, invitee.username AS invitee_username,
  r.invite_code_snapshot, r.first_paid_order_id,
  EXTRACT(EPOCH FROM r.first_paid_at)::BIGINT AS first_paid_at_unix_secs,
  r.source_json::TEXT AS source_json,
  EXTRACT(EPOCH FROM r.created_at)::BIGINT AS created_at_unix_secs
FROM user_referrals r
LEFT JOIN users inviter ON inviter.id = r.inviter_user_id
LEFT JOIN users invitee ON invitee.id = r.invitee_user_id
WHERE ($1::TEXT IS NULL OR r.inviter_user_id = $1)
  AND ($2::TEXT IS NULL OR r.invitee_user_id = $2)
ORDER BY r.created_at DESC
LIMIT $3
"#,
            )
            .bind(inviter_user_id)
            .bind(invitee_user_id)
            .bind(REFERRAL_FETCH_LIMIT as i64)
            .fetch_all(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return rows.iter().map(|row| relationship_from_row!(row)).collect();
        }
        Ok(Vec::new())
    }

    async fn find_referral_relationship(
        &self,
        referral_id: &str,
    ) -> Result<Option<ReferralRelationshipRecord>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                r#"
SELECT
  r.id, r.inviter_user_id, inviter.username AS inviter_username,
  r.invitee_user_id, invitee.username AS invitee_username,
  r.invite_code_snapshot, r.first_paid_order_id,
  EXTRACT(EPOCH FROM r.first_paid_at)::BIGINT AS first_paid_at_unix_secs,
  r.source_json::TEXT AS source_json,
  EXTRACT(EPOCH FROM r.created_at)::BIGINT AS created_at_unix_secs
FROM user_referrals r
LEFT JOIN users inviter ON inviter.id = r.inviter_user_id
LEFT JOIN users invitee ON invitee.id = r.invitee_user_id
WHERE r.id = $1
LIMIT 1
"#,
            )
            .bind(referral_id)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row.map(|row| relationship_from_row!(&row)).transpose();
        }
        Ok(None)
    }

    async fn find_referral_relationship_by_invitee(
        &self,
        invitee_user_id: &str,
    ) -> Result<Option<ReferralRelationshipRecord>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                r#"
SELECT
  r.id, r.inviter_user_id, inviter.username AS inviter_username,
  r.invitee_user_id, invitee.username AS invitee_username,
  r.invite_code_snapshot, r.first_paid_order_id,
  EXTRACT(EPOCH FROM r.first_paid_at)::BIGINT AS first_paid_at_unix_secs,
  r.source_json::TEXT AS source_json,
  EXTRACT(EPOCH FROM r.created_at)::BIGINT AS created_at_unix_secs
FROM user_referrals r
LEFT JOIN users inviter ON inviter.id = r.inviter_user_id
LEFT JOIN users invitee ON invitee.id = r.invitee_user_id
WHERE r.invitee_user_id = $1
LIMIT 1
"#,
            )
            .bind(invitee_user_id)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row.map(|row| relationship_from_row!(&row)).transpose();
        }
        Ok(None)
    }

    async fn list_referral_rewards_raw(
        &self,
        inviter_user_id: Option<&str>,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(Vec::new());
        };
        if let Some(backend) = backends.postgres() {
            let rows = sqlx::query(
                r#"
SELECT
  id, referral_id, inviter_user_id, invitee_user_id, reward_type, source_order_id,
  trigger_point, CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status, wallet_transaction_id, idempotency_key,
  CAST(reversed_amount_usd AS DOUBLE PRECISION) AS reversed_amount_usd,
  CAST(pending_reversal_amount_usd AS DOUBLE PRECISION) AS pending_reversal_amount_usd,
  admin_operator_id, admin_note,
  EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix_secs,
  EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix_secs
FROM referral_rewards
WHERE ($1::TEXT IS NULL OR inviter_user_id = $1)
ORDER BY created_at DESC
LIMIT $2
"#,
            )
            .bind(inviter_user_id)
            .bind(REFERRAL_FETCH_LIMIT as i64)
            .fetch_all(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return rows.iter().map(|row| reward_from_row!(row)).collect();
        }
        Ok(Vec::new())
    }

    async fn find_referral_reward(
        &self,
        reward_id: &str,
    ) -> Result<Option<ReferralRewardRecord>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                r#"
SELECT
  id, referral_id, inviter_user_id, invitee_user_id, reward_type, source_order_id,
  trigger_point, CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status, wallet_transaction_id, idempotency_key,
  CAST(reversed_amount_usd AS DOUBLE PRECISION) AS reversed_amount_usd,
  CAST(pending_reversal_amount_usd AS DOUBLE PRECISION) AS pending_reversal_amount_usd,
  admin_operator_id, admin_note,
  EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix_secs,
  EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix_secs
FROM referral_rewards
WHERE id = $1
LIMIT 1
"#,
            )
            .bind(reward_id)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row.map(|row| reward_from_row!(&row)).transpose();
        }
        Ok(None)
    }

    async fn find_referral_reward_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ReferralRewardRecord>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                r#"
SELECT
  id, referral_id, inviter_user_id, invitee_user_id, reward_type, source_order_id,
  trigger_point, CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status, wallet_transaction_id, idempotency_key,
  CAST(reversed_amount_usd AS DOUBLE PRECISION) AS reversed_amount_usd,
  CAST(pending_reversal_amount_usd AS DOUBLE PRECISION) AS pending_reversal_amount_usd,
  admin_operator_id, admin_note,
  EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix_secs,
  EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix_secs
FROM referral_rewards
WHERE idempotency_key = $1
LIMIT 1
"#,
            )
            .bind(idempotency_key)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row.map(|row| reward_from_row!(&row)).transpose();
        }
        Ok(None)
    }

    async fn find_applied_referral_rewards_by_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(Vec::new());
        };
        if let Some(backend) = backends.postgres() {
            let rows = sqlx::query(
                r#"
SELECT
  id, referral_id, inviter_user_id, invitee_user_id, reward_type, source_order_id,
  trigger_point, CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status, wallet_transaction_id, idempotency_key,
  CAST(reversed_amount_usd AS DOUBLE PRECISION) AS reversed_amount_usd,
  CAST(pending_reversal_amount_usd AS DOUBLE PRECISION) AS pending_reversal_amount_usd,
  admin_operator_id, admin_note,
  EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix_secs,
  EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix_secs
FROM referral_rewards
WHERE source_order_id = $1 AND status = 'applied'
ORDER BY created_at ASC
"#,
            )
            .bind(order_id)
            .fetch_all(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return rows.iter().map(|row| reward_from_row!(row)).collect();
        }
        Ok(Vec::new())
    }

    async fn insert_referral_reward(
        &self,
        relationship: &ReferralRelationshipRecord,
        reward_type: &str,
        source_order_id: Option<&str>,
        trigger_point: &str,
        amount_usd: f64,
        idempotency_key: &str,
        initial_status: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(false);
        };
        let reward_id = uuid::Uuid::new_v4().to_string();
        if let Some(backend) = backends.postgres() {
            let affected = sqlx::query(
                r#"
INSERT INTO referral_rewards (
  id, referral_id, inviter_user_id, invitee_user_id, reward_type, source_order_id,
  trigger_point, amount_usd, status, idempotency_key, created_at, updated_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW(), NOW())
ON CONFLICT (idempotency_key) DO NOTHING
"#,
            )
            .bind(&reward_id)
            .bind(&relationship.id)
            .bind(&relationship.inviter_user_id)
            .bind(&relationship.invitee_user_id)
            .bind(reward_type)
            .bind(source_order_id)
            .bind(trigger_point)
            .bind(amount_usd)
            .bind(initial_status)
            .bind(idempotency_key)
            .execute(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?
            .rows_affected();
            return Ok(affected > 0);
        }
        Ok(false)
    }

    async fn find_referral_payment_order_context(
        &self,
        order_id: &str,
    ) -> Result<Option<ReferralPaymentOrderContext>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                r#"
SELECT id, user_id, CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd, payment_method, status, order_kind
FROM payment_orders
WHERE id = $1
"#,
            )
            .bind(order_id)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row.map(payment_order_context_from_row).transpose();
        }
        Ok(None)
    }

    async fn find_referral_payment_order_refund_context(
        &self,
        order_id: &str,
    ) -> Result<Option<ReferralPaymentOrderRefundContext>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                r#"
SELECT
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  CAST(refunded_amount_usd AS DOUBLE PRECISION) AS refunded_amount_usd
FROM payment_orders
WHERE id = $1
"#,
            )
            .bind(order_id)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row.map(payment_order_refund_context_from_row).transpose();
        }
        Ok(None)
    }

    async fn mark_referral_first_paid_order(
        &self,
        referral_id: &str,
        order_id: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(false);
        };
        if let Some(backend) = backends.postgres() {
            let affected = sqlx::query(
                r#"
UPDATE user_referrals
SET first_paid_order_id = $2,
    first_paid_at = NOW(),
    updated_at = NOW()
WHERE id = $1 AND first_paid_order_id IS NULL
"#,
            )
            .bind(referral_id)
            .bind(order_id)
            .execute(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?
            .rows_affected();
            return Ok(affected > 0);
        }
        Ok(false)
    }

    async fn update_referral_reward_status(
        &self,
        reward_id: &str,
        status: &str,
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<bool, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(false);
        };
        if let Some(backend) = backends.postgres() {
            let affected = sqlx::query(
                r#"
UPDATE referral_rewards
SET status = $2,
    admin_operator_id = COALESCE($3, admin_operator_id),
    admin_note = COALESCE($4, admin_note),
    updated_at = NOW()
WHERE id = $1 AND status IN ('pending', 'failed')
"#,
            )
            .bind(reward_id)
            .bind(status)
            .bind(operator_id)
            .bind(note)
            .execute(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?
            .rows_affected();
            return Ok(affected > 0);
        }
        Ok(false)
    }

    async fn credit_pending_referral_rewards(
        &self,
        idempotency_keys: &[String],
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        let mut credited = Vec::new();
        for key in idempotency_keys {
            if let Some(target) = self.referral_credit_target_by_key(key).await? {
                self.credit_referral_reward(target, operator_id, note)
                    .await?;
                if let Some(updated) = self.find_referral_reward_by_idempotency_key(key).await? {
                    credited.push(updated);
                }
            }
        }
        Ok(credited)
    }

    async fn referral_credit_target_by_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ReferralCreditTarget>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                r#"
SELECT
  rw.id, rw.inviter_user_id, rw.invitee_user_id,
  CAST(rw.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  rw.reward_type,
  rw.trigger_point, wallets.id AS wallet_id
FROM referral_rewards rw
JOIN wallets ON wallets.user_id = rw.inviter_user_id
WHERE rw.idempotency_key = $1
  AND rw.status IN ('pending', 'failed')
"#,
            )
            .bind(idempotency_key)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row.map(credit_target_from_row).transpose();
        }
        Ok(None)
    }

    async fn credit_referral_reward(
        &self,
        target: ReferralCreditTarget,
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<(), DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(());
        };
        if let Some(backend) = backends.postgres() {
            let mut tx = backend
                .pool_clone()
                .begin()
                .await
                .map_err(DataLayerError::postgres)?;
            let claimed = sqlx::query(
                r#"
UPDATE referral_rewards
SET status = 'applying',
    admin_operator_id = COALESCE($2, admin_operator_id),
    admin_note = COALESCE($3, admin_note),
    updated_at = NOW()
WHERE id = $1 AND status IN ('pending', 'failed')
"#,
            )
            .bind(&target.id)
            .bind(operator_id)
            .bind(note)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?
            .rows_affected();
            if claimed == 0 {
                tx.commit().await.map_err(DataLayerError::postgres)?;
                return Ok(());
            }
            let wallet = sqlx::query(
                r#"
SELECT
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance
FROM wallets
WHERE id = $1
FOR UPDATE
"#,
            )
            .bind(&target.wallet_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            let Some(wallet) = wallet else {
                sqlx::query(
                    r#"
UPDATE referral_rewards
SET status = 'failed',
    admin_operator_id = COALESCE($2, admin_operator_id),
    admin_note = COALESCE($3, admin_note),
    updated_at = NOW()
WHERE id = $1
"#,
                )
                .bind(&target.id)
                .bind(operator_id)
                .bind(note.or(Some("邀请人钱包不存在")))
                .execute(&mut *tx)
                .await
                .map_err(DataLayerError::postgres)?;
                tx.commit().await.map_err(DataLayerError::postgres)?;
                return Ok(());
            };
            let balance = row_f64!(wallet, "balance");
            let gift_before = row_f64!(wallet, "gift_balance");
            let gift_after = gift_before + target.amount_usd;
            let tx_id = uuid::Uuid::new_v4().to_string();
            let description = note
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| reward_description(&target));
            sqlx::query(
                r#"
UPDATE wallets
SET gift_balance = $2,
    total_adjusted = total_adjusted + $3,
    updated_at = NOW()
WHERE id = $1
"#,
            )
            .bind(&target.wallet_id)
            .bind(gift_after)
            .bind(target.amount_usd)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            sqlx::query(
                r#"
INSERT INTO wallet_transactions (
  id, wallet_id, category, reason_code, amount,
  balance_before, balance_after,
  recharge_balance_before, recharge_balance_after,
  gift_balance_before, gift_balance_after,
  link_type, link_id, operator_id, description, created_at
)
VALUES ($1, $2, 'adjust', 'referral_reward', $3, $4, $5, $6, $6, $7, $8,
        'referral_reward', $9, $10, $11, NOW())
"#,
            )
            .bind(&tx_id)
            .bind(&target.wallet_id)
            .bind(target.amount_usd)
            .bind(balance + gift_before)
            .bind(balance + gift_after)
            .bind(balance)
            .bind(gift_before)
            .bind(gift_after)
            .bind(&target.id)
            .bind(operator_id)
            .bind(&description)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            sqlx::query(
                r#"
UPDATE referral_rewards
SET status = 'applied',
    wallet_transaction_id = $2,
    admin_operator_id = COALESCE($3, admin_operator_id),
    admin_note = COALESCE($4, admin_note),
    updated_at = NOW()
WHERE id = $1
"#,
            )
            .bind(&target.id)
            .bind(&tx_id)
            .bind(operator_id)
            .bind(note)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            tx.commit().await.map_err(DataLayerError::postgres)?;
            return Ok(());
        }
        Ok(())
    }

    async fn find_niffler_referral_ledger_payment_target(
        &self,
        ledger_id: &str,
    ) -> Result<Option<NifflerReferralLedgerPaymentTarget>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let row = sqlx::query(
                r#"
SELECT
  id, order_id, inviter_user_id, invitee_user_id,
  CAST(reward_amount_usd AS DOUBLE PRECISION) AS reward_amount_usd,
  status
FROM niffler_referral_reward_ledger
WHERE id = $1
"#,
            )
            .bind(ledger_id)
            .fetch_optional(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return row
                .map(niffler_referral_ledger_payment_target_from_row)
                .transpose();
        }
        Ok(None)
    }

    async fn find_niffler_ledger_managed_referral_rewards_by_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<ReferralRewardRecord>, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(Vec::new());
        };
        if let Some(backend) = backends.postgres() {
            let rows = sqlx::query(
                r#"
SELECT
  id, referral_id, inviter_user_id, invitee_user_id, reward_type, source_order_id,
  trigger_point, CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status, wallet_transaction_id, idempotency_key,
  CAST(reversed_amount_usd AS DOUBLE PRECISION) AS reversed_amount_usd,
  CAST(pending_reversal_amount_usd AS DOUBLE PRECISION) AS pending_reversal_amount_usd,
  admin_operator_id, admin_note,
  EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix_secs,
  EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix_secs
FROM referral_rewards
WHERE source_order_id = $1 AND status IN ('ledger_pending', 'ledger_failed')
ORDER BY created_at ASC
"#,
            )
            .bind(order_id)
            .fetch_all(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?;
            return rows.iter().map(|row| reward_from_row!(row)).collect();
        }
        Ok(Vec::new())
    }

    async fn cancel_niffler_referral_reward_ledger_target(
        &self,
        target: &NifflerReferralLedgerPaymentTarget,
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<ReferralMutationStatus, DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(ReferralMutationStatus::Unavailable);
        };
        let reason = note.unwrap_or("管理员取消返利发放");
        if let Some(backend) = backends.postgres() {
            let mut tx = backend
                .pool_clone()
                .begin()
                .await
                .map_err(DataLayerError::postgres)?;
            let now_ms = now_unix_ms() as i64;
            let affected = sqlx::query(
                r#"
UPDATE niffler_referral_reward_ledger
SET status = 'cancelled',
    failure_reason = NULL,
    cancelled_at_unix_ms = $2,
    updated_at_unix_ms = $2
WHERE id = $1 AND status IN ('pending', 'failed')
"#,
            )
            .bind(&target.id)
            .bind(now_ms)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?
            .rows_affected();
            if affected == 0 {
                tx.commit().await.map_err(DataLayerError::postgres)?;
                return Ok(ReferralMutationStatus::Invalid);
            }
            sqlx::query(
                r#"
UPDATE referral_rewards
SET status = 'ledger_cancelled',
    admin_operator_id = COALESCE($2, admin_operator_id),
    admin_note = COALESCE($3, admin_note),
    updated_at = NOW()
WHERE source_order_id = $1 AND status IN ('ledger_pending', 'ledger_failed')
"#,
            )
            .bind(&target.order_id)
            .bind(operator_id)
            .bind(reason)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            sqlx::query(
                r#"
INSERT INTO niffler_referral_reward_events (
  id, reward_ledger_id, event_kind, reason, actor_id, event_snapshot, created_at_unix_ms
)
VALUES ($1, $2, 'cancelled', $3, $4, $5, $6)
"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&target.id)
            .bind(reason)
            .bind(operator_id)
            .bind(serde_json::json!({
                "order_id": target.order_id,
                "reward_amount_usd": target.reward_amount_usd,
            }))
            .bind(now_ms)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            tx.commit().await.map_err(DataLayerError::postgres)?;
            return Ok(ReferralMutationStatus::Applied);
        }
        Ok(ReferralMutationStatus::Unavailable)
    }

    pub(crate) async fn pay_niffler_referral_reward_ledger(
        &self,
        ledger: &StoredNifflerReferralRewardLedger,
        rewards: &[ReferralRewardRecord],
        operator_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<Option<String>, DataLayerError> {
        if ledger.reward_amount_usd <= 0.0 || rewards.is_empty() {
            return Ok(None);
        }
        let Some(backends) = self.backends.as_ref() else {
            return Ok(None);
        };
        if let Some(backend) = backends.postgres() {
            let mut tx = backend
                .pool_clone()
                .begin()
                .await
                .map_err(DataLayerError::postgres)?;
            let row = sqlx::query(
                r#"
SELECT status
FROM niffler_referral_reward_ledger
WHERE id = $1
FOR UPDATE
"#,
            )
            .bind(&ledger.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            if !row
                .as_ref()
                .and_then(|row| row.try_get::<String, _>("status").ok())
                .is_some_and(|status| matches!(status.as_str(), "pending" | "failed"))
            {
                tx.commit().await.map_err(DataLayerError::postgres)?;
                return Ok(None);
            }
            let wallet = sqlx::query(
                r#"
SELECT
  id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance
FROM wallets
WHERE user_id = $1
FOR UPDATE
"#,
            )
            .bind(&ledger.inviter_user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            let Some(wallet) = wallet else {
                mark_postgres_niffler_referral_ledger_failed(
                    &mut tx,
                    ledger,
                    rewards,
                    operator_id,
                    "邀请人钱包不存在",
                )
                .await?;
                tx.commit().await.map_err(DataLayerError::postgres)?;
                return Ok(None);
            };
            let wallet_id = row_string!(wallet, "id");
            let balance = row_f64!(wallet, "balance");
            let gift_before = row_f64!(wallet, "gift_balance");
            let gift_after = gift_before + ledger.reward_amount_usd;
            let tx_id = uuid::Uuid::new_v4().to_string();
            let description = note
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| "邀请返利".to_string());
            sqlx::query(
                r#"
UPDATE wallets
SET gift_balance = $2,
    total_adjusted = total_adjusted + $3,
    updated_at = NOW()
WHERE id = $1
"#,
            )
            .bind(&wallet_id)
            .bind(gift_after)
            .bind(ledger.reward_amount_usd)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            sqlx::query(
                r#"
INSERT INTO wallet_transactions (
  id, wallet_id, category, reason_code, amount,
  balance_before, balance_after,
  recharge_balance_before, recharge_balance_after,
  gift_balance_before, gift_balance_after,
  link_type, link_id, operator_id, description, created_at
)
VALUES ($1, $2, 'adjust', 'referral_reward', $3, $4, $5, $6, $6, $7, $8,
        'niffler_referral_reward_ledger', $9, $10, $11, NOW())
"#,
            )
            .bind(&tx_id)
            .bind(&wallet_id)
            .bind(ledger.reward_amount_usd)
            .bind(balance + gift_before)
            .bind(balance + gift_after)
            .bind(balance)
            .bind(gift_before)
            .bind(gift_after)
            .bind(&ledger.id)
            .bind(operator_id)
            .bind(&description)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            let now_ms = now_unix_ms() as i64;
            sqlx::query(
                r#"
UPDATE niffler_referral_reward_ledger
SET status = 'paid',
    failure_reason = NULL,
    paid_at_unix_ms = $2,
    updated_at_unix_ms = $2
WHERE id = $1 AND status IN ('pending', 'failed')
"#,
            )
            .bind(&ledger.id)
            .bind(now_ms)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            for reward in rewards {
                sqlx::query(
                    r#"
UPDATE referral_rewards
SET status = 'applied',
    wallet_transaction_id = $2,
    admin_operator_id = COALESCE($3, admin_operator_id),
    admin_note = COALESCE($4, admin_note),
    updated_at = NOW()
WHERE id = $1 AND status IN ('ledger_pending', 'ledger_failed')
"#,
                )
                .bind(&reward.id)
                .bind(&tx_id)
                .bind(operator_id)
                .bind(note)
                .execute(&mut *tx)
                .await
                .map_err(DataLayerError::postgres)?;
            }
            sqlx::query(
                r#"
INSERT INTO niffler_referral_reward_events (
  id, reward_ledger_id, event_kind, reason, actor_id, event_snapshot, created_at_unix_ms
)
VALUES ($1, $2, 'paid', $3, $4, $5, $6)
"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&ledger.id)
            .bind(note)
            .bind(operator_id)
            .bind(niffler_referral_paid_event_snapshot(
                &tx_id,
                rewards,
                ledger.reward_amount_usd,
            ))
            .bind(now_ms)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            tx.commit().await.map_err(DataLayerError::postgres)?;
            return Ok(Some(tx_id));
        }
        Ok(None)
    }

    async fn apply_referral_reward_reversal(
        &self,
        reward: &ReferralRewardRecord,
        target_reversal_amount_usd: f64,
    ) -> Result<(), DataLayerError> {
        let Some(backends) = self.backends.as_ref() else {
            return Ok(());
        };
        if let Some(backend) = backends.postgres() {
            let mut tx = backend
                .pool_clone()
                .begin()
                .await
                .map_err(DataLayerError::postgres)?;
            let reward_row = sqlx::query(
                r#"
SELECT
  CAST(reversed_amount_usd AS DOUBLE PRECISION) AS reversed_amount_usd,
  CAST(pending_reversal_amount_usd AS DOUBLE PRECISION) AS pending_reversal_amount_usd
FROM referral_rewards
WHERE id = $1
FOR UPDATE
"#,
            )
            .bind(&reward.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            let Some(reward_row) = reward_row else {
                tx.commit().await.map_err(DataLayerError::postgres)?;
                return Ok(());
            };
            let current_reversed = row_f64!(reward_row, "reversed_amount_usd");
            let current_pending = row_f64!(reward_row, "pending_reversal_amount_usd");
            let amount_usd =
                (target_reversal_amount_usd - current_reversed - current_pending).max(0.0);
            if amount_usd <= 0.0 {
                tx.commit().await.map_err(DataLayerError::postgres)?;
                return Ok(());
            }
            let wallet = sqlx::query(
                r#"
SELECT
  id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance
FROM wallets
WHERE user_id = $1
FOR UPDATE
"#,
            )
            .bind(&reward.inviter_user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            let Some(wallet) = wallet else {
                tx.commit().await.map_err(DataLayerError::postgres)?;
                return Ok(());
            };
            let wallet_id = row_string!(wallet, "id");
            let balance = row_f64!(wallet, "balance");
            let gift_before = row_f64!(wallet, "gift_balance");
            let actual_reverse = gift_before.min(amount_usd);
            let pending_reverse = (amount_usd - actual_reverse).max(0.0);
            let gift_after = gift_before - actual_reverse;
            let tx_id = uuid::Uuid::new_v4().to_string();
            if actual_reverse > 0.0 {
                sqlx::query(
                    r#"
UPDATE wallets
SET gift_balance = $2,
    total_adjusted = total_adjusted - $3,
    updated_at = NOW()
WHERE id = $1
"#,
                )
                .bind(&wallet_id)
                .bind(gift_after)
                .bind(actual_reverse)
                .execute(&mut *tx)
                .await
                .map_err(DataLayerError::postgres)?;
                sqlx::query(
                    r#"
INSERT INTO wallet_transactions (
  id, wallet_id, category, reason_code, amount,
  balance_before, balance_after,
  recharge_balance_before, recharge_balance_after,
  gift_balance_before, gift_balance_after,
  link_type, link_id, description, created_at
)
VALUES ($1, $2, 'adjust', 'referral_reward_reversal', $3, $4, $5, $6, $6, $7, $8,
        'referral_reward', $9, '邀请返利退款冲回', NOW())
"#,
                )
                .bind(&tx_id)
                .bind(&wallet_id)
                .bind(-actual_reverse)
                .bind(balance + gift_before)
                .bind(balance + gift_after)
                .bind(balance)
                .bind(gift_before)
                .bind(gift_after)
                .bind(&reward.id)
                .execute(&mut *tx)
                .await
                .map_err(DataLayerError::postgres)?;
            }
            sqlx::query(
                r#"
UPDATE referral_rewards
SET reversed_amount_usd = reversed_amount_usd + $2,
    pending_reversal_amount_usd = pending_reversal_amount_usd + $3,
    status = CASE
      WHEN reversed_amount_usd + $2 >= amount_usd THEN 'reversed'
      ELSE status
    END,
    updated_at = NOW()
WHERE id = $1
"#,
            )
            .bind(&reward.id)
            .bind(actual_reverse)
            .bind(pending_reverse)
            .execute(&mut *tx)
            .await
            .map_err(DataLayerError::postgres)?;
            tx.commit().await.map_err(DataLayerError::postgres)?;
            return Ok(());
        }
        Ok(())
    }
}

async fn mark_postgres_niffler_referral_ledger_failed(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ledger: &StoredNifflerReferralRewardLedger,
    rewards: &[ReferralRewardRecord],
    operator_id: Option<&str>,
    reason: &str,
) -> Result<(), DataLayerError> {
    let now_ms = now_unix_ms() as i64;
    sqlx::query(
        r#"
UPDATE niffler_referral_reward_ledger
SET status = 'failed',
    failure_reason = $2,
    retry_count = retry_count + 1,
    updated_at_unix_ms = $3
WHERE id = $1 AND status IN ('pending', 'failed')
"#,
    )
    .bind(&ledger.id)
    .bind(reason)
    .bind(now_ms)
    .execute(&mut **tx)
    .await
    .map_err(DataLayerError::postgres)?;
    for reward in rewards {
        sqlx::query(
            r#"
UPDATE referral_rewards
SET status = 'ledger_failed',
    admin_operator_id = COALESCE($2, admin_operator_id),
    admin_note = COALESCE($3, admin_note),
    updated_at = NOW()
WHERE id = $1 AND status IN ('ledger_pending', 'ledger_failed')
"#,
        )
        .bind(&reward.id)
        .bind(operator_id)
        .bind(reason)
        .execute(&mut **tx)
        .await
        .map_err(DataLayerError::postgres)?;
    }
    sqlx::query(
        r#"
INSERT INTO niffler_referral_reward_events (
  id, reward_ledger_id, event_kind, reason, actor_id, event_snapshot, created_at_unix_ms
)
VALUES ($1, $2, 'failed', $3, $4, $5, $6)
"#,
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&ledger.id)
    .bind(reason)
    .bind(operator_id)
    .bind(niffler_referral_failed_event_snapshot(
        rewards,
        ledger.reward_amount_usd,
    ))
    .bind(now_ms)
    .execute(&mut **tx)
    .await
    .map_err(DataLayerError::postgres)?;
    Ok(())
}

fn niffler_referral_paid_event_snapshot(
    wallet_transaction_id: &str,
    rewards: &[ReferralRewardRecord],
    reward_amount_usd: f64,
) -> serde_json::Value {
    serde_json::json!({
        "wallet_transaction_id": wallet_transaction_id,
        "reward_amount_usd": reward_amount_usd,
        "legacy_reward_ids": rewards.iter().map(|reward| reward.id.clone()).collect::<Vec<_>>(),
        "legacy_reward_idempotency_keys": rewards.iter().map(|reward| reward.idempotency_key.clone()).collect::<Vec<_>>(),
    })
}

fn niffler_referral_failed_event_snapshot(
    rewards: &[ReferralRewardRecord],
    reward_amount_usd: f64,
) -> serde_json::Value {
    serde_json::json!({
        "reward_amount_usd": reward_amount_usd,
        "legacy_reward_ids": rewards.iter().map(|reward| reward.id.clone()).collect::<Vec<_>>(),
        "legacy_reward_idempotency_keys": rewards.iter().map(|reward| reward.idempotency_key.clone()).collect::<Vec<_>>(),
    })
}

fn payment_order_context_from_row<R>(row: R) -> Result<ReferralPaymentOrderContext, DataLayerError>
where
    R: Row,
    for<'c> &'c str: sqlx::ColumnIndex<R>,
    for<'r> String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> f64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    let Some(user_id) = row_optional_string!(row, "user_id") else {
        return Err(DataLayerError::InvalidInput(
            "payment order has no user_id".to_string(),
        ));
    };
    Ok(ReferralPaymentOrderContext {
        id: row_string!(row, "id"),
        user_id,
        amount_usd: row_f64!(row, "amount_usd"),
        payment_method: row_string!(row, "payment_method"),
        status: row_string!(row, "status"),
        order_kind: row_string!(row, "order_kind"),
    })
}

fn payment_order_refund_context_from_row<R>(
    row: R,
) -> Result<ReferralPaymentOrderRefundContext, DataLayerError>
where
    R: Row,
    for<'c> &'c str: sqlx::ColumnIndex<R>,
    for<'r> f64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    Ok(ReferralPaymentOrderRefundContext {
        amount_usd: row_f64!(row, "amount_usd"),
        refunded_amount_usd: row_f64!(row, "refunded_amount_usd"),
    })
}

fn credit_target_from_row<R>(row: R) -> Result<ReferralCreditTarget, DataLayerError>
where
    R: Row,
    for<'c> &'c str: sqlx::ColumnIndex<R>,
    for<'r> String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> f64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    Ok(ReferralCreditTarget {
        id: row_string!(row, "id"),
        wallet_id: row_string!(row, "wallet_id"),
        inviter_user_id: row_string!(row, "inviter_user_id"),
        invitee_user_id: row_string!(row, "invitee_user_id"),
        amount_usd: row_f64!(row, "amount_usd"),
        reward_type: row_string!(row, "reward_type"),
        trigger_point: row_string!(row, "trigger_point"),
    })
}

fn niffler_referral_ledger_payment_target_from_row<R>(
    row: R,
) -> Result<NifflerReferralLedgerPaymentTarget, DataLayerError>
where
    R: Row,
    for<'c> &'c str: sqlx::ColumnIndex<R>,
    for<'r> String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> f64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    Ok(NifflerReferralLedgerPaymentTarget {
        id: row_string!(row, "id"),
        order_id: row_string!(row, "order_id"),
        inviter_user_id: row_string!(row, "inviter_user_id"),
        invitee_user_id: row_string!(row, "invitee_user_id"),
        reward_amount_usd: row_f64!(row, "reward_amount_usd"),
        status: row_string!(row, "status"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn referral_retry_only_allows_failed_rewards() {
        assert!(referral_retry_allowed("failed"));

        for status in [
            "pending",
            "applied",
            "reversed",
            "voided",
            "ledger_pending",
            "ledger_failed",
        ] {
            assert!(!referral_retry_allowed(status), "{status} must not retry");
        }
    }

    #[test]
    fn referral_void_only_allows_pending_and_failed_rewards() {
        assert!(referral_void_allowed("pending"));
        assert!(referral_void_allowed("failed"));

        for status in [
            "applied",
            "reversed",
            "voided",
            "ledger_pending",
            "ledger_failed",
        ] {
            assert!(!referral_void_allowed(status), "{status} must not void");
        }
    }

    #[test]
    fn referral_reversal_delta_uses_cumulative_refund_ratio() {
        let first = referral_reversal_delta(10.0, 100.0, 20.0, 0.0, 0.0);
        assert!((first - 2.0).abs() < f64::EPSILON);

        let second = referral_reversal_delta(10.0, 100.0, 50.0, 2.0, 0.0);
        assert!((second - 3.0).abs() < f64::EPSILON);

        let repeated = referral_reversal_delta(10.0, 100.0, 50.0, 2.0, 3.0);
        assert_eq!(repeated, 0.0);

        let full = referral_reversal_delta(10.0, 100.0, 125.0, 5.0, 0.0);
        assert!((full - 5.0).abs() < f64::EPSILON);
    }
}

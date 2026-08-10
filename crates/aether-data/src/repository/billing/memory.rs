use std::collections::BTreeMap;
use std::sync::RwLock;

use async_trait::async_trait;

use super::quota::{entitlement_allows_global_model, quota_base_amount};
use super::{
    AdminBillingMutationOutcome, BillingPlanRecord, BillingPlanWriteInput, BillingReadRepository,
    PaymentGatewayConfigRecord, PaymentGatewayConfigWriteInput, StoredBillingModelContext,
    UserDailyQuotaAvailabilityRecord, UserPlanEntitlementRecord, UserPlanEntitlementUpdateInput,
};
use crate::DataLayerError;

type BillingContextKey = (String, String, Option<String>);
type BillingContextMap = BTreeMap<BillingContextKey, StoredBillingModelContext>;

#[derive(Debug, Default)]
pub struct InMemoryBillingReadRepository {
    by_key: RwLock<BillingContextMap>,
    gateway_configs_by_provider: RwLock<BTreeMap<String, PaymentGatewayConfigRecord>>,
    billing_plans_by_id: RwLock<BTreeMap<String, BillingPlanRecord>>,
    entitlements_by_id: RwLock<BTreeMap<String, UserPlanEntitlementRecord>>,
}

impl InMemoryBillingReadRepository {
    pub fn seed<I>(items: I) -> Self
    where
        I: IntoIterator<Item = StoredBillingModelContext>,
    {
        let mut by_key = BTreeMap::new();
        for item in items {
            by_key.insert(
                (
                    item.provider_id.clone(),
                    item.global_model_name.clone(),
                    item.provider_api_key_id.clone(),
                ),
                item,
            );
        }
        Self {
            by_key: RwLock::new(by_key),
            gateway_configs_by_provider: RwLock::new(BTreeMap::new()),
            billing_plans_by_id: RwLock::new(BTreeMap::new()),
            entitlements_by_id: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn with_billing_plans<I>(self, plans: I) -> Self
    where
        I: IntoIterator<Item = BillingPlanRecord>,
    {
        {
            let mut records = self
                .billing_plans_by_id
                .write()
                .expect("billing repository lock");
            for plan in plans {
                records.insert(plan.id.clone(), plan);
            }
        }
        self
    }

    pub fn with_user_plan_entitlements<I>(self, entitlements: I) -> Self
    where
        I: IntoIterator<Item = UserPlanEntitlementRecord>,
    {
        {
            let mut records = self
                .entitlements_by_id
                .write()
                .expect("billing repository lock");
            for entitlement in entitlements {
                records.insert(entitlement.id.clone(), entitlement);
            }
        }
        self
    }
}

fn current_unix_secs() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

fn billing_plan_from_input(
    id: String,
    input: &BillingPlanWriteInput,
    created_at: u64,
) -> BillingPlanRecord {
    BillingPlanRecord {
        id,
        title: input.title.clone(),
        description: input.description.clone(),
        price_amount: input.price_amount,
        price_currency: input.price_currency.clone(),
        duration_unit: input.duration_unit.clone(),
        duration_value: input.duration_value,
        enabled: input.enabled,
        sort_order: input.sort_order,
        max_active_per_user: input.max_active_per_user,
        purchase_limit_scope: input.purchase_limit_scope.clone(),
        allowed_provider_ids: input.allowed_provider_ids.clone(),
        entitlements_json: input.entitlements_json.clone(),
        created_at_unix_secs: created_at,
        updated_at_unix_secs: current_unix_secs(),
    }
}

fn daily_quota_availability_from_entitlements(
    entitlements: impl IntoIterator<Item = UserPlanEntitlementRecord>,
    now: u64,
    global_model_id: Option<&str>,
) -> UserDailyQuotaAvailabilityRecord {
    let mut has_active_daily_quota = false;
    let mut total_quota_usd = 0.0;
    let used_usd = 0.0;
    let mut remaining_usd = 0.0;
    let mut base_remaining_usd = 0.0;
    let mut allow_wallet_overage = true;
    let mut eligible_entitlement_ids = Vec::new();
    let mut allowed_provider_ids = std::collections::BTreeSet::new();
    let mut provider_ids_by_entitlement = std::collections::BTreeMap::new();
    for entitlement in entitlements {
        if entitlement.status != "active"
            || entitlement.starts_at_unix_secs > now
            || entitlement.expires_at_unix_secs <= now
        {
            continue;
        }
        let Some(items) = entitlement.entitlements_snapshot.as_array() else {
            continue;
        };
        for item in items {
            if item.get("type").and_then(serde_json::Value::as_str) != Some("daily_quota") {
                continue;
            }
            let allowed_global_model_ids = item
                .get("allowed_global_model_ids")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>()
                })
                .filter(|items| !items.is_empty());
            if global_model_id.is_some()
                && !entitlement_allows_global_model(
                    allowed_global_model_ids.as_deref(),
                    global_model_id,
                )
            {
                continue;
            }
            let daily_quota_usd = item
                .get("daily_quota_usd")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            if !daily_quota_usd.is_finite() || daily_quota_usd <= 0.0 {
                continue;
            }
            has_active_daily_quota = true;
            if !eligible_entitlement_ids.contains(&entitlement.id) {
                eligible_entitlement_ids.push(entitlement.id.clone());
                allowed_provider_ids.extend(entitlement.allowed_provider_ids.iter().cloned());
                provider_ids_by_entitlement.insert(
                    entitlement.id.clone(),
                    entitlement.allowed_provider_ids.clone(),
                );
            }
            total_quota_usd += daily_quota_usd;
            remaining_usd += daily_quota_usd;
            let quota_multiplier = item
                .get("quota_multiplier")
                .or_else(|| item.get("package_multiplier"))
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(1.0);
            base_remaining_usd += quota_base_amount(daily_quota_usd, quota_multiplier);
            allow_wallet_overage &= item
                .get("allow_wallet_overage")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
        }
    }
    UserDailyQuotaAvailabilityRecord {
        has_active_daily_quota,
        total_quota_usd,
        used_usd,
        remaining_usd,
        base_remaining_usd,
        allow_wallet_overage,
        eligible_entitlement_ids,
        allowed_provider_ids: allowed_provider_ids.into_iter().collect(),
        provider_ids_by_entitlement,
    }
}

#[async_trait]
impl BillingReadRepository for InMemoryBillingReadRepository {
    async fn find_model_context(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        global_model_name: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        let by_key = self.by_key.read().expect("billing repository lock");
        if let Some(value) = find_context_by_provider_model_name(
            &by_key,
            provider_id,
            provider_api_key_id,
            global_model_name,
        ) {
            return Ok(Some(value));
        }

        let key = (
            provider_id.to_string(),
            global_model_name.to_string(),
            provider_api_key_id.map(ToOwned::to_owned),
        );
        if let Some(value) = by_key.get(&key) {
            return Ok(Some(value.clone()));
        }

        if let Some(value) = by_key
            .get(&(provider_id.to_string(), global_model_name.to_string(), None))
            .cloned()
        {
            return Ok(Some(value));
        }

        Ok(by_key
            .iter()
            .find(|((stored_provider_id, stored_model_name, _), _)| {
                stored_provider_id == provider_id && stored_model_name == global_model_name
            })
            .map(|(_, value)| value.clone()))
    }

    async fn find_model_context_by_model_id(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        model_id: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        let by_key = self.by_key.read().expect("billing repository lock");
        if let Some(value) =
            find_context_by_model_id_and_key(&by_key, provider_id, provider_api_key_id, model_id)
        {
            return Ok(Some(value));
        }
        if let Some(value) = find_context_by_model_id_and_key(&by_key, provider_id, None, model_id)
        {
            return Ok(Some(value));
        }
        Ok(by_key
            .iter()
            .find(|((stored_provider_id, _, _), value)| {
                stored_provider_id == provider_id && value.model_id.as_deref() == Some(model_id)
            })
            .map(|(_, value)| value.clone()))
    }

    async fn find_payment_gateway_config(
        &self,
        provider: &str,
    ) -> Result<Option<PaymentGatewayConfigRecord>, DataLayerError> {
        Ok(self
            .gateway_configs_by_provider
            .read()
            .expect("billing repository lock")
            .get(&provider.trim().to_ascii_lowercase())
            .cloned())
    }

    async fn upsert_payment_gateway_config(
        &self,
        input: &PaymentGatewayConfigWriteInput,
    ) -> Result<AdminBillingMutationOutcome<PaymentGatewayConfigRecord>, DataLayerError> {
        let provider = input.provider.trim().to_ascii_lowercase();
        let now = current_unix_secs();
        let mut configs = self
            .gateway_configs_by_provider
            .write()
            .expect("billing repository lock");
        let created_at = configs
            .get(&provider)
            .map(|value| value.created_at_unix_secs)
            .unwrap_or(now);
        let merchant_key_encrypted = if input.preserve_existing_secret {
            configs
                .get(&provider)
                .and_then(|value| value.merchant_key_encrypted.clone())
        } else {
            input.merchant_key_encrypted.clone()
        };
        let webhook_secret_encrypted = if input.preserve_existing_webhook_secret {
            configs
                .get(&provider)
                .and_then(|value| value.webhook_secret_encrypted.clone())
        } else {
            input.webhook_secret_encrypted.clone()
        };
        let record = PaymentGatewayConfigRecord {
            provider: provider.clone(),
            enabled: input.enabled,
            endpoint_url: input.endpoint_url.clone(),
            callback_base_url: input.callback_base_url.clone(),
            merchant_id: input.merchant_id.clone(),
            merchant_key_encrypted,
            webhook_secret_encrypted,
            pay_currency: input.pay_currency.clone(),
            usd_exchange_rate: input.usd_exchange_rate,
            min_recharge_usd: input.min_recharge_usd,
            channels_json: input.channels_json.clone(),
            created_at_unix_secs: created_at,
            updated_at_unix_secs: now,
        };
        configs.insert(provider, record.clone());
        Ok(AdminBillingMutationOutcome::Applied(record))
    }

    async fn list_billing_plans(
        &self,
        include_disabled: bool,
    ) -> Result<Option<Vec<BillingPlanRecord>>, DataLayerError> {
        let mut items = self
            .billing_plans_by_id
            .read()
            .expect("billing repository lock")
            .values()
            .filter(|item| include_disabled || item.enabled)
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.sort_order
                .cmp(&right.sort_order)
                .then_with(|| left.price_amount.total_cmp(&right.price_amount))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(Some(items))
    }

    async fn find_billing_plan(
        &self,
        plan_id: &str,
    ) -> Result<Option<BillingPlanRecord>, DataLayerError> {
        Ok(self
            .billing_plans_by_id
            .read()
            .expect("billing repository lock")
            .get(plan_id)
            .cloned())
    }

    async fn create_billing_plan(
        &self,
        input: &BillingPlanWriteInput,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, DataLayerError> {
        let id = uuid::Uuid::new_v4().to_string();
        let record = billing_plan_from_input(id.clone(), input, current_unix_secs());
        self.billing_plans_by_id
            .write()
            .expect("billing repository lock")
            .insert(id, record.clone());
        Ok(AdminBillingMutationOutcome::Applied(record))
    }

    async fn update_billing_plan(
        &self,
        plan_id: &str,
        input: &BillingPlanWriteInput,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, DataLayerError> {
        let mut plans = self
            .billing_plans_by_id
            .write()
            .expect("billing repository lock");
        let Some(existing) = plans.get(plan_id).cloned() else {
            return Ok(AdminBillingMutationOutcome::NotFound);
        };
        let record =
            billing_plan_from_input(plan_id.to_string(), input, existing.created_at_unix_secs);
        plans.insert(plan_id.to_string(), record.clone());
        Ok(AdminBillingMutationOutcome::Applied(record))
    }

    async fn set_billing_plan_enabled(
        &self,
        plan_id: &str,
        enabled: bool,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, DataLayerError> {
        let mut plans = self
            .billing_plans_by_id
            .write()
            .expect("billing repository lock");
        let Some(record) = plans.get_mut(plan_id) else {
            return Ok(AdminBillingMutationOutcome::NotFound);
        };
        record.enabled = enabled;
        record.updated_at_unix_secs = current_unix_secs();
        Ok(AdminBillingMutationOutcome::Applied(record.clone()))
    }

    async fn delete_billing_plan(
        &self,
        plan_id: &str,
    ) -> Result<AdminBillingMutationOutcome<()>, DataLayerError> {
        let mut plans = self
            .billing_plans_by_id
            .write()
            .expect("billing repository lock");
        if !plans.contains_key(plan_id) {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        let has_entitlements = self
            .entitlements_by_id
            .read()
            .expect("billing repository lock")
            .values()
            .any(|item| item.plan_id == plan_id);
        if has_entitlements {
            return Ok(AdminBillingMutationOutcome::Invalid(
                "套餐已有订单或权益，不能删除，请停用该套餐".to_string(),
            ));
        }
        plans.remove(plan_id);
        Ok(AdminBillingMutationOutcome::Applied(()))
    }

    async fn list_user_plan_entitlements(
        &self,
        user_id: &str,
    ) -> Result<Option<Vec<UserPlanEntitlementRecord>>, DataLayerError> {
        let mut items = self
            .entitlements_by_id
            .read()
            .expect("billing repository lock")
            .values()
            .filter(|item| item.user_id == user_id)
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by_key(|item| {
            std::cmp::Reverse((item.created_at_unix_secs, item.expires_at_unix_secs))
        });
        Ok(Some(items))
    }

    async fn cancel_user_plan_entitlement(
        &self,
        user_id: &str,
        entitlement_id: &str,
    ) -> Result<AdminBillingMutationOutcome<UserPlanEntitlementRecord>, DataLayerError> {
        let now = current_unix_secs();
        let mut records = self
            .entitlements_by_id
            .write()
            .expect("billing repository lock");
        let Some(existing) = records.get(entitlement_id) else {
            return Ok(AdminBillingMutationOutcome::NotFound);
        };
        if existing.user_id != user_id
            || existing.status != "active"
            || existing.expires_at_unix_secs <= now
        {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        let record = records
            .get_mut(entitlement_id)
            .expect("validated entitlement should still exist");
        record.status = "cancelled".to_string();
        record.expires_at_unix_secs = record.expires_at_unix_secs.min(now);
        record.updated_at_unix_secs = now;
        Ok(AdminBillingMutationOutcome::Applied(record.clone()))
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
        let now = current_unix_secs();
        let mut records = self
            .entitlements_by_id
            .write()
            .expect("billing repository lock");
        let Some(existing) = records.get(entitlement_id) else {
            return Ok(AdminBillingMutationOutcome::NotFound);
        };
        if existing.user_id != user_id
            || existing.status != "active"
            || existing.expires_at_unix_secs <= now
        {
            return Ok(AdminBillingMutationOutcome::NotFound);
        }
        if records.values().any(|item| {
            item.id != entitlement_id
                && item.user_id == user_id
                && item.status == "active"
                && item.starts_at_unix_secs < input.expires_at_unix_secs
                && item.expires_at_unix_secs > input.starts_at_unix_secs
        }) {
            return Ok(AdminBillingMutationOutcome::Invalid(
                "同一时间只能有一个生效套餐".to_string(),
            ));
        }
        let record = records
            .get_mut(entitlement_id)
            .expect("validated entitlement should still exist");
        record.starts_at_unix_secs = input.starts_at_unix_secs;
        record.expires_at_unix_secs = input.expires_at_unix_secs;
        if let Some(entitlements_snapshot) = input.entitlements_snapshot.as_ref() {
            record.entitlements_snapshot = entitlements_snapshot.clone();
        }
        if let Some(allowed_provider_ids) = input.allowed_provider_ids.as_ref() {
            record.allowed_provider_ids = allowed_provider_ids.clone();
        }
        record.updated_at_unix_secs = now;
        Ok(AdminBillingMutationOutcome::Applied(record.clone()))
    }

    async fn find_user_daily_quota_availability(
        &self,
        user_id: &str,
    ) -> Result<Option<UserDailyQuotaAvailabilityRecord>, DataLayerError> {
        self.find_user_daily_quota_availability_for_global_model(user_id, None)
            .await
    }

    async fn find_user_daily_quota_availability_for_global_model(
        &self,
        user_id: &str,
        global_model_id: Option<&str>,
    ) -> Result<Option<UserDailyQuotaAvailabilityRecord>, DataLayerError> {
        let now = current_unix_secs();
        let entitlements = self
            .entitlements_by_id
            .read()
            .expect("billing repository lock")
            .values()
            .filter(|item| item.user_id == user_id)
            .cloned()
            .collect::<Vec<_>>();
        Ok(Some(daily_quota_availability_from_entitlements(
            entitlements,
            now,
            global_model_id,
        )))
    }
}

fn find_context_by_provider_model_name(
    by_key: &BillingContextMap,
    provider_id: &str,
    provider_api_key_id: Option<&str>,
    provider_model_name: &str,
) -> Option<StoredBillingModelContext> {
    find_context_by_provider_model_name_and_key(
        by_key,
        provider_id,
        provider_api_key_id,
        provider_model_name,
    )
    .or_else(|| {
        find_context_by_provider_model_name_and_key(by_key, provider_id, None, provider_model_name)
    })
    .or_else(|| {
        by_key
            .iter()
            .find(|((stored_provider_id, _, _), value)| {
                stored_provider_id == provider_id
                    && value.model_provider_model_name.as_deref() == Some(provider_model_name)
            })
            .map(|(_, value)| value.clone())
    })
}

fn find_context_by_provider_model_name_and_key(
    by_key: &BillingContextMap,
    provider_id: &str,
    provider_api_key_id: Option<&str>,
    provider_model_name: &str,
) -> Option<StoredBillingModelContext> {
    by_key
        .iter()
        .find(|((stored_provider_id, _, stored_key_id), value)| {
            stored_provider_id == provider_id
                && stored_key_id.as_deref() == provider_api_key_id
                && value.model_provider_model_name.as_deref() == Some(provider_model_name)
        })
        .map(|(_, value)| value.clone())
}

fn find_context_by_model_id_and_key(
    by_key: &BillingContextMap,
    provider_id: &str,
    provider_api_key_id: Option<&str>,
    model_id: &str,
) -> Option<StoredBillingModelContext> {
    by_key
        .iter()
        .find(|((stored_provider_id, _, stored_key_id), value)| {
            stored_provider_id == provider_id
                && stored_key_id.as_deref() == provider_api_key_id
                && value.model_id.as_deref() == Some(model_id)
        })
        .map(|(_, value)| value.clone())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::InMemoryBillingReadRepository;
    use crate::repository::billing::{BillingReadRepository, StoredBillingModelContext};

    fn sample_context() -> StoredBillingModelContext {
        StoredBillingModelContext::new(
            "provider-1".to_string(),
            Some("pay_as_you_go".to_string()),
            None,
            Some("key-1".to_string()),
            Some(json!({"openai:chat": 0.8})),
            Some(60),
            "global-model-1".to_string(),
            "gpt-5".to_string(),
            Some(json!({"streaming": true})),
            Some(0.02),
            Some(json!({"tiers":[{"up_to":null,"input_price_per_1m":3.0,"output_price_per_1m":15.0}]})),
            Some("model-1".to_string()),
            Some("gpt-5-upstream".to_string()),
            None,
            Some(0.01),
            None,
        )
        .expect("billing context should build")
    }

    #[tokio::test]
    async fn falls_back_to_provider_without_key_scope() {
        let repository = InMemoryBillingReadRepository::seed(vec![sample_context()]);
        let stored = repository
            .find_model_context("provider-1", Some("key-2"), "gpt-5")
            .await
            .expect("lookup should succeed")
            .expect("context should exist");

        assert_eq!(stored.provider_id, "provider-1");
        assert_eq!(stored.global_model_name, "gpt-5");
    }

    #[tokio::test]
    async fn resolves_by_provider_model_name_before_global_name_collision() {
        let global_named_context = StoredBillingModelContext::new(
            "provider-1".to_string(),
            Some("pay_as_you_go".to_string()),
            None,
            Some("key-1".to_string()),
            None,
            Some(60),
            "global-model-blank".to_string(),
            "claude-sonnet-4-6".to_string(),
            None,
            None,
            None,
            Some("model-blank".to_string()),
            Some("blank-upstream".to_string()),
            None,
            None,
            None,
        )
        .expect("blank billing context should build");
        let provider_priced_context = StoredBillingModelContext::new(
            "provider-1".to_string(),
            Some("pay_as_you_go".to_string()),
            None,
            Some("key-1".to_string()),
            None,
            Some(60),
            "global-model-priced".to_string(),
            "claude-opus-4-6".to_string(),
            None,
            None,
            Some(json!({"tiers":[{"up_to":null,"input_price_per_1m":3.0,"output_price_per_1m":15.0}]})),
            Some("model-priced".to_string()),
            Some("claude-sonnet-4-6".to_string()),
            None,
            None,
            None,
        )
        .expect("priced billing context should build");
        let repository = InMemoryBillingReadRepository::seed(vec![
            global_named_context,
            provider_priced_context,
        ]);

        let stored = repository
            .find_model_context("provider-1", Some("key-1"), "claude-sonnet-4-6")
            .await
            .expect("lookup should succeed")
            .expect("context should exist");

        assert_eq!(stored.global_model_name, "claude-opus-4-6");
        assert_eq!(
            stored.model_provider_model_name.as_deref(),
            Some("claude-sonnet-4-6")
        );
        assert!(stored.default_tiered_pricing.is_some());
    }

    #[tokio::test]
    async fn resolves_by_model_id() {
        let repository = InMemoryBillingReadRepository::seed(vec![sample_context()]);
        let stored = repository
            .find_model_context_by_model_id("provider-1", Some("key-1"), "model-1")
            .await
            .expect("lookup should succeed")
            .expect("context should exist");

        assert_eq!(stored.global_model_name, "gpt-5");
        assert_eq!(
            stored.model_provider_model_name.as_deref(),
            Some("gpt-5-upstream")
        );
    }
}

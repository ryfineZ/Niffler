use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredBillingModelContext {
    pub provider_id: String,
    pub provider_billing_type: Option<String>,
    pub provider_config: Option<Value>,
    pub provider_api_key_id: Option<String>,
    pub provider_api_key_rate_multipliers: Option<Value>,
    pub provider_api_key_cache_ttl_minutes: Option<i64>,
    pub global_model_id: String,
    pub global_model_name: String,
    pub global_model_config: Option<Value>,
    pub default_price_per_request: Option<f64>,
    pub default_tiered_pricing: Option<Value>,
    pub model_id: Option<String>,
    pub model_provider_model_name: Option<String>,
    pub model_config: Option<Value>,
    pub model_price_per_request: Option<f64>,
    pub model_tiered_pricing: Option<Value>,
}

impl StoredBillingModelContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: String,
        provider_billing_type: Option<String>,
        provider_config: Option<Value>,
        provider_api_key_id: Option<String>,
        provider_api_key_rate_multipliers: Option<Value>,
        provider_api_key_cache_ttl_minutes: Option<i64>,
        global_model_id: String,
        global_model_name: String,
        global_model_config: Option<Value>,
        default_price_per_request: Option<f64>,
        default_tiered_pricing: Option<Value>,
        model_id: Option<String>,
        model_provider_model_name: Option<String>,
        model_config: Option<Value>,
        model_price_per_request: Option<f64>,
        model_tiered_pricing: Option<Value>,
    ) -> Result<Self, crate::DataLayerError> {
        if provider_id.trim().is_empty() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "billing.provider_id is empty".to_string(),
            ));
        }
        if global_model_id.trim().is_empty() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "billing.global_model_id is empty".to_string(),
            ));
        }
        if global_model_name.trim().is_empty() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "billing.global_model_name is empty".to_string(),
            ));
        }
        Ok(Self {
            provider_id,
            provider_billing_type,
            provider_config,
            provider_api_key_id,
            provider_api_key_rate_multipliers,
            provider_api_key_cache_ttl_minutes,
            global_model_id,
            global_model_name,
            global_model_config,
            default_price_per_request,
            default_tiered_pricing,
            model_id,
            model_provider_model_name,
            model_config,
            model_price_per_request,
            model_tiered_pricing,
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AdminBillingRuleRecord {
    pub id: String,
    pub name: String,
    pub task_type: String,
    pub global_model_id: Option<String>,
    pub model_id: Option<String>,
    pub expression: String,
    pub variables: Value,
    pub dimension_mappings: Value,
    pub is_enabled: bool,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdminBillingRuleWriteInput {
    pub name: String,
    pub task_type: String,
    pub global_model_id: Option<String>,
    pub model_id: Option<String>,
    pub expression: String,
    pub variables: Value,
    pub dimension_mappings: Value,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AdminBillingCollectorRecord {
    pub id: String,
    pub api_format: String,
    pub task_type: String,
    pub dimension_name: String,
    pub source_type: String,
    pub source_path: Option<String>,
    pub value_type: String,
    pub transform_expression: Option<String>,
    pub default_value: Option<String>,
    pub priority: i32,
    pub is_enabled: bool,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdminBillingCollectorWriteInput {
    pub api_format: String,
    pub task_type: String,
    pub dimension_name: String,
    pub source_type: String,
    pub source_path: Option<String>,
    pub value_type: String,
    pub transform_expression: Option<String>,
    pub default_value: Option<String>,
    pub priority: i32,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AdminBillingPresetApplyResult {
    pub preset: String,
    pub mode: String,
    pub created: u64,
    pub updated: u64,
    pub skipped: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdminBillingMutationOutcome<T> {
    Applied(T),
    NotFound,
    Invalid(String),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaymentGatewayConfigRecord {
    pub provider: String,
    pub enabled: bool,
    pub endpoint_url: String,
    pub callback_base_url: Option<String>,
    pub merchant_id: String,
    pub merchant_key_encrypted: Option<String>,
    pub webhook_secret_encrypted: Option<String>,
    pub pay_currency: String,
    pub usd_exchange_rate: f64,
    pub min_recharge_usd: f64,
    pub channels_json: Value,
    pub created_at_unix_secs: u64,
    pub updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaymentGatewayConfigWriteInput {
    pub provider: String,
    pub enabled: bool,
    pub endpoint_url: String,
    pub callback_base_url: Option<String>,
    pub merchant_id: String,
    pub merchant_key_encrypted: Option<String>,
    pub preserve_existing_secret: bool,
    pub webhook_secret_encrypted: Option<String>,
    pub preserve_existing_webhook_secret: bool,
    pub pay_currency: String,
    pub usd_exchange_rate: f64,
    pub min_recharge_usd: f64,
    pub channels_json: Value,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BillingPlanRecord {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub price_amount: f64,
    pub price_currency: String,
    pub duration_unit: String,
    pub duration_value: i64,
    pub enabled: bool,
    pub sort_order: i64,
    pub max_active_per_user: i64,
    pub purchase_limit_scope: String,
    #[serde(default)]
    pub allowed_provider_ids: Vec<String>,
    pub entitlements_json: Value,
    pub created_at_unix_secs: u64,
    pub updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BillingPlanWriteInput {
    pub title: String,
    pub description: Option<String>,
    pub price_amount: f64,
    pub price_currency: String,
    pub duration_unit: String,
    pub duration_value: i64,
    pub enabled: bool,
    pub sort_order: i64,
    pub max_active_per_user: i64,
    pub purchase_limit_scope: String,
    pub allowed_provider_ids: Vec<String>,
    pub entitlements_json: Value,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UserPlanEntitlementRecord {
    pub id: String,
    pub user_id: String,
    pub plan_id: String,
    pub payment_order_id: String,
    pub status: String,
    pub starts_at_unix_secs: u64,
    pub expires_at_unix_secs: u64,
    #[serde(default)]
    pub allowed_provider_ids: Vec<String>,
    pub entitlements_snapshot: Value,
    pub created_at_unix_secs: u64,
    pub updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserPlanEntitlementUpdateInput {
    pub starts_at_unix_secs: u64,
    pub expires_at_unix_secs: u64,
    pub allowed_provider_ids: Option<Vec<String>>,
    pub entitlements_snapshot: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingFundingSource {
    Wallet,
    Plan,
    Unlimited,
    Free,
}

impl BillingFundingSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wallet => "wallet",
            Self::Plan => "plan",
            Self::Unlimited => "unlimited",
            Self::Free => "free",
        }
    }

    pub fn from_database(value: &str) -> Result<Self, crate::DataLayerError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "wallet" => Ok(Self::Wallet),
            "plan" => Ok(Self::Plan),
            "unlimited" => Ok(Self::Unlimited),
            "free" => Ok(Self::Free),
            other => Err(crate::DataLayerError::UnexpectedValue(format!(
                "unsupported billing funding source: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BillingRequestAdmissionInput {
    pub request_id: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub wallet_id: Option<String>,
    pub global_model_id: Option<String>,
    pub funding_source: BillingFundingSource,
    pub wallet_balance_at_admission: Option<f64>,
    pub wallet_payment_allowed: bool,
    pub wallet_overage_allowed: bool,
    pub entitlement_ids: Vec<String>,
    #[serde(default)]
    pub entitlement_provider_scopes: std::collections::BTreeMap<String, Vec<String>>,
    pub allowed_provider_ids: Vec<String>,
    pub schema_version: u16,
}

impl BillingRequestAdmissionInput {
    pub fn validate(&self) -> Result<(), crate::DataLayerError> {
        if self.request_id.trim().is_empty() {
            return Err(crate::DataLayerError::InvalidInput(
                "billing admission request_id cannot be empty".to_string(),
            ));
        }
        if self
            .wallet_balance_at_admission
            .is_some_and(|value| !value.is_finite())
        {
            return Err(crate::DataLayerError::InvalidInput(
                "billing admission wallet balance must be finite".to_string(),
            ));
        }
        if self.schema_version == 0 {
            return Err(crate::DataLayerError::InvalidInput(
                "billing admission schema_version must be positive".to_string(),
            ));
        }
        if self.funding_source == BillingFundingSource::Plan {
            if self.entitlement_ids.is_empty() {
                return Err(crate::DataLayerError::InvalidInput(
                    "plan billing admission requires entitlements".to_string(),
                ));
            }
            if self.entitlement_provider_scopes.len() != self.entitlement_ids.len()
                || self.entitlement_ids.iter().any(|entitlement_id| {
                    !self
                        .entitlement_provider_scopes
                        .contains_key(entitlement_id)
                })
            {
                return Err(crate::DataLayerError::InvalidInput(
                    "plan billing admission requires one provider scope per entitlement"
                        .to_string(),
                ));
            }
            if self
                .entitlement_provider_scopes
                .values()
                .any(|provider_ids| {
                    provider_ids.iter().any(|provider_id| {
                        !self
                            .allowed_provider_ids
                            .iter()
                            .any(|allowed| allowed == provider_id)
                    })
                })
            {
                return Err(crate::DataLayerError::InvalidInput(
                    "entitlement provider scope is outside plan billing admission".to_string(),
                ));
            }
            let scoped_provider_ids = self
                .entitlement_provider_scopes
                .values()
                .flatten()
                .collect::<std::collections::BTreeSet<_>>();
            let allowed_provider_ids = self
                .allowed_provider_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>();
            if scoped_provider_ids != allowed_provider_ids {
                return Err(crate::DataLayerError::InvalidInput(
                    "plan billing admission provider list must match entitlement scopes"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BillingRequestAdmissionRecord {
    pub request_id: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub wallet_id: Option<String>,
    pub global_model_id: Option<String>,
    pub funding_source: BillingFundingSource,
    pub wallet_balance_at_admission: Option<f64>,
    pub wallet_payment_allowed: bool,
    pub wallet_overage_allowed: bool,
    pub entitlement_ids: Vec<String>,
    #[serde(default)]
    pub entitlement_provider_scopes: std::collections::BTreeMap<String, Vec<String>>,
    pub allowed_provider_ids: Vec<String>,
    pub billing_admitted: bool,
    pub status: String,
    pub rejection_reason: Option<String>,
    pub schema_version: u16,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl BillingRequestAdmissionRecord {
    pub fn to_input(&self) -> BillingRequestAdmissionInput {
        BillingRequestAdmissionInput {
            request_id: self.request_id.clone(),
            user_id: self.user_id.clone(),
            api_key_id: self.api_key_id.clone(),
            wallet_id: self.wallet_id.clone(),
            global_model_id: self.global_model_id.clone(),
            funding_source: self.funding_source,
            wallet_balance_at_admission: self.wallet_balance_at_admission,
            wallet_payment_allowed: self.wallet_payment_allowed,
            wallet_overage_allowed: self.wallet_overage_allowed,
            entitlement_ids: self.entitlement_ids.clone(),
            entitlement_provider_scopes: self.entitlement_provider_scopes.clone(),
            allowed_provider_ids: self.allowed_provider_ids.clone(),
            schema_version: self.schema_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UserDailyQuotaAvailabilityRecord {
    pub has_active_daily_quota: bool,
    pub total_quota_usd: f64,
    pub used_usd: f64,
    pub remaining_usd: f64,
    #[serde(default)]
    pub base_remaining_usd: f64,
    pub allow_wallet_overage: bool,
    #[serde(default)]
    pub eligible_entitlement_ids: Vec<String>,
    #[serde(default)]
    pub allowed_provider_ids: Vec<String>,
    #[serde(default)]
    pub provider_ids_by_entitlement: std::collections::BTreeMap<String, Vec<String>>,
}

impl UserDailyQuotaAvailabilityRecord {
    pub fn has_legacy_eligible_entitlements(&self) -> bool {
        self.eligible_entitlement_ids.iter().any(|entitlement_id| {
            self.provider_ids_by_entitlement
                .get(entitlement_id)
                .is_none_or(Vec::is_empty)
        })
    }

    pub fn provider_scoped_entitlement_ids_for_provider(&self, provider_id: &str) -> Vec<String> {
        self.eligible_entitlement_ids
            .iter()
            .filter(|entitlement_id| {
                self.provider_ids_by_entitlement
                    .get(*entitlement_id)
                    .is_some_and(|provider_ids| {
                        !provider_ids.is_empty()
                            && provider_ids.iter().any(|allowed| allowed == provider_id)
                    })
            })
            .cloned()
            .collect()
    }

    pub fn eligible_entitlement_ids_for_provider(&self, provider_id: &str) -> Vec<String> {
        if self.provider_ids_by_entitlement.is_empty() {
            return if self.allowed_provider_ids.is_empty()
                || self
                    .allowed_provider_ids
                    .iter()
                    .any(|allowed| allowed == provider_id)
            {
                self.eligible_entitlement_ids.clone()
            } else {
                Vec::new()
            };
        }

        self.eligible_entitlement_ids
            .iter()
            .filter(|entitlement_id| {
                self.provider_ids_by_entitlement
                    .get(*entitlement_id)
                    .is_none_or(|provider_ids| {
                        provider_ids.is_empty()
                            || provider_ids.iter().any(|allowed| allowed == provider_id)
                    })
            })
            .cloned()
            .collect()
    }
}

#[async_trait]
pub trait BillingReadRepository: Send + Sync {
    async fn find_model_context(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        global_model_name: &str,
    ) -> Result<Option<StoredBillingModelContext>, crate::DataLayerError>;

    async fn find_model_context_by_model_id(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        model_id: &str,
    ) -> Result<Option<StoredBillingModelContext>, crate::DataLayerError> {
        let _ = (provider_id, provider_api_key_id, model_id);
        Ok(None)
    }

    async fn admin_billing_enabled_default_value_exists(
        &self,
        api_format: &str,
        task_type: &str,
        dimension_name: &str,
        existing_id: Option<&str>,
    ) -> Result<Option<bool>, crate::DataLayerError> {
        let _ = (api_format, task_type, dimension_name, existing_id);
        Ok(None)
    }

    async fn create_admin_billing_rule(
        &self,
        input: &AdminBillingRuleWriteInput,
    ) -> Result<AdminBillingMutationOutcome<AdminBillingRuleRecord>, crate::DataLayerError> {
        let _ = input;
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn list_admin_billing_rules(
        &self,
        task_type: Option<&str>,
        is_enabled: Option<bool>,
        page: u32,
        page_size: u32,
    ) -> Result<Option<(Vec<AdminBillingRuleRecord>, u64)>, crate::DataLayerError> {
        let _ = (task_type, is_enabled, page, page_size);
        Ok(None)
    }

    async fn find_admin_billing_rule(
        &self,
        rule_id: &str,
    ) -> Result<Option<AdminBillingRuleRecord>, crate::DataLayerError> {
        let _ = rule_id;
        Ok(None)
    }

    async fn update_admin_billing_rule(
        &self,
        rule_id: &str,
        input: &AdminBillingRuleWriteInput,
    ) -> Result<AdminBillingMutationOutcome<AdminBillingRuleRecord>, crate::DataLayerError> {
        let _ = (rule_id, input);
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn create_admin_billing_collector(
        &self,
        input: &AdminBillingCollectorWriteInput,
    ) -> Result<AdminBillingMutationOutcome<AdminBillingCollectorRecord>, crate::DataLayerError>
    {
        let _ = input;
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn list_admin_billing_collectors(
        &self,
        api_format: Option<&str>,
        task_type: Option<&str>,
        dimension_name: Option<&str>,
        is_enabled: Option<bool>,
        page: u32,
        page_size: u32,
    ) -> Result<Option<(Vec<AdminBillingCollectorRecord>, u64)>, crate::DataLayerError> {
        let _ = (
            api_format,
            task_type,
            dimension_name,
            is_enabled,
            page,
            page_size,
        );
        Ok(None)
    }

    async fn find_admin_billing_collector(
        &self,
        collector_id: &str,
    ) -> Result<Option<AdminBillingCollectorRecord>, crate::DataLayerError> {
        let _ = collector_id;
        Ok(None)
    }

    async fn update_admin_billing_collector(
        &self,
        collector_id: &str,
        input: &AdminBillingCollectorWriteInput,
    ) -> Result<AdminBillingMutationOutcome<AdminBillingCollectorRecord>, crate::DataLayerError>
    {
        let _ = (collector_id, input);
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn apply_admin_billing_preset(
        &self,
        preset: &str,
        mode: &str,
        collectors: &[AdminBillingCollectorWriteInput],
    ) -> Result<AdminBillingMutationOutcome<AdminBillingPresetApplyResult>, crate::DataLayerError>
    {
        let _ = (preset, mode, collectors);
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn find_payment_gateway_config(
        &self,
        provider: &str,
    ) -> Result<Option<PaymentGatewayConfigRecord>, crate::DataLayerError> {
        let _ = provider;
        Ok(None)
    }

    async fn upsert_payment_gateway_config(
        &self,
        input: &PaymentGatewayConfigWriteInput,
    ) -> Result<AdminBillingMutationOutcome<PaymentGatewayConfigRecord>, crate::DataLayerError>
    {
        let _ = input;
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn list_billing_plans(
        &self,
        include_disabled: bool,
    ) -> Result<Option<Vec<BillingPlanRecord>>, crate::DataLayerError> {
        let _ = include_disabled;
        Ok(None)
    }

    async fn find_billing_plan(
        &self,
        plan_id: &str,
    ) -> Result<Option<BillingPlanRecord>, crate::DataLayerError> {
        let _ = plan_id;
        Ok(None)
    }

    async fn create_billing_plan(
        &self,
        input: &BillingPlanWriteInput,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, crate::DataLayerError> {
        let _ = input;
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn update_billing_plan(
        &self,
        plan_id: &str,
        input: &BillingPlanWriteInput,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, crate::DataLayerError> {
        let _ = (plan_id, input);
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn set_billing_plan_enabled(
        &self,
        plan_id: &str,
        enabled: bool,
    ) -> Result<AdminBillingMutationOutcome<BillingPlanRecord>, crate::DataLayerError> {
        let _ = (plan_id, enabled);
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn delete_billing_plan(
        &self,
        plan_id: &str,
    ) -> Result<AdminBillingMutationOutcome<()>, crate::DataLayerError> {
        let _ = plan_id;
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn list_user_plan_entitlements(
        &self,
        user_id: &str,
    ) -> Result<Option<Vec<UserPlanEntitlementRecord>>, crate::DataLayerError> {
        let _ = user_id;
        Ok(None)
    }

    async fn cancel_user_plan_entitlement(
        &self,
        user_id: &str,
        entitlement_id: &str,
    ) -> Result<AdminBillingMutationOutcome<UserPlanEntitlementRecord>, crate::DataLayerError> {
        let _ = (user_id, entitlement_id);
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn update_user_plan_entitlement(
        &self,
        user_id: &str,
        entitlement_id: &str,
        input: &UserPlanEntitlementUpdateInput,
    ) -> Result<AdminBillingMutationOutcome<UserPlanEntitlementRecord>, crate::DataLayerError> {
        let _ = (user_id, entitlement_id, input);
        Ok(AdminBillingMutationOutcome::Unavailable)
    }

    async fn find_user_daily_quota_availability(
        &self,
        user_id: &str,
    ) -> Result<Option<UserDailyQuotaAvailabilityRecord>, crate::DataLayerError> {
        let _ = user_id;
        Ok(None)
    }

    async fn find_user_daily_quota_availability_for_global_model(
        &self,
        user_id: &str,
        global_model_id: Option<&str>,
    ) -> Result<Option<UserDailyQuotaAvailabilityRecord>, crate::DataLayerError> {
        let _ = global_model_id;
        self.find_user_daily_quota_availability(user_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BillingFundingSource, BillingRequestAdmissionInput, UserDailyQuotaAvailabilityRecord,
    };

    fn plan_admission() -> BillingRequestAdmissionInput {
        BillingRequestAdmissionInput {
            request_id: "request-1".to_string(),
            user_id: Some("user-1".to_string()),
            api_key_id: Some("key-1".to_string()),
            wallet_id: Some("wallet-1".to_string()),
            global_model_id: Some("global-model-1".to_string()),
            funding_source: BillingFundingSource::Plan,
            wallet_balance_at_admission: Some(-1.0),
            wallet_payment_allowed: false,
            wallet_overage_allowed: true,
            entitlement_ids: vec!["entitlement-1".to_string()],
            entitlement_provider_scopes: std::collections::BTreeMap::from([(
                "entitlement-1".to_string(),
                vec!["provider-1".to_string()],
            )]),
            allowed_provider_ids: vec!["provider-1".to_string()],
            schema_version: 1,
        }
    }

    #[test]
    fn plan_admission_accepts_negative_wallet_with_matching_provider() {
        plan_admission().validate().expect("admission should pass");
    }

    #[test]
    fn plan_admission_rejects_provider_scope_outside_allowed_set() {
        let mut admission = plan_admission();
        admission
            .entitlement_provider_scopes
            .insert("entitlement-1".to_string(), vec!["provider-2".to_string()]);
        assert!(admission.validate().is_err());
    }

    #[test]
    fn plan_admission_rejects_extra_provider_without_entitlement_scope() {
        let mut admission = plan_admission();
        admission
            .allowed_provider_ids
            .push("provider-wallet".to_string());

        assert!(admission.validate().is_err());
    }

    #[test]
    fn legacy_plan_admission_accepts_empty_provider_scope() {
        let mut admission = plan_admission();
        admission
            .entitlement_provider_scopes
            .insert("entitlement-1".to_string(), Vec::new());
        admission.allowed_provider_ids.clear();

        admission.validate().expect("legacy admission should pass");
    }

    #[test]
    fn quota_selects_only_entitlements_that_cover_the_actual_provider() {
        let quota = UserDailyQuotaAvailabilityRecord {
            has_active_daily_quota: true,
            total_quota_usd: 20.0,
            used_usd: 0.0,
            remaining_usd: 20.0,
            base_remaining_usd: 20.0,
            allow_wallet_overage: true,
            eligible_entitlement_ids: vec![
                "entitlement-a".to_string(),
                "entitlement-b".to_string(),
                "legacy-entitlement".to_string(),
            ],
            allowed_provider_ids: vec!["provider-a".to_string(), "provider-b".to_string()],
            provider_ids_by_entitlement: std::collections::BTreeMap::from([
                ("entitlement-a".to_string(), vec!["provider-a".to_string()]),
                ("entitlement-b".to_string(), vec!["provider-b".to_string()]),
                ("legacy-entitlement".to_string(), Vec::new()),
            ]),
        };

        assert_eq!(
            quota.eligible_entitlement_ids_for_provider("provider-a"),
            vec![
                "entitlement-a".to_string(),
                "legacy-entitlement".to_string()
            ]
        );
    }
}

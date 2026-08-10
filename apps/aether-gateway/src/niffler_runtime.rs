use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use aether_data_contracts::repository::niffler_core::{
    NifflerProductPlanModelListQuery, NifflerRuntimeAccountModelAccessListQuery,
    NifflerRuntimeRolloutTargetScope, StoredNifflerProductPlanModel,
    StoredNifflerRuntimeRolloutSetting,
};

use crate::clock::current_unix_ms;
use crate::{AppState, GatewayError};

const RUNTIME_ROLLOUT_DECISION_CACHE_TTL: Duration = Duration::from_secs(30);
const RUNTIME_ROLLOUT_DECISION_CACHE_MAX_ENTRIES: usize = 2_048;
const RUNTIME_POLICY_SNAPSHOT_CACHE_TTL: Duration = Duration::from_secs(10);
const RUNTIME_POLICY_SNAPSHOT_CACHE_MAX_ENTRIES: usize = 2_048;
const RUNTIME_MODEL_ACCESS_CACHE_TTL: Duration = Duration::from_secs(5);
const RUNTIME_MODEL_ACCESS_CACHE_MAX_ENTRIES: usize = 8_192;
const RUNTIME_PRODUCT_PLAN_MODEL_PAGE_SIZE: usize = 500;
const RUNTIME_ACCOUNT_MODEL_ACCESS_PAGE_SIZE: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NifflerRuntimeRolloutDecisionSource {
    ApiKey,
    ProductPlan,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NifflerRuntimeRolloutDecision {
    pub(crate) api_key_id: String,
    pub(crate) product_plan_id: Option<String>,
    pub(crate) source: Option<NifflerRuntimeRolloutDecisionSource>,
    pub(crate) enable_new_routing: bool,
    pub(crate) enable_settlement_snapshot: bool,
    pub(crate) enable_error_return_rules: bool,
    pub(crate) enable_billing_reservation: bool,
    pub(crate) enable_referral_ledger: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NifflerRuntimePolicySnapshot {
    pub(crate) api_key_id: String,
    pub(crate) product_plan_id: Option<String>,
    pub(crate) product_plan_name: Option<String>,
    pub(crate) allowed_models: Vec<String>,
    pub(crate) sales_multiplier: f64,
    pub(crate) model_sales_multipliers: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NifflerRuntimeAllowedAccount {
    pub(crate) upstream_service_id: String,
    pub(crate) upstream_account_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NifflerRuntimeModelAccessSnapshot {
    pub(crate) api_key_id: String,
    pub(crate) requested_model: String,
    pub(crate) lookup_model: String,
    pub(crate) allowed_accounts: BTreeSet<NifflerRuntimeAllowedAccount>,
}

impl NifflerRuntimeModelAccessSnapshot {
    pub(crate) fn allows(&self, upstream_service_id: &str, upstream_account_id: &str) -> bool {
        self.allowed_accounts
            .contains(&NifflerRuntimeAllowedAccount {
                upstream_service_id: upstream_service_id.to_string(),
                upstream_account_id: upstream_account_id.to_string(),
            })
    }
}

impl NifflerRuntimeRolloutDecision {
    fn disabled(api_key_id: String, product_plan_id: Option<String>) -> Self {
        Self {
            api_key_id,
            product_plan_id,
            source: None,
            enable_new_routing: false,
            enable_settlement_snapshot: false,
            enable_error_return_rules: false,
            enable_billing_reservation: false,
            enable_referral_ledger: false,
        }
    }

    fn from_setting(
        api_key_id: String,
        product_plan_id: Option<String>,
        source: NifflerRuntimeRolloutDecisionSource,
        setting: &StoredNifflerRuntimeRolloutSetting,
    ) -> Self {
        Self {
            api_key_id,
            product_plan_id,
            source: Some(source),
            enable_new_routing: setting.enable_new_routing,
            enable_settlement_snapshot: setting.enable_settlement_snapshot,
            enable_error_return_rules: setting.enable_error_return_rules,
            // 金额预占已退出请求准入，保留旧配置字段只为兼容历史数据。
            enable_billing_reservation: false,
            enable_referral_ledger: setting.enable_referral_ledger,
        }
    }
}

pub(crate) async fn resolve_niffler_runtime_rollout_decision(
    state: &AppState,
    api_key_id: &str,
) -> Result<NifflerRuntimeRolloutDecision, GatewayError> {
    let api_key_id = api_key_id.trim();
    if api_key_id.is_empty() {
        return Ok(NifflerRuntimeRolloutDecision::disabled(String::new(), None));
    }

    if let Some(cached) = state
        .niffler_runtime_rollout_decision_cache
        .get_fresh(api_key_id, RUNTIME_ROLLOUT_DECISION_CACHE_TTL)
    {
        return Ok(cached);
    }

    let decision = resolve_niffler_runtime_rollout_decision_uncached(state, api_key_id).await?;
    state.niffler_runtime_rollout_decision_cache.insert(
        api_key_id.to_string(),
        decision.clone(),
        RUNTIME_ROLLOUT_DECISION_CACHE_TTL,
        RUNTIME_ROLLOUT_DECISION_CACHE_MAX_ENTRIES,
    );
    Ok(decision)
}

pub(crate) async fn resolve_niffler_runtime_policy_snapshot(
    state: &AppState,
    api_key_id: &str,
) -> Result<Option<NifflerRuntimePolicySnapshot>, GatewayError> {
    let api_key_id = api_key_id.trim();
    if api_key_id.is_empty() {
        return Ok(None);
    }
    if let Some(cached) = state
        .niffler_runtime_snapshot_cache
        .get_policy_fresh(api_key_id, RUNTIME_POLICY_SNAPSHOT_CACHE_TTL)
    {
        return Ok(Some(cached));
    }

    let Some(snapshot) =
        resolve_niffler_runtime_policy_snapshot_uncached(state, api_key_id).await?
    else {
        return Ok(None);
    };
    state.niffler_runtime_snapshot_cache.insert_policy(
        api_key_id.to_string(),
        snapshot.clone(),
        RUNTIME_POLICY_SNAPSHOT_CACHE_TTL,
        RUNTIME_POLICY_SNAPSHOT_CACHE_MAX_ENTRIES,
    );
    Ok(Some(snapshot))
}

pub(crate) async fn resolve_niffler_runtime_model_access_snapshot(
    state: &AppState,
    api_key_id: &str,
    requested_model: &str,
) -> Result<Option<NifflerRuntimeModelAccessSnapshot>, GatewayError> {
    let api_key_id = api_key_id.trim();
    let requested_model = requested_model.trim();
    if api_key_id.is_empty() || requested_model.is_empty() {
        return Ok(None);
    }
    let Some(policy_snapshot) = resolve_niffler_runtime_policy_snapshot(state, api_key_id).await?
    else {
        return Ok(None);
    };
    let lookup_model = niffler_runtime_model_lookup_name(requested_model);
    let cache_key = format!("{api_key_id}:{}", cache_key_component(&lookup_model));
    if let Some(cached) = state
        .niffler_runtime_snapshot_cache
        .get_model_access_fresh(&cache_key, RUNTIME_MODEL_ACCESS_CACHE_TTL)
    {
        return Ok(Some(cached));
    }

    let snapshot = resolve_niffler_runtime_model_access_snapshot_uncached(
        state,
        &policy_snapshot,
        api_key_id,
        requested_model,
        &lookup_model,
    )
    .await?;
    state.niffler_runtime_snapshot_cache.insert_model_access(
        cache_key,
        snapshot.clone(),
        RUNTIME_MODEL_ACCESS_CACHE_TTL,
        RUNTIME_MODEL_ACCESS_CACHE_MAX_ENTRIES,
    );
    Ok(Some(snapshot))
}

async fn resolve_niffler_runtime_policy_snapshot_uncached(
    state: &AppState,
    api_key_id: &str,
) -> Result<Option<NifflerRuntimePolicySnapshot>, GatewayError> {
    let decision = resolve_niffler_runtime_rollout_decision(state, api_key_id).await?;
    if !decision.enable_new_routing {
        return Ok(None);
    }

    let product_plan_id = match decision.product_plan_id.as_ref() {
        Some(product_plan_id) => Some(product_plan_id.clone()),
        None => state
            .find_niffler_api_key_product_plan_binding_by_api_key_id(api_key_id)
            .await?
            .map(|binding| binding.product_plan_id),
    };
    let Some(product_plan_id) = product_plan_id else {
        return Ok(Some(empty_policy_snapshot(api_key_id, None, None)));
    };

    let Some(product_plan) = state
        .find_niffler_product_plan_by_id(&product_plan_id)
        .await?
    else {
        return Ok(Some(empty_policy_snapshot(
            api_key_id,
            Some(product_plan_id),
            None,
        )));
    };
    if !product_plan.is_active {
        return Ok(Some(empty_policy_snapshot(
            api_key_id,
            Some(product_plan.id),
            Some(product_plan.display_name),
        )));
    }

    let plan_models = list_all_enabled_product_plan_models(state, &product_plan.id).await?;
    let allowed_models = plan_models
        .iter()
        .filter_map(|model| non_empty_string(&model.model_name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let model_sales_multipliers = build_model_sales_multiplier_payload(state, &plan_models).await?;

    Ok(Some(NifflerRuntimePolicySnapshot {
        api_key_id: api_key_id.to_string(),
        product_plan_id: Some(product_plan.id),
        product_plan_name: Some(product_plan.display_name),
        allowed_models,
        sales_multiplier: normalize_sales_multiplier(product_plan.sales_multiplier),
        model_sales_multipliers,
    }))
}

async fn resolve_niffler_runtime_model_access_snapshot_uncached(
    state: &AppState,
    policy_snapshot: &NifflerRuntimePolicySnapshot,
    api_key_id: &str,
    requested_model: &str,
    lookup_model: &str,
) -> Result<NifflerRuntimeModelAccessSnapshot, GatewayError> {
    if !policy_allows_requested_model(policy_snapshot, requested_model, lookup_model) {
        return Ok(NifflerRuntimeModelAccessSnapshot {
            api_key_id: api_key_id.to_string(),
            requested_model: requested_model.to_string(),
            lookup_model: lookup_model.to_string(),
            allowed_accounts: BTreeSet::new(),
        });
    }

    let mut offset = 0usize;
    let mut allowed_accounts = BTreeSet::new();
    loop {
        let page = state
            .list_niffler_runtime_account_model_access(&NifflerRuntimeAccountModelAccessListQuery {
                model_name: lookup_model.to_string(),
                now_unix_ms: current_unix_ms(),
                offset,
                limit: RUNTIME_ACCOUNT_MODEL_ACCESS_PAGE_SIZE,
            })
            .await?;
        let item_count = page.items.len();
        for item in page.items {
            allowed_accounts.insert(NifflerRuntimeAllowedAccount {
                upstream_service_id: item.upstream_service_id,
                upstream_account_id: item.upstream_account_id,
            });
        }
        offset = offset.saturating_add(item_count);
        if item_count == 0 || offset >= page.total {
            break;
        }
    }

    Ok(NifflerRuntimeModelAccessSnapshot {
        api_key_id: api_key_id.to_string(),
        requested_model: requested_model.to_string(),
        lookup_model: lookup_model.to_string(),
        allowed_accounts,
    })
}

async fn list_all_enabled_product_plan_models(
    state: &AppState,
    product_plan_id: &str,
) -> Result<Vec<StoredNifflerProductPlanModel>, GatewayError> {
    let mut offset = 0usize;
    let mut items = Vec::new();
    loop {
        let page = state
            .list_niffler_product_plan_models(&NifflerProductPlanModelListQuery {
                product_plan_id: product_plan_id.to_string(),
                enabled_only: true,
                search: None,
                offset,
                limit: RUNTIME_PRODUCT_PLAN_MODEL_PAGE_SIZE,
            })
            .await?;
        let item_count = page.items.len();
        items.extend(page.items);
        offset = offset.saturating_add(item_count);
        if item_count == 0 || offset >= page.total {
            break;
        }
    }
    Ok(items)
}

async fn build_model_sales_multiplier_payload(
    state: &AppState,
    plan_models: &[StoredNifflerProductPlanModel],
) -> Result<Option<serde_json::Value>, GatewayError> {
    let mut payload = serde_json::Map::new();
    let mut resolved_model_ids: BTreeMap<String, String> = BTreeMap::new();
    for plan_model in plan_models {
        let Some(multiplier) = plan_model
            .sales_multiplier_override
            .map(normalize_sales_multiplier)
        else {
            continue;
        };
        let Some(model_name) = non_empty_string(&plan_model.model_name) else {
            continue;
        };
        let global_model_id = match resolved_model_ids.get(&model_name) {
            Some(global_model_id) => global_model_id.clone(),
            None => {
                let Some(global_model) = state.get_admin_global_model_by_name(&model_name).await?
                else {
                    return Err(GatewayError::Internal(format!(
                        "niffler product plan model {model_name} has sales multiplier override but no matching global model"
                    )));
                };
                resolved_model_ids.insert(model_name.clone(), global_model.id.clone());
                global_model.id
            }
        };
        if let Some(number) = serde_json::Number::from_f64(multiplier) {
            payload.insert(global_model_id, serde_json::Value::Number(number));
        }
    }
    if payload.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::Value::Object(payload)))
    }
}

fn empty_policy_snapshot(
    api_key_id: &str,
    product_plan_id: Option<String>,
    product_plan_name: Option<String>,
) -> NifflerRuntimePolicySnapshot {
    NifflerRuntimePolicySnapshot {
        api_key_id: api_key_id.to_string(),
        product_plan_id,
        product_plan_name,
        allowed_models: Vec::new(),
        sales_multiplier: 1.0,
        model_sales_multipliers: None,
    }
}

fn policy_allows_requested_model(
    policy_snapshot: &NifflerRuntimePolicySnapshot,
    requested_model: &str,
    lookup_model: &str,
) -> bool {
    policy_snapshot
        .allowed_models
        .iter()
        .any(|model| model == requested_model || model == lookup_model)
}

fn niffler_runtime_model_lookup_name(requested_model: &str) -> String {
    crate::ai_serving::model_directive_base_model(requested_model)
        .unwrap_or_else(|| requested_model.to_string())
        .trim()
        .to_string()
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn normalize_sales_multiplier(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        1.0
    }
}

fn cache_key_component(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
}

async fn resolve_niffler_runtime_rollout_decision_uncached(
    state: &AppState,
    api_key_id: &str,
) -> Result<NifflerRuntimeRolloutDecision, GatewayError> {
    let api_key_id_owned = api_key_id.to_string();
    let key_setting = state
        .find_niffler_runtime_rollout_setting(NifflerRuntimeRolloutTargetScope::ApiKey, api_key_id)
        .await?;
    if let Some(setting) = key_setting.as_ref().filter(|setting| setting.is_active) {
        return Ok(NifflerRuntimeRolloutDecision::from_setting(
            api_key_id_owned,
            None,
            NifflerRuntimeRolloutDecisionSource::ApiKey,
            setting,
        ));
    }

    let Some(binding) = state
        .find_niffler_api_key_product_plan_binding_by_api_key_id(api_key_id)
        .await?
    else {
        return Ok(NifflerRuntimeRolloutDecision::disabled(
            api_key_id_owned,
            None,
        ));
    };

    let product_plan_id = binding.product_plan_id.clone();
    let Some(product_plan) = state
        .find_niffler_product_plan_by_id(&product_plan_id)
        .await?
    else {
        return Ok(NifflerRuntimeRolloutDecision::disabled(
            api_key_id_owned,
            Some(product_plan_id),
        ));
    };
    if !product_plan.is_active {
        return Ok(NifflerRuntimeRolloutDecision::disabled(
            api_key_id_owned,
            Some(product_plan_id),
        ));
    }

    let product_plan_setting = state
        .find_niffler_runtime_rollout_setting(
            NifflerRuntimeRolloutTargetScope::ProductPlan,
            &product_plan_id,
        )
        .await?;
    let Some(setting) = product_plan_setting
        .as_ref()
        .filter(|setting| setting.is_active)
    else {
        return Ok(NifflerRuntimeRolloutDecision::disabled(
            api_key_id_owned,
            Some(product_plan_id),
        ));
    };

    Ok(NifflerRuntimeRolloutDecision::from_setting(
        api_key_id_owned,
        Some(product_plan_id),
        NifflerRuntimeRolloutDecisionSource::ProductPlan,
        setting,
    ))
}

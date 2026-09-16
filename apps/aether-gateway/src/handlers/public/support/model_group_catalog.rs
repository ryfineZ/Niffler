use std::collections::{BTreeMap, BTreeSet};

use aether_data::repository::users::StoredUserGroup;
use aether_data_contracts::repository::{
    global_models::{PublicGlobalModelQuery, StoredPublicGlobalModel},
    niffler_core::{
        NifflerProductPlanListQuery, NifflerProductPlanModelListQuery, StoredNifflerProductPlan,
        StoredNifflerProductPlanModel,
    },
};
use axum::{body::Body, http, response::IntoResponse, response::Response, Json};
use serde_json::json;

use super::{
    build_auth_error_response, resolve_request_portal, sanitize_public_model_config_for_user,
    validate_official_usd_registration_group, AppState, GatewayPublicRequestContext,
};

#[path = "model_group_catalog/health.rs"]
mod health;

use self::health::{load_public_model_health_snapshot, PublicModelHealthSummary};

const PAGE_SIZE: usize = 1000;

pub(super) struct PublicModelGroupCatalogSource {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) sales_multiplier: f64,
    pub(super) model_sales_multipliers: BTreeMap<String, f64>,
    pub(super) allowed_models: Option<Vec<String>>,
    pub(super) allowed_models_mode: String,
}

impl PublicModelGroupCatalogSource {
    pub(super) fn from_product_plan(
        plan: StoredNifflerProductPlan,
        plan_models: Vec<StoredNifflerProductPlanModel>,
    ) -> Self {
        let allowed_models = plan_models
            .iter()
            .map(|model| model.model_name.clone())
            .collect::<Vec<_>>();
        let model_sales_multipliers = plan_models
            .into_iter()
            .filter_map(|model| {
                model
                    .sales_multiplier_override
                    .map(|multiplier| (model.model_name, multiplier))
            })
            .collect();
        Self {
            id: plan.id,
            name: plan.display_name,
            sales_multiplier: plan.sales_multiplier,
            model_sales_multipliers,
            allowed_models: Some(allowed_models),
            allowed_models_mode: "specific".to_string(),
        }
    }

    pub(super) fn from_legacy_group(group: StoredUserGroup) -> Self {
        let model_sales_multipliers = group
            .model_sales_multipliers
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        Self {
            id: group.id,
            name: group.name,
            sales_multiplier: group.sales_multiplier,
            model_sales_multipliers,
            allowed_models: group.allowed_models,
            allowed_models_mode: group.allowed_models_mode,
        }
    }

    pub(super) fn allows_model(&self, model: &StoredPublicGlobalModel) -> bool {
        match self
            .allowed_models_mode
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "inherit" | "unrestricted" => true,
            "specific" => self.allowed_models.as_ref().is_some_and(|items| {
                items.iter().any(|value| {
                    value == &model.id
                        || value == &model.name
                        || model
                            .display_name
                            .as_ref()
                            .is_some_and(|display_name| value == display_name)
                })
            }),
            "deny_all" => false,
            _ => false,
        }
    }

    pub(super) fn normalize_model_multiplier_keys(&mut self, models: &[StoredPublicGlobalModel]) {
        self.model_sales_multipliers = std::mem::take(&mut self.model_sales_multipliers)
            .into_iter()
            .map(|(key, value)| {
                let normalized = models
                    .iter()
                    .find(|model| model.id == key)
                    .map(|model| model.name.clone())
                    .unwrap_or(key);
                (normalized, value)
            })
            .collect();
    }
}

pub(super) async fn build_public_model_group_catalog_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    let portal = match resolve_request_portal(state, request_context, headers).await {
        Ok(value) => value,
        Err(_) => return unavailable_response(),
    };
    let models = match list_all_active_models(state).await {
        Ok(models) => models,
        Err(()) => return unavailable_response(),
    };
    let mut groups = match load_catalog_groups(state, &portal).await {
        Ok(groups) => groups,
        Err(()) => return unavailable_response(),
    };

    groups.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    for group in &mut groups {
        group.normalize_model_multiplier_keys(&models);
    }

    let health_by_model = load_public_model_health_snapshot(state, &models).await;
    let mut group_payloads = Vec::with_capacity(groups.len());
    let use_discount_terms = portal.is_official_usd();
    for group in groups {
        let mut model_payloads = Vec::new();
        for model in models.iter().filter(|model| group.allows_model(model)) {
            let health = health_by_model.get(&model.id).cloned().unwrap_or_default();
            model_payloads.push(public_model_payload(model, health));
        }
        let sales_multiplier = group.sales_multiplier;
        let model_sales_multipliers = group.model_sales_multipliers;
        let mut payload = json!({
            "id": group.id,
            "name": group.name,
            "allowed_models": group.allowed_models,
            "allowed_models_mode": group.allowed_models_mode,
            "models": model_payloads,
        });
        if use_discount_terms {
            payload["discount"] = json!(sales_multiplier);
            payload["model_discounts"] = json!(model_sales_multipliers);
        } else {
            payload["sales_multiplier"] = json!(sales_multiplier);
            payload["model_sales_multipliers"] = json!(model_sales_multipliers);
        }
        group_payloads.push(payload);
    }

    Json(json!({ "groups": group_payloads })).into_response()
}

async fn load_catalog_groups(
    state: &AppState,
    portal: &super::PortalContext,
) -> Result<Vec<PublicModelGroupCatalogSource>, ()> {
    if portal.is_official_usd() {
        let group_id = validate_official_usd_registration_group(state, portal)
            .await
            .map_err(|_| ())?;
        let group = state
            .find_user_group_by_id(&group_id)
            .await
            .map_err(|_| ())?
            .ok_or(())?;
        return Ok(vec![PublicModelGroupCatalogSource::from_legacy_group(
            group,
        )]);
    }
    let product_plans = if state.has_niffler_core_reader() {
        list_all_product_plans(state).await?
    } else {
        Vec::new()
    };
    let shadowed_legacy_group_ids = public_active_product_plan_ids(&product_plans);

    let mut groups = Vec::new();
    for plan in product_plans
        .into_iter()
        .filter(|plan| plan.is_public && plan.is_active)
    {
        let models = list_all_enabled_plan_models(state, &plan.id).await?;
        groups.push(PublicModelGroupCatalogSource::from_product_plan(
            plan, models,
        ));
    }

    if state.has_user_data_reader() {
        let legacy_groups = state.list_user_groups().await.map_err(|_| ())?;
        groups.extend(
            legacy_groups
                .into_iter()
                .filter(|group| group.visibility == "public")
                .filter(|group| !shadowed_legacy_group_ids.contains(&group.id))
                .map(PublicModelGroupCatalogSource::from_legacy_group),
        );
    }

    Ok(groups)
}

fn public_active_product_plan_ids(product_plans: &[StoredNifflerProductPlan]) -> BTreeSet<String> {
    product_plans
        .iter()
        .filter(|plan| plan.is_public && plan.is_active)
        .map(|plan| plan.id.clone())
        .collect()
}

async fn list_all_active_models(state: &AppState) -> Result<Vec<StoredPublicGlobalModel>, ()> {
    let mut offset = 0usize;
    let mut models = Vec::new();
    loop {
        let page = state
            .list_public_global_models(&PublicGlobalModelQuery {
                offset,
                limit: PAGE_SIZE,
                is_active: Some(true),
                search: None,
            })
            .await
            .map_err(|_| ())?;
        let item_count = page.items.len();
        models.extend(page.items);
        offset = offset.saturating_add(item_count);
        if item_count == 0 || offset >= page.total {
            return Ok(models);
        }
    }
}

async fn list_all_product_plans(state: &AppState) -> Result<Vec<StoredNifflerProductPlan>, ()> {
    let mut offset = 0usize;
    let mut plans = Vec::new();
    loop {
        let page = state
            .list_niffler_product_plans(&NifflerProductPlanListQuery {
                include_inactive: true,
                public_only: false,
                search: None,
                offset,
                limit: PAGE_SIZE,
            })
            .await
            .map_err(|_| ())?;
        let item_count = page.items.len();
        plans.extend(page.items);
        offset = offset.saturating_add(item_count);
        if item_count == 0 || offset >= page.total {
            return Ok(plans);
        }
    }
}

async fn list_all_enabled_plan_models(
    state: &AppState,
    product_plan_id: &str,
) -> Result<Vec<StoredNifflerProductPlanModel>, ()> {
    let mut offset = 0usize;
    let mut models = Vec::new();
    loop {
        let page = state
            .list_niffler_product_plan_models(&NifflerProductPlanModelListQuery {
                product_plan_id: product_plan_id.to_string(),
                enabled_only: true,
                search: None,
                offset,
                limit: PAGE_SIZE,
            })
            .await
            .map_err(|_| ())?;
        let item_count = page.items.len();
        models.extend(page.items);
        offset = offset.saturating_add(item_count);
        if item_count == 0 || offset >= page.total {
            return Ok(models);
        }
    }
}

fn public_model_payload(
    model: &StoredPublicGlobalModel,
    health: PublicModelHealthSummary,
) -> serde_json::Value {
    json!({
        "id": model.id,
        "name": model.name,
        "display_name": model.display_name,
        "is_active": model.is_active,
        "default_price_per_request": model.default_price_per_request,
        "default_tiered_pricing": model.default_tiered_pricing,
        "supported_capabilities": model.supported_capabilities,
        "config": sanitize_public_model_config_for_user(model.config.clone()),
        "usage_count": model.usage_count,
        "health": health,
    })
}

fn unavailable_response() -> Response<Body> {
    build_auth_error_response(
        http::StatusCode::SERVICE_UNAVAILABLE,
        "模型目录暂不可用",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> StoredPublicGlobalModel {
        StoredPublicGlobalModel {
            id: "gm-gpt".to_string(),
            name: "gpt-5".to_string(),
            display_name: Some("GPT 5".to_string()),
            is_active: true,
            default_price_per_request: None,
            default_tiered_pricing: None,
            supported_capabilities: None,
            config: None,
            usage_count: 0,
        }
    }

    fn source(mode: &str, allowed_models: Option<Vec<&str>>) -> PublicModelGroupCatalogSource {
        PublicModelGroupCatalogSource {
            id: "group".to_string(),
            name: "Public".to_string(),
            sales_multiplier: 1.0,
            model_sales_multipliers: BTreeMap::new(),
            allowed_models: allowed_models
                .map(|items| items.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>()),
            allowed_models_mode: mode.to_string(),
        }
    }

    fn product_plan(id: &str, is_public: bool, is_active: bool) -> StoredNifflerProductPlan {
        StoredNifflerProductPlan {
            id: id.to_string(),
            display_name: id.to_string(),
            is_public,
            is_active,
            sales_multiplier: 1.0,
            description: None,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        }
    }

    #[test]
    fn restrictive_empty_catalog_does_not_expose_models() {
        assert!(!source("specific", Some(Vec::new())).allows_model(&model()));
        assert!(!source("deny_all", None).allows_model(&model()));
        assert!(!source("unknown", None).allows_model(&model()));
    }

    #[test]
    fn specific_catalog_accepts_model_id_name_or_display_name() {
        for allowed in ["gm-gpt", "gpt-5", "GPT 5"] {
            assert!(source("specific", Some(vec![allowed])).allows_model(&model()));
        }
    }

    #[test]
    fn unrestricted_catalog_exposes_models() {
        assert!(source("unrestricted", None).allows_model(&model()));
        assert!(source("inherit", None).allows_model(&model()));
    }

    #[test]
    fn product_plan_is_the_authoritative_catalog_and_price_source() {
        let plan = StoredNifflerProductPlan {
            id: "plan-1".to_string(),
            display_name: "Public plan".to_string(),
            is_public: true,
            is_active: true,
            sales_multiplier: 0.5,
            description: None,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        };
        let plan_model = StoredNifflerProductPlanModel {
            id: "plan-model-1".to_string(),
            product_plan_id: "plan-1".to_string(),
            model_name: "gpt-5".to_string(),
            is_enabled: true,
            sales_multiplier_override: Some(0.25),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        };

        let catalog = PublicModelGroupCatalogSource::from_product_plan(plan, vec![plan_model]);

        assert_eq!(catalog.sales_multiplier, 0.5);
        assert_eq!(catalog.model_sales_multipliers.get("gpt-5"), Some(&0.25));
        assert!(catalog.allows_model(&model()));
    }

    #[test]
    fn only_public_active_product_plans_shadow_legacy_groups() {
        let plans = vec![
            product_plan("active-public", true, true),
            product_plan("inactive-public", true, false),
            product_plan("active-private", false, true),
        ];

        let shadowed = public_active_product_plan_ids(&plans);

        assert_eq!(shadowed, BTreeSet::from(["active-public".to_string()]));
        assert!(!shadowed.contains("inactive-public"));
        assert!(!shadowed.contains("active-private"));
    }
}

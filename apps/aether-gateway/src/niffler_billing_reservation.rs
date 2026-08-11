use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use axum::body::Bytes;
use tokio::sync::Mutex;
use uuid::Uuid;

use aether_data_contracts::repository::niffler_core::CreateNifflerBillingReservationRecord;

use crate::clock::current_unix_ms;
use crate::control::{
    estimate_request_wallet_reservation, extract_requested_model, GatewayControlDecision,
    GatewayLocalAuthRejection,
};
use crate::niffler_runtime::resolve_niffler_runtime_rollout_decision;
use crate::{AppState, GatewayError};

const ACTIVE_RESERVATION_EPSILON_USD: f64 = 0.000_000_01;
const BILLING_RESERVATION_TTL_MS: u64 = 30 * 60 * 1000;
const BILLING_RESERVATION_SCOPE_LOCK_SHARDS: usize = 64;

static BILLING_RESERVATION_SCOPE_LOCKS: LazyLock<Vec<Mutex<()>>> = LazyLock::new(|| {
    (0..BILLING_RESERVATION_SCOPE_LOCK_SHARDS)
        .map(|_| Mutex::new(()))
        .collect()
});

pub(crate) async fn prepare_niffler_billing_reservation_for_request(
    state: &AppState,
    decision: Option<&GatewayControlDecision>,
    request_id: &str,
    uri: &http::Uri,
    headers: &http::HeaderMap,
    body: &Bytes,
) -> Result<Option<GatewayLocalAuthRejection>, GatewayError> {
    let Some(decision) = decision else {
        return Ok(None);
    };
    if decision.route_class.as_deref() != Some("ai_public") {
        return Ok(None);
    }
    let Some(auth_context) = decision.auth_context.as_ref() else {
        return Ok(None);
    };
    if auth_context.local_rejection.is_some() || !auth_context.access_allowed {
        return Ok(None);
    }
    let rollout = resolve_niffler_runtime_rollout_decision(state, &auth_context.api_key_id).await?;
    if !rollout.enable_billing_reservation {
        return Ok(None);
    }

    let requested_model = extract_requested_model(decision, uri, headers, body);
    let mut requested_global_model_id = None;
    let Some(estimate) = estimate_request_wallet_reservation(
        state,
        decision,
        auth_context,
        requested_model.as_deref(),
        &mut requested_global_model_id,
        None,
        true,
        headers,
        body,
    )
    .await?
    else {
        return Ok(None);
    };
    if estimate.wallet_reservation_usd <= ACTIVE_RESERVATION_EPSILON_USD {
        return Ok(None);
    }
    let Some(wallet_available_usd) = estimate.available_usd else {
        return Ok(None);
    };
    let reservation_scope = format!("user:{}", auth_context.user_id);
    let _scope_guard = billing_reservation_scope_lock(&reservation_scope)
        .lock()
        .await;
    let now_unix_ms = current_unix_ms();
    let active_reserved_usd = state
        .sum_active_niffler_billing_reservation_wallet_usd(
            Some(auth_context.user_id.as_str()),
            None,
            now_unix_ms,
        )
        .await?;
    let available_after_active = (wallet_available_usd - active_reserved_usd).max(0.0);
    if estimate.wallet_reservation_usd > available_after_active + ACTIVE_RESERVATION_EPSILON_USD {
        return Ok(Some(GatewayLocalAuthRejection::BalanceDenied {
            remaining: Some(available_after_active),
        }));
    }

    let request_id = request_id.trim();
    if request_id.is_empty() {
        return Ok(None);
    }
    let record = CreateNifflerBillingReservationRecord {
        id: stable_uuid("niffler-billing-reservation", request_id),
        request_id: request_id.to_string(),
        user_id: Some(auth_context.user_id.clone()),
        api_key_id: Some(auth_context.api_key_id.clone()),
        product_plan_id: rollout.product_plan_id.clone(),
        reserved_total_usd: estimate.wallet_reservation_usd,
        wallet_reserved_usd: estimate.wallet_reservation_usd,
        entitlement_reserved_usd: 0.0,
        reserved_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: now_unix_ms.saturating_add(BILLING_RESERVATION_TTL_MS),
        idempotency_key: stable_idempotency_key("reservation", request_id),
        event_id: stable_uuid("niffler-billing-reservation-event-reserved", request_id),
        event_idempotency_key: stable_idempotency_key("reservation-event-reserved", request_id),
        actor_id: Some(auth_context.user_id.clone()),
    };
    if state
        .create_niffler_billing_reservation(record)
        .await?
        .is_none()
    {
        return Err(GatewayError::Internal(
            "niffler billing reservation writer unavailable".to_string(),
        ));
    }
    Ok(None)
}

fn stable_uuid(prefix: &str, request_id: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("{prefix}:{request_id}").as_bytes(),
    )
    .to_string()
}

fn stable_idempotency_key(prefix: &str, request_id: &str) -> String {
    format!("{prefix}:{request_id}")
}

fn billing_reservation_scope_lock(scope: &str) -> &'static Mutex<()> {
    let mut hasher = DefaultHasher::new();
    scope.hash(&mut hasher);
    let index = (hasher.finish() as usize) % BILLING_RESERVATION_SCOPE_LOCK_SHARDS;
    &BILLING_RESERVATION_SCOPE_LOCKS[index]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aether_data::repository::billing::InMemoryBillingReadRepository;
    use aether_data::repository::candidate_selection::InMemoryMinimalCandidateSelectionReadRepository;
    use aether_data::repository::wallet::StoredWalletSnapshot;
    use aether_data::{DatabaseDriver, SqlDatabaseConfig, SqlPoolConfig};
    use aether_data_contracts::repository::billing::StoredBillingModelContext;
    use aether_data_contracts::repository::candidate_selection::{
        StoredMinimalCandidateSelectionRow, StoredProviderModelMapping,
    };
    use aether_data_contracts::repository::niffler_core::{
        CreateNifflerBillingReservationRecord, CreateNifflerProductPlanRecord,
        NifflerBillingReservationListQuery, NifflerRuntimeRolloutTargetScope,
        UpsertNifflerApiKeyProductPlanBindingRecord, UpsertNifflerRuntimeRolloutSettingRecord,
    };
    use axum::body::Bytes;
    use axum::http::{HeaderMap, Uri};
    use serde_json::json;

    use super::prepare_niffler_billing_reservation_for_request;
    use crate::clock::current_unix_ms;
    use crate::control::{GatewayControlAuthContext, GatewayControlDecision};
    use crate::data::GatewayDataConfig;
    use crate::AppState;

    async fn sqlite_state_with_billing_overrides() -> AppState {
        let mut pool = SqlPoolConfig::default();
        pool.min_connections = 0;
        pool.max_connections = 1;
        let database = SqlDatabaseConfig::new(DatabaseDriver::Sqlite, "sqlite::memory:", pool)
            .expect("sqlite database config should build");
        let state = AppState::new()
            .expect("app state should build")
            .with_data_config(GatewayDataConfig::from_database_config(database))
            .expect("sqlite data config should wire");
        assert!(state
            .run_database_migrations()
            .await
            .expect("sqlite migrations should run"));

        let candidate_repository =
            Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(vec![
                candidate_row(),
            ]));
        let billing_repository =
            Arc::new(InMemoryBillingReadRepository::seed(vec![billing_context()]));
        let data = (*state.data)
            .clone()
            .with_minimal_candidate_selection_and_billing_overrides_for_tests(
                candidate_repository,
                billing_repository,
            );
        state
            .with_data_state_for_tests(data)
            .with_auth_wallets_for_tests(vec![wallet("user-1", 10.0)])
    }

    fn candidate_row() -> StoredMinimalCandidateSelectionRow {
        StoredMinimalCandidateSelectionRow {
            provider_id: "provider-1".to_string(),
            provider_name: "Provider 1".to_string(),
            provider_type: "openai".to_string(),
            provider_priority: 0,
            provider_is_active: true,
            endpoint_id: "endpoint-1".to_string(),
            endpoint_api_format: "openai:chat".to_string(),
            endpoint_api_family: Some("openai".to_string()),
            endpoint_kind: Some("chat".to_string()),
            endpoint_is_active: true,
            key_id: "key-1".to_string(),
            key_name: "key".to_string(),
            key_auth_type: "api_key".to_string(),
            key_is_active: true,
            key_api_formats: Some(vec!["openai:chat".to_string()]),
            key_allowed_models: None,
            key_capabilities: None,
            key_internal_priority: 0,
            key_global_priority_by_format: None,
            model_id: "model-1".to_string(),
            global_model_id: "global-model-1".to_string(),
            global_model_name: "gpt-5".to_string(),
            global_model_mappings: Some(vec!["gpt-5".to_string()]),
            global_model_supports_streaming: Some(true),
            model_provider_model_name: "gpt-5-upstream".to_string(),
            model_provider_model_mappings: Some(vec![StoredProviderModelMapping {
                name: "gpt-5-upstream".to_string(),
                priority: 1,
                api_formats: Some(vec!["openai:chat".to_string()]),
                endpoint_ids: None,
            }]),
            model_supports_streaming: Some(true),
            model_is_active: true,
            model_is_available: true,
        }
    }

    fn billing_context() -> StoredBillingModelContext {
        StoredBillingModelContext::new(
            "provider-1".to_string(),
            None,
            None,
            Some("key-1".to_string()),
            None,
            Some(60),
            "global-model-1".to_string(),
            "gpt-5".to_string(),
            None,
            None,
            Some(json!({
                "tiers": [{
                    "up_to": null,
                    "input_price_per_1m": 1.0,
                    "output_price_per_1m": 2.0
                }]
            })),
            Some("model-1".to_string()),
            Some("gpt-5-upstream".to_string()),
            None,
            None,
            None,
        )
        .expect("billing context should build")
    }

    fn wallet(user_id: &str, balance: f64) -> StoredWalletSnapshot {
        StoredWalletSnapshot::new(
            format!("wallet-{user_id}"),
            Some(user_id.to_string()),
            None,
            balance,
            0.0,
            "finite".to_string(),
            "USD".to_string(),
            "active".to_string(),
            balance,
            0.0,
            0.0,
            0.0,
            100,
        )
        .expect("wallet should build")
    }

    fn decision() -> GatewayControlDecision {
        let mut decision = GatewayControlDecision::synthetic(
            "/v1/chat/completions",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
        );
        decision.auth_context = Some(GatewayControlAuthContext {
            user_id: "user-1".to_string(),
            api_key_id: "api-key-1".to_string(),
            username: None,
            api_key_name: None,
            api_key_group_id: None,
            api_key_group_name: None,
            sales_multiplier: 1.0,
            model_sales_multipliers: None,
            balance_remaining: None,
            access_allowed: true,
            user_rate_limit: None,
            api_key_rate_limit: None,
            api_key_is_standalone: false,
            admin_bypass_limits: false,
            local_rejection: None,
            allowed_models: Some(vec!["gpt-5".to_string()]),
        });
        decision
    }

    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            "application/json"
                .parse()
                .expect("content type should parse"),
        );
        headers
    }

    fn body() -> Bytes {
        Bytes::from_static(
            br#"{"model":"gpt-5","messages":[{"role":"user","content":"hi"}],"max_tokens":1000}"#,
        )
    }

    async fn reservation_count(state: &AppState) -> usize {
        state
            .list_niffler_billing_reservations(&NifflerBillingReservationListQuery {
                status: None,
                user_id: None,
                api_key_id: None,
                request_id: None,
                expires_at_gte_unix_ms: None,
                expires_at_lte_unix_ms: None,
                expires_at_lt_unix_ms: None,
                finalized_at_gte_unix_ms: None,
                finalized_at_lt_unix_ms: None,
                offset: 0,
                limit: 10,
            })
            .await
            .expect("billing reservations should list")
            .items
            .len()
    }

    async fn enable_api_key_reservation(state: &AppState) {
        state
            .upsert_niffler_runtime_rollout_setting(UpsertNifflerRuntimeRolloutSettingRecord {
                id: "rollout-api-key-1".to_string(),
                target_scope: NifflerRuntimeRolloutTargetScope::ApiKey,
                target_id: "api-key-1".to_string(),
                enable_new_routing: false,
                enable_settlement_snapshot: false,
                enable_error_return_rules: false,
                enable_billing_reservation: true,
                enable_referral_ledger: false,
                is_active: true,
                config: None,
                created_at_unix_ms: 1_700_000_000_000,
                updated_at_unix_ms: 1_700_000_000_000,
            })
            .await
            .expect("api key rollout should upsert");
    }

    async fn enable_product_plan_reservation(state: &AppState) {
        state
            .create_niffler_product_plan(CreateNifflerProductPlanRecord {
                id: "plan-1".to_string(),
                display_name: "Plan 1".to_string(),
                is_public: false,
                is_active: true,
                sales_multiplier: 1.0,
                description: None,
                created_at_unix_ms: 1_700_000_000_000,
                updated_at_unix_ms: 1_700_000_000_000,
            })
            .await
            .expect("product plan should create");
        state
            .upsert_niffler_api_key_product_plan_binding(
                UpsertNifflerApiKeyProductPlanBindingRecord {
                    id: "binding-api-key-1-plan-1".to_string(),
                    api_key_id: "api-key-1".to_string(),
                    product_plan_id: "plan-1".to_string(),
                    config: None,
                    created_at_unix_ms: 1_700_000_000_000,
                    updated_at_unix_ms: 1_700_000_000_000,
                },
            )
            .await
            .expect("api key product plan binding should upsert");
        state
            .upsert_niffler_runtime_rollout_setting(UpsertNifflerRuntimeRolloutSettingRecord {
                id: "rollout-product-plan-1".to_string(),
                target_scope: NifflerRuntimeRolloutTargetScope::ProductPlan,
                target_id: "plan-1".to_string(),
                enable_new_routing: false,
                enable_settlement_snapshot: false,
                enable_error_return_rules: false,
                enable_billing_reservation: true,
                enable_referral_ledger: false,
                is_active: true,
                config: None,
                created_at_unix_ms: 1_700_000_000_000,
                updated_at_unix_ms: 1_700_000_000_000,
            })
            .await
            .expect("product plan rollout should upsert");
    }

    #[tokio::test]
    async fn api_key_rollout_cannot_reenable_billing_reservation() {
        let state = sqlite_state_with_billing_overrides().await;
        enable_api_key_reservation(&state).await;
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");

        let rejection = prepare_niffler_billing_reservation_for_request(
            &state,
            Some(&decision()),
            "request-api-key-reservation-1",
            &uri,
            &headers(),
            &body(),
        )
        .await
        .expect("reservation preparation should run");

        assert_eq!(rejection, None);
        assert_eq!(reservation_count(&state).await, 0);
    }

    #[tokio::test]
    async fn product_plan_rollout_cannot_reenable_billing_reservation() {
        let state = sqlite_state_with_billing_overrides().await;
        enable_product_plan_reservation(&state).await;
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");

        let rejection = prepare_niffler_billing_reservation_for_request(
            &state,
            Some(&decision()),
            "request-product-plan-reservation-1",
            &uri,
            &headers(),
            &body(),
        )
        .await
        .expect("reservation preparation should run");

        assert_eq!(rejection, None);
        assert_eq!(reservation_count(&state).await, 0);
    }

    #[tokio::test]
    async fn legacy_active_reservations_do_not_reduce_available_wallet() {
        let state = sqlite_state_with_billing_overrides().await;
        enable_api_key_reservation(&state).await;
        let uri: Uri = "/v1/chat/completions".parse().expect("uri should parse");
        let request_body = body();
        prepare_niffler_billing_reservation_for_request(
            &state,
            Some(&decision()),
            "request-api-key-reservation-existing",
            &uri,
            &headers(),
            &request_body,
        )
        .await
        .expect("first reservation preparation should run");

        state
            .create_niffler_billing_reservation(CreateNifflerBillingReservationRecord {
                id: "reservation-large-active".to_string(),
                request_id: "request-large-active".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("api-key-1".to_string()),
                product_plan_id: None,
                reserved_total_usd: 9.999,
                wallet_reserved_usd: 9.999,
                entitlement_reserved_usd: 0.0,
                reserved_at_unix_ms: current_unix_ms(),
                expires_at_unix_ms: current_unix_ms().saturating_add(60_000),
                idempotency_key: "reservation-large-active".to_string(),
                event_id: "reservation-large-active-event".to_string(),
                event_idempotency_key: "reservation-large-active-event".to_string(),
                actor_id: Some("test".to_string()),
            })
            .await
            .expect("large active reservation should create");

        let rejection = prepare_niffler_billing_reservation_for_request(
            &state,
            Some(&decision()),
            "request-api-key-reservation-denied",
            &uri,
            &headers(),
            &request_body,
        )
        .await
        .expect("reservation preparation should run");

        assert_eq!(rejection, None);
    }
}

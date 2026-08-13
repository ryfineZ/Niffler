use std::{collections::BTreeMap, sync::Arc};

use aether_ai_serving::{run_ai_authenticated_decision_input, AiAuthenticatedDecisionInputPort};
use aether_routing_core::{
    rank_vector_for_candidate, CandidateKind, ResolvedRoutingPolicy, RoutingCandidateFacts,
    RoutingCandidateTrace, RoutingDecisionTrace, RoutingPoolExpansionTrace, RoutingRulePhase,
};
use aether_scheduler_core::ClientSessionAffinity;
use async_trait::async_trait;
use http::StatusCode;
use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use tracing::warn;

use crate::ai_serving::planner::codex_identity_convergence::{
    build_codex_oauth_identity_convergence_request_context,
    CodexOAuthIdentityConvergenceRequestContext,
};
use crate::ai_serving::planner::common::extract_standard_requested_model;
use crate::ai_serving::{ExecutionRuntimeAuthContext, GatewayAuthApiKeySnapshot, PlannerAppState};
use crate::client_session_affinity::{
    client_session_affinity_from_request, codex_encrypted_context_handoff_from_request,
};
use crate::clock::current_unix_secs;
use crate::managed_instructions::{
    apply_managed_instructions_to_decision, has_applied_managed_instructions_state,
    record_managed_instructions_user_group, resolve_managed_instructions_config,
    ManagedInstructionsBindingSnapshot,
};
use crate::routing::{
    apply_routing_mutation_plan, build_routing_trace_seed, resolve_gateway_routing_policy,
    select_gateway_routing_group, GatewayRoutingPolicyInput, GatewayRoutingSelectionError,
    GatewayRoutingSelectionInput, ROUTING_GROUP_HEADER,
};
use crate::{AiExecutionDecision, AppState, GatewayError};

#[derive(Debug, Clone)]
pub(crate) struct ResolvedLocalDecisionAuthInput {
    pub(crate) auth_context: ExecutionRuntimeAuthContext,
    pub(crate) auth_snapshot: GatewayAuthApiKeySnapshot,
    pub(crate) required_capabilities: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalRequestedModelDecisionInput {
    pub(crate) auth_context: ExecutionRuntimeAuthContext,
    pub(crate) requested_model: String,
    pub(crate) auth_snapshot: GatewayAuthApiKeySnapshot,
    pub(crate) required_capabilities: Option<serde_json::Value>,
    pub(crate) request_auth_channel: Option<String>,
    pub(crate) client_session_affinity: Option<ClientSessionAffinity>,
    pub(crate) defer_scheduler_affinity_until_success: bool,
    pub(crate) routing_policy: Option<ResolvedRoutingPolicy>,
    pub(crate) routing_trace_seed: Option<RoutingDecisionTrace>,
    pub(crate) routing_context: Option<LocalRoutingRequestContext>,
    pub(crate) codex_oauth_identity_convergence:
        Option<CodexOAuthIdentityConvergenceRequestContext>,
    pub(crate) managed_instructions_snapshot:
        Arc<tokio::sync::OnceCell<ManagedInstructionsBindingSnapshot>>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalAuthenticatedDecisionInput {
    pub(crate) auth_context: ExecutionRuntimeAuthContext,
    pub(crate) auth_snapshot: GatewayAuthApiKeySnapshot,
    pub(crate) required_capabilities: Option<serde_json::Value>,
    pub(crate) client_session_affinity: Option<ClientSessionAffinity>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalRoutingRequestContext {
    pub(crate) group_id: Option<String>,
    pub(crate) group_version: Option<i64>,
    pub(crate) group_config_json: Value,
    pub(crate) selection_source: String,
    pub(crate) client_api_format: String,
    pub(crate) effective_body_json: Value,
    pub(crate) effective_headers: HeaderMap,
}

impl LocalRequestedModelDecisionInput {
    pub(crate) fn effective_body_json<'a>(&'a self, fallback: &'a Value) -> &'a Value {
        self.routing_context
            .as_ref()
            .map(|context| &context.effective_body_json)
            .unwrap_or(fallback)
    }

    pub(crate) fn effective_headers<'a>(&'a self, fallback: &'a HeaderMap) -> &'a HeaderMap {
        self.routing_context
            .as_ref()
            .map(|context| &context.effective_headers)
            .unwrap_or(fallback)
    }
}

pub(crate) fn apply_provider_request_routing_policy_to_decision(
    input: &LocalRequestedModelDecisionInput,
    decision: &mut AiExecutionDecision,
) -> Result<(), GatewayError> {
    let Some(context) = input.routing_context.as_ref() else {
        return Ok(());
    };
    let provider_api_format = decision
        .provider_api_format
        .as_deref()
        .unwrap_or(context.client_api_format.as_str());
    let resolved_model = decision
        .mapped_model
        .as_deref()
        .or(decision.model_name.as_deref())
        .unwrap_or(input.requested_model.as_str());
    let original_provider_request_body = decision.provider_request_body.clone();
    let mut provider_request_body = original_provider_request_body
        .clone()
        .unwrap_or(serde_json::Value::Null);
    let mut provider_headers = btree_headers_to_header_map(&decision.provider_request_headers)?;
    let provider_headers_json = headers_to_routing_value(&provider_headers);
    let policy = resolve_gateway_routing_policy(GatewayRoutingPolicyInput {
        group_id: context.group_id.as_deref(),
        group_version: context.group_version,
        group_config_json: &context.group_config_json,
        selection_source: context.selection_source.as_str(),
        requested_model: input.requested_model.as_str(),
        resolved_model,
        api_format: provider_api_format,
        user_id: Some(input.auth_context.user_id.as_str()),
        api_key_id: Some(input.auth_context.api_key_id.as_str()),
        headers: &provider_headers_json,
        body: &provider_request_body,
        phase: RoutingRulePhase::ProviderRequest,
    })?;
    ensure_report_context_routing_trace(input, decision, &policy);
    if policy.mutation_plan.is_empty() {
        return Ok(());
    }
    if original_provider_request_body.is_none() && !policy.mutation_plan.body_patch.is_empty() {
        return Err(GatewayError::Client {
            status: StatusCode::BAD_REQUEST,
            message: "routing provider_request body patch cannot be applied to a binary or empty upstream body".to_string(),
        });
    }
    apply_routing_mutation_plan(
        &mut provider_request_body,
        &mut provider_headers,
        &policy.mutation_plan,
    )?;
    decision.provider_request_headers = header_map_to_btree_headers(&provider_headers);
    if original_provider_request_body.is_some() {
        decision.provider_request_body = Some(provider_request_body);
    }
    update_report_context_provider_request_mutation(decision, &policy);
    Ok(())
}

pub(crate) async fn apply_final_provider_request_policies_to_decision(
    input: &LocalRequestedModelDecisionInput,
    decision: &mut AiExecutionDecision,
) -> Result<(), GatewayError> {
    let already_applied = has_applied_managed_instructions_state(decision);
    if !already_applied {
        apply_provider_request_routing_policy_to_decision(input, decision)?;
    }
    let snapshot = input
        .managed_instructions_snapshot
        .get_or_try_init(|| async {
            let managed_instructions = input
                .auth_snapshot
                .api_key_group_managed_instructions
                .as_ref();
            let config = resolve_managed_instructions_config(managed_instructions)
                .map_err(GatewayError::Internal)?;
            Ok::<ManagedInstructionsBindingSnapshot, GatewayError>(
                ManagedInstructionsBindingSnapshot {
                    user_group_id: input.auth_snapshot.api_key_group_id.clone(),
                    managed_instructions_config_value: managed_instructions.cloned(),
                    config,
                },
            )
        })
        .await?;
    let current_user_group_id = input.auth_snapshot.api_key_group_id.as_ref();
    let current_managed_instructions_config_value = input
        .auth_snapshot
        .api_key_group_managed_instructions
        .as_ref();
    if snapshot.user_group_id.as_ref() != current_user_group_id
        || snapshot.managed_instructions_config_value.as_ref()
            != current_managed_instructions_config_value
    {
        return Err(GatewayError::Internal(format!(
            "同一请求的用户分组或受管理提示词配置发生变化：原分组 {:?}，当前分组 {:?}",
            snapshot.user_group_id, current_user_group_id,
        )));
    }
    if let Some(config) = snapshot.config.as_ref() {
        let user_group_id = snapshot
            .user_group_id
            .as_deref()
            .ok_or_else(|| GatewayError::Internal("受管理提示词配置缺少用户分组 ID".to_string()))?;
        apply_managed_instructions_to_decision(decision, config)?;
        record_managed_instructions_user_group(decision, user_group_id)?;
    }
    Ok(())
}

struct GatewayAuthenticatedDecisionInputPort<'a> {
    state: PlannerAppState<'a>,
    now_unix_secs: u64,
}

#[async_trait]
impl AiAuthenticatedDecisionInputPort for GatewayAuthenticatedDecisionInputPort<'_> {
    type AuthContext = ExecutionRuntimeAuthContext;
    type AuthSnapshot = GatewayAuthApiKeySnapshot;
    type RequiredCapabilities = serde_json::Value;
    type ResolvedInput = ResolvedLocalDecisionAuthInput;
    type Error = GatewayError;

    async fn read_auth_snapshot(
        &self,
        auth_context: &Self::AuthContext,
    ) -> Result<Option<Self::AuthSnapshot>, Self::Error> {
        self.state
            .read_auth_api_key_snapshot(
                &auth_context.user_id,
                &auth_context.api_key_id,
                self.now_unix_secs,
            )
            .await
    }

    async fn resolve_required_capabilities(
        &self,
        auth_context: &Self::AuthContext,
        requested_model: Option<&str>,
        explicit_required_capabilities: Option<&Self::RequiredCapabilities>,
    ) -> Result<Option<Self::RequiredCapabilities>, Self::Error> {
        Ok(self
            .state
            .resolve_request_candidate_required_capabilities(
                &auth_context.user_id,
                &auth_context.api_key_id,
                requested_model,
                explicit_required_capabilities,
            )
            .await)
    }

    fn build_resolved_input(
        &self,
        auth_context: Self::AuthContext,
        auth_snapshot: Self::AuthSnapshot,
        required_capabilities: Option<Self::RequiredCapabilities>,
    ) -> Self::ResolvedInput {
        ResolvedLocalDecisionAuthInput {
            auth_context,
            auth_snapshot,
            required_capabilities,
        }
    }
}

pub(crate) fn build_local_requested_model_decision_input(
    resolved_input: ResolvedLocalDecisionAuthInput,
    requested_model: String,
) -> LocalRequestedModelDecisionInput {
    LocalRequestedModelDecisionInput {
        auth_context: resolved_input.auth_context,
        requested_model,
        auth_snapshot: resolved_input.auth_snapshot,
        required_capabilities: resolved_input.required_capabilities,
        request_auth_channel: None,
        client_session_affinity: None,
        defer_scheduler_affinity_until_success: false,
        routing_policy: None,
        routing_trace_seed: None,
        routing_context: None,
        codex_oauth_identity_convergence: None,
        managed_instructions_snapshot: Arc::new(tokio::sync::OnceCell::new()),
    }
}

pub(crate) async fn attach_routing_policy_to_local_requested_model_input(
    state: &AppState,
    parts: &http::request::Parts,
    input: &mut LocalRequestedModelDecisionInput,
    body_json: &Value,
    client_api_format: &str,
) -> Result<(), GatewayError> {
    let explicit_group = routing_header_value_str(&parts.headers, ROUTING_GROUP_HEADER);
    let selected_group = match state.routing_group_read_repository() {
        Some(repository) => {
            let user_group_ids = match state
                .list_user_groups_for_user(&input.auth_context.user_id)
                .await
            {
                Ok(groups) => groups.into_iter().map(|group| group.id).collect::<Vec<_>>(),
                Err(error) => {
                    warn!(
                        user_id = %input.auth_context.user_id,
                        error = ?error,
                        "gateway routing profile user group lookup failed"
                    );
                    Vec::new()
                }
            };
            let selection = select_gateway_routing_group(
                repository.as_ref(),
                GatewayRoutingSelectionInput {
                    explicit_group: explicit_group.as_deref(),
                    user_id: Some(input.auth_context.user_id.as_str()),
                    api_key_id: Some(input.auth_context.api_key_id.as_str()),
                    user_group_ids: &user_group_ids,
                },
            )
            .await
            .map_err(routing_selection_error)?;
            selection.group.map(|group| {
                (
                    Some(group.id),
                    Some(group.version),
                    group.config_json,
                    selection.source,
                )
            })
        }
        None => {
            if explicit_group
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                return Err(routing_selection_error(
                    GatewayRoutingSelectionError::NotFound(explicit_group.unwrap_or_default()),
                ));
            }
            None
        }
    };

    let Some((group_id, group_version, group_config_json, selection_source)) = selected_group
    else {
        input.client_session_affinity =
            client_session_affinity_from_request(&parts.headers, Some(body_json));
        input.defer_scheduler_affinity_until_success =
            codex_encrypted_context_handoff_applies(client_api_format)
                && codex_encrypted_context_handoff_from_request(
                    &parts.headers,
                    Some(body_json),
                    input.client_session_affinity.as_ref(),
                );
        input.routing_policy = None;
        input.routing_trace_seed = None;
        input.routing_context = None;
        input.codex_oauth_identity_convergence =
            Some(build_codex_oauth_identity_convergence_request_context(
                &parts.headers,
                body_json,
                input.client_session_affinity.as_ref(),
                &input.auth_context.api_key_id,
            )?);
        return Ok(());
    };

    let headers_json = headers_to_routing_value(&parts.headers);
    let policy = resolve_gateway_routing_policy(GatewayRoutingPolicyInput {
        group_id: group_id.as_deref(),
        group_version,
        group_config_json: &group_config_json,
        selection_source: selection_source.as_str(),
        requested_model: input.requested_model.as_str(),
        resolved_model: input.requested_model.as_str(),
        api_format: client_api_format,
        user_id: Some(input.auth_context.user_id.as_str()),
        api_key_id: Some(input.auth_context.api_key_id.as_str()),
        headers: &headers_json,
        body: body_json,
        phase: RoutingRulePhase::ClientRequest,
    })?;
    let mut effective_body_json = body_json.clone();
    let mut effective_headers = parts.headers.clone();
    apply_routing_mutation_plan(
        &mut effective_body_json,
        &mut effective_headers,
        &policy.mutation_plan,
    )?;

    let mut requested_model_changed = false;
    if let Some(mut mutated_model) = extract_standard_requested_model(&effective_body_json) {
        mutated_model = mutated_model.trim().to_string();
        if !mutated_model.is_empty() && mutated_model != input.requested_model {
            input.requested_model = mutated_model;
            requested_model_changed = true;
        }
    }
    if requested_model_changed {
        input.required_capabilities = PlannerAppState::new(state)
            .resolve_request_candidate_required_capabilities(
                &input.auth_context.user_id,
                &input.auth_context.api_key_id,
                Some(input.requested_model.as_str()),
                input.required_capabilities.as_ref(),
            )
            .await;
    }

    let effective_headers_json = headers_to_routing_value(&effective_headers);
    input.client_session_affinity =
        client_session_affinity_from_request(&effective_headers, Some(&effective_body_json));
    input.defer_scheduler_affinity_until_success =
        codex_encrypted_context_handoff_applies(client_api_format)
            && codex_encrypted_context_handoff_from_request(
                &effective_headers,
                Some(&effective_body_json),
                input.client_session_affinity.as_ref(),
            );
    let mut final_policy = resolve_gateway_routing_policy(GatewayRoutingPolicyInput {
        group_id: group_id.as_deref(),
        group_version,
        group_config_json: &group_config_json,
        selection_source: selection_source.as_str(),
        requested_model: input.requested_model.as_str(),
        resolved_model: input.requested_model.as_str(),
        api_format: client_api_format,
        user_id: Some(input.auth_context.user_id.as_str()),
        api_key_id: Some(input.auth_context.api_key_id.as_str()),
        headers: &effective_headers_json,
        body: &effective_body_json,
        phase: RoutingRulePhase::ClientRequest,
    })?;
    final_policy.mutation_plan = policy.mutation_plan.clone();
    input.routing_trace_seed = Some(build_routing_trace_seed(&final_policy, client_api_format));
    input.routing_policy = Some(final_policy);
    input.codex_oauth_identity_convergence =
        Some(build_codex_oauth_identity_convergence_request_context(
            &effective_headers,
            &effective_body_json,
            input.client_session_affinity.as_ref(),
            &input.auth_context.api_key_id,
        )?);
    input.routing_context = Some(LocalRoutingRequestContext {
        group_id,
        group_version,
        group_config_json,
        selection_source,
        client_api_format: client_api_format.to_string(),
        effective_body_json,
        effective_headers,
    });
    Ok(())
}

fn codex_encrypted_context_handoff_applies(client_api_format: &str) -> bool {
    matches!(
        client_api_format.trim().to_ascii_lowercase().as_str(),
        "openai:responses" | "openai:responses:compact"
    )
}

pub(crate) fn build_local_authenticated_decision_input(
    resolved_input: ResolvedLocalDecisionAuthInput,
) -> LocalAuthenticatedDecisionInput {
    LocalAuthenticatedDecisionInput {
        auth_context: resolved_input.auth_context,
        auth_snapshot: resolved_input.auth_snapshot,
        required_capabilities: resolved_input.required_capabilities,
        client_session_affinity: None,
    }
}

pub(crate) async fn resolve_local_authenticated_decision_input(
    state: &AppState,
    auth_context: ExecutionRuntimeAuthContext,
    requested_model: Option<&str>,
    explicit_required_capabilities: Option<&serde_json::Value>,
) -> Result<Option<ResolvedLocalDecisionAuthInput>, GatewayError> {
    let port = GatewayAuthenticatedDecisionInputPort {
        state: PlannerAppState::new(state),
        now_unix_secs: current_unix_secs(),
    };

    run_ai_authenticated_decision_input(
        &port,
        auth_context,
        requested_model,
        explicit_required_capabilities,
    )
    .await
}

fn routing_selection_error(error: GatewayRoutingSelectionError) -> GatewayError {
    GatewayError::Client {
        status: StatusCode::FORBIDDEN,
        message: error.to_string(),
    }
}

fn headers_to_routing_value(headers: &http::HeaderMap) -> Value {
    let mut object = serde_json::Map::new();
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            object.insert(name.as_str().to_ascii_lowercase(), json!(value));
        }
    }
    Value::Object(object)
}

fn routing_header_value_str(headers: &http::HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn btree_headers_to_header_map(
    headers: &BTreeMap<String, String>,
) -> Result<HeaderMap, GatewayError> {
    let mut output = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|err| GatewayError::Client {
            status: StatusCode::BAD_REQUEST,
            message: format!("invalid provider request header name in routing mutation: {err}"),
        })?;
        let value = HeaderValue::from_str(value).map_err(|err| GatewayError::Client {
            status: StatusCode::BAD_REQUEST,
            message: format!("invalid provider request header value in routing mutation: {err}"),
        })?;
        output.insert(name, value);
    }
    Ok(output)
}

fn header_map_to_btree_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn update_report_context_provider_request_mutation(
    decision: &mut AiExecutionDecision,
    policy: &ResolvedRoutingPolicy,
) {
    let Some(serde_json::Value::Object(object)) = decision.report_context.as_mut() else {
        return;
    };
    let body_paths = policy
        .mutation_plan
        .body_patch
        .iter()
        .map(|operation| operation.path().to_string())
        .collect::<Vec<_>>();
    let header_names = policy
        .mutation_plan
        .header_patch
        .iter()
        .map(|operation| operation.name().to_string())
        .collect::<Vec<_>>();
    let trace_patch_summary = serde_json::json!({
        "body_paths": body_paths,
        "header_names": header_names,
    });
    if let Some(serde_json::Value::Object(routing_trace)) = object.get_mut("routing_trace") {
        routing_trace.insert(
            "provider_request_patch_summary".to_string(),
            trace_patch_summary.clone(),
        );
    }
    object.insert(
        "provider_request_headers".to_string(),
        serde_json::json!(decision.provider_request_headers),
    );
    object.insert(
        "routing_provider_request_patch_summary".to_string(),
        serde_json::json!({
            "body_paths": trace_patch_summary["body_paths"].clone(),
            "header_names": trace_patch_summary["header_names"].clone(),
            "matched_rules": policy
                .matched_rules
                .iter()
                .map(|rule| rule.id.clone())
                .collect::<Vec<_>>()
        }),
    );
}

fn ensure_report_context_routing_trace(
    input: &LocalRequestedModelDecisionInput,
    decision: &mut AiExecutionDecision,
    policy: &ResolvedRoutingPolicy,
) {
    let Some(serde_json::Value::Object(object)) = decision.report_context.as_mut() else {
        return;
    };
    if object.get("routing_trace").is_some() {
        return;
    }

    let client_api_format = decision
        .client_api_format
        .as_deref()
        .or_else(|| {
            input
                .routing_context
                .as_ref()
                .map(|context| context.client_api_format.as_str())
        })
        .unwrap_or_default();
    let mut trace = input
        .routing_trace_seed
        .clone()
        .unwrap_or_else(|| build_routing_trace_seed(policy, client_api_format));

    let candidate_group_id = object
        .get("candidate_group_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let pool_key_index = object
        .get("pool_key_index")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let is_pool_expansion = candidate_group_id.is_some() && pool_key_index.is_some();
    let candidate_kind = if is_pool_expansion {
        CandidateKind::PoolGroup
    } else {
        CandidateKind::Provider
    };
    let provider_id = candidate_group_id
        .clone()
        .or_else(|| decision.provider_id.clone())
        .unwrap_or_default();
    let endpoint_id = decision.endpoint_id.clone().unwrap_or_default();
    let model_id = object
        .get("model_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| decision.mapped_model.clone())
        .or_else(|| decision.model_name.clone())
        .unwrap_or_else(|| input.requested_model.clone());
    let key_id = decision.key_id.clone().filter(|_| !is_pool_expansion);
    let provider_priority = object
        .get("provider_priority")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or_default();
    let key_priority = object
        .get("priority_slot")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or_default();
    trace.global_candidates.push(RoutingCandidateTrace {
        candidate_kind,
        provider_id: provider_id.clone(),
        endpoint_id,
        model_id: model_id.clone(),
        key_id: key_id.clone(),
        ranking_vector: rank_vector_for_candidate(
            &policy.ranking_overlay,
            &RoutingCandidateFacts {
                candidate_kind,
                provider_id: provider_id.clone(),
                endpoint_id: decision.endpoint_id.clone().unwrap_or_default(),
                model_id,
                key_id,
                provider_priority,
                key_priority,
            },
        ),
        skip_reason: None,
        selected_order: object
            .get("candidate_index")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
    });

    if is_pool_expansion {
        if let (Some(pool_group_id), Some(key_id)) = (candidate_group_id, decision.key_id.clone()) {
            trace.pool_expansion.push(RoutingPoolExpansionTrace {
                pool_group_id,
                key_id,
                pool_ranking_vector: Vec::new(),
                pool_skip_reason: None,
                selected_order: pool_key_index,
            });
        }
    }

    object.insert("routing_trace".to_string(), serde_json::json!(trace));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_auth_context() -> ExecutionRuntimeAuthContext {
        ExecutionRuntimeAuthContext {
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
            api_key_is_standalone: false,
        }
    }

    fn sample_auth_snapshot() -> GatewayAuthApiKeySnapshot {
        GatewayAuthApiKeySnapshot {
            user_id: "user-1".to_string(),
            username: "alice".to_string(),
            email: None,
            user_role: "user".to_string(),
            user_auth_source: "local".to_string(),
            user_is_active: true,
            user_is_deleted: false,
            user_rate_limit: None,
            user_concurrent_limit: None,
            user_allowed_providers: None,
            user_allowed_api_formats: None,
            user_allowed_models: None,
            api_key_id: "api-key-1".to_string(),
            api_key_name: Some("default".to_string()),
            api_key_group_id: None,
            api_key_group_name: None,
            api_key_group_visibility: None,
            api_key_group_sales_multiplier: 1.0,
            api_key_group_model_sales_multipliers: None,
            api_key_group_managed_instructions: None,
            api_key_is_active: true,
            api_key_is_locked: false,
            api_key_is_standalone: false,
            api_key_rate_limit: None,
            api_key_concurrent_limit: None,
            api_key_expires_at_unix_secs: None,
            api_key_allowed_providers: None,
            api_key_allowed_api_formats: None,
            api_key_allowed_models: None,
            currently_usable: true,
        }
    }

    fn sample_decision_input() -> LocalRequestedModelDecisionInput {
        LocalRequestedModelDecisionInput {
            auth_context: sample_auth_context(),
            requested_model: "gpt-5".to_string(),
            auth_snapshot: sample_auth_snapshot(),
            required_capabilities: None,
            request_auth_channel: None,
            client_session_affinity: None,
            defer_scheduler_affinity_until_success: false,
            routing_policy: None,
            routing_trace_seed: None,
            routing_context: Some(LocalRoutingRequestContext {
                group_id: Some("group-1".to_string()),
                group_version: Some(3),
                selection_source: "explicit_header".to_string(),
                client_api_format: "openai:chat".to_string(),
                effective_body_json: json!({"model":"gpt-5"}),
                effective_headers: HeaderMap::new(),
                group_config_json: json!({
                    "allowed_models": ["gpt-5"],
                    "rules": [{
                        "id": "provider-patch",
                        "priority": 1,
                        "enabled": true,
                        "phase": "provider_request",
                        "conditions": {},
                        "actions": [
                            {
                                "type": "json_patch_body",
                                "patch": [{
                                    "op": "add",
                                    "path": "/metadata/routing",
                                    "value": "provider"
                                }]
                            },
                            {
                                "type": "patch_headers",
                                "patch": [{
                                    "op": "set",
                                    "name": "x-provider-route",
                                    "value": "provider"
                                }]
                            }
                        ]
                    }]
                }),
            }),
            codex_oauth_identity_convergence: None,
            managed_instructions_snapshot: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    fn set_managed_instructions(
        input: &mut LocalRequestedModelDecisionInput,
        user_group_id: &str,
        profile_id: &str,
        enabled: bool,
    ) {
        input.auth_snapshot.api_key_group_id = Some(user_group_id.to_string());
        input.auth_snapshot.api_key_group_managed_instructions = Some(json!({
            "enabled": enabled,
            "profile_id": profile_id,
            "merge_mode": "prepend"
        }));
    }

    fn sample_decision() -> AiExecutionDecision {
        AiExecutionDecision {
            action: "execution_runtime_sync_decision".to_string(),
            decision_kind: Some("openai_chat_sync".to_string()),
            execution_strategy: None,
            conversion_mode: None,
            request_id: Some("trace-1".to_string()),
            candidate_id: Some("candidate-1".to_string()),
            provider_name: Some("provider".to_string()),
            provider_id: Some("provider-1".to_string()),
            endpoint_id: Some("endpoint-1".to_string()),
            key_id: Some("key-1".to_string()),
            upstream_base_url: None,
            upstream_url: None,
            provider_request_method: None,
            auth_header: None,
            auth_value: None,
            provider_api_format: Some("openai:chat".to_string()),
            client_api_format: Some("openai:chat".to_string()),
            provider_contract: None,
            client_contract: None,
            model_name: Some("gpt-5".to_string()),
            mapped_model: Some("gpt-5".to_string()),
            prompt_cache_key: None,
            extra_headers: BTreeMap::new(),
            provider_request_headers: BTreeMap::from([(
                "content-type".to_string(),
                "application/json".to_string(),
            )]),
            provider_request_body: Some(json!({"model":"gpt-5","metadata":{}})),
            provider_request_body_base64: None,
            content_type: Some("application/json".to_string()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
            upstream_is_stream: false,
            report_kind: Some("local_sync_success".to_string()),
            report_context: Some(json!({
                "candidate_index": 0,
                "retry_index": 0,
                "model_id": "model-1"
            })),
            auth_context: Some(sample_auth_context()),
        }
    }

    fn set_provider_request_rules(input: &mut LocalRequestedModelDecisionInput, actions: Value) {
        let config = json!({
            "allowed_models": ["gpt-5"],
            "rules": [{
                "id": "provider-patch",
                "priority": 1,
                "enabled": true,
                "phase": "provider_request",
                "conditions": {},
                "actions": actions
            }]
        });
        input
            .routing_context
            .as_mut()
            .expect("sample input should include routing context")
            .group_config_json = config;
    }

    #[test]
    fn provider_request_routing_policy_mutates_decision_body_headers_and_report_context() {
        let input = sample_decision_input();
        let mut decision = sample_decision();

        apply_provider_request_routing_policy_to_decision(&input, &mut decision)
            .expect("provider routing mutation should apply");

        assert_eq!(
            decision.provider_request_body.as_ref().unwrap()["metadata"]["routing"],
            json!("provider")
        );
        assert_eq!(
            decision
                .provider_request_headers
                .get("x-provider-route")
                .map(String::as_str),
            Some("provider")
        );
        let report_context = decision.report_context.as_ref().unwrap();
        assert_eq!(
            report_context["routing_provider_request_patch_summary"]["matched_rules"],
            json!(["provider-patch"])
        );
        assert_eq!(
            report_context["routing_trace"]["provider_request_patch_summary"]["body_paths"],
            json!(["/metadata/routing"])
        );
        assert_eq!(
            report_context["routing_trace"]["global_candidates"][0]["provider_id"],
            json!("provider-1")
        );
    }

    #[test]
    fn managed_instructions_preserve_provider_request_routing_body_result() {
        let mut input = sample_decision_input();
        set_provider_request_rules(
            &mut input,
            json!([{
                "type": "json_patch_body",
                "patch": [{
                    "op": "add",
                    "path": "/instructions",
                    "value": "instructions from provider routing"
                }]
            }]),
        );
        let mut decision = sample_decision();
        decision.provider_api_format = Some("openai:responses".to_string());

        apply_provider_request_routing_policy_to_decision(&input, &mut decision)
            .expect("provider routing mutation should apply");
        let config = crate::managed_instructions::ResolvedManagedInstructionsConfig {
            enabled: true,
            merge_mode: crate::managed_instructions::ManagedInstructionsMergeMode::Prepend,
            profile: crate::managed_instructions::managed_instructions_profile(
                "security_research_v1",
            )
            .expect("security profile"),
        };
        crate::managed_instructions::apply_managed_instructions_to_decision(&mut decision, &config)
            .expect("managed instructions should apply after routing");

        let instructions = decision.provider_request_body.as_ref().unwrap()["instructions"]
            .as_str()
            .expect("instructions string");
        assert!(instructions.starts_with(&config.profile.embedded_text));
        assert!(instructions.contains(
            "<niffler-client-instructions>\ninstructions from provider routing\n</niffler-client-instructions>"
        ));
    }

    #[test]
    fn cloned_decision_inputs_share_one_managed_instruction_binding_snapshot() {
        let input = sample_decision_input();
        let cloned = input.clone();
        input
            .managed_instructions_snapshot
            .set(ManagedInstructionsBindingSnapshot {
                user_group_id: Some("user-group-1".to_string()),
                managed_instructions_config_value: None,
                config: None,
            })
            .expect("snapshot should initialize once");

        assert_eq!(
            cloned
                .managed_instructions_snapshot
                .get()
                .and_then(|snapshot| snapshot.user_group_id.as_deref()),
            Some("user-group-1")
        );
        assert!(cloned
            .managed_instructions_snapshot
            .set(ManagedInstructionsBindingSnapshot {
                user_group_id: Some("user-group-2".to_string()),
                managed_instructions_config_value: None,
                config: None,
            })
            .is_err());
    }

    #[tokio::test]
    async fn final_provider_request_policy_applies_user_group_config_and_ignores_routing_group_switch(
    ) {
        let mut input = sample_decision_input();
        set_managed_instructions(
            &mut input,
            "user-group-security",
            "security_research_v1",
            true,
        );

        let mut responses_decision = sample_decision();
        responses_decision.provider_api_format = Some("openai:responses".to_string());
        responses_decision.provider_request_body = Some(json!({
            "model": "gpt-5",
            "instructions": "client responses instructions",
            "input": [],
            "metadata": {}
        }));
        apply_final_provider_request_policies_to_decision(&input, &mut responses_decision)
            .await
            .expect("user group profile should apply to Responses");
        let responses_instructions = responses_decision.provider_request_body.as_ref().unwrap()
            ["instructions"]
            .as_str()
            .expect("Responses instructions string");
        assert!(responses_instructions.starts_with(
            &crate::managed_instructions::managed_instructions_profile("security_research_v1")
                .expect("security profile")
                .embedded_text
        ));
        assert!(responses_instructions.contains("client responses instructions"));
        assert_eq!(
            responses_decision.report_context.as_ref().unwrap()["managed_instructions"]
                ["target_field"],
            json!("instructions")
        );
        assert_eq!(
            responses_decision.report_context.as_ref().unwrap()["managed_instructions"]
                ["user_group_id"],
            json!("user-group-security")
        );

        let mut chat_decision = sample_decision();
        chat_decision.provider_request_body = Some(json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "hello"}],
            "metadata": {}
        }));
        apply_final_provider_request_policies_to_decision(&input, &mut chat_decision)
            .await
            .expect("user group profile should apply to Chat");
        assert_eq!(
            chat_decision.provider_request_body.as_ref().unwrap()["messages"][0]["role"],
            json!("system")
        );
        assert_eq!(
            chat_decision.provider_request_body.as_ref().unwrap()["messages"][1],
            json!({"role": "user", "content": "hello"})
        );
        assert_eq!(
            chat_decision.report_context.as_ref().unwrap()["managed_instructions"]["target_field"],
            json!("messages[0]")
        );

        let original_claude_system = json!({
            "type": "text",
            "text": "client claude system",
            "cache_control": {"type": "ephemeral"}
        });
        let mut claude_decision = sample_decision();
        claude_decision.provider_api_format = Some("claude:messages".to_string());
        claude_decision.provider_request_body = Some(json!({
            "model": "claude-sonnet",
            "system": [original_claude_system.clone()],
            "messages": [],
            "metadata": {}
        }));
        apply_final_provider_request_policies_to_decision(&input, &mut claude_decision)
            .await
            .expect("user group profile should apply to Claude");
        assert_eq!(
            claude_decision.provider_request_body.as_ref().unwrap()["system"][0]["type"],
            json!("text")
        );
        assert_eq!(
            claude_decision.provider_request_body.as_ref().unwrap()["system"][1],
            original_claude_system
        );
        assert_eq!(
            claude_decision.report_context.as_ref().unwrap()["managed_instructions"]
                ["target_field"],
            json!("system[0]")
        );

        assert_eq!(
            input
                .managed_instructions_snapshot
                .get()
                .and_then(|snapshot| snapshot.user_group_id.as_deref()),
            Some("user-group-security")
        );

        let mut switched_routing_input = input.clone();
        let switched_context = switched_routing_input
            .routing_context
            .as_mut()
            .expect("sample input should include routing context");
        switched_context.group_id = Some("group-2".to_string());
        switched_context.group_version = Some(4);
        let mut switched_decision = sample_decision();
        switched_decision.provider_request_body = Some(json!({
            "model": "gpt-5",
            "messages": [],
            "metadata": {}
        }));
        apply_final_provider_request_policies_to_decision(
            &switched_routing_input,
            &mut switched_decision,
        )
        .await
        .expect("routing group switch must not change the managed profile");
        assert_eq!(
            switched_decision.report_context.as_ref().unwrap()["managed_instructions"]
                ["user_group_id"],
            json!("user-group-security")
        );

        let mut switched_user_group_input = input.clone();
        switched_user_group_input.auth_snapshot.api_key_group_id =
            Some("user-group-adult".to_string());
        let mut conflicting_decision = sample_decision();
        let error = apply_final_provider_request_policies_to_decision(
            &switched_user_group_input,
            &mut conflicting_decision,
        )
        .await
        .expect_err("user group switch inside one request should fail");
        assert!(
            matches!(error, GatewayError::Internal(message) if message.contains("user-group-adult"))
        );
    }

    #[tokio::test]
    async fn final_provider_request_policy_is_idempotent_after_provider_routing() {
        let mut input = sample_decision_input();
        set_provider_request_rules(
            &mut input,
            json!([{
                "type": "json_patch_body",
                "patch": [{
                    "op": "add",
                    "path": "/instructions",
                    "value": "instructions from provider routing"
                }]
            }]),
        );
        set_managed_instructions(
            &mut input,
            "user-group-security",
            "security_research_v1",
            true,
        );
        let mut decision = sample_decision();
        decision.provider_api_format = Some("openai:responses".to_string());

        apply_final_provider_request_policies_to_decision(&input, &mut decision)
            .await
            .expect("first application should succeed");
        let body_after_first_application = decision.provider_request_body.clone();

        apply_final_provider_request_policies_to_decision(&input, &mut decision)
            .await
            .expect("second application should deduplicate");

        assert_eq!(decision.provider_request_body, body_after_first_application);
        let metadata = &decision.report_context.as_ref().unwrap()["managed_instructions"];
        assert_eq!(metadata["applied"], json!(true));
        assert_eq!(metadata["deduplicated"], json!(true));
        assert_eq!(metadata["reason"], json!("already_applied"));
    }

    #[tokio::test]
    async fn final_provider_request_policy_rejects_user_group_config_change() {
        let mut input = sample_decision_input();
        set_managed_instructions(
            &mut input,
            "user-group-security",
            "security_research_v1",
            true,
        );
        let mut first_decision = sample_decision();
        first_decision.provider_api_format = Some("openai:responses".to_string());
        apply_final_provider_request_policies_to_decision(&input, &mut first_decision)
            .await
            .expect("first group configuration should apply");

        let mut changed_input = input.clone();
        changed_input
            .auth_snapshot
            .api_key_group_managed_instructions = Some(json!({
            "enabled": true,
            "profile_id": "adult_fiction_v1",
            "merge_mode": "prepend"
        }));
        let mut changed_decision = sample_decision();
        changed_decision.provider_api_format = Some("openai:responses".to_string());
        let error = apply_final_provider_request_policies_to_decision(
            &changed_input,
            &mut changed_decision,
        )
        .await
        .expect_err("same user group with different configuration should fail");

        assert!(matches!(error, GatewayError::Internal(message) if message.contains("配置")));
    }

    #[tokio::test]
    async fn separate_requests_use_their_api_key_user_group_profile() {
        let mut security_input = sample_decision_input();
        set_managed_instructions(
            &mut security_input,
            "user-group-security",
            "security_research_v1",
            true,
        );
        let mut adult_input = sample_decision_input();
        set_managed_instructions(
            &mut adult_input,
            "user-group-adult",
            "adult_fiction_v1",
            true,
        );
        let mut security_decision = sample_decision();
        security_decision.provider_request_body = Some(json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "security"}],
            "metadata": {}
        }));
        let mut adult_decision = sample_decision();
        adult_decision.provider_request_body = Some(json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "adult"}],
            "metadata": {}
        }));

        apply_final_provider_request_policies_to_decision(&security_input, &mut security_decision)
            .await
            .expect("security user group should apply");
        apply_final_provider_request_policies_to_decision(&adult_input, &mut adult_decision)
            .await
            .expect("adult user group should apply");

        let security_system = security_decision.provider_request_body.as_ref().unwrap()["messages"]
            [0]["content"]
            .as_str()
            .expect("security system message");
        let adult_system = adult_decision.provider_request_body.as_ref().unwrap()["messages"][0]
            ["content"]
            .as_str()
            .expect("adult system message");
        assert!(security_system.contains("Isolated CTF laboratory environment:"));
        assert!(!security_system.contains("Creative-writing scope:"));
        assert!(adult_system.contains("Creative-writing scope:"));
        assert!(!adult_system.contains("Isolated CTF laboratory environment:"));
    }

    #[tokio::test]
    async fn final_provider_request_policy_records_user_group_when_profile_is_disabled() {
        let mut input = sample_decision_input();
        set_managed_instructions(&mut input, "user-group-adult", "adult_fiction_v1", false);
        let mut decision = sample_decision();
        decision.provider_request_body = Some(json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "hello"}],
            "metadata": {}
        }));

        apply_final_provider_request_policies_to_decision(&input, &mut decision)
            .await
            .expect("disabled group profile should leave instructions unchanged");

        assert_eq!(
            decision.provider_request_body.as_ref().unwrap()["messages"],
            json!([{"role": "user", "content": "hello"}])
        );
        let metadata = &decision.report_context.as_ref().unwrap()["managed_instructions"];
        assert_eq!(metadata["applied"], json!(false));
        assert_eq!(metadata["deduplicated"], json!(false));
        assert_eq!(metadata["reason"], json!("disabled"));
        assert_eq!(metadata["user_group_id"], json!("user-group-adult"));
    }

    #[test]
    fn provider_request_routing_policy_rejects_body_patch_without_json_body() {
        let input = sample_decision_input();
        let mut decision = sample_decision();
        decision.provider_request_body = None;
        decision.provider_request_body_base64 = Some("AA==".to_string());

        let error = apply_provider_request_routing_policy_to_decision(&input, &mut decision)
            .expect_err("provider body patch should reject binary upstream bodies");

        match error {
            GatewayError::Client { status, message } => {
                assert_eq!(status, StatusCode::BAD_REQUEST);
                assert!(message.contains("binary or empty upstream body"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(
            decision
                .report_context
                .as_ref()
                .and_then(|context| context.get("routing_trace"))
                .is_some(),
            "failed provider_request mutation should still seed routing trace"
        );
    }

    #[tokio::test]
    async fn codex_encrypted_context_without_session_attaches_routing_policy_without_local_rejection(
    ) {
        let state = AppState::new().expect("state should build");
        let (parts, _) = http::Request::builder()
            .header(http::header::USER_AGENT, "codex-tui/0.135.0")
            .body(())
            .expect("request should build")
            .into_parts();
        let body = json!({
            "model": "gpt-5.5",
            "input": [{
                "type": "compaction",
                "encrypted_content": "encrypted-payload"
            }]
        });
        let resolved_input = ResolvedLocalDecisionAuthInput {
            auth_context: sample_auth_context(),
            auth_snapshot: sample_auth_snapshot(),
            required_capabilities: None,
        };
        let mut input =
            build_local_requested_model_decision_input(resolved_input, "gpt-5.5".to_string());

        attach_routing_policy_to_local_requested_model_input(
            &state,
            &parts,
            &mut input,
            &body,
            "openai:responses",
        )
        .await
        .expect("Codex encrypted context without explicit session should not be rejected locally");

        assert!(input.client_session_affinity.is_none());
        assert!(input.defer_scheduler_affinity_until_success);
        assert!(input.routing_policy.is_none());
    }

    #[tokio::test]
    async fn codex_encrypted_context_handoff_only_applies_to_responses_api_format() {
        let state = AppState::new().expect("state should build");
        let (parts, _) = http::Request::builder()
            .header(http::header::USER_AGENT, "codex-tui/0.135.0")
            .body(())
            .expect("request should build")
            .into_parts();
        let body = json!({
            "model": "gpt-5.5",
            "messages": [{
                "role": "user",
                "content": "hello",
                "encrypted_content": "encrypted-payload"
            }]
        });
        let resolved_input = ResolvedLocalDecisionAuthInput {
            auth_context: sample_auth_context(),
            auth_snapshot: sample_auth_snapshot(),
            required_capabilities: None,
        };
        let mut input =
            build_local_requested_model_decision_input(resolved_input, "gpt-5.5".to_string());

        attach_routing_policy_to_local_requested_model_input(
            &state,
            &parts,
            &mut input,
            &body,
            "openai:chat",
        )
        .await
        .expect("non-Responses Codex request should keep normal routing behavior");

        assert!(!input.defer_scheduler_affinity_until_success);
    }

    #[test]
    fn provider_request_routing_policy_allows_header_patch_without_json_body() {
        let mut input = sample_decision_input();
        set_provider_request_rules(
            &mut input,
            json!([{
                "type": "patch_headers",
                "patch": [{
                    "op": "set",
                    "name": "x-provider-route",
                    "value": "header-only"
                }]
            }]),
        );
        let mut decision = sample_decision();
        decision.provider_request_body = None;
        decision.provider_request_body_base64 = Some("AA==".to_string());

        apply_provider_request_routing_policy_to_decision(&input, &mut decision)
            .expect("header-only provider routing mutation should apply without JSON body");

        assert_eq!(decision.provider_request_body, None);
        assert_eq!(
            decision
                .provider_request_headers
                .get("x-provider-route")
                .map(String::as_str),
            Some("header-only")
        );
        assert_eq!(
            decision.report_context.as_ref().unwrap()["routing_trace"]
                ["provider_request_patch_summary"]["header_names"],
            json!(["x-provider-route"])
        );
    }

    #[test]
    fn provider_request_routing_trace_records_pool_expansion_candidate() {
        let input = sample_decision_input();
        let mut decision = sample_decision();
        decision.report_context = Some(json!({
            "candidate_index": 2,
            "retry_index": 2,
            "model_id": "model-1",
            "candidate_group_id": "pool-group-1",
            "pool_key_index": 1,
            "provider_priority": 7,
            "priority_slot": 3
        }));

        apply_provider_request_routing_policy_to_decision(&input, &mut decision)
            .expect("provider routing mutation should seed pool trace");

        let routing_trace = &decision.report_context.as_ref().unwrap()["routing_trace"];
        assert_eq!(
            routing_trace["global_candidates"][0]["candidate_kind"],
            json!("pool_group")
        );
        assert_eq!(
            routing_trace["global_candidates"][0]["provider_id"],
            json!("pool-group-1")
        );
        assert_eq!(routing_trace["global_candidates"][0]["key_id"], Value::Null);
        assert_eq!(
            routing_trace["pool_expansion"][0]["pool_group_id"],
            json!("pool-group-1")
        );
        assert_eq!(routing_trace["pool_expansion"][0]["key_id"], json!("key-1"));
        assert_eq!(
            routing_trace["pool_expansion"][0]["selected_order"],
            json!(1)
        );
    }
}

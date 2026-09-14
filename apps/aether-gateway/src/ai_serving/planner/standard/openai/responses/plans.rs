use std::collections::HashSet;

use async_trait::async_trait;
use tracing::warn;

use super::super::openai_request_is_image_generation_intent;
use super::decision::{
    build_local_openai_responses_candidate_attempt_source,
    maybe_build_local_openai_responses_decision_payload_for_candidate,
    resolve_local_openai_responses_decision_input, LocalOpenAiResponsesCandidateAttempt,
    LocalOpenAiResponsesCandidateAttemptSource, LocalOpenAiResponsesDecisionInput,
    LocalOpenAiResponsesSpec,
};
use crate::ai_serving::planner::candidate_materialization::LocalExecutionAttemptSource;
use crate::ai_serving::planner::plan_builders::{
    build_openai_responses_stream_plan_from_decision,
    build_openai_responses_sync_plan_from_decision, AiStreamAttempt, AiSyncAttempt,
};
use crate::ai_serving::planner::runtime_miss::{
    apply_local_runtime_candidate_evaluation_progress,
    apply_local_runtime_candidate_terminal_reason, set_local_runtime_miss_diagnostic_reason,
};
use crate::ai_serving::planner::spec_metadata::local_openai_responses_spec_metadata;
use crate::ai_serving::GatewayControlDecision;
pub(crate) use crate::ai_serving::{
    resolve_openai_responses_stream_spec as resolve_stream_spec,
    resolve_openai_responses_sync_spec as resolve_sync_spec,
};
use crate::orchestration::{
    local_stream_failover_policy_to_value, read_global_stream_failover_policy,
    LocalStreamFailoverPolicy,
};
use crate::{AppState, GatewayError};

pub(crate) struct LocalOpenAiResponsesSyncAttemptSource<'a> {
    state: &'a AppState,
    parts: &'a http::request::Parts,
    trace_id: &'a str,
    body_json: serde_json::Value,
    input: LocalOpenAiResponsesDecisionInput,
    spec: LocalOpenAiResponsesSpec,
    candidates: LocalOpenAiResponsesCandidateAttemptSource<'a>,
}

pub(crate) struct LocalOpenAiResponsesStreamAttemptSource<'a> {
    state: &'a AppState,
    parts: &'a http::request::Parts,
    trace_id: &'a str,
    body_json: serde_json::Value,
    input: LocalOpenAiResponsesDecisionInput,
    spec: LocalOpenAiResponsesSpec,
    candidates: LocalOpenAiResponsesCandidateAttemptSource<'a>,
    stream_failover_attempt_budget_enabled: bool,
    stream_failover_attempt_budget: StreamFailoverAttemptBudget,
    global_policy: LocalStreamFailoverPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamFailoverAttemptAdmission {
    Allowed,
    DuplicateAccount,
    BudgetExhausted,
}

#[derive(Default)]
struct StreamFailoverAttemptBudget {
    seen_accounts: HashSet<(String, String)>,
    attempts: u64,
}

impl StreamFailoverAttemptBudget {
    fn admit(
        &mut self,
        provider_id: &str,
        _endpoint_id: &str,
        key_id: &str,
        max_account_switches: u64,
    ) -> StreamFailoverAttemptAdmission {
        let account = (provider_id.to_string(), key_id.to_string());
        if !self.seen_accounts.insert(account) {
            return StreamFailoverAttemptAdmission::DuplicateAccount;
        }

        let attempts = &mut self.attempts;
        let max_attempts = max_account_switches.saturating_add(1);
        if *attempts >= max_attempts {
            return StreamFailoverAttemptAdmission::BudgetExhausted;
        }
        *attempts = attempts.saturating_add(1);
        StreamFailoverAttemptAdmission::Allowed
    }

    fn admit_attempt(
        &mut self,
        attempt: &LocalOpenAiResponsesCandidateAttempt,
        policy: &LocalStreamFailoverPolicy,
    ) -> StreamFailoverAttemptAdmission {
        if !policy.enabled {
            return StreamFailoverAttemptAdmission::Allowed;
        }
        let candidate = &attempt.eligible.candidate;
        self.admit(
            &candidate.provider_id,
            &candidate.endpoint_id,
            &candidate.key_id,
            policy.max_account_switches,
        )
    }
}

pub(super) async fn build_local_sync_attempt_source<'a>(
    state: &'a AppState,
    parts: &'a http::request::Parts,
    trace_id: &'a str,
    decision: &'a GatewayControlDecision,
    body_json: &'a serde_json::Value,
    spec: LocalOpenAiResponsesSpec,
) -> Result<Option<(LocalOpenAiResponsesSyncAttemptSource<'a>, usize)>, GatewayError> {
    let spec_metadata = local_openai_responses_spec_metadata(spec);
    let Some(input) = resolve_local_openai_responses_decision_input(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        spec_metadata.decision_kind,
    )
    .await?
    else {
        return Ok(None);
    };
    set_local_runtime_miss_diagnostic_reason(
        state,
        trace_id,
        decision,
        spec_metadata.decision_kind,
        Some(input.requested_model.as_str()),
        "candidate_evaluation_incomplete",
    );
    let effective_body_json = input.effective_body_json(body_json).clone();
    let (candidates, candidate_count) = build_local_openai_responses_candidate_attempt_source(
        state,
        trace_id,
        &input,
        &effective_body_json,
        spec,
    )
    .await?;
    apply_local_runtime_candidate_evaluation_progress(state, trace_id, candidate_count);
    if candidate_count == 0 {
        return Ok(None);
    }

    Ok(Some((
        LocalOpenAiResponsesSyncAttemptSource {
            state,
            parts,
            trace_id,
            body_json: effective_body_json,
            input,
            spec,
            candidates,
        },
        candidate_count,
    )))
}

pub(super) async fn build_local_stream_attempt_source<'a>(
    state: &'a AppState,
    parts: &'a http::request::Parts,
    trace_id: &'a str,
    decision: &'a GatewayControlDecision,
    body_json: &'a serde_json::Value,
    spec: LocalOpenAiResponsesSpec,
) -> Result<Option<(LocalOpenAiResponsesStreamAttemptSource<'a>, usize)>, GatewayError> {
    let spec_metadata = local_openai_responses_spec_metadata(spec);
    let Some(input) = resolve_local_openai_responses_decision_input(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        spec_metadata.decision_kind,
    )
    .await?
    else {
        return Ok(None);
    };
    set_local_runtime_miss_diagnostic_reason(
        state,
        trace_id,
        decision,
        spec_metadata.decision_kind,
        Some(input.requested_model.as_str()),
        "candidate_evaluation_incomplete",
    );
    let effective_body_json = input.effective_body_json(body_json).clone();
    let stream_failover_attempt_budget_enabled =
        !openai_request_is_image_generation_intent(&input.requested_model, &effective_body_json);
    let (candidates, candidate_count) = build_local_openai_responses_candidate_attempt_source(
        state,
        trace_id,
        &input,
        &effective_body_json,
        spec,
    )
    .await?;
    apply_local_runtime_candidate_evaluation_progress(state, trace_id, candidate_count);
    if candidate_count == 0 {
        return Ok(None);
    }

    Ok(Some((
        LocalOpenAiResponsesStreamAttemptSource {
            state,
            parts,
            trace_id,
            body_json: effective_body_json,
            input,
            spec,
            candidates,
            stream_failover_attempt_budget_enabled,
            stream_failover_attempt_budget: StreamFailoverAttemptBudget::default(),
            global_policy: read_global_stream_failover_policy(state).await,
        },
        candidate_count,
    )))
}

#[async_trait]
impl LocalExecutionAttemptSource<AiSyncAttempt> for LocalOpenAiResponsesSyncAttemptSource<'_> {
    async fn next_execution_attempt(&mut self) -> Result<Option<AiSyncAttempt>, GatewayError> {
        while let Some(attempt) = self.candidates.next_attempt().await {
            match self.build_sync_attempt(attempt).await? {
                Some(attempt) => return Ok(Some(attempt)),
                None => continue,
            }
        }
        apply_local_runtime_candidate_terminal_reason(
            self.state,
            self.trace_id,
            "no_local_sync_plans",
        );
        Ok(None)
    }

    async fn drain_execution_attempts(&mut self) -> Result<Vec<AiSyncAttempt>, GatewayError> {
        let mut drained = Vec::new();
        for attempt in self.candidates.drain_static_attempts() {
            if let Some(attempt) = self.build_sync_attempt(attempt).await? {
                drained.push(attempt);
            }
        }
        Ok(drained)
    }
}

#[async_trait]
impl LocalExecutionAttemptSource<AiStreamAttempt> for LocalOpenAiResponsesStreamAttemptSource<'_> {
    async fn next_execution_attempt(&mut self) -> Result<Option<AiStreamAttempt>, GatewayError> {
        while let Some(attempt) = self.candidates.next_attempt().await {
            if self.stream_failover_attempt_budget_enabled {
                let admission = self
                    .stream_failover_attempt_budget
                    .admit_attempt(&attempt, &self.global_policy);
                if admission != StreamFailoverAttemptAdmission::Allowed {
                    if let Some(lease) = attempt.eligible.orchestration.pool_key_lease.as_ref() {
                        crate::handlers::shared::provider_pool::release_admin_provider_pool_key_lease(
                            self.state.runtime_state.as_ref(), lease,
                        ).await.map_err(|err| GatewayError::Internal(format!("failed to release unused pool key lease: {err:?}")))?;
                    }
                    if admission == StreamFailoverAttemptAdmission::BudgetExhausted {
                        break;
                    }
                    continue;
                }
            }
            match self.build_stream_attempt(attempt).await? {
                Some(attempt) => return Ok(Some(attempt)),
                None => continue,
            }
        }
        apply_local_runtime_candidate_terminal_reason(
            self.state,
            self.trace_id,
            "no_local_stream_plans",
        );
        Ok(None)
    }

    async fn drain_execution_attempts(&mut self) -> Result<Vec<AiStreamAttempt>, GatewayError> {
        let mut drained = Vec::new();
        for attempt in self.candidates.drain_static_attempts() {
            if let Some(attempt) = self.build_stream_attempt(attempt).await? {
                drained.push(attempt);
            }
        }
        Ok(drained)
    }
}

impl LocalOpenAiResponsesSyncAttemptSource<'_> {
    async fn build_sync_attempt(
        &self,
        attempt: LocalOpenAiResponsesCandidateAttempt,
    ) -> Result<Option<AiSyncAttempt>, GatewayError> {
        let Some(payload) = maybe_build_local_openai_responses_decision_payload_for_candidate(
            self.state,
            self.parts,
            self.trace_id,
            &self.body_json,
            &self.input,
            attempt,
            self.spec,
        )
        .await?
        else {
            return Ok(None);
        };

        match build_openai_responses_sync_plan_from_decision(
            self.parts,
            &self.body_json,
            payload,
            self.spec.compact,
        ) {
            Ok(value) => Ok(value),
            Err(err) => {
                warn!(
                    trace_id = %self.trace_id,
                    error = ?err,
                    "gateway local openai responses sync decision plan build failed"
                );
                Ok(None)
            }
        }
    }
}

impl LocalOpenAiResponsesStreamAttemptSource<'_> {
    async fn build_stream_attempt(
        &self,
        attempt: LocalOpenAiResponsesCandidateAttempt,
    ) -> Result<Option<AiStreamAttempt>, GatewayError> {
        let Some(payload) = maybe_build_local_openai_responses_decision_payload_for_candidate(
            self.state,
            self.parts,
            self.trace_id,
            &self.body_json,
            &self.input,
            attempt,
            self.spec,
        )
        .await?
        else {
            return Ok(None);
        };

        match build_openai_responses_stream_plan_from_decision(
            self.parts,
            &self.body_json,
            payload,
            self.spec.compact,
        ) {
            Ok(mut value) => {
                if let Some(attempt) = value.as_mut() {
                    let context = attempt
                        .report_context
                        .get_or_insert_with(|| serde_json::json!({}));
                    context["global_stream_failover_policy"] =
                        local_stream_failover_policy_to_value(&self.global_policy);
                }
                Ok(value)
            }
            Err(err) => {
                warn!(
                    trace_id = %self.trace_id,
                    error = ?err,
                    "gateway local openai responses stream decision plan build failed"
                );
                Ok(None)
            }
        }
    }
}

pub(super) async fn build_local_sync_plan_and_reports(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    spec: LocalOpenAiResponsesSpec,
) -> Result<Vec<AiSyncAttempt>, GatewayError> {
    let spec_metadata = local_openai_responses_spec_metadata(spec);
    let Some(input) = resolve_local_openai_responses_decision_input(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        spec_metadata.decision_kind,
    )
    .await?
    else {
        return Ok(Vec::new());
    };
    set_local_runtime_miss_diagnostic_reason(
        state,
        trace_id,
        decision,
        spec_metadata.decision_kind,
        Some(input.requested_model.as_str()),
        "candidate_evaluation_incomplete",
    );

    let (mut source, candidate_count) = build_local_openai_responses_candidate_attempt_source(
        state, trace_id, &input, body_json, spec,
    )
    .await?;
    apply_local_runtime_candidate_evaluation_progress(state, trace_id, candidate_count);
    if candidate_count == 0 {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    while let Some(attempt) = source.next_attempt().await {
        let Some(payload) = maybe_build_local_openai_responses_decision_payload_for_candidate(
            state, parts, trace_id, body_json, &input, attempt, spec,
        )
        .await?
        else {
            continue;
        };

        match build_openai_responses_sync_plan_from_decision(
            parts,
            body_json,
            payload,
            spec.compact,
        ) {
            Ok(Some(value)) => plans.push(value),
            Ok(None) => {}
            Err(err) => {
                warn!(
                    trace_id = %trace_id,
                    api_format = spec_metadata.api_format,
                    error = ?err,
                    "gateway local openai responses sync decision plan build failed"
                );
            }
        }
    }

    apply_local_runtime_candidate_terminal_reason(state, trace_id, "no_local_sync_plans");
    Ok(plans)
}

pub(super) async fn build_local_stream_plan_and_reports(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    spec: LocalOpenAiResponsesSpec,
) -> Result<Vec<AiStreamAttempt>, GatewayError> {
    let spec_metadata = local_openai_responses_spec_metadata(spec);
    let Some(input) = resolve_local_openai_responses_decision_input(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        spec_metadata.decision_kind,
    )
    .await?
    else {
        return Ok(Vec::new());
    };
    set_local_runtime_miss_diagnostic_reason(
        state,
        trace_id,
        decision,
        spec_metadata.decision_kind,
        Some(input.requested_model.as_str()),
        "candidate_evaluation_incomplete",
    );

    let (mut source, candidate_count) = build_local_openai_responses_candidate_attempt_source(
        state, trace_id, &input, body_json, spec,
    )
    .await?;
    apply_local_runtime_candidate_evaluation_progress(state, trace_id, candidate_count);
    if candidate_count == 0 {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    while let Some(attempt) = source.next_attempt().await {
        let Some(payload) = maybe_build_local_openai_responses_decision_payload_for_candidate(
            state, parts, trace_id, body_json, &input, attempt, spec,
        )
        .await?
        else {
            continue;
        };

        match build_openai_responses_stream_plan_from_decision(
            parts,
            body_json,
            payload,
            spec.compact,
        ) {
            Ok(Some(value)) => plans.push(value),
            Ok(None) => {}
            Err(err) => {
                warn!(
                    trace_id = %trace_id,
                    api_format = spec_metadata.api_format,
                    error = ?err,
                    "gateway local openai responses stream decision plan build failed"
                );
            }
        }
    }

    apply_local_runtime_candidate_terminal_reason(state, trace_id, "no_local_stream_plans");
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::{StreamFailoverAttemptAdmission, StreamFailoverAttemptBudget};

    #[test]
    fn stream_failover_budget_spans_providers_and_endpoints() {
        let mut budget = StreamFailoverAttemptBudget::default();
        assert_eq!(
            budget.admit("p1", "e1", "k1", 1),
            StreamFailoverAttemptAdmission::Allowed
        );
        assert_eq!(
            budget.admit("p1", "e2", "k1", 1),
            StreamFailoverAttemptAdmission::DuplicateAccount
        );
        assert_eq!(
            budget.admit("p2", "e2", "k2", 1),
            StreamFailoverAttemptAdmission::Allowed
        );
        assert_eq!(
            budget.admit("p3", "e3", "k3", 1),
            StreamFailoverAttemptAdmission::BudgetExhausted
        );
    }

    #[test]
    fn stream_failover_budget_rejects_duplicate_accounts() {
        let mut budget = StreamFailoverAttemptBudget::default();

        assert_eq!(
            budget.admit("provider", "endpoint", "key-a", 2),
            StreamFailoverAttemptAdmission::Allowed
        );
        assert_eq!(
            budget.admit("provider", "endpoint", "key-a", 2),
            StreamFailoverAttemptAdmission::DuplicateAccount
        );
        assert_eq!(
            budget.admit("provider", "endpoint", "key-b", 2),
            StreamFailoverAttemptAdmission::Allowed
        );
    }

    #[test]
    fn stream_failover_zero_switches_allows_only_first_account() {
        let mut budget = StreamFailoverAttemptBudget::default();

        assert_eq!(
            budget.admit("provider", "endpoint", "key-a", 0),
            StreamFailoverAttemptAdmission::Allowed
        );
        assert_eq!(
            budget.admit("provider", "endpoint", "key-b", 0),
            StreamFailoverAttemptAdmission::BudgetExhausted
        );
    }

    #[test]
    fn stream_failover_two_switches_allows_three_distinct_accounts() {
        let mut budget = StreamFailoverAttemptBudget::default();

        for key_id in ["key-a", "key-b", "key-c"] {
            assert_eq!(
                budget.admit("provider", "endpoint", key_id, 2),
                StreamFailoverAttemptAdmission::Allowed
            );
        }
        assert_eq!(
            budget.admit("provider", "endpoint", "key-d", 2),
            StreamFailoverAttemptAdmission::BudgetExhausted
        );
    }
}

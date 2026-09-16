use async_trait::async_trait;
use tracing::warn;

use super::super::super::openai_request_is_image_generation_intent;
use super::super::{
    build_lazy_local_openai_chat_candidate_attempt_source,
    build_local_openai_chat_image_candidate_attempt_source,
    maybe_build_local_openai_chat_decision_payload_for_candidate, AppState, GatewayControlDecision,
    GatewayError, LocalOpenAiChatCandidateAttempt, LocalOpenAiChatCandidateAttemptSource,
    LocalOpenAiChatDecisionInput,
};
use super::diagnostic::{
    set_local_openai_chat_candidate_evaluation_diagnostic, set_local_openai_chat_miss_diagnostic,
};
use super::openai_chat_upstream_is_stream_for_candidate;
use super::resolve::resolve_local_openai_chat_decision_input;
use crate::ai_serving::planner::candidate_materialization::LocalExecutionAttemptSource;
use crate::ai_serving::planner::common::OPENAI_CHAT_STREAM_PLAN_KIND;
use crate::ai_serving::planner::plan_builders::{
    build_openai_chat_stream_plan_from_decision, AiStreamAttempt,
};
use crate::ai_serving::planner::runtime_miss::apply_local_runtime_candidate_terminal_reason;
use crate::orchestration::{
    read_global_stream_failover_policy, LocalStreamFailoverPolicy, StreamFailoverAttemptAdmission,
    StreamFailoverAttemptBudget,
};

pub(crate) struct LocalOpenAiChatStreamAttemptSource<'a> {
    state: &'a AppState,
    parts: &'a http::request::Parts,
    trace_id: &'a str,
    body_json: serde_json::Value,
    input: LocalOpenAiChatDecisionInput,
    candidates: LocalOpenAiChatCandidateAttemptSource<'a>,
    stream_failover_attempt_budget: StreamFailoverAttemptBudget,
    global_policy: LocalStreamFailoverPolicy,
    stream_failover_attempt_budget_enabled: bool,
}

pub(crate) async fn build_local_openai_chat_stream_attempt_source<'a>(
    state: &'a AppState,
    parts: &'a http::request::Parts,
    trace_id: &'a str,
    decision: &'a GatewayControlDecision,
    body_json: &'a serde_json::Value,
    plan_kind: &str,
) -> Result<Option<(LocalOpenAiChatStreamAttemptSource<'a>, usize)>, GatewayError> {
    if plan_kind != OPENAI_CHAT_STREAM_PLAN_KIND {
        return Ok(None);
    }

    let Some(input) = resolve_local_openai_chat_decision_input(
        state, parts, trace_id, decision, body_json, plan_kind, true,
    )
    .await?
    else {
        return Ok(None);
    };
    let effective_body_json = input.effective_body_json(body_json).clone();

    let image_generation_intent =
        openai_request_is_image_generation_intent(&input.requested_model, body_json);
    let (mut candidates, mut candidate_count) = if image_generation_intent {
        let (image_candidates, image_candidate_count) =
            build_local_openai_chat_image_candidate_attempt_source(
                state,
                trace_id,
                &input,
                &effective_body_json,
            )
            .await?;
        if image_candidate_count > 0 {
            (image_candidates, image_candidate_count)
        } else {
            build_lazy_local_openai_chat_candidate_attempt_source(
                state,
                trace_id,
                &input,
                &effective_body_json,
                true,
            )
            .await
        }
    } else {
        build_lazy_local_openai_chat_candidate_attempt_source(
            state,
            trace_id,
            &input,
            &effective_body_json,
            true,
        )
        .await
    };
    if !image_generation_intent && candidate_count == 0 {
        let (image_candidates, image_candidate_count) =
            build_local_openai_chat_image_candidate_attempt_source(
                state,
                trace_id,
                &input,
                &effective_body_json,
            )
            .await?;
        if image_candidate_count > 0 {
            candidates = image_candidates;
            candidate_count = image_candidate_count;
        }
    }
    if candidate_count == 0 {
        set_local_openai_chat_candidate_evaluation_diagnostic(
            state,
            trace_id,
            decision,
            plan_kind,
            Some(input.requested_model.as_str()),
            0,
        );
        return Ok(None);
    }
    set_local_openai_chat_candidate_evaluation_diagnostic(
        state,
        trace_id,
        decision,
        plan_kind,
        Some(input.requested_model.as_str()),
        candidate_count,
    );

    Ok(Some((
        LocalOpenAiChatStreamAttemptSource {
            state,
            parts,
            trace_id,
            body_json: effective_body_json,
            input,
            candidates,
            stream_failover_attempt_budget: StreamFailoverAttemptBudget::default(),
            global_policy: read_global_stream_failover_policy(state).await,
            stream_failover_attempt_budget_enabled: !image_generation_intent,
        },
        candidate_count,
    )))
}

#[async_trait]
impl LocalExecutionAttemptSource<AiStreamAttempt> for LocalOpenAiChatStreamAttemptSource<'_> {
    async fn next_execution_attempt(&mut self) -> Result<Option<AiStreamAttempt>, GatewayError> {
        while let Some(attempt) = self.candidates.next_attempt().await {
            if self.stream_failover_attempt_budget_enabled
                && crate::ai_serving::is_openai_responses_format(
                    &attempt.eligible.provider_api_format,
                )
            {
                let candidate = &attempt.eligible.candidate;
                let admission = self.stream_failover_attempt_budget.admit_with_policy(
                    &candidate.provider_id,
                    &candidate.endpoint_id,
                    &candidate.key_id,
                    &self.global_policy,
                );
                if admission != StreamFailoverAttemptAdmission::Allowed {
                    if let Some(lease) = attempt.eligible.orchestration.pool_key_lease.as_ref() {
                        crate::handlers::shared::provider_pool::release_admin_provider_pool_key_lease(
                            self.state.runtime_state.as_ref(), lease,
                        )
                        .await
                        .map_err(|err| GatewayError::Internal(format!("failed to release unused pool key lease: {err:?}")))?;
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

impl LocalOpenAiChatStreamAttemptSource<'_> {
    async fn build_stream_attempt(
        &self,
        attempt: LocalOpenAiChatCandidateAttempt,
    ) -> Result<Option<AiStreamAttempt>, GatewayError> {
        let upstream_is_stream = openai_chat_upstream_is_stream_for_candidate(
            &attempt.eligible.transport,
            attempt.eligible.provider_api_format.as_str(),
            true,
        );
        let Some(payload) = maybe_build_local_openai_chat_decision_payload_for_candidate(
            self.state,
            self.parts,
            self.trace_id,
            &self.body_json,
            &self.input,
            attempt,
            OPENAI_CHAT_STREAM_PLAN_KIND,
            "openai_chat_stream_success",
            upstream_is_stream,
        )
        .await?
        else {
            return Ok(None);
        };

        match build_openai_chat_stream_plan_from_decision(self.parts, &self.body_json, payload) {
            Ok(value) => Ok(value),
            Err(err) => {
                warn!(
                    trace_id = %self.trace_id,
                    error = ?err,
                    "gateway local openai chat stream decision plan build failed"
                );
                Ok(None)
            }
        }
    }
}

pub(crate) async fn build_local_openai_chat_stream_plan_and_reports(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<Vec<AiStreamAttempt>, GatewayError> {
    if plan_kind != OPENAI_CHAT_STREAM_PLAN_KIND {
        return Ok(Vec::new());
    }

    let Some(input) = resolve_local_openai_chat_decision_input(
        state, parts, trace_id, decision, body_json, plan_kind, true,
    )
    .await?
    else {
        return Ok(Vec::new());
    };

    let Some((mut attempt_source, candidate_count)) =
        build_local_openai_chat_stream_attempt_source(
            state, parts, trace_id, decision, body_json, plan_kind,
        )
        .await?
    else {
        set_local_openai_chat_candidate_evaluation_diagnostic(
            state,
            trace_id,
            decision,
            plan_kind,
            Some(input.requested_model.as_str()),
            0,
        );
        return Ok(Vec::new());
    };

    let mut plans = Vec::new();
    while let Some(attempt) = attempt_source.next_execution_attempt().await? {
        plans.push(attempt);
        if plans.len() >= candidate_count {
            break;
        }
    }

    apply_local_runtime_candidate_terminal_reason(state, trace_id, "no_local_stream_plans");

    Ok(plans)
}

use std::collections::{BTreeMap, VecDeque};
use std::io::Error as IoError;
use std::time::Instant;

use axum::body::{to_bytes, Body, Bytes};
use axum::http::header::{CACHE_CONTROL, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue, Response, StatusCode};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::ai_serving::api::{
    build_local_gemini_files_stream_attempt_source_for_kind,
    build_local_gemini_files_sync_attempt_source_for_kind,
    build_local_image_stream_attempt_source_for_kind,
    build_local_image_sync_attempt_source_for_kind,
    build_local_openai_chat_stream_attempt_source_for_kind,
    build_local_openai_chat_stream_plan_and_reports_for_kind,
    build_local_openai_chat_sync_attempt_source_for_kind,
    build_local_openai_chat_sync_plan_and_reports_for_kind,
    build_local_openai_responses_stream_attempt_source_for_kind,
    build_local_openai_responses_stream_plan_and_reports_for_kind,
    build_local_openai_responses_sync_attempt_source_for_kind,
    build_local_openai_responses_sync_plan_and_reports_for_kind,
    build_local_same_format_stream_attempt_source, build_local_same_format_stream_plan_and_reports,
    build_local_same_format_sync_attempt_source, build_local_same_format_sync_plan_and_reports,
    build_local_video_sync_attempt_source_for_kind, build_standard_family_stream_attempt_source,
    build_standard_family_sync_attempt_source, parse_direct_request_body,
    resolve_claude_stream_spec, resolve_claude_sync_spec, resolve_gemini_stream_spec,
    resolve_gemini_sync_spec, resolve_local_same_format_stream_spec,
    resolve_local_same_format_sync_spec, set_local_openai_chat_execution_exhausted_diagnostic,
    set_local_openai_image_execution_exhausted_diagnostic, AiStreamAttempt, AiSyncAttempt,
    LocalStandardSpec, EXECUTION_RUNTIME_STREAM_DECISION_ACTION,
    EXECUTION_RUNTIME_SYNC_DECISION_ACTION,
};
use crate::ai_serving::LocalExecutionAttemptSource;
use crate::api::response::{
    attach_control_metadata_headers, build_client_response_from_parts_with_mutator,
};
use crate::constants::EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS;
use crate::control::GatewayControlDecision;
use crate::execution_runtime::sync::{
    build_openai_image_sync_json_whitespace_heartbeat_stream, execute_execution_runtime_sync,
};
use crate::executor::candidate_loop::{
    execute_stream_attempt_source, execute_sync_attempt_source, execute_sync_plan_and_reports,
    mark_unused_local_candidates, release_duplicate_attempt_lease, RequestAttemptRegistry,
};
use crate::executor::{
    build_local_execution_exhaustion, record_failed_usage_for_exhausted_request,
    LocalExecutionRequestOutcome,
};
use crate::handlers::shared::system_config_bool;
use crate::{AiExecutionDecision, AppState, GatewayError};

const ENABLE_OPENAI_IMAGE_SYNC_HEARTBEAT_CONFIG_KEY: &str = "enable_openai_image_sync_heartbeat";
const OPENAI_IMAGE_SYNC_HEARTBEAT_INTERNAL_ERROR_STATUS: u16 = 502;
const OPENAI_IMAGE_SYNC_HEARTBEAT_EXHAUSTED_STATUS: u16 = 503;
const OPENAI_IMAGE_SYNC_HEARTBEAT_ERROR_MESSAGE_LIMIT: usize = 4096;

pub(crate) async fn maybe_execute_sync_local_path(
    state: &AppState,
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
    trace_id: &str,
    decision: &GatewayControlDecision,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    super::maybe_execute_via_sync_decision_path(state, parts, body_bytes, trace_id, decision).await
}

pub(crate) async fn maybe_execute_stream_local_path(
    state: &AppState,
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
    trace_id: &str,
    decision: &GatewayControlDecision,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    super::maybe_execute_via_stream_decision_path(state, parts, body_bytes, trace_id, decision)
        .await
}

pub(crate) async fn maybe_execute_sync_via_local_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((attempt_source, candidate_count)) =
        build_local_openai_chat_sync_attempt_source_for_kind(
            state, parts, trace_id, decision, body_json, plan_kind,
        )
        .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    let outcome = execute_sync_attempt_source::<AiSyncAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await?;

    if let LocalExecutionRequestOutcome::Exhausted(_) = &outcome {
        set_local_openai_chat_execution_exhausted_diagnostic(
            state,
            trace_id,
            decision,
            plan_kind,
            body_json,
            candidate_count,
        );
    }

    Ok(outcome)
}

pub(crate) async fn maybe_execute_stream_via_local_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((attempt_source, candidate_count)) =
        build_local_openai_chat_stream_attempt_source_for_kind(
            state, parts, trace_id, decision, body_json, plan_kind,
        )
        .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    let outcome = execute_stream_attempt_source::<AiStreamAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await?;

    if let LocalExecutionRequestOutcome::Exhausted(_) = &outcome {
        set_local_openai_chat_execution_exhausted_diagnostic(
            state,
            trace_id,
            decision,
            plan_kind,
            body_json,
            candidate_count,
        );
    }

    Ok(outcome)
}

pub(crate) async fn maybe_execute_sync_via_local_openai_responses_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((attempt_source, _candidate_count)) =
        build_local_openai_responses_sync_attempt_source_for_kind(
            state, parts, trace_id, decision, body_json, plan_kind,
        )
        .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_sync_attempt_source::<AiSyncAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

pub(crate) async fn maybe_execute_stream_via_local_openai_responses_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((attempt_source, _candidate_count)) =
        build_local_openai_responses_stream_attempt_source_for_kind(
            state, parts, trace_id, decision, body_json, plan_kind,
        )
        .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_stream_attempt_source::<AiStreamAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

pub(crate) async fn maybe_execute_sync_via_standard_family_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
    resolve_sync_spec: fn(&str) -> Option<LocalStandardSpec>,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(spec) = resolve_sync_spec(plan_kind) else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    let Some((attempt_source, _candidate_count)) = build_standard_family_sync_attempt_source(
        state, parts, trace_id, decision, body_json, spec,
    )
    .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_sync_attempt_source::<AiSyncAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

pub(crate) async fn maybe_execute_stream_via_standard_family_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
    resolve_stream_spec: fn(&str) -> Option<LocalStandardSpec>,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(spec) = resolve_stream_spec(plan_kind) else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    let Some((attempt_source, _candidate_count)) = build_standard_family_stream_attempt_source(
        state, parts, trace_id, decision, body_json, spec,
    )
    .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_stream_attempt_source::<AiStreamAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

pub(crate) async fn maybe_execute_sync_via_local_standard_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let mut exhausted = None;

    match maybe_execute_sync_via_standard_family_decision(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        plan_kind,
        resolve_claude_sync_spec,
    )
    .await?
    {
        LocalExecutionRequestOutcome::Responded(response) => {
            return Ok(LocalExecutionRequestOutcome::Responded(response));
        }
        LocalExecutionRequestOutcome::Exhausted(outcome) => exhausted = Some(outcome),
        LocalExecutionRequestOutcome::NoPath => {}
    }

    match maybe_execute_sync_via_standard_family_decision(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        plan_kind,
        resolve_gemini_sync_spec,
    )
    .await?
    {
        LocalExecutionRequestOutcome::Responded(response) => {
            Ok(LocalExecutionRequestOutcome::Responded(response))
        }
        LocalExecutionRequestOutcome::Exhausted(outcome) => {
            Ok(LocalExecutionRequestOutcome::Exhausted(outcome))
        }
        LocalExecutionRequestOutcome::NoPath => Ok(exhausted
            .map(LocalExecutionRequestOutcome::Exhausted)
            .unwrap_or(LocalExecutionRequestOutcome::NoPath)),
    }
}

pub(crate) async fn maybe_execute_stream_via_local_standard_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let mut exhausted = None;

    match maybe_execute_stream_via_standard_family_decision(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        plan_kind,
        resolve_claude_stream_spec,
    )
    .await?
    {
        LocalExecutionRequestOutcome::Responded(response) => {
            return Ok(LocalExecutionRequestOutcome::Responded(response));
        }
        LocalExecutionRequestOutcome::Exhausted(outcome) => exhausted = Some(outcome),
        LocalExecutionRequestOutcome::NoPath => {}
    }

    match maybe_execute_stream_via_standard_family_decision(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        plan_kind,
        resolve_gemini_stream_spec,
    )
    .await?
    {
        LocalExecutionRequestOutcome::Responded(response) => {
            Ok(LocalExecutionRequestOutcome::Responded(response))
        }
        LocalExecutionRequestOutcome::Exhausted(outcome) => {
            Ok(LocalExecutionRequestOutcome::Exhausted(outcome))
        }
        LocalExecutionRequestOutcome::NoPath => Ok(exhausted
            .map(LocalExecutionRequestOutcome::Exhausted)
            .unwrap_or(LocalExecutionRequestOutcome::NoPath)),
    }
}

pub(crate) async fn maybe_execute_sync_via_local_same_format_provider_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(spec) = resolve_local_same_format_sync_spec(plan_kind) else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    let Some((attempt_source, _candidate_count)) = build_local_same_format_sync_attempt_source(
        state, parts, trace_id, decision, body_json, spec,
    )
    .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_sync_attempt_source::<AiSyncAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

pub(crate) async fn maybe_execute_stream_via_local_same_format_provider_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(spec) = resolve_local_same_format_stream_spec(plan_kind) else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    let Some((attempt_source, _candidate_count)) = build_local_same_format_stream_attempt_source(
        state, parts, trace_id, decision, body_json, spec,
    )
    .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_stream_attempt_source::<AiStreamAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

pub(crate) async fn maybe_execute_sync_via_local_gemini_files_decision(
    state: &AppState,
    parts: &http::request::Parts,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
    body_is_empty: bool,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((attempt_source, _candidate_count)) =
        build_local_gemini_files_sync_attempt_source_for_kind(
            state,
            parts,
            body_json,
            body_base64,
            body_is_empty,
            trace_id,
            decision,
            plan_kind,
        )
        .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_sync_attempt_source::<AiSyncAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

async fn openai_image_sync_heartbeat_enabled(state: &AppState) -> bool {
    match state
        .read_system_config_json_value(ENABLE_OPENAI_IMAGE_SYNC_HEARTBEAT_CONFIG_KEY)
        .await
    {
        Ok(value) => system_config_bool(value.as_ref(), true),
        Err(err) => {
            tracing::warn!(
                event_name = "openai_image_sync_heartbeat_config_read_failed",
                log_type = "ops",
                error = ?err,
                "gateway failed to read sync image heartbeat config; defaulting enabled"
            );
            true
        }
    }
}

fn build_openai_image_sync_heartbeat_shell_response(
    state: AppState,
    request_path: String,
    trace_id: String,
    decision: GatewayControlDecision,
    plan_kind: String,
    attempts: Vec<AiSyncAttempt>,
    attempt_registry: Option<RequestAttemptRegistry>,
) -> Result<Response<Body>, GatewayError> {
    let request_id = attempts
        .first()
        .map(|attempt| attempt.plan.request_id.clone())
        .filter(|value| !value.trim().is_empty());
    let trace_id_for_response = trace_id.clone();
    let decision_for_response = decision.clone();
    let started_at = Instant::now();
    let (tx, rx) = mpsc::channel::<Result<Bytes, IoError>>(1);

    tokio::spawn(async move {
        let bytes = openai_image_sync_heartbeat_final_bytes(
            execute_openai_image_sync_heartbeat_attempts(
                state,
                request_path,
                trace_id,
                decision,
                plan_kind,
                attempts,
                attempt_registry,
                started_at,
            )
            .await,
        )
        .await;
        let _ = tx.send(Ok(Bytes::from(bytes))).await;
    });

    let headers = BTreeMap::from([(
        CONTENT_TYPE.as_str().to_string(),
        "application/json".to_string(),
    )]);
    let response = build_client_response_from_parts_with_mutator(
        StatusCode::OK.as_u16(),
        &headers,
        Body::from_stream(build_openai_image_sync_json_whitespace_heartbeat_stream(rx)),
        trace_id_for_response.as_str(),
        Some(&decision_for_response),
        |headers| {
            headers.remove(CONTENT_LENGTH);
            headers.remove(CONTENT_ENCODING);
            headers.insert(
                CACHE_CONTROL,
                HeaderValue::from_static("no-cache, no-transform"),
            );
            headers.insert(
                HeaderName::from_static("x-accel-buffering"),
                HeaderValue::from_static("no"),
            );
            Ok(())
        },
    )?;
    attach_control_metadata_headers(response, request_id.as_deref(), None)
}

async fn execute_openai_image_sync_heartbeat_attempts(
    state: AppState,
    request_path: String,
    trace_id: String,
    decision: GatewayControlDecision,
    plan_kind: String,
    attempts: Vec<AiSyncAttempt>,
    attempt_registry: Option<RequestAttemptRegistry>,
    started_at: Instant,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let mut attempts = VecDeque::from(attempts);
    let mut last_attempted = None;

    while let Some(attempt) = attempts.pop_front() {
        let plan = attempt.plan;
        let report_kind = attempt.report_kind;
        let report_context = attempt.report_context;
        if attempt_registry
            .as_ref()
            .is_some_and(|registry| !registry.begin_attempt(&plan, report_context.as_ref()))
        {
            release_duplicate_attempt_lease(&state, report_context.as_ref()).await;
            continue;
        }
        last_attempted = Some((plan.clone(), report_context.clone()));
        match execute_execution_runtime_sync(
            &state,
            request_path.as_str(),
            plan,
            trace_id.as_str(),
            &decision,
            plan_kind.as_str(),
            report_kind,
            report_context,
        )
        .await?
        {
            Some(response) => {
                mark_unused_local_candidates(&state, attempts.into_iter().collect()).await;
                return Ok(LocalExecutionRequestOutcome::responded(response));
            }
            None => continue,
        }
    }

    let Some((last_plan, last_report_context)) = last_attempted else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };
    let exhaustion =
        build_local_execution_exhaustion(&state, &last_plan, last_report_context.as_ref()).await;
    record_failed_usage_for_exhausted_request(
        &state,
        exhaustion,
        &started_at,
        "OpenAI image sync heartbeat exhausted all local candidates",
        EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS,
        None,
    )
    .await;
    Ok(LocalExecutionRequestOutcome::NoPath)
}

async fn openai_image_sync_heartbeat_final_bytes(
    result: Result<LocalExecutionRequestOutcome, GatewayError>,
) -> Vec<u8> {
    match result {
        Ok(LocalExecutionRequestOutcome::Responded(response)) => {
            openai_image_sync_heartbeat_response_body_bytes(response).await
        }
        Ok(LocalExecutionRequestOutcome::Exhausted(_))
        | Ok(LocalExecutionRequestOutcome::NoPath) => openai_image_sync_heartbeat_error_body(
            OPENAI_IMAGE_SYNC_HEARTBEAT_EXHAUSTED_STATUS,
            "OpenAI image sync exhausted all local candidates",
        ),
        Err(err) => openai_image_sync_heartbeat_error_body(
            OPENAI_IMAGE_SYNC_HEARTBEAT_INTERNAL_ERROR_STATUS,
            &format!("{err:?}"),
        ),
    }
}

async fn openai_image_sync_heartbeat_response_body_bytes(response: Response<Body>) -> Vec<u8> {
    let status_code = response.status().as_u16();
    match to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) if status_code < 400 && !bytes.is_empty() => bytes.to_vec(),
        Ok(bytes) if status_code >= 400 => {
            openai_image_sync_heartbeat_error_body_from_response(status_code, bytes.as_ref())
        }
        Ok(_) => openai_image_sync_heartbeat_error_body(
            OPENAI_IMAGE_SYNC_HEARTBEAT_INTERNAL_ERROR_STATUS,
            "empty sync image response",
        ),
        Err(err) => openai_image_sync_heartbeat_error_body(
            OPENAI_IMAGE_SYNC_HEARTBEAT_INTERNAL_ERROR_STATUS,
            &err.to_string(),
        ),
    }
}

fn openai_image_sync_heartbeat_error_body_from_response(status_code: u16, body: &[u8]) -> Vec<u8> {
    if let Ok(mut value) = serde_json::from_slice::<Value>(body) {
        if let Some(error) = value.get_mut("error").and_then(Value::as_object_mut) {
            error.insert("upstream_status".to_string(), Value::from(status_code));
            error
                .entry("type".to_string())
                .or_insert_with(|| Value::String("upstream_error".to_string()));
            error.entry("message".to_string()).or_insert_with(|| {
                Value::String(format!("upstream returned status {status_code}"))
            });
            return serde_json::to_vec(&value).unwrap_or_else(|_| {
                openai_image_sync_heartbeat_error_body(
                    status_code,
                    &format!("upstream returned status {status_code}"),
                )
            });
        }
    }

    let message = openai_image_sync_heartbeat_error_message_from_body(status_code, body);
    openai_image_sync_heartbeat_error_body(status_code, message.as_str())
}

fn openai_image_sync_heartbeat_error_message_from_body(status_code: u16, body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body).trim().to_string();
    if text.is_empty() {
        return format!("upstream returned status {status_code}");
    }
    text.chars()
        .take(OPENAI_IMAGE_SYNC_HEARTBEAT_ERROR_MESSAGE_LIMIT)
        .collect()
}

fn openai_image_sync_heartbeat_error_body(status_code: u16, message: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "error": {
            "type": "upstream_error",
            "message": message,
            "code": status_code,
            "upstream_status": status_code,
        }
    }))
    .unwrap_or_else(|_| {
        format!(
            "{{\"error\":{{\"type\":\"upstream_error\",\"code\":{status_code},\"upstream_status\":{status_code}}}}}"
        )
        .into_bytes()
    })
}

pub(crate) async fn maybe_execute_sync_via_local_image_decision(
    state: &AppState,
    parts: &http::request::Parts,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((mut attempt_source, candidate_count)) =
        build_local_image_sync_attempt_source_for_kind(
            state,
            parts,
            body_json,
            body_base64,
            trace_id,
            decision,
            plan_kind,
        )
        .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    if openai_image_sync_heartbeat_enabled(state).await {
        let mut attempts = Vec::new();
        while let Some(attempt) = attempt_source.next_execution_attempt().await? {
            attempts.push(attempt);
        }
        return Ok(LocalExecutionRequestOutcome::responded(
            build_openai_image_sync_heartbeat_shell_response(
                state.clone(),
                parts.uri.path().to_string(),
                trace_id.to_string(),
                decision.clone(),
                plan_kind.to_string(),
                attempts,
                parts.extensions.get::<RequestAttemptRegistry>().cloned(),
            )?,
        ));
    }

    let outcome = execute_sync_attempt_source::<AiSyncAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await?;

    if let LocalExecutionRequestOutcome::Exhausted(_) = &outcome {
        set_local_openai_image_execution_exhausted_diagnostic(
            state,
            trace_id,
            decision,
            plan_kind,
            body_json,
            candidate_count,
        );
    }

    Ok(outcome)
}

pub(crate) async fn maybe_execute_stream_via_local_gemini_files_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((attempt_source, _candidate_count)) =
        build_local_gemini_files_stream_attempt_source_for_kind(
            state, parts, trace_id, decision, plan_kind,
        )
        .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_stream_attempt_source::<AiStreamAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

pub(crate) async fn maybe_execute_stream_via_local_image_decision(
    state: &AppState,
    parts: &http::request::Parts,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((attempt_source, candidate_count)) = build_local_image_stream_attempt_source_for_kind(
        state,
        parts,
        body_json,
        body_base64,
        trace_id,
        decision,
        plan_kind,
    )
    .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    let outcome = execute_stream_attempt_source::<AiStreamAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await?;

    if let LocalExecutionRequestOutcome::Exhausted(_) = &outcome {
        set_local_openai_image_execution_exhausted_diagnostic(
            state,
            trace_id,
            decision,
            plan_kind,
            body_json,
            candidate_count,
        );
    }

    Ok(outcome)
}

pub(crate) async fn maybe_execute_sync_via_local_video_decision(
    state: &AppState,
    parts: &http::request::Parts,
    body_json: &serde_json::Value,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some((attempt_source, _candidate_count)) = build_local_video_sync_attempt_source_for_kind(
        state, parts, body_json, trace_id, decision, plan_kind,
    )
    .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_sync_attempt_source::<AiSyncAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
    )
    .await
}

pub(crate) async fn maybe_execute_sync_request(
    state: &AppState,
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
    trace_id: &str,
    decision: Option<&GatewayControlDecision>,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(decision) = decision else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };
    #[cfg(not(test))]
    {
        if parts.method != http::Method::POST {
            return Ok(LocalExecutionRequestOutcome::NoPath);
        }
        return maybe_execute_sync_local_path(state, parts, body_bytes, trace_id, decision).await;
    }
    #[cfg(test)]
    {
        if state
            .execution_runtime_override_base_url()
            .unwrap_or_default()
            .is_empty()
            && parts.method != http::Method::POST
        {
            return Ok(LocalExecutionRequestOutcome::NoPath);
        }
        maybe_execute_sync_local_path(state, parts, body_bytes, trace_id, decision).await
    }
}

pub(crate) async fn maybe_execute_stream_request(
    state: &AppState,
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
    trace_id: &str,
    decision: Option<&GatewayControlDecision>,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(decision) = decision else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };
    #[cfg(not(test))]
    {
        if parts.method != http::Method::POST {
            return Ok(LocalExecutionRequestOutcome::NoPath);
        }
        return maybe_execute_stream_local_path(state, parts, body_bytes, trace_id, decision).await;
    }
    #[cfg(test)]
    {
        if state
            .execution_runtime_override_base_url()
            .unwrap_or_default()
            .is_empty()
            && parts.method != http::Method::POST
        {
            return Ok(LocalExecutionRequestOutcome::NoPath);
        }
        maybe_execute_stream_local_path(state, parts, body_bytes, trace_id, decision).await
    }
}

pub(crate) fn planner_decision_action(action: &str) -> bool {
    matches!(
        action,
        EXECUTION_RUNTIME_SYNC_DECISION_ACTION | EXECUTION_RUNTIME_STREAM_DECISION_ACTION
    )
}

pub(crate) fn parse_local_request_body(
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
) -> Option<(serde_json::Value, Option<String>)> {
    parse_direct_request_body(parts, body_bytes)
}

pub(crate) fn decision_payload_is_direct_execution(payload: &AiExecutionDecision) -> bool {
    planner_decision_action(payload.action.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const TEST_OPENAI_IMAGE_SYNC_PLAN_KIND: &str = "openai_image_sync";

    fn test_openai_image_heartbeat_decision() -> GatewayControlDecision {
        GatewayControlDecision::synthetic(
            "/v1/images/generations",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("image".to_string()),
            Some("openai:image".to_string()),
        )
        .with_execution_runtime_candidate(true)
    }

    fn test_openai_image_heartbeat_plan(
        endpoint_id: &str,
        candidate_id: &str,
    ) -> aether_contracts::ExecutionPlan {
        aether_contracts::ExecutionPlan {
            request_id: "trace-image-heartbeat-retry".to_string(),
            candidate_id: Some(candidate_id.to_string()),
            provider_name: Some("OpenAI".to_string()),
            provider_id: "provider-openai".to_string(),
            endpoint_id: endpoint_id.to_string(),
            key_id: "key-openai".to_string(),
            method: "POST".to_string(),
            url: "https://api.openai.com/v1/images/generations".to_string(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".to_string()),
            content_encoding: None,
            body: aether_contracts::RequestBody::from_json(json!({"prompt": "test"})),
            stream: false,
            client_api_format: "openai:image".to_string(),
            provider_api_format: "openai:image".to_string(),
            model_name: Some("gpt-image-1".to_string()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        }
    }

    fn test_openai_image_heartbeat_attempt(
        candidate_index: u32,
        endpoint_id: &str,
        candidate_id: &str,
    ) -> AiSyncAttempt {
        AiSyncAttempt {
            plan: test_openai_image_heartbeat_plan(endpoint_id, candidate_id),
            report_kind: None,
            report_context: Some(json!({
                "candidate_index": candidate_index,
                "retry_index": 0,
            })),
        }
    }

    fn test_openai_image_execution_result(
        plan: &aether_contracts::ExecutionPlan,
        status_code: u16,
        body_json: Value,
    ) -> aether_contracts::ExecutionResult {
        aether_contracts::ExecutionResult {
            request_id: plan.request_id.clone(),
            candidate_id: plan.candidate_id.clone(),
            status_code,
            headers: BTreeMap::from([(
                CONTENT_TYPE.as_str().to_string(),
                "application/json".to_string(),
            )]),
            body: Some(aether_contracts::ResponseBody {
                json_body: Some(body_json),
                body_bytes_b64: None,
            }),
            telemetry: Some(aether_contracts::ExecutionTelemetry {
                ttfb_ms: None,
                elapsed_ms: Some(10),
                upstream_bytes: None,
            }),
            error: None,
        }
    }

    #[tokio::test]
    async fn openai_image_sync_heartbeat_success_body_is_unchanged() {
        let response = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(r#"{"data":[{"b64_json":"x"}]}"#))
            .expect("response should build");

        let bytes = openai_image_sync_heartbeat_response_body_bytes(response).await;
        let body: Value = serde_json::from_slice(&bytes).expect("body should decode");

        assert_eq!(body, json!({"data": [{"b64_json": "x"}]}));
    }

    #[tokio::test]
    async fn openai_image_sync_heartbeat_missing_config_defaults_enabled() {
        let state = AppState::new().expect("state should build");

        assert!(openai_image_sync_heartbeat_enabled(&state).await);
    }

    #[tokio::test]
    async fn openai_image_sync_heartbeat_explicit_false_disables_it() {
        let state = AppState::new().expect("state should build");
        state
            .upsert_system_config_json_value(
                ENABLE_OPENAI_IMAGE_SYNC_HEARTBEAT_CONFIG_KEY,
                &json!(false),
                None,
            )
            .await
            .expect("heartbeat config should be saved");

        assert!(!openai_image_sync_heartbeat_enabled(&state).await);
    }

    #[tokio::test]
    async fn openai_image_sync_heartbeat_error_body_includes_upstream_status() {
        let response = Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .body(Body::from(
                r#"{"error":{"type":"rate_limit","message":"slow down"}}"#,
            ))
            .expect("response should build");

        let bytes = openai_image_sync_heartbeat_response_body_bytes(response).await;
        let body: Value = serde_json::from_slice(&bytes).expect("body should decode");

        assert_eq!(body["error"]["type"], json!("rate_limit"));
        assert_eq!(body["error"]["message"], json!("slow down"));
        assert_eq!(body["error"]["upstream_status"], json!(429));
    }

    #[test]
    fn openai_image_sync_heartbeat_non_json_error_body_is_wrapped() {
        let bytes =
            openai_image_sync_heartbeat_error_body_from_response(502, b"bad gateway from upstream");
        let body: Value = serde_json::from_slice(&bytes).expect("body should decode");

        assert_eq!(body["error"]["type"], json!("upstream_error"));
        assert_eq!(body["error"]["message"], json!("bad gateway from upstream"));
        assert_eq!(body["error"]["upstream_status"], json!(502));
    }

    #[tokio::test]
    async fn openai_image_sync_heartbeat_no_path_returns_json_error_body() {
        let bytes =
            openai_image_sync_heartbeat_final_bytes(Ok(LocalExecutionRequestOutcome::NoPath)).await;
        let body: Value = serde_json::from_slice(&bytes).expect("body should decode");

        assert_eq!(body["error"]["type"], json!("upstream_error"));
        assert_eq!(body["error"]["upstream_status"], json!(503));
    }

    #[tokio::test]
    async fn openai_image_sync_heartbeat_attempts_retry_first_candidate_then_return_second() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_override = Arc::clone(&call_count);
        let state = AppState::new()
            .expect("state should build")
            .with_execution_runtime_sync_override_for_tests(move |plan| {
                call_count_for_override.fetch_add(1, Ordering::SeqCst);
                if plan.endpoint_id == "endpoint-retry" {
                    Ok(test_openai_image_execution_result(
                        plan,
                        StatusCode::TOO_MANY_REQUESTS.as_u16(),
                        json!({"error": {"message": "retry this candidate"}}),
                    ))
                } else {
                    Ok(test_openai_image_execution_result(
                        plan,
                        StatusCode::OK.as_u16(),
                        json!({"data": [{"b64_json": "second-candidate"}]}),
                    ))
                }
            });
        let attempts = vec![
            test_openai_image_heartbeat_attempt(0, "endpoint-retry", "candidate-retry"),
            test_openai_image_heartbeat_attempt(1, "endpoint-success", "candidate-success"),
        ];

        let outcome = execute_openai_image_sync_heartbeat_attempts(
            state,
            "/v1/images/generations".to_string(),
            "trace-image-heartbeat-retry".to_string(),
            test_openai_image_heartbeat_decision(),
            TEST_OPENAI_IMAGE_SYNC_PLAN_KIND.to_string(),
            attempts,
            None,
            Instant::now(),
        )
        .await
        .expect("heartbeat attempts should execute");
        let LocalExecutionRequestOutcome::Responded(response) = outcome else {
            panic!("second candidate should return a response");
        };
        let bytes = openai_image_sync_heartbeat_response_body_bytes(response).await;
        let body: Value = serde_json::from_slice(&bytes).expect("body should decode");

        assert_eq!(call_count.load(Ordering::SeqCst), 2);
        assert_eq!(body, json!({"data": [{"b64_json": "second-candidate"}]}));
    }

    #[tokio::test]
    async fn openai_image_sync_heartbeat_skips_a_service_path_already_attempted() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_override = Arc::clone(&call_count);
        let state = AppState::new()
            .expect("state should build")
            .with_execution_runtime_sync_override_for_tests(move |plan| {
                call_count_for_override.fetch_add(1, Ordering::SeqCst);
                Ok(test_openai_image_execution_result(
                    plan,
                    StatusCode::TOO_MANY_REQUESTS.as_u16(),
                    json!({"error": {"message": "retry later"}}),
                ))
            });
        let attempts = vec![
            test_openai_image_heartbeat_attempt(0, "endpoint-duplicate", "candidate-first"),
            test_openai_image_heartbeat_attempt(1, "endpoint-duplicate", "candidate-duplicate"),
        ];

        let outcome = execute_openai_image_sync_heartbeat_attempts(
            state,
            "/v1/images/generations".to_string(),
            "trace-image-heartbeat-duplicate".to_string(),
            test_openai_image_heartbeat_decision(),
            TEST_OPENAI_IMAGE_SYNC_PLAN_KIND.to_string(),
            attempts,
            Some(RequestAttemptRegistry::default()),
            Instant::now(),
        )
        .await
        .expect("heartbeat attempts should execute");

        assert!(matches!(outcome, LocalExecutionRequestOutcome::NoPath));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }
}

use std::collections::BTreeMap;

use aether_contracts::ExecutionError;
use aether_data_contracts::repository::candidates::RequestCandidateStatus;
use aether_data_contracts::repository::niffler_core::{
    CreateNifflerBillingReservationDryRunRecord, CreateNifflerRouteAttemptRecord,
    CreateNifflerSettlementSnapshotRecord, FinalizeNifflerBillingReservationRecord,
    NifflerBillingReservationStatus,
};
use aether_scheduler_core::{
    execution_error_details, parse_request_candidate_report_context,
    SchedulerRequestCandidateStatusUpdate,
};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::clock::current_unix_ms;
use crate::log_ids::short_request_id;
use crate::niffler_runtime::{
    resolve_niffler_runtime_rollout_decision, NifflerRuntimeRolloutDecision,
};
use crate::orchestration::{apply_local_report_effect, LocalReportEffect};
use crate::request_candidate_runtime::record_report_request_candidate_status;
use crate::task_runtime::{spawn_fire_and_forget, TASK_KEY_USAGE_SYNC_REPORT};
use crate::{AppState, GatewayError};

mod context;
use context::{report_context_is_locally_actionable, resolve_locally_actionable_report_context};

use aether_usage_runtime::{
    is_local_ai_stream_report_kind, is_local_ai_sync_report_kind, report_request_id,
    should_handle_local_stream_report, should_handle_local_sync_report,
    sync_report_represents_failure,
};
pub(crate) use aether_usage_runtime::{GatewayStreamReportRequest, GatewaySyncReportRequest};

const TASK_KEY_NIFFLER_SHADOW_REPORT: &str = "usage.niffler.shadow";
const NIFFLER_BILLING_RESERVATION_DRY_RUN_MATCH_TOLERANCE_USD: f64 = 0.000001;

fn log_local_report_handled(
    trace_id: &str,
    report_kind: &str,
    report_scope: &'static str,
    report_context: Option<&serde_json::Value>,
) {
    debug!(
        event_name = "execution_report_handled_locally",
        log_type = "debug",
        debug_context = "redacted",
        trace_id = %trace_id,
        report_scope,
        report_kind = %report_kind,
        report_request_id = %short_request_id(report_request_id(report_context)),
        has_report_context = report_context.is_some(),
        "gateway handled execution report locally"
    );
}

fn log_local_report_effect_only(
    trace_id: &str,
    report_kind: &str,
    report_scope: &'static str,
    report_context: Option<&serde_json::Value>,
) {
    debug!(
        event_name = "execution_report_effect_handled_locally",
        log_type = "debug",
        debug_context = "redacted",
        trace_id = %trace_id,
        report_scope,
        report_kind = %report_kind,
        report_request_id = %short_request_id(report_request_id(report_context)),
        has_report_context = report_context.is_some(),
        "gateway handled execution report locally without actionable request-candidate context"
    );
}

fn log_dropped_report(
    trace_id: &str,
    report_kind: &str,
    report_scope: &'static str,
    report_context: Option<&serde_json::Value>,
) {
    warn!(
        event_name = "execution_report_dropped",
        log_type = "ops",
        status = "dropped",
        trace_id = %trace_id,
        report_scope,
        report_kind = %report_kind,
        report_request_id = %short_request_id(report_request_id(report_context)),
        has_report_context = report_context.is_some(),
        "gateway dropped execution report because local handling context was not actionable"
    );
}

pub(crate) async fn submit_sync_report(
    state: &AppState,
    mut payload: GatewaySyncReportRequest,
) -> Result<(), GatewayError> {
    let original_report_context = payload.report_context.take();
    if let Some(report_context) =
        resolve_locally_actionable_report_context(state, original_report_context.as_ref()).await
    {
        payload.report_context = Some(report_context);
        if should_handle_local_sync_report(
            payload.report_context.as_ref(),
            payload.report_kind.as_str(),
        ) {
            handle_local_sync_report(state, &payload).await;
            log_local_report_handled(
                payload.trace_id.as_str(),
                &payload.report_kind,
                "sync",
                payload.report_context.as_ref(),
            );
            return Ok(());
        }
    }
    payload.report_context = original_report_context;

    if should_handle_local_sync_report(
        payload.report_context.as_ref(),
        payload.report_kind.as_str(),
    ) {
        handle_local_sync_report(state, &payload).await;
        log_local_report_handled(
            payload.trace_id.as_str(),
            &payload.report_kind,
            "sync",
            payload.report_context.as_ref(),
        );
        return Ok(());
    }

    if payload.report_context.is_some()
        && is_local_ai_sync_report_kind(payload.report_kind.as_str())
    {
        handle_local_sync_report(state, &payload).await;
        log_local_report_effect_only(
            payload.trace_id.as_str(),
            &payload.report_kind,
            "sync",
            payload.report_context.as_ref(),
        );
        return Ok(());
    }

    log_dropped_report(
        payload.trace_id.as_str(),
        &payload.report_kind,
        "sync",
        payload.report_context.as_ref(),
    );
    Ok(())
}

pub(crate) fn spawn_sync_report(state: AppState, payload: GatewaySyncReportRequest) {
    let report_request_id_for_log =
        short_request_id(report_request_id(payload.report_context.as_ref()));
    spawn_fire_and_forget(TASK_KEY_USAGE_SYNC_REPORT, async move {
        let trace_id = payload.trace_id.clone();
        if let Err(err) = submit_sync_report(&state, payload).await {
            warn!(
                event_name = "execution_report_submit_failed",
                log_type = "ops",
                trace_id = %trace_id,
                report_scope = "sync",
                report_request_id = %report_request_id_for_log,
                error = ?err,
                "gateway failed to submit sync execution report"
            );
        }
    });
}

pub(crate) async fn submit_stream_report(
    state: &AppState,
    mut payload: GatewayStreamReportRequest,
) -> Result<(), GatewayError> {
    let original_report_context = payload.report_context.take();
    if let Some(report_context) =
        resolve_locally_actionable_report_context(state, original_report_context.as_ref()).await
    {
        payload.report_context = Some(report_context);
        if should_handle_local_stream_report(
            payload.report_context.as_ref(),
            payload.report_kind.as_str(),
        ) {
            handle_local_stream_report(state, &payload).await;
            log_local_report_handled(
                payload.trace_id.as_str(),
                &payload.report_kind,
                "stream",
                payload.report_context.as_ref(),
            );
            return Ok(());
        }
    }
    payload.report_context = original_report_context;

    if should_handle_local_stream_report(
        payload.report_context.as_ref(),
        payload.report_kind.as_str(),
    ) {
        handle_local_stream_report(state, &payload).await;
        log_local_report_handled(
            payload.trace_id.as_str(),
            &payload.report_kind,
            "stream",
            payload.report_context.as_ref(),
        );
        return Ok(());
    }

    if payload.report_context.is_some()
        && is_local_ai_stream_report_kind(payload.report_kind.as_str())
    {
        handle_local_stream_report(state, &payload).await;
        log_local_report_effect_only(
            payload.trace_id.as_str(),
            &payload.report_kind,
            "stream",
            payload.report_context.as_ref(),
        );
        return Ok(());
    }

    log_dropped_report(
        payload.trace_id.as_str(),
        &payload.report_kind,
        "stream",
        payload.report_context.as_ref(),
    );
    Ok(())
}

async fn handle_local_sync_report(state: &AppState, payload: &GatewaySyncReportRequest) {
    let terminal_unix_ms = current_unix_ms();
    let (error_type, error_message) =
        execution_error_details(None::<&ExecutionError>, payload.body_json.as_ref());
    let status = if sync_report_represents_failure(payload, error_type.as_deref()) {
        RequestCandidateStatus::Failed
    } else {
        RequestCandidateStatus::Success
    };
    let latency_ms = payload
        .telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.elapsed_ms);
    record_report_request_candidate_status(
        state,
        payload.report_context.as_ref(),
        SchedulerRequestCandidateStatusUpdate {
            status,
            status_code: Some(payload.status_code),
            error_type,
            error_message,
            latency_ms,
            started_at_unix_ms: None,
            finished_at_unix_ms: Some(terminal_unix_ms),
        },
    )
    .await;
    apply_local_report_effect(state, LocalReportEffect::Sync { payload }).await;
    spawn_niffler_shadow_reports(
        state,
        payload.report_context.as_ref(),
        status,
        Some(payload.status_code),
        latency_ms,
        terminal_unix_ms,
    );
}

async fn handle_local_stream_report(state: &AppState, payload: &GatewayStreamReportRequest) {
    let terminal_unix_ms = current_unix_ms();
    let latency_ms = payload
        .telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.elapsed_ms);
    record_report_request_candidate_status(
        state,
        payload.report_context.as_ref(),
        SchedulerRequestCandidateStatusUpdate {
            status: RequestCandidateStatus::Success,
            status_code: Some(payload.status_code),
            error_type: None,
            error_message: None,
            latency_ms,
            started_at_unix_ms: None,
            finished_at_unix_ms: Some(terminal_unix_ms),
        },
    )
    .await;
    apply_local_report_effect(state, LocalReportEffect::Stream { payload }).await;
    spawn_niffler_shadow_reports(
        state,
        payload.report_context.as_ref(),
        RequestCandidateStatus::Success,
        Some(payload.status_code),
        latency_ms,
        terminal_unix_ms,
    );
}

fn spawn_niffler_shadow_reports(
    state: &AppState,
    report_context: Option<&serde_json::Value>,
    status: RequestCandidateStatus,
    upstream_status_code: Option<u16>,
    latency_ms: Option<u64>,
    created_at_unix_ms: u64,
) {
    let Some(context) = report_context.cloned() else {
        return;
    };
    let Some(api_key_id) = parse_request_candidate_report_context(Some(&context))
        .and_then(|metadata| metadata.api_key_id)
    else {
        return;
    };

    let state = state.clone();
    spawn_fire_and_forget(TASK_KEY_NIFFLER_SHADOW_REPORT, async move {
        let decision = match resolve_niffler_runtime_rollout_decision(&state, &api_key_id).await {
            Ok(decision) => decision,
            Err(error) => {
                warn!(
                    event_name = "niffler_shadow_rollout_failed",
                    log_type = "ops",
                    api_key_id = %api_key_id,
                    error = ?error,
                    "gateway skipped niffler shadow writes because rollout lookup failed"
                );
                return;
            }
        };

        if decision.enable_new_routing {
            if let Some(record) = build_niffler_route_attempt_shadow_record(
                Some(&context),
                &decision,
                status,
                upstream_status_code,
                latency_ms,
                created_at_unix_ms,
            ) {
                if let Err(error) = state.create_niffler_route_attempt(record).await {
                    warn!(
                        event_name = "niffler_route_attempt_shadow_write_failed",
                        log_type = "ops",
                        api_key_id = %api_key_id,
                        error = ?error,
                        "gateway failed to write niffler route attempt shadow record"
                    );
                }
            } else {
                warn!(
                    event_name = "niffler_route_attempt_shadow_record_skipped",
                    log_type = "ops",
                    api_key_id = %api_key_id,
                    report_request_id = %short_request_id(aether_usage_runtime::report_request_id(Some(&context))),
                    "gateway skipped niffler route attempt shadow write because report context is incomplete"
                );
            }
        }

        if decision.enable_settlement_snapshot {
            if let Some(record) = build_niffler_settlement_snapshot_shadow_record(
                Some(&context),
                &decision,
                created_at_unix_ms,
            ) {
                if let Err(error) = state.create_niffler_settlement_snapshot(record).await {
                    warn!(
                        event_name = "niffler_settlement_snapshot_shadow_write_failed",
                        log_type = "ops",
                        api_key_id = %api_key_id,
                        error = ?error,
                        "gateway failed to write niffler settlement snapshot shadow record"
                    );
                }
            } else {
                warn!(
                    event_name = "niffler_settlement_snapshot_shadow_record_skipped",
                    log_type = "ops",
                    api_key_id = %api_key_id,
                    report_request_id = %short_request_id(aether_usage_runtime::report_request_id(Some(&context))),
                    "gateway skipped niffler settlement snapshot shadow write because report context has no complete settlement snapshot"
                );
            }
        }

        if decision.enable_billing_reservation {
            if let Some(record) = build_niffler_billing_reservation_dry_run_shadow_record(
                Some(&context),
                &decision,
                created_at_unix_ms,
            ) {
                if let Err(error) = state
                    .create_niffler_billing_reservation_dry_run(record)
                    .await
                {
                    warn!(
                        event_name = "niffler_billing_reservation_dry_run_shadow_write_failed",
                        log_type = "ops",
                        api_key_id = %api_key_id,
                        error = ?error,
                        "gateway failed to write niffler billing reservation dry run shadow record"
                    );
                }
            } else {
                warn!(
                    event_name = "niffler_billing_reservation_dry_run_shadow_record_skipped",
                    log_type = "ops",
                    api_key_id = %api_key_id,
                    report_request_id = %short_request_id(aether_usage_runtime::report_request_id(Some(&context))),
                    "gateway skipped niffler billing reservation dry run write because settlement or charge data is incomplete"
                );
            }
            finalize_niffler_billing_reservation(
                &state,
                &context,
                &decision,
                status,
                upstream_status_code,
                created_at_unix_ms,
            )
            .await;
        }
    });
}

async fn finalize_niffler_billing_reservation(
    state: &AppState,
    context: &serde_json::Value,
    decision: &NifflerRuntimeRolloutDecision,
    status: RequestCandidateStatus,
    upstream_status_code: Option<u16>,
    finalized_at_unix_ms: u64,
) {
    let Some(metadata) = parse_request_candidate_report_context(Some(context)) else {
        return;
    };
    let Some(request_id) = metadata.request_id else {
        return;
    };
    let (final_status, settlement_snapshot_id, release_reason) = match status {
        RequestCandidateStatus::Success => {
            match build_niffler_settlement_snapshot_shadow_record(
                Some(context),
                decision,
                finalized_at_unix_ms,
            ) {
                Some(record) => {
                    let snapshot_id = record.id.clone();
                    if let Err(error) = state.create_niffler_settlement_snapshot(record).await {
                        warn!(
                            event_name = "niffler_billing_reservation_settlement_snapshot_write_failed",
                            log_type = "ops",
                            report_request_id = %short_request_id(request_id.as_str()),
                            error = ?error,
                            "gateway failed to write settlement snapshot before finalizing niffler billing reservation"
                        );
                        (
                            NifflerBillingReservationStatus::ManualReview,
                            None,
                            Some("settlement_snapshot_write_failed".to_string()),
                        )
                    } else {
                        (
                            NifflerBillingReservationStatus::Settled,
                            Some(snapshot_id),
                            None,
                        )
                    }
                }
                None => (
                    NifflerBillingReservationStatus::ManualReview,
                    None,
                    Some("missing_settlement_snapshot".to_string()),
                ),
            }
        }
        RequestCandidateStatus::Failed => (
            NifflerBillingReservationStatus::Released,
            None,
            Some(release_reason("failed", upstream_status_code)),
        ),
        RequestCandidateStatus::Cancelled => (
            NifflerBillingReservationStatus::Released,
            None,
            Some(release_reason("cancelled", upstream_status_code)),
        ),
        _ => (
            NifflerBillingReservationStatus::Released,
            None,
            Some(release_reason("not_success", upstream_status_code)),
        ),
    };
    let record = FinalizeNifflerBillingReservationRecord {
        request_id: request_id.clone(),
        status: final_status,
        finalized_at_unix_ms,
        settlement_snapshot_id,
        release_reason,
        event_id: stable_niffler_billing_reservation_event_id(&request_id, final_status),
        event_idempotency_key: format!(
            "reservation-event-finalize:{}:{}",
            final_status.as_str(),
            request_id
        ),
        actor_id: metadata.user_id,
    };
    if let Err(error) = state
        .finalize_niffler_billing_reservation_by_request_id(record)
        .await
    {
        warn!(
            event_name = "niffler_billing_reservation_finalize_failed",
            log_type = "ops",
            report_request_id = %short_request_id(request_id.as_str()),
            error = ?error,
            "gateway failed to finalize niffler billing reservation"
        );
    }
}

fn release_reason(label: &str, upstream_status_code: Option<u16>) -> String {
    match upstream_status_code {
        Some(status_code) => format!("{label}:upstream_status_{status_code}"),
        None => label.to_string(),
    }
}

fn stable_niffler_billing_reservation_event_id(
    request_id: &str,
    status: NifflerBillingReservationStatus,
) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!(
            "niffler-billing-reservation-event-finalize:{}:{request_id}",
            status.as_str()
        )
        .as_bytes(),
    )
    .to_string()
}

fn build_niffler_route_attempt_shadow_record(
    report_context: Option<&serde_json::Value>,
    decision: &NifflerRuntimeRolloutDecision,
    status: RequestCandidateStatus,
    upstream_status_code: Option<u16>,
    latency_ms: Option<u64>,
    created_at_unix_ms: u64,
) -> Option<CreateNifflerRouteAttemptRecord> {
    let context = report_context?;
    let metadata = parse_request_candidate_report_context(Some(context))?;
    let request_id = metadata.request_id?;
    let model_name = resolve_niffler_route_attempt_model_name(context, metadata.mapped_model)?;
    let attempt_index = metadata.candidate_index.unwrap_or_default();
    let id = build_niffler_route_attempt_shadow_id(&request_id, attempt_index);

    Some(CreateNifflerRouteAttemptRecord {
        id,
        request_id,
        upstream_service_id: metadata.provider_id,
        upstream_account_id: metadata.key_id,
        product_plan_id: decision.product_plan_id.clone(),
        model_name,
        attempt_index,
        status: niffler_route_attempt_status(status).to_string(),
        skip_reason: None,
        upstream_status_code,
        latency_ms,
        created_at_unix_ms,
    })
}

fn build_niffler_settlement_snapshot_shadow_record(
    report_context: Option<&serde_json::Value>,
    decision: &NifflerRuntimeRolloutDecision,
    created_at_unix_ms: u64,
) -> Option<CreateNifflerSettlementSnapshotRecord> {
    let context = report_context?;
    let metadata = parse_request_candidate_report_context(Some(context))?;
    let request_id = metadata.request_id?;
    let settlement_snapshot = context.get("settlement_snapshot")?.as_object()?;
    let charge_breakdown = context.get("charge_breakdown_snapshot")?.as_object()?;
    let pricing_snapshot = settlement_snapshot.get("pricing_snapshot")?.clone();
    let requested_model_name = resolve_niffler_settlement_requested_model_name(
        context,
        settlement_snapshot,
        metadata.mapped_model.clone(),
    )?;
    let upstream_execution_model_name =
        metadata
            .mapped_model
            .and_then(non_empty_string)
            .or_else(|| {
                settlement_snapshot_nested_string(
                    settlement_snapshot,
                    "pricing_snapshot",
                    "provider_model_name",
                )
            });
    let image_tool_model_name = string_field(context, "image_tool_model_name")
        .or_else(|| string_field(context, "image_model"))
        .or_else(|| {
            settlement_snapshot_nested_string(
                settlement_snapshot,
                "pricing_snapshot",
                "image_tool_model_name",
            )
        });
    let wallet_charge = snapshot_number(charge_breakdown, "wallet_debit_usd");
    let entitlement_charge = snapshot_number(charge_breakdown, "package_debit_usd");
    if wallet_charge.is_none() && entitlement_charge.is_none() {
        return None;
    }
    let wallet_charge_usd = wallet_charge.unwrap_or_default();
    let entitlement_charge_usd = entitlement_charge.unwrap_or_default();
    let upstream_cost_usd = settlement_snapshot_number(settlement_snapshot, "actual_total_cost")
        .or_else(|| context_number_field(context, "actual_total_cost_usd"))?;
    let user_charge_usd = wallet_charge_usd + entitlement_charge_usd;
    let gross_margin_usd = user_charge_usd - upstream_cost_usd;
    if !gross_margin_usd.is_finite() {
        return None;
    }

    Some(CreateNifflerSettlementSnapshotRecord {
        id: build_niffler_settlement_snapshot_shadow_id(&request_id),
        request_id,
        user_id: metadata.user_id,
        api_key_id: metadata.api_key_id,
        product_plan_id: decision.product_plan_id.clone(),
        upstream_service_id: metadata.provider_id,
        upstream_account_id: metadata.key_id,
        requested_model_name,
        upstream_execution_model_name,
        image_tool_model_name,
        pricing_snapshot,
        wallet_charge_usd,
        entitlement_charge_usd,
        upstream_cost_usd,
        gross_margin_usd,
        created_at_unix_ms,
        finalized_at_unix_ms: Some(created_at_unix_ms),
    })
}

fn build_niffler_settlement_snapshot_shadow_id(request_id: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("niffler-settlement-snapshot:{request_id}").as_bytes(),
    )
    .to_string()
}

fn build_niffler_billing_reservation_dry_run_shadow_record(
    report_context: Option<&serde_json::Value>,
    decision: &NifflerRuntimeRolloutDecision,
    created_at_unix_ms: u64,
) -> Option<CreateNifflerBillingReservationDryRunRecord> {
    let context = report_context?;
    let metadata = parse_request_candidate_report_context(Some(context))?;
    let request_id = metadata.request_id?;
    let settlement_snapshot = context.get("settlement_snapshot")?.as_object()?;
    let charge_breakdown = context.get("charge_breakdown_snapshot")?.as_object()?;
    let requested_model_name = resolve_niffler_settlement_requested_model_name(
        context,
        settlement_snapshot,
        metadata.mapped_model,
    )?;
    let estimated_reservation_usd =
        settlement_snapshot_number(settlement_snapshot, "user_total_cost_usd")
            .or_else(|| settlement_snapshot_number(settlement_snapshot, "total_cost"))?;
    let wallet_charge = snapshot_number(charge_breakdown, "wallet_debit_usd");
    let entitlement_charge = snapshot_number(charge_breakdown, "package_debit_usd");
    if wallet_charge.is_none() && entitlement_charge.is_none() {
        return None;
    }
    let legacy_final_charge_usd =
        wallet_charge.unwrap_or_default() + entitlement_charge.unwrap_or_default();
    if !legacy_final_charge_usd.is_finite() {
        return None;
    }
    let difference_usd = estimated_reservation_usd - legacy_final_charge_usd;
    if !difference_usd.is_finite() {
        return None;
    }

    Some(CreateNifflerBillingReservationDryRunRecord {
        id: build_niffler_billing_reservation_dry_run_shadow_id(&request_id),
        request_id,
        user_id: metadata.user_id,
        api_key_id: metadata.api_key_id,
        product_plan_id: decision.product_plan_id.clone(),
        requested_model_name,
        estimated_reservation_usd,
        legacy_final_charge_usd,
        difference_usd,
        estimation_source: "settlement_snapshot_total_cost".to_string(),
        status: niffler_billing_reservation_dry_run_status(difference_usd).to_string(),
        created_at_unix_ms,
        finalized_at_unix_ms: Some(created_at_unix_ms),
    })
}

fn build_niffler_billing_reservation_dry_run_shadow_id(request_id: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("niffler-billing-reservation-dry-run:{request_id}").as_bytes(),
    )
    .to_string()
}

fn niffler_billing_reservation_dry_run_status(difference_usd: f64) -> &'static str {
    if difference_usd.abs() <= NIFFLER_BILLING_RESERVATION_DRY_RUN_MATCH_TOLERANCE_USD {
        "matched"
    } else if difference_usd > 0.0 {
        "over_reserved"
    } else {
        "under_reserved"
    }
}

fn resolve_niffler_settlement_requested_model_name(
    context: &serde_json::Value,
    settlement_snapshot: &serde_json::Map<String, serde_json::Value>,
    mapped_model: Option<String>,
) -> Option<String> {
    string_field(context, "model")
        .or_else(|| string_field(context, "global_model_name"))
        .or_else(|| string_field(context, "requested_model"))
        .or_else(|| {
            settlement_snapshot_nested_string(
                settlement_snapshot,
                "pricing_snapshot",
                "global_model_name",
            )
        })
        .or_else(|| mapped_model.and_then(non_empty_string))
}

fn build_niffler_route_attempt_shadow_id(request_id: &str, attempt_index: u32) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("niffler-route-attempt:{request_id}:{attempt_index}").as_bytes(),
    )
    .to_string()
}

fn resolve_niffler_route_attempt_model_name(
    context: &serde_json::Value,
    mapped_model: Option<String>,
) -> Option<String> {
    mapped_model
        .and_then(non_empty_string)
        .or_else(|| string_field(context, "model"))
        .or_else(|| string_field(context, "global_model_name"))
        .or_else(|| string_field(context, "requested_model"))
}

fn niffler_route_attempt_status(status: RequestCandidateStatus) -> &'static str {
    match status {
        RequestCandidateStatus::Success => "success",
        RequestCandidateStatus::Skipped => "skipped",
        RequestCandidateStatus::Cancelled => "cancelled",
        RequestCandidateStatus::Failed => "failed",
        RequestCandidateStatus::Available
        | RequestCandidateStatus::Unused
        | RequestCandidateStatus::Pending
        | RequestCandidateStatus::Streaming => "failed",
    }
}

fn string_field(context: &serde_json::Value, key: &str) -> Option<String> {
    context
        .get(key)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| non_empty_string(value.to_string()))
}

fn context_number_field(context: &serde_json::Value, key: &str) -> Option<f64> {
    context
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn settlement_snapshot_number(
    snapshot: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<f64> {
    snapshot
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn snapshot_number(
    snapshot: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<f64> {
    snapshot
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn settlement_snapshot_nested_string(
    snapshot: &serde_json::Map<String, serde_json::Value>,
    child: &str,
    key: &str,
) -> Option<String> {
    snapshot
        .get(child)
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.get(key))
        .and_then(serde_json::Value::as_str)
        .and_then(|value| non_empty_string(value.to_string()))
}

fn non_empty_string(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use aether_data::repository::candidates::InMemoryRequestCandidateRepository;
    use aether_data::repository::gemini_file_mappings::{
        GeminiFileMappingReadRepository, InMemoryGeminiFileMappingRepository,
    };
    use aether_data::repository::provider_catalog::InMemoryProviderCatalogReadRepository;
    use aether_data::repository::usage::InMemoryUsageReadRepository;
    use aether_data::repository::video_tasks::InMemoryVideoTaskRepository;
    use aether_data::{DatabaseDriver, SqlDatabaseConfig, SqlPoolConfig};
    use aether_data_contracts::repository::candidates::{
        RequestCandidateReadRepository, RequestCandidateStatus, StoredRequestCandidate,
    };
    use aether_data_contracts::repository::niffler_core::{
        CreateNifflerBillingReservationRecord, NifflerBillingReservationListQuery,
        NifflerBillingReservationStatus, NifflerSettlementSnapshotListQuery,
    };
    use aether_data_contracts::repository::provider_catalog::{
        ProviderCatalogReadRepository, StoredProviderCatalogKey, StoredProviderCatalogProvider,
    };
    use aether_data_contracts::repository::video_tasks::{
        UpsertVideoTask, VideoTaskStatus, VideoTaskWriteRepository,
    };
    use serde_json::json;

    use super::{
        build_niffler_billing_reservation_dry_run_shadow_record,
        build_niffler_route_attempt_shadow_record, build_niffler_settlement_snapshot_shadow_record,
        finalize_niffler_billing_reservation, resolve_locally_actionable_report_context,
        submit_stream_report, submit_sync_report, GatewayStreamReportRequest,
        GatewaySyncReportRequest,
    };
    use crate::data::{GatewayDataConfig, GatewayDataState};
    use crate::niffler_runtime::{
        NifflerRuntimeRolloutDecision, NifflerRuntimeRolloutDecisionSource,
    };
    use crate::AppState;

    fn sample_request_candidate(id: &str, request_id: &str) -> StoredRequestCandidate {
        StoredRequestCandidate::new(
            id.to_string(),
            request_id.to_string(),
            Some("user-reporting-tests-123".to_string()),
            Some("api-key-reporting-tests-123".to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            0,
            0,
            Some("provider-reporting-tests-123".to_string()),
            Some("endpoint-reporting-tests-123".to_string()),
            Some("key-reporting-tests-123".to_string()),
            RequestCandidateStatus::Pending,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            1_700_000_000_000,
            Some(1_700_000_000_000),
            None,
        )
        .expect("request candidate should build")
    }

    fn sample_request_candidate_with_transport(
        id: &str,
        request_id: &str,
        user_id: &str,
        api_key_id: &str,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> StoredRequestCandidate {
        StoredRequestCandidate::new(
            id.to_string(),
            request_id.to_string(),
            Some(user_id.to_string()),
            Some(api_key_id.to_string()),
            Some("alice".to_string()),
            Some("default".to_string()),
            0,
            0,
            Some(provider_id.to_string()),
            Some(endpoint_id.to_string()),
            Some(key_id.to_string()),
            RequestCandidateStatus::Pending,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            1_700_000_000_000,
            Some(1_700_000_000_000),
            None,
        )
        .expect("request candidate should build")
    }

    fn build_test_state(repository: Arc<InMemoryRequestCandidateRepository>) -> AppState {
        AppState::new()
            .expect("gateway state should build")
            .with_data_state_for_tests(
                GatewayDataState::with_request_candidate_and_usage_repository_for_tests(
                    repository,
                    Arc::new(InMemoryUsageReadRepository::default()),
                ),
            )
    }

    fn build_video_test_state(
        video_repository: Arc<InMemoryVideoTaskRepository>,
        request_candidate_repository: Arc<InMemoryRequestCandidateRepository>,
    ) -> AppState {
        AppState::new()
            .expect("gateway state should build")
            .with_data_state_for_tests(
                GatewayDataState::with_video_task_and_request_candidate_repository_for_tests(
                    video_repository,
                    request_candidate_repository,
                ),
            )
    }

    fn build_gemini_file_mapping_test_state(
        request_candidate_repository: Arc<InMemoryRequestCandidateRepository>,
        gemini_file_mapping_repository: Arc<InMemoryGeminiFileMappingRepository>,
    ) -> AppState {
        AppState::new()
            .expect("gateway state should build")
            .with_data_state_for_tests(
            GatewayDataState::with_request_candidate_and_gemini_file_mapping_repository_for_tests(
                request_candidate_repository,
                gemini_file_mapping_repository,
            ),
        )
    }

    fn build_provider_catalog_test_state(
        repository: Arc<InMemoryProviderCatalogReadRepository>,
    ) -> AppState {
        AppState::new()
            .expect("gateway state should build")
            .with_data_state_for_tests(
                GatewayDataState::with_provider_catalog_repository_for_tests(repository),
            )
    }

    fn sample_provider_catalog_provider(
        provider_id: &str,
        provider_type: &str,
    ) -> StoredProviderCatalogProvider {
        StoredProviderCatalogProvider::new(
            provider_id.to_string(),
            provider_type.to_string(),
            None,
            provider_type.to_string(),
        )
        .expect("provider should build")
    }

    fn sample_provider_catalog_key(key_id: &str, provider_id: &str) -> StoredProviderCatalogKey {
        StoredProviderCatalogKey::new(
            key_id.to_string(),
            provider_id.to_string(),
            "default".to_string(),
            "bearer".to_string(),
            None,
            true,
        )
        .expect("key should build")
        .with_transport_fields(
            Some(json!(["openai:responses"])),
            "sk-codex-test".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("key transport should build")
    }

    fn sample_codex_paid_headers() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("x-codex-plan-type".to_string(), "team".to_string()),
            (
                "x-codex-primary-used-percent".to_string(),
                "100".to_string(),
            ),
            (
                "x-codex-secondary-used-percent".to_string(),
                "31".to_string(),
            ),
            (
                "x-codex-primary-window-minutes".to_string(),
                "300".to_string(),
            ),
            (
                "x-codex-secondary-window-minutes".to_string(),
                "10080".to_string(),
            ),
            (
                "x-codex-primary-reset-after-seconds".to_string(),
                "15160".to_string(),
            ),
            (
                "x-codex-secondary-reset-after-seconds".to_string(),
                "524059".to_string(),
            ),
        ])
    }

    fn enabled_rollout_decision(api_key_id: &str) -> NifflerRuntimeRolloutDecision {
        NifflerRuntimeRolloutDecision {
            api_key_id: api_key_id.to_string(),
            product_plan_id: Some("product-plan-reporting-tests-123".to_string()),
            source: Some(NifflerRuntimeRolloutDecisionSource::ProductPlan),
            enable_new_routing: true,
            enable_settlement_snapshot: false,
            enable_error_return_rules: false,
            enable_billing_reservation: false,
            enable_referral_ledger: false,
        }
    }

    async fn sqlite_niffler_state() -> AppState {
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
        state
    }

    fn billing_reservation_record(request_id: &str) -> CreateNifflerBillingReservationRecord {
        CreateNifflerBillingReservationRecord {
            id: format!("reservation-{request_id}"),
            request_id: request_id.to_string(),
            user_id: Some("user-finalize-tests-123".to_string()),
            api_key_id: Some("api-key-finalize-tests-123".to_string()),
            product_plan_id: Some("product-plan-reporting-tests-123".to_string()),
            reserved_total_usd: 0.24,
            wallet_reserved_usd: 0.24,
            entitlement_reserved_usd: 0.0,
            reserved_at_unix_ms: 1_700_000_000_000,
            expires_at_unix_ms: 1_700_000_060_000,
            idempotency_key: format!("reservation-idempotency-{request_id}"),
            event_id: format!("reserved-event-{request_id}"),
            event_idempotency_key: format!("reserved-event-idempotency-{request_id}"),
            actor_id: Some("user-finalize-tests-123".to_string()),
        }
    }

    async fn seed_video_task(
        repository: &InMemoryVideoTaskRepository,
        id: &str,
        short_id: Option<&str>,
        request_id: &str,
        user_id: &str,
        api_key_id: &str,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
        client_api_format: &str,
        provider_api_format: &str,
    ) {
        repository
            .upsert(UpsertVideoTask {
                id: id.to_string(),
                short_id: short_id.map(ToOwned::to_owned),
                request_id: request_id.to_string(),
                user_id: Some(user_id.to_string()),
                api_key_id: Some(api_key_id.to_string()),
                username: Some("video-user".to_string()),
                api_key_name: Some("video-key".to_string()),
                external_task_id: Some("ext-video-task-reporting-123".to_string()),
                provider_id: Some(provider_id.to_string()),
                endpoint_id: Some(endpoint_id.to_string()),
                key_id: Some(key_id.to_string()),
                client_api_format: Some(client_api_format.to_string()),
                provider_api_format: Some(provider_api_format.to_string()),
                format_converted: false,
                model: Some("video-model".to_string()),
                prompt: Some("video prompt".to_string()),
                original_request_body: Some(json!({"prompt": "video prompt"})),
                duration_seconds: Some(4),
                resolution: Some("720p".to_string()),
                aspect_ratio: Some("16:9".to_string()),
                size: Some("1280x720".to_string()),
                status: VideoTaskStatus::Submitted,
                progress_percent: 0,
                progress_message: None,
                retry_count: 0,
                poll_interval_seconds: 10,
                next_poll_at_unix_secs: Some(1_700_000_010),
                poll_count: 0,
                max_poll_count: 360,
                created_at_unix_ms: 1_700_000_000,
                submitted_at_unix_secs: Some(1_700_000_000),
                completed_at_unix_secs: None,
                updated_at_unix_secs: 1_700_000_000,
                error_code: None,
                error_message: None,
                video_url: None,
                request_metadata: None,
            })
            .await
            .expect("video task should upsert");
    }

    #[tokio::test]
    async fn keeps_request_id_only_context_non_actionable_without_existing_candidate() {
        let repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state = build_test_state(repository);
        let report_context = json!({
            "request_id": "req-reporting-weak-123",
            "client_api_format": "openai:chat"
        });

        let resolved =
            resolve_locally_actionable_report_context(&state, Some(&report_context)).await;

        assert!(resolved.is_none());
    }

    #[tokio::test]
    async fn submit_sync_report_handles_request_id_only_context_locally_when_unique_candidate_exists(
    ) {
        let repository = Arc::new(InMemoryRequestCandidateRepository::seed(vec![
            sample_request_candidate("cand-reporting-sync-123", "req-reporting-sync-123"),
        ]));
        let state = build_test_state(Arc::clone(&repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-reporting-sync-123".to_string(),
                report_kind: "openai_chat_sync_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-reporting-sync-123",
                    "client_api_format": "openai:chat"
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("sync report should stay local");

        let stored = repository
            .list_by_request_id("req-reporting-sync-123")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "cand-reporting-sync-123");
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
        assert_eq!(
            stored[0].provider_id.as_deref(),
            Some("provider-reporting-tests-123")
        );
    }

    #[tokio::test]
    async fn submit_sync_report_handles_openai_image_success_locally_when_unique_candidate_exists()
    {
        let repository = Arc::new(InMemoryRequestCandidateRepository::seed(vec![
            sample_request_candidate(
                "cand-reporting-image-sync-123",
                "req-reporting-image-sync-123",
            ),
        ]));
        let state = build_test_state(Arc::clone(&repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-reporting-image-sync-123".to_string(),
                report_kind: "openai_image_sync_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-reporting-image-sync-123",
                    "client_api_format": "openai:image"
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                body_json: Some(json!({
                    "created": 1776855978,
                    "data": [{
                        "b64_json": "aGVsbG8="
                    }]
                })),
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("image sync report should stay local");

        let stored = repository
            .list_by_request_id("req-reporting-image-sync-123")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "cand-reporting-image-sync-123");
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
        assert_eq!(stored[0].status_code, Some(200));
    }

    #[tokio::test]
    async fn submit_sync_report_treats_null_error_field_as_success() {
        let repository = Arc::new(InMemoryRequestCandidateRepository::seed(vec![
            sample_request_candidate("cand-reporting-sync-null-1", "req-reporting-sync-null-1"),
        ]));
        let state = build_test_state(Arc::clone(&repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-reporting-sync-null-1".to_string(),
                report_kind: "claude_cli_sync_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-reporting-sync-null-1",
                    "client_api_format": "claude:messages",
                    "provider_api_format": "openai:responses"
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                body_json: Some(json!({
                    "id": "resp_1",
                    "status": "completed",
                    "error": null
                })),
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("sync report should stay local");

        let stored = repository
            .list_by_request_id("req-reporting-sync-null-1")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
        assert_eq!(stored[0].status_code, Some(200));
    }

    #[tokio::test]
    async fn submit_stream_report_handles_request_id_only_context_locally_when_unique_candidate_exists(
    ) {
        let repository = Arc::new(InMemoryRequestCandidateRepository::seed(vec![
            sample_request_candidate("cand-reporting-stream-123", "req-reporting-stream-123"),
        ]));
        let state = build_test_state(Arc::clone(&repository));

        submit_stream_report(
            &state,
            GatewayStreamReportRequest {
                trace_id: "trace-reporting-stream-123".to_string(),
                report_kind: "openai_chat_stream_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-reporting-stream-123",
                    "client_api_format": "openai:chat"
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                provider_body_base64: None,
                provider_body_state: None,
                client_body_base64: None,
                client_body_state: None,
                terminal_summary: None,
                telemetry: None,
            },
        )
        .await
        .expect("stream report should stay local");

        let stored = repository
            .list_by_request_id("req-reporting-stream-123")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "cand-reporting-stream-123");
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
        assert_eq!(
            stored[0].endpoint_id.as_deref(),
            Some("endpoint-reporting-tests-123")
        );
    }

    #[tokio::test]
    async fn submit_sync_report_updates_codex_quota_from_response_headers() {
        crate::orchestration::clear_local_report_effect_caches_for_tests();

        let provider_catalog_repository = Arc::new(InMemoryProviderCatalogReadRepository::seed(
            vec![sample_provider_catalog_provider(
                "provider-codex-sync",
                "codex",
            )],
            Vec::new(),
            vec![sample_provider_catalog_key(
                "key-codex-sync",
                "provider-codex-sync",
            )],
        ));
        let state = build_provider_catalog_test_state(Arc::clone(&provider_catalog_repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-codex-reporting-sync".to_string(),
                report_kind: "openai_responses_sync_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-codex-reporting-sync",
                    "key_id": "key-codex-sync"
                })),
                status_code: 200,
                headers: sample_codex_paid_headers(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("sync report should stay local");

        let reloaded = provider_catalog_repository
            .list_keys_by_ids(&["key-codex-sync".to_string()])
            .await
            .expect("keys should list");
        let codex = reloaded[0]
            .upstream_metadata
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata| metadata.get("codex"))
            .and_then(serde_json::Value::as_object)
            .expect("codex metadata should exist");
        assert_eq!(codex.get("primary_used_percent"), Some(&json!(31.0)));
        assert_eq!(codex.get("secondary_used_percent"), Some(&json!(100.0)));
        let quota = reloaded[0]
            .status_snapshot
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|snapshot| snapshot.get("quota"))
            .and_then(serde_json::Value::as_object)
            .expect("quota snapshot should exist");
        assert_eq!(quota.get("provider_type"), Some(&json!("codex")));
        assert_eq!(quota.get("source"), Some(&json!("response_headers")));
        assert_eq!(quota.get("code"), Some(&json!("exhausted")));
        assert_eq!(quota.get("updated_at"), quota.get("observed_at"));
    }

    #[tokio::test]
    async fn submit_sync_report_preserves_codex_feature_quota_and_reset_credits() {
        crate::orchestration::clear_local_report_effect_caches_for_tests();

        let mut key = sample_provider_catalog_key(
            "key-codex-sync-feature-quota",
            "provider-codex-sync-feature-quota",
        );
        key.upstream_metadata = Some(json!({
            "codex": {
                "updated_at": 1_777_000_000u64,
                "plan_type": "pro",
                "rate_limit_reset_credits": { "available_count": 1 },
                "windows": [
                    {
                        "code": "weekly",
                        "label": "7D",
                        "scope": "account",
                        "source_role": "secondary",
                        "used_percent": 5.0,
                        "reset_after_seconds": 500_000u64,
                        "window_seconds": 604_800u64,
                        "window_minutes": 10_080u64
                    },
                    {
                        "code": "spark:5h",
                        "label": "GPT-5.3-Codex-Spark 5H",
                        "scope": "feature",
                        "source_role": "primary",
                        "used_percent": 0.0,
                        "reset_after_seconds": 15_000u64,
                        "window_seconds": 18_000u64,
                        "window_minutes": 300u64
                    },
                    {
                        "code": "spark:weekly",
                        "label": "GPT-5.3-Codex-Spark 7D",
                        "scope": "feature",
                        "source_role": "secondary",
                        "used_percent": 0.0,
                        "reset_after_seconds": 500_000u64,
                        "window_seconds": 604_800u64,
                        "window_minutes": 10_080u64
                    }
                ]
            }
        }));
        let provider_catalog_repository = Arc::new(InMemoryProviderCatalogReadRepository::seed(
            vec![sample_provider_catalog_provider(
                "provider-codex-sync-feature-quota",
                "codex",
            )],
            Vec::new(),
            vec![key],
        ));
        let state = build_provider_catalog_test_state(Arc::clone(&provider_catalog_repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-codex-reporting-sync-feature-quota".to_string(),
                report_kind: "openai_responses_sync_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-codex-reporting-sync-feature-quota",
                    "key_id": "key-codex-sync-feature-quota"
                })),
                status_code: 200,
                headers: sample_codex_paid_headers(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("sync report should stay local");

        let reloaded = provider_catalog_repository
            .list_keys_by_ids(&["key-codex-sync-feature-quota".to_string()])
            .await
            .expect("keys should list");
        let codex = reloaded[0]
            .upstream_metadata
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata| metadata.get("codex"))
            .and_then(serde_json::Value::as_object)
            .expect("codex metadata should exist");
        let metadata_windows = codex
            .get("windows")
            .and_then(serde_json::Value::as_array)
            .expect("codex metadata windows should exist");
        assert_eq!(
            codex.get("rate_limit_reset_credits"),
            Some(&json!({ "available_count": 1 }))
        );
        assert!(metadata_windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("spark:5h"))));
        assert!(metadata_windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("spark:weekly"))));

        let quota = reloaded[0]
            .status_snapshot
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|snapshot| snapshot.get("quota"))
            .and_then(serde_json::Value::as_object)
            .expect("quota snapshot should exist");
        let quota_windows = quota
            .get("windows")
            .and_then(serde_json::Value::as_array)
            .expect("quota windows should exist");
        assert_eq!(quota.get("source"), Some(&json!("response_headers")));
        assert_eq!(
            quota.get("reset_credits"),
            Some(&json!({ "available_count": 1 }))
        );
        assert!(quota_windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("spark:5h"))));
        assert!(quota_windows
            .iter()
            .any(|window| window.get("code") == Some(&json!("spark:weekly"))));
    }

    #[tokio::test]
    async fn submit_sync_report_updates_codex_quota_from_provider_response_headers() {
        crate::orchestration::clear_local_report_effect_caches_for_tests();

        let provider_catalog_repository = Arc::new(InMemoryProviderCatalogReadRepository::seed(
            vec![sample_provider_catalog_provider(
                "provider-codex-sync-provider-headers",
                "codex",
            )],
            Vec::new(),
            vec![sample_provider_catalog_key(
                "key-codex-sync-provider-headers",
                "provider-codex-sync-provider-headers",
            )],
        ));
        let state = build_provider_catalog_test_state(Arc::clone(&provider_catalog_repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-codex-reporting-sync-provider-headers".to_string(),
                report_kind: "openai_responses_sync_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-codex-reporting-sync-provider-headers",
                    "key_id": "key-codex-sync-provider-headers",
                    "provider_response_headers": sample_codex_paid_headers()
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("sync report should stay local");

        let reloaded = provider_catalog_repository
            .list_keys_by_ids(&["key-codex-sync-provider-headers".to_string()])
            .await
            .expect("keys should list");
        let codex = reloaded[0]
            .upstream_metadata
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata| metadata.get("codex"))
            .and_then(serde_json::Value::as_object)
            .expect("codex metadata should exist");
        assert_eq!(codex.get("primary_used_percent"), Some(&json!(31.0)));
        assert_eq!(codex.get("secondary_used_percent"), Some(&json!(100.0)));
    }

    #[tokio::test]
    async fn submit_stream_report_updates_codex_quota_from_response_headers() {
        crate::orchestration::clear_local_report_effect_caches_for_tests();

        let provider_catalog_repository = Arc::new(InMemoryProviderCatalogReadRepository::seed(
            vec![sample_provider_catalog_provider(
                "provider-codex-stream",
                "codex",
            )],
            Vec::new(),
            vec![sample_provider_catalog_key(
                "key-codex-stream",
                "provider-codex-stream",
            )],
        ));
        let state = build_provider_catalog_test_state(Arc::clone(&provider_catalog_repository));

        submit_stream_report(
            &state,
            GatewayStreamReportRequest {
                trace_id: "trace-codex-reporting-stream".to_string(),
                report_kind: "openai_responses_stream_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-codex-reporting-stream",
                    "key_id": "key-codex-stream"
                })),
                status_code: 200,
                headers: sample_codex_paid_headers(),
                provider_body_base64: None,
                provider_body_state: None,
                client_body_base64: None,
                client_body_state: None,
                terminal_summary: None,
                telemetry: None,
            },
        )
        .await
        .expect("stream report should stay local");

        let reloaded = provider_catalog_repository
            .list_keys_by_ids(&["key-codex-stream".to_string()])
            .await
            .expect("keys should list");
        let codex = reloaded[0]
            .upstream_metadata
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata| metadata.get("codex"))
            .and_then(serde_json::Value::as_object)
            .expect("codex metadata should exist");
        assert_eq!(codex.get("primary_used_percent"), Some(&json!(31.0)));
        assert_eq!(codex.get("secondary_used_percent"), Some(&json!(100.0)));
        let quota = reloaded[0]
            .status_snapshot
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|snapshot| snapshot.get("quota"))
            .and_then(serde_json::Value::as_object)
            .expect("quota snapshot should exist");
        assert_eq!(quota.get("provider_type"), Some(&json!("codex")));
        assert_eq!(quota.get("source"), Some(&json!("response_headers")));
        assert_eq!(quota.get("code"), Some(&json!("exhausted")));
        assert_eq!(quota.get("updated_at"), quota.get("observed_at"));
    }

    #[tokio::test]
    async fn submit_stream_report_updates_codex_quota_from_provider_response_headers() {
        crate::orchestration::clear_local_report_effect_caches_for_tests();

        let provider_catalog_repository = Arc::new(InMemoryProviderCatalogReadRepository::seed(
            vec![sample_provider_catalog_provider(
                "provider-codex-stream-provider-headers",
                "codex",
            )],
            Vec::new(),
            vec![sample_provider_catalog_key(
                "key-codex-stream-provider-headers",
                "provider-codex-stream-provider-headers",
            )],
        ));
        let state = build_provider_catalog_test_state(Arc::clone(&provider_catalog_repository));

        submit_stream_report(
            &state,
            GatewayStreamReportRequest {
                trace_id: "trace-codex-reporting-stream-provider-headers".to_string(),
                report_kind: "openai_responses_stream_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-codex-reporting-stream-provider-headers",
                    "key_id": "key-codex-stream-provider-headers",
                    "provider_response_headers": sample_codex_paid_headers()
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                provider_body_base64: None,
                provider_body_state: None,
                client_body_base64: None,
                client_body_state: None,
                terminal_summary: None,
                telemetry: None,
            },
        )
        .await
        .expect("stream report should stay local");

        let reloaded = provider_catalog_repository
            .list_keys_by_ids(&["key-codex-stream-provider-headers".to_string()])
            .await
            .expect("keys should list");
        let codex = reloaded[0]
            .upstream_metadata
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata| metadata.get("codex"))
            .and_then(serde_json::Value::as_object)
            .expect("codex metadata should exist");
        assert_eq!(codex.get("primary_used_percent"), Some(&json!(31.0)));
        assert_eq!(codex.get("secondary_used_percent"), Some(&json!(100.0)));
    }

    #[tokio::test]
    async fn submit_sync_report_stores_gemini_file_mapping_locally_when_payload_contains_file_json()
    {
        let request_candidate_repository =
            Arc::new(InMemoryRequestCandidateRepository::seed(vec![
                sample_request_candidate(
                    "cand-gemini-files-store-123",
                    "req-gemini-files-store-123",
                ),
            ]));
        let gemini_file_mapping_repository =
            Arc::new(InMemoryGeminiFileMappingRepository::default());
        let state = build_gemini_file_mapping_test_state(
            Arc::clone(&request_candidate_repository),
            Arc::clone(&gemini_file_mapping_repository),
        );

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-gemini-files-store-123".to_string(),
                report_kind: "gemini_files_store_mapping".to_string(),
                report_context: Some(json!({
                    "request_id": "req-gemini-files-store-123",
                    "candidate_id": "cand-gemini-files-store-123",
                    "candidate_index": 0,
                    "provider_id": "provider-reporting-tests-123",
                    "endpoint_id": "endpoint-reporting-tests-123",
                    "key_id": "key-reporting-tests-123",
                    "file_key_id": "key-reporting-tests-123",
                    "user_id": "user-reporting-tests-123",
                })),
                status_code: 200,
                headers: BTreeMap::from([(
                    "content-type".to_string(),
                    "application/json".to_string(),
                )]),
                body_json: Some(json!({
                    "file": {
                        "name": "abc123",
                        "displayName": "test-image",
                        "mimeType": "image/png"
                    }
                })),
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("gemini files mapping report should stay local");

        let stored = gemini_file_mapping_repository
            .find_by_file_name("files/abc123")
            .await
            .expect("gemini file mapping should read")
            .expect("gemini file mapping should exist");
        assert_eq!(stored.key_id, "key-reporting-tests-123");
        assert_eq!(stored.user_id.as_deref(), Some("user-reporting-tests-123"));
        assert_eq!(stored.display_name.as_deref(), Some("test-image"));
        assert_eq!(stored.mime_type.as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn submit_sync_report_stores_gemini_file_mapping_without_actionable_candidate_context() {
        let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let gemini_file_mapping_repository =
            Arc::new(InMemoryGeminiFileMappingRepository::default());
        let state = build_gemini_file_mapping_test_state(
            Arc::clone(&request_candidate_repository),
            Arc::clone(&gemini_file_mapping_repository),
        );

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-gemini-files-store-no-candidate-123".to_string(),
                report_kind: "gemini_files_store_mapping".to_string(),
                report_context: Some(json!({
                    "request_id": "req-gemini-files-store-no-candidate-123",
                    "file_key_id": "key-reporting-tests-123",
                    "user_id": "user-reporting-tests-123",
                })),
                status_code: 200,
                headers: BTreeMap::from([(
                    "content-type".to_string(),
                    "application/json".to_string(),
                )]),
                body_json: Some(json!({
                    "file": {
                        "name": "fallback123",
                        "displayName": "fallback-image",
                        "mimeType": "image/png"
                    }
                })),
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("gemini files mapping fallback report should stay local");

        let stored = gemini_file_mapping_repository
            .find_by_file_name("files/fallback123")
            .await
            .expect("gemini file mapping should read")
            .expect("gemini file mapping should exist");
        assert_eq!(stored.key_id, "key-reporting-tests-123");
        assert_eq!(stored.user_id.as_deref(), Some("user-reporting-tests-123"));
        assert_eq!(stored.display_name.as_deref(), Some("fallback-image"));
        assert_eq!(stored.mime_type.as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn submit_sync_report_deletes_gemini_file_mapping_locally_on_success() {
        let request_candidate_repository =
            Arc::new(InMemoryRequestCandidateRepository::seed(vec![
                sample_request_candidate(
                    "cand-gemini-files-delete-123",
                    "req-gemini-files-delete-123",
                ),
            ]));
        let gemini_file_mapping_repository = Arc::new(InMemoryGeminiFileMappingRepository::seed([
            aether_data::repository::gemini_file_mappings::StoredGeminiFileMapping::new(
                "mapping-gemini-files-delete-123".to_string(),
                "files/delete-me".to_string(),
                "key-reporting-tests-123".to_string(),
                1_700_000_000,
                1_700_172_800,
            )
            .expect("gemini file mapping should build"),
        ]));
        let state = build_gemini_file_mapping_test_state(
            Arc::clone(&request_candidate_repository),
            Arc::clone(&gemini_file_mapping_repository),
        );

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-gemini-files-delete-123".to_string(),
                report_kind: "gemini_files_delete_mapping".to_string(),
                report_context: Some(json!({
                    "request_id": "req-gemini-files-delete-123",
                    "candidate_id": "cand-gemini-files-delete-123",
                    "candidate_index": 0,
                    "provider_id": "provider-reporting-tests-123",
                    "endpoint_id": "endpoint-reporting-tests-123",
                    "key_id": "key-reporting-tests-123",
                    "file_name": "delete-me",
                })),
                status_code: 204,
                headers: BTreeMap::new(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("gemini files delete mapping report should stay local");

        assert!(gemini_file_mapping_repository
            .find_by_file_name("files/delete-me")
            .await
            .expect("gemini file mapping should read")
            .is_none());
    }

    #[tokio::test]
    async fn submit_sync_report_treats_openai_video_delete_404_success_as_local_success() {
        let repository = Arc::new(InMemoryRequestCandidateRepository::seed(vec![
            sample_request_candidate(
                "cand-reporting-video-delete-123",
                "req-reporting-video-delete-123",
            ),
        ]));
        let state = build_test_state(Arc::clone(&repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-reporting-video-delete-123".to_string(),
                report_kind: "openai_video_delete_sync_success".to_string(),
                report_context: Some(json!({
                    "request_id": "req-reporting-video-delete-123",
                    "provider_id": "provider-reporting-tests-123",
                    "endpoint_id": "endpoint-reporting-tests-123",
                    "key_id": "key-reporting-tests-123",
                })),
                status_code: 404,
                headers: BTreeMap::new(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("video delete sync report should stay local");

        let stored = repository
            .list_by_request_id("req-reporting-video-delete-123")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
        assert_eq!(stored[0].status_code, Some(404));
    }

    #[tokio::test]
    async fn submit_sync_report_handles_local_task_id_only_context_locally_when_video_task_exists()
    {
        let video_repository = Arc::new(InMemoryVideoTaskRepository::default());
        seed_video_task(
            &video_repository,
            "task-openai-video-reporting-123",
            None,
            "req-openai-video-reporting-123",
            "user-openai-video-reporting-123",
            "api-key-openai-video-reporting-123",
            "provider-openai-video-reporting-123",
            "endpoint-openai-video-reporting-123",
            "key-openai-video-reporting-123",
            "openai:video",
            "openai:video",
        )
        .await;
        let request_candidate_repository =
            Arc::new(InMemoryRequestCandidateRepository::seed(vec![
                sample_request_candidate_with_transport(
                    "cand-openai-video-reporting-123",
                    "req-openai-video-reporting-123",
                    "user-openai-video-reporting-123",
                    "api-key-openai-video-reporting-123",
                    "provider-openai-video-reporting-123",
                    "endpoint-openai-video-reporting-123",
                    "key-openai-video-reporting-123",
                ),
            ]));
        let state =
            build_video_test_state(video_repository, Arc::clone(&request_candidate_repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-openai-video-reporting-123".to_string(),
                report_kind: "openai_video_create_sync_success".to_string(),
                report_context: Some(json!({
                    "local_task_id": "task-openai-video-reporting-123"
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("video report should stay local");

        let stored = request_candidate_repository
            .list_by_request_id("req-openai-video-reporting-123")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "cand-openai-video-reporting-123");
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
    }

    #[tokio::test]
    async fn submit_sync_report_handles_local_short_id_only_context_locally_when_video_task_exists()
    {
        let video_repository = Arc::new(InMemoryVideoTaskRepository::default());
        seed_video_task(
            &video_repository,
            "task-gemini-video-reporting-123",
            Some("short-gemini-video-reporting-123"),
            "req-gemini-video-reporting-123",
            "user-gemini-video-reporting-123",
            "api-key-gemini-video-reporting-123",
            "provider-gemini-video-reporting-123",
            "endpoint-gemini-video-reporting-123",
            "key-gemini-video-reporting-123",
            "gemini:video",
            "gemini:video",
        )
        .await;
        let request_candidate_repository =
            Arc::new(InMemoryRequestCandidateRepository::seed(vec![
                sample_request_candidate_with_transport(
                    "cand-gemini-video-reporting-123",
                    "req-gemini-video-reporting-123",
                    "user-gemini-video-reporting-123",
                    "api-key-gemini-video-reporting-123",
                    "provider-gemini-video-reporting-123",
                    "endpoint-gemini-video-reporting-123",
                    "key-gemini-video-reporting-123",
                ),
            ]));
        let state =
            build_video_test_state(video_repository, Arc::clone(&request_candidate_repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-gemini-video-reporting-123".to_string(),
                report_kind: "gemini_video_create_sync_success".to_string(),
                report_context: Some(json!({
                    "local_short_id": "short-gemini-video-reporting-123"
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("video report should stay local");

        let stored = request_candidate_repository
            .list_by_request_id("req-gemini-video-reporting-123")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "cand-gemini-video-reporting-123");
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
    }

    #[tokio::test]
    async fn submit_sync_report_handles_task_id_only_context_locally_when_video_task_id_exists() {
        let video_repository = Arc::new(InMemoryVideoTaskRepository::default());
        seed_video_task(
            &video_repository,
            "task-openai-video-task-id-123",
            None,
            "req-openai-video-task-id-123",
            "user-openai-video-task-id-123",
            "api-key-openai-video-task-id-123",
            "provider-openai-video-task-id-123",
            "endpoint-openai-video-task-id-123",
            "key-openai-video-task-id-123",
            "openai:video",
            "openai:video",
        )
        .await;
        let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state =
            build_video_test_state(video_repository, Arc::clone(&request_candidate_repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-openai-video-task-id-123".to_string(),
                report_kind: "openai_video_cancel_sync_success".to_string(),
                report_context: Some(json!({
                    "task_id": "task-openai-video-task-id-123"
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("video cancel report should stay local");

        let stored = request_candidate_repository
            .list_by_request_id("req-openai-video-task-id-123")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
        assert_eq!(
            stored[0].provider_id.as_deref(),
            Some("provider-openai-video-task-id-123")
        );
        assert_eq!(
            stored[0].endpoint_id.as_deref(),
            Some("endpoint-openai-video-task-id-123")
        );
        assert_eq!(
            stored[0].key_id.as_deref(),
            Some("key-openai-video-task-id-123")
        );
    }

    #[tokio::test]
    async fn submit_sync_report_handles_external_task_id_context_locally_when_video_task_exists() {
        let video_repository = Arc::new(InMemoryVideoTaskRepository::default());
        seed_video_task(
            &video_repository,
            "task-gemini-video-external-id-123",
            Some("short-gemini-video-external-id-123"),
            "req-gemini-video-external-id-123",
            "user-gemini-video-external-id-123",
            "api-key-gemini-video-external-id-123",
            "provider-gemini-video-external-id-123",
            "endpoint-gemini-video-external-id-123",
            "key-gemini-video-external-id-123",
            "gemini:video",
            "gemini:video",
        )
        .await;
        video_repository
            .upsert(UpsertVideoTask {
                id: "task-gemini-video-external-id-123".to_string(),
                short_id: Some("short-gemini-video-external-id-123".to_string()),
                request_id: "req-gemini-video-external-id-123".to_string(),
                user_id: Some("user-gemini-video-external-id-123".to_string()),
                api_key_id: Some("api-key-gemini-video-external-id-123".to_string()),
                username: Some("video-user".to_string()),
                api_key_name: Some("video-key".to_string()),
                external_task_id: Some("models/veo-3/operations/ext-gemini-video-123".to_string()),
                provider_id: Some("provider-gemini-video-external-id-123".to_string()),
                endpoint_id: Some("endpoint-gemini-video-external-id-123".to_string()),
                key_id: Some("key-gemini-video-external-id-123".to_string()),
                client_api_format: Some("gemini:video".to_string()),
                provider_api_format: Some("gemini:video".to_string()),
                format_converted: false,
                model: Some("video-model".to_string()),
                prompt: Some("video prompt".to_string()),
                original_request_body: Some(json!({"prompt": "video prompt"})),
                duration_seconds: Some(4),
                resolution: Some("720p".to_string()),
                aspect_ratio: Some("16:9".to_string()),
                size: Some("1280x720".to_string()),
                status: VideoTaskStatus::Submitted,
                progress_percent: 0,
                progress_message: None,
                retry_count: 0,
                poll_interval_seconds: 10,
                next_poll_at_unix_secs: Some(1_700_000_010),
                poll_count: 0,
                max_poll_count: 360,
                created_at_unix_ms: 1_700_000_000,
                submitted_at_unix_secs: Some(1_700_000_000),
                completed_at_unix_secs: None,
                updated_at_unix_secs: 1_700_000_000,
                error_code: None,
                error_message: None,
                video_url: None,
                request_metadata: None,
            })
            .await
            .expect("video task should update external id");
        let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state =
            build_video_test_state(video_repository, Arc::clone(&request_candidate_repository));

        submit_sync_report(
            &state,
            GatewaySyncReportRequest {
                trace_id: "trace-gemini-video-external-id-123".to_string(),
                report_kind: "gemini_video_cancel_sync_success".to_string(),
                report_context: Some(json!({
                    "user_id": "user-gemini-video-external-id-123",
                    "task_id": "models/veo-3/operations/ext-gemini-video-123"
                })),
                status_code: 200,
                headers: BTreeMap::new(),
                body_json: None,
                client_body_json: None,
                body_base64: None,
                telemetry: None,
            },
        )
        .await
        .expect("gemini video cancel report should stay local");

        let stored = request_candidate_repository
            .list_by_request_id("req-gemini-video-external-id-123")
            .await
            .expect("request candidates should list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
        assert_eq!(
            stored[0].provider_id.as_deref(),
            Some("provider-gemini-video-external-id-123")
        );
        assert_eq!(
            stored[0].endpoint_id.as_deref(),
            Some("endpoint-gemini-video-external-id-123")
        );
        assert_eq!(
            stored[0].key_id.as_deref(),
            Some("key-gemini-video-external-id-123")
        );
    }

    #[test]
    fn builds_niffler_route_attempt_shadow_record_from_mapped_model() {
        let decision = enabled_rollout_decision("api-key-shadow-123");
        let context = json!({
            "request_id": "req-shadow-123",
            "api_key_id": "api-key-shadow-123",
            "provider_id": "provider-shadow-123",
            "key_id": "provider-key-shadow-123",
            "candidate_index": 2,
            "mapped_model": "gpt-5.5-upstream",
            "model": "gpt-5.5"
        });

        let record = build_niffler_route_attempt_shadow_record(
            Some(&context),
            &decision,
            RequestCandidateStatus::Success,
            Some(200),
            Some(1234),
            1_700_000_000_000,
        )
        .expect("shadow record should build");

        assert_eq!(record.request_id, "req-shadow-123");
        assert_eq!(
            record.upstream_service_id.as_deref(),
            Some("provider-shadow-123")
        );
        assert_eq!(
            record.upstream_account_id.as_deref(),
            Some("provider-key-shadow-123")
        );
        assert_eq!(
            record.product_plan_id.as_deref(),
            Some("product-plan-reporting-tests-123")
        );
        assert_eq!(record.model_name, "gpt-5.5-upstream");
        assert_eq!(record.attempt_index, 2);
        assert_eq!(record.status, "success");
        assert_eq!(record.upstream_status_code, Some(200));
        assert_eq!(record.latency_ms, Some(1234));
    }

    #[test]
    fn niffler_route_attempt_shadow_record_uses_stable_id() {
        let decision = enabled_rollout_decision("api-key-shadow-stable-123");
        let context = json!({
            "request_id": "req-shadow-stable-123",
            "api_key_id": "api-key-shadow-stable-123",
            "provider_id": "provider-shadow-stable-123",
            "key_id": "provider-key-shadow-stable-123",
            "candidate_index": 1,
            "model": "gpt-5.4-mini"
        });

        let first = build_niffler_route_attempt_shadow_record(
            Some(&context),
            &decision,
            RequestCandidateStatus::Failed,
            Some(503),
            None,
            1_700_000_000_000,
        )
        .expect("first shadow record should build");
        let second = build_niffler_route_attempt_shadow_record(
            Some(&context),
            &decision,
            RequestCandidateStatus::Failed,
            Some(503),
            None,
            1_700_000_000_999,
        )
        .expect("second shadow record should build");

        assert_eq!(first.id, second.id);
        assert_eq!(first.status, "failed");
    }

    #[test]
    fn skips_niffler_route_attempt_shadow_record_without_model_name() {
        let decision = enabled_rollout_decision("api-key-shadow-no-model-123");
        let context = json!({
            "request_id": "req-shadow-no-model-123",
            "api_key_id": "api-key-shadow-no-model-123",
            "provider_id": "provider-shadow-no-model-123",
            "key_id": "provider-key-shadow-no-model-123",
            "candidate_index": 0
        });

        let record = build_niffler_route_attempt_shadow_record(
            Some(&context),
            &decision,
            RequestCandidateStatus::Success,
            Some(200),
            None,
            1_700_000_000_000,
        );

        assert!(record.is_none());
    }

    #[test]
    fn builds_niffler_settlement_snapshot_shadow_record_from_existing_snapshot() {
        let mut decision = enabled_rollout_decision("api-key-settlement-shadow-123");
        decision.enable_settlement_snapshot = true;
        let context = json!({
            "request_id": "req-settlement-shadow-123",
            "user_id": "user-settlement-shadow-123",
            "api_key_id": "api-key-settlement-shadow-123",
            "provider_id": "provider-settlement-shadow-123",
            "key_id": "provider-key-settlement-shadow-123",
            "model": "gpt-5.5",
            "mapped_model": "gpt-5.5-upstream",
            "settlement_snapshot": {
                "schema_version": "3.0",
                "pricing_snapshot": {
                    "provider_id": "provider-settlement-shadow-123",
                    "provider_api_key_id": "provider-key-settlement-shadow-123",
                    "global_model_name": "gpt-5.5",
                    "provider_model_name": "gpt-5.5-upstream",
                    "sales_multiplier": 1.2
                },
                "base_cost_usd": 0.2,
                "user_total_cost_usd": 0.24,
                "total_cost": 0.24,
                "actual_total_cost": 0.08
            },
            "charge_breakdown_snapshot": {
                "wallet_debit_usd": 0.14,
                "package_debit_usd": 0.1
            }
        });

        let record = build_niffler_settlement_snapshot_shadow_record(
            Some(&context),
            &decision,
            1_700_000_000_000,
        )
        .expect("settlement snapshot record should build");

        assert_eq!(record.request_id, "req-settlement-shadow-123");
        assert_eq!(
            record.user_id.as_deref(),
            Some("user-settlement-shadow-123")
        );
        assert_eq!(
            record.api_key_id.as_deref(),
            Some("api-key-settlement-shadow-123")
        );
        assert_eq!(
            record.product_plan_id.as_deref(),
            Some("product-plan-reporting-tests-123")
        );
        assert_eq!(
            record.upstream_service_id.as_deref(),
            Some("provider-settlement-shadow-123")
        );
        assert_eq!(
            record.upstream_account_id.as_deref(),
            Some("provider-key-settlement-shadow-123")
        );
        assert_eq!(record.requested_model_name, "gpt-5.5");
        assert_eq!(
            record.upstream_execution_model_name.as_deref(),
            Some("gpt-5.5-upstream")
        );
        assert_eq!(record.wallet_charge_usd, 0.14);
        assert_eq!(record.entitlement_charge_usd, 0.1);
        assert_eq!(record.upstream_cost_usd, 0.08);
        assert!((record.gross_margin_usd - 0.16).abs() < f64::EPSILON);
        assert_eq!(record.finalized_at_unix_ms, Some(1_700_000_000_000));
    }

    #[test]
    fn skips_niffler_settlement_snapshot_shadow_record_without_charge_breakdown() {
        let mut decision = enabled_rollout_decision("api-key-settlement-no-charge-123");
        decision.enable_settlement_snapshot = true;
        let context = json!({
            "request_id": "req-settlement-no-charge-123",
            "api_key_id": "api-key-settlement-no-charge-123",
            "model": "gpt-5.5",
            "settlement_snapshot": {
                "pricing_snapshot": {
                    "global_model_name": "gpt-5.5"
                },
                "actual_total_cost": 0.08,
                "total_cost": 0.24
            }
        });

        let record = build_niffler_settlement_snapshot_shadow_record(
            Some(&context),
            &decision,
            1_700_000_000_000,
        );

        assert!(record.is_none());
    }

    #[test]
    fn builds_niffler_billing_reservation_dry_run_matched_record() {
        let mut decision = enabled_rollout_decision("api-key-reservation-dry-run-123");
        decision.enable_billing_reservation = true;
        let context = json!({
            "request_id": "req-reservation-dry-run-123",
            "user_id": "user-reservation-dry-run-123",
            "api_key_id": "api-key-reservation-dry-run-123",
            "model": "gpt-5.5",
            "mapped_model": "gpt-5.5-upstream",
            "settlement_snapshot": {
                "pricing_snapshot": {
                    "global_model_name": "gpt-5.5",
                    "provider_model_name": "gpt-5.5-upstream"
                },
                "user_total_cost_usd": 0.24,
                "total_cost": 0.24
            },
            "charge_breakdown_snapshot": {
                "wallet_debit_usd": 0.14,
                "package_debit_usd": 0.1
            }
        });

        let record = build_niffler_billing_reservation_dry_run_shadow_record(
            Some(&context),
            &decision,
            1_700_000_000_000,
        )
        .expect("billing reservation dry run should build");

        assert_eq!(record.request_id, "req-reservation-dry-run-123");
        assert_eq!(
            record.user_id.as_deref(),
            Some("user-reservation-dry-run-123")
        );
        assert_eq!(
            record.api_key_id.as_deref(),
            Some("api-key-reservation-dry-run-123")
        );
        assert_eq!(
            record.product_plan_id.as_deref(),
            Some("product-plan-reporting-tests-123")
        );
        assert_eq!(record.requested_model_name, "gpt-5.5");
        assert_eq!(record.estimated_reservation_usd, 0.24);
        assert!((record.legacy_final_charge_usd - 0.24).abs() < f64::EPSILON);
        assert_eq!(record.status, "matched");
        assert_eq!(record.estimation_source, "settlement_snapshot_total_cost");
        assert_eq!(record.finalized_at_unix_ms, Some(1_700_000_000_000));
    }

    #[test]
    fn builds_niffler_billing_reservation_dry_run_over_reserved_record() {
        let mut decision = enabled_rollout_decision("api-key-reservation-over-123");
        decision.enable_billing_reservation = true;
        let context = json!({
            "request_id": "req-reservation-over-123",
            "api_key_id": "api-key-reservation-over-123",
            "model": "gpt-5.5",
            "settlement_snapshot": {
                "pricing_snapshot": {
                    "global_model_name": "gpt-5.5"
                },
                "total_cost": 0.3
            },
            "charge_breakdown_snapshot": {
                "wallet_debit_usd": 0.2,
                "package_debit_usd": 0.05
            }
        });

        let record = build_niffler_billing_reservation_dry_run_shadow_record(
            Some(&context),
            &decision,
            1_700_000_000_000,
        )
        .expect("billing reservation dry run should build");

        assert_eq!(record.status, "over_reserved");
        assert!((record.difference_usd - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn builds_niffler_billing_reservation_dry_run_under_reserved_record() {
        let mut decision = enabled_rollout_decision("api-key-reservation-under-123");
        decision.enable_billing_reservation = true;
        let context = json!({
            "request_id": "req-reservation-under-123",
            "api_key_id": "api-key-reservation-under-123",
            "model": "gpt-5.5",
            "settlement_snapshot": {
                "pricing_snapshot": {
                    "global_model_name": "gpt-5.5"
                },
                "total_cost": 0.2
            },
            "charge_breakdown_snapshot": {
                "wallet_debit_usd": 0.22,
                "package_debit_usd": 0.05
            }
        });

        let record = build_niffler_billing_reservation_dry_run_shadow_record(
            Some(&context),
            &decision,
            1_700_000_000_000,
        )
        .expect("billing reservation dry run should build");

        assert_eq!(record.status, "under_reserved");
        assert!((record.difference_usd + 0.07).abs() < f64::EPSILON);
    }

    #[test]
    fn skips_niffler_billing_reservation_dry_run_without_legacy_charge() {
        let mut decision = enabled_rollout_decision("api-key-reservation-no-charge-123");
        decision.enable_billing_reservation = true;
        let context = json!({
            "request_id": "req-reservation-no-charge-123",
            "api_key_id": "api-key-reservation-no-charge-123",
            "model": "gpt-5.5",
            "settlement_snapshot": {
                "pricing_snapshot": {
                    "global_model_name": "gpt-5.5"
                },
                "total_cost": 0.24
            },
            "charge_breakdown_snapshot": {}
        });

        let record = build_niffler_billing_reservation_dry_run_shadow_record(
            Some(&context),
            &decision,
            1_700_000_000_000,
        );

        assert!(record.is_none());
    }

    #[tokio::test]
    async fn finalizes_niffler_billing_reservations_by_request_outcome() {
        let state = sqlite_niffler_state().await;
        for request_id in [
            "req-reservation-finalize-success-123",
            "req-reservation-finalize-failed-123",
            "req-reservation-finalize-manual-review-123",
        ] {
            state
                .create_niffler_billing_reservation(billing_reservation_record(request_id))
                .await
                .expect("billing reservation should create");
        }

        let mut decision = enabled_rollout_decision("api-key-finalize-tests-123");
        decision.source = Some(NifflerRuntimeRolloutDecisionSource::ApiKey);
        decision.enable_billing_reservation = true;

        let success_context = json!({
            "request_id": "req-reservation-finalize-success-123",
            "user_id": "user-finalize-tests-123",
            "api_key_id": "api-key-finalize-tests-123",
            "provider_id": "provider-finalize-tests-123",
            "key_id": "provider-key-finalize-tests-123",
            "model": "gpt-5.5",
            "mapped_model": "gpt-5.5-upstream",
            "settlement_snapshot": {
                "pricing_snapshot": {
                    "provider_id": "provider-finalize-tests-123",
                    "provider_api_key_id": "provider-key-finalize-tests-123",
                    "global_model_name": "gpt-5.5",
                    "provider_model_name": "gpt-5.5-upstream"
                },
                "user_total_cost_usd": 0.24,
                "total_cost": 0.24,
                "actual_total_cost": 0.08
            },
            "charge_breakdown_snapshot": {
                "wallet_debit_usd": 0.14,
                "package_debit_usd": 0.1
            }
        });
        finalize_niffler_billing_reservation(
            &state,
            &success_context,
            &decision,
            RequestCandidateStatus::Success,
            Some(200),
            1_700_000_001_000,
        )
        .await;

        let failed_context = json!({
            "request_id": "req-reservation-finalize-failed-123",
            "user_id": "user-finalize-tests-123",
            "api_key_id": "api-key-finalize-tests-123"
        });
        finalize_niffler_billing_reservation(
            &state,
            &failed_context,
            &decision,
            RequestCandidateStatus::Failed,
            Some(429),
            1_700_000_002_000,
        )
        .await;

        let missing_snapshot_context = json!({
            "request_id": "req-reservation-finalize-manual-review-123",
            "user_id": "user-finalize-tests-123",
            "api_key_id": "api-key-finalize-tests-123"
        });
        finalize_niffler_billing_reservation(
            &state,
            &missing_snapshot_context,
            &decision,
            RequestCandidateStatus::Success,
            Some(200),
            1_700_000_003_000,
        )
        .await;

        let success = state
            .list_niffler_billing_reservations(&NifflerBillingReservationListQuery {
                status: None,
                user_id: None,
                api_key_id: None,
                request_id: Some("req-reservation-finalize-success-123".to_string()),
                expires_at_gte_unix_ms: None,
                expires_at_lte_unix_ms: None,
                expires_at_lt_unix_ms: None,
                finalized_at_gte_unix_ms: None,
                finalized_at_lt_unix_ms: None,
                offset: 0,
                limit: 1,
            })
            .await
            .expect("success reservation should read");
        assert_eq!(
            success.items[0].status,
            NifflerBillingReservationStatus::Settled
        );
        assert!(success.items[0].settlement_snapshot_id.is_some());

        let snapshots = state
            .list_niffler_settlement_snapshots(&NifflerSettlementSnapshotListQuery {
                request_id: Some("req-reservation-finalize-success-123".to_string()),
                user_id: None,
                api_key_id: None,
                product_plan_id: None,
                offset: 0,
                limit: 1,
            })
            .await
            .expect("settlement snapshot should read");
        assert_eq!(snapshots.items.len(), 1);

        let failed = state
            .list_niffler_billing_reservations(&NifflerBillingReservationListQuery {
                status: None,
                user_id: None,
                api_key_id: None,
                request_id: Some("req-reservation-finalize-failed-123".to_string()),
                expires_at_gte_unix_ms: None,
                expires_at_lte_unix_ms: None,
                expires_at_lt_unix_ms: None,
                finalized_at_gte_unix_ms: None,
                finalized_at_lt_unix_ms: None,
                offset: 0,
                limit: 1,
            })
            .await
            .expect("failed reservation should read");
        assert_eq!(
            failed.items[0].status,
            NifflerBillingReservationStatus::Released
        );
        assert_eq!(
            failed.items[0].release_reason.as_deref(),
            Some("failed:upstream_status_429")
        );

        let manual_review = state
            .list_niffler_billing_reservations(&NifflerBillingReservationListQuery {
                status: None,
                user_id: None,
                api_key_id: None,
                request_id: Some("req-reservation-finalize-manual-review-123".to_string()),
                expires_at_gte_unix_ms: None,
                expires_at_lte_unix_ms: None,
                expires_at_lt_unix_ms: None,
                finalized_at_gte_unix_ms: None,
                finalized_at_lt_unix_ms: None,
                offset: 0,
                limit: 1,
            })
            .await
            .expect("manual review reservation should read");
        assert_eq!(
            manual_review.items[0].status,
            NifflerBillingReservationStatus::ManualReview
        );
        assert_eq!(
            manual_review.items[0].release_reason.as_deref(),
            Some("missing_settlement_snapshot")
        );
    }
}

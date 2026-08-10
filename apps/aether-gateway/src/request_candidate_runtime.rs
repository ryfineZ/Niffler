use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use aether_contracts::ExecutionPlan;
use aether_data_contracts::repository::billing::BillingRequestAdmissionInput;
use aether_data_contracts::repository::candidates::{
    RequestCandidateStatus, StoredRequestCandidate, UpsertRequestCandidateRecord,
};
use aether_scheduler_core::{
    build_execution_request_candidate_seed, build_local_request_candidate_status_record,
    build_report_request_candidate_status_record,
    finalize_execution_request_candidate_report_context, parse_request_candidate_report_context,
    resolve_report_request_candidate_slot as resolve_report_request_candidate_slot_from_candidates,
    LocalRequestCandidateStatusRecordInput, ReportRequestCandidateStatusRecordInput,
    SchedulerMinimalCandidateSelectionCandidate, SchedulerRequestCandidateStatusUpdate,
    SchedulerResolvedReportRequestCandidateSlot,
};
use aether_usage_runtime::build_locally_actionable_report_context_from_request_candidate;
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, Notify};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::clock::current_unix_ms;
use crate::log_ids::short_request_id;
use crate::GatewayError;

const REQUEST_CANDIDATE_STATUS_WRITE_QUEUE_CAPACITY: usize = 4096;
const REQUEST_CANDIDATE_STATUS_BACKGROUND_RUNTIME_THREADS: usize = 1;
const REQUEST_CANDIDATE_STATUS_BACKGROUND_RUNTIME_STACK_BYTES: usize = 4 * 1024 * 1024;
const REQUEST_CANDIDATE_STATUS_BACKGROUND_RUNTIME_THREAD_NAME: &str =
    "aether-request-candidate-writer";

#[derive(Debug, Clone)]
pub(crate) struct LocalRequestCandidateStatusSnapshot {
    candidate_id: String,
    request_id: String,
    user_id: Option<String>,
    api_key_id: Option<String>,
    candidate_index: u32,
    retry_index: u32,
    provider_id: String,
    endpoint_id: String,
    key_id: String,
}

#[async_trait]
pub(crate) trait RequestCandidateRuntimeReader {
    async fn read_request_candidates_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredRequestCandidate>, GatewayError>;
}

#[async_trait]
pub(crate) trait RequestCandidateRuntimeWriter: Send + Sync {
    fn has_request_candidate_data_writer(&self) -> bool;

    fn request_candidate_status_write_queue(
        &self,
    ) -> Option<Arc<RequestCandidateStatusWriteQueue>> {
        None
    }

    fn clone_request_candidate_writer(&self) -> Option<Arc<dyn RequestCandidateRuntimeWriter>> {
        None
    }

    async fn upsert_request_candidate(
        &self,
        candidate: UpsertRequestCandidateRecord,
    ) -> Result<Option<StoredRequestCandidate>, GatewayError>;

    async fn resolve_request_billing_admission(
        &self,
        _candidate: &UpsertRequestCandidateRecord,
        _report_context: Option<&Value>,
    ) -> Result<Option<BillingRequestAdmissionInput>, GatewayError> {
        Ok(None)
    }

    async fn upsert_request_candidate_with_billing_admission(
        &self,
        candidate: UpsertRequestCandidateRecord,
        _admission: BillingRequestAdmissionInput,
    ) -> Result<Option<StoredRequestCandidate>, GatewayError> {
        self.upsert_request_candidate(candidate).await
    }
}

struct RequestCandidateStatusWriteJob {
    writer: Arc<dyn RequestCandidateRuntimeWriter>,
    record: UpsertRequestCandidateRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestCandidateStatusWriteQueueError {
    Closed,
}

impl std::fmt::Display for RequestCandidateStatusWriteQueueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => formatter.write_str("request candidate status write queue is closed"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RequestCandidateStatusWriteQueue {
    sender: mpsc::Sender<RequestCandidateStatusWriteJob>,
    pending: Arc<AtomicUsize>,
    pending_drained: Arc<Notify>,
}

impl RequestCandidateStatusWriteQueue {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::channel(REQUEST_CANDIDATE_STATUS_WRITE_QUEUE_CAPACITY);
        let pending = Arc::new(AtomicUsize::new(0));
        let pending_drained = Arc::new(Notify::new());
        spawn_on_request_candidate_status_background_runtime(
            run_request_candidate_status_write_worker(
                receiver,
                Arc::clone(&pending),
                Arc::clone(&pending_drained),
            ),
        );
        Self {
            sender,
            pending,
            pending_drained,
        }
    }

    pub(crate) async fn submit_record(
        &self,
        writer: Arc<dyn RequestCandidateRuntimeWriter>,
        record: UpsertRequestCandidateRecord,
    ) -> Result<
        (),
        (
            RequestCandidateStatusWriteQueueError,
            UpsertRequestCandidateRecord,
        ),
    > {
        self.pending.fetch_add(1, Ordering::AcqRel);
        match self
            .sender
            .send(RequestCandidateStatusWriteJob { writer, record })
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => {
                if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
                    self.pending_drained.notify_waiters();
                }
                Err((
                    RequestCandidateStatusWriteQueueError::Closed,
                    error.0.record,
                ))
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn flush(&self) {
        loop {
            let notified = self.pending_drained.notified();
            if self.pending.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
}

async fn run_request_candidate_status_write_worker(
    mut receiver: mpsc::Receiver<RequestCandidateStatusWriteJob>,
    pending: Arc<AtomicUsize>,
    pending_drained: Arc<Notify>,
) {
    while let Some(job) = receiver.recv().await {
        persist_local_request_candidate_status_record(job.writer.as_ref(), job.record).await;
        if pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            pending_drained.notify_waiters();
        }
    }
}

fn spawn_on_request_candidate_status_background_runtime<F>(
    task: F,
) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    request_candidate_status_background_runtime()
        .handle()
        .spawn(task)
}

fn request_candidate_status_background_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<&'static tokio::runtime::Runtime> = OnceLock::new();

    RUNTIME.get_or_init(|| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(REQUEST_CANDIDATE_STATUS_BACKGROUND_RUNTIME_THREADS)
            .thread_name(REQUEST_CANDIDATE_STATUS_BACKGROUND_RUNTIME_THREAD_NAME)
            .thread_stack_size(REQUEST_CANDIDATE_STATUS_BACKGROUND_RUNTIME_STACK_BYTES)
            .build()
            .expect("request candidate status background runtime should build");
        Box::leak(Box::new(runtime))
    })
}

#[async_trait]
pub(crate) trait RequestCandidateRuntimeCapabilityReader {
    async fn read_request_candidate_user_model_capability_settings(
        &self,
        user_id: &str,
    ) -> Result<Option<Value>, GatewayError>;

    async fn read_request_candidate_api_key_force_capabilities(
        &self,
        user_id: &str,
        api_key_id: &str,
    ) -> Result<Option<Value>, GatewayError>;
}

pub(crate) async fn resolve_request_candidate_required_capabilities(
    state: &(impl RequestCandidateRuntimeCapabilityReader + ?Sized),
    user_id: &str,
    api_key_id: &str,
    requested_model: Option<&str>,
    explicit_required_capabilities: Option<&Value>,
    enable_model_directives: bool,
) -> Option<Value> {
    let mut merged = serde_json::Map::new();

    match state
        .read_request_candidate_user_model_capability_settings(user_id)
        .await
    {
        Ok(settings) => merge_capability_object(
            &mut merged,
            select_requested_model_capabilities(
                settings.as_ref(),
                requested_model,
                enable_model_directives,
            ),
        ),
        Err(error) => {
            warn!(
                user_id = %user_id,
                api_key_id = %api_key_id,
                requested_model = requested_model.unwrap_or_default(),
                error = ?error,
                "gateway request candidate user model capabilities lookup failed"
            );
        }
    }

    match state
        .read_request_candidate_api_key_force_capabilities(user_id, api_key_id)
        .await
    {
        Ok(force_capabilities) => {
            merge_capability_object(&mut merged, force_capabilities.as_ref());
        }
        Err(error) => {
            warn!(
                user_id = %user_id,
                api_key_id = %api_key_id,
                requested_model = requested_model.unwrap_or_default(),
                error = ?error,
                "gateway request candidate api key capabilities lookup failed"
            );
        }
    }

    merge_capability_object(&mut merged, explicit_required_capabilities);

    (!merged.is_empty()).then_some(Value::Object(merged))
}

fn merge_capability_object(target: &mut serde_json::Map<String, Value>, source: Option<&Value>) {
    let Some(source) = source.and_then(Value::as_object) else {
        return;
    };

    for (capability, value) in source {
        if capability.trim().is_empty() {
            continue;
        }
        target.insert(capability.clone(), value.clone());
    }
}

fn select_requested_model_capabilities<'a>(
    settings: Option<&'a Value>,
    requested_model: Option<&str>,
    enable_model_directives: bool,
) -> Option<&'a Value> {
    let requested_model = requested_model
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let settings = settings?.as_object()?;

    find_model_capabilities(settings, requested_model).or_else(|| {
        enable_model_directives
            .then(|| crate::ai_serving::model_directive_base_model(requested_model))
            .flatten()
            .as_deref()
            .and_then(|base_model| find_model_capabilities(settings, base_model))
    })
}

fn find_model_capabilities<'a>(
    settings: &'a serde_json::Map<String, Value>,
    requested_model: &str,
) -> Option<&'a Value> {
    settings.get(requested_model).or_else(|| {
        settings.iter().find_map(|(model_name, capabilities)| {
            model_name
                .trim()
                .eq_ignore_ascii_case(requested_model)
                .then_some(capabilities)
        })
    })
}

fn request_candidate_status_label(status: RequestCandidateStatus) -> &'static str {
    match status {
        RequestCandidateStatus::Available => "available",
        RequestCandidateStatus::Unused => "unused",
        RequestCandidateStatus::Pending => "pending",
        RequestCandidateStatus::Streaming => "streaming",
        RequestCandidateStatus::Success => "success",
        RequestCandidateStatus::Failed => "failed",
        RequestCandidateStatus::Cancelled => "cancelled",
        RequestCandidateStatus::Skipped => "skipped",
    }
}

pub(crate) fn snapshot_local_request_candidate_status(
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
) -> Option<LocalRequestCandidateStatusSnapshot> {
    let candidate_id = plan
        .candidate_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let metadata = parse_request_candidate_report_context(report_context)?;
    let candidate_index = metadata.candidate_index?;

    Some(LocalRequestCandidateStatusSnapshot {
        candidate_id: candidate_id.to_string(),
        request_id: plan.request_id.clone(),
        user_id: metadata.user_id,
        api_key_id: metadata.api_key_id,
        candidate_index,
        retry_index: metadata.retry_index,
        provider_id: plan.provider_id.clone(),
        endpoint_id: plan.endpoint_id.clone(),
        key_id: plan.key_id.clone(),
    })
}

async fn persist_local_request_candidate_status_record(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    record: UpsertRequestCandidateRecord,
) {
    let candidate_id = record.id.clone();
    let request_id = short_request_id(record.request_id.as_str());
    let candidate_index = record.candidate_index;
    let retry_index = record.retry_index;
    let status = record.status;

    match state.upsert_request_candidate(record).await {
        Ok(Some(stored)) => {
            debug!(
                event_name = "request_candidate_status_persisted",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %stored.id,
                candidate_index,
                retry_index,
                status = request_candidate_status_label(status),
                source = "local_status",
                "gateway persisted request candidate status update"
            );
        }
        Ok(None) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = request_candidate_status_label(status),
                source = "local_status",
                "gateway skipped request candidate persistence because writer is unavailable"
            );
        }
        Err(err) => {
            warn!(
                event_name = "request_candidate_status_persist_failed",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %candidate_id,
                error = ?err,
                "gateway failed to persist request candidate status update"
            );
        }
    }
}

async fn submit_local_request_candidate_status_record(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    record: UpsertRequestCandidateRecord,
) -> Result<(), UpsertRequestCandidateRecord> {
    let Some(queue) = state.request_candidate_status_write_queue() else {
        return Err(record);
    };
    let Some(writer) = state.clone_request_candidate_writer() else {
        return Err(record);
    };

    match queue.submit_record(writer, record).await {
        Ok(()) => {
            if cfg!(test) {
                queue.flush().await;
            }
            Ok(())
        }
        Err((err, record)) => {
            warn!(
                event_name = "request_candidate_status_queue_submit_failed",
                log_type = "event",
                error = %err,
                "gateway request candidate status queue rejected update; writing inline"
            );
            Err(record)
        }
    }
}

pub(crate) async fn record_local_request_candidate_status(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    let Some(record) =
        build_local_request_candidate_status_record(LocalRequestCandidateStatusRecordInput {
            plan,
            report_context,
            status_update,
        })
    else {
        return;
    };
    persist_local_request_candidate_status_record(state, record).await;
}

pub(crate) async fn submit_local_request_candidate_status(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    let Some(record) =
        build_local_request_candidate_status_record(LocalRequestCandidateStatusRecordInput {
            plan,
            report_context,
            status_update,
        })
    else {
        return;
    };
    if let Err(record) = submit_local_request_candidate_status_record(state, record).await {
        persist_local_request_candidate_status_record(state, record).await;
    }
}

pub(crate) async fn record_local_request_candidate_extra_data(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    status: RequestCandidateStatus,
    status_code: Option<u16>,
    latency_ms: Option<u64>,
    extra_data: Value,
) {
    let Some(snapshot) = snapshot_local_request_candidate_status(plan, report_context) else {
        return;
    };
    let record = UpsertRequestCandidateRecord {
        id: snapshot.candidate_id.clone(),
        request_id: snapshot.request_id.clone(),
        user_id: snapshot.user_id.clone(),
        api_key_id: snapshot.api_key_id.clone(),
        username: None,
        api_key_name: None,
        candidate_index: snapshot.candidate_index,
        retry_index: snapshot.retry_index,
        provider_id: Some(snapshot.provider_id.clone()),
        endpoint_id: Some(snapshot.endpoint_id.clone()),
        key_id: Some(snapshot.key_id.clone()),
        status,
        skip_reason: None,
        is_cached: None,
        status_code,
        error_type: None,
        error_message: None,
        latency_ms,
        concurrent_requests: None,
        extra_data: Some(extra_data),
        required_capabilities: None,
        created_at_unix_ms: None,
        started_at_unix_ms: None,
        finished_at_unix_ms: None,
    };
    persist_local_request_candidate_status_record(state, record).await;
}

pub(crate) async fn submit_local_request_candidate_extra_data(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    status: RequestCandidateStatus,
    status_code: Option<u16>,
    latency_ms: Option<u64>,
    extra_data: Value,
) {
    let Some(snapshot) = snapshot_local_request_candidate_status(plan, report_context) else {
        return;
    };
    let record = UpsertRequestCandidateRecord {
        id: snapshot.candidate_id.clone(),
        request_id: snapshot.request_id.clone(),
        user_id: snapshot.user_id.clone(),
        api_key_id: snapshot.api_key_id.clone(),
        username: None,
        api_key_name: None,
        candidate_index: snapshot.candidate_index,
        retry_index: snapshot.retry_index,
        provider_id: Some(snapshot.provider_id.clone()),
        endpoint_id: Some(snapshot.endpoint_id.clone()),
        key_id: Some(snapshot.key_id.clone()),
        status,
        skip_reason: None,
        is_cached: None,
        status_code,
        error_type: None,
        error_message: None,
        latency_ms,
        concurrent_requests: None,
        extra_data: Some(extra_data),
        required_capabilities: None,
        created_at_unix_ms: None,
        started_at_unix_ms: None,
        finished_at_unix_ms: None,
    };
    if let Err(record) = submit_local_request_candidate_status_record(state, record).await {
        persist_local_request_candidate_status_record(state, record).await;
    }
}

pub(crate) async fn record_local_request_candidate_status_snapshot(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    snapshot: &LocalRequestCandidateStatusSnapshot,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    let SchedulerRequestCandidateStatusUpdate {
        status,
        status_code,
        error_type,
        error_message,
        latency_ms,
        started_at_unix_ms,
        finished_at_unix_ms,
    } = status_update;
    let record = UpsertRequestCandidateRecord {
        id: snapshot.candidate_id.clone(),
        request_id: snapshot.request_id.clone(),
        user_id: snapshot.user_id.clone(),
        api_key_id: snapshot.api_key_id.clone(),
        username: None,
        api_key_name: None,
        candidate_index: snapshot.candidate_index,
        retry_index: snapshot.retry_index,
        provider_id: Some(snapshot.provider_id.clone()),
        endpoint_id: Some(snapshot.endpoint_id.clone()),
        key_id: Some(snapshot.key_id.clone()),
        status,
        skip_reason: None,
        is_cached: None,
        status_code,
        error_type,
        error_message,
        latency_ms,
        concurrent_requests: None,
        extra_data: None,
        required_capabilities: None,
        created_at_unix_ms: None,
        started_at_unix_ms,
        finished_at_unix_ms,
    };
    persist_local_request_candidate_status_record(state, record).await;
}

pub(crate) async fn submit_local_request_candidate_status_snapshot(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    snapshot: &LocalRequestCandidateStatusSnapshot,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    let SchedulerRequestCandidateStatusUpdate {
        status,
        status_code,
        error_type,
        error_message,
        latency_ms,
        started_at_unix_ms,
        finished_at_unix_ms,
    } = status_update;
    let record = UpsertRequestCandidateRecord {
        id: snapshot.candidate_id.clone(),
        request_id: snapshot.request_id.clone(),
        user_id: snapshot.user_id.clone(),
        api_key_id: snapshot.api_key_id.clone(),
        username: None,
        api_key_name: None,
        candidate_index: snapshot.candidate_index,
        retry_index: snapshot.retry_index,
        provider_id: Some(snapshot.provider_id.clone()),
        endpoint_id: Some(snapshot.endpoint_id.clone()),
        key_id: Some(snapshot.key_id.clone()),
        status,
        skip_reason: None,
        is_cached: None,
        status_code,
        error_type,
        error_message,
        latency_ms,
        concurrent_requests: None,
        extra_data: None,
        required_capabilities: None,
        created_at_unix_ms: None,
        started_at_unix_ms,
        finished_at_unix_ms,
    };
    if let Err(record) = submit_local_request_candidate_status_record(state, record).await {
        persist_local_request_candidate_status_record(state, record).await;
    }
}

pub(crate) async fn record_report_request_candidate_status(
    state: &(impl RequestCandidateRuntimeReader + RequestCandidateRuntimeWriter + ?Sized),
    report_context: Option<&Value>,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    let Some(slot) = resolve_report_request_candidate_slot(state, report_context).await else {
        return;
    };
    let request_id = slot.request_id.clone();
    let request_id_for_log = short_request_id(request_id.as_str());
    let candidate_index = slot.candidate_index;
    let retry_index = slot.retry_index;
    let record =
        build_report_request_candidate_status_record(ReportRequestCandidateStatusRecordInput {
            slot,
            status_update,
            now_unix_ms: current_unix_ms(),
        });
    let candidate_id = record.id.clone();
    let status = record.status;

    if let Err(record) = submit_local_request_candidate_status_record(state, record).await {
        let candidate_id = record.id.clone();
        match state.upsert_request_candidate(record).await {
            Ok(Some(stored)) => {
                debug!(
                    event_name = "request_candidate_report_status_persisted",
                    log_type = "event",
                    request_id = %request_id_for_log,
                    candidate_id = %stored.id,
                    candidate_index,
                    retry_index,
                    status = request_candidate_status_label(status),
                    source = "report_status",
                    "gateway persisted report-driven request candidate status update"
                );
            }
            Ok(None) => {
                warn!(
                    event_name = "request_candidate_writer_unavailable",
                    log_type = "event",
                    request_id = %request_id_for_log,
                    candidate_id = %candidate_id,
                    candidate_index,
                    retry_index,
                    status = request_candidate_status_label(status),
                    source = "report_status",
                    "gateway skipped request candidate persistence because writer is unavailable"
                );
            }
            Err(err) => {
                warn!(
                    event_name = "request_candidate_report_status_persist_failed",
                    log_type = "event",
                    request_id = %request_id_for_log,
                    candidate_index,
                    retry_index,
                    error = ?err,
                    "gateway failed to persist report-driven request candidate status update"
                );
            }
        }
    } else {
        debug!(
            event_name = "request_candidate_report_status_queued",
            log_type = "event",
            request_id = %request_id_for_log,
            candidate_id = %candidate_id,
            candidate_index,
            retry_index,
            status = request_candidate_status_label(status),
            source = "report_status",
            "gateway queued report-driven request candidate status update"
        );
    }
}

pub(crate) async fn ensure_execution_request_candidate_slot(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &mut ExecutionPlan,
    report_context: &mut Option<Value>,
) -> Result<(), GatewayError> {
    let generated_candidate_id = plan
        .candidate_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let seed = build_execution_request_candidate_seed(
        plan,
        report_context.as_ref(),
        current_unix_ms(),
        generated_candidate_id,
    );
    let generated_candidate_id = seed.upsert_record.id.clone();
    let request_id = short_request_id(plan.request_id.as_str());
    let admission = state
        .resolve_request_billing_admission(&seed.upsert_record, Some(&seed.report_context))
        .await?;

    if !state.has_request_candidate_data_writer() {
        if admission.is_some() {
            return Err(GatewayError::Internal(
                "计费准入无法保存，请求已停止".to_string(),
            ));
        }
        warn!(
            event_name = "request_candidate_writer_unavailable",
            log_type = "event",
            request_id = %request_id,
            provider_id = %plan.provider_id,
            endpoint_id = %plan.endpoint_id,
            key_id = %plan.key_id,
            source = "seed",
            "gateway skipped request candidate seed because writer is unavailable"
        );
        return Ok(());
    }

    let candidate_write = match admission {
        Some(admission) => {
            state
                .upsert_request_candidate_with_billing_admission(seed.upsert_record, admission)
                .await
        }
        None => state.upsert_request_candidate(seed.upsert_record).await,
    };
    let candidate_id = match candidate_write {
        Ok(Some(stored)) => {
            info!(
                event_name = "request_candidate_slot_seeded",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %stored.id,
                provider_id = %plan.provider_id,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                source = "seed",
                "gateway seeded execution request candidate slot"
            );
            stored.id
        }
        Ok(None) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %generated_candidate_id,
                provider_id = %plan.provider_id,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                source = "seed",
                "gateway skipped request candidate seed because writer is unavailable"
            );
            generated_candidate_id
        }
        Err(err) => {
            warn!(
                event_name = "request_candidate_slot_seed_failed",
                log_type = "event",
                request_id = %request_id,
                error = ?err,
                "gateway failed to seed execution request candidate slot"
            );
            return Err(err);
        }
    };

    plan.candidate_id = Some(candidate_id.clone());
    *report_context = Some(finalize_execution_request_candidate_report_context(
        seed.report_context,
        &candidate_id,
    ));
    Ok(())
}

pub(crate) async fn persist_available_local_candidate(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    trace_id: &str,
    user_id: &str,
    api_key_id: &str,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    candidate_index: u32,
    retry_index: u32,
    candidate_id: &str,
    required_capabilities: Option<&Value>,
    extra_data: Option<serde_json::Value>,
    created_at_unix_ms: u64,
    error_context: &'static str,
) -> String {
    match state
        .upsert_request_candidate(UpsertRequestCandidateRecord {
            id: candidate_id.to_string(),
            request_id: trace_id.to_string(),
            user_id: Some(user_id.to_string()),
            api_key_id: Some(api_key_id.to_string()),
            username: None,
            api_key_name: None,
            candidate_index,
            retry_index,
            provider_id: Some(candidate.provider_id.clone()),
            endpoint_id: Some(candidate.endpoint_id.clone()),
            key_id: Some(candidate.key_id.clone()),
            status: RequestCandidateStatus::Available,
            skip_reason: None,
            is_cached: Some(false),
            status_code: None,
            error_type: None,
            error_message: None,
            latency_ms: None,
            concurrent_requests: None,
            extra_data,
            required_capabilities: required_capabilities.cloned(),
            created_at_unix_ms: Some(created_at_unix_ms),
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
        })
        .await
    {
        Ok(Some(stored)) => {
            debug!(
                event_name = "request_candidate_status_persisted",
                log_type = "event",
                request_id = %short_request_id(trace_id),
                candidate_id = %stored.id,
                candidate_index,
                retry_index,
                status = "available",
                source = "planner_available",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                has_required_capabilities = required_capabilities.is_some(),
                "gateway persisted available local request candidate"
            );
            stored.id
        }
        Ok(None) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %short_request_id(trace_id),
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = "available",
                source = "planner_available",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                "gateway skipped request candidate persistence because writer is unavailable"
            );
            candidate_id.to_string()
        }
        Err(err) => {
            warn!(
                trace_id = %trace_id,
                candidate_id = %candidate_id,
                error = ?err,
                "{error_context}"
            );
            candidate_id.to_string()
        }
    }
}

pub(crate) async fn persist_skipped_local_candidate(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    trace_id: &str,
    user_id: &str,
    api_key_id: &str,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    candidate_index: u32,
    retry_index: u32,
    candidate_id: &str,
    required_capabilities: Option<&Value>,
    skip_reason: &str,
    extra_data: Option<serde_json::Value>,
    finished_at_unix_ms: u64,
    error_context: &'static str,
) {
    match state
        .upsert_request_candidate(UpsertRequestCandidateRecord {
            id: candidate_id.to_string(),
            request_id: trace_id.to_string(),
            user_id: Some(user_id.to_string()),
            api_key_id: Some(api_key_id.to_string()),
            username: None,
            api_key_name: None,
            candidate_index,
            retry_index,
            provider_id: Some(candidate.provider_id.clone()),
            endpoint_id: Some(candidate.endpoint_id.clone()),
            key_id: Some(candidate.key_id.clone()),
            status: RequestCandidateStatus::Skipped,
            skip_reason: Some(skip_reason.to_string()),
            is_cached: Some(false),
            status_code: None,
            error_type: None,
            error_message: None,
            latency_ms: None,
            concurrent_requests: None,
            extra_data,
            required_capabilities: required_capabilities.cloned(),
            created_at_unix_ms: None,
            started_at_unix_ms: None,
            finished_at_unix_ms: Some(finished_at_unix_ms),
        })
        .await
    {
        Ok(Some(stored)) => {
            debug!(
                event_name = "request_candidate_status_persisted",
                log_type = "event",
                request_id = %short_request_id(trace_id),
                candidate_id = %stored.id,
                candidate_index,
                retry_index,
                status = "skipped",
                skip_reason,
                source = "planner_skipped",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                has_required_capabilities = required_capabilities.is_some(),
                "gateway persisted skipped local request candidate"
            );
        }
        Ok(None) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %short_request_id(trace_id),
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = "skipped",
                skip_reason,
                source = "planner_skipped",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                "gateway skipped request candidate persistence because writer is unavailable"
            );
        }
        Err(err) => {
            warn!(
                trace_id = %trace_id,
                candidate_id = %candidate_id,
                skip_reason,
                error = ?err,
                "{error_context}"
            );
        }
    }
}

pub(crate) async fn resolve_locally_actionable_request_candidate_report_context(
    state: &(impl RequestCandidateRuntimeReader + ?Sized),
    context: &Value,
) -> Option<Value> {
    let request_id = context
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let existing_candidates = state
        .read_request_candidates_by_request_id(request_id)
        .await
        .ok()?;
    if existing_candidates.len() != 1 {
        return None;
    }

    build_locally_actionable_report_context_from_request_candidate(context, &existing_candidates[0])
}

async fn resolve_report_request_candidate_slot(
    state: &(impl RequestCandidateRuntimeReader + ?Sized),
    report_context: Option<&Value>,
) -> Option<SchedulerResolvedReportRequestCandidateSlot> {
    let metadata = parse_request_candidate_report_context(report_context)?;
    let request_id = metadata.request_id.clone()?;
    let existing_candidates = state
        .read_request_candidates_by_request_id(request_id.as_str())
        .await
        .ok()
        .unwrap_or_default();
    resolve_report_request_candidate_slot_from_candidates(
        &existing_candidates,
        metadata,
        current_unix_ms(),
        Uuid::new_v4().to_string(),
    )
}

#[cfg(test)]
pub(crate) async fn flush_request_candidate_status_writes(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
) {
    if let Some(queue) = state.request_candidate_status_write_queue() {
        queue.flush().await;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use aether_contracts::{ExecutionPlan, RequestBody};
    use aether_data::repository::auth::{
        InMemoryAuthApiKeySnapshotRepository, StoredAuthApiKeyExportRecord,
    };
    use aether_data::repository::candidates::InMemoryRequestCandidateRepository;
    use aether_data::repository::usage::InMemoryUsageReadRepository;
    use aether_data_contracts::repository::billing::{
        BillingFundingSource, BillingRequestAdmissionInput, BillingRequestAdmissionRecord,
    };
    use aether_data_contracts::repository::candidates::{
        RequestCandidateReadRepository, RequestCandidateStatus, StoredRequestCandidate,
        UpsertRequestCandidateRecord,
    };
    use aether_scheduler_core::SchedulerMinimalCandidateSelectionCandidate;
    use async_trait::async_trait;
    use serde_json::json;
    use tokio::sync::Mutex;

    use super::{
        ensure_execution_request_candidate_slot, flush_request_candidate_status_writes,
        persist_available_local_candidate, record_report_request_candidate_status,
        resolve_request_candidate_required_capabilities, RequestCandidateRuntimeWriter,
        RequestCandidateStatusWriteQueue, SchedulerRequestCandidateStatusUpdate,
    };
    use crate::data::GatewayDataState;
    use crate::{AppState, GatewayError};

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

    fn build_test_state_with_auth(
        repository: Arc<InMemoryRequestCandidateRepository>,
        auth_repository: Arc<InMemoryAuthApiKeySnapshotRepository>,
    ) -> AppState {
        AppState::new()
            .expect("gateway state should build")
            .with_data_state_for_tests(
                GatewayDataState::with_request_candidate_and_usage_repository_for_tests(
                    repository,
                    Arc::new(InMemoryUsageReadRepository::default()),
                )
                .with_auth_api_key_reader(auth_repository),
            )
    }

    fn sample_plan() -> ExecutionPlan {
        ExecutionPlan {
            request_id: "req-request-candidate-seed-123".to_string(),
            candidate_id: None,
            provider_name: Some("openai".to_string()),
            provider_id: "provider-request-candidate-seed-123".to_string(),
            endpoint_id: "endpoint-request-candidate-seed-123".to_string(),
            key_id: "key-request-candidate-seed-123".to_string(),
            method: "POST".to_string(),
            url: "https://api.openai.example/v1/chat/completions".to_string(),
            headers: BTreeMap::new(),
            content_type: Some("application/json".to_string()),
            content_encoding: None,
            body: RequestBody::from_json(json!({"model": "gpt-5", "messages": []})),
            stream: false,
            client_api_format: "openai:chat".to_string(),
            provider_api_format: "openai:chat".to_string(),
            model_name: Some("gpt-5".to_string()),
            proxy: None,
            transport_profile: None,
            timeouts: None,
        }
    }

    fn sample_minimal_candidate() -> SchedulerMinimalCandidateSelectionCandidate {
        SchedulerMinimalCandidateSelectionCandidate {
            provider_id: "provider-1".to_string(),
            provider_name: "Provider".to_string(),
            provider_type: "custom".to_string(),
            provider_priority: 0,
            endpoint_id: "endpoint-1".to_string(),
            endpoint_api_format: "openai:chat".to_string(),
            key_id: "provider-key-1".to_string(),
            key_name: "provider-key-1".to_string(),
            key_auth_type: "api_key".to_string(),
            key_internal_priority: 0,
            key_global_priority_for_format: Some(0),
            key_capabilities: Some(json!({"provider_only_capability": true})),
            model_id: "model-1".to_string(),
            global_model_id: "global-model-1".to_string(),
            global_model_name: "gpt-5".to_string(),
            selected_provider_model_name: "gpt-5".to_string(),
            mapping_matched_model: None,
        }
    }

    #[derive(Debug, Default)]
    struct RecordingRequestCandidateWriter {
        statuses: Mutex<Vec<RequestCandidateStatus>>,
    }

    #[derive(Debug)]
    struct ExistingAdmissionWriter {
        admission: BillingRequestAdmissionRecord,
        resolution_calls: AtomicUsize,
        admission_write_calls: AtomicUsize,
    }

    #[async_trait]
    impl RequestCandidateRuntimeWriter for ExistingAdmissionWriter {
        fn has_request_candidate_data_writer(&self) -> bool {
            true
        }

        async fn upsert_request_candidate(
            &self,
            _candidate: UpsertRequestCandidateRecord,
        ) -> Result<Option<StoredRequestCandidate>, GatewayError> {
            Ok(None)
        }

        async fn resolve_request_billing_admission(
            &self,
            _candidate: &UpsertRequestCandidateRecord,
            report_context: Option<&serde_json::Value>,
        ) -> Result<Option<BillingRequestAdmissionInput>, GatewayError> {
            self.resolution_calls.fetch_add(1, Ordering::SeqCst);
            let admission = report_context
                .and_then(serde_json::Value::as_object)
                .and_then(|object| object.get("billing_admission"))
                .cloned()
                .ok_or_else(|| GatewayError::Internal("缺少请求扣费决定".to_string()))?;
            serde_json::from_value(admission)
                .map(Some)
                .map_err(|error| GatewayError::Internal(error.to_string()))
        }

        async fn upsert_request_candidate_with_billing_admission(
            &self,
            _candidate: UpsertRequestCandidateRecord,
            admission: BillingRequestAdmissionInput,
        ) -> Result<Option<StoredRequestCandidate>, GatewayError> {
            assert_eq!(admission, self.admission.to_input());
            self.admission_write_calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
    }

    #[async_trait]
    impl RequestCandidateRuntimeWriter for RecordingRequestCandidateWriter {
        fn has_request_candidate_data_writer(&self) -> bool {
            true
        }

        async fn upsert_request_candidate(
            &self,
            candidate: UpsertRequestCandidateRecord,
        ) -> Result<Option<StoredRequestCandidate>, GatewayError> {
            if candidate.status == RequestCandidateStatus::Pending {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            self.statuses.lock().await.push(candidate.status);
            Ok(None)
        }
    }

    fn status_record(status: RequestCandidateStatus) -> UpsertRequestCandidateRecord {
        UpsertRequestCandidateRecord {
            id: "candidate-queue-order".to_string(),
            request_id: "request-queue-order".to_string(),
            user_id: None,
            api_key_id: None,
            username: None,
            api_key_name: None,
            candidate_index: 0,
            retry_index: 0,
            provider_id: Some("provider-queue-order".to_string()),
            endpoint_id: Some("endpoint-queue-order".to_string()),
            key_id: Some("key-queue-order".to_string()),
            status,
            skip_reason: None,
            is_cached: None,
            status_code: None,
            error_type: None,
            error_message: None,
            latency_ms: None,
            concurrent_requests: None,
            extra_data: None,
            required_capabilities: None,
            created_at_unix_ms: None,
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
        }
    }

    #[tokio::test]
    async fn request_candidate_status_queue_preserves_enqueue_order_when_first_write_is_slow() {
        let queue = RequestCandidateStatusWriteQueue::new();
        let writer = Arc::new(RecordingRequestCandidateWriter::default());
        let writer_trait: Arc<dyn RequestCandidateRuntimeWriter> = writer.clone();

        queue
            .submit_record(
                Arc::clone(&writer_trait),
                status_record(RequestCandidateStatus::Pending),
            )
            .await
            .expect("pending status should be queued");
        queue
            .submit_record(writer_trait, status_record(RequestCandidateStatus::Success))
            .await
            .expect("success status should be queued");
        queue.flush().await;

        let statuses = writer.statuses.lock().await.clone();
        assert_eq!(
            statuses,
            vec![
                RequestCandidateStatus::Pending,
                RequestCandidateStatus::Success
            ]
        );
    }

    #[tokio::test]
    async fn seeds_execution_request_candidate_slot_for_plan_without_candidate_id() {
        let repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state = build_test_state(Arc::clone(&repository));
        let mut plan = sample_plan();
        let mut report_context = Some(json!({
            "request_id": "req-request-candidate-seed-123",
            "client_api_format": "openai:chat"
        }));

        ensure_execution_request_candidate_slot(&state, &mut plan, &mut report_context)
            .await
            .expect("request candidate slot should seed");

        let candidate_id = plan
            .candidate_id
            .clone()
            .expect("candidate id should be seeded");
        let report_context = report_context.expect("report context should be populated");
        assert_eq!(
            report_context
                .get("candidate_id")
                .and_then(|value| value.as_str()),
            Some(candidate_id.as_str())
        );
        assert_eq!(
            report_context
                .get("candidate_index")
                .and_then(|value| value.as_u64()),
            Some(0)
        );
        assert_eq!(
            report_context
                .get("provider_id")
                .and_then(|value| value.as_str()),
            Some("provider-request-candidate-seed-123")
        );

        let stored = repository
            .list_by_request_id("req-request-candidate-seed-123")
            .await
            .expect("request candidates should read");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, candidate_id);
        assert_eq!(stored[0].status, RequestCandidateStatus::Pending);
        assert_eq!(
            stored[0].provider_id.as_deref(),
            Some("provider-request-candidate-seed-123")
        );
        assert_eq!(
            stored[0].endpoint_id.as_deref(),
            Some("endpoint-request-candidate-seed-123")
        );
        assert_eq!(
            stored[0].key_id.as_deref(),
            Some("key-request-candidate-seed-123")
        );
    }

    #[tokio::test]
    async fn retry_reuses_report_admission_without_reading_current_balance_again() {
        let writer = ExistingAdmissionWriter {
            admission: BillingRequestAdmissionRecord {
                request_id: "req-request-candidate-seed-123".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("api-key-1".to_string()),
                wallet_id: Some("wallet-1".to_string()),
                global_model_id: Some("global-model-1".to_string()),
                funding_source: BillingFundingSource::Wallet,
                wallet_balance_at_admission: Some(1.0),
                wallet_payment_allowed: true,
                wallet_overage_allowed: false,
                entitlement_ids: Vec::new(),
                entitlement_provider_scopes: std::collections::BTreeMap::new(),
                allowed_provider_ids: Vec::new(),
                billing_admitted: true,
                status: "admitted".to_string(),
                rejection_reason: None,
                schema_version: 1,
                created_at_unix_ms: 1,
                updated_at_unix_ms: 1,
            },
            resolution_calls: AtomicUsize::new(0),
            admission_write_calls: AtomicUsize::new(0),
        };
        let mut plan = sample_plan();
        let mut report_context = Some(json!({
            "request_id": "req-request-candidate-seed-123",
            "global_model_id": "global-model-1",
            "billing_admission": writer.admission.to_input()
        }));

        ensure_execution_request_candidate_slot(&writer, &mut plan, &mut report_context)
            .await
            .expect("retry should reuse the stored admission");

        assert_eq!(writer.resolution_calls.load(Ordering::SeqCst), 1);
        assert_eq!(writer.admission_write_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn seeds_execution_request_candidate_slot_with_existing_candidate_id() {
        let repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state = build_test_state(Arc::clone(&repository));
        let mut plan = sample_plan();
        plan.candidate_id = Some("cand-existing-123".to_string());
        let mut report_context = Some(json!({
            "request_id": "req-request-candidate-seed-123",
            "candidate_index": 2,
            "client_api_format": "openai:chat"
        }));

        ensure_execution_request_candidate_slot(&state, &mut plan, &mut report_context)
            .await
            .expect("request candidate slot should seed");

        assert_eq!(plan.candidate_id.as_deref(), Some("cand-existing-123"));
        let report_context = report_context.expect("report context should be populated");
        assert_eq!(
            report_context
                .get("candidate_id")
                .and_then(|value| value.as_str()),
            Some("cand-existing-123")
        );
        let stored = repository
            .list_by_request_id("req-request-candidate-seed-123")
            .await
            .expect("request candidates should read");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "cand-existing-123");
        assert_eq!(stored[0].status, RequestCandidateStatus::Pending);
        assert_eq!(stored[0].candidate_index, 2);
    }

    #[tokio::test]
    async fn records_report_request_candidate_status_for_existing_slot() {
        let repository = Arc::new(InMemoryRequestCandidateRepository::seed(vec![
            StoredRequestCandidate::new(
                "cand-report-123".to_string(),
                "req-report-123".to_string(),
                Some("user-1".to_string()),
                Some("api-key-1".to_string()),
                None,
                None,
                0,
                0,
                Some("provider-report-123".to_string()),
                Some("endpoint-report-123".to_string()),
                Some("key-report-123".to_string()),
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
                100_000,
                Some(100_000),
                None,
            )
            .expect("request candidate should build"),
        ]));
        let state = build_test_state(Arc::clone(&repository));
        let report_context = json!({
            "request_id": "req-report-123",
            "candidate_id": "cand-report-123",
            "candidate_index": 0,
            "retry_index": 0,
            "provider_id": "provider-report-123",
            "endpoint_id": "endpoint-report-123",
            "key_id": "key-report-123"
        });

        record_report_request_candidate_status(
            &state,
            Some(&report_context),
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Success,
                status_code: Some(200),
                error_type: None,
                error_message: None,
                latency_ms: Some(25),
                started_at_unix_ms: Some(101),
                finished_at_unix_ms: Some(102),
            },
        )
        .await;
        flush_request_candidate_status_writes(&state).await;

        let stored = repository
            .list_by_request_id("req-report-123")
            .await
            .expect("request candidates should read");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "cand-report-123");
        assert_eq!(stored[0].status, RequestCandidateStatus::Success);
        assert_eq!(stored[0].status_code, Some(200));
        assert_eq!(stored[0].latency_ms, Some(25));
        assert_eq!(stored[0].started_at_unix_ms, Some(101));
        assert_eq!(stored[0].finished_at_unix_ms, Some(102));
    }

    #[tokio::test]
    async fn resolves_request_candidate_required_capabilities_from_user_model_and_api_key() {
        let repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let auth_repository = Arc::new(
            InMemoryAuthApiKeySnapshotRepository::default().with_export_records(vec![
                StoredAuthApiKeyExportRecord::new(
                    "user-1".to_string(),
                    "api-key-1".to_string(),
                    "hash-1".to_string(),
                    None,
                    Some("default".to_string()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(json!({"cache_1h": false, "context_1m": true})),
                    true,
                    None,
                    false,
                    0,
                    0,
                    0.0,
                    false,
                )
                .expect("export record should build"),
            ]),
        );
        let state = build_test_state_with_auth(repository, auth_repository)
            .with_auth_user_model_capability_settings_for_tests(
                "user-1",
                json!({
                    "gpt-5": {
                        "cache_1h": true,
                        "context_1m": false
                    }
                }),
            );
        let explicit_required_capabilities = json!({"gemini_files": true});

        let required_capabilities = resolve_request_candidate_required_capabilities(
            &state,
            "user-1",
            "api-key-1",
            Some("gpt-5"),
            Some(&explicit_required_capabilities),
            false,
        )
        .await
        .expect("required capabilities should resolve");

        assert_eq!(required_capabilities["cache_1h"], json!(false));
        assert_eq!(required_capabilities["context_1m"], json!(true));
        assert_eq!(required_capabilities["gemini_files"], json!(true));
    }

    #[tokio::test]
    async fn persists_request_required_capabilities_instead_of_provider_key_capabilities() {
        let repository = Arc::new(InMemoryRequestCandidateRepository::default());
        let state = build_test_state(Arc::clone(&repository));
        let required_capabilities = json!({"cache_1h": true});

        persist_available_local_candidate(
            &state,
            "req-runtime-cap-123",
            "user-1",
            "api-key-1",
            &sample_minimal_candidate(),
            0,
            0,
            "cand-runtime-cap-123",
            Some(&required_capabilities),
            None,
            100_000,
            "request candidate persist should succeed",
        )
        .await;

        let stored = repository
            .list_by_request_id("req-runtime-cap-123")
            .await
            .expect("request candidates should read");
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].required_capabilities,
            Some(required_capabilities.clone())
        );
        assert_ne!(
            stored[0].required_capabilities,
            sample_minimal_candidate().key_capabilities
        );
    }
}

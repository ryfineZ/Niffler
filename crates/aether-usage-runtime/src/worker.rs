use std::sync::Arc;
use std::time::Duration;

use aether_data_contracts::repository::usage::{StoredRequestUsageAudit, UpsertUsageRecord};
use aether_data_contracts::DataLayerError;
use aether_runtime_state::{RuntimeQueueEntry, RuntimeQueueStore};
use async_trait::async_trait;
use tracing::warn;

use crate::executor::spawn_on_usage_background_runtime;
use crate::{
    build_upsert_usage_record_from_event, settle_usage_if_needed, UsageEvent, UsageQueue,
    UsageRuntimeConfig, UsageSettlementWriter,
};

const USAGE_EVENT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const USAGE_EVENT_MAX_ATTEMPTS: usize = 3;

#[async_trait]
pub trait UsageEventRecorder: Send + Sync {
    async fn record_usage_event(&self, event: &UsageEvent) -> Result<(), DataLayerError>;
}

#[async_trait]
pub trait ManualProxyNodeCounter: Send + Sync {
    async fn increment_manual_proxy_node_requests(
        &self,
        node_id: &str,
        total_delta: i64,
        failed_delta: i64,
        latency_ms: Option<i64>,
    ) -> Result<(), DataLayerError>;
}

#[async_trait]
pub trait UsageRecordWriter: Send + Sync {
    async fn upsert_usage_record(
        &self,
        record: UpsertUsageRecord,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError>;
}

/// Persist a usage row while tolerating a provider API key being deleted after routing.
///
/// Usage events are asynchronous, so a key can disappear between request routing and queue
/// consumption. The provider and endpoint remain useful audit dimensions; only the stale foreign
/// key reference is removed before retrying the write.
pub async fn upsert_usage_record_with_provider_api_key_fallback<T>(
    data: &T,
    record: UpsertUsageRecord,
) -> Result<Option<StoredRequestUsageAudit>, DataLayerError>
where
    T: UsageRecordWriter + Send + Sync + ?Sized,
{
    match data.upsert_usage_record(record.clone()).await {
        Ok(stored) => Ok(stored),
        Err(err)
            if record.provider_api_key_id.is_some()
                && is_missing_provider_api_key_foreign_key(&err) =>
        {
            let provider_api_key_id = record.provider_api_key_id.clone();
            let mut fallback_record = record;
            fallback_record.provider_api_key_id = None;
            warn!(
                event_name = "usage_provider_api_key_missing_fallback",
                log_type = "ops",
                request_id = %fallback_record.request_id,
                provider_api_key_id = ?provider_api_key_id,
                error = %err,
                "provider API key was deleted before the usage event was consumed; preserving the usage record without the stale key reference"
            );
            data.upsert_usage_record(fallback_record).await
        }
        Err(err) => Err(err),
    }
}

pub struct UsageDataEventRecorder<T> {
    data: Arc<T>,
}

impl<T> UsageDataEventRecorder<T> {
    pub fn new(data: Arc<T>) -> Self {
        Self { data }
    }
}

#[async_trait]
impl<T> UsageEventRecorder for UsageDataEventRecorder<T>
where
    T: UsageRecordWriter + UsageSettlementWriter + ManualProxyNodeCounter + Send + Sync,
{
    async fn record_usage_event(&self, event: &UsageEvent) -> Result<(), DataLayerError> {
        write_event_record(self.data.as_ref(), event).await
    }
}

pub struct UsageQueueWorker {
    queue: UsageQueue,
    recorder: Arc<dyn UsageEventRecorder>,
    consumer: String,
    config: UsageRuntimeConfig,
}

impl UsageQueueWorker {
    pub fn new(
        runner: Arc<dyn RuntimeQueueStore>,
        recorder: Arc<dyn UsageEventRecorder>,
        config: UsageRuntimeConfig,
    ) -> Result<Self, DataLayerError> {
        let queue = UsageQueue::new(runner, config.clone())?;
        let consumer = consumer_name();
        Ok(Self {
            queue,
            recorder,
            consumer,
            config,
        })
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        spawn_on_usage_background_runtime(async move {
            loop {
                self.run_forever().await;
                warn!(
                    event_name = "usage_worker_restarting",
                    log_type = "ops",
                    worker_consumer = %self.consumer,
                    worker_group = %self.config.consumer_group,
                    "usage worker stopped; restarting"
                );
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
    }

    async fn run_forever(&self) {
        if let Err(err) = self.queue.ensure_consumer_group().await {
            warn!(
                event_name = "usage_worker_consumer_group_failed",
                log_type = "ops",
                worker_consumer = %self.consumer,
                worker_group = %self.config.consumer_group,
                error = %err,
                "usage worker failed to ensure consumer group"
            );
            return;
        }

        let mut reclaim_interval =
            tokio::time::interval(Duration::from_millis(self.config.reclaim_interval_ms));
        reclaim_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        reclaim_interval.tick().await;

        loop {
            tokio::select! {
                _ = reclaim_interval.tick() => {
                    match self.queue.claim_stale(&self.consumer, "0-0").await {
                        Ok(entries) => {
                            if let Err(err) = self.process_entries(entries).await {
                                warn!(
                                    event_name = "usage_worker_reclaim_process_failed",
                                    log_type = "ops",
                                    worker_consumer = %self.consumer,
                                    worker_group = %self.config.consumer_group,
                                    error = %err,
                                    "usage worker failed while reclaiming stale entries"
                                );
                            }
                        }
                        Err(err) => warn!(
                            event_name = "usage_worker_reclaim_failed",
                            log_type = "ops",
                            worker_consumer = %self.consumer,
                            worker_group = %self.config.consumer_group,
                            error = %err,
                            "usage worker failed to reclaim stale entries"
                        ),
                    }
                }
                result = self.queue.read_group(&self.consumer) => {
                    match result {
                        Ok(entries) => {
                            if let Err(err) = self.process_entries(entries).await {
                                warn!(
                                    event_name = "usage_worker_process_failed",
                                    log_type = "ops",
                                    worker_consumer = %self.consumer,
                                    worker_group = %self.config.consumer_group,
                                    error = %err,
                                    "usage worker failed to process queue entries"
                                );
                                tokio::time::sleep(Duration::from_millis(250)).await;
                            }
                        }
                        Err(err) => {
                            warn!(
                                event_name = "usage_worker_read_failed",
                                log_type = "ops",
                                worker_consumer = %self.consumer,
                                worker_group = %self.config.consumer_group,
                                error = %err,
                                "usage worker failed to read queue"
                            );
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                }
            }
        }
    }

    async fn process_entries(&self, entries: Vec<RuntimeQueueEntry>) -> Result<(), DataLayerError> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut ack_ids = Vec::new();
        for entry in entries {
            match self.process_entry(&entry).await {
                Ok(should_ack) => {
                    if should_ack {
                        ack_ids.push(entry.id.clone());
                    }
                }
                Err(err) => {
                    if !ack_ids.is_empty() {
                        let _ = self.queue.ack_and_delete(&ack_ids).await;
                    }
                    return Err(err);
                }
            }
        }

        if !ack_ids.is_empty() {
            self.queue.ack_and_delete(&ack_ids).await?;
        }

        Ok(())
    }

    async fn process_entry(&self, entry: &RuntimeQueueEntry) -> Result<bool, DataLayerError> {
        let event = match UsageEvent::from_stream_fields(&entry.fields) {
            Ok(event) => event,
            Err(err) => {
                self.queue.push_dead_letter(entry, &err.to_string()).await?;
                return Ok(true);
            }
        };

        let mut last_error = None;
        for attempt in 1..=USAGE_EVENT_MAX_ATTEMPTS {
            match tokio::time::timeout(
                USAGE_EVENT_ATTEMPT_TIMEOUT,
                self.recorder.record_usage_event(&event),
            )
            .await
            {
                Ok(Ok(())) => return Ok(true),
                Ok(Err(err)) => last_error = Some(err.to_string()),
                Err(_) => {
                    last_error = Some(format!(
                        "record_usage_event timed out after {}s",
                        USAGE_EVENT_ATTEMPT_TIMEOUT.as_secs()
                    ));
                }
            }
            if attempt < USAGE_EVENT_MAX_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
            }
        }

        let reason = last_error.unwrap_or_else(|| "record_usage_event failed".to_string());
        self.queue.push_dead_letter(entry, &reason).await?;
        warn!(
            event_name = "usage_worker_event_dead_lettered",
            log_type = "ops",
            worker_consumer = %self.consumer,
            worker_group = %self.config.consumer_group,
            request_id = %event.request_id,
            attempts = USAGE_EVENT_MAX_ATTEMPTS,
            error = %reason,
            "usage event processing failed after retries; moved to dead-letter stream"
        );
        Ok(true)
    }
}

pub fn build_usage_queue_worker<T>(
    runner: Arc<dyn RuntimeQueueStore>,
    data: Arc<T>,
    config: UsageRuntimeConfig,
) -> Result<UsageQueueWorker, DataLayerError>
where
    T: UsageRecordWriter + UsageSettlementWriter + ManualProxyNodeCounter + Send + Sync + 'static,
{
    UsageQueueWorker::new(runner, Arc::new(UsageDataEventRecorder::new(data)), config)
}

pub async fn write_event_record<T>(data: &T, event: &UsageEvent) -> Result<(), DataLayerError>
where
    T: UsageRecordWriter + UsageSettlementWriter + ManualProxyNodeCounter + Send + Sync,
{
    let record = build_upsert_usage_record_from_event(event)?;
    let stored = upsert_usage_record_with_provider_api_key_fallback(data, record).await?;
    if let Some(stored) = stored {
        if let Err(err) = settle_usage_if_needed(data, &stored).await {
            warn!(
                event_name = "usage_settlement_deferred",
                log_type = "ops",
                request_id = %event.request_id,
                billing_status = ?stored.billing_status,
                error = %err,
                "usage record was written but settlement failed; leaving it for retry"
            );
        }
    }
    increment_manual_proxy_node_from_event(data, event).await;
    Ok(())
}

fn is_missing_provider_api_key_foreign_key(error: &DataLayerError) -> bool {
    matches!(
        error,
        DataLayerError::Postgres(message)
            if message.contains("usage_provider_api_key_id_fkey")
    )
}

async fn increment_manual_proxy_node_from_event<T>(data: &T, event: &UsageEvent)
where
    T: ManualProxyNodeCounter + Send + Sync,
{
    let is_terminal = matches!(
        event.event_type,
        crate::UsageEventType::Completed | crate::UsageEventType::Failed
    );
    if !is_terminal {
        return;
    }
    let Some(node_id) = extract_manual_proxy_node_id(event) else {
        return;
    };
    let failed = matches!(event.event_type, crate::UsageEventType::Failed);
    let failed_delta = if failed { 1i64 } else { 0i64 };
    let latency_ms = event.data.response_time_ms.map(|v| v as i64);
    if let Err(err) = data
        .increment_manual_proxy_node_requests(&node_id, 1, failed_delta, latency_ms)
        .await
    {
        warn!(
            event_name = "manual_proxy_node_increment_failed",
            log_type = "ops",
            node_id = %node_id,
            error = ?err,
            "failed to increment manual proxy node request count"
        );
    }
}

fn extract_manual_proxy_node_id(event: &UsageEvent) -> Option<String> {
    let metadata = event.data.request_metadata.as_ref()?;
    let proxy = metadata.get("proxy")?.as_object()?;
    let mode = proxy
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if mode == "tunnel" {
        return None;
    }
    proxy
        .get("node_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(String::from)
}

fn consumer_name() -> String {
    let host = std::env::var("HOSTNAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "aether-gateway".to_string());
    format!("{host}:{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use aether_data_contracts::repository::settlement::{
        StoredUsageSettlement, UsageSettlementInput,
    };
    use aether_data_contracts::repository::usage::{StoredRequestUsageAudit, UpsertUsageRecord};
    use async_trait::async_trait;

    use super::{write_event_record, ManualProxyNodeCounter, UsageRecordWriter};
    use crate::{UsageEvent, UsageEventData, UsageEventType, UsageSettlementWriter};

    #[derive(Default)]
    struct TestUsageStore {
        records: Mutex<Vec<UpsertUsageRecord>>,
        settlements: Mutex<Vec<UsageSettlementInput>>,
        settlement_error: bool,
        reject_provider_api_key_fk: bool,
    }

    #[async_trait]
    impl UsageRecordWriter for TestUsageStore {
        async fn upsert_usage_record(
            &self,
            record: UpsertUsageRecord,
        ) -> Result<Option<StoredRequestUsageAudit>, aether_data_contracts::DataLayerError>
        {
            if self.reject_provider_api_key_fk && record.provider_api_key_id.is_some() {
                return Err(aether_data_contracts::DataLayerError::Postgres(
                    "insert or update on table \"usage\" violates foreign key constraint \"usage_provider_api_key_id_fkey\"".to_string(),
                ));
            }
            self.records
                .lock()
                .expect("records lock")
                .push(record.clone());
            Ok(Some(
                StoredRequestUsageAudit::new(
                    "usage-1".to_string(),
                    record.request_id,
                    record.user_id,
                    record.api_key_id,
                    record.username,
                    record.api_key_name,
                    record.provider_name,
                    record.model,
                    record.target_model,
                    record.provider_id,
                    record.provider_endpoint_id,
                    record.provider_api_key_id,
                    record.request_type,
                    record.api_format,
                    record.api_family,
                    record.endpoint_kind,
                    record.endpoint_api_format,
                    record.provider_api_family,
                    record.provider_endpoint_kind,
                    record.has_format_conversion.unwrap_or(false),
                    record.is_stream.unwrap_or(false),
                    record.input_tokens.unwrap_or_default() as i32,
                    record.output_tokens.unwrap_or_default() as i32,
                    record.total_tokens.unwrap_or_default() as i32,
                    record.total_cost_usd.unwrap_or_default(),
                    record.actual_total_cost_usd.unwrap_or_default(),
                    record.status_code.map(i32::from),
                    record.error_message,
                    record.error_category,
                    record.response_time_ms.map(|value| value as i32),
                    record.first_byte_time_ms.map(|value| value as i32),
                    record.status,
                    record.billing_status,
                    record
                        .created_at_unix_ms
                        .unwrap_or(record.updated_at_unix_secs) as i64,
                    record.updated_at_unix_secs as i64,
                    record.finalized_at_unix_secs.map(|value| value as i64),
                )
                .expect("stored usage should build"),
            ))
        }
    }

    #[async_trait]
    impl UsageSettlementWriter for TestUsageStore {
        fn has_usage_settlement_writer(&self) -> bool {
            true
        }

        async fn settle_usage(
            &self,
            input: UsageSettlementInput,
        ) -> Result<Option<StoredUsageSettlement>, aether_data_contracts::DataLayerError> {
            if self.settlement_error {
                return Err(aether_data_contracts::DataLayerError::UnexpectedValue(
                    "synthetic settlement failure".to_string(),
                ));
            }
            self.settlements
                .lock()
                .expect("settlements lock")
                .push(input);
            Ok(None)
        }
    }

    #[async_trait]
    impl ManualProxyNodeCounter for TestUsageStore {
        async fn increment_manual_proxy_node_requests(
            &self,
            _node_id: &str,
            _total_delta: i64,
            _failed_delta: i64,
            _latency_ms: Option<i64>,
        ) -> Result<(), aether_data_contracts::DataLayerError> {
            Ok(())
        }
    }

    fn sample_event() -> UsageEvent {
        UsageEvent::new(
            UsageEventType::Completed,
            "req-worker-123".to_string(),
            UsageEventData {
                user_id: Some("user-worker-123".to_string()),
                api_key_id: Some("api-key-worker-123".to_string()),
                provider_name: "openai".to_string(),
                provider_id: Some("provider-worker-123".to_string()),
                provider_endpoint_id: Some("endpoint-worker-123".to_string()),
                provider_api_key_id: Some("provider-key-worker-123".to_string()),
                model: "gpt-5".to_string(),
                api_format: Some("openai:chat".to_string()),
                endpoint_api_format: Some("openai:chat".to_string()),
                is_stream: Some(false),
                status_code: Some(200),
                input_tokens: Some(4),
                output_tokens: Some(6),
                total_tokens: Some(10),
                total_cost_usd: Some(1.25),
                actual_total_cost_usd: Some(1.25),
                response_time_ms: Some(52),
                ..UsageEventData::default()
            },
        )
    }

    #[tokio::test]
    async fn write_event_record_persists_usage_and_triggers_settlement() {
        let store = TestUsageStore::default();
        let event = sample_event();

        write_event_record(&store, &event)
            .await
            .expect("worker should write usage record");

        let records = store.records.lock().expect("records lock");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].request_id, "req-worker-123");
        assert_eq!(records[0].status, "completed");
        drop(records);

        let settlements = store.settlements.lock().expect("settlements lock");
        assert_eq!(settlements.len(), 1);
        assert_eq!(settlements[0].request_id, "req-worker-123");
    }

    #[tokio::test]
    async fn write_event_record_keeps_usage_when_settlement_fails() {
        let store = TestUsageStore {
            settlement_error: true,
            ..TestUsageStore::default()
        };
        let event = sample_event();

        write_event_record(&store, &event)
            .await
            .expect("settlement failure must not stop usage consumption");

        assert_eq!(store.records.lock().expect("records lock").len(), 1);
        assert!(store
            .settlements
            .lock()
            .expect("settlements lock")
            .is_empty());
    }

    #[tokio::test]
    async fn write_event_record_recovers_when_provider_key_was_deleted() {
        let store = TestUsageStore {
            reject_provider_api_key_fk: true,
            ..TestUsageStore::default()
        };
        let event = sample_event();

        write_event_record(&store, &event)
            .await
            .expect("stale provider key must not block usage consumption");

        let records = store.records.lock().expect("records lock");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].provider_api_key_id, None);
        assert_eq!(
            event.data.provider_api_key_id.as_deref(),
            Some("provider-key-worker-123")
        );
    }
}

use aether_data_contracts::DataLayerError;
use aether_usage_runtime::{settle_usage_if_needed, should_attempt_usage_settlement};
use futures_util::{stream, StreamExt};
use std::collections::HashSet;
use tracing::error;

use crate::data::GatewayDataState;

const PENDING_SETTLEMENT_RETRY_BATCH_SIZE: usize = 100;
const PENDING_SETTLEMENT_RETRY_MAX_BATCHES_PER_RUN: usize = 1_000;
const PENDING_SETTLEMENT_RETRY_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct PendingSettlementRetrySummary {
    pub(super) settled: usize,
    pub(super) failed: usize,
}

pub(super) async fn retry_pending_usage_settlements_once(
    data: &GatewayDataState,
) -> Result<PendingSettlementRetrySummary, DataLayerError> {
    if !data.has_usage_audit_reader() || !data.has_settlement_writer() {
        return Ok(PendingSettlementRetrySummary::default());
    }

    let mut summary = PendingSettlementRetrySummary::default();
    let mut processed_request_ids = HashSet::new();
    for _ in 0..PENDING_SETTLEMENT_RETRY_MAX_BATCHES_PER_RUN {
        let pending = data
            .list_pending_terminal_usage_for_settlement(PENDING_SETTLEMENT_RETRY_BATCH_SIZE)
            .await?;
        if pending.is_empty() {
            break;
        }
        let work = pending
            .into_iter()
            .filter(|usage| should_attempt_usage_settlement(usage))
            .filter(|usage| processed_request_ids.insert(usage.request_id.clone()))
            .collect::<Vec<_>>();
        if work.is_empty() {
            break;
        }
        let outcomes = stream::iter(work.into_iter().map(|usage| async move {
            let request_id = usage.request_id.clone();
            let result = settle_usage_if_needed(data, &usage).await;
            (request_id, result)
        }))
        .buffer_unordered(PENDING_SETTLEMENT_RETRY_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        for (request_id, result) in outcomes {
            match result {
                Ok(()) => summary.settled += 1,
                Err(err) => {
                    summary.failed += 1;
                    error!(
                        event_name = "usage_pending_settlement_retry_failed",
                        log_type = "ops",
                        worker = "pending_cleanup",
                        request_id = %request_id,
                        error = %err,
                        "gateway could not settle a completed usage record; cost remains pending"
                    );
                }
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use aether_data::repository::usage::InMemoryUsageReadRepository;
    use aether_data_contracts::repository::settlement::{
        SettlementWriteRepository, StoredUsageSettlement, UsageSettlementInput,
    };
    use aether_data_contracts::repository::usage::StoredRequestUsageAudit;
    use async_trait::async_trait;

    use super::retry_pending_usage_settlements_once;
    use crate::data::GatewayDataState;

    #[derive(Default)]
    struct RecordingSettlementWriter {
        requests: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl SettlementWriteRepository for RecordingSettlementWriter {
        async fn settle_usage(
            &self,
            input: UsageSettlementInput,
        ) -> Result<Option<StoredUsageSettlement>, aether_data_contracts::DataLayerError> {
            self.requests
                .lock()
                .expect("settlement requests lock")
                .push(input.request_id.clone());
            Ok(Some(StoredUsageSettlement {
                request_id: input.request_id,
                wallet_id: None,
                billing_status: "settled".to_string(),
                wallet_balance_before: None,
                wallet_balance_after: None,
                wallet_recharge_balance_before: None,
                wallet_recharge_balance_after: None,
                wallet_gift_balance_before: None,
                wallet_gift_balance_after: None,
                provider_monthly_used_usd: None,
                finalized_at_unix_secs: input.finalized_at_unix_secs,
            }))
        }
    }

    #[tokio::test]
    async fn maintenance_retries_terminal_pending_usage() {
        let usage = StoredRequestUsageAudit::new(
            "usage-retry-1".to_string(),
            "request-retry-1".to_string(),
            Some("user-1".to_string()),
            Some("api-key-1".to_string()),
            None,
            None,
            "OpenAI".to_string(),
            "gpt-test".to_string(),
            None,
            Some("provider-1".to_string()),
            None,
            None,
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            Some("openai:chat".to_string()),
            Some("openai".to_string()),
            Some("chat".to_string()),
            false,
            false,
            10,
            5,
            15,
            0.1,
            0.05,
            Some(200),
            None,
            None,
            Some(100),
            Some(20),
            "completed".to_string(),
            "pending".to_string(),
            100,
            101,
            Some(101),
        )
        .expect("usage should build");
        let usage_reader = Arc::new(InMemoryUsageReadRepository::seed(vec![usage]));
        let settlement_writer = Arc::new(RecordingSettlementWriter::default());
        let data = GatewayDataState::with_usage_reader_for_tests(usage_reader)
            .with_settlement_writer_for_tests(settlement_writer.clone());

        let summary = retry_pending_usage_settlements_once(&data)
            .await
            .expect("pending settlement retry should run");

        assert_eq!(summary.settled, 1);
        assert_eq!(summary.failed, 0);
        assert_eq!(
            *settlement_writer
                .requests
                .lock()
                .expect("settlement requests lock"),
            vec!["request-retry-1".to_string()]
        );
    }
}

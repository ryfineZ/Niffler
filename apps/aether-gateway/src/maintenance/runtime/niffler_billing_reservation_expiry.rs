use aether_data_contracts::repository::niffler_core::{
    FinalizeNifflerBillingReservationRecord, NifflerBillingReservationListQuery,
    NifflerBillingReservationStatus,
};
use aether_data_contracts::DataLayerError;
use uuid::Uuid;

use crate::data::GatewayDataState;

use super::{
    now_unix_secs, NIFFLER_BILLING_RESERVATION_EXPIRY_BATCH_SIZE,
    NIFFLER_BILLING_RESERVATION_EXPIRY_MAX_BATCHES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub(crate) struct NifflerBillingReservationExpirySummary {
    pub(crate) scanned: usize,
    pub(crate) expired: usize,
    pub(crate) capped: bool,
}

pub(crate) async fn perform_niffler_billing_reservation_expiry_once(
    data: &GatewayDataState,
) -> Result<NifflerBillingReservationExpirySummary, DataLayerError> {
    if !data.has_niffler_core_reader() || !data.has_niffler_core_writer() {
        return Ok(NifflerBillingReservationExpirySummary::default());
    }

    let now_unix_ms = now_unix_secs().saturating_mul(1_000);
    let mut summary = NifflerBillingReservationExpirySummary::default();

    for batch_index in 0..NIFFLER_BILLING_RESERVATION_EXPIRY_MAX_BATCHES {
        let page = data
            .list_niffler_billing_reservations(&NifflerBillingReservationListQuery {
                status: Some(NifflerBillingReservationStatus::Active),
                user_id: None,
                api_key_id: None,
                request_id: None,
                expires_at_gte_unix_ms: None,
                expires_at_lte_unix_ms: Some(now_unix_ms),
                expires_at_lt_unix_ms: None,
                finalized_at_gte_unix_ms: None,
                finalized_at_lt_unix_ms: None,
                offset: 0,
                limit: NIFFLER_BILLING_RESERVATION_EXPIRY_BATCH_SIZE,
            })
            .await?;

        if page.items.is_empty() {
            break;
        }

        summary.scanned = summary.scanned.saturating_add(page.items.len());
        for reservation in page.items {
            let finalized = data
                .finalize_niffler_billing_reservation_by_request_id(
                    FinalizeNifflerBillingReservationRecord {
                        request_id: reservation.request_id.clone(),
                        status: NifflerBillingReservationStatus::Expired,
                        finalized_at_unix_ms: now_unix_ms,
                        settlement_snapshot_id: None,
                        release_reason: Some("reservation_expired".to_string()),
                        event_id: stable_expired_event_id(&reservation.request_id),
                        event_idempotency_key: format!(
                            "niffler-billing-reservation-event-expired:{}",
                            reservation.request_id
                        ),
                        actor_id: Some("system".to_string()),
                    },
                )
                .await?;
            if finalized
                .as_ref()
                .is_some_and(|record| record.status == NifflerBillingReservationStatus::Expired)
            {
                summary.expired = summary.expired.saturating_add(1);
            }
        }

        if summary.scanned < (batch_index + 1) * NIFFLER_BILLING_RESERVATION_EXPIRY_BATCH_SIZE {
            break;
        }
        summary.capped = batch_index + 1 >= NIFFLER_BILLING_RESERVATION_EXPIRY_MAX_BATCHES;
    }

    Ok(summary)
}

fn stable_expired_event_id(request_id: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("niffler-billing-reservation-event-expired:{request_id}").as_bytes(),
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_data_contracts::repository::niffler_core::CreateNifflerBillingReservationRecord;

    fn reservation_record(
        request_id: &str,
        wallet_reserved_usd: f64,
        expires_at_unix_ms: u64,
    ) -> CreateNifflerBillingReservationRecord {
        CreateNifflerBillingReservationRecord {
            id: Uuid::new_v4().to_string(),
            request_id: request_id.to_string(),
            user_id: Some("user-1".to_string()),
            api_key_id: Some("key-1".to_string()),
            product_plan_id: Some("plan-1".to_string()),
            reserved_total_usd: wallet_reserved_usd,
            wallet_reserved_usd,
            entitlement_reserved_usd: 0.0,
            reserved_at_unix_ms: 1_000,
            expires_at_unix_ms,
            idempotency_key: format!("reservation-idempotency-{request_id}"),
            event_id: Uuid::new_v4().to_string(),
            event_idempotency_key: format!("reserved-event-idempotency-{request_id}"),
            actor_id: Some("test".to_string()),
        }
    }

    #[tokio::test]
    async fn expiry_worker_releases_only_expired_active_reservations_when_url_is_set() {
        let Some(state) = crate::data::tests::postgres_app_state_when_url_is_set(
            "expiry_worker_releases_only_expired_active_reservations",
        )
        .await
        else {
            return;
        };
        let expired_request_id = Uuid::new_v4().to_string();
        let open_request_id = Uuid::new_v4().to_string();
        let now_unix_ms = now_unix_secs().saturating_mul(1_000);
        state
            .data
            .create_niffler_billing_reservation(reservation_record(
                &expired_request_id,
                1.0,
                now_unix_ms.saturating_sub(1),
            ))
            .await
            .expect("expired reservation should create");
        state
            .data
            .create_niffler_billing_reservation(reservation_record(
                &open_request_id,
                2.0,
                now_unix_ms.saturating_add(60_000),
            ))
            .await
            .expect("open reservation should create");

        let summary = perform_niffler_billing_reservation_expiry_once(&state.data)
            .await
            .expect("expiry worker should run");
        assert!(summary.scanned >= 1);
        assert!(summary.expired >= 1);
        assert!(!summary.capped);

        let expired = state
            .data
            .list_niffler_billing_reservations(&NifflerBillingReservationListQuery {
                status: None,
                user_id: None,
                api_key_id: None,
                request_id: Some(expired_request_id),
                expires_at_gte_unix_ms: None,
                expires_at_lte_unix_ms: None,
                expires_at_lt_unix_ms: None,
                finalized_at_gte_unix_ms: None,
                finalized_at_lt_unix_ms: None,
                offset: 0,
                limit: 1,
            })
            .await
            .expect("expired reservation should read");
        assert_eq!(
            expired.items[0].status,
            NifflerBillingReservationStatus::Expired
        );

        let open = state
            .data
            .list_niffler_billing_reservations(&NifflerBillingReservationListQuery {
                status: None,
                user_id: None,
                api_key_id: None,
                request_id: Some(open_request_id),
                expires_at_gte_unix_ms: None,
                expires_at_lte_unix_ms: None,
                expires_at_lt_unix_ms: None,
                finalized_at_gte_unix_ms: None,
                finalized_at_lt_unix_ms: None,
                offset: 0,
                limit: 1,
            })
            .await
            .expect("open reservation should read");
        assert_eq!(
            open.items[0].status,
            NifflerBillingReservationStatus::Active
        );
    }
}

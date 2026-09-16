mod postgres;

pub use aether_data_contracts::repository::niffler_core::{
    CreateNifflerAccountRiskEventRecord, CreateNifflerBillingReservationDryRunRecord,
    CreateNifflerBillingReservationRecord, CreateNifflerErrorReturnSettingRecord,
    CreateNifflerProductPlanRecord, CreateNifflerReferralRewardLedgerRecord,
    CreateNifflerRouteAttemptRecord, CreateNifflerSettlementSnapshotRecord,
    CreateNifflerUpstreamAccountRecord, CreateNifflerUpstreamServiceRecord,
    FinalizeNifflerBillingReservationRecord, NifflerAccountModelCapabilityListQuery,
    NifflerAccountProtectionAction, NifflerAccountStatus, NifflerApiKeyProductPlanBindingListQuery,
    NifflerBillingReservationDryRunListQuery, NifflerBillingReservationListQuery,
    NifflerBillingReservationStatus, NifflerConsistencyCheckListQuery, NifflerCoreReadRepository,
    NifflerCoreRepository, NifflerCoreWriteRepository, NifflerErrorResponseScope,
    NifflerErrorReturnSettingListQuery, NifflerPauseDuration, NifflerProductPlanListQuery,
    NifflerProductPlanModelListQuery, NifflerProtocolKind, NifflerReferralRewardLedgerListQuery,
    NifflerReferralRewardLedgerStatus, NifflerRouteAttemptListQuery,
    NifflerRuntimeAccountModelAccessListQuery, NifflerRuntimeRolloutSettingListQuery,
    NifflerRuntimeRolloutTargetScope, NifflerServiceCapabilityKind,
    NifflerSettlementSnapshotListQuery, NifflerStabilityObservationListQuery,
    NifflerUpstreamAccountListQuery, NifflerUpstreamErrorHandlingStep,
    NifflerUpstreamServiceCapabilityListQuery, NifflerUpstreamServiceListQuery,
    NifflerUserResponseMode, StoredNifflerAccountModelCapability,
    StoredNifflerAccountModelCapabilityListPage, StoredNifflerAccountRiskEvent,
    StoredNifflerApiKeyProductPlanBinding, StoredNifflerApiKeyProductPlanBindingListPage,
    StoredNifflerBillingReservation, StoredNifflerBillingReservationDryRun,
    StoredNifflerBillingReservationDryRunListPage, StoredNifflerBillingReservationListPage,
    StoredNifflerConsistencyCheckItem, StoredNifflerConsistencyCheckListPage,
    StoredNifflerErrorReturnSetting, StoredNifflerErrorReturnSettingListPage,
    StoredNifflerProductPlan, StoredNifflerProductPlanListPage, StoredNifflerProductPlanModel,
    StoredNifflerProductPlanModelListPage, StoredNifflerReferralRewardLedger,
    StoredNifflerReferralRewardLedgerListPage, StoredNifflerRouteAttempt,
    StoredNifflerRouteAttemptListItem, StoredNifflerRouteAttemptListPage,
    StoredNifflerRuntimeAccountModelAccess, StoredNifflerRuntimeAccountModelAccessListPage,
    StoredNifflerRuntimeRolloutSetting, StoredNifflerRuntimeRolloutSettingListPage,
    StoredNifflerSettlementSnapshot, StoredNifflerSettlementSnapshotListItem,
    StoredNifflerSettlementSnapshotListPage, StoredNifflerStabilityObservation,
    StoredNifflerStabilityObservationListPage, StoredNifflerUpstreamAccount,
    StoredNifflerUpstreamAccountListPage, StoredNifflerUpstreamService,
    StoredNifflerUpstreamServiceCapability, StoredNifflerUpstreamServiceCapabilityListPage,
    StoredNifflerUpstreamServiceListPage, UpsertNifflerApiKeyProductPlanBindingRecord,
    UpsertNifflerProductPlanModelRecord, UpsertNifflerRuntimeRolloutSettingRecord,
    UpsertNifflerStabilityObservationRecord, UpsertNifflerUpstreamServiceCapabilityRecord,
};
pub use postgres::SqlxNifflerCoreRepository;

pub(crate) fn i64_from_u64(value: u64, field: &str) -> Result<i64, crate::DataLayerError> {
    i64::try_from(value).map_err(|_| {
        crate::DataLayerError::InvalidInput(format!("{field} is too large for database"))
    })
}

pub(crate) fn u64_from_i64(value: i64, field: &str) -> Result<u64, crate::DataLayerError> {
    u64::try_from(value).map_err(|_| {
        crate::DataLayerError::UnexpectedValue(format!("{field} is negative: {value}"))
    })
}

pub(crate) fn bounded_limit(limit: usize) -> i64 {
    i64::try_from(limit.clamp(1, 200)).unwrap_or(200)
}

pub(crate) fn bounded_offset(offset: usize) -> i64 {
    i64::try_from(offset).unwrap_or(i64::MAX)
}

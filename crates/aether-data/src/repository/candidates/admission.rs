use aether_data_contracts::repository::billing::{
    BillingRequestAdmissionInput, BillingRequestAdmissionRecord,
};

use super::UpsertRequestCandidateRecord;
use crate::DataLayerError;

pub(super) fn validate_candidate_admission(
    candidate: &UpsertRequestCandidateRecord,
    admission: &BillingRequestAdmissionInput,
) -> Result<(), DataLayerError> {
    candidate.validate()?;
    admission.validate()?;
    if candidate.request_id != admission.request_id {
        return Err(DataLayerError::InvalidInput(
            "candidate and billing admission request_id must match".to_string(),
        ));
    }
    let provider_allowed = match admission.funding_source {
        aether_data_contracts::repository::billing::BillingFundingSource::Plan => {
            candidate.provider_id.as_deref().is_some_and(|provider_id| {
                admission
                    .entitlement_provider_scopes
                    .values()
                    .any(|provider_ids| {
                        provider_ids.is_empty()
                            || provider_ids.iter().any(|allowed| allowed == provider_id)
                    })
            })
        }
        _ => true,
    };
    if !provider_allowed {
        return Err(DataLayerError::InvalidInput(
            "candidate provider is outside billing admission".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_stored_admission_matches_input(
    stored: &BillingRequestAdmissionRecord,
    input: &BillingRequestAdmissionInput,
) -> Result<(), DataLayerError> {
    let same_identity = stored.request_id == input.request_id
        && stored.user_id == input.user_id
        && stored.api_key_id == input.api_key_id
        && stored.wallet_id == input.wallet_id
        && stored.global_model_id == input.global_model_id
        && stored.funding_source == input.funding_source
        && stored.wallet_balance_at_admission == input.wallet_balance_at_admission
        && stored.wallet_payment_allowed == input.wallet_payment_allowed
        && stored.wallet_overage_allowed == input.wallet_overage_allowed
        && stored.entitlement_ids == input.entitlement_ids
        && stored.entitlement_provider_scopes == input.entitlement_provider_scopes
        && stored.allowed_provider_ids == input.allowed_provider_ids
        && stored.schema_version == input.schema_version;
    if !same_identity || !stored.billing_admitted || stored.status != "admitted" {
        return Err(DataLayerError::InvalidInput(format!(
            "request_id {} already has a different billing admission",
            input.request_id
        )));
    }
    Ok(())
}

pub(super) fn admission_record_from_input(
    input: BillingRequestAdmissionInput,
    created_at_unix_ms: u64,
) -> BillingRequestAdmissionRecord {
    BillingRequestAdmissionRecord {
        request_id: input.request_id,
        user_id: input.user_id,
        api_key_id: input.api_key_id,
        wallet_id: input.wallet_id,
        global_model_id: input.global_model_id,
        funding_source: input.funding_source,
        wallet_balance_at_admission: input.wallet_balance_at_admission,
        wallet_payment_allowed: input.wallet_payment_allowed,
        wallet_overage_allowed: input.wallet_overage_allowed,
        entitlement_ids: input.entitlement_ids,
        entitlement_provider_scopes: input.entitlement_provider_scopes,
        allowed_provider_ids: input.allowed_provider_ids,
        billing_admitted: true,
        status: "admitted".to_string(),
        rejection_reason: None,
        schema_version: input.schema_version,
        created_at_unix_ms,
        updated_at_unix_ms: created_at_unix_ms,
    }
}

pub(super) fn current_unix_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or_default()
}

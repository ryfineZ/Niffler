use crate::{AppState, GatewayError};
use aether_data_contracts::repository::billing::{
    BillingFundingSource, BillingRequestAdmissionInput, UserDailyQuotaAvailabilityRecord,
};
use aether_data_contracts::repository::{candidate_selection, candidates, quota};
use axum::http::StatusCode;

impl AppState {
    pub(crate) async fn find_request_billing_admission(
        &self,
        request_id: &str,
    ) -> Result<
        Option<aether_data_contracts::repository::billing::BillingRequestAdmissionRecord>,
        GatewayError,
    > {
        self.data
            .find_request_billing_admission(request_id)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn resolve_request_billing_admission(
        &self,
        candidate: &candidates::UpsertRequestCandidateRecord,
        report_context: Option<&serde_json::Value>,
    ) -> Result<Option<BillingRequestAdmissionInput>, GatewayError> {
        let Some(user_id) = candidate
            .user_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            return Ok(None);
        };
        let Some(api_key_id) = candidate
            .api_key_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            return Ok(None);
        };
        if !self.has_wallet_data_reader() {
            return Ok(None);
        }
        let selected_provider_id = candidate
            .provider_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| GatewayError::Internal("计费准入缺少实际供应商".to_string()))?;
        let global_model_id = report_context
            .and_then(serde_json::Value::as_object)
            .and_then(|object| object.get("global_model_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| GatewayError::Internal("计费准入缺少全局模型".to_string()))?;
        let api_key_is_standalone = report_context
            .and_then(serde_json::Value::as_object)
            .and_then(|object| object.get("api_key_is_standalone"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        if let Some(value) = report_context
            .and_then(serde_json::Value::as_object)
            .and_then(|object| object.get("billing_admission"))
        {
            let admission =
                serde_json::from_value::<BillingRequestAdmissionInput>(value.clone())
                    .map_err(|err| GatewayError::Internal(format!("计费准入上下文无效: {err}")))?;
            admission
                .validate()
                .map_err(|err| GatewayError::Internal(err.to_string()))?;
            let identity_matches = admission.request_id == candidate.request_id
                && admission.user_id.as_deref() == Some(user_id)
                && admission.api_key_id.as_deref() == Some(api_key_id)
                && admission.global_model_id.as_deref() == Some(global_model_id);
            if !identity_matches {
                return Err(GatewayError::Internal(
                    "计费准入上下文与实际请求不一致".to_string(),
                ));
            }
            return Ok(Some(admission));
        }

        let wallet_future =
            self.read_wallet_snapshot_for_auth(user_id, api_key_id, api_key_is_standalone);
        let quota_future = async {
            if api_key_is_standalone {
                Ok(None)
            } else {
                self.find_user_daily_quota_availability_for_global_model(
                    user_id,
                    Some(global_model_id),
                )
                .await
            }
        };
        let (wallet, quota): (_, Option<UserDailyQuotaAvailabilityRecord>) =
            tokio::try_join!(wallet_future, quota_future)?;
        let wallet = wallet.ok_or_else(|| GatewayError::Client {
            status: StatusCode::PAYMENT_REQUIRED,
            message: "钱包不可用".to_string(),
        })?;
        if !wallet.status.eq_ignore_ascii_case("active") {
            return Err(GatewayError::Client {
                status: StatusCode::PAYMENT_REQUIRED,
                message: "钱包不可用".to_string(),
            });
        }
        let actual_wallet_balance = wallet.balance + wallet.gift_balance;
        let plan_quota = quota.as_ref().and_then(|quota| {
            let selected_entitlement_ids =
                quota.eligible_entitlement_ids_for_provider(selected_provider_id);
            (quota.base_remaining_usd > 0.000_000_01 && !selected_entitlement_ids.is_empty())
                .then_some(quota)
        });

        let (funding_source, entitlement_ids, entitlement_provider_scopes, allowed_provider_ids) =
            if wallet.limit_mode.eq_ignore_ascii_case("unlimited") {
                (
                    BillingFundingSource::Unlimited,
                    Vec::new(),
                    std::collections::BTreeMap::new(),
                    Vec::new(),
                )
            } else if let Some(quota) = plan_quota {
                let entitlement_provider_scopes = quota
                    .eligible_entitlement_ids
                    .iter()
                    .map(|entitlement_id| {
                        (
                            entitlement_id.clone(),
                            quota
                                .provider_ids_by_entitlement
                                .get(entitlement_id)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    })
                    .collect::<std::collections::BTreeMap<_, _>>();
                let allowed_provider_ids = entitlement_provider_scopes
                    .values()
                    .flatten()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                (
                    BillingFundingSource::Plan,
                    entitlement_provider_scopes.keys().cloned().collect(),
                    entitlement_provider_scopes,
                    allowed_provider_ids.into_iter().collect(),
                )
            } else if actual_wallet_balance > 0.0 {
                (
                    BillingFundingSource::Wallet,
                    Vec::new(),
                    std::collections::BTreeMap::new(),
                    Vec::new(),
                )
            } else {
                let message = if actual_wallet_balance < 0.0 {
                    format!(
                        "钱包欠费 ${:.2}，当前请求没有可用套餐额度",
                        -actual_wallet_balance
                    )
                } else {
                    "钱包余额为 $0.00，当前请求没有可用套餐额度".to_string()
                };
                return Err(GatewayError::Client {
                    status: StatusCode::PAYMENT_REQUIRED,
                    message,
                });
            };

        Ok(Some(BillingRequestAdmissionInput {
            request_id: candidate.request_id.clone(),
            user_id: Some(user_id.to_string()),
            api_key_id: Some(api_key_id.to_string()),
            wallet_id: Some(wallet.id),
            global_model_id: Some(global_model_id.to_string()),
            funding_source,
            wallet_balance_at_admission: Some(actual_wallet_balance),
            wallet_payment_allowed: !wallet.limit_mode.eq_ignore_ascii_case("unlimited")
                && actual_wallet_balance > 0.0,
            wallet_overage_allowed: funding_source == BillingFundingSource::Plan,
            entitlement_ids,
            entitlement_provider_scopes,
            allowed_provider_ids,
            schema_version: 1,
        }))
    }

    pub(crate) async fn list_minimal_candidate_selection_rows_for_api_format(
        &self,
        api_format: &str,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_minimal_candidate_selection_rows_for_api_format(api_format)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_minimal_candidate_selection_rows_for_api_format_and_global_model(
        &self,
        api_format: &str,
        global_model_name: &str,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_minimal_candidate_selection_rows(api_format, global_model_name)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_minimal_candidate_selection_rows_for_api_format_and_requested_model(
        &self,
        api_format: &str,
        requested_model_name: &str,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_minimal_candidate_selection_rows_for_requested_model(
                api_format,
                requested_model_name,
            )
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_minimal_candidate_selection_rows_for_api_format_and_requested_model_page(
        &self,
        query: &candidate_selection::StoredRequestedModelCandidateRowsQuery,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_minimal_candidate_selection_rows_for_requested_model_page(query)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_pool_key_candidate_rows_for_group(
        &self,
        query: &candidate_selection::StoredPoolKeyCandidateRowsQuery,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_pool_key_candidate_rows_for_group(query)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_pool_key_candidate_rows_for_group_key_ids(
        &self,
        query: &candidate_selection::StoredPoolKeyCandidateRowsByKeyIdsQuery,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_pool_key_candidate_rows_for_group_key_ids(query)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn read_provider_quota_snapshot(
        &self,
        provider_id: &str,
    ) -> Result<Option<quota::StoredProviderQuotaSnapshot>, GatewayError> {
        self.data
            .find_provider_quota_by_provider_id(provider_id)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn read_provider_quota_snapshots(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<quota::StoredProviderQuotaSnapshot>, GatewayError> {
        self.data
            .find_provider_quotas_by_provider_ids(provider_ids)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn read_recent_request_candidates(
        &self,
        limit: usize,
    ) -> Result<Vec<candidates::StoredRequestCandidate>, GatewayError> {
        self.data
            .list_recent_request_candidates(limit)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn upsert_request_candidate(
        &self,
        candidate: candidates::UpsertRequestCandidateRecord,
    ) -> Result<Option<candidates::StoredRequestCandidate>, GatewayError> {
        self.data
            .upsert_request_candidate(candidate)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn upsert_request_candidate_with_billing_admission(
        &self,
        candidate: candidates::UpsertRequestCandidateRecord,
        admission: aether_data_contracts::repository::billing::BillingRequestAdmissionInput,
    ) -> Result<
        Option<(
            candidates::StoredRequestCandidate,
            aether_data_contracts::repository::billing::BillingRequestAdmissionRecord,
        )>,
        GatewayError,
    > {
        self.data
            .upsert_request_candidate_with_billing_admission(candidate, admission)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }
}

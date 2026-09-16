use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use aether_data_contracts::repository::billing::{
    BillingFundingSource, BillingRequestAdmissionInput,
};
use async_trait::async_trait;

use super::{
    plan_finite_wallet_debit, settlement_billing_status_for_usage_status,
    SettlementWriteRepository, StoredUsageSettlement, UsageSettlementInput,
};
use crate::repository::wallet::{InMemoryWalletRepository, StoredWalletSnapshot};
use crate::DataLayerError;

#[derive(Debug)]
enum InMemorySettlementWalletStore {
    Owned(RwLock<BTreeMap<String, StoredWalletSnapshot>>),
    Shared(Arc<InMemoryWalletRepository>),
}

impl Default for InMemorySettlementWalletStore {
    fn default() -> Self {
        Self::Owned(RwLock::new(BTreeMap::new()))
    }
}

impl InMemorySettlementWalletStore {
    fn seeded<I>(items: I) -> Self
    where
        I: IntoIterator<Item = StoredWalletSnapshot>,
    {
        let mut wallets_by_id = BTreeMap::new();
        for item in items {
            wallets_by_id.insert(item.id.clone(), item);
        }
        Self::Owned(RwLock::new(wallets_by_id))
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut BTreeMap<String, StoredWalletSnapshot>) -> R) -> R {
        match self {
            Self::Owned(wallets_by_id) => {
                let mut wallets = wallets_by_id.write().expect("settlement repo lock");
                f(&mut wallets)
            }
            Self::Shared(repository) => repository.with_wallets_mut(f),
        }
    }
}

#[derive(Debug, Default)]
pub struct InMemorySettlementRepository {
    wallets: InMemorySettlementWalletStore,
    admissions_by_request_id: RwLock<BTreeMap<String, BillingRequestAdmissionInput>>,
    provider_monthly_used: RwLock<BTreeMap<String, f64>>,
    settlements: RwLock<BTreeMap<String, StoredUsageSettlement>>,
}

impl InMemorySettlementRepository {
    pub fn seed<I>(items: I) -> Self
    where
        I: IntoIterator<Item = StoredWalletSnapshot>,
    {
        Self {
            wallets: InMemorySettlementWalletStore::seeded(items),
            admissions_by_request_id: RwLock::new(BTreeMap::new()),
            provider_monthly_used: RwLock::new(BTreeMap::new()),
            settlements: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn from_wallet_repository(wallet_repository: Arc<InMemoryWalletRepository>) -> Self {
        Self {
            wallets: InMemorySettlementWalletStore::Shared(wallet_repository),
            admissions_by_request_id: RwLock::new(BTreeMap::new()),
            provider_monthly_used: RwLock::new(BTreeMap::new()),
            settlements: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn seed_with_admissions<I, J>(wallets: I, admissions: J) -> Self
    where
        I: IntoIterator<Item = StoredWalletSnapshot>,
        J: IntoIterator<Item = BillingRequestAdmissionInput>,
    {
        let repository = Self::seed(wallets);
        repository.insert_admissions(admissions);
        repository
    }

    pub fn from_wallet_repository_with_admissions<I>(
        wallet_repository: Arc<InMemoryWalletRepository>,
        admissions: I,
    ) -> Self
    where
        I: IntoIterator<Item = BillingRequestAdmissionInput>,
    {
        let repository = Self::from_wallet_repository(wallet_repository);
        repository.insert_admissions(admissions);
        repository
    }

    pub fn insert_admission(&self, admission: BillingRequestAdmissionInput) {
        self.insert_admissions([admission]);
    }

    fn insert_admissions<I>(&self, admissions: I)
    where
        I: IntoIterator<Item = BillingRequestAdmissionInput>,
    {
        let mut stored = self
            .admissions_by_request_id
            .write()
            .expect("billing admission lock");
        for admission in admissions {
            stored.insert(admission.request_id.clone(), admission);
        }
    }
}

#[async_trait]
impl SettlementWriteRepository for InMemorySettlementRepository {
    async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
        input.validate()?;
        if input.billing_status != "pending" {
            let existing = self
                .settlements
                .read()
                .expect("settlement snapshot lock")
                .get(&input.request_id)
                .cloned();
            return Ok(Some(existing.unwrap_or(StoredUsageSettlement {
                request_id: input.request_id,
                wallet_id: None,
                billing_status: input.billing_status,
                wallet_balance_before: None,
                wallet_balance_after: None,
                wallet_recharge_balance_before: None,
                wallet_recharge_balance_after: None,
                wallet_gift_balance_before: None,
                wallet_gift_balance_after: None,
                provider_monthly_used_usd: None,
                finalized_at_unix_secs: input.finalized_at_unix_secs,
            })));
        }

        let final_billing_status =
            settlement_billing_status_for_usage_status(&input.status).to_string();
        let billing_admission = if final_billing_status == "settled" {
            Some(
                self.admissions_by_request_id
                    .read()
                    .expect("billing admission lock")
                    .get(&input.request_id)
                    .cloned()
                    .ok_or_else(|| {
                        DataLayerError::UnexpectedValue(format!(
                            "billing admission missing for request {}",
                            input.request_id
                        ))
                    })?,
            )
        } else {
            None
        };
        if billing_admission
            .as_ref()
            .is_some_and(|admission| admission.funding_source == BillingFundingSource::Plan)
        {
            return Err(DataLayerError::UnexpectedValue(
                "in-memory settlement does not implement plan quota accounting".to_string(),
            ));
        }
        let mut settlement = self.wallets.with_mut(|wallets| {
            let wallet_id = billing_admission
                .as_ref()
                .and_then(|admission| admission.wallet_id.clone());
            let wallet = wallet_id
                .as_deref()
                .and_then(|wallet_id| wallets.get_mut(wallet_id));

            let mut settlement = StoredUsageSettlement {
                request_id: input.request_id.clone(),
                wallet_id: None,
                billing_status: final_billing_status.to_string(),
                wallet_balance_before: None,
                wallet_balance_after: None,
                wallet_recharge_balance_before: None,
                wallet_recharge_balance_after: None,
                wallet_gift_balance_before: None,
                wallet_gift_balance_after: None,
                provider_monthly_used_usd: None,
                finalized_at_unix_secs: input.finalized_at_unix_secs,
            };

            let funding_source = billing_admission
                .as_ref()
                .map(|admission| admission.funding_source);
            let wallet_required = matches!(
                funding_source,
                Some(BillingFundingSource::Wallet | BillingFundingSource::Unlimited)
            );
            if wallet_required && wallet.is_none() {
                return Err(DataLayerError::UnexpectedValue(format!(
                    "billing admission wallet missing for request {}",
                    input.request_id
                )));
            }
            if funding_source == Some(BillingFundingSource::Wallet)
                && billing_admission
                    .as_ref()
                    .is_some_and(|admission| !admission.wallet_payment_allowed)
            {
                return Err(DataLayerError::UnexpectedValue(format!(
                    "wallet payment was not admitted for request {}",
                    input.request_id
                )));
            }

            if let Some(wallet) = wallet {
                let before_recharge = wallet.balance;
                let before_gift = wallet.gift_balance;
                let before_total = before_recharge + before_gift;
                settlement.wallet_id = Some(wallet.id.clone());
                settlement.wallet_balance_before = Some(before_total);
                settlement.wallet_recharge_balance_before = Some(before_recharge);
                settlement.wallet_gift_balance_before = Some(before_gift);

                if final_billing_status == "settled" {
                    if funding_source == Some(BillingFundingSource::Unlimited) {
                        wallet.total_consumed += input.total_cost_usd;
                    } else if funding_source == Some(BillingFundingSource::Wallet) {
                        let debit_plan = plan_finite_wallet_debit(
                            before_recharge,
                            before_gift,
                            input.total_cost_usd,
                        );
                        (wallet.balance, wallet.gift_balance) =
                            debit_plan.after_balances(before_recharge, before_gift);
                        wallet.total_consumed += input.total_cost_usd;
                    }
                }

                settlement.wallet_recharge_balance_after = Some(wallet.balance);
                settlement.wallet_gift_balance_after = Some(wallet.gift_balance);
                settlement.wallet_balance_after = Some(wallet.balance + wallet.gift_balance);
            }

            Ok(settlement)
        })?;

        if final_billing_status == "settled" {
            if let Some(provider_id) = input.provider_id {
                let mut quotas = self
                    .provider_monthly_used
                    .write()
                    .expect("provider quota lock");
                let value = quotas.entry(provider_id).or_insert(0.0);
                *value += input.actual_total_cost_usd;
                settlement.provider_monthly_used_usd = Some(*value);
            }
        }

        self.settlements
            .write()
            .expect("settlement snapshot lock")
            .insert(settlement.request_id.clone(), settlement.clone());

        Ok(Some(settlement))
    }
}

#[cfg(test)]
mod tests {
    use super::InMemorySettlementRepository;
    use crate::repository::settlement::{SettlementWriteRepository, UsageSettlementInput};
    use crate::repository::wallet::StoredWalletSnapshot;
    use aether_data_contracts::repository::billing::{
        BillingFundingSource, BillingRequestAdmissionInput,
    };
    use std::collections::BTreeMap;

    fn sample_wallet() -> StoredWalletSnapshot {
        StoredWalletSnapshot::new(
            "wallet-1".to_string(),
            Some("user-1".to_string()),
            Some("key-1".to_string()),
            10.0,
            2.0,
            "finite".to_string(),
            "USD".to_string(),
            "active".to_string(),
            0.0,
            0.0,
            0.0,
            0.0,
            100,
        )
        .expect("wallet should build")
    }

    fn sample_user_wallet(wallet_id: &str, user_id: &str) -> StoredWalletSnapshot {
        StoredWalletSnapshot::new(
            wallet_id.to_string(),
            Some(user_id.to_string()),
            None,
            10.0,
            2.0,
            "finite".to_string(),
            "USD".to_string(),
            "active".to_string(),
            0.0,
            0.0,
            0.0,
            0.0,
            100,
        )
        .expect("wallet should build")
    }

    fn wallet_admission(
        request_id: &str,
        wallet_id: &str,
        wallet_balance: f64,
    ) -> BillingRequestAdmissionInput {
        BillingRequestAdmissionInput {
            request_id: request_id.to_string(),
            user_id: Some("user-1".to_string()),
            api_key_id: Some("key-1".to_string()),
            wallet_id: Some(wallet_id.to_string()),
            global_model_id: Some("global-model-1".to_string()),
            funding_source: BillingFundingSource::Wallet,
            wallet_balance_at_admission: Some(wallet_balance),
            wallet_payment_allowed: wallet_balance > 0.0,
            wallet_overage_allowed: false,
            entitlement_ids: Vec::new(),
            entitlement_provider_scopes: BTreeMap::new(),
            allowed_provider_ids: Vec::new(),
            schema_version: 1,
        }
    }

    #[tokio::test]
    async fn settles_usage_against_wallet_and_provider_quota() {
        let repository = InMemorySettlementRepository::seed_with_admissions(
            vec![sample_wallet()],
            vec![wallet_admission("req-1", "wallet-1", 12.0)],
        );
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "req-1".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("key-1".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 1.5,
                finalized_at_unix_secs: Some(200),
            })
            .await
            .expect("settlement should succeed")
            .expect("settlement should exist");

        assert_eq!(settlement.billing_status, "settled");
        assert_eq!(settlement.wallet_balance_before, Some(12.0));
        assert_eq!(settlement.wallet_balance_after, Some(9.0));
        assert_eq!(settlement.provider_monthly_used_usd, Some(1.5));
    }

    #[tokio::test]
    async fn wallet_settlement_uses_the_wallet_saved_at_request_start() {
        let repository = InMemorySettlementRepository::seed_with_admissions(
            vec![sample_user_wallet("wallet-user-1", "user-1")],
            vec![wallet_admission("req-user-wallet", "wallet-user-1", 12.0)],
        );
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "req-user-wallet".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("normal-key-without-wallet".to_string()),
                api_key_is_standalone: false,
                provider_id: None,
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 1.5,
                finalized_at_unix_secs: Some(200),
            })
            .await
            .expect("settlement should succeed")
            .expect("settlement should exist");

        assert_eq!(settlement.wallet_id.as_deref(), Some("wallet-user-1"));
        assert_eq!(settlement.wallet_balance_before, Some(12.0));
        assert_eq!(settlement.wallet_balance_after, Some(9.0));
    }

    #[tokio::test]
    async fn settles_cancelled_usage_against_wallet_and_provider_quota() {
        let repository = InMemorySettlementRepository::seed_with_admissions(
            vec![sample_wallet()],
            vec![wallet_admission("req-cancelled", "wallet-1", 12.0)],
        );
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "req-cancelled".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("key-1".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "cancelled".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 1.5,
                finalized_at_unix_secs: Some(200),
            })
            .await
            .expect("settlement should succeed")
            .expect("settlement should exist");

        assert_eq!(settlement.billing_status, "settled");
        assert_eq!(settlement.wallet_balance_before, Some(12.0));
        assert_eq!(settlement.wallet_balance_after, Some(9.0));
        assert_eq!(settlement.provider_monthly_used_usd, Some(1.5));
    }

    #[tokio::test]
    async fn standalone_key_settlement_never_falls_back_to_owner_wallet() {
        let repository = InMemorySettlementRepository::seed_with_admissions(
            vec![sample_user_wallet("wallet-admin-owner", "admin-owner")],
            vec![wallet_admission(
                "req-standalone-no-key-wallet",
                "missing-key-wallet",
                1.0,
            )],
        );
        let result = repository
            .settle_usage(UsageSettlementInput {
                request_id: "req-standalone-no-key-wallet".to_string(),
                user_id: Some("admin-owner".to_string()),
                api_key_id: Some("standalone-key-without-wallet".to_string()),
                api_key_is_standalone: true,
                provider_id: None,
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 3.0,
                total_cost_usd: 3.0,
                actual_total_cost_usd: 1.5,
                finalized_at_unix_secs: Some(200),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn finite_wallet_insufficient_balance_overdraws_and_settles() {
        let repository = InMemorySettlementRepository::seed_with_admissions(
            vec![sample_wallet()],
            vec![wallet_admission(
                "req-insufficient-wallet",
                "wallet-1",
                12.0,
            )],
        );
        let settlement = repository
            .settle_usage(UsageSettlementInput {
                request_id: "req-insufficient-wallet".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("key-1".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 15.0,
                total_cost_usd: 15.0,
                actual_total_cost_usd: 7.5,
                finalized_at_unix_secs: Some(200),
            })
            .await
            .expect("settlement should succeed")
            .expect("settlement should exist");

        assert_eq!(settlement.billing_status, "settled");
        assert_eq!(settlement.wallet_balance_before, Some(12.0));
        assert_eq!(settlement.wallet_balance_after, Some(-3.0));
        assert_eq!(settlement.wallet_recharge_balance_after, Some(-3.0));
        assert_eq!(settlement.wallet_gift_balance_after, Some(0.0));
        assert_eq!(settlement.provider_monthly_used_usd, Some(7.5));
    }

    #[tokio::test]
    async fn returns_stored_snapshot_when_usage_is_already_finalized() {
        let repository = InMemorySettlementRepository::seed_with_admissions(
            vec![sample_wallet()],
            vec![wallet_admission("req-2", "wallet-1", 12.0)],
        );
        let settled = repository
            .settle_usage(UsageSettlementInput {
                request_id: "req-2".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("key-1".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "pending".to_string(),
                base_cost_usd: 2.0,
                total_cost_usd: 2.0,
                actual_total_cost_usd: 1.0,
                finalized_at_unix_secs: Some(250),
            })
            .await
            .expect("settlement should succeed")
            .expect("settlement should exist");

        let replay = repository
            .settle_usage(UsageSettlementInput {
                request_id: "req-2".to_string(),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("key-1".to_string()),
                api_key_is_standalone: false,
                provider_id: Some("provider-1".to_string()),
                global_model_id: None,
                global_model_name: None,
                model: None,
                status: "completed".to_string(),
                billing_status: "settled".to_string(),
                base_cost_usd: 2.0,
                total_cost_usd: 2.0,
                actual_total_cost_usd: 1.0,
                finalized_at_unix_secs: Some(250),
            })
            .await
            .expect("replay should succeed")
            .expect("snapshot should exist");

        assert_eq!(replay, settled);
    }
}

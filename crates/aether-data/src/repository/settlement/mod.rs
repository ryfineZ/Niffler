mod memory;
mod postgres;

const SETTLEMENT_EPSILON_USD: f64 = 0.000_000_01;

#[derive(Debug, Clone)]
struct SettlementBillingAdmission {
    funding_source: aether_data_contracts::repository::billing::BillingFundingSource,
    wallet_id: Option<String>,
    wallet_payment_allowed: bool,
    wallet_overage_allowed: bool,
    entitlement_ids: Vec<String>,
    entitlement_provider_scopes: std::collections::BTreeMap<String, Vec<String>>,
    allowed_provider_ids: Vec<String>,
}

impl SettlementBillingAdmission {
    fn uses_plan_for_provider(&self, provider_id: Option<&str>) -> bool {
        self.funding_source
            == aether_data_contracts::repository::billing::BillingFundingSource::Plan
            && provider_id.is_some_and(|provider_id| {
                self.entitlement_provider_scopes.values().any(Vec::is_empty)
                    || (self
                        .allowed_provider_ids
                        .iter()
                        .any(|allowed| allowed == provider_id)
                        && self
                            .entitlement_provider_scopes
                            .values()
                            .any(|provider_ids| {
                                provider_ids.iter().any(|allowed| allowed == provider_id)
                            }))
            })
    }

    fn plan_allows_provider(&self, provider_id: Option<&str>) -> bool {
        self.funding_source
            != aether_data_contracts::repository::billing::BillingFundingSource::Plan
            || self.uses_plan_for_provider(provider_id)
    }

    fn wallet_can_overdraft(&self) -> bool {
        match self.funding_source {
            aether_data_contracts::repository::billing::BillingFundingSource::Wallet => {
                self.wallet_payment_allowed
            }
            aether_data_contracts::repository::billing::BillingFundingSource::Plan => {
                self.wallet_overage_allowed
            }
            _ => false,
        }
    }

    fn entitlement_ids_for_provider(&self, provider_id: Option<&str>) -> Vec<String> {
        let Some(provider_id) = provider_id else {
            return Vec::new();
        };
        self.entitlement_ids
            .iter()
            .filter(|entitlement_id| {
                self.entitlement_provider_scopes
                    .get(*entitlement_id)
                    .is_some_and(|provider_ids| {
                        provider_ids.is_empty()
                            || provider_ids.iter().any(|allowed| allowed == provider_id)
                    })
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct WalletDebitPlan {
    recharge_deduction: f64,
    gift_deduction: f64,
    recharge_overdraft: f64,
}

impl WalletDebitPlan {
    fn after_balances(self, recharge_balance: f64, gift_balance: f64) -> (f64, f64) {
        (
            recharge_balance - self.recharge_deduction - self.recharge_overdraft,
            gift_balance - self.gift_deduction,
        )
    }
}

#[cfg(test)]
mod admission_tests {
    use super::SettlementBillingAdmission;
    use aether_data_contracts::repository::billing::BillingFundingSource;
    use std::collections::BTreeMap;

    fn provider_scoped_plan_admission() -> SettlementBillingAdmission {
        SettlementBillingAdmission {
            funding_source: BillingFundingSource::Plan,
            wallet_id: Some("wallet-1".to_string()),
            wallet_payment_allowed: true,
            wallet_overage_allowed: true,
            entitlement_ids: vec!["entitlement-1".to_string()],
            entitlement_provider_scopes: BTreeMap::from([(
                "entitlement-1".to_string(),
                vec!["provider-plan".to_string()],
            )]),
            allowed_provider_ids: vec!["provider-plan".to_string()],
        }
    }

    #[test]
    fn provider_scoped_plan_never_changes_to_wallet_for_an_outside_provider() {
        let admission = provider_scoped_plan_admission();

        assert!(admission.plan_allows_provider(Some("provider-plan")));
        assert!(!admission.plan_allows_provider(Some("provider-wallet")));
    }

    #[test]
    fn legacy_saved_admission_still_settles_after_the_live_scope_update() {
        let mut admission = provider_scoped_plan_admission();
        admission.allowed_provider_ids.clear();
        admission
            .entitlement_provider_scopes
            .insert("entitlement-1".to_string(), Vec::new());

        assert!(admission.plan_allows_provider(Some("provider-used-before-update")));
        assert_eq!(
            admission.entitlement_ids_for_provider(Some("provider-used-before-update")),
            vec!["entitlement-1".to_string()]
        );
    }
}

fn finite_wallet_available_usd(recharge_balance: f64, gift_balance: f64) -> f64 {
    recharge_balance.max(0.0) + gift_balance.max(0.0)
}

fn plan_finite_wallet_debit(
    recharge_balance: f64,
    gift_balance: f64,
    requested_usd: f64,
) -> WalletDebitPlan {
    let requested_usd = requested_usd.max(0.0);
    let recharge_deduction = recharge_balance.max(0.0).min(requested_usd);
    let after_recharge_remaining = (requested_usd - recharge_deduction).max(0.0);
    let gift_deduction = gift_balance.max(0.0).min(after_recharge_remaining);
    let recharge_overdraft = (after_recharge_remaining - gift_deduction).max(0.0);
    WalletDebitPlan {
        recharge_deduction,
        gift_deduction,
        recharge_overdraft,
    }
}

fn settlement_billing_status_for_usage_status(status: &str) -> &'static str {
    match status {
        "completed" | "cancelled" => "settled",
        _ => "void",
    }
}

fn settlement_wallet_charge_multiplier(input: &UsageSettlementInput) -> f64 {
    if input.base_cost_usd > SETTLEMENT_EPSILON_USD
        && input.total_cost_usd.is_finite()
        && input.base_cost_usd.is_finite()
    {
        (input.total_cost_usd / input.base_cost_usd).max(0.0)
    } else {
        1.0
    }
}

#[allow(unused_imports)]
pub(crate) use aether_data_contracts::repository::settlement::{
    SettlementRepository, SettlementWriteRepository, StoredUsageSettlement, UsageSettlementInput,
};
pub use memory::InMemorySettlementRepository;
pub use postgres::SqlxSettlementRepository;

#[cfg(test)]
mod tests {
    use super::settlement_billing_status_for_usage_status;

    #[test]
    fn cancelled_usage_status_is_billable() {
        assert_eq!(
            settlement_billing_status_for_usage_status("completed"),
            "settled"
        );
        assert_eq!(
            settlement_billing_status_for_usage_status("cancelled"),
            "settled"
        );
        assert_eq!(settlement_billing_status_for_usage_status("failed"), "void");
    }
}

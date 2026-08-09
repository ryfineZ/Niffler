use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalletLimitMode {
    Finite,
    Unlimited,
}

impl WalletLimitMode {
    pub fn parse(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("unlimited") {
            Self::Unlimited
        } else {
            Self::Finite
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalletStatus {
    Active,
    Inactive,
}

impl WalletStatus {
    pub fn parse(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("active") {
            Self::Active
        } else {
            Self::Inactive
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalletSnapshot {
    pub wallet_id: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub recharge_balance: f64,
    pub gift_balance: f64,
    pub limit_mode: WalletLimitMode,
    pub currency: String,
    pub status: WalletStatus,
}

impl WalletSnapshot {
    pub fn spendable_balance(&self) -> f64 {
        quantize_money(self.recharge_balance + self.gift_balance)
    }

    pub fn refundable_balance(&self) -> f64 {
        quantize_money(self.recharge_balance)
    }

    pub fn balance_snapshot(&self) -> Option<f64> {
        let signed_balance = self.spendable_balance();
        if signed_balance < 0.0 {
            return Some(signed_balance);
        }
        match self.limit_mode {
            WalletLimitMode::Unlimited => None,
            WalletLimitMode::Finite => Some(signed_balance),
        }
    }

    pub fn access_decision(&self, _is_admin: bool) -> WalletAccessDecision {
        if self.status != WalletStatus::Active {
            return WalletAccessDecision::wallet_unavailable(self.balance_snapshot());
        }
        let remaining = self.spendable_balance();
        if remaining < 0.0 {
            return WalletAccessDecision::balance_denied(Some(remaining));
        }
        if self.limit_mode == WalletLimitMode::Unlimited {
            return WalletAccessDecision::allowed(None);
        }
        if remaining <= 0.0 {
            return WalletAccessDecision::balance_denied(Some(remaining));
        }
        WalletAccessDecision::allowed(Some(remaining))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalletAccessFailure {
    WalletUnavailable,
    BalanceDenied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalletAccessDecision {
    pub allowed: bool,
    pub remaining: Option<f64>,
    pub failure: Option<WalletAccessFailure>,
}

impl WalletAccessDecision {
    pub fn allowed(remaining: Option<f64>) -> Self {
        Self {
            allowed: true,
            remaining,
            failure: None,
        }
    }

    pub fn wallet_unavailable(remaining: Option<f64>) -> Self {
        Self {
            allowed: false,
            remaining,
            failure: Some(WalletAccessFailure::WalletUnavailable),
        }
    }

    pub fn balance_denied(remaining: Option<f64>) -> Self {
        Self {
            allowed: false,
            remaining,
            failure: Some(WalletAccessFailure::BalanceDenied),
        }
    }
}

pub fn quantize_money(value: f64) -> f64 {
    (value * 100_000_000.0).round() / 100_000_000.0
}

#[cfg(test)]
mod tests {
    use super::{WalletAccessFailure, WalletLimitMode, WalletSnapshot, WalletStatus};

    fn wallet_snapshot(limit_mode: WalletLimitMode, recharge: f64, gift: f64) -> WalletSnapshot {
        WalletSnapshot {
            wallet_id: "wallet-1".to_string(),
            user_id: Some("user-1".to_string()),
            api_key_id: None,
            recharge_balance: recharge,
            gift_balance: gift,
            limit_mode,
            currency: "USD".to_string(),
            status: WalletStatus::Active,
        }
    }

    #[test]
    fn finite_wallet_denies_empty_balance() {
        let decision = wallet_snapshot(WalletLimitMode::Finite, 0.0, 0.0).access_decision(false);
        assert!(!decision.allowed);
        assert_eq!(decision.failure, Some(WalletAccessFailure::BalanceDenied));
        assert_eq!(decision.remaining, Some(0.0));
    }

    #[test]
    fn finite_wallet_uses_the_signed_sum_of_recharge_and_gift_balances() {
        let decision = wallet_snapshot(WalletLimitMode::Finite, -10.0, 20.0).access_decision(false);
        assert!(decision.allowed);
        assert_eq!(decision.failure, None);
        assert_eq!(decision.remaining, Some(10.0));
    }

    #[test]
    fn unlimited_wallet_ignores_balance() {
        let decision =
            wallet_snapshot(WalletLimitMode::Unlimited, -10.0, 0.0).access_decision(false);
        assert!(!decision.allowed);
        assert_eq!(decision.failure, Some(WalletAccessFailure::BalanceDenied));

        let decision = wallet_snapshot(WalletLimitMode::Unlimited, 0.0, 0.0).access_decision(false);
        assert!(decision.allowed);
        assert_eq!(decision.remaining, None);
    }

    #[test]
    fn inactive_wallet_is_unavailable() {
        let mut wallet = wallet_snapshot(WalletLimitMode::Finite, 10.0, 2.0);
        wallet.status = WalletStatus::Inactive;
        let decision = wallet.access_decision(false);
        assert!(!decision.allowed);
        assert_eq!(
            decision.failure,
            Some(WalletAccessFailure::WalletUnavailable)
        );
    }
}

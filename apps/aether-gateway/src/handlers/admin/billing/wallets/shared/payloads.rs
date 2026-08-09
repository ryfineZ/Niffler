use crate::handlers::admin::request::AdminAppState;
use crate::handlers::admin::shared::unix_secs_to_rfc3339;
use crate::handlers::shared::round_to;
use crate::GatewayError;
use serde_json::json;

#[derive(Clone)]
pub(in super::super) struct AdminWalletOwnerSummary {
    pub(in super::super) owner_type: &'static str,
    pub(in super::super) owner_name: Option<String>,
}

pub(in super::super) fn build_admin_wallet_payment_order_payload(
    order: &crate::AdminWalletPaymentOrderRecord,
) -> serde_json::Value {
    json!({
        "id": order.id,
        "order_no": order.order_no,
        "wallet_id": order.wallet_id,
        "user_id": order.user_id,
        "amount_usd": order.amount_usd,
        "pay_amount": order.pay_amount,
        "pay_currency": order.pay_currency,
        "exchange_rate": order.exchange_rate,
        "refunded_amount_usd": order.refunded_amount_usd,
        "refundable_amount_usd": order.refundable_amount_usd,
        "payment_method": order.payment_method,
        "payment_provider": order.payment_provider,
        "payment_channel": order.payment_channel,
        "order_kind": order.order_kind,
        "product_id": order.product_id,
        "product_snapshot": order.product_snapshot,
        "fulfillment_status": order.fulfillment_status,
        "fulfillment_error": order.fulfillment_error,
        "gateway_order_id": order.gateway_order_id,
        "gateway_response": order.gateway_response,
        "status": order.status,
        "created_at": unix_secs_to_rfc3339(order.created_at_unix_ms),
        "paid_at": order.paid_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "credited_at": order.credited_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "expires_at": order.expires_at_unix_secs.and_then(unix_secs_to_rfc3339),
    })
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn build_admin_wallet_transaction_payload(
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
    owner: &AdminWalletOwnerSummary,
    transaction_id: String,
    category: &str,
    reason_code: &str,
    amount: f64,
    balance_before: f64,
    balance_after: f64,
    recharge_balance_before: f64,
    recharge_balance_after: f64,
    gift_balance_before: f64,
    gift_balance_after: f64,
    link_type: Option<&str>,
    link_id: Option<&str>,
    operator_id: Option<&str>,
    description: Option<&str>,
    created_at: Option<String>,
) -> serde_json::Value {
    json!({
        "id": transaction_id,
        "wallet_id": wallet.id,
        "owner_type": owner.owner_type,
        "owner_name": owner.owner_name.clone(),
        "wallet_status": wallet.status,
        "category": category,
        "reason_code": reason_code,
        "amount": amount,
        "balance_before": balance_before,
        "balance_after": balance_after,
        "recharge_balance_before": recharge_balance_before,
        "recharge_balance_after": recharge_balance_after,
        "gift_balance_before": gift_balance_before,
        "gift_balance_after": gift_balance_after,
        "link_type": link_type,
        "link_id": link_id,
        "operator_id": operator_id,
        "operator_name": serde_json::Value::Null,
        "operator_email": serde_json::Value::Null,
        "description": description,
        "created_at": created_at,
    })
}

pub(in super::super) fn wallet_owner_summary_from_fields(
    user_id: Option<&str>,
    user_name: Option<String>,
    api_key_id: Option<&str>,
    api_key_name: Option<String>,
) -> AdminWalletOwnerSummary {
    if user_id.is_some() {
        return AdminWalletOwnerSummary {
            owner_type: "user",
            owner_name: user_name,
        };
    }
    if let Some(api_key_id) = api_key_id {
        return AdminWalletOwnerSummary {
            owner_type: "api_key",
            owner_name: api_key_name
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(format!("Key-{}", &api_key_id[..api_key_id.len().min(8)]))),
        };
    }
    AdminWalletOwnerSummary {
        owner_type: "orphaned",
        owner_name: None,
    }
}

pub(in super::super) async fn resolve_admin_wallet_owner_summary(
    state: &AdminAppState<'_>,
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
) -> Result<AdminWalletOwnerSummary, GatewayError> {
    if let Some(user_id) = wallet.user_id.as_deref() {
        let user = state.find_user_auth_by_id(user_id).await?;
        Ok(AdminWalletOwnerSummary {
            owner_type: "user",
            owner_name: user.map(|record| record.username),
        })
    } else if let Some(api_key_id) = wallet.api_key_id.as_deref() {
        let api_key_ids = vec![api_key_id.to_string()];
        let snapshots = state
            .list_auth_api_key_snapshots_by_ids(&api_key_ids)
            .await?;
        let owner_name = snapshots
            .into_iter()
            .find(|snapshot| snapshot.api_key_id == api_key_id)
            .and_then(|snapshot| snapshot.api_key_name)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some(format!("Key-{}", &api_key_id[..api_key_id.len().min(8)])));
        Ok(AdminWalletOwnerSummary {
            owner_type: "api_key",
            owner_name,
        })
    } else {
        Ok(AdminWalletOwnerSummary {
            owner_type: "orphaned",
            owner_name: None,
        })
    }
}

pub(in super::super) fn build_admin_wallet_summary_payload(
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
    owner: &AdminWalletOwnerSummary,
) -> serde_json::Value {
    json!({
        "id": wallet.id.clone(),
        "user_id": wallet.user_id.clone(),
        "api_key_id": wallet.api_key_id.clone(),
        "owner_type": owner.owner_type,
        "owner_name": owner.owner_name.clone(),
        "balance": wallet.balance + wallet.gift_balance,
        "recharge_balance": wallet.balance,
        "gift_balance": wallet.gift_balance,
        "refundable_balance": wallet.balance,
        "currency": wallet.currency.clone(),
        "status": wallet.status.clone(),
        "limit_mode": wallet.limit_mode.clone(),
        "unlimited": wallet.limit_mode.eq_ignore_ascii_case("unlimited"),
        "total_recharged": wallet.total_recharged,
        "total_consumed": wallet.total_consumed,
        "total_refunded": wallet.total_refunded,
        "total_adjusted": wallet.total_adjusted,
        "created_at": serde_json::Value::Null,
        "updated_at": unix_secs_to_rfc3339(wallet.updated_at_unix_secs),
    })
}

pub(in super::super) async fn build_admin_wallet_summary_payload_with_package(
    state: &AdminAppState<'_>,
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
    owner: &AdminWalletOwnerSummary,
) -> Result<serde_json::Value, GatewayError> {
    let mut payload = build_admin_wallet_summary_payload(wallet, owner);
    enrich_admin_wallet_package_summary(
        state,
        &mut payload,
        wallet.user_id.as_deref(),
        wallet.balance + wallet.gift_balance,
        wallet.limit_mode.eq_ignore_ascii_case("unlimited"),
    )
    .await?;
    Ok(payload)
}

pub(in super::super) async fn enrich_admin_wallet_package_summary(
    state: &AdminAppState<'_>,
    payload: &mut serde_json::Value,
    user_id: Option<&str>,
    wallet_balance: f64,
    unlimited: bool,
) -> Result<(), GatewayError> {
    let daily_quota = match user_id {
        Some(user_id) if !user_id.trim().is_empty() => {
            state
                .app()
                .find_user_daily_quota_availability(user_id)
                .await?
        }
        _ => None,
    };
    let (has_active_daily_quota, total_quota_usd, used_usd, remaining_usd, allow_wallet_overage) =
        daily_quota
            .map(|quota| {
                (
                    quota.has_active_daily_quota,
                    quota.total_quota_usd,
                    quota.used_usd,
                    quota.remaining_usd,
                    quota.allow_wallet_overage,
                )
            })
            .unwrap_or((false, 0.0, 0.0, 0.0, false));
    let package_balance = if has_active_daily_quota {
        remaining_usd.max(0.0)
    } else {
        0.0
    };
    let spendable_wallet_balance = wallet_balance.max(0.0);
    let debt_usd = (-wallet_balance).max(0.0);
    let billing_state = if unlimited {
        "unlimited"
    } else if wallet_balance < 0.0 {
        "in_debt"
    } else {
        "active"
    };

    payload["daily_quota"] = json!({
        "has_active": has_active_daily_quota,
        "total_usd": round_to(total_quota_usd.max(0.0), 6),
        "used_usd": round_to(used_usd.max(0.0), 6),
        "remaining_usd": round_to(package_balance, 6),
        "allow_wallet_overage": allow_wallet_overage,
    });
    payload["package_balance"] = json!(round_to(package_balance, 6));
    payload["actual_wallet_balance"] = json!(round_to(wallet_balance, 6));
    payload["spendable_wallet_balance"] = json!(round_to(spendable_wallet_balance, 6));
    payload["debt_usd"] = json!(round_to(debt_usd, 6));
    payload["billing_state"] = json!(billing_state);
    payload["wallet_balance"] = json!(round_to(wallet_balance, 6));
    payload["total_available_balance"] = if unlimited {
        serde_json::Value::Null
    } else {
        json!(round_to(spendable_wallet_balance + package_balance, 6))
    };
    payload["deduction_order"] = json!([
        "package_daily_quota",
        "wallet_recharge_balance",
        "wallet_gift_balance"
    ]);
    Ok(())
}

pub(in super::super) fn build_admin_wallet_refund_payload(
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
    owner: &AdminWalletOwnerSummary,
    refund: &crate::AdminWalletRefundRecord,
) -> serde_json::Value {
    json!({
        "id": refund.id.clone(),
        "refund_no": refund.refund_no.clone(),
        "wallet_id": refund.wallet_id.clone(),
        "owner_type": owner.owner_type,
        "owner_name": owner.owner_name.clone(),
        "wallet_status": wallet.status.clone(),
        "user_id": refund.user_id.clone(),
        "payment_order_id": refund.payment_order_id.clone(),
        "source_type": refund.source_type.clone(),
        "source_id": refund.source_id.clone(),
        "refund_mode": refund.refund_mode.clone(),
        "amount_usd": refund.amount_usd,
        "status": refund.status.clone(),
        "reason": refund.reason.clone(),
        "failure_reason": refund.failure_reason.clone(),
        "gateway_refund_id": refund.gateway_refund_id.clone(),
        "payout_method": refund.payout_method.clone(),
        "payout_reference": refund.payout_reference.clone(),
        "payout_proof": refund.payout_proof.clone(),
        "requested_by": refund.requested_by.clone(),
        "approved_by": refund.approved_by.clone(),
        "processed_by": refund.processed_by.clone(),
        "created_at": unix_secs_to_rfc3339(refund.created_at_unix_ms),
        "updated_at": unix_secs_to_rfc3339(refund.updated_at_unix_secs),
        "processed_at": refund.processed_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "completed_at": refund.completed_at_unix_secs.and_then(unix_secs_to_rfc3339),
    })
}

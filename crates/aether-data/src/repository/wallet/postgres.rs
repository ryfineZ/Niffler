use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use futures_util::{stream::TryStream, TryStreamExt};
use sqlx::{postgres::PgRow, PgPool, Row};
use uuid::Uuid;

use super::plan_overrides::{
    admin_grant_expires_at_unix_secs, admin_grant_starts_at_unix_secs,
    entitlements_with_admin_grant_overrides, plan_provider_ids_snapshot,
};
use super::types::{
    payment_callback_amount_matches, payment_callback_currency_matches,
    payment_callback_settled_amount_usd, plan_purchase_payment_amounts,
    redeem_code_credits_recharge_balance, redeem_code_payment_method,
    redeem_code_refundable_amount, AdjustWalletBalanceInput, AdminPaymentOrderListQuery,
    AdminRedeemCodeBatchListQuery, AdminRedeemCodeListQuery, AdminWalletLedgerQuery,
    AdminWalletListQuery, AdminWalletRefundRequestListQuery, CancelPaymentOrderInput,
    CompleteAdminWalletRefundInput, CreateAdminRedeemCodeBatchInput,
    CreateAdminRedeemCodeBatchResult, CreateManualWalletRechargeInput,
    CreatePlanPurchaseOrderInput, CreatePlanPurchaseOrderOutcome, CreateWalletRechargeOrderInput,
    CreateWalletRechargeOrderOutcome, CreateWalletRefundRequestInput,
    CreateWalletRefundRequestOutcome, CreatedAdminRedeemCodePlaintext,
    CreditAdminPaymentOrderInput, DeleteAdminRedeemCodeBatchInput,
    DisableAdminRedeemCodeBatchInput, DisableAdminRedeemCodeInput, FailAdminWalletRefundInput,
    ProcessAdminWalletRefundInput, ProcessPaymentCallbackInput, ProcessPaymentCallbackOutcome,
    RedeemWalletCodeInput, RedeemWalletCodeOutcome, StoredAdminPaymentCallback,
    StoredAdminPaymentCallbackPage, StoredAdminPaymentOrder, StoredAdminPaymentOrderPage,
    StoredAdminRedeemCode, StoredAdminRedeemCodeBatch, StoredAdminRedeemCodeBatchPage,
    StoredAdminRedeemCodePage, StoredAdminWalletLedgerItem, StoredAdminWalletLedgerPage,
    StoredAdminWalletListItem, StoredAdminWalletListPage, StoredAdminWalletRefund,
    StoredAdminWalletRefundPage, StoredAdminWalletRefundRequestItem,
    StoredAdminWalletRefundRequestPage, StoredAdminWalletTransaction,
    StoredAdminWalletTransactionPage, StoredWalletDailyUsageLedger,
    StoredWalletDailyUsageLedgerPage, StoredWalletSnapshot, UpdatePendingPaymentOrderGatewayInput,
    WalletLookupKey, WalletMutationOutcome, WalletReadRepository, WalletWriteRepository,
};
use crate::{
    driver::postgres::PostgresTransactionRunner,
    error::{postgres_error, SqlxResultExt},
    DataLayerError,
};

const FIND_BY_WALLET_ID_SQL: &str = r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE id = $1
LIMIT 1
"#;

const FIND_BY_USER_ID_SQL: &str = r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE user_id = $1
LIMIT 1
"#;

const FIND_BY_API_KEY_ID_SQL: &str = r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE api_key_id = $1
LIMIT 1
"#;

const ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS: &str = r#"
  id,
  order_no,
  wallet_id,
  user_id,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  CAST(debt_repayment_usd AS DOUBLE PRECISION) AS debt_repayment_usd,
  CAST(pay_amount AS DOUBLE PRECISION) AS pay_amount,
  pay_currency,
  CAST(exchange_rate AS DOUBLE PRECISION) AS exchange_rate,
  CAST(refunded_amount_usd AS DOUBLE PRECISION) AS refunded_amount_usd,
  CAST(refundable_amount_usd AS DOUBLE PRECISION) AS refundable_amount_usd,
  payment_method,
  payment_provider,
  payment_channel,
  order_kind,
  product_id,
  product_snapshot,
  fulfillment_status,
  fulfillment_error,
  gateway_order_id,
  gateway_response,
  status,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM paid_at) AS BIGINT) AS paid_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM credited_at) AS BIGINT) AS credited_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM expires_at) AS BIGINT) AS expires_at_unix_secs
"#;

const LIST_BY_USER_IDS_SQL: &str = r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE user_id = ANY($1)
"#;

const LIST_BY_API_KEY_IDS_SQL: &str = r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE api_key_id = ANY($1)
"#;

const COUNT_ADMIN_WALLETS_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM wallets w
LEFT JOIN users ON users.id = w.user_id
LEFT JOIN api_keys ON api_keys.id = w.api_key_id
WHERE ($1::TEXT IS NULL OR w.status = $1)
  AND (
    $2::TEXT IS NULL
    OR ($2 = 'user' AND w.user_id IS NOT NULL)
    OR ($2 = 'api_key' AND w.api_key_id IS NOT NULL)
  )
  AND (
    NULLIF(BTRIM($3::TEXT), '') IS NULL
    OR users.username ILIKE '%' || BTRIM($3::TEXT) || '%'
    OR users.email ILIKE '%' || BTRIM($3::TEXT) || '%'
    OR w.user_id ILIKE '%' || BTRIM($3::TEXT) || '%'
    OR api_keys.name ILIKE '%' || BTRIM($3::TEXT) || '%'
    OR w.api_key_id ILIKE '%' || BTRIM($3::TEXT) || '%'
  )
"#;

const LIST_ADMIN_WALLETS_SQL: &str = r#"
SELECT
  w.id,
  w.user_id,
  w.api_key_id,
  CAST(w.balance AS DOUBLE PRECISION) AS balance,
  CAST(w.gift_balance AS DOUBLE PRECISION) AS gift_balance,
  w.limit_mode,
  w.currency,
  w.status,
  CAST(w.total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(w.total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(w.total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(w.total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  users.username AS user_name,
  api_keys.name AS api_key_name,
  CAST(EXTRACT(EPOCH FROM w.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM w.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets w
LEFT JOIN users ON users.id = w.user_id
LEFT JOIN api_keys ON api_keys.id = w.api_key_id
WHERE ($1::TEXT IS NULL OR w.status = $1)
  AND (
    $2::TEXT IS NULL
    OR ($2 = 'user' AND w.user_id IS NOT NULL)
    OR ($2 = 'api_key' AND w.api_key_id IS NOT NULL)
  )
  AND (
    NULLIF(BTRIM($3::TEXT), '') IS NULL
    OR users.username ILIKE '%' || BTRIM($3::TEXT) || '%'
    OR users.email ILIKE '%' || BTRIM($3::TEXT) || '%'
    OR w.user_id ILIKE '%' || BTRIM($3::TEXT) || '%'
    OR api_keys.name ILIKE '%' || BTRIM($3::TEXT) || '%'
    OR w.api_key_id ILIKE '%' || BTRIM($3::TEXT) || '%'
  )
ORDER BY w.updated_at DESC
OFFSET $4
LIMIT $5
"#;

const COUNT_ADMIN_WALLET_LEDGER_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM wallet_transactions tx
JOIN wallets w ON w.id = tx.wallet_id
LEFT JOIN users wallet_users ON wallet_users.id = w.user_id
LEFT JOIN api_keys ON api_keys.id = w.api_key_id
WHERE ($1::TEXT IS NULL OR tx.category = $1)
  AND ($2::TEXT IS NULL OR tx.reason_code = $2)
  AND (
    $3::TEXT IS NULL
    OR ($3 = 'user' AND w.user_id IS NOT NULL)
    OR ($3 = 'api_key' AND w.api_key_id IS NOT NULL)
  )
  AND (
    NULLIF(BTRIM($4::TEXT), '') IS NULL
    OR wallet_users.username ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR wallet_users.email ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR w.user_id ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR api_keys.name ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR w.api_key_id ILIKE '%' || BTRIM($4::TEXT) || '%'
  )
"#;

const LIST_ADMIN_WALLET_LEDGER_SQL: &str = r#"
SELECT
  tx.id,
  tx.wallet_id,
  tx.category,
  tx.reason_code,
  CAST(tx.amount AS DOUBLE PRECISION) AS amount,
  CAST(tx.balance_before AS DOUBLE PRECISION) AS balance_before,
  CAST(tx.balance_after AS DOUBLE PRECISION) AS balance_after,
  CAST(tx.recharge_balance_before AS DOUBLE PRECISION) AS recharge_balance_before,
  CAST(tx.recharge_balance_after AS DOUBLE PRECISION) AS recharge_balance_after,
  CAST(tx.gift_balance_before AS DOUBLE PRECISION) AS gift_balance_before,
  CAST(tx.gift_balance_after AS DOUBLE PRECISION) AS gift_balance_after,
  tx.link_type,
  tx.link_id,
  tx.operator_id,
  tx.description,
  w.user_id,
  w.api_key_id,
  w.status AS wallet_status,
  wallet_users.username AS wallet_user_name,
  api_keys.name AS api_key_name,
  operator_users.username AS operator_name,
  operator_users.email AS operator_email,
  CAST(EXTRACT(EPOCH FROM tx.created_at) AS BIGINT) AS created_at_unix_ms
FROM wallet_transactions tx
JOIN wallets w ON w.id = tx.wallet_id
LEFT JOIN users wallet_users ON wallet_users.id = w.user_id
LEFT JOIN api_keys ON api_keys.id = w.api_key_id
LEFT JOIN users operator_users ON operator_users.id = tx.operator_id
WHERE ($1::TEXT IS NULL OR tx.category = $1)
  AND ($2::TEXT IS NULL OR tx.reason_code = $2)
  AND (
    $3::TEXT IS NULL
    OR ($3 = 'user' AND w.user_id IS NOT NULL)
    OR ($3 = 'api_key' AND w.api_key_id IS NOT NULL)
  )
  AND (
    NULLIF(BTRIM($4::TEXT), '') IS NULL
    OR wallet_users.username ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR wallet_users.email ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR w.user_id ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR api_keys.name ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR w.api_key_id ILIKE '%' || BTRIM($4::TEXT) || '%'
  )
ORDER BY tx.created_at DESC
OFFSET $5
LIMIT $6
"#;

const COUNT_ADMIN_WALLET_REFUND_REQUESTS_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM refund_requests rr
JOIN wallets w ON w.id = rr.wallet_id
LEFT JOIN users wallet_users ON wallet_users.id = w.user_id
LEFT JOIN api_keys ON api_keys.id = w.api_key_id
WHERE ($1::TEXT IS NULL OR rr.status = $1)
  AND w.user_id IS NOT NULL
  AND (
    NULLIF(BTRIM($2::TEXT), '') IS NULL
    OR wallet_users.username ILIKE '%' || BTRIM($2::TEXT) || '%'
    OR wallet_users.email ILIKE '%' || BTRIM($2::TEXT) || '%'
    OR w.user_id ILIKE '%' || BTRIM($2::TEXT) || '%'
    OR api_keys.name ILIKE '%' || BTRIM($2::TEXT) || '%'
  )
"#;

const LIST_ADMIN_WALLET_REFUND_REQUESTS_SQL: &str = r#"
SELECT
  rr.id,
  rr.refund_no,
  rr.wallet_id,
  rr.user_id,
  rr.payment_order_id,
  rr.source_type,
  rr.source_id,
  rr.refund_mode,
  CAST(rr.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  rr.status,
  rr.reason,
  rr.failure_reason,
  rr.gateway_refund_id,
  rr.payout_method,
  rr.payout_reference,
  rr.payout_proof,
  rr.requested_by,
  rr.approved_by,
  rr.processed_by,
  w.user_id AS wallet_user_id,
  w.api_key_id AS wallet_api_key_id,
  w.status AS wallet_status,
  wallet_users.username AS wallet_user_name,
  api_keys.name AS api_key_name,
  CAST(EXTRACT(EPOCH FROM rr.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM rr.updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM rr.processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM rr.completed_at) AS BIGINT) AS completed_at_unix_secs
FROM refund_requests rr
JOIN wallets w ON w.id = rr.wallet_id
LEFT JOIN users wallet_users ON wallet_users.id = w.user_id
LEFT JOIN api_keys ON api_keys.id = w.api_key_id
WHERE ($1::TEXT IS NULL OR rr.status = $1)
  AND w.user_id IS NOT NULL
  AND (
    NULLIF(BTRIM($2::TEXT), '') IS NULL
    OR wallet_users.username ILIKE '%' || BTRIM($2::TEXT) || '%'
    OR wallet_users.email ILIKE '%' || BTRIM($2::TEXT) || '%'
    OR w.user_id ILIKE '%' || BTRIM($2::TEXT) || '%'
    OR api_keys.name ILIKE '%' || BTRIM($2::TEXT) || '%'
  )
ORDER BY rr.created_at DESC
OFFSET $3
LIMIT $4
"#;

const COUNT_ADMIN_WALLET_TRANSACTIONS_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM wallet_transactions
WHERE wallet_id = $1
"#;

const LIST_ADMIN_WALLET_TRANSACTIONS_SQL: &str = r#"
SELECT
  tx.id,
  tx.wallet_id,
  tx.category,
  tx.reason_code,
  CAST(tx.amount AS DOUBLE PRECISION) AS amount,
  CAST(tx.balance_before AS DOUBLE PRECISION) AS balance_before,
  CAST(tx.balance_after AS DOUBLE PRECISION) AS balance_after,
  CAST(tx.recharge_balance_before AS DOUBLE PRECISION) AS recharge_balance_before,
  CAST(tx.recharge_balance_after AS DOUBLE PRECISION) AS recharge_balance_after,
  CAST(tx.gift_balance_before AS DOUBLE PRECISION) AS gift_balance_before,
  CAST(tx.gift_balance_after AS DOUBLE PRECISION) AS gift_balance_after,
  tx.link_type,
  tx.link_id,
  tx.operator_id,
  tx.description,
  operator_users.username AS operator_name,
  operator_users.email AS operator_email,
  CAST(EXTRACT(EPOCH FROM tx.created_at) AS BIGINT) AS created_at_unix_ms
FROM wallet_transactions tx
LEFT JOIN users operator_users
  ON operator_users.id = tx.operator_id
WHERE tx.wallet_id = $1
ORDER BY tx.created_at DESC
OFFSET $2
LIMIT $3
"#;

const FIND_WALLET_TODAY_USAGE_SQL: &str = r#"
SELECT
  id,
  billing_date::text AS billing_date,
  billing_timezone,
  CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
  total_requests,
  input_tokens,
  output_tokens,
  cache_creation_tokens,
  cache_read_tokens,
  CAST(EXTRACT(EPOCH FROM first_finalized_at) AS BIGINT) AS first_finalized_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM last_finalized_at) AS BIGINT) AS last_finalized_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM aggregated_at) AS BIGINT) AS aggregated_at_unix_secs
FROM wallet_daily_usage_ledgers
WHERE wallet_id = $1
  AND billing_timezone = $2
  AND billing_date = (timezone($2, now()))::date
LIMIT 1
"#;

const COUNT_WALLET_DAILY_USAGE_HISTORY_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM wallet_daily_usage_ledgers
WHERE wallet_id = $1
  AND billing_timezone = $2
  AND billing_date < (timezone($2, now()))::date
"#;

const LIST_WALLET_DAILY_USAGE_HISTORY_SQL: &str = r#"
SELECT
  id,
  billing_date::text AS billing_date,
  billing_timezone,
  CAST(total_cost_usd AS DOUBLE PRECISION) AS total_cost_usd,
  total_requests,
  input_tokens,
  output_tokens,
  cache_creation_tokens,
  cache_read_tokens,
  CAST(EXTRACT(EPOCH FROM first_finalized_at) AS BIGINT) AS first_finalized_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM last_finalized_at) AS BIGINT) AS last_finalized_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM aggregated_at) AS BIGINT) AS aggregated_at_unix_secs
FROM wallet_daily_usage_ledgers
WHERE wallet_id = $1
  AND billing_timezone = $2
  AND billing_date < (timezone($2, now()))::date
ORDER BY billing_date DESC
LIMIT $3
"#;

const COUNT_ADMIN_WALLET_REFUNDS_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM refund_requests
WHERE wallet_id = $1
"#;

const COUNT_PENDING_REFUNDS_BY_USER_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM refund_requests
WHERE user_id = $1
  AND status = ANY($2::TEXT[])
"#;

const LIST_ADMIN_WALLET_REFUNDS_SQL: &str = r#"
SELECT
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
FROM refund_requests
WHERE wallet_id = $1
ORDER BY created_at DESC
OFFSET $2
LIMIT $3
"#;

const COUNT_ADMIN_PAYMENT_ORDERS_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM payment_orders po
LEFT JOIN wallets w ON w.id = po.wallet_id
LEFT JOIN users order_users ON order_users.id = po.user_id
LEFT JOIN users wallet_users ON wallet_users.id = w.user_id
WHERE ($1::TEXT IS NULL OR po.payment_method = $1)
  AND ($2::TEXT IS NULL OR po.order_kind = $2)
  AND (
    $3::TEXT IS NULL
    OR (
      CASE
        WHEN po.status = 'pending' AND po.expires_at IS NOT NULL AND po.expires_at < NOW() THEN 'expired'
        ELSE po.status
      END
    ) = $3
  )
  AND (
    NULLIF(BTRIM($4::TEXT), '') IS NULL
    OR order_users.username ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR order_users.email ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR po.user_id ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR wallet_users.username ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR wallet_users.email ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR w.user_id ILIKE '%' || BTRIM($4::TEXT) || '%'
  )
"#;

const LIST_ADMIN_PAYMENT_ORDERS_SQL: &str = r#"
SELECT
  po.id,
  po.order_no,
  po.wallet_id,
  po.user_id,
  CASE
    WHEN w.user_id IS NOT NULL THEN 'user'
    WHEN w.api_key_id IS NOT NULL THEN 'api_key'
    ELSE NULL
  END AS owner_type,
  COALESCE(wallet_users.username, wallet_api_keys.name) AS owner_name,
  CAST(po.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  CAST(po.debt_repayment_usd AS DOUBLE PRECISION) AS debt_repayment_usd,
  CAST(po.pay_amount AS DOUBLE PRECISION) AS pay_amount,
  po.pay_currency,
  CAST(po.exchange_rate AS DOUBLE PRECISION) AS exchange_rate,
  CAST(po.refunded_amount_usd AS DOUBLE PRECISION) AS refunded_amount_usd,
  CAST(po.refundable_amount_usd AS DOUBLE PRECISION) AS refundable_amount_usd,
  po.payment_method,
  po.payment_provider,
  po.payment_channel,
  po.order_kind,
  po.product_id,
  po.product_snapshot,
  po.fulfillment_status,
  po.fulfillment_error,
  po.gateway_order_id,
  po.gateway_response,
  po.status,
  CAST(EXTRACT(EPOCH FROM po.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM po.paid_at) AS BIGINT) AS paid_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM po.credited_at) AS BIGINT) AS credited_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM po.expires_at) AS BIGINT) AS expires_at_unix_secs
FROM payment_orders po
LEFT JOIN wallets w ON w.id = po.wallet_id
LEFT JOIN users order_users ON order_users.id = po.user_id
LEFT JOIN users wallet_users ON wallet_users.id = w.user_id
LEFT JOIN api_keys wallet_api_keys ON wallet_api_keys.id = w.api_key_id
WHERE ($1::TEXT IS NULL OR po.payment_method = $1)
  AND ($2::TEXT IS NULL OR po.order_kind = $2)
  AND (
    $3::TEXT IS NULL
    OR (
      CASE
        WHEN po.status = 'pending' AND po.expires_at IS NOT NULL AND po.expires_at < NOW() THEN 'expired'
        ELSE po.status
      END
    ) = $3
  )
  AND (
    NULLIF(BTRIM($4::TEXT), '') IS NULL
    OR order_users.username ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR order_users.email ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR po.user_id ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR wallet_users.username ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR wallet_users.email ILIKE '%' || BTRIM($4::TEXT) || '%'
    OR w.user_id ILIKE '%' || BTRIM($4::TEXT) || '%'
  )
ORDER BY po.created_at DESC
OFFSET $5
LIMIT $6
"#;

const FIND_ADMIN_PAYMENT_ORDER_SQL: &str = r#"
SELECT
  id,
  order_no,
  wallet_id,
  user_id,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  CAST(debt_repayment_usd AS DOUBLE PRECISION) AS debt_repayment_usd,
  CAST(pay_amount AS DOUBLE PRECISION) AS pay_amount,
  pay_currency,
  CAST(exchange_rate AS DOUBLE PRECISION) AS exchange_rate,
  CAST(refunded_amount_usd AS DOUBLE PRECISION) AS refunded_amount_usd,
  CAST(refundable_amount_usd AS DOUBLE PRECISION) AS refundable_amount_usd,
  payment_method,
  payment_provider,
  payment_channel,
  order_kind,
  product_id,
  product_snapshot,
  fulfillment_status,
  fulfillment_error,
  gateway_order_id,
  gateway_response,
  status,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM paid_at) AS BIGINT) AS paid_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM credited_at) AS BIGINT) AS credited_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM expires_at) AS BIGINT) AS expires_at_unix_secs
FROM payment_orders
WHERE id = $1
LIMIT 1
"#;

const COUNT_WALLET_PAYMENT_ORDERS_BY_USER_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM payment_orders
WHERE user_id = $1
"#;

const COUNT_PENDING_PAYMENT_ORDERS_BY_USER_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM payment_orders
WHERE user_id = $1
  AND status = ANY($2::TEXT[])
"#;

const LIST_WALLET_PAYMENT_ORDERS_BY_USER_SQL: &str = r#"
SELECT
  id,
  order_no,
  wallet_id,
  user_id,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  CAST(debt_repayment_usd AS DOUBLE PRECISION) AS debt_repayment_usd,
  CAST(pay_amount AS DOUBLE PRECISION) AS pay_amount,
  pay_currency,
  CAST(exchange_rate AS DOUBLE PRECISION) AS exchange_rate,
  CAST(refunded_amount_usd AS DOUBLE PRECISION) AS refunded_amount_usd,
  CAST(refundable_amount_usd AS DOUBLE PRECISION) AS refundable_amount_usd,
  payment_method,
  payment_provider,
  payment_channel,
  order_kind,
  product_id,
  product_snapshot,
  fulfillment_status,
  fulfillment_error,
  gateway_order_id,
  gateway_response,
  CASE
    WHEN status = 'pending' AND expires_at IS NOT NULL AND expires_at < now() THEN 'expired'
    ELSE status
  END AS status,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM paid_at) AS BIGINT) AS paid_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM credited_at) AS BIGINT) AS credited_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM expires_at) AS BIGINT) AS expires_at_unix_secs
FROM payment_orders
WHERE user_id = $1
ORDER BY created_at DESC
OFFSET $2
LIMIT $3
"#;

const FIND_WALLET_PAYMENT_ORDER_BY_USER_SQL: &str = r#"
SELECT
  id,
  order_no,
  wallet_id,
  user_id,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  CAST(debt_repayment_usd AS DOUBLE PRECISION) AS debt_repayment_usd,
  CAST(pay_amount AS DOUBLE PRECISION) AS pay_amount,
  pay_currency,
  CAST(exchange_rate AS DOUBLE PRECISION) AS exchange_rate,
  CAST(refunded_amount_usd AS DOUBLE PRECISION) AS refunded_amount_usd,
  CAST(refundable_amount_usd AS DOUBLE PRECISION) AS refundable_amount_usd,
  payment_method,
  payment_provider,
  payment_channel,
  order_kind,
  product_id,
  product_snapshot,
  fulfillment_status,
  fulfillment_error,
  gateway_order_id,
  gateway_response,
  CASE
    WHEN status = 'pending' AND expires_at IS NOT NULL AND expires_at < now() THEN 'expired'
    ELSE status
  END AS status,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM paid_at) AS BIGINT) AS paid_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM credited_at) AS BIGINT) AS credited_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM expires_at) AS BIGINT) AS expires_at_unix_secs
FROM payment_orders
WHERE user_id = $1
  AND id = $2
LIMIT 1
"#;

const FIND_WALLET_REFUND_SQL: &str = r#"
SELECT
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
FROM refund_requests
WHERE wallet_id = $1
  AND id = $2
LIMIT 1
"#;

const COUNT_ADMIN_PAYMENT_CALLBACKS_SQL: &str = r#"
SELECT COUNT(*) AS total
FROM payment_callbacks
WHERE ($1::TEXT IS NULL OR payment_method = $1)
"#;

const LIST_ADMIN_PAYMENT_CALLBACKS_SQL: &str = r#"
SELECT
  id,
  payment_order_id,
  payment_method,
  callback_key,
  order_no,
  gateway_order_id,
  payload_hash,
  signature_valid,
  status,
  payload,
  error_message,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs
FROM payment_callbacks
WHERE ($1::TEXT IS NULL OR payment_method = $1)
ORDER BY created_at DESC
OFFSET $2
LIMIT $3
"#;

#[derive(Debug, Clone)]
pub struct SqlxWalletRepository {
    pool: PgPool,
    tx_runner: PostgresTransactionRunner,
}

impl SqlxWalletRepository {
    pub fn new(pool: PgPool) -> Self {
        let tx_runner = PostgresTransactionRunner::new(pool.clone());
        Self { pool, tx_runner }
    }
}

#[async_trait]
impl WalletReadRepository for SqlxWalletRepository {
    async fn find(
        &self,
        key: WalletLookupKey<'_>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let query = match key {
            WalletLookupKey::WalletId(_) => FIND_BY_WALLET_ID_SQL,
            WalletLookupKey::UserId(_) => FIND_BY_USER_ID_SQL,
            WalletLookupKey::ApiKeyId(_) => FIND_BY_API_KEY_ID_SQL,
        };
        let bind = match key {
            WalletLookupKey::WalletId(value)
            | WalletLookupKey::UserId(value)
            | WalletLookupKey::ApiKeyId(value) => value,
        };
        let row = sqlx::query(query)
            .bind(bind)
            .fetch_optional(&self.pool)
            .await
            .map_postgres_err()?;
        row.as_ref().map(map_wallet_row).transpose()
    }

    async fn update_auth_user_wallet_limit_mode(
        &self,
        user_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let result = sqlx::query(
            "UPDATE wallets SET limit_mode = $2, updated_at = NOW() WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(limit_mode)
        .execute(&self.pool)
        .await
        .map_postgres_err()?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.find(WalletLookupKey::UserId(user_id)).await
    }

    async fn update_auth_api_key_wallet_limit_mode(
        &self,
        api_key_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let result = sqlx::query(
            "UPDATE wallets SET limit_mode = $2, updated_at = NOW() WHERE api_key_id = $1",
        )
        .bind(api_key_id)
        .bind(limit_mode)
        .execute(&self.pool)
        .await
        .map_postgres_err()?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.find(WalletLookupKey::ApiKeyId(api_key_id)).await
    }

    async fn initialize_auth_user_wallet(
        &self,
        user_id: &str,
        initial_gift_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        initialize_postgres_auth_wallet(
            &self.pool,
            Some(user_id),
            None,
            initial_gift_usd,
            unlimited,
        )
        .await
    }

    async fn initialize_auth_api_key_wallet(
        &self,
        api_key_id: &str,
        initial_gift_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        initialize_postgres_auth_wallet(
            &self.pool,
            None,
            Some(api_key_id),
            initial_gift_usd,
            unlimited,
        )
        .await
    }

    async fn update_auth_user_wallet_snapshot(
        &self,
        user_id: &str,
        balance: f64,
        gift_balance: f64,
        limit_mode: &str,
        currency: &str,
        status: &str,
        total_recharged: f64,
        total_consumed: f64,
        total_refunded: f64,
        total_adjusted: f64,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        update_postgres_wallet_snapshot(
            &self.pool,
            "user_id",
            user_id,
            balance,
            gift_balance,
            limit_mode,
            currency,
            status,
            total_recharged,
            total_consumed,
            total_refunded,
            total_adjusted,
            updated_at_unix_secs,
        )
        .await?;
        self.find(WalletLookupKey::UserId(user_id)).await
    }

    async fn update_auth_api_key_wallet_snapshot(
        &self,
        api_key_id: &str,
        balance: f64,
        gift_balance: f64,
        limit_mode: &str,
        currency: &str,
        status: &str,
        total_recharged: f64,
        total_consumed: f64,
        total_refunded: f64,
        total_adjusted: f64,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        update_postgres_wallet_snapshot(
            &self.pool,
            "api_key_id",
            api_key_id,
            balance,
            gift_balance,
            limit_mode,
            currency,
            status,
            total_recharged,
            total_consumed,
            total_refunded,
            total_adjusted,
            updated_at_unix_secs,
        )
        .await?;
        self.find(WalletLookupKey::ApiKeyId(api_key_id)).await
    }

    async fn list_wallets_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }
        collect_query_rows(
            sqlx::query(LIST_BY_USER_IDS_SQL)
                .bind(user_ids)
                .fetch(&self.pool),
            map_wallet_row,
        )
        .await
    }
    async fn list_wallets_by_api_key_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
        if api_key_ids.is_empty() {
            return Ok(Vec::new());
        }
        collect_query_rows(
            sqlx::query(LIST_BY_API_KEY_IDS_SQL)
                .bind(api_key_ids)
                .fetch(&self.pool),
            map_wallet_row,
        )
        .await
    }

    async fn list_admin_wallets(
        &self,
        query: &AdminWalletListQuery,
    ) -> Result<StoredAdminWalletListPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_ADMIN_WALLETS_SQL)
                .bind(query.status.as_deref())
                .bind(query.owner_type.as_deref())
                .bind(query.user_search.as_deref())
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_ADMIN_WALLETS_SQL)
                .bind(query.status.as_deref())
                .bind(query.owner_type.as_deref())
                .bind(query.user_search.as_deref())
                .bind(as_i64(query.offset, "wallet offset")?)
                .bind(as_i64(query.limit, "wallet limit")?)
                .fetch(&self.pool),
            map_admin_wallet_list_item_row,
        )
        .await?;
        Ok(StoredAdminWalletListPage { items, total })
    }

    async fn list_admin_wallet_ledger(
        &self,
        query: &AdminWalletLedgerQuery,
    ) -> Result<StoredAdminWalletLedgerPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_ADMIN_WALLET_LEDGER_SQL)
                .bind(query.category.as_deref())
                .bind(query.reason_code.as_deref())
                .bind(query.owner_type.as_deref())
                .bind(query.user_search.as_deref())
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_ADMIN_WALLET_LEDGER_SQL)
                .bind(query.category.as_deref())
                .bind(query.reason_code.as_deref())
                .bind(query.owner_type.as_deref())
                .bind(query.user_search.as_deref())
                .bind(as_i64(query.offset, "wallet ledger offset")?)
                .bind(as_i64(query.limit, "wallet ledger limit")?)
                .fetch(&self.pool),
            map_admin_wallet_ledger_item_row,
        )
        .await?;
        Ok(StoredAdminWalletLedgerPage { items, total })
    }

    async fn list_admin_wallet_refund_requests(
        &self,
        query: &AdminWalletRefundRequestListQuery,
    ) -> Result<StoredAdminWalletRefundRequestPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_ADMIN_WALLET_REFUND_REQUESTS_SQL)
                .bind(query.status.as_deref())
                .bind(query.user_search.as_deref())
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_ADMIN_WALLET_REFUND_REQUESTS_SQL)
                .bind(query.status.as_deref())
                .bind(query.user_search.as_deref())
                .bind(as_i64(query.offset, "wallet refund request offset")?)
                .bind(as_i64(query.limit, "wallet refund request limit")?)
                .fetch(&self.pool),
            map_admin_wallet_refund_request_item_row,
        )
        .await?;
        Ok(StoredAdminWalletRefundRequestPage { items, total })
    }

    async fn list_admin_wallet_transactions(
        &self,
        wallet_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<StoredAdminWalletTransactionPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_ADMIN_WALLET_TRANSACTIONS_SQL)
                .bind(wallet_id)
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_ADMIN_WALLET_TRANSACTIONS_SQL)
                .bind(wallet_id)
                .bind(as_i64(offset, "wallet transaction offset")?)
                .bind(as_i64(limit, "wallet transaction limit")?)
                .fetch(&self.pool),
            map_admin_wallet_transaction_row,
        )
        .await?;
        Ok(StoredAdminWalletTransactionPage { items, total })
    }

    async fn find_wallet_today_usage(
        &self,
        wallet_id: &str,
        billing_timezone: &str,
    ) -> Result<Option<StoredWalletDailyUsageLedger>, DataLayerError> {
        let row = sqlx::query(FIND_WALLET_TODAY_USAGE_SQL)
            .bind(wallet_id)
            .bind(billing_timezone)
            .fetch_optional(&self.pool)
            .await
            .map_postgres_err()?;
        row.as_ref().map(map_wallet_daily_usage_row).transpose()
    }

    async fn list_wallet_daily_usage_history(
        &self,
        wallet_id: &str,
        billing_timezone: &str,
        limit: usize,
    ) -> Result<StoredWalletDailyUsageLedgerPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_WALLET_DAILY_USAGE_HISTORY_SQL)
                .bind(wallet_id)
                .bind(billing_timezone)
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_WALLET_DAILY_USAGE_HISTORY_SQL)
                .bind(wallet_id)
                .bind(billing_timezone)
                .bind(as_i64(limit, "wallet daily usage history limit")?)
                .fetch(&self.pool),
            map_wallet_daily_usage_row,
        )
        .await?;
        Ok(StoredWalletDailyUsageLedgerPage { items, total })
    }

    async fn list_admin_wallet_refunds(
        &self,
        wallet_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<StoredAdminWalletRefundPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_ADMIN_WALLET_REFUNDS_SQL)
                .bind(wallet_id)
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_ADMIN_WALLET_REFUNDS_SQL)
                .bind(wallet_id)
                .bind(as_i64(offset, "wallet refund offset")?)
                .bind(as_i64(limit, "wallet refund limit")?)
                .fetch(&self.pool),
            map_admin_wallet_refund_row,
        )
        .await?;
        Ok(StoredAdminWalletRefundPage { items, total })
    }

    async fn list_admin_payment_orders(
        &self,
        query: &AdminPaymentOrderListQuery,
    ) -> Result<StoredAdminPaymentOrderPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_ADMIN_PAYMENT_ORDERS_SQL)
                .bind(query.payment_method.as_deref())
                .bind(query.order_kind.as_deref())
                .bind(query.status.as_deref())
                .bind(query.user_search.as_deref())
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_ADMIN_PAYMENT_ORDERS_SQL)
                .bind(query.payment_method.as_deref())
                .bind(query.order_kind.as_deref())
                .bind(query.status.as_deref())
                .bind(query.user_search.as_deref())
                .bind(as_i64(query.offset, "payment order offset")?)
                .bind(as_i64(query.limit, "payment order limit")?)
                .fetch(&self.pool),
            map_admin_payment_order_row,
        )
        .await?;
        Ok(StoredAdminPaymentOrderPage { items, total })
    }

    async fn find_admin_payment_order(
        &self,
        order_id: &str,
    ) -> Result<Option<StoredAdminPaymentOrder>, DataLayerError> {
        let row = sqlx::query(FIND_ADMIN_PAYMENT_ORDER_SQL)
            .bind(order_id)
            .fetch_optional(&self.pool)
            .await
            .map_postgres_err()?;
        row.as_ref().map(map_admin_payment_order_row).transpose()
    }

    async fn list_wallet_payment_orders_by_user_id(
        &self,
        user_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<StoredAdminPaymentOrderPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_WALLET_PAYMENT_ORDERS_BY_USER_SQL)
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_WALLET_PAYMENT_ORDERS_BY_USER_SQL)
                .bind(user_id)
                .bind(as_i64(offset, "wallet payment order offset")?)
                .bind(as_i64(limit, "wallet payment order limit")?)
                .fetch(&self.pool),
            map_admin_payment_order_row,
        )
        .await?;
        Ok(StoredAdminPaymentOrderPage { items, total })
    }

    async fn count_pending_refunds_by_user_id(&self, user_id: &str) -> Result<u64, DataLayerError> {
        let statuses = vec![
            "pending_approval".to_string(),
            "approved".to_string(),
            "processing".to_string(),
        ];
        read_count(
            sqlx::query(COUNT_PENDING_REFUNDS_BY_USER_SQL)
                .bind(user_id)
                .bind(statuses)
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )
    }

    async fn count_pending_payment_orders_by_user_id(
        &self,
        user_id: &str,
    ) -> Result<u64, DataLayerError> {
        let statuses = vec!["pending".to_string(), "paid".to_string()];
        read_count(
            sqlx::query(COUNT_PENDING_PAYMENT_ORDERS_BY_USER_SQL)
                .bind(user_id)
                .bind(statuses)
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )
    }

    async fn find_wallet_payment_order_by_user_id(
        &self,
        user_id: &str,
        order_id: &str,
    ) -> Result<Option<StoredAdminPaymentOrder>, DataLayerError> {
        let row = sqlx::query(FIND_WALLET_PAYMENT_ORDER_BY_USER_SQL)
            .bind(user_id)
            .bind(order_id)
            .fetch_optional(&self.pool)
            .await
            .map_postgres_err()?;
        row.as_ref().map(map_admin_payment_order_row).transpose()
    }

    async fn find_wallet_refund(
        &self,
        wallet_id: &str,
        refund_id: &str,
    ) -> Result<Option<StoredAdminWalletRefund>, DataLayerError> {
        let row = sqlx::query(FIND_WALLET_REFUND_SQL)
            .bind(wallet_id)
            .bind(refund_id)
            .fetch_optional(&self.pool)
            .await
            .map_postgres_err()?;
        row.as_ref().map(map_admin_wallet_refund_row).transpose()
    }

    async fn list_admin_payment_callbacks(
        &self,
        payment_method: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<StoredAdminPaymentCallbackPage, DataLayerError> {
        let total = read_count(
            sqlx::query(COUNT_ADMIN_PAYMENT_CALLBACKS_SQL)
                .bind(payment_method)
                .fetch_one(&self.pool)
                .await
                .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(LIST_ADMIN_PAYMENT_CALLBACKS_SQL)
                .bind(payment_method)
                .bind(as_i64(offset, "payment callback offset")?)
                .bind(as_i64(limit, "payment callback limit")?)
                .fetch(&self.pool),
            map_admin_payment_callback_row,
        )
        .await?;
        Ok(StoredAdminPaymentCallbackPage { items, total })
    }

    async fn list_admin_redeem_code_batches(
        &self,
        query: &AdminRedeemCodeBatchListQuery,
    ) -> Result<StoredAdminRedeemCodeBatchPage, DataLayerError> {
        let total = read_count(
            sqlx::query(
                r#"
SELECT COUNT(*) AS total
FROM redeem_code_batches
WHERE $1::TEXT IS NULL OR status = $1
                "#,
            )
            .bind(query.status.as_deref())
            .fetch_one(&self.pool)
            .await
            .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(
                r#"
SELECT
  batches.id,
  batches.name,
  CAST(batches.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  batches.currency,
  batches.balance_bucket,
  CAST(batches.total_count AS BIGINT) AS total_count,
  CAST(COALESCE(stats.redeemed_count, 0) AS BIGINT) AS redeemed_count,
  CAST(COALESCE(stats.active_count, 0) AS BIGINT) AS active_count,
  batches.status,
  batches.description,
  batches.created_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM batches.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM batches.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_code_batches AS batches
LEFT JOIN (
  SELECT
    batch_id,
    COUNT(*) FILTER (WHERE status = 'redeemed') AS redeemed_count,
    COUNT(*) FILTER (WHERE status = 'active') AS active_count
  FROM redeem_codes
  GROUP BY batch_id
) AS stats
  ON stats.batch_id = batches.id
WHERE $1::TEXT IS NULL OR batches.status = $1
ORDER BY batches.created_at DESC, batches.id DESC
OFFSET $2
LIMIT $3
                "#,
            )
            .bind(query.status.as_deref())
            .bind(as_i64(query.offset, "redeem code batch offset")?)
            .bind(as_i64(query.limit, "redeem code batch limit")?)
            .fetch(&self.pool),
            map_admin_redeem_code_batch_row,
        )
        .await?;
        Ok(StoredAdminRedeemCodeBatchPage { items, total })
    }

    async fn find_admin_redeem_code_batch(
        &self,
        batch_id: &str,
    ) -> Result<Option<StoredAdminRedeemCodeBatch>, DataLayerError> {
        let row = sqlx::query(
            r#"
SELECT
  batches.id,
  batches.name,
  CAST(batches.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  batches.currency,
  batches.balance_bucket,
  CAST(batches.total_count AS BIGINT) AS total_count,
  CAST(COALESCE(stats.redeemed_count, 0) AS BIGINT) AS redeemed_count,
  CAST(COALESCE(stats.active_count, 0) AS BIGINT) AS active_count,
  batches.status,
  batches.description,
  batches.created_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM batches.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM batches.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_code_batches AS batches
LEFT JOIN (
  SELECT
    batch_id,
    COUNT(*) FILTER (WHERE status = 'redeemed') AS redeemed_count,
    COUNT(*) FILTER (WHERE status = 'active') AS active_count
  FROM redeem_codes
  GROUP BY batch_id
) AS stats
  ON stats.batch_id = batches.id
WHERE batches.id = $1
LIMIT 1
            "#,
        )
        .bind(batch_id)
        .fetch_optional(&self.pool)
        .await
        .map_postgres_err()?;
        row.as_ref()
            .map(map_admin_redeem_code_batch_row)
            .transpose()
    }

    async fn list_admin_redeem_codes(
        &self,
        query: &AdminRedeemCodeListQuery,
    ) -> Result<StoredAdminRedeemCodePage, DataLayerError> {
        let total = read_count(
            sqlx::query(
                r#"
SELECT COUNT(*) AS total
FROM redeem_codes
WHERE batch_id = $1
  AND ($2::TEXT IS NULL OR status = $2)
                "#,
            )
            .bind(&query.batch_id)
            .bind(query.status.as_deref())
            .fetch_one(&self.pool)
            .await
            .map_postgres_err()?,
        )?;
        let items = collect_query_rows(
            sqlx::query(
                r#"
SELECT
  codes.id,
  codes.batch_id,
  batches.name AS batch_name,
  codes.code_prefix,
  codes.code_suffix,
  codes.status,
  codes.redeemed_by_user_id,
  users.username AS redeemed_by_user_name,
  codes.redeemed_wallet_id,
  codes.redeemed_payment_order_id,
  orders.order_no AS redeemed_order_no,
  CAST(EXTRACT(EPOCH FROM codes.redeemed_at) AS BIGINT) AS redeemed_at_unix_secs,
  codes.disabled_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM codes.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM codes.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_codes AS codes
JOIN redeem_code_batches AS batches
  ON batches.id = codes.batch_id
LEFT JOIN users
  ON users.id = codes.redeemed_by_user_id
LEFT JOIN payment_orders AS orders
  ON orders.id = codes.redeemed_payment_order_id
WHERE codes.batch_id = $1
  AND ($2::TEXT IS NULL OR codes.status = $2)
ORDER BY codes.created_at DESC, codes.id DESC
OFFSET $3
LIMIT $4
                "#,
            )
            .bind(&query.batch_id)
            .bind(query.status.as_deref())
            .bind(as_i64(query.offset, "redeem code offset")?)
            .bind(as_i64(query.limit, "redeem code limit")?)
            .fetch(&self.pool),
            map_admin_redeem_code_row,
        )
        .await?;
        Ok(StoredAdminRedeemCodePage { items, total })
    }
}

#[async_trait]
impl WalletWriteRepository for SqlxWalletRepository {
    async fn create_wallet_recharge_order(
        &self,
        input: CreateWalletRechargeOrderInput,
    ) -> Result<CreateWalletRechargeOrderOutcome, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let wallet_row = match sqlx::query(
                        r#"
SELECT id, status, CAST(balance + gift_balance AS DOUBLE PRECISION) AS signed_balance
FROM wallets
WHERE user_id = $1
LIMIT 1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.user_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    {
                        Some(row) => row,
                        None => {
                            let wallet_id = input
                                .preferred_wallet_id
                                .clone()
                                .unwrap_or_else(|| Uuid::new_v4().to_string());
                            sqlx::query(
                                r#"
INSERT INTO wallets (
  id,
  user_id,
  balance,
  gift_balance,
  limit_mode,
  currency,
  status,
  total_recharged,
  total_consumed,
  total_refunded,
  total_adjusted,
  created_at,
  updated_at
)
VALUES (
  $1,
  $2,
  0,
  0,
  'finite',
  'USD',
  'active',
  0,
  0,
  0,
  0,
  NOW(),
  NOW()
)
ON CONFLICT (user_id) DO UPDATE
SET updated_at = wallets.updated_at
RETURNING id, status, CAST(balance + gift_balance AS DOUBLE PRECISION) AS signed_balance
                                "#,
                            )
                            .bind(&wallet_id)
                            .bind(&input.user_id)
                            .fetch_one(&mut **tx)
                            .await
                            .map_postgres_err()?
                        }
                    };
                    let wallet_id: String = row_get(&wallet_row, "id")?;
                    let wallet_status: String = row_get(&wallet_row, "status")?;
                    if wallet_status != "active" {
                        return Ok(CreateWalletRechargeOrderOutcome::WalletInactive);
                    }

                    let expires_at = i64::try_from(input.expires_at_unix_secs).map_err(|_| {
                        DataLayerError::InvalidInput(
                            "wallet recharge expires_at overflow".to_string(),
                        )
                    })?;
                    let sql = format!(
                        r#"
INSERT INTO payment_orders (
  id,
  order_no,
  wallet_id,
  user_id,
  amount_usd,
  pay_amount,
  pay_currency,
  exchange_rate,
  refunded_amount_usd,
  refundable_amount_usd,
  payment_method,
  payment_provider,
  payment_channel,
  order_kind,
  fulfillment_status,
  gateway_order_id,
  gateway_response,
  status,
  created_at,
  expires_at
)
VALUES (
  $1,
  $2,
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  0,
  0,
  $9,
  $10,
  $11,
  'wallet_recharge',
  'pending',
  $12,
  $13,
  'pending',
  NOW(),
  to_timestamp($14)
)
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let row = sqlx::query(&sql)
                        .bind(Uuid::new_v4().to_string())
                        .bind(&input.order_no)
                        .bind(&wallet_id)
                        .bind(&input.user_id)
                        .bind(input.amount_usd)
                        .bind(input.pay_amount)
                        .bind(input.pay_currency.as_deref())
                        .bind(input.exchange_rate)
                        .bind(&input.payment_method)
                        .bind(input.payment_provider.as_deref())
                        .bind(input.payment_channel.as_deref())
                        .bind(&input.gateway_order_id)
                        .bind(&input.gateway_response)
                        .bind(expires_at)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    Ok(CreateWalletRechargeOrderOutcome::Created(
                        map_admin_payment_order_row(&row)?,
                    ))
                })
            })
            .await
    }

    async fn create_plan_purchase_order(
        &self,
        input: CreatePlanPurchaseOrderInput,
    ) -> Result<CreatePlanPurchaseOrderOutcome, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let wallet_row = match sqlx::query(
                        r#"
SELECT id, status, CAST(balance + gift_balance AS DOUBLE PRECISION) AS signed_balance
FROM wallets
WHERE user_id = $1
LIMIT 1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.user_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    {
                        Some(row) => row,
                        None => {
                            let wallet_id = input
                                .preferred_wallet_id
                                .clone()
                                .unwrap_or_else(|| Uuid::new_v4().to_string());
                            sqlx::query(
                                r#"
INSERT INTO wallets (
  id, user_id, balance, gift_balance, limit_mode, currency, status,
  total_recharged, total_consumed, total_refunded, total_adjusted,
  created_at, updated_at
)
VALUES ($1, $2, 0, 0, 'finite', 'USD', 'active', 0, 0, 0, 0, NOW(), NOW())
ON CONFLICT (user_id) DO UPDATE
SET updated_at = wallets.updated_at
RETURNING id, status, CAST(balance + gift_balance AS DOUBLE PRECISION) AS signed_balance
                                "#,
                            )
                            .bind(&wallet_id)
                            .bind(&input.user_id)
                            .fetch_one(&mut **tx)
                            .await
                            .map_postgres_err()?
                        }
                    };
                    let wallet_id: String = row_get(&wallet_row, "id")?;
                    let wallet_status: String = row_get(&wallet_row, "status")?;
                    if wallet_status != "active" {
                        return Ok(CreatePlanPurchaseOrderOutcome::WalletInactive);
                    }
                    let signed_balance: f64 = row_get(&wallet_row, "signed_balance")?;
                    let payment_amounts = plan_purchase_payment_amounts(&input, signed_balance)?;

                    let requested_starts_at = plan_starts_at(&input.product_snapshot, Utc::now());
                    let requested_expires_at =
                        plan_expires_at(&input.product_snapshot, requested_starts_at);
                    if has_other_overlapping_plan_postgres(
                        tx,
                        &input.user_id,
                        &input.product_id,
                        requested_starts_at,
                        requested_expires_at,
                    )
                    .await?
                    {
                        return Ok(CreatePlanPurchaseOrderOutcome::OverlappingPlanExists);
                    }
                    if has_pending_other_plan_purchase_postgres(
                        tx,
                        &input.user_id,
                        &input.product_id,
                    )
                    .await?
                    {
                        return Ok(CreatePlanPurchaseOrderOutcome::OverlappingPlanExists);
                    }

                    let purchase_limit_scope = plan_purchase_limit_scope(&input.product_snapshot);
                    if purchase_limit_scope != "unlimited" {
                        let max_active_per_user = plan_max_active_per_user(&input.product_snapshot);
                        let mut active_count = if purchase_limit_scope == "lifetime" {
                            sqlx::query_scalar::<_, i64>(
                                r#"
SELECT COUNT(*)::bigint
FROM user_plan_entitlements
WHERE user_id = $1
  AND plan_id = $2
  AND status = 'active'
                        "#,
                            )
                            .bind(&input.user_id)
                            .bind(&input.product_id)
                            .fetch_one(&mut **tx)
                            .await
                            .map_postgres_err()?
                        } else {
                            0
                        };
                        active_count += sqlx::query_scalar::<_, i64>(
                            r#"
	SELECT COUNT(*)::bigint
	FROM payment_orders
	WHERE user_id = $1
	  AND product_id = $2
	  AND order_kind = 'plan_purchase'
	  AND status = 'pending'
	  AND expires_at > NOW()
	                        "#,
                        )
                        .bind(&input.user_id)
                        .bind(&input.product_id)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        if active_count >= max_active_per_user {
                            return Ok(CreatePlanPurchaseOrderOutcome::ActivePlanLimitReached);
                        }
                    }

                    let expires_at = i64::try_from(input.expires_at_unix_secs).map_err(|_| {
                        DataLayerError::InvalidInput(
                            "plan purchase expires_at overflow".to_string(),
                        )
                    })?;
                    let sql = format!(
                        r#"
INSERT INTO payment_orders (
  id,
  order_no,
  wallet_id,
  user_id,
  amount_usd,
  debt_repayment_usd,
  pay_amount,
  pay_currency,
  exchange_rate,
  refunded_amount_usd,
  refundable_amount_usd,
  payment_method,
  payment_provider,
  payment_channel,
  order_kind,
  product_id,
  product_snapshot,
  fulfillment_status,
  gateway_order_id,
  gateway_response,
  status,
  created_at,
  expires_at
)
VALUES (
  $1, $2, $3, $4, $5, $6, $7, $8, $9, 0, 0, $10, $11, $12,
  'plan_purchase', $13, $14, 'pending', $15, $16, 'pending', NOW(),
  to_timestamp($17)
)
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let row = sqlx::query(&sql)
                        .bind(Uuid::new_v4().to_string())
                        .bind(&input.order_no)
                        .bind(&wallet_id)
                        .bind(&input.user_id)
                        .bind(payment_amounts.amount_usd)
                        .bind(payment_amounts.debt_repayment_usd)
                        .bind(payment_amounts.pay_amount)
                        .bind(&input.pay_currency)
                        .bind(input.exchange_rate)
                        .bind(&input.payment_method)
                        .bind(input.payment_provider.as_deref())
                        .bind(input.payment_channel.as_deref())
                        .bind(&input.product_id)
                        .bind(&input.product_snapshot)
                        .bind(&input.gateway_order_id)
                        .bind(&input.gateway_response)
                        .bind(expires_at)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    Ok(CreatePlanPurchaseOrderOutcome::Created(
                        map_admin_payment_order_row(&row)?,
                    ))
                })
            })
            .await
    }

    async fn create_wallet_refund_request(
        &self,
        input: CreateWalletRefundRequestInput,
    ) -> Result<CreateWalletRefundRequestOutcome, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(locked_wallet_row) = sqlx::query(
                        r#"
SELECT
  id,
  CAST(balance AS DOUBLE PRECISION) AS balance
FROM wallets
WHERE id = $1
LIMIT 1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(CreateWalletRefundRequestOutcome::WalletMissing);
                    };
                    let wallet_recharge_balance: f64 = row_get(&locked_wallet_row, "balance")?;
                    let wallet_reserved_row = sqlx::query(
                        r#"
SELECT COALESCE(CAST(SUM(amount_usd) AS DOUBLE PRECISION), 0) AS total
FROM refund_requests
WHERE wallet_id = $1
  AND status IN ('pending_approval', 'approved')
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    let wallet_reserved_amount: f64 = row_get(&wallet_reserved_row, "total")?;
                    if input.amount_usd > (wallet_recharge_balance - wallet_reserved_amount) {
                        return Ok(
                            CreateWalletRefundRequestOutcome::RefundAmountExceedsAvailableBalance,
                        );
                    }

                    let mut payment_order_id = None;
                    let mut source_type = input
                        .source_type
                        .clone()
                        .unwrap_or_else(|| "wallet_balance".to_string());
                    let mut source_id = input.source_id.clone();
                    let mut refund_mode = input
                        .refund_mode
                        .clone()
                        .unwrap_or_else(|| "offline_payout".to_string());
                    if let Some(order_id) = input.payment_order_id.as_deref() {
                        let Some(order_row) = sqlx::query(
                            r#"
SELECT
  id,
  status,
  payment_method,
  CAST(refundable_amount_usd AS DOUBLE PRECISION) AS refundable_amount_usd
FROM payment_orders
WHERE id = $1
  AND wallet_id = $2
LIMIT 1
FOR UPDATE
                            "#,
                        )
                        .bind(order_id)
                        .bind(&input.wallet_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?
                        else {
                            return Ok(CreateWalletRefundRequestOutcome::PaymentOrderNotFound);
                        };
                        let status: String = row_get(&order_row, "status")?;
                        if status != "credited" {
                            return Ok(CreateWalletRefundRequestOutcome::PaymentOrderNotRefundable);
                        }
                        let order_reserved_row = sqlx::query(
                            r#"
SELECT COALESCE(CAST(SUM(amount_usd) AS DOUBLE PRECISION), 0) AS total
FROM refund_requests
WHERE payment_order_id = $1
  AND status IN ('pending_approval', 'approved')
                            "#,
                        )
                        .bind(order_id)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        let refundable_amount: f64 =
                            row_get(&order_row, "refundable_amount_usd")?;
                        let reserved_amount: f64 = row_get(&order_reserved_row, "total")?;
                        if input.amount_usd > (refundable_amount - reserved_amount) {
                            return Ok(
                                CreateWalletRefundRequestOutcome::RefundAmountExceedsAvailableOrderAmount,
                            );
                        }
                        payment_order_id = Some(order_id.to_string());
                        source_type = "payment_order".to_string();
                        source_id = Some(order_id.to_string());
                        if input.refund_mode.is_none() {
                            let payment_method: String = row_get(&order_row, "payment_method")?;
                            refund_mode =
                                default_refund_mode_for_payment_method(&payment_method).to_string();
                        }
                    }

                    let insert_result = sqlx::query(
                        r#"
INSERT INTO refund_requests (
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  amount_usd,
  status,
  reason,
  requested_by,
  idempotency_key,
  created_at,
  updated_at
)
VALUES (
  $1,
  $2,
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $9,
  'pending_approval',
  $10,
  $11,
  $12,
  NOW(),
  NOW()
)
RETURNING
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
                        "#,
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&input.refund_no)
                    .bind(&input.wallet_id)
                    .bind(&input.user_id)
                    .bind(payment_order_id.as_deref())
                    .bind(&source_type)
                    .bind(source_id.as_deref())
                    .bind(&refund_mode)
                    .bind(input.amount_usd)
                    .bind(input.reason.as_deref())
                    .bind(&input.user_id)
                    .bind(input.idempotency_key.as_deref())
                    .fetch_one(&mut **tx)
                    .await;
                    match insert_result {
                        Ok(row) => Ok(CreateWalletRefundRequestOutcome::Created(
                            map_admin_wallet_refund_row(&row)?,
                        )),
                        Err(sqlx::Error::Database(err))
                            if err.code().as_deref() == Some("23505") =>
                        {
                            if let Some(idempotency_key) = input.idempotency_key.as_deref() {
                                let existing = sqlx::query(
                                    r#"
SELECT
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
FROM refund_requests
WHERE user_id = $1
  AND idempotency_key = $2
LIMIT 1
                                    "#,
                                )
                                .bind(&input.user_id)
                                .bind(idempotency_key)
                                .fetch_optional(&mut **tx)
                                .await
                                .map_postgres_err()?;
                                if let Some(row) = existing {
                                    return Ok(CreateWalletRefundRequestOutcome::Duplicate(
                                        map_admin_wallet_refund_row(&row)?,
                                    ));
                                }
                            }
                            Ok(CreateWalletRefundRequestOutcome::DuplicateRejected)
                        }
                        Err(err) => Err(postgres_error(err)),
                    }
                })
            })
            .await
    }

    async fn process_payment_callback(
        &self,
        input: ProcessPaymentCallbackInput,
    ) -> Result<ProcessPaymentCallbackOutcome, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let existing_callback = sqlx::query(
                        r#"
SELECT id, payment_order_id, status, order_no, gateway_order_id
FROM payment_callbacks
WHERE callback_key = $1
LIMIT 1
                        "#,
                    )
                    .bind(&input.callback_key)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    let duplicate = existing_callback.is_some();
                    let callback_id = if let Some(row) = existing_callback.as_ref() {
                        let status: String = row_get(row, "status")?;
                        if status == "processed" {
                            return Ok(ProcessPaymentCallbackOutcome::DuplicateProcessed {
                                order_id: row_get(row, "payment_order_id")?,
                            });
                        }
                        row_get(row, "id")?
                    } else {
                        let callback_id = Uuid::new_v4().to_string();
                        sqlx::query(
                            r#"
INSERT INTO payment_callbacks (
  id,
  payment_order_id,
  payment_method,
  callback_key,
  order_no,
  gateway_order_id,
  payload_hash,
  signature_valid,
  status,
  payload,
  error_message,
  created_at,
  processed_at
)
VALUES (
  $1,
  NULL,
  $2,
  $3,
  $4,
  $5,
  $6,
  $7,
  'received',
  $8,
  NULL,
  NOW(),
  NULL
)
                            "#,
                        )
                        .bind(&callback_id)
                        .bind(&input.payment_method)
                        .bind(&input.callback_key)
                        .bind(input.order_no.as_deref())
                        .bind(input.gateway_order_id.as_deref())
                        .bind(&input.payload_hash)
                        .bind(input.signature_valid)
                        .bind(&input.payload)
                        .execute(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        callback_id
                    };

                    if !input.signature_valid {
                        update_payment_callback_failure(
                            tx,
                            &callback_id,
                            &input,
                            "invalid callback signature",
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed {
                            duplicate,
                            error: "invalid callback signature".to_string(),
                        });
                    }

                    let lookup_order_no = input.order_no.clone().or_else(|| {
                        existing_callback
                            .as_ref()
                            .and_then(|row| row.try_get("order_no").ok())
                    });
                    let lookup_gateway_order_id = input.gateway_order_id.clone().or_else(|| {
                        existing_callback
                            .as_ref()
                            .and_then(|row| row.try_get("gateway_order_id").ok())
                    });

                    let order_lookup_by_order_no_sql = format!(
                        r#"
SELECT
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
FROM payment_orders
WHERE order_no = $1
LIMIT 1
FOR UPDATE
                            "#,
                    );
                    let order_lookup_by_gateway_order_id_sql = format!(
                        r#"
SELECT
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
FROM payment_orders
WHERE gateway_order_id = $1
LIMIT 1
FOR UPDATE
                            "#,
                    );

                    let order_row = if let Some(order_no) = lookup_order_no.as_deref() {
                        sqlx::query(&order_lookup_by_order_no_sql)
                            .bind(order_no)
                            .fetch_optional(&mut **tx)
                            .await
                            .map_postgres_err()?
                    } else if let Some(gateway_order_id) = lookup_gateway_order_id.as_deref() {
                        sqlx::query(&order_lookup_by_gateway_order_id_sql)
                            .bind(gateway_order_id)
                            .fetch_optional(&mut **tx)
                            .await
                            .map_postgres_err()?
                    } else {
                        None
                    };

                    let Some(order_row) = order_row else {
                        update_payment_callback_failure(
                            tx,
                            &callback_id,
                            &input,
                            "payment order not found",
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed {
                            duplicate,
                            error: "payment order not found".to_string(),
                        });
                    };

                    let order_id: String = row_get(&order_row, "id")?;
                    let order_no: String = row_get(&order_row, "order_no")?;
                    let order_wallet_id: String = row_get(&order_row, "wallet_id")?;
                    let order_payment_method: String = row_get(&order_row, "payment_method")?;
                    let order_payment_provider: Option<String> =
                        row_get(&order_row, "payment_provider")?;
                    let order_payment_channel: Option<String> =
                        row_get(&order_row, "payment_channel")?;
                    let now = Utc::now();
                    let order_kind: String = row_get(&order_row, "order_kind")?;
                    let order_amount_usd: f64 = row_get(&order_row, "amount_usd")?;
                    let order_debt_repayment_usd: f64 = row_get(&order_row, "debt_repayment_usd")?;
                    let order_pay_amount: Option<f64> = row_get(&order_row, "pay_amount")?;
                    let order_pay_currency: Option<String> = row_get(&order_row, "pay_currency")?;
                    let order_exchange_rate: Option<f64> = row_get(&order_row, "exchange_rate")?;
                    let order_status: String = row_get(&order_row, "status")?;
                    let expires_at_unix_secs: Option<i64> =
                        row_get(&order_row, "expires_at_unix_secs")?;

                    let Some(settled_amount_usd) = payment_callback_settled_amount_usd(
                        input.amount_usd,
                        input.pay_amount,
                        order_amount_usd,
                        order_pay_amount,
                        order_exchange_rate,
                    ) else {
                        update_payment_callback_failure(
                            tx,
                            &callback_id,
                            &input,
                            "callback amount invalid",
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed {
                            duplicate,
                            error: "callback amount invalid".to_string(),
                        });
                    };
                    if !payment_callback_currency_matches(
                        input.pay_currency.as_deref(),
                        order_pay_currency.as_deref(),
                    ) {
                        update_payment_callback_failure(
                            tx,
                            &callback_id,
                            &input,
                            "payment currency mismatch",
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed {
                            duplicate,
                            error: "payment currency mismatch".to_string(),
                        });
                    }
                    if !payment_callback_amount_matches(
                        settled_amount_usd,
                        input.pay_amount,
                        order_amount_usd,
                        order_pay_amount,
                    ) {
                        update_payment_callback_failure(
                            tx,
                            &callback_id,
                            &input,
                            "callback amount mismatch",
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed {
                            duplicate,
                            error: "callback amount mismatch".to_string(),
                        });
                    }
                    if !order_payment_method.eq_ignore_ascii_case(&input.payment_method) {
                        update_payment_callback_failure(
                            tx,
                            &callback_id,
                            &input,
                            "payment method mismatch",
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed {
                            duplicate,
                            error: "payment method mismatch".to_string(),
                        });
                    }
                    if let Some(expected_provider) = input.payment_provider.as_deref() {
                        if order_payment_provider
                            .as_deref()
                            .is_some_and(|value| !value.eq_ignore_ascii_case(expected_provider))
                        {
                            update_payment_callback_failure(
                                tx,
                                &callback_id,
                                &input,
                                "payment provider mismatch",
                            )
                            .await?;
                            return Ok(ProcessPaymentCallbackOutcome::Failed {
                                duplicate,
                                error: "payment provider mismatch".to_string(),
                            });
                        }
                    }
                    if let Some(expected_channel) = input.payment_channel.as_deref() {
                        if order_payment_channel
                            .as_deref()
                            .is_some_and(|value| !value.eq_ignore_ascii_case(expected_channel))
                        {
                            update_payment_callback_failure(
                                tx,
                                &callback_id,
                                &input,
                                "payment channel mismatch",
                            )
                            .await?;
                            return Ok(ProcessPaymentCallbackOutcome::Failed {
                                duplicate,
                                error: "payment channel mismatch".to_string(),
                            });
                        }
                    }
                    if order_status == "credited" {
                        mark_payment_callback_processed(
                            tx,
                            &callback_id,
                            &input,
                            &order_id,
                            &order_no,
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::AlreadyCredited {
                            duplicate,
                            order_id,
                            order_no,
                            wallet_id: order_wallet_id,
                        });
                    }
                    if matches!(
                        order_status.as_str(),
                        "failed" | "expired" | "refunded" | "cancelled"
                    ) {
                        let error = format!("payment order is not creditable: {order_status}");
                        update_payment_callback_failure(tx, &callback_id, &input, &error).await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed { duplicate, error });
                    }
                    if order_status == "pending" {
                        let now = Utc::now().timestamp();
                        if expires_at_unix_secs.is_some_and(|value| value < now) {
                            sqlx::query(
                                "UPDATE payment_orders SET status = 'expired' WHERE id = $1",
                            )
                            .bind(&order_id)
                            .execute(&mut **tx)
                            .await
                            .map_postgres_err()?;
                            update_payment_callback_failure(
                                tx,
                                &callback_id,
                                &input,
                                "payment order expired",
                            )
                            .await?;
                            return Ok(ProcessPaymentCallbackOutcome::Failed {
                                duplicate,
                                error: "payment order expired".to_string(),
                            });
                        }
                    }

                    if order_kind == "plan_purchase" {
                        let product_id: Option<String> = row_get(&order_row, "product_id")?;
                        let product_snapshot: Option<serde_json::Value> =
                            row_get(&order_row, "product_snapshot")?;
                        let order_user_id: Option<String> = row_get(&order_row, "user_id")?;
                        let Some(user_id) = order_user_id else {
                            update_payment_callback_failure(
                                tx,
                                &callback_id,
                                &input,
                                "payment order user missing",
                            )
                            .await?;
                            return Ok(ProcessPaymentCallbackOutcome::Failed {
                                duplicate,
                                error: "payment order user missing".to_string(),
                            });
                        };
                        let snapshot = product_snapshot.unwrap_or_else(|| serde_json::json!({}));
                        let plan_id = product_id.unwrap_or_else(|| {
                            snapshot
                                .get("id")
                                .and_then(|value| value.as_str())
                                .unwrap_or("unknown")
                                .to_string()
                        });
                        let existing_entitlement_id = sqlx::query_scalar::<_, String>(
                            r#"
SELECT id
FROM user_plan_entitlements
WHERE payment_order_id = $1
LIMIT 1
                            "#,
                        )
                        .bind(&order_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        if existing_entitlement_id.is_none() {
                            lock_plan_renewal_scope_postgres(tx, &order_wallet_id).await?;
                            let requested_starts_at = plan_starts_at(&snapshot, now);
                            let starts_at = plan_renewal_starts_at_postgres(
                                tx,
                                &user_id,
                                &plan_id,
                                requested_starts_at,
                            )
                            .await?;
                            let expires_at = plan_expires_at(&snapshot, starts_at);
                            if has_other_overlapping_plan_postgres(
                                tx, &user_id, &plan_id, starts_at, expires_at,
                            )
                            .await?
                            {
                                update_payment_callback_failure(
                                    tx,
                                    &callback_id,
                                    &input,
                                    "another plan overlaps this entitlement",
                                )
                                .await?;
                                return Ok(ProcessPaymentCallbackOutcome::Failed {
                                    duplicate,
                                    error: "another plan overlaps this entitlement".to_string(),
                                });
                            }
                            let entitlements = plan_entitlements_snapshot(&snapshot);
                            let purchase_limit_scope = plan_purchase_limit_scope(&snapshot);
                            if purchase_limit_scope != "unlimited" {
                                let max_active_per_user = plan_max_active_per_user(&snapshot);
                                let active_count = if purchase_limit_scope == "lifetime" {
                                    sqlx::query_scalar::<_, i64>(
                                        r#"
SELECT COUNT(*)::bigint
FROM user_plan_entitlements
WHERE user_id = $1
  AND plan_id = $2
  AND status = 'active'
                                "#,
                                    )
                                    .bind(&user_id)
                                    .bind(&plan_id)
                                    .fetch_one(&mut **tx)
                                    .await
                                    .map_postgres_err()?
                                } else {
                                    0
                                };
                                if active_count >= max_active_per_user {
                                    update_payment_callback_failure(
                                        tx,
                                        &callback_id,
                                        &input,
                                        "plan purchase limit reached",
                                    )
                                    .await?;
                                    return Ok(ProcessPaymentCallbackOutcome::Failed {
                                        duplicate,
                                        error: "plan purchase limit reached".to_string(),
                                    });
                                }
                            }
                            apply_plan_debt_repayment_postgres(
                                tx,
                                &order_wallet_id,
                                &order_id,
                                &order_payment_method,
                                order_debt_repayment_usd,
                            )
                            .await?;
                            let entitlement_id = Uuid::new_v4().to_string();
                            sqlx::query(
                                r#"
INSERT INTO user_plan_entitlements (
  id, user_id, plan_id, payment_order_id, status, starts_at, expires_at,
  entitlements_snapshot, created_at, updated_at
)
VALUES ($1, $2, $3, $4, 'active', $5, $6, $7, NOW(), NOW())
                                "#,
                            )
                            .bind(&entitlement_id)
                            .bind(&user_id)
                            .bind(&plan_id)
                            .bind(&order_id)
                            .bind(starts_at)
                            .bind(expires_at)
                            .bind(&entitlements)
                            .execute(&mut **tx)
                            .await
                            .map_postgres_err()?;
                            save_entitlement_providers_postgres(
                                tx,
                                &entitlement_id,
                                &plan_provider_ids_snapshot(&snapshot),
                            )
                            .await?;
                            apply_plan_wallet_credit_postgres(
                                tx,
                                &order_wallet_id,
                                &order_id,
                                &input.payment_method,
                                &entitlements,
                            )
                            .await?;
                        }
                        let sql = format!(
                            r#"
UPDATE payment_orders
SET gateway_order_id = COALESCE($2, gateway_order_id),
    gateway_response = $3,
    pay_amount = COALESCE($4, pay_amount),
    pay_currency = COALESCE(pay_currency, $5),
    exchange_rate = COALESCE(exchange_rate, $6),
    payment_provider = COALESCE(payment_provider, $7),
    payment_channel = COALESCE(payment_channel, $8),
    status = 'credited',
    fulfillment_status = 'fulfilled',
    fulfillment_error = NULL,
    paid_at = COALESCE(paid_at, NOW()),
    credited_at = NOW(),
    refundable_amount_usd = 0
WHERE id = $1
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                            "#,
                        );
                        let updated_order_row = sqlx::query(&sql)
                            .bind(&order_id)
                            .bind(input.gateway_order_id.as_deref())
                            .bind(&input.payload)
                            .bind(input.pay_amount)
                            .bind(input.pay_currency.as_deref())
                            .bind(input.exchange_rate)
                            .bind(input.payment_provider.as_deref())
                            .bind(input.payment_channel.as_deref())
                            .fetch_one(&mut **tx)
                            .await
                            .map_postgres_err()?;
                        mark_payment_callback_processed(
                            tx,
                            &callback_id,
                            &input,
                            &order_id,
                            &order_no,
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Applied {
                            duplicate,
                            order_id,
                            order_no,
                            wallet_id: order_wallet_id,
                            order: map_admin_payment_order_row(&updated_order_row)?,
                        });
                    }

                    let Some(wallet_row) = sqlx::query(
                        r#"
SELECT
  id,
  status,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance
FROM wallets
WHERE id = $1
LIMIT 1
FOR UPDATE
                        "#,
                    )
                    .bind(&order_wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        update_payment_callback_failure(
                            tx,
                            &callback_id,
                            &input,
                            "wallet not found",
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed {
                            duplicate,
                            error: "wallet not found".to_string(),
                        });
                    };
                    let wallet_status: String = row_get(&wallet_row, "status")?;
                    if wallet_status != "active" {
                        update_payment_callback_failure(
                            tx,
                            &callback_id,
                            &input,
                            "wallet is not active",
                        )
                        .await?;
                        return Ok(ProcessPaymentCallbackOutcome::Failed {
                            duplicate,
                            error: "wallet is not active".to_string(),
                        });
                    }

                    let before_recharge: f64 = row_get(&wallet_row, "balance")?;
                    let before_gift: f64 = row_get(&wallet_row, "gift_balance")?;
                    let before_total = before_recharge + before_gift;
                    let after_recharge = before_recharge + settled_amount_usd;
                    let after_total = after_recharge + before_gift;

                    sqlx::query(
                        r#"
UPDATE wallets
SET balance = $2,
    total_recharged = total_recharged + $3,
    updated_at = NOW()
WHERE id = $1
                        "#,
                    )
                    .bind(&order_wallet_id)
                    .bind(after_recharge)
                    .bind(settled_amount_usd)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    sqlx::query(
                        r#"
INSERT INTO wallet_transactions (
  id,
  wallet_id,
  category,
  reason_code,
  amount,
  balance_before,
  balance_after,
  recharge_balance_before,
  recharge_balance_after,
  gift_balance_before,
  gift_balance_after,
  link_type,
  link_id,
  operator_id,
  description,
  created_at
)
VALUES (
  $1,
  $2,
  'recharge',
  'topup_gateway',
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $9,
  'payment_order',
  $10,
  NULL,
  $11,
  NOW()
)
                        "#,
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&order_wallet_id)
                    .bind(settled_amount_usd)
                    .bind(before_total)
                    .bind(after_total)
                    .bind(before_recharge)
                    .bind(after_recharge)
                    .bind(before_gift)
                    .bind(before_gift)
                    .bind(&order_id)
                    .bind(format!("充值到账({})", input.payment_method))
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    let sql = format!(
                        r#"
UPDATE payment_orders
SET gateway_order_id = COALESCE($2, gateway_order_id),
    gateway_response = $3,
    pay_amount = COALESCE($4, pay_amount),
    pay_currency = COALESCE(pay_currency, $5),
    exchange_rate = COALESCE(exchange_rate, $6),
    payment_provider = COALESCE(payment_provider, $7),
    payment_channel = COALESCE(payment_channel, $8),
    amount_usd = $9,
    status = 'credited',
    paid_at = COALESCE(paid_at, NOW()),
    credited_at = NOW(),
    refundable_amount_usd = $9
WHERE id = $1
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let updated_order_row = sqlx::query(&sql)
                        .bind(&order_id)
                        .bind(input.gateway_order_id.as_deref())
                        .bind(&input.payload)
                        .bind(input.pay_amount)
                        .bind(input.pay_currency.as_deref())
                        .bind(input.exchange_rate)
                        .bind(input.payment_provider.as_deref())
                        .bind(input.payment_channel.as_deref())
                        .bind(settled_amount_usd)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    mark_payment_callback_processed(tx, &callback_id, &input, &order_id, &order_no)
                        .await?;
                    Ok(ProcessPaymentCallbackOutcome::Applied {
                        duplicate,
                        order_id,
                        order_no,
                        wallet_id: order_wallet_id,
                        order: map_admin_payment_order_row(&updated_order_row)?,
                    })
                })
            })
            .await
    }

    async fn adjust_wallet_balance(
        &self,
        input: AdjustWalletBalanceInput,
    ) -> Result<Option<(StoredWalletSnapshot, StoredAdminWalletTransaction)>, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(row) = sqlx::query(
                        r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted
FROM wallets
WHERE id = $1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(None);
                    };

                    let before_recharge: f64 = row_get(&row, "balance")?;
                    let before_gift: f64 = row_get(&row, "gift_balance")?;
                    let before_total = before_recharge + before_gift;
                    let mut after_recharge = before_recharge;
                    let mut after_gift = before_gift;

                    if input.amount_usd > 0.0 {
                        if input.balance_type.eq_ignore_ascii_case("gift") {
                            after_gift += input.amount_usd;
                        } else {
                            after_recharge += input.amount_usd;
                        }
                    } else {
                        let mut remaining = -input.amount_usd;
                        let consume_positive_bucket = |balance: &mut f64, to_consume: &mut f64| {
                            if *to_consume <= 0.0 {
                                return;
                            }
                            let available = (*balance).max(0.0);
                            let consumed = available.min(*to_consume);
                            *balance -= consumed;
                            *to_consume -= consumed;
                        };
                        if input.balance_type.eq_ignore_ascii_case("gift") {
                            consume_positive_bucket(&mut after_gift, &mut remaining);
                            consume_positive_bucket(&mut after_recharge, &mut remaining);
                        } else {
                            consume_positive_bucket(&mut after_recharge, &mut remaining);
                            consume_positive_bucket(&mut after_gift, &mut remaining);
                        }
                        if remaining > 0.0 {
                            after_recharge -= remaining;
                        }
                    }

                    let wallet_row = sqlx::query(
                        r#"
UPDATE wallets
SET
  balance = $2,
  gift_balance = $3,
  total_adjusted = total_adjusted + $4,
  updated_at = NOW()
WHERE id = $1
RETURNING
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .bind(after_recharge)
                    .bind(after_gift)
                    .bind(input.amount_usd)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    let wallet = map_wallet_row(&wallet_row)?;

                    let transaction_id = Uuid::new_v4().to_string();
                    let created_at = Utc::now().timestamp().max(0) as u64;
                    let description = input
                        .description
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("管理员调账")
                        .to_string();
                    sqlx::query(
                        r#"
INSERT INTO wallet_transactions (
  id,
  wallet_id,
  category,
  reason_code,
  amount,
  balance_before,
  balance_after,
  recharge_balance_before,
  recharge_balance_after,
  gift_balance_before,
  gift_balance_after,
  link_type,
  link_id,
  operator_id,
  description,
  created_at
)
VALUES (
  $1,
  $2,
  'adjust',
  'adjust_admin',
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $9,
  'admin_action',
  $10,
  $11,
  $12,
  NOW()
)
                        "#,
                    )
                    .bind(&transaction_id)
                    .bind(&input.wallet_id)
                    .bind(input.amount_usd)
                    .bind(before_total)
                    .bind(after_recharge + after_gift)
                    .bind(before_recharge)
                    .bind(after_recharge)
                    .bind(before_gift)
                    .bind(after_gift)
                    .bind(&input.wallet_id)
                    .bind(input.operator_id.as_deref())
                    .bind(&description)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    Ok(Some((
                        wallet,
                        StoredAdminWalletTransaction {
                            id: transaction_id,
                            wallet_id: input.wallet_id,
                            category: "adjust".to_string(),
                            reason_code: "adjust_admin".to_string(),
                            amount: input.amount_usd,
                            balance_before: before_total,
                            balance_after: after_recharge + after_gift,
                            recharge_balance_before: before_recharge,
                            recharge_balance_after: after_recharge,
                            gift_balance_before: before_gift,
                            gift_balance_after: after_gift,
                            link_type: Some("admin_action".to_string()),
                            link_id: Some(row_get(&wallet_row, "id")?),
                            operator_id: input.operator_id,
                            operator_name: None,
                            operator_email: None,
                            description: Some(description),
                            created_at_unix_ms: Some(created_at),
                        },
                    )))
                })
            })
            .await
    }

    async fn create_manual_wallet_recharge(
        &self,
        input: CreateManualWalletRechargeInput,
    ) -> Result<Option<(StoredWalletSnapshot, StoredAdminPaymentOrder)>, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(wallet_row) = sqlx::query(
                        r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted
FROM wallets
WHERE id = $1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(None);
                    };

                    let before_recharge: f64 = row_get(&wallet_row, "balance")?;
                    let before_gift: f64 = row_get(&wallet_row, "gift_balance")?;
                    let user_id: Option<String> = row_get(&wallet_row, "user_id")?;
                    let gateway_response = serde_json::json!({
                        "source": "manual",
                        "operator_id": input.operator_id,
                        "description": input.description,
                    });

                    let order_id = Uuid::new_v4().to_string();
                    sqlx::query(
                        r#"
INSERT INTO payment_orders (
  id,
  order_no,
  wallet_id,
  user_id,
  amount_usd,
  refunded_amount_usd,
  refundable_amount_usd,
  payment_method,
  status,
  gateway_response,
  created_at,
  paid_at,
  credited_at
)
VALUES (
  $1,
  $2,
  $3,
  $4,
  $5,
  0,
  $5,
  $6,
  'credited',
  $7,
  NOW(),
  NOW(),
  NOW()
)
                        "#,
                    )
                    .bind(&order_id)
                    .bind(&input.order_no)
                    .bind(&input.wallet_id)
                    .bind(user_id.as_deref())
                    .bind(input.amount_usd)
                    .bind(&input.payment_method)
                    .bind(&gateway_response)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    let after_recharge = before_recharge + input.amount_usd;
                    let wallet_row = sqlx::query(
                        r#"
UPDATE wallets
SET
  balance = $2,
  total_recharged = total_recharged + $3,
  updated_at = NOW()
WHERE id = $1
RETURNING
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .bind(after_recharge)
                    .bind(input.amount_usd)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    let wallet = map_wallet_row(&wallet_row)?;

                    let reason_code = if matches!(
                        input.payment_method.as_str(),
                        "card_code" | "gift_code" | "card_recharge"
                    ) {
                        "topup_card_code"
                    } else {
                        "topup_admin_manual"
                    };
                    sqlx::query(
                        r#"
INSERT INTO wallet_transactions (
  id,
  wallet_id,
  category,
  reason_code,
  amount,
  balance_before,
  balance_after,
  recharge_balance_before,
  recharge_balance_after,
  gift_balance_before,
  gift_balance_after,
  link_type,
  link_id,
  operator_id,
  description,
  created_at
)
VALUES (
  $1,
  $2,
  'recharge',
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $9,
  $9,
  'payment_order',
  $10,
  $11,
  $12,
  NOW()
)
                        "#,
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&input.wallet_id)
                    .bind(reason_code)
                    .bind(input.amount_usd)
                    .bind(before_recharge + before_gift)
                    .bind(after_recharge + before_gift)
                    .bind(before_recharge)
                    .bind(after_recharge)
                    .bind(before_gift)
                    .bind(&order_id)
                    .bind(input.operator_id.as_deref())
                    .bind(
                        input
                            .description
                            .as_deref()
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or("管理员手动充值"),
                    )
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    let sql = format!(
                        r#"
SELECT
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
FROM payment_orders
WHERE id = $1
LIMIT 1
                        "#,
                    );
                    let order_row = sqlx::query(&sql)
                        .bind(&order_id)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    Ok(Some((wallet, map_admin_payment_order_row(&order_row)?)))
                })
            })
            .await
    }

    async fn process_admin_wallet_refund(
        &self,
        input: ProcessAdminWalletRefundInput,
    ) -> Result<
        WalletMutationOutcome<(
            StoredWalletSnapshot,
            StoredAdminWalletRefund,
            StoredAdminWalletTransaction,
        )>,
        DataLayerError,
    > {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(refund_row) = sqlx::query(
                        r#"
SELECT
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
FROM refund_requests
WHERE id = $1 AND wallet_id = $2
FOR UPDATE
                        "#,
                    )
                    .bind(&input.refund_id)
                    .bind(&input.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let refund = map_admin_wallet_refund_row(&refund_row)?;
                    if !matches!(refund.status.as_str(), "approved" | "pending_approval") {
                        return Ok(WalletMutationOutcome::Invalid(
                            "refund status is not approvable".to_string(),
                        ));
                    }

                    let Some(wallet_row) = sqlx::query(
                        r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE id = $1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::Invalid(
                            "wallet not found".to_string(),
                        ));
                    };
                    let before_recharge: f64 = row_get(&wallet_row, "balance")?;
                    let before_gift: f64 = row_get(&wallet_row, "gift_balance")?;
                    let before_total = before_recharge + before_gift;
                    let amount_usd = refund.amount_usd;
                    let after_recharge = before_recharge - amount_usd;
                    if after_recharge < 0.0 {
                        return Ok(WalletMutationOutcome::Invalid(
                            "refund amount exceeds refundable recharge balance".to_string(),
                        ));
                    }

                    if let Some(payment_order_id) = refund.payment_order_id.as_deref() {
                        let Some(order_row) = sqlx::query(
                            r#"
SELECT
  id,
  CAST(refunded_amount_usd AS DOUBLE PRECISION) AS refunded_amount_usd,
  CAST(refundable_amount_usd AS DOUBLE PRECISION) AS refundable_amount_usd
FROM payment_orders
WHERE id = $1
FOR UPDATE
                            "#,
                        )
                        .bind(payment_order_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?
                        else {
                            return Ok(WalletMutationOutcome::Invalid(
                                "payment order not found".to_string(),
                            ));
                        };
                        let refundable_amount: f64 = row_get(&order_row, "refundable_amount_usd")?;
                        if amount_usd > refundable_amount {
                            return Ok(WalletMutationOutcome::Invalid(
                                "refund amount exceeds refundable amount".to_string(),
                            ));
                        }
                        sqlx::query(
                            r#"
UPDATE payment_orders
SET
  refunded_amount_usd = refunded_amount_usd + $2,
  refundable_amount_usd = refundable_amount_usd - $2
WHERE id = $1
                            "#,
                        )
                        .bind(payment_order_id)
                        .bind(amount_usd)
                        .execute(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    }

                    let wallet_row = sqlx::query(
                        r#"
UPDATE wallets
SET
  balance = $2,
  total_refunded = total_refunded + $3,
  updated_at = NOW()
WHERE id = $1
RETURNING
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .bind(after_recharge)
                    .bind(amount_usd)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    let wallet = map_wallet_row(&wallet_row)?;

                    let transaction_id = Uuid::new_v4().to_string();
                    let created_at_unix_ms = Utc::now().timestamp().max(0) as u64;
                    sqlx::query(
                        r#"
INSERT INTO wallet_transactions (
  id,
  wallet_id,
  category,
  reason_code,
  amount,
  balance_before,
  balance_after,
  recharge_balance_before,
  recharge_balance_after,
  gift_balance_before,
  gift_balance_after,
  link_type,
  link_id,
  operator_id,
  description,
  created_at
)
VALUES (
  $1,
  $2,
  'refund',
  'refund_out',
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $9,
  'refund_request',
  $10,
  $11,
  '退款占款',
  NOW()
)
                        "#,
                    )
                    .bind(&transaction_id)
                    .bind(&input.wallet_id)
                    .bind(-amount_usd)
                    .bind(before_total)
                    .bind(after_recharge + before_gift)
                    .bind(before_recharge)
                    .bind(after_recharge)
                    .bind(before_gift)
                    .bind(before_gift)
                    .bind(&input.refund_id)
                    .bind(input.operator_id.as_deref())
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    let refund_row = sqlx::query(
                        r#"
UPDATE refund_requests
SET
  status = 'processing',
  approved_by = $3,
  processed_by = $3,
  processed_at = NOW(),
  updated_at = NOW()
WHERE id = $1 AND wallet_id = $2
RETURNING
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
                        "#,
                    )
                    .bind(&input.refund_id)
                    .bind(&input.wallet_id)
                    .bind(input.operator_id.as_deref())
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied((
                        wallet,
                        map_admin_wallet_refund_row(&refund_row)?,
                        StoredAdminWalletTransaction {
                            id: transaction_id,
                            wallet_id: input.wallet_id.clone(),
                            category: "refund".to_string(),
                            reason_code: "refund_out".to_string(),
                            amount: -amount_usd,
                            balance_before: before_total,
                            balance_after: after_recharge + before_gift,
                            recharge_balance_before: before_recharge,
                            recharge_balance_after: after_recharge,
                            gift_balance_before: before_gift,
                            gift_balance_after: before_gift,
                            link_type: Some("refund_request".to_string()),
                            link_id: Some(input.refund_id.clone()),
                            operator_id: input.operator_id.clone(),
                            operator_name: None,
                            operator_email: None,
                            description: Some("退款占款".to_string()),
                            created_at_unix_ms: Some(created_at_unix_ms),
                        },
                    )))
                })
            })
            .await
    }

    async fn complete_admin_wallet_refund(
        &self,
        input: CompleteAdminWalletRefundInput,
    ) -> Result<WalletMutationOutcome<StoredAdminWalletRefund>, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(current_refund) = sqlx::query(
                        r#"
SELECT status
FROM refund_requests
WHERE id = $1 AND wallet_id = $2
FOR UPDATE
                        "#,
                    )
                    .bind(&input.refund_id)
                    .bind(&input.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let status: String = row_get(&current_refund, "status")?;
                    if status != "processing" {
                        return Ok(WalletMutationOutcome::Invalid(
                            "refund status must be processing before completion".to_string(),
                        ));
                    }

                    let refund_row = sqlx::query(
                        r#"
UPDATE refund_requests
SET
  status = 'succeeded',
  gateway_refund_id = $3,
  payout_reference = $4,
  payout_proof = $5,
  completed_at = NOW(),
  updated_at = NOW()
WHERE id = $1 AND wallet_id = $2
RETURNING
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
                        "#,
                    )
                    .bind(&input.refund_id)
                    .bind(&input.wallet_id)
                    .bind(input.gateway_refund_id.as_deref())
                    .bind(input.payout_reference.as_deref())
                    .bind(&input.payout_proof)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied(map_admin_wallet_refund_row(
                        &refund_row,
                    )?))
                })
            })
            .await
    }

    async fn fail_admin_wallet_refund(
        &self,
        input: FailAdminWalletRefundInput,
    ) -> Result<
        WalletMutationOutcome<(
            StoredWalletSnapshot,
            StoredAdminWalletRefund,
            Option<StoredAdminWalletTransaction>,
        )>,
        DataLayerError,
    > {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(refund_row) = sqlx::query(
                        r#"
SELECT
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
FROM refund_requests
WHERE id = $1 AND wallet_id = $2
FOR UPDATE
                        "#,
                    )
                    .bind(&input.refund_id)
                    .bind(&input.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let refund = map_admin_wallet_refund_row(&refund_row)?;

                    if matches!(refund.status.as_str(), "pending_approval" | "approved") {
                        let refund_row = sqlx::query(
                            r#"
UPDATE refund_requests
SET
  status = 'failed',
  failure_reason = $3,
  updated_at = NOW()
WHERE id = $1 AND wallet_id = $2
RETURNING
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
                            "#,
                        )
                        .bind(&input.refund_id)
                        .bind(&input.wallet_id)
                        .bind(&input.reason)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        let wallet_row = sqlx::query(
                            r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE id = $1
                            "#,
                        )
                        .bind(&input.wallet_id)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        return Ok(WalletMutationOutcome::Applied((
                            map_wallet_row(&wallet_row)?,
                            map_admin_wallet_refund_row(&refund_row)?,
                            None,
                        )));
                    }

                    if refund.status != "processing" {
                        return Ok(WalletMutationOutcome::Invalid(format!(
                            "cannot fail refund in status: {}",
                            refund.status
                        )));
                    }

                    let Some(wallet_row) = sqlx::query(
                        r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE id = $1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::Invalid(
                            "wallet not found".to_string(),
                        ));
                    };
                    let amount_usd = refund.amount_usd;
                    let before_recharge: f64 = row_get(&wallet_row, "balance")?;
                    let before_gift: f64 = row_get(&wallet_row, "gift_balance")?;
                    let before_total = before_recharge + before_gift;
                    let after_recharge = before_recharge + amount_usd;

                    let wallet_row = sqlx::query(
                        r#"
UPDATE wallets
SET
  balance = $2,
  total_refunded = GREATEST(total_refunded - $3, 0),
  updated_at = NOW()
WHERE id = $1
RETURNING
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
                        "#,
                    )
                    .bind(&input.wallet_id)
                    .bind(after_recharge)
                    .bind(amount_usd)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    let wallet = map_wallet_row(&wallet_row)?;

                    let transaction_id = Uuid::new_v4().to_string();
                    let created_at_unix_ms = Utc::now().timestamp().max(0) as u64;
                    sqlx::query(
                        r#"
INSERT INTO wallet_transactions (
  id,
  wallet_id,
  category,
  reason_code,
  amount,
  balance_before,
  balance_after,
  recharge_balance_before,
  recharge_balance_after,
  gift_balance_before,
  gift_balance_after,
  link_type,
  link_id,
  operator_id,
  description,
  created_at
)
VALUES (
  $1,
  $2,
  'refund',
  'refund_revert',
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $9,
  'refund_request',
  $10,
  $11,
  '退款失败回补',
  NOW()
)
                        "#,
                    )
                    .bind(&transaction_id)
                    .bind(&input.wallet_id)
                    .bind(amount_usd)
                    .bind(before_total)
                    .bind(after_recharge + before_gift)
                    .bind(before_recharge)
                    .bind(after_recharge)
                    .bind(before_gift)
                    .bind(before_gift)
                    .bind(&input.refund_id)
                    .bind(input.operator_id.as_deref())
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    if let Some(payment_order_id) = refund.payment_order_id.as_deref() {
                        let _ = sqlx::query(
                            r#"
UPDATE payment_orders
SET
  refunded_amount_usd = refunded_amount_usd - $2,
  refundable_amount_usd = refundable_amount_usd + $2
WHERE id = $1
                            "#,
                        )
                        .bind(payment_order_id)
                        .bind(amount_usd)
                        .execute(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    }

                    let refund_row = sqlx::query(
                        r#"
UPDATE refund_requests
SET
  status = 'failed',
  failure_reason = $3,
  updated_at = NOW()
WHERE id = $1 AND wallet_id = $2
RETURNING
  id,
  refund_no,
  wallet_id,
  user_id,
  payment_order_id,
  source_type,
  source_id,
  refund_mode,
  CAST(amount_usd AS DOUBLE PRECISION) AS amount_usd,
  status,
  reason,
  failure_reason,
  gateway_refund_id,
  payout_method,
  payout_reference,
  payout_proof,
  requested_by,
  approved_by,
  processed_by,
  CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM processed_at) AS BIGINT) AS processed_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM completed_at) AS BIGINT) AS completed_at_unix_secs
                        "#,
                    )
                    .bind(&input.refund_id)
                    .bind(&input.wallet_id)
                    .bind(&input.reason)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied((
                        wallet,
                        map_admin_wallet_refund_row(&refund_row)?,
                        Some(StoredAdminWalletTransaction {
                            id: transaction_id,
                            wallet_id: input.wallet_id.clone(),
                            category: "refund".to_string(),
                            reason_code: "refund_revert".to_string(),
                            amount: amount_usd,
                            balance_before: before_total,
                            balance_after: after_recharge + before_gift,
                            recharge_balance_before: before_recharge,
                            recharge_balance_after: after_recharge,
                            gift_balance_before: before_gift,
                            gift_balance_after: before_gift,
                            link_type: Some("refund_request".to_string()),
                            link_id: Some(input.refund_id.clone()),
                            operator_id: input.operator_id.clone(),
                            operator_name: None,
                            operator_email: None,
                            description: Some("退款失败回补".to_string()),
                            created_at_unix_ms: Some(created_at_unix_ms),
                        }),
                    )))
                })
            })
            .await
    }

    async fn expire_admin_payment_order(
        &self,
        order_id: &str,
    ) -> Result<WalletMutationOutcome<(StoredAdminPaymentOrder, bool)>, DataLayerError> {
        let order_id = order_id.to_string();
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let sql = format!(
                        r#"
SELECT
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
FROM payment_orders
WHERE id = $1
FOR UPDATE
                        "#,
                    );
                    let Some(row) = sqlx::query(&sql)
                        .bind(&order_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let order = map_admin_payment_order_row(&row)?;
                    if order.status == "credited" {
                        return Ok(WalletMutationOutcome::Invalid(
                            "credited order cannot be expired".to_string(),
                        ));
                    }
                    if order.status == "expired" {
                        return Ok(WalletMutationOutcome::Applied((order, false)));
                    }
                    if order.status != "pending" {
                        return Ok(WalletMutationOutcome::Invalid(format!(
                            "only pending order can be expired: {}",
                            order.status
                        )));
                    }
                    let mut gateway_response =
                        payment_gateway_response_map(order.gateway_response.clone());
                    gateway_response.insert(
                        "expire_reason".to_string(),
                        serde_json::Value::String("admin_mark_expired".to_string()),
                    );
                    gateway_response.insert(
                        "expired_at".to_string(),
                        serde_json::Value::String(Utc::now().to_rfc3339()),
                    );
                    let sql = format!(
                        r#"
UPDATE payment_orders
SET
  status = 'expired',
  gateway_response = $2
WHERE id = $1
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let row = sqlx::query(&sql)
                        .bind(&order_id)
                        .bind(serde_json::Value::Object(gateway_response))
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied((
                        map_admin_payment_order_row(&row)?,
                        true,
                    )))
                })
            })
            .await
    }

    async fn fail_admin_payment_order(
        &self,
        order_id: &str,
    ) -> Result<WalletMutationOutcome<StoredAdminPaymentOrder>, DataLayerError> {
        let order_id = order_id.to_string();
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let sql = format!(
                        r#"
SELECT
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
FROM payment_orders
WHERE id = $1
FOR UPDATE
                        "#,
                    );
                    let Some(row) = sqlx::query(&sql)
                        .bind(&order_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let order = map_admin_payment_order_row(&row)?;
                    if order.status == "credited" {
                        return Ok(WalletMutationOutcome::Invalid(
                            "credited order cannot be failed".to_string(),
                        ));
                    }
                    let mut gateway_response =
                        payment_gateway_response_map(order.gateway_response.clone());
                    gateway_response.insert(
                        "failure_reason".to_string(),
                        serde_json::Value::String("admin_mark_failed".to_string()),
                    );
                    gateway_response.insert(
                        "failed_at".to_string(),
                        serde_json::Value::String(Utc::now().to_rfc3339()),
                    );
                    let sql = format!(
                        r#"
UPDATE payment_orders
SET
  status = 'failed',
  gateway_response = $2
WHERE id = $1
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let row = sqlx::query(&sql)
                        .bind(&order_id)
                        .bind(serde_json::Value::Object(gateway_response))
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied(map_admin_payment_order_row(
                        &row,
                    )?))
                })
            })
            .await
    }

    async fn cancel_payment_order(
        &self,
        input: CancelPaymentOrderInput,
    ) -> Result<WalletMutationOutcome<(StoredAdminPaymentOrder, bool)>, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let sql = format!(
                        r#"
SELECT
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
FROM payment_orders
WHERE order_no = $1
FOR UPDATE
                        "#,
                    );
                    let Some(row) = sqlx::query(&sql)
                        .bind(&input.order_no)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let order = map_admin_payment_order_row(&row)?;
                    if let Some(expected_provider) = input.expected_payment_provider.as_deref() {
                        if !order
                            .payment_provider
                            .as_deref()
                            .is_some_and(|value| value.eq_ignore_ascii_case(expected_provider))
                        {
                            return Ok(WalletMutationOutcome::Invalid(format!(
                                "payment provider is not cancellable: {}",
                                order.payment_provider.as_deref().unwrap_or_default()
                            )));
                        }
                    }
                    if order.status == "cancelled" {
                        return Ok(WalletMutationOutcome::Applied((order, false)));
                    }
                    if order.status != "pending" {
                        return Ok(WalletMutationOutcome::Invalid(format!(
                            "payment order is not cancellable: {}",
                            order.status
                        )));
                    }
                    let mut gateway_response =
                        payment_gateway_response_map(order.gateway_response.clone());
                    gateway_response.insert(
                        "cancel_reason".to_string(),
                        serde_json::Value::String(input.cancel_reason),
                    );
                    gateway_response.insert(
                        "cancel_source".to_string(),
                        serde_json::Value::String(input.cancel_source),
                    );
                    gateway_response.insert(
                        "cancelled_at".to_string(),
                        serde_json::Value::String(Utc::now().to_rfc3339()),
                    );
                    let sql = format!(
                        r#"
UPDATE payment_orders
SET
  status = 'cancelled',
  fulfillment_status = 'cancelled',
  gateway_response = $2
WHERE order_no = $1
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let row = sqlx::query(&sql)
                        .bind(&input.order_no)
                        .bind(serde_json::Value::Object(gateway_response))
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied((
                        map_admin_payment_order_row(&row)?,
                        true,
                    )))
                })
            })
            .await
    }

    async fn update_pending_payment_order_gateway(
        &self,
        input: UpdatePendingPaymentOrderGatewayInput,
    ) -> Result<WalletMutationOutcome<StoredAdminPaymentOrder>, DataLayerError> {
        if input.gateway_order_id.trim().is_empty() {
            return Err(DataLayerError::InvalidInput(
                "gateway_order_id is required".to_string(),
            ));
        }
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let sql = format!(
                        r#"
SELECT
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
FROM payment_orders
WHERE id = $1
FOR UPDATE
                        "#,
                    );
                    let Some(row) = sqlx::query(&sql)
                        .bind(&input.order_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let order = map_admin_payment_order_row(&row)?;
                    if order.status != "pending" {
                        return Ok(WalletMutationOutcome::Invalid(format!(
                            "payment order is not pending: {}",
                            order.status
                        )));
                    }
                    let sql = format!(
                        r#"
UPDATE payment_orders
SET
  gateway_order_id = $2,
  gateway_response = $3
WHERE id = $1
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let row = sqlx::query(&sql)
                        .bind(&input.order_id)
                        .bind(input.gateway_order_id.trim())
                        .bind(input.gateway_response)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied(map_admin_payment_order_row(
                        &row,
                    )?))
                })
            })
            .await
    }

    async fn credit_admin_payment_order(
        &self,
        input: CreditAdminPaymentOrderInput,
    ) -> Result<WalletMutationOutcome<(StoredAdminPaymentOrder, bool)>, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let sql = format!(
                        r#"
SELECT
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
FROM payment_orders
WHERE id = $1
FOR UPDATE
                        "#,
                    );
                    let Some(order_row) = sqlx::query(&sql)
                        .bind(&input.order_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let order = map_admin_payment_order_row(&order_row)?;
                    if order.status == "credited" {
                        return Ok(WalletMutationOutcome::Applied((order, false)));
                    }
                    if matches!(
                        order.status.as_str(),
                        "failed" | "expired" | "refunded" | "cancelled"
                    ) {
                        return Ok(WalletMutationOutcome::Invalid(format!(
                            "payment order is not creditable: {}",
                            order.status
                        )));
                    }
                    if order
                        .expires_at_unix_secs
                        .is_some_and(|value| value < Utc::now().timestamp().max(0) as u64)
                    {
                        return Ok(WalletMutationOutcome::Invalid(
                            "payment order expired".to_string(),
                        ));
                    }

                    let now = Utc::now();
                    let order_kind: String = row_get(&order_row, "order_kind")?;
                    if order_kind == "plan_purchase" {
                        let order_user_id: Option<String> = row_get(&order_row, "user_id")?;
                        let Some(user_id) = order_user_id else {
                            return Ok(WalletMutationOutcome::Invalid(
                                "payment order user missing".to_string(),
                            ));
                        };
                        let product_id: Option<String> = row_get(&order_row, "product_id")?;
                        let product_snapshot: Option<serde_json::Value> =
                            row_get(&order_row, "product_snapshot")?;
                        let snapshot = product_snapshot.unwrap_or_else(|| serde_json::json!({}));
                        let plan_id = product_id.unwrap_or_else(|| {
                            snapshot
                                .get("id")
                                .and_then(|value| value.as_str())
                                .unwrap_or("unknown")
                                .to_string()
                        });
                        let existing_entitlement_id = sqlx::query_scalar::<_, String>(
                            r#"
SELECT id
FROM user_plan_entitlements
WHERE payment_order_id = $1
LIMIT 1
                            "#,
                        )
                        .bind(&input.order_id)
                        .fetch_optional(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        if existing_entitlement_id.is_none() {
                            lock_plan_renewal_scope_postgres(tx, &order.wallet_id).await?;
                            let requested_starts_at = plan_starts_at(&snapshot, now);
                            let starts_at = plan_renewal_starts_at_postgres(
                                tx,
                                &user_id,
                                &plan_id,
                                requested_starts_at,
                            )
                            .await?;
                            let expires_at = plan_expires_at(&snapshot, starts_at);
                            if has_other_overlapping_plan_postgres(
                                tx, &user_id, &plan_id, starts_at, expires_at,
                            )
                            .await?
                            {
                                return Ok(WalletMutationOutcome::Invalid(
                                    "another plan overlaps this entitlement".to_string(),
                                ));
                            }
                            let entitlements = plan_entitlements_snapshot(&snapshot);
                            let purchase_limit_scope = plan_purchase_limit_scope(&snapshot);
                            if purchase_limit_scope != "unlimited" {
                                let max_active_per_user = plan_max_active_per_user(&snapshot);
                                let active_count = if purchase_limit_scope == "lifetime" {
                                    sqlx::query_scalar::<_, i64>(
                                        r#"
SELECT COUNT(*)::bigint
FROM user_plan_entitlements
WHERE user_id = $1
  AND plan_id = $2
  AND status = 'active'
                                "#,
                                    )
                                    .bind(&user_id)
                                    .bind(&plan_id)
                                    .fetch_one(&mut **tx)
                                    .await
                                    .map_postgres_err()?
                                } else {
                                    0
                                };
                                if active_count >= max_active_per_user {
                                    return Ok(WalletMutationOutcome::Invalid(
                                        "plan purchase limit reached".to_string(),
                                    ));
                                }
                            }
                            apply_plan_debt_repayment_postgres(
                                tx,
                                &order.wallet_id,
                                &input.order_id,
                                &order.payment_method,
                                order.debt_repayment_usd,
                            )
                            .await?;
                            let entitlement_id = Uuid::new_v4().to_string();
                            sqlx::query(
                                r#"
INSERT INTO user_plan_entitlements (
  id, user_id, plan_id, payment_order_id, status, starts_at, expires_at,
  entitlements_snapshot, created_at, updated_at
)
VALUES ($1, $2, $3, $4, 'active', $5, $6, $7, NOW(), NOW())
                                "#,
                            )
                            .bind(&entitlement_id)
                            .bind(&user_id)
                            .bind(&plan_id)
                            .bind(&input.order_id)
                            .bind(starts_at)
                            .bind(expires_at)
                            .bind(&entitlements)
                            .execute(&mut **tx)
                            .await
                            .map_postgres_err()?;
                            save_entitlement_providers_postgres(
                                tx,
                                &entitlement_id,
                                &plan_provider_ids_snapshot(&snapshot),
                            )
                            .await?;
                            apply_plan_wallet_credit_postgres(
                                tx,
                                &order.wallet_id,
                                &input.order_id,
                                &order.payment_method,
                                &entitlements,
                            )
                            .await?;
                        }

                        let mut gateway_response =
                            payment_gateway_response_map(order.gateway_response.clone());
                        if let Some(serde_json::Value::Object(map)) =
                            input.gateway_response_patch.clone()
                        {
                            gateway_response.extend(map);
                        }
                        gateway_response
                            .insert("manual_credit".to_string(), serde_json::Value::Bool(true));
                        gateway_response.insert(
                            "credited_by".to_string(),
                            input
                                .operator_id
                                .clone()
                                .map(serde_json::Value::String)
                                .unwrap_or(serde_json::Value::Null),
                        );
                        let next_gateway_order_id =
                            input.gateway_order_id.clone().or(order.gateway_order_id);
                        let next_pay_amount = input.pay_amount.or(order.pay_amount);
                        let next_pay_currency = input.pay_currency.clone().or(order.pay_currency);
                        let next_exchange_rate = input.exchange_rate.or(order.exchange_rate);
                        let next_paid_at_unix_secs = order
                            .paid_at_unix_secs
                            .or(Some(now.timestamp().max(0) as u64));

                        let sql = format!(
                            r#"
UPDATE payment_orders
SET
  gateway_order_id = $2,
  gateway_response = $3,
  pay_amount = $4,
  pay_currency = $5,
  exchange_rate = $6,
  status = 'credited',
  fulfillment_status = 'fulfilled',
  fulfillment_error = NULL,
  paid_at = COALESCE(to_timestamp($7), NOW()),
  credited_at = NOW(),
  refundable_amount_usd = 0
WHERE id = $1
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                        );
                        let row = sqlx::query(&sql)
                            .bind(&input.order_id)
                            .bind(next_gateway_order_id)
                            .bind(serde_json::Value::Object(gateway_response))
                            .bind(next_pay_amount)
                            .bind(next_pay_currency)
                            .bind(next_exchange_rate)
                            .bind(
                                i64::try_from(
                                    next_paid_at_unix_secs.unwrap_or(now.timestamp().max(0) as u64),
                                )
                                .unwrap_or_default(),
                            )
                            .fetch_one(&mut **tx)
                            .await
                            .map_postgres_err()?;
                        return Ok(WalletMutationOutcome::Applied((
                            map_admin_payment_order_row(&row)?,
                            true,
                        )));
                    }

                    let Some(wallet_row) = sqlx::query(
                        r#"
SELECT
  id,
  status,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance
FROM wallets
WHERE id = $1
FOR UPDATE
                        "#,
                    )
                    .bind(&order.wallet_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::Invalid(
                            "wallet not found".to_string(),
                        ));
                    };
                    let wallet_status: String = row_get(&wallet_row, "status")?;
                    if wallet_status != "active" {
                        return Ok(WalletMutationOutcome::Invalid(
                            "wallet is not active".to_string(),
                        ));
                    }

                    let before_recharge: f64 = row_get(&wallet_row, "balance")?;
                    let before_gift: f64 = row_get(&wallet_row, "gift_balance")?;
                    let before_total = before_recharge + before_gift;
                    let after_recharge = before_recharge + order.amount_usd;
                    let now_unix_secs = Utc::now().timestamp().max(0) as u64;
                    sqlx::query(
                        r#"
UPDATE wallets
SET
  balance = $2,
  total_recharged = total_recharged + $3,
  updated_at = NOW()
WHERE id = $1
                        "#,
                    )
                    .bind(&order.wallet_id)
                    .bind(after_recharge)
                    .bind(order.amount_usd)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    sqlx::query(
                        r#"
INSERT INTO wallet_transactions (
  id,
  wallet_id,
  category,
  reason_code,
  amount,
  balance_before,
  balance_after,
  recharge_balance_before,
  recharge_balance_after,
  gift_balance_before,
  gift_balance_after,
  link_type,
  link_id,
  operator_id,
  description,
  created_at
)
VALUES (
  $1,
  $2,
  'recharge',
  'topup_gateway',
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $8,
  'payment_order',
  $9,
  NULL,
  $10,
  NOW()
)
                        "#,
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&order.wallet_id)
                    .bind(order.amount_usd)
                    .bind(before_total)
                    .bind(after_recharge + before_gift)
                    .bind(before_recharge)
                    .bind(after_recharge)
                    .bind(before_gift)
                    .bind(&input.order_id)
                    .bind(format!("充值到账({})", order.payment_method))
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    let mut gateway_response =
                        payment_gateway_response_map(order.gateway_response.clone());
                    if let Some(serde_json::Value::Object(map)) = input.gateway_response_patch {
                        gateway_response.extend(map);
                    }
                    gateway_response
                        .insert("manual_credit".to_string(), serde_json::Value::Bool(true));
                    gateway_response.insert(
                        "credited_by".to_string(),
                        input
                            .operator_id
                            .clone()
                            .map(serde_json::Value::String)
                            .unwrap_or(serde_json::Value::Null),
                    );
                    let next_gateway_order_id = input
                        .gateway_order_id
                        .clone()
                        .or(order.gateway_order_id.clone());
                    let next_pay_amount = input.pay_amount.or(order.pay_amount);
                    let next_pay_currency =
                        input.pay_currency.clone().or(order.pay_currency.clone());
                    let next_exchange_rate = input.exchange_rate.or(order.exchange_rate);
                    let next_paid_at_unix_secs = order.paid_at_unix_secs.or(Some(now_unix_secs));

                    let sql = format!(
                        r#"
UPDATE payment_orders
SET
  gateway_order_id = $2,
  gateway_response = $3,
  pay_amount = $4,
  pay_currency = $5,
  exchange_rate = $6,
  status = 'credited',
  paid_at = COALESCE(to_timestamp($7), NOW()),
  credited_at = NOW(),
  refundable_amount_usd = amount_usd
WHERE id = $1
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let row = sqlx::query(&sql)
                        .bind(&input.order_id)
                        .bind(next_gateway_order_id)
                        .bind(serde_json::Value::Object(gateway_response))
                        .bind(next_pay_amount)
                        .bind(next_pay_currency)
                        .bind(next_exchange_rate)
                        .bind(
                            i64::try_from(next_paid_at_unix_secs.unwrap_or(now_unix_secs))
                                .unwrap_or_default(),
                        )
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied((
                        map_admin_payment_order_row(&row)?,
                        true,
                    )))
                })
            })
            .await
    }

    async fn create_admin_redeem_code_batch(
        &self,
        input: CreateAdminRedeemCodeBatchInput,
    ) -> Result<CreateAdminRedeemCodeBatchResult, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let batch_id = Uuid::new_v4().to_string();
                    let expires_at = input
                        .expires_at_unix_secs
                        .map(|value| {
                            i64::try_from(value).map_err(|_| {
                                DataLayerError::InvalidInput(
                                    "redeem code batch expires_at overflow".to_string(),
                                )
                            })
                        })
                        .transpose()?;

                    sqlx::query(
                        r#"
INSERT INTO redeem_code_batches (
  id,
  name,
  amount_usd,
  currency,
  balance_bucket,
  total_count,
  status,
  description,
  created_by,
  expires_at,
  created_at,
  updated_at
)
VALUES (
  $1,
  $2,
  $3,
  $4,
  $5,
  $6,
  'active',
  $7,
  $8,
  CASE
    WHEN $9 IS NULL THEN NULL
    ELSE to_timestamp($9)
  END,
  NOW(),
  NOW()
)
                        "#,
                    )
                    .bind(&batch_id)
                    .bind(&input.name)
                    .bind(input.amount_usd)
                    .bind(&input.currency)
                    .bind(&input.balance_bucket)
                    .bind(i32::try_from(input.total_count).map_err(|_| {
                        DataLayerError::InvalidInput(
                            "redeem code batch total_count overflow".to_string(),
                        )
                    })?)
                    .bind(input.description.as_deref())
                    .bind(input.created_by.as_deref())
                    .bind(expires_at)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    let mut plaintext_codes = Vec::with_capacity(input.total_count);
                    for _ in 0..input.total_count {
                        let (normalized_code, display_code) = loop {
                            let normalized = generate_redeem_code_normalized();
                            let code_hash = hash_redeem_code(&normalized);
                            let insert_result = sqlx::query(
                                r#"
INSERT INTO redeem_codes (
  id,
  batch_id,
  code_hash,
  code_prefix,
  code_suffix,
  status,
  created_at,
  updated_at
)
VALUES (
  $1,
  $2,
  $3,
  $4,
  $5,
  'active',
  NOW(),
  NOW()
)
                                "#,
                            )
                            .bind(Uuid::new_v4().to_string())
                            .bind(&batch_id)
                            .bind(&code_hash)
                            .bind(redeem_code_prefix(&normalized))
                            .bind(redeem_code_suffix(&normalized))
                            .execute(&mut **tx)
                            .await;
                            match insert_result {
                                Ok(_) => {
                                    break (normalized.clone(), format_redeem_code(&normalized))
                                }
                                Err(sqlx::Error::Database(err))
                                    if err.code().as_deref() == Some("23505") =>
                                {
                                    continue;
                                }
                                Err(err) => return Err(DataLayerError::postgres(err)),
                            }
                        };
                        let code_hash = hash_redeem_code(&normalized_code);
                        let row = sqlx::query(
                            r#"
SELECT
  codes.id,
  codes.batch_id,
  batches.name AS batch_name,
  codes.code_prefix,
  codes.code_suffix,
  codes.status,
  codes.redeemed_by_user_id,
  NULL::TEXT AS redeemed_by_user_name,
  codes.redeemed_wallet_id,
  codes.redeemed_payment_order_id,
  NULL::TEXT AS redeemed_order_no,
  CAST(EXTRACT(EPOCH FROM codes.redeemed_at) AS BIGINT) AS redeemed_at_unix_secs,
  codes.disabled_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM codes.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM codes.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_codes AS codes
JOIN redeem_code_batches AS batches
  ON batches.id = codes.batch_id
WHERE codes.code_hash = $1
LIMIT 1
                            "#,
                        )
                        .bind(&code_hash)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        let code = map_admin_redeem_code_row(&row)?;
                        plaintext_codes.push(CreatedAdminRedeemCodePlaintext {
                            code_id: code.id,
                            code: display_code,
                            masked_code: code.masked_code,
                        });
                    }

                    let batch_row = sqlx::query(
                        r#"
SELECT
  batches.id,
  batches.name,
  CAST(batches.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  batches.currency,
  batches.balance_bucket,
  CAST(batches.total_count AS BIGINT) AS total_count,
  CAST(COALESCE(stats.redeemed_count, 0) AS BIGINT) AS redeemed_count,
  CAST(COALESCE(stats.active_count, 0) AS BIGINT) AS active_count,
  batches.status,
  batches.description,
  batches.created_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM batches.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM batches.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_code_batches AS batches
LEFT JOIN (
  SELECT
    batch_id,
    COUNT(*) FILTER (WHERE status = 'redeemed') AS redeemed_count,
    COUNT(*) FILTER (WHERE status = 'active') AS active_count
  FROM redeem_codes
  GROUP BY batch_id
) AS stats
  ON stats.batch_id = batches.id
WHERE batches.id = $1
LIMIT 1
                        "#,
                    )
                    .bind(&batch_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    Ok(CreateAdminRedeemCodeBatchResult {
                        batch: map_admin_redeem_code_batch_row(&batch_row)?,
                        codes: plaintext_codes,
                    })
                })
            })
            .await
    }

    async fn disable_admin_redeem_code_batch(
        &self,
        input: DisableAdminRedeemCodeBatchInput,
    ) -> Result<WalletMutationOutcome<StoredAdminRedeemCodeBatch>, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(current_batch) = sqlx::query(
                        r#"
SELECT status
FROM redeem_code_batches
WHERE id = $1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.batch_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let status: String = row_get(&current_batch, "status")?;
                    if status == "disabled" {
                        let row = sqlx::query(
                            r#"
SELECT
  batches.id,
  batches.name,
  CAST(batches.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  batches.currency,
  batches.balance_bucket,
  CAST(batches.total_count AS BIGINT) AS total_count,
  CAST(COALESCE(stats.redeemed_count, 0) AS BIGINT) AS redeemed_count,
  CAST(COALESCE(stats.active_count, 0) AS BIGINT) AS active_count,
  batches.status,
  batches.description,
  batches.created_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM batches.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM batches.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_code_batches AS batches
LEFT JOIN (
  SELECT
    batch_id,
    COUNT(*) FILTER (WHERE status = 'redeemed') AS redeemed_count,
    COUNT(*) FILTER (WHERE status = 'active') AS active_count
  FROM redeem_codes
  GROUP BY batch_id
) AS stats
  ON stats.batch_id = batches.id
WHERE batches.id = $1
LIMIT 1
                            "#,
                        )
                        .bind(&input.batch_id)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;
                        return Ok(WalletMutationOutcome::Applied(
                            map_admin_redeem_code_batch_row(&row)?,
                        ));
                    }

                    sqlx::query(
                        r#"
UPDATE redeem_code_batches
SET
  status = 'disabled',
  updated_at = NOW()
WHERE id = $1
                        "#,
                    )
                    .bind(&input.batch_id)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    sqlx::query(
                        r#"
UPDATE redeem_codes
SET
  status = 'disabled',
  disabled_by = COALESCE($2, disabled_by),
  updated_at = NOW()
WHERE batch_id = $1
  AND status = 'active'
                        "#,
                    )
                    .bind(&input.batch_id)
                    .bind(input.operator_id.as_deref())
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    let row = sqlx::query(
                        r#"
SELECT
  batches.id,
  batches.name,
  CAST(batches.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  batches.currency,
  batches.balance_bucket,
  CAST(batches.total_count AS BIGINT) AS total_count,
  CAST(COALESCE(stats.redeemed_count, 0) AS BIGINT) AS redeemed_count,
  CAST(COALESCE(stats.active_count, 0) AS BIGINT) AS active_count,
  batches.status,
  batches.description,
  batches.created_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM batches.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM batches.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_code_batches AS batches
LEFT JOIN (
  SELECT
    batch_id,
    COUNT(*) FILTER (WHERE status = 'redeemed') AS redeemed_count,
    COUNT(*) FILTER (WHERE status = 'active') AS active_count
  FROM redeem_codes
  GROUP BY batch_id
) AS stats
  ON stats.batch_id = batches.id
WHERE batches.id = $1
LIMIT 1
                        "#,
                    )
                    .bind(&input.batch_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    Ok(WalletMutationOutcome::Applied(
                        map_admin_redeem_code_batch_row(&row)?,
                    ))
                })
            })
            .await
    }

    async fn delete_admin_redeem_code_batch(
        &self,
        input: DeleteAdminRedeemCodeBatchInput,
    ) -> Result<WalletMutationOutcome<StoredAdminRedeemCodeBatch>, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(current_batch) = sqlx::query(
                        r#"
SELECT status
FROM redeem_code_batches
WHERE id = $1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.batch_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let status: String = row_get(&current_batch, "status")?;
                    if status != "disabled" {
                        return Ok(WalletMutationOutcome::Invalid(
                            "only disabled redeem code batch can be deleted".to_string(),
                        ));
                    }

                    let redeemed_count_row = sqlx::query(
                        r#"
SELECT COUNT(*) AS total
FROM redeem_codes
WHERE batch_id = $1
  AND status = 'redeemed'
                        "#,
                    )
                    .bind(&input.batch_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    let redeemed_count = read_count(redeemed_count_row)?;
                    if redeemed_count > 0 {
                        return Ok(WalletMutationOutcome::Invalid(
                            "redeemed batch cannot be deleted".to_string(),
                        ));
                    }

                    let row = sqlx::query(
                        r#"
SELECT
  batches.id,
  batches.name,
  CAST(batches.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  batches.currency,
  batches.balance_bucket,
  CAST(batches.total_count AS BIGINT) AS total_count,
  CAST(COALESCE(stats.redeemed_count, 0) AS BIGINT) AS redeemed_count,
  CAST(COALESCE(stats.active_count, 0) AS BIGINT) AS active_count,
  batches.status,
  batches.description,
  batches.created_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM batches.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM batches.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_code_batches AS batches
LEFT JOIN (
  SELECT
    batch_id,
    COUNT(*) FILTER (WHERE status = 'redeemed') AS redeemed_count,
    COUNT(*) FILTER (WHERE status = 'active') AS active_count
  FROM redeem_codes
  GROUP BY batch_id
) AS stats
  ON stats.batch_id = batches.id
WHERE batches.id = $1
LIMIT 1
                        "#,
                    )
                    .bind(&input.batch_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    let batch = map_admin_redeem_code_batch_row(&row)?;
                    let _ = input.operator_id;

                    sqlx::query(
                        r#"
DELETE FROM redeem_code_batches
WHERE id = $1
                        "#,
                    )
                    .bind(&input.batch_id)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    Ok(WalletMutationOutcome::Applied(batch))
                })
            })
            .await
    }

    async fn disable_admin_redeem_code(
        &self,
        input: DisableAdminRedeemCodeInput,
    ) -> Result<WalletMutationOutcome<StoredAdminRedeemCode>, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let Some(current_code) = sqlx::query(
                        r#"
SELECT batch_id, status
FROM redeem_codes
WHERE id = $1
FOR UPDATE
                        "#,
                    )
                    .bind(&input.code_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(WalletMutationOutcome::NotFound);
                    };
                    let batch_id: String = row_get(&current_code, "batch_id")?;
                    let status: String = row_get(&current_code, "status")?;
                    if status == "redeemed" {
                        return Ok(WalletMutationOutcome::Invalid(
                            "redeemed code cannot be disabled".to_string(),
                        ));
                    }
                    if status != "disabled" {
                        sqlx::query(
                            r#"
UPDATE redeem_codes
SET
  status = 'disabled',
  disabled_by = COALESCE($2, disabled_by),
  updated_at = NOW()
WHERE id = $1
                            "#,
                        )
                        .bind(&input.code_id)
                        .bind(input.operator_id.as_deref())
                        .execute(&mut **tx)
                        .await
                        .map_postgres_err()?;
                    }

                    let row = sqlx::query(
                        r#"
SELECT
  codes.id,
  codes.batch_id,
  batches.name AS batch_name,
  codes.code_prefix,
  codes.code_suffix,
  codes.status,
  codes.redeemed_by_user_id,
  users.username AS redeemed_by_user_name,
  codes.redeemed_wallet_id,
  codes.redeemed_payment_order_id,
  orders.order_no AS redeemed_order_no,
  CAST(EXTRACT(EPOCH FROM codes.redeemed_at) AS BIGINT) AS redeemed_at_unix_secs,
  codes.disabled_by,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM codes.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM codes.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM redeem_codes AS codes
JOIN redeem_code_batches AS batches
  ON batches.id = codes.batch_id
LEFT JOIN users
  ON users.id = codes.redeemed_by_user_id
LEFT JOIN payment_orders AS orders
  ON orders.id = codes.redeemed_payment_order_id
WHERE codes.id = $1
LIMIT 1
                        "#,
                    )
                    .bind(&input.code_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    sqlx::query(
                        r#"
UPDATE redeem_code_batches
SET
  updated_at = NOW()
WHERE id = $1
                        "#,
                    )
                    .bind(&batch_id)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    Ok(WalletMutationOutcome::Applied(map_admin_redeem_code_row(
                        &row,
                    )?))
                })
            })
            .await
    }

    async fn redeem_wallet_code(
        &self,
        input: RedeemWalletCodeInput,
    ) -> Result<RedeemWalletCodeOutcome, DataLayerError> {
        self.tx_runner
            .run_read_write(|tx| {
                Box::pin(async move {
                    let normalized_code = match normalize_redeem_code(&input.code) {
                        Some(value) => value,
                        None => return Ok(RedeemWalletCodeOutcome::InvalidCode),
                    };
                    let code_hash = hash_redeem_code(&normalized_code);
                    let now = Utc::now();
                    let now_unix_secs = now.timestamp().max(0) as u64;

                    let Some(code_row) = sqlx::query(
                        r#"
SELECT
  codes.id,
  codes.batch_id,
  codes.status,
  batches.name AS batch_name,
  batches.status AS batch_status,
  batches.balance_bucket,
  CAST(batches.amount_usd AS DOUBLE PRECISION) AS amount_usd,
  CAST(EXTRACT(EPOCH FROM batches.expires_at) AS BIGINT) AS batch_expires_at_unix_secs,
  CAST(EXTRACT(EPOCH FROM codes.redeemed_at) AS BIGINT) AS redeemed_at_unix_secs
FROM redeem_codes AS codes
JOIN redeem_code_batches AS batches
  ON batches.id = codes.batch_id
WHERE codes.code_hash = $1
LIMIT 1
FOR UPDATE OF codes, batches
                        "#,
                    )
                    .bind(&code_hash)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    else {
                        return Ok(RedeemWalletCodeOutcome::CodeNotFound);
                    };
                    let code_id: String = row_get(&code_row, "id")?;
                    let batch_id: String = row_get(&code_row, "batch_id")?;
                    let batch_name: String = row_get(&code_row, "batch_name")?;
                    let code_status: String = row_get(&code_row, "status")?;
                    let batch_status: String = row_get(&code_row, "batch_status")?;
                    let balance_bucket: String = row_get(&code_row, "balance_bucket")?;
                    let amount_usd: f64 = row_get(&code_row, "amount_usd")?;
                    let batch_expires_at_unix_secs = parse_optional_timestamp(
                        row_get(&code_row, "batch_expires_at_unix_secs")?,
                        "redeem_code_batches.expires_at",
                    )?;
                    let credits_recharge_balance =
                        redeem_code_credits_recharge_balance(&balance_bucket);

                    if code_status == "disabled" {
                        return Ok(RedeemWalletCodeOutcome::CodeDisabled);
                    }
                    if code_status == "redeemed" {
                        return Ok(RedeemWalletCodeOutcome::CodeRedeemed);
                    }
                    if batch_status != "active" {
                        return Ok(RedeemWalletCodeOutcome::BatchDisabled);
                    }
                    if batch_expires_at_unix_secs.is_some_and(|value| value <= now_unix_secs) {
                        return Ok(RedeemWalletCodeOutcome::CodeExpired);
                    }

                    let wallet_row = match sqlx::query(
                        r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
WHERE user_id = $1
FOR UPDATE
LIMIT 1
                        "#,
                    )
                    .bind(&input.user_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_postgres_err()?
                    {
                        Some(row) => row,
                        None => {
                            let wallet_id = Uuid::new_v4().to_string();
                            sqlx::query(
                                r#"
INSERT INTO wallets (
  id,
  user_id,
  balance,
  gift_balance,
  limit_mode,
  currency,
  status,
  total_recharged,
  total_consumed,
  total_refunded,
  total_adjusted,
  created_at,
  updated_at
)
VALUES (
  $1,
  $2,
  0,
  0,
  'finite',
  'USD',
  'active',
  0,
  0,
  0,
  0,
  NOW(),
  NOW()
)
ON CONFLICT (user_id) DO UPDATE
SET updated_at = wallets.updated_at
RETURNING
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
                                "#,
                            )
                            .bind(&wallet_id)
                            .bind(&input.user_id)
                            .fetch_one(&mut **tx)
                            .await
                            .map_postgres_err()?
                        }
                    };
                    let wallet_snapshot = map_wallet_row(&wallet_row)?;
                    if wallet_snapshot.status != "active" {
                        return Ok(RedeemWalletCodeOutcome::WalletInactive);
                    }

                    let before_recharge = wallet_snapshot.balance;
                    let before_gift = wallet_snapshot.gift_balance;
                    let before_total = before_recharge + before_gift;
                    let after_recharge = if credits_recharge_balance {
                        before_recharge + amount_usd
                    } else {
                        before_recharge
                    };
                    let after_gift = if credits_recharge_balance {
                        before_gift
                    } else {
                        before_gift + amount_usd
                    };

                    let wallet_row = sqlx::query(
                        r#"
UPDATE wallets
SET
  balance = $2,
  gift_balance = $3,
  total_recharged = total_recharged + $4,
  updated_at = NOW()
WHERE id = $1
RETURNING
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
  limit_mode,
  currency,
  status,
  CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
  CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
                        "#,
                    )
                    .bind(&wallet_snapshot.id)
                    .bind(after_recharge)
                    .bind(after_gift)
                    .bind(amount_usd)
                    .fetch_one(&mut **tx)
                    .await
                    .map_postgres_err()?;
                    let wallet = map_wallet_row(&wallet_row)?;

                    let order_id = Uuid::new_v4().to_string();
                    let gateway_order_id = format!("card_{}", Uuid::new_v4().simple());
                    let payment_method = redeem_code_payment_method(&balance_bucket);
                    let refundable_amount_usd =
                        redeem_code_refundable_amount(&balance_bucket, amount_usd);
                    let gateway_response = serde_json::json!({
                        "source": "redeem_code",
                        "batch_id": batch_id,
                        "batch_name": batch_name,
                        "balance_bucket": balance_bucket,
                    });
                    let sql = format!(
                        r#"
INSERT INTO payment_orders (
  id,
  order_no,
  wallet_id,
  user_id,
  amount_usd,
  refunded_amount_usd,
  refundable_amount_usd,
  payment_method,
  gateway_order_id,
  gateway_response,
  status,
  created_at,
  paid_at,
  credited_at
)
VALUES (
  $1,
  $2,
  $3,
  $4,
  $5,
  0,
  $6,
  $7,
  $8,
  $9,
  'credited',
  NOW(),
  NOW(),
  NOW()
)
RETURNING
{ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS}
                        "#,
                    );
                    let order_row = sqlx::query(&sql)
                        .bind(&order_id)
                        .bind(&input.order_no)
                        .bind(&wallet.id)
                        .bind(&input.user_id)
                        .bind(amount_usd)
                        .bind(refundable_amount_usd)
                        .bind(payment_method)
                        .bind(&gateway_order_id)
                        .bind(&gateway_response)
                        .fetch_one(&mut **tx)
                        .await
                        .map_postgres_err()?;

                    sqlx::query(
                        r#"
INSERT INTO wallet_transactions (
  id,
  wallet_id,
  category,
  reason_code,
  amount,
  balance_before,
  balance_after,
  recharge_balance_before,
  recharge_balance_after,
  gift_balance_before,
  gift_balance_after,
  link_type,
  link_id,
  operator_id,
  description,
  created_at
)
VALUES (
  $1,
  $2,
  'recharge',
  'topup_card_code',
  $3,
  $4,
  $5,
  $6,
  $7,
  $8,
  $9,
  'payment_order',
  $10,
  NULL,
  '兑换码充值',
  NOW()
)
                        "#,
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&wallet.id)
                    .bind(amount_usd)
                    .bind(before_total)
                    .bind(after_recharge + after_gift)
                    .bind(before_recharge)
                    .bind(after_recharge)
                    .bind(before_gift)
                    .bind(after_gift)
                    .bind(&order_id)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    sqlx::query(
                        r#"
UPDATE redeem_codes
SET
  status = 'redeemed',
  redeemed_by_user_id = $2,
  redeemed_wallet_id = $3,
  redeemed_payment_order_id = $4,
  redeemed_at = NOW(),
  updated_at = NOW()
WHERE id = $1
                        "#,
                    )
                    .bind(&code_id)
                    .bind(&input.user_id)
                    .bind(&wallet.id)
                    .bind(&order_id)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    sqlx::query(
                        r#"
UPDATE redeem_code_batches
SET
  updated_at = NOW()
WHERE id = $1
                        "#,
                    )
                    .bind(&batch_id)
                    .execute(&mut **tx)
                    .await
                    .map_postgres_err()?;

                    Ok(RedeemWalletCodeOutcome::Redeemed {
                        wallet,
                        order: map_admin_payment_order_row(&order_row)?,
                        amount_usd,
                        batch_name,
                    })
                })
            })
            .await
    }
}

fn as_i64(value: usize, field: &str) -> Result<i64, DataLayerError> {
    i64::try_from(value)
        .map_err(|_| DataLayerError::UnexpectedValue(format!("invalid {field}: {value}")))
}

fn default_refund_mode_for_payment_method(payment_method: &str) -> &'static str {
    if matches!(
        payment_method,
        "admin_manual" | "card_recharge" | "card_code" | "gift_code"
    ) {
        return "offline_payout";
    }
    "original_channel"
}

fn payment_gateway_response_map(
    value: Option<serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    match value {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

fn normalize_redeem_code(value: &str) -> Option<String> {
    let normalized = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect::<String>();
    if normalized.len() < 16 {
        None
    } else {
        Some(normalized)
    }
}

fn generate_redeem_code_normalized() -> String {
    Uuid::new_v4().simple().to_string().to_ascii_uppercase()
}

fn format_redeem_code(normalized: &str) -> String {
    normalized
        .as_bytes()
        .chunks(8)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("-")
}

fn hash_redeem_code(normalized: &str) -> String {
    use sha2::Digest;

    format!("{:x}", sha2::Sha256::digest(normalized.as_bytes()))
}

fn redeem_code_prefix(normalized: &str) -> String {
    normalized.chars().take(4).collect()
}

fn redeem_code_suffix(normalized: &str) -> String {
    normalized
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn mask_redeem_code(prefix: &str, suffix: &str) -> String {
    format!("{prefix}****{suffix}")
}

fn plan_entitlements_snapshot(snapshot: &serde_json::Value) -> serde_json::Value {
    let entitlements = snapshot
        .get("entitlements")
        .or_else(|| snapshot.get("entitlements_json"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    entitlements_with_admin_grant_overrides(snapshot, entitlements)
}

fn plan_max_active_per_user(snapshot: &serde_json::Value) -> i64 {
    snapshot
        .get("max_active_per_user")
        .and_then(|value| value.as_i64())
        .unwrap_or(1)
        .max(1)
}

fn plan_purchase_limit_scope(snapshot: &serde_json::Value) -> &str {
    match snapshot
        .get("purchase_limit_scope")
        .and_then(|value| value.as_str())
    {
        Some("lifetime") => "lifetime",
        Some("unlimited") => "unlimited",
        _ => "active_period",
    }
}

async fn lock_plan_renewal_scope_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    wallet_id: &str,
) -> Result<(), DataLayerError> {
    sqlx::query("SELECT id FROM wallets WHERE id = $1 LIMIT 1 FOR UPDATE")
        .bind(wallet_id)
        .fetch_optional(&mut **tx)
        .await
        .map_postgres_err()?;
    Ok(())
}

async fn has_other_overlapping_plan_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    user_id: &str,
    plan_id: &str,
    starts_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> Result<bool, DataLayerError> {
    sqlx::query_scalar::<_, bool>(
        r#"
SELECT EXISTS (
  SELECT 1
  FROM user_plan_entitlements
  WHERE user_id = $1
    AND plan_id <> $2
    AND status = 'active'
    AND starts_at < $4
    AND expires_at > $3
)
        "#,
    )
    .bind(user_id)
    .bind(plan_id)
    .bind(starts_at)
    .bind(expires_at)
    .fetch_one(&mut **tx)
    .await
    .map_postgres_err()
}

async fn has_pending_other_plan_purchase_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    user_id: &str,
    plan_id: &str,
) -> Result<bool, DataLayerError> {
    sqlx::query_scalar::<_, bool>(
        r#"
SELECT EXISTS (
  SELECT 1
  FROM payment_orders
  WHERE user_id = $1
    AND order_kind = 'plan_purchase'
    AND product_id <> $2
    AND status = 'pending'
    AND expires_at > NOW()
)
        "#,
    )
    .bind(user_id)
    .bind(plan_id)
    .fetch_one(&mut **tx)
    .await
    .map_postgres_err()
}

async fn save_entitlement_providers_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    entitlement_id: &str,
    provider_ids: &[String],
) -> Result<(), DataLayerError> {
    for provider_id in provider_ids {
        sqlx::query(
            r#"
INSERT INTO user_entitlement_providers (user_entitlement_id, provider_id)
VALUES ($1, $2)
ON CONFLICT (user_entitlement_id, provider_id) DO NOTHING
            "#,
        )
        .bind(entitlement_id)
        .bind(provider_id)
        .execute(&mut **tx)
        .await
        .map_postgres_err()?;
    }
    Ok(())
}

async fn apply_plan_debt_repayment_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    wallet_id: &str,
    order_id: &str,
    payment_method: &str,
    amount_usd: f64,
) -> Result<(), DataLayerError> {
    if payment_method == "admin_grant" || amount_usd <= 0.0 {
        return Ok(());
    }
    if !amount_usd.is_finite() {
        return Err(DataLayerError::UnexpectedValue(
            "plan debt repayment amount is invalid".to_string(),
        ));
    }
    let Some(row) = sqlx::query(
        r#"
SELECT
  status,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance
FROM wallets
WHERE id = $1
LIMIT 1
FOR UPDATE
        "#,
    )
    .bind(wallet_id)
    .fetch_optional(&mut **tx)
    .await
    .map_postgres_err()?
    else {
        return Err(DataLayerError::UnexpectedValue(
            "wallet not found for plan debt repayment".to_string(),
        ));
    };
    let status: String = row_get(&row, "status")?;
    if status != "active" {
        return Err(DataLayerError::UnexpectedValue(
            "wallet is not active for plan debt repayment".to_string(),
        ));
    }
    let before_recharge: f64 = row_get(&row, "balance")?;
    let before_gift: f64 = row_get(&row, "gift_balance")?;
    let before_total = before_recharge + before_gift;
    let after_recharge = before_recharge + amount_usd;
    let after_total = after_recharge + before_gift;
    sqlx::query(
        r#"
UPDATE wallets
SET balance = $2,
    total_recharged = total_recharged + $3,
    updated_at = NOW()
WHERE id = $1
        "#,
    )
    .bind(wallet_id)
    .bind(after_recharge)
    .bind(amount_usd)
    .execute(&mut **tx)
    .await
    .map_postgres_err()?;
    sqlx::query(
        r#"
INSERT INTO wallet_transactions (
  id, wallet_id, category, reason_code, amount, balance_before, balance_after,
  recharge_balance_before, recharge_balance_after, gift_balance_before,
  gift_balance_after, link_type, link_id, operator_id, description, created_at
)
VALUES ($1, $2, 'recharge', 'plan_debt_repayment', $3, $4, $5, $6, $7, $8, $8,
        'payment_order', $9, NULL, $10, NOW())
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(wallet_id)
    .bind(amount_usd)
    .bind(before_total)
    .bind(after_total)
    .bind(before_recharge)
    .bind(after_recharge)
    .bind(before_gift)
    .bind(order_id)
    .bind(format!("套餐订单结清欠款({payment_method})"))
    .execute(&mut **tx)
    .await
    .map_postgres_err()?;
    Ok(())
}

async fn plan_renewal_starts_at_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    user_id: &str,
    plan_id: &str,
    requested_starts_at: DateTime<Utc>,
) -> Result<DateTime<Utc>, DataLayerError> {
    let latest_expires_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
SELECT expires_at
FROM user_plan_entitlements
WHERE user_id = $1
  AND plan_id = $2
  AND status = 'active'
  AND expires_at > $3
ORDER BY expires_at DESC
LIMIT 1
FOR UPDATE
        "#,
    )
    .bind(user_id)
    .bind(plan_id)
    .bind(requested_starts_at)
    .fetch_optional(&mut **tx)
    .await
    .map_postgres_err()?;
    Ok(latest_expires_at
        .filter(|value| *value > requested_starts_at)
        .unwrap_or(requested_starts_at))
}

fn plan_starts_at(snapshot: &serde_json::Value, now: DateTime<Utc>) -> DateTime<Utc> {
    admin_grant_starts_at_unix_secs(snapshot)
        .and_then(|value| Utc.timestamp_opt(value, 0).single())
        .unwrap_or(now)
}

fn plan_expires_at(snapshot: &serde_json::Value, starts_at: DateTime<Utc>) -> DateTime<Utc> {
    if let Some(value) = admin_grant_expires_at_unix_secs(snapshot)
        .and_then(|value| Utc.timestamp_opt(value, 0).single())
        .filter(|value| *value > starts_at)
    {
        return value;
    }
    let duration_value = snapshot
        .get("duration_value")
        .and_then(|value| value.as_i64())
        .unwrap_or(1)
        .max(1);
    match snapshot
        .get("duration_unit")
        .and_then(|value| value.as_str())
        .unwrap_or("month")
    {
        "day" => starts_at + chrono::Duration::days(duration_value),
        "year" => starts_at + chrono::Duration::days(365 * duration_value),
        "custom" => starts_at + chrono::Duration::days(duration_value),
        _ => starts_at + chrono::Duration::days(30 * duration_value),
    }
}

async fn apply_plan_wallet_credit_postgres(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    wallet_id: &str,
    order_id: &str,
    payment_method: &str,
    entitlements: &serde_json::Value,
) -> Result<(), DataLayerError> {
    let credits = entitlements
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(|value| value.as_str()) == Some("wallet_credit"))
        .filter_map(|item| {
            let amount = item.get("amount_usd").and_then(|value| value.as_f64())?;
            if amount <= 0.0 || !amount.is_finite() {
                return None;
            }
            let bucket = item
                .get("balance_bucket")
                .and_then(|value| value.as_str())
                .unwrap_or("gift")
                .to_ascii_lowercase();
            Some((amount, bucket))
        })
        .collect::<Vec<_>>();
    if credits.is_empty() {
        return Ok(());
    }

    let Some(wallet_row) = sqlx::query(
        r#"
SELECT
  id,
  status,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance
FROM wallets
WHERE id = $1
LIMIT 1
FOR UPDATE
        "#,
    )
    .bind(wallet_id)
    .fetch_optional(&mut **tx)
    .await
    .map_postgres_err()?
    else {
        return Err(DataLayerError::UnexpectedValue(
            "wallet not found for plan wallet_credit".to_string(),
        ));
    };
    let wallet_status: String = row_get(&wallet_row, "status")?;
    if wallet_status != "active" {
        return Err(DataLayerError::UnexpectedValue(
            "wallet is not active for plan wallet_credit".to_string(),
        ));
    }
    let mut recharge_balance: f64 = row_get(&wallet_row, "balance")?;
    let mut gift_balance: f64 = row_get(&wallet_row, "gift_balance")?;
    for (amount, bucket) in credits {
        let before_recharge = recharge_balance;
        let before_gift = gift_balance;
        let before_total = before_recharge + before_gift;
        let credits_recharge = bucket == "recharge";
        if credits_recharge {
            recharge_balance += amount;
        } else {
            gift_balance += amount;
        }
        let after_total = recharge_balance + gift_balance;
        sqlx::query(
            r#"
UPDATE wallets
SET balance = $2,
    gift_balance = $3,
    total_recharged = total_recharged + $4,
    updated_at = NOW()
WHERE id = $1
            "#,
        )
        .bind(wallet_id)
        .bind(recharge_balance)
        .bind(gift_balance)
        .bind(if credits_recharge { amount } else { 0.0 })
        .execute(&mut **tx)
        .await
        .map_postgres_err()?;
        sqlx::query(
            r#"
INSERT INTO wallet_transactions (
  id, wallet_id, category, reason_code, amount, balance_before, balance_after,
  recharge_balance_before, recharge_balance_after, gift_balance_before,
  gift_balance_after, link_type, link_id, operator_id, description, created_at
)
VALUES (
  $1, $2, 'recharge', 'plan_wallet_credit', $3, $4, $5, $6, $7, $8, $9,
  'payment_order', $10, NULL, $11, NOW()
)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(wallet_id)
        .bind(amount)
        .bind(before_total)
        .bind(after_total)
        .bind(before_recharge)
        .bind(recharge_balance)
        .bind(before_gift)
        .bind(gift_balance)
        .bind(order_id)
        .bind(format!("套餐附赠余额({payment_method})"))
        .execute(&mut **tx)
        .await
        .map_postgres_err()?;
    }
    Ok(())
}

async fn update_payment_callback_failure(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    callback_id: &str,
    input: &ProcessPaymentCallbackInput,
    error: &str,
) -> Result<(), DataLayerError> {
    sqlx::query(
        r#"
UPDATE payment_callbacks
SET signature_valid = $2,
    status = 'failed',
    error_message = $3,
    payload_hash = $4,
    payload = $5,
    processed_at = NOW(),
    order_no = COALESCE($6, order_no),
    gateway_order_id = COALESCE($7, gateway_order_id)
WHERE id = $1
        "#,
    )
    .bind(callback_id)
    .bind(input.signature_valid)
    .bind(error)
    .bind(&input.payload_hash)
    .bind(&input.payload)
    .bind(input.order_no.as_deref())
    .bind(input.gateway_order_id.as_deref())
    .execute(&mut **tx)
    .await
    .map_postgres_err()?;
    Ok(())
}

async fn mark_payment_callback_processed(
    tx: &mut crate::driver::postgres::PostgresTransaction,
    callback_id: &str,
    input: &ProcessPaymentCallbackInput,
    order_id: &str,
    order_no: &str,
) -> Result<(), DataLayerError> {
    sqlx::query(
        r#"
UPDATE payment_callbacks
SET payment_order_id = $2,
    signature_valid = true,
    status = 'processed',
    error_message = NULL,
    payload_hash = $3,
    payload = $4,
    processed_at = NOW(),
    order_no = $5,
    gateway_order_id = COALESCE($6, gateway_order_id)
WHERE id = $1
        "#,
    )
    .bind(callback_id)
    .bind(order_id)
    .bind(&input.payload_hash)
    .bind(&input.payload)
    .bind(order_no)
    .bind(input.gateway_order_id.as_deref())
    .execute(&mut **tx)
    .await
    .map_postgres_err()?;
    Ok(())
}

fn parse_timestamp(value: i64, field: &str) -> Result<u64, DataLayerError> {
    u64::try_from(value)
        .map_err(|_| DataLayerError::UnexpectedValue(format!("{field} is negative: {value}")))
}

fn parse_optional_timestamp(
    value: Option<i64>,
    field: &str,
) -> Result<Option<u64>, DataLayerError> {
    value.map(|inner| parse_timestamp(inner, field)).transpose()
}

fn row_get<T>(row: &PgRow, column: &str) -> Result<T, DataLayerError>
where
    for<'r> T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column).map_postgres_err()
}

fn read_count(row: PgRow) -> Result<u64, DataLayerError> {
    let total = row.try_get::<i64, _>("total").map_postgres_err()?;
    Ok(total.max(0) as u64)
}

async fn collect_query_rows<T, S>(
    mut rows: S,
    map_row: fn(&PgRow) -> Result<T, DataLayerError>,
) -> Result<Vec<T>, DataLayerError>
where
    S: TryStream<Ok = PgRow, Error = sqlx::Error> + Unpin,
{
    let mut items = Vec::new();
    while let Some(row) = rows.try_next().await.map_postgres_err()? {
        items.push(map_row(&row)?);
    }
    Ok(items)
}

fn map_admin_wallet_list_item_row(
    row: &PgRow,
) -> Result<StoredAdminWalletListItem, DataLayerError> {
    Ok(StoredAdminWalletListItem {
        id: row_get(row, "id")?,
        user_id: row_get(row, "user_id")?,
        api_key_id: row_get(row, "api_key_id")?,
        balance: row_get(row, "balance")?,
        gift_balance: row_get(row, "gift_balance")?,
        limit_mode: row_get(row, "limit_mode")?,
        currency: row_get(row, "currency")?,
        status: row_get(row, "status")?,
        total_recharged: row_get(row, "total_recharged")?,
        total_consumed: row_get(row, "total_consumed")?,
        total_refunded: row_get(row, "total_refunded")?,
        total_adjusted: row_get(row, "total_adjusted")?,
        user_name: row_get(row, "user_name")?,
        api_key_name: row_get(row, "api_key_name")?,
        created_at_unix_ms: parse_optional_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "wallets.created_at",
        )?,
        updated_at_unix_secs: parse_optional_timestamp(
            row_get(row, "updated_at_unix_secs")?,
            "wallets.updated_at",
        )?,
    })
}

fn map_admin_wallet_ledger_item_row(
    row: &PgRow,
) -> Result<StoredAdminWalletLedgerItem, DataLayerError> {
    Ok(StoredAdminWalletLedgerItem {
        id: row_get(row, "id")?,
        wallet_id: row_get(row, "wallet_id")?,
        category: row_get(row, "category")?,
        reason_code: row_get(row, "reason_code")?,
        amount: row_get(row, "amount")?,
        balance_before: row_get(row, "balance_before")?,
        balance_after: row_get(row, "balance_after")?,
        recharge_balance_before: row_get(row, "recharge_balance_before")?,
        recharge_balance_after: row_get(row, "recharge_balance_after")?,
        gift_balance_before: row_get(row, "gift_balance_before")?,
        gift_balance_after: row_get(row, "gift_balance_after")?,
        link_type: row_get(row, "link_type")?,
        link_id: row_get(row, "link_id")?,
        operator_id: row_get(row, "operator_id")?,
        operator_name: row_get(row, "operator_name")?,
        operator_email: row_get(row, "operator_email")?,
        description: row_get(row, "description")?,
        wallet_user_id: row_get(row, "user_id")?,
        wallet_user_name: row_get(row, "wallet_user_name")?,
        wallet_api_key_id: row_get(row, "api_key_id")?,
        api_key_name: row_get(row, "api_key_name")?,
        wallet_status: row_get(row, "wallet_status")?,
        created_at_unix_ms: parse_optional_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "wallet_transactions.created_at",
        )?,
    })
}

fn map_admin_wallet_refund_request_item_row(
    row: &PgRow,
) -> Result<StoredAdminWalletRefundRequestItem, DataLayerError> {
    Ok(StoredAdminWalletRefundRequestItem {
        id: row_get(row, "id")?,
        refund_no: row_get(row, "refund_no")?,
        wallet_id: row_get(row, "wallet_id")?,
        user_id: row_get(row, "user_id")?,
        payment_order_id: row_get(row, "payment_order_id")?,
        source_type: row_get(row, "source_type")?,
        source_id: row_get(row, "source_id")?,
        refund_mode: row_get(row, "refund_mode")?,
        amount_usd: row_get(row, "amount_usd")?,
        status: row_get(row, "status")?,
        reason: row_get(row, "reason")?,
        failure_reason: row_get(row, "failure_reason")?,
        gateway_refund_id: row_get(row, "gateway_refund_id")?,
        payout_method: row_get(row, "payout_method")?,
        payout_reference: row_get(row, "payout_reference")?,
        payout_proof: row_get(row, "payout_proof")?,
        requested_by: row_get(row, "requested_by")?,
        approved_by: row_get(row, "approved_by")?,
        processed_by: row_get(row, "processed_by")?,
        wallet_user_id: row_get(row, "wallet_user_id")?,
        wallet_user_name: row_get(row, "wallet_user_name")?,
        wallet_api_key_id: row_get(row, "wallet_api_key_id")?,
        api_key_name: row_get(row, "api_key_name")?,
        wallet_status: row_get(row, "wallet_status")?,
        created_at_unix_ms: parse_optional_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "refund_requests.created_at",
        )?,
        updated_at_unix_secs: parse_optional_timestamp(
            row_get(row, "updated_at_unix_secs")?,
            "refund_requests.updated_at",
        )?,
        processed_at_unix_secs: parse_optional_timestamp(
            row_get(row, "processed_at_unix_secs")?,
            "refund_requests.processed_at",
        )?,
        completed_at_unix_secs: parse_optional_timestamp(
            row_get(row, "completed_at_unix_secs")?,
            "refund_requests.completed_at",
        )?,
    })
}

fn map_admin_wallet_transaction_row(
    row: &PgRow,
) -> Result<StoredAdminWalletTransaction, DataLayerError> {
    Ok(StoredAdminWalletTransaction {
        id: row_get(row, "id")?,
        wallet_id: row_get(row, "wallet_id")?,
        category: row_get(row, "category")?,
        reason_code: row_get(row, "reason_code")?,
        amount: row_get(row, "amount")?,
        balance_before: row_get(row, "balance_before")?,
        balance_after: row_get(row, "balance_after")?,
        recharge_balance_before: row_get(row, "recharge_balance_before")?,
        recharge_balance_after: row_get(row, "recharge_balance_after")?,
        gift_balance_before: row_get(row, "gift_balance_before")?,
        gift_balance_after: row_get(row, "gift_balance_after")?,
        link_type: row_get(row, "link_type")?,
        link_id: row_get(row, "link_id")?,
        operator_id: row_get(row, "operator_id")?,
        operator_name: row_get(row, "operator_name")?,
        operator_email: row_get(row, "operator_email")?,
        description: row_get(row, "description")?,
        created_at_unix_ms: parse_optional_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "wallet_transactions.created_at",
        )?,
    })
}

fn map_wallet_daily_usage_row(row: &PgRow) -> Result<StoredWalletDailyUsageLedger, DataLayerError> {
    Ok(StoredWalletDailyUsageLedger {
        id: row_get(row, "id")?,
        billing_date: row_get(row, "billing_date")?,
        billing_timezone: row_get(row, "billing_timezone")?,
        total_cost_usd: row_get(row, "total_cost_usd")?,
        total_requests: row_get::<i64>(row, "total_requests")?.max(0) as u64,
        input_tokens: row_get::<i64>(row, "input_tokens")?.max(0) as u64,
        output_tokens: row_get::<i64>(row, "output_tokens")?.max(0) as u64,
        cache_creation_tokens: row_get::<i64>(row, "cache_creation_tokens")?.max(0) as u64,
        cache_read_tokens: row_get::<i64>(row, "cache_read_tokens")?.max(0) as u64,
        first_finalized_at_unix_secs: parse_optional_timestamp(
            row_get(row, "first_finalized_at_unix_secs")?,
            "wallet_daily_usage_ledgers.first_finalized_at",
        )?,
        last_finalized_at_unix_secs: parse_optional_timestamp(
            row_get(row, "last_finalized_at_unix_secs")?,
            "wallet_daily_usage_ledgers.last_finalized_at",
        )?,
        aggregated_at_unix_secs: parse_optional_timestamp(
            row_get(row, "aggregated_at_unix_secs")?,
            "wallet_daily_usage_ledgers.aggregated_at",
        )?,
    })
}

fn map_admin_wallet_refund_row(row: &PgRow) -> Result<StoredAdminWalletRefund, DataLayerError> {
    Ok(StoredAdminWalletRefund {
        id: row_get(row, "id")?,
        refund_no: row_get(row, "refund_no")?,
        wallet_id: row_get(row, "wallet_id")?,
        user_id: row_get(row, "user_id")?,
        payment_order_id: row_get(row, "payment_order_id")?,
        source_type: row_get(row, "source_type")?,
        source_id: row_get(row, "source_id")?,
        refund_mode: row_get(row, "refund_mode")?,
        amount_usd: row_get(row, "amount_usd")?,
        status: row_get(row, "status")?,
        reason: row_get(row, "reason")?,
        failure_reason: row_get(row, "failure_reason")?,
        gateway_refund_id: row_get(row, "gateway_refund_id")?,
        payout_method: row_get(row, "payout_method")?,
        payout_reference: row_get(row, "payout_reference")?,
        payout_proof: row_get(row, "payout_proof")?,
        requested_by: row_get(row, "requested_by")?,
        approved_by: row_get(row, "approved_by")?,
        processed_by: row_get(row, "processed_by")?,
        created_at_unix_ms: parse_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "refund_requests.created_at",
        )?,
        updated_at_unix_secs: parse_timestamp(
            row_get(row, "updated_at_unix_secs")?,
            "refund_requests.updated_at",
        )?,
        processed_at_unix_secs: parse_optional_timestamp(
            row_get(row, "processed_at_unix_secs")?,
            "refund_requests.processed_at",
        )?,
        completed_at_unix_secs: parse_optional_timestamp(
            row_get(row, "completed_at_unix_secs")?,
            "refund_requests.completed_at",
        )?,
    })
}

fn map_admin_payment_callback_row(
    row: &PgRow,
) -> Result<StoredAdminPaymentCallback, DataLayerError> {
    Ok(StoredAdminPaymentCallback {
        id: row_get(row, "id")?,
        payment_order_id: row_get(row, "payment_order_id")?,
        payment_method: row_get(row, "payment_method")?,
        callback_key: row_get(row, "callback_key")?,
        order_no: row_get(row, "order_no")?,
        gateway_order_id: row_get(row, "gateway_order_id")?,
        payload_hash: row_get(row, "payload_hash")?,
        signature_valid: row_get(row, "signature_valid")?,
        status: row_get(row, "status")?,
        payload: row_get(row, "payload")?,
        error_message: row_get(row, "error_message")?,
        created_at_unix_ms: parse_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "payment_callbacks.created_at",
        )?,
        processed_at_unix_secs: parse_optional_timestamp(
            row_get(row, "processed_at_unix_secs")?,
            "payment_callbacks.processed_at",
        )?,
    })
}

fn map_admin_redeem_code_batch_row(
    row: &PgRow,
) -> Result<StoredAdminRedeemCodeBatch, DataLayerError> {
    Ok(StoredAdminRedeemCodeBatch {
        id: row_get(row, "id")?,
        name: row_get(row, "name")?,
        amount_usd: row_get(row, "amount_usd")?,
        currency: row_get(row, "currency")?,
        balance_bucket: row_get(row, "balance_bucket")?,
        total_count: row_get::<i64>(row, "total_count")?.max(0) as u64,
        redeemed_count: row_get::<i64>(row, "redeemed_count")?.max(0) as u64,
        active_count: row_get::<i64>(row, "active_count")?.max(0) as u64,
        status: row_get(row, "status")?,
        description: row_get(row, "description")?,
        created_by: row_get(row, "created_by")?,
        expires_at_unix_secs: parse_optional_timestamp(
            row_get(row, "expires_at_unix_secs")?,
            "redeem_code_batches.expires_at",
        )?,
        created_at_unix_ms: parse_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "redeem_code_batches.created_at",
        )?,
        updated_at_unix_secs: parse_timestamp(
            row_get(row, "updated_at_unix_secs")?,
            "redeem_code_batches.updated_at",
        )?,
    })
}

fn map_admin_redeem_code_row(row: &PgRow) -> Result<StoredAdminRedeemCode, DataLayerError> {
    let code_prefix: String = row_get(row, "code_prefix")?;
    let code_suffix: String = row_get(row, "code_suffix")?;
    Ok(StoredAdminRedeemCode {
        id: row_get(row, "id")?,
        batch_id: row_get(row, "batch_id")?,
        batch_name: row_get(row, "batch_name")?,
        code_prefix: code_prefix.clone(),
        code_suffix: code_suffix.clone(),
        masked_code: mask_redeem_code(&code_prefix, &code_suffix),
        status: row_get(row, "status")?,
        redeemed_by_user_id: row_get(row, "redeemed_by_user_id")?,
        redeemed_by_user_name: row_get(row, "redeemed_by_user_name")?,
        redeemed_wallet_id: row_get(row, "redeemed_wallet_id")?,
        redeemed_payment_order_id: row_get(row, "redeemed_payment_order_id")?,
        redeemed_order_no: row_get(row, "redeemed_order_no")?,
        redeemed_at_unix_secs: parse_optional_timestamp(
            row_get(row, "redeemed_at_unix_secs")?,
            "redeem_codes.redeemed_at",
        )?,
        disabled_by: row_get(row, "disabled_by")?,
        expires_at_unix_secs: parse_optional_timestamp(
            row_get(row, "expires_at_unix_secs")?,
            "redeem_code_batches.expires_at",
        )?,
        created_at_unix_ms: parse_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "redeem_codes.created_at",
        )?,
        updated_at_unix_secs: parse_timestamp(
            row_get(row, "updated_at_unix_secs")?,
            "redeem_codes.updated_at",
        )?,
    })
}

fn map_admin_payment_order_row(row: &PgRow) -> Result<StoredAdminPaymentOrder, DataLayerError> {
    Ok(StoredAdminPaymentOrder {
        id: row_get(row, "id")?,
        order_no: row_get(row, "order_no")?,
        wallet_id: row_get(row, "wallet_id")?,
        user_id: row_get(row, "user_id")?,
        owner_type: row.try_get("owner_type").ok().flatten(),
        owner_name: row.try_get("owner_name").ok().flatten(),
        amount_usd: row_get(row, "amount_usd")?,
        debt_repayment_usd: row_get(row, "debt_repayment_usd")?,
        pay_amount: row_get(row, "pay_amount")?,
        pay_currency: row_get(row, "pay_currency")?,
        exchange_rate: row_get(row, "exchange_rate")?,
        refunded_amount_usd: row_get(row, "refunded_amount_usd")?,
        refundable_amount_usd: row_get(row, "refundable_amount_usd")?,
        payment_method: row_get(row, "payment_method")?,
        payment_provider: row_get(row, "payment_provider")?,
        payment_channel: row_get(row, "payment_channel")?,
        order_kind: row_get(row, "order_kind")?,
        product_id: row_get(row, "product_id")?,
        product_snapshot: row_get(row, "product_snapshot")?,
        fulfillment_status: row_get(row, "fulfillment_status")?,
        fulfillment_error: row_get(row, "fulfillment_error")?,
        gateway_order_id: row_get(row, "gateway_order_id")?,
        gateway_response: row_get(row, "gateway_response")?,
        status: row_get(row, "status")?,
        created_at_unix_ms: parse_timestamp(
            row_get(row, "created_at_unix_ms")?,
            "payment_orders.created_at",
        )?,
        paid_at_unix_secs: parse_optional_timestamp(
            row_get(row, "paid_at_unix_secs")?,
            "payment_orders.paid_at",
        )?,
        credited_at_unix_secs: parse_optional_timestamp(
            row_get(row, "credited_at_unix_secs")?,
            "payment_orders.credited_at",
        )?,
        expires_at_unix_secs: parse_optional_timestamp(
            row_get(row, "expires_at_unix_secs")?,
            "payment_orders.expires_at",
        )?,
    })
}

fn map_wallet_row(row: &sqlx::postgres::PgRow) -> Result<StoredWalletSnapshot, DataLayerError> {
    StoredWalletSnapshot::new(
        row_get(row, "id")?,
        row_get(row, "user_id")?,
        row_get(row, "api_key_id")?,
        row_get(row, "balance")?,
        row_get(row, "gift_balance")?,
        row_get(row, "limit_mode")?,
        row_get(row, "currency")?,
        row_get(row, "status")?,
        row_get(row, "total_recharged")?,
        row_get(row, "total_consumed")?,
        row_get(row, "total_refunded")?,
        row_get(row, "total_adjusted")?,
        row_get(row, "updated_at_unix_secs")?,
    )
}

#[allow(clippy::too_many_arguments)]
async fn update_postgres_wallet_snapshot(
    pool: &PgPool,
    owner_column: &str,
    owner_id: &str,
    balance: f64,
    gift_balance: f64,
    limit_mode: &str,
    currency: &str,
    status: &str,
    total_recharged: f64,
    total_consumed: f64,
    total_refunded: f64,
    total_adjusted: f64,
    updated_at_unix_secs: Option<u64>,
) -> Result<(), DataLayerError> {
    let owner_predicate = match owner_column {
        "user_id" => "user_id = $1",
        "api_key_id" => "api_key_id = $1",
        _ => {
            return Err(DataLayerError::UnexpectedValue(format!(
                "unsupported wallet owner column: {owner_column}"
            )));
        }
    };
    let sql = format!(
        r#"
UPDATE wallets
SET balance = $2,
    gift_balance = $3,
    limit_mode = $4,
    currency = $5,
    status = $6,
    total_recharged = $7,
    total_consumed = $8,
    total_refunded = $9,
    total_adjusted = $10,
    updated_at = COALESCE(to_timestamp($11::DOUBLE PRECISION), NOW())
WHERE {owner_predicate}
"#
    );
    sqlx::query(&sql)
        .bind(owner_id)
        .bind(balance)
        .bind(gift_balance)
        .bind(limit_mode)
        .bind(currency)
        .bind(status)
        .bind(total_recharged)
        .bind(total_consumed)
        .bind(total_refunded)
        .bind(total_adjusted)
        .bind(updated_at_unix_secs.map(|value| value as i64))
        .execute(pool)
        .await
        .map_postgres_err()?;
    Ok(())
}

async fn initialize_postgres_auth_wallet(
    pool: &PgPool,
    user_id: Option<&str>,
    api_key_id: Option<&str>,
    initial_gift_usd: f64,
    unlimited: bool,
) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
    let gift_amount = if unlimited {
        0.0
    } else {
        initial_gift_usd.max(0.0)
    };
    let mut tx = pool.begin().await.map_postgres_err()?;
    let wallet_row = sqlx::query(
        r#"
INSERT INTO wallets (
    id, user_id, api_key_id, balance, gift_balance, limit_mode, currency,
    status, total_recharged, total_consumed, total_refunded, total_adjusted,
    created_at, updated_at
)
VALUES ($1, $2, $3, 0, $4, $5, 'USD', 'active', 0, 0, 0, $6, NOW(), NOW())
RETURNING
    id,
    user_id,
    api_key_id,
    CAST(balance AS DOUBLE PRECISION) AS balance,
    CAST(gift_balance AS DOUBLE PRECISION) AS gift_balance,
    limit_mode,
    currency,
    status,
    CAST(total_recharged AS DOUBLE PRECISION) AS total_recharged,
    CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
    CAST(total_refunded AS DOUBLE PRECISION) AS total_refunded,
    CAST(total_adjusted AS DOUBLE PRECISION) AS total_adjusted,
    CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
"#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(user_id)
    .bind(api_key_id)
    .bind(gift_amount)
    .bind(if unlimited { "unlimited" } else { "finite" })
    .bind(gift_amount)
    .fetch_one(&mut *tx)
    .await
    .map_postgres_err()?;
    let wallet = map_wallet_row(&wallet_row)?;
    if gift_amount > 0.0 {
        let link_id = user_id.or(api_key_id).unwrap_or_default();
        let description = if api_key_id.is_some() {
            "独立余额 Key 初始赠款"
        } else {
            "用户初始赠款"
        };
        sqlx::query(
            r#"
INSERT INTO wallet_transactions (
    id, wallet_id, category, reason_code, amount, balance_before,
    balance_after, recharge_balance_before, recharge_balance_after,
    gift_balance_before, gift_balance_after, link_type, link_id, operator_id,
    description, created_at
)
VALUES ($1, $2, 'gift', 'gift_initial', $3, 0, $3, 0, 0, 0, $3, 'system_task', $4, NULL, $5, NOW())
"#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&wallet.id)
        .bind(gift_amount)
        .bind(link_id)
        .bind(description)
        .execute(&mut *tx)
        .await
        .map_postgres_err()?;
    }
    tx.commit().await.map_err(postgres_error)?;
    Ok(Some(wallet))
}

#[cfg(test)]
mod tests {
    use super::{
        SqlxWalletRepository, ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS, FIND_ADMIN_PAYMENT_ORDER_SQL,
        FIND_WALLET_PAYMENT_ORDER_BY_USER_SQL, LIST_ADMIN_PAYMENT_ORDERS_SQL,
        LIST_WALLET_PAYMENT_ORDERS_BY_USER_SQL,
    };
    use crate::driver::postgres::{PostgresPoolConfig, PostgresPoolFactory};
    use crate::repository::wallet::{
        AdminPaymentOrderListQuery, CreatePlanPurchaseOrderInput, CreatePlanPurchaseOrderOutcome,
        CreateWalletRechargeOrderInput, CreditAdminPaymentOrderInput, ProcessPaymentCallbackInput,
        ProcessPaymentCallbackOutcome, WalletLookupKey, WalletMutationOutcome,
        WalletReadRepository, WalletWriteRepository,
    };
    use serde_json::json;

    #[tokio::test]
    async fn repository_constructs_from_lazy_pool() {
        let factory = PostgresPoolFactory::new(PostgresPoolConfig {
            database_url: "postgres://localhost/aether".to_string(),
            min_connections: 1,
            max_connections: 4,
            acquire_timeout_ms: 1_000,
            idle_timeout_ms: 5_000,
            max_lifetime_ms: 30_000,
            statement_cache_capacity: 64,
            require_ssl: false,
        })
        .expect("factory should build");

        let pool = factory.connect_lazy().expect("pool should build");
        let _repository = SqlxWalletRepository::new(pool);
    }

    #[test]
    fn admin_payment_order_mutations_use_shared_returning_columns() {
        let source = include_str!("postgres.rs");
        for function_name in [
            "create_wallet_recharge_order",
            "create_plan_purchase_order",
            "process_payment_callback",
            "create_manual_wallet_recharge",
            "expire_admin_payment_order",
            "fail_admin_payment_order",
            "credit_admin_payment_order",
            "redeem_wallet_code",
        ] {
            let marker = format!("async fn {function_name}");
            let function_source = source
                .split(&marker)
                .nth(1)
                .unwrap_or_else(|| panic!("{function_name} should exist"));
            let next_function = function_source
                .find("\n    async fn ")
                .unwrap_or(function_source.len());
            let function_source = &function_source[..next_function];

            assert!(
                function_source.contains("ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS"),
                "{function_name} should use the shared payment order projection"
            );
            assert!(
                !function_source.contains("RETURNING\n  id,\n  order_no"),
                "{function_name} must not hand-write an incomplete payment order projection"
            );
            assert!(
                !function_source.contains("SELECT\n  id,\n  order_no"),
                "{function_name} must not hand-write an incomplete payment order lookup"
            );
        }
    }

    #[test]
    fn admin_payment_order_returning_columns_cover_mapper_fields() {
        for column in [
            "id",
            "order_no",
            "wallet_id",
            "user_id",
            "amount_usd",
            "debt_repayment_usd",
            "pay_amount",
            "pay_currency",
            "exchange_rate",
            "refunded_amount_usd",
            "refundable_amount_usd",
            "payment_method",
            "payment_provider",
            "payment_channel",
            "order_kind",
            "product_id",
            "product_snapshot",
            "fulfillment_status",
            "fulfillment_error",
            "gateway_order_id",
            "gateway_response",
            "status",
            "created_at_unix_ms",
            "paid_at_unix_secs",
            "credited_at_unix_secs",
            "expires_at_unix_secs",
        ] {
            assert!(
                ADMIN_PAYMENT_ORDER_RETURNING_COLUMNS.contains(column),
                "payment order projection should include {column}"
            );
        }
    }

    #[test]
    fn payment_order_read_queries_include_debt_repayment() {
        for (query_name, query) in [
            (
                "LIST_ADMIN_PAYMENT_ORDERS_SQL",
                LIST_ADMIN_PAYMENT_ORDERS_SQL,
            ),
            ("FIND_ADMIN_PAYMENT_ORDER_SQL", FIND_ADMIN_PAYMENT_ORDER_SQL),
            (
                "LIST_WALLET_PAYMENT_ORDERS_BY_USER_SQL",
                LIST_WALLET_PAYMENT_ORDERS_BY_USER_SQL,
            ),
            (
                "FIND_WALLET_PAYMENT_ORDER_BY_USER_SQL",
                FIND_WALLET_PAYMENT_ORDER_BY_USER_SQL,
            ),
        ] {
            assert!(
                query.contains("AS debt_repayment_usd"),
                "{query_name} must return debt_repayment_usd for the shared order mapper"
            );
        }
    }

    #[tokio::test]
    async fn postgres_payment_callback_credits_the_full_settled_decimal_amount_when_url_is_set() {
        let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            eprintln!(
                "skipping postgres payment callback precision test because AETHER_TEST_POSTGRES_URL is unset"
            );
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("postgres test pool should connect");
        crate::lifecycle::migrate::run_migrations(&pool)
            .await
            .expect("postgres migrations should run");

        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let key = &suffix[..20];
        let user_id = format!("u-pay-{key}");
        let wallet_id = format!("w-pay-{key}");
        let order_no = format!("po-pay-{suffix}");
        let gateway_order_id = format!("gw-pay-{suffix}");
        sqlx::query("INSERT INTO users (id, username, email_verified) VALUES ($1, $2, FALSE)")
            .bind(&user_id)
            .bind(format!("payment-precision-{suffix}"))
            .execute(&pool)
            .await
            .expect("user should seed");

        let repository = SqlxWalletRepository::new(pool.clone());
        repository
            .create_wallet_recharge_order(CreateWalletRechargeOrderInput {
                preferred_wallet_id: Some(wallet_id.clone()),
                user_id: user_id.clone(),
                amount_usd: 10.0,
                pay_amount: Some(72.0),
                pay_currency: Some("CNY".to_string()),
                exchange_rate: Some(7.2),
                payment_method: "epay".to_string(),
                payment_provider: Some("epay".to_string()),
                payment_channel: Some("alipay".to_string()),
                gateway_order_id: gateway_order_id.clone(),
                gateway_response: json!({}),
                order_no: order_no.clone(),
                expires_at_unix_secs: 4_102_444_800,
            })
            .await
            .expect("recharge order should create");

        let callback_input = ProcessPaymentCallbackInput {
            payment_method: "epay".to_string(),
            payment_provider: Some("epay".to_string()),
            payment_channel: Some("alipay".to_string()),
            callback_key: format!("callback-pay-{suffix}"),
            order_no: Some(order_no),
            gateway_order_id: Some(gateway_order_id),
            amount_usd: 72.01 / 7.3,
            pay_amount: Some(72.01),
            pay_currency: Some("CNY".to_string()),
            exchange_rate: Some(7.3),
            payload_hash: format!("payload-pay-{suffix}"),
            payload: json!({ "money": "72.01" }),
            signature_valid: true,
        };
        let callback = repository
            .process_payment_callback(callback_input.clone())
            .await
            .expect("callback should process");
        let ProcessPaymentCallbackOutcome::Applied { order, .. } = callback else {
            panic!("callback should credit the order");
        };
        let expected_amount_usd = 10.001_388_89;
        assert!((order.amount_usd - expected_amount_usd).abs() < 0.000_000_001);
        assert!((order.refundable_amount_usd - expected_amount_usd).abs() < 0.000_000_001);
        assert_eq!(order.pay_amount, Some(72.01));
        assert_eq!(order.pay_currency.as_deref(), Some("CNY"));
        assert_eq!(order.exchange_rate, Some(7.2));

        let wallet = repository
            .find(WalletLookupKey::UserId(&user_id))
            .await
            .expect("wallet should query")
            .expect("wallet should exist");
        assert!((wallet.balance - expected_amount_usd).abs() < 0.000_000_001);
        assert!((wallet.total_recharged - expected_amount_usd).abs() < 0.000_000_001);

        let transaction_amount: f64 = sqlx::query_scalar(
            "SELECT CAST(amount AS DOUBLE PRECISION) FROM wallet_transactions WHERE link_type = 'payment_order' AND link_id = $1",
        )
        .bind(&order.id)
        .fetch_one(&pool)
        .await
        .expect("wallet transaction should query");
        assert!((transaction_amount - expected_amount_usd).abs() < 0.000_000_001);

        let duplicate = repository
            .process_payment_callback(callback_input)
            .await
            .expect("duplicate callback should resolve");
        assert!(matches!(
            duplicate,
            ProcessPaymentCallbackOutcome::DuplicateProcessed { .. }
        ));
        let wallet_after_duplicate = repository
            .find(WalletLookupKey::UserId(&user_id))
            .await
            .expect("wallet should query")
            .expect("wallet should exist");
        assert!((wallet_after_duplicate.balance - expected_amount_usd).abs() < 0.000_000_001);

        let currency_order_no = format!("po-pay-currency-{suffix}");
        let currency_gateway_order_id = format!("gw-pay-currency-{suffix}");
        repository
            .create_wallet_recharge_order(CreateWalletRechargeOrderInput {
                preferred_wallet_id: Some(wallet_id),
                user_id: user_id.clone(),
                amount_usd: 10.0,
                pay_amount: Some(72.0),
                pay_currency: Some("CNY".to_string()),
                exchange_rate: Some(7.2),
                payment_method: "dodopay".to_string(),
                payment_provider: Some("dodopay".to_string()),
                payment_channel: Some("alipay".to_string()),
                gateway_order_id: currency_gateway_order_id.clone(),
                gateway_response: json!({}),
                order_no: currency_order_no.clone(),
                expires_at_unix_secs: 4_102_444_800,
            })
            .await
            .expect("currency validation order should create");
        let currency_mismatch = repository
            .process_payment_callback(ProcessPaymentCallbackInput {
                payment_method: "dodopay".to_string(),
                payment_provider: Some("dodopay".to_string()),
                payment_channel: Some("alipay".to_string()),
                callback_key: format!("callback-pay-currency-{suffix}"),
                order_no: Some(currency_order_no),
                gateway_order_id: Some(currency_gateway_order_id),
                amount_usd: 72.0,
                pay_amount: Some(72.0),
                pay_currency: Some("USD".to_string()),
                exchange_rate: Some(1.0),
                payload_hash: format!("payload-pay-currency-{suffix}"),
                payload: json!({ "currency": "USD", "amount": "72.00" }),
                signature_valid: true,
            })
            .await
            .expect("currency mismatch callback should resolve");
        assert!(matches!(
            currency_mismatch,
            ProcessPaymentCallbackOutcome::Failed { ref error, .. }
                if error == "payment currency mismatch"
        ));
        let wallet_after_currency_mismatch = repository
            .find(WalletLookupKey::UserId(&user_id))
            .await
            .expect("wallet should query")
            .expect("wallet should exist");
        assert!(
            (wallet_after_currency_mismatch.balance - expected_amount_usd).abs() < 0.000_000_001
        );
    }

    #[tokio::test]
    async fn postgres_plan_purchase_repays_only_the_recorded_debt_when_url_is_set() {
        let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            eprintln!("skipping postgres plan debt repayment test because AETHER_TEST_POSTGRES_URL is unset");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("postgres test pool should connect");
        crate::lifecycle::migrate::run_migrations(&pool)
            .await
            .expect("postgres migrations should run");

        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let key = &suffix[..24];
        let user_id = format!("u-debt-{key}");
        let wallet_id = format!("w-debt-{key}");
        let plan_id = format!("p-debt-{key}");
        sqlx::query("INSERT INTO users (id, username, email_verified) VALUES ($1, $2, FALSE)")
            .bind(&user_id)
            .bind(format!("plan-debt-{suffix}"))
            .execute(&pool)
            .await
            .expect("user should seed");
        sqlx::query(
            r#"
INSERT INTO billing_plans (
  id, title, price_amount, price_currency, duration_unit, duration_value,
  max_active_per_user, purchase_limit_scope, entitlements_json, created_at, updated_at
) VALUES ($1, '欠费回调测试套餐', 1, 'USD', 'month', 1, 1, 'active_period', $2, NOW(), NOW())
            "#,
        )
        .bind(&plan_id)
        .bind(json!([{"type": "daily_quota", "daily_quota_usd": 10.0}]))
        .execute(&pool)
        .await
        .expect("billing plan should seed");

        let repository = SqlxWalletRepository::new(pool.clone());
        repository
            .create_wallet_recharge_order(CreateWalletRechargeOrderInput {
                preferred_wallet_id: Some(wallet_id.clone()),
                user_id: user_id.clone(),
                amount_usd: 1.0,
                pay_amount: Some(1.0),
                pay_currency: Some("USD".to_string()),
                exchange_rate: Some(1.0),
                payment_method: "bootstrap".to_string(),
                payment_provider: None,
                payment_channel: None,
                gateway_order_id: format!("gw-bootstrap-{suffix}"),
                gateway_response: json!({}),
                order_no: format!("po-bootstrap-{suffix}"),
                expires_at_unix_secs: 4_102_444_800,
            })
            .await
            .expect("wallet should create");
        sqlx::query("UPDATE wallets SET balance = -3 WHERE id = $1")
            .bind(&wallet_id)
            .execute(&pool)
            .await
            .expect("wallet should start in debt");

        let order = match repository
            .create_plan_purchase_order(CreatePlanPurchaseOrderInput {
                preferred_wallet_id: None,
                user_id: user_id.clone(),
                amount_usd: 1.0,
                pay_amount: 1.0,
                pay_currency: "USD".to_string(),
                exchange_rate: 1.0,
                payment_method: "epay".to_string(),
                payment_provider: Some("epay".to_string()),
                payment_channel: Some("alipay".to_string()),
                gateway_order_id: format!("gw-plan-{suffix}"),
                gateway_response: json!({}),
                order_no: format!("po-plan-{suffix}"),
                product_id: plan_id.clone(),
                product_snapshot: json!({
                    "id": plan_id,
                    "duration_unit": "month",
                    "duration_value": 1,
                    "purchase_limit_scope": "active_period",
                    "max_active_per_user": 1,
                    "entitlements": [{"type": "daily_quota", "daily_quota_usd": 10.0}]
                }),
                expires_at_unix_secs: 4_102_444_800,
            })
            .await
            .expect("plan order should create")
        {
            CreatePlanPurchaseOrderOutcome::Created(order) => order,
            other => panic!("debt should be included in the plan order, got {other:?}"),
        };
        assert_eq!(order.amount_usd, 4.0);
        assert_eq!(order.debt_repayment_usd, 3.0);
        assert_eq!(order.pay_amount, Some(4.0));

        let admin_page = repository
            .list_admin_payment_orders(&AdminPaymentOrderListQuery {
                status: None,
                payment_method: None,
                order_kind: None,
                user_search: Some(user_id.clone()),
                limit: 10,
                offset: 0,
            })
            .await
            .expect("admin payment order list should include debt repayment");
        let admin_list_order = admin_page
            .items
            .iter()
            .find(|item| item.id == order.id)
            .expect("admin payment order list should contain the plan order");
        assert_eq!(admin_list_order.debt_repayment_usd, 3.0);

        let admin_detail = repository
            .find_admin_payment_order(&order.id)
            .await
            .expect("admin payment order detail should include debt repayment")
            .expect("admin payment order detail should exist");
        assert_eq!(admin_detail.debt_repayment_usd, 3.0);

        let wallet_page = repository
            .list_wallet_payment_orders_by_user_id(&user_id, 10, 0)
            .await
            .expect("wallet payment order list should include debt repayment");
        let wallet_list_order = wallet_page
            .items
            .iter()
            .find(|item| item.id == order.id)
            .expect("wallet payment order list should contain the plan order");
        assert_eq!(wallet_list_order.debt_repayment_usd, 3.0);

        let wallet_detail = repository
            .find_wallet_payment_order_by_user_id(&user_id, &order.id)
            .await
            .expect("wallet payment order detail should include debt repayment")
            .expect("wallet payment order detail should exist");
        assert_eq!(wallet_detail.debt_repayment_usd, 3.0);

        sqlx::query("UPDATE wallets SET balance = -4 WHERE id = $1")
            .bind(&wallet_id)
            .execute(&pool)
            .await
            .expect("new usage should add one dollar of debt after checkout");
        let credit = repository
            .credit_admin_payment_order(CreditAdminPaymentOrderInput {
                order_id: order.id.clone(),
                gateway_order_id: Some(format!("gw-plan-paid-{suffix}")),
                pay_amount: Some(4.0),
                pay_currency: Some("USD".to_string()),
                exchange_rate: Some(1.0),
                gateway_response_patch: None,
                operator_id: Some("postgres-test".to_string()),
            })
            .await
            .expect("credit should return a business outcome");
        let WalletMutationOutcome::Applied((credited_order, true)) = credit else {
            panic!("paid plan order should be fulfilled");
        };
        assert_eq!(credited_order.status, "credited");

        let balance: f64 = sqlx::query_scalar(
            "SELECT CAST(balance + gift_balance AS DOUBLE PRECISION) FROM wallets WHERE id = $1",
        )
        .bind(&wallet_id)
        .fetch_one(&pool)
        .await
        .expect("wallet balance should query");
        assert_eq!(balance, -1.0);
        let debt_ledger_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM wallet_transactions WHERE wallet_id = $1 AND reason_code = 'plan_debt_repayment' AND link_id = $2",
        )
        .bind(&wallet_id)
        .bind(&order.id)
        .fetch_one(&pool)
        .await
        .expect("debt repayment ledger should query");
        assert_eq!(debt_ledger_count, 1);
        let entitlement_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_plan_entitlements WHERE payment_order_id = $1",
        )
        .bind(&order.id)
        .fetch_one(&pool)
        .await
        .expect("entitlement should query");
        assert_eq!(entitlement_count, 1);

        let duplicate = repository
            .credit_admin_payment_order(CreditAdminPaymentOrderInput {
                order_id: order.id.clone(),
                gateway_order_id: Some(format!("gw-plan-paid-{suffix}")),
                pay_amount: Some(4.0),
                pay_currency: Some("USD".to_string()),
                exchange_rate: Some(1.0),
                gateway_response_patch: None,
                operator_id: Some("postgres-test".to_string()),
            })
            .await
            .expect("duplicate credit should return the existing result");
        let WalletMutationOutcome::Applied((_order, false)) = duplicate else {
            panic!("duplicate credit should not apply again");
        };
        let balance_after_duplicate: f64 = sqlx::query_scalar(
            "SELECT CAST(balance + gift_balance AS DOUBLE PRECISION) FROM wallets WHERE id = $1",
        )
        .bind(&wallet_id)
        .fetch_one(&pool)
        .await
        .expect("wallet balance should query after duplicate credit");
        assert_eq!(balance_after_duplicate, -1.0);
    }

    #[tokio::test]
    async fn postgres_plan_purchase_limits_are_enforced_when_url_is_set() {
        let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            eprintln!(
                "skipping postgres plan purchase limit test because AETHER_TEST_POSTGRES_URL is unset"
            );
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("postgres test pool should connect");
        crate::lifecycle::migrate::run_migrations(&pool)
            .await
            .expect("postgres migrations should run");

        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let key = &suffix[..20];
        let active_user_id = format!("u-active-{key}");
        let active_wallet_id = format!("w-active-{key}");
        let active_plan_id = format!("p-active-{key}");
        let other_plan_id = format!("p-other-{key}");
        let lifetime_user_id = format!("u-life-{key}");
        let lifetime_wallet_id = format!("w-life-{key}");
        let lifetime_plan_id = format!("p-life-{key}");
        let repository = SqlxWalletRepository::new(pool.clone());

        for (user_id, wallet_id, label) in [
            (&active_user_id, &active_wallet_id, "active"),
            (&lifetime_user_id, &lifetime_wallet_id, "lifetime"),
        ] {
            sqlx::query("INSERT INTO users (id, username, email_verified) VALUES ($1, $2, FALSE)")
                .bind(user_id)
                .bind(format!("plan-limit-{label}-{suffix}"))
                .execute(&pool)
                .await
                .expect("plan limit user should seed");
            repository
                .create_wallet_recharge_order(CreateWalletRechargeOrderInput {
                    preferred_wallet_id: Some(wallet_id.to_string()),
                    user_id: user_id.to_string(),
                    amount_usd: 1.0,
                    pay_amount: Some(1.0),
                    pay_currency: Some("USD".to_string()),
                    exchange_rate: Some(1.0),
                    payment_method: "bootstrap".to_string(),
                    payment_provider: None,
                    payment_channel: None,
                    gateway_order_id: format!("gw-bootstrap-{label}-{suffix}"),
                    gateway_response: json!({ "bootstrap": true }),
                    order_no: format!("po-bootstrap-{label}-{suffix}"),
                    expires_at_unix_secs: 4_102_444_800,
                })
                .await
                .expect("plan limit wallet should create");
        }

        let active_snapshot = json!({
            "id": active_plan_id,
            "title": "Active plan",
            "duration_unit": "month",
            "duration_value": 1,
            "max_active_per_user": 1,
            "purchase_limit_scope": "active_period",
            "entitlements": [{"type": "daily_quota", "daily_quota_usd": 10.0}]
        });
        let other_snapshot = json!({
            "id": other_plan_id,
            "title": "Other plan",
            "duration_unit": "month",
            "duration_value": 1,
            "max_active_per_user": 1,
            "purchase_limit_scope": "active_period",
            "entitlements": [{"type": "daily_quota", "daily_quota_usd": 20.0}]
        });
        let lifetime_snapshot = json!({
            "id": lifetime_plan_id,
            "title": "Lifetime plan",
            "duration_unit": "month",
            "duration_value": 1,
            "max_active_per_user": 1,
            "purchase_limit_scope": "lifetime",
            "entitlements": [{"type": "wallet_credit", "amount_usd": 1.0, "balance_bucket": "gift"}]
        });
        for (plan_id, title, scope, snapshot) in [
            (
                &active_plan_id,
                "Active plan",
                "active_period",
                &active_snapshot,
            ),
            (
                &other_plan_id,
                "Other plan",
                "active_period",
                &other_snapshot,
            ),
            (
                &lifetime_plan_id,
                "Lifetime plan",
                "lifetime",
                &lifetime_snapshot,
            ),
        ] {
            sqlx::query(
                r#"
INSERT INTO billing_plans (
  id, title, price_amount, price_currency, duration_unit, duration_value,
  max_active_per_user, purchase_limit_scope, entitlements_json, created_at, updated_at
) VALUES ($1, $2, 1, 'USD', 'month', 1, 1, $3, $4, NOW(), NOW())
                "#,
            )
            .bind(plan_id)
            .bind(title)
            .bind(scope)
            .bind(snapshot["entitlements"].clone())
            .execute(&pool)
            .await
            .expect("plan should seed");
        }

        let active_order = match repository
            .create_plan_purchase_order(plan_purchase_input(
                &active_user_id,
                &active_plan_id,
                &active_snapshot,
                &format!("active-first-{suffix}"),
            ))
            .await
            .expect("first active-period plan order should resolve")
        {
            CreatePlanPurchaseOrderOutcome::Created(order) => order,
            other => panic!("first active-period plan order should be created, got {other:?}"),
        };
        let duplicate_pending = repository
            .create_plan_purchase_order(plan_purchase_input(
                &active_user_id,
                &active_plan_id,
                &active_snapshot,
                &format!("active-duplicate-{suffix}"),
            ))
            .await
            .expect("duplicate pending order should resolve");
        assert!(matches!(
            duplicate_pending,
            CreatePlanPurchaseOrderOutcome::ActivePlanLimitReached
        ));
        let different_pending = repository
            .create_plan_purchase_order(plan_purchase_input(
                &active_user_id,
                &other_plan_id,
                &other_snapshot,
                &format!("other-pending-{suffix}"),
            ))
            .await
            .expect("different pending plan should resolve");
        assert!(matches!(
            different_pending,
            CreatePlanPurchaseOrderOutcome::OverlappingPlanExists
        ));

        let WalletMutationOutcome::Applied((_, true)) = repository
            .credit_admin_payment_order(CreditAdminPaymentOrderInput {
                order_id: active_order.id,
                gateway_order_id: Some(format!("gw-active-paid-{suffix}")),
                pay_amount: Some(1.0),
                pay_currency: Some("USD".to_string()),
                exchange_rate: Some(1.0),
                gateway_response_patch: Some(json!({ "settled": true })),
                operator_id: Some("postgres-test".to_string()),
            })
            .await
            .expect("active-period plan credit should resolve")
        else {
            panic!("active-period plan credit should apply");
        };
        let different_active = repository
            .create_plan_purchase_order(plan_purchase_input(
                &active_user_id,
                &other_plan_id,
                &other_snapshot,
                &format!("other-active-{suffix}"),
            ))
            .await
            .expect("different active plan should resolve");
        assert!(matches!(
            different_active,
            CreatePlanPurchaseOrderOutcome::OverlappingPlanExists
        ));

        let lifetime_order = match repository
            .create_plan_purchase_order(plan_purchase_input(
                &lifetime_user_id,
                &lifetime_plan_id,
                &lifetime_snapshot,
                &format!("lifetime-first-{suffix}"),
            ))
            .await
            .expect("first lifetime plan order should resolve")
        {
            CreatePlanPurchaseOrderOutcome::Created(order) => order,
            other => panic!("first lifetime plan order should be created, got {other:?}"),
        };
        let WalletMutationOutcome::Applied((_, true)) = repository
            .credit_admin_payment_order(CreditAdminPaymentOrderInput {
                order_id: lifetime_order.id,
                gateway_order_id: Some(format!("gw-lifetime-paid-{suffix}")),
                pay_amount: Some(1.0),
                pay_currency: Some("USD".to_string()),
                exchange_rate: Some(1.0),
                gateway_response_patch: Some(json!({ "settled": true })),
                operator_id: Some("postgres-test".to_string()),
            })
            .await
            .expect("lifetime plan credit should resolve")
        else {
            panic!("lifetime plan credit should apply");
        };
        let lifetime_second = repository
            .create_plan_purchase_order(plan_purchase_input(
                &lifetime_user_id,
                &lifetime_plan_id,
                &lifetime_snapshot,
                &format!("lifetime-second-{suffix}"),
            ))
            .await
            .expect("second lifetime plan order should resolve");
        assert!(matches!(
            lifetime_second,
            CreatePlanPurchaseOrderOutcome::ActivePlanLimitReached
        ));
    }

    fn plan_purchase_input(
        user_id: &str,
        plan_id: &str,
        snapshot: &serde_json::Value,
        order_key: &str,
    ) -> CreatePlanPurchaseOrderInput {
        CreatePlanPurchaseOrderInput {
            preferred_wallet_id: None,
            user_id: user_id.to_string(),
            amount_usd: 1.0,
            pay_amount: 1.0,
            pay_currency: "USD".to_string(),
            exchange_rate: 1.0,
            payment_method: "epay".to_string(),
            payment_provider: Some("epay".to_string()),
            payment_channel: Some("card".to_string()),
            gateway_order_id: format!("gw-{order_key}"),
            gateway_response: json!({ "checkout": true }),
            order_no: format!("po-{order_key}"),
            product_id: plan_id.to_string(),
            product_snapshot: snapshot.clone(),
            expires_at_unix_secs: 4_102_444_800,
        }
    }
}

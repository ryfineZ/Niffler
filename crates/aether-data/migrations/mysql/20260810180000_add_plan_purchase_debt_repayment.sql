ALTER TABLE payment_orders
  ADD COLUMN debt_repayment_usd DECIMAL(20,8) NOT NULL DEFAULT 0 AFTER amount_usd;

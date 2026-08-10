ALTER TABLE payment_orders
  ADD COLUMN debt_repayment_usd REAL NOT NULL DEFAULT 0;

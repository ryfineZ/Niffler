ALTER TABLE public.payment_orders
  ADD COLUMN debt_repayment_usd numeric(20,8) NOT NULL DEFAULT 0;

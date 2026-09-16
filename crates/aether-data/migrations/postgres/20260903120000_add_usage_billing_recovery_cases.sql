-- Track historical usage billing recovery batches separately from the original usage facts.
-- Estimates are explicitly marked and must never be treated as upstream exact usage.
CREATE TABLE IF NOT EXISTS public.usage_billing_recovery_cases (
    request_id character varying(128) PRIMARY KEY
        REFERENCES public.usage(request_id) ON DELETE CASCADE,
    recovery_batch_id character varying(128) NOT NULL,
    user_id character varying(64),
    wallet_id character varying(64),
    provider_id character varying(64),
    provider_api_key_id character varying(64),
    provider_name character varying(255) NOT NULL,
    model character varying(255) NOT NULL,
    evidence_status character varying(32) NOT NULL,
    estimator_version character varying(64) NOT NULL,
    baseline_start timestamp with time zone NOT NULL,
    baseline_end timestamp with time zone NOT NULL,
    baseline_sample_count bigint NOT NULL DEFAULT 0,
    estimated_input_tokens bigint,
    estimated_output_tokens bigint,
    estimated_total_tokens bigint,
    estimated_base_cost_usd double precision,
    sales_multiplier double precision,
    estimated_user_cost_usd double precision,
    funding_source character varying(32),
    action character varying(32) NOT NULL DEFAULT 'staged',
    applied_at timestamp with time zone,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    updated_at timestamp with time zone NOT NULL DEFAULT now(),
    notes jsonb NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT usage_billing_recovery_cases_evidence_check CHECK (
        evidence_status IN ('provider_model_p50', 'model_p50', 'provider_p50', 'unrecoverable')
    ),
    CONSTRAINT usage_billing_recovery_cases_action_check CHECK (
        action IN ('staged', 'settled', 'waived', 'manual_review')
    )
);

CREATE INDEX IF NOT EXISTS ix_usage_billing_recovery_cases_batch
    ON public.usage_billing_recovery_cases (recovery_batch_id, action);
CREATE INDEX IF NOT EXISTS ix_usage_billing_recovery_cases_user
    ON public.usage_billing_recovery_cases (user_id, created_at);

COMMENT ON TABLE public.usage_billing_recovery_cases IS
    'Auditable historical usage billing recovery cases. Estimated values are explicitly marked and are not upstream exact usage.';

CREATE TABLE IF NOT EXISTS public.billing_plan_providers (
    plan_id character varying(64) NOT NULL
        REFERENCES public.billing_plans(id) ON DELETE CASCADE,
    provider_id character varying(36) NOT NULL
        REFERENCES public.providers(id) ON DELETE RESTRICT,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY (plan_id, provider_id)
);

CREATE INDEX IF NOT EXISTS idx_billing_plan_providers_provider
    ON public.billing_plan_providers (provider_id, plan_id);

CREATE TABLE IF NOT EXISTS public.user_entitlement_providers (
    user_entitlement_id character varying(64) NOT NULL
        REFERENCES public.user_plan_entitlements(id) ON DELETE CASCADE,
    provider_id character varying(36) NOT NULL
        REFERENCES public.providers(id) ON DELETE RESTRICT,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY (user_entitlement_id, provider_id)
);

CREATE INDEX IF NOT EXISTS idx_user_entitlement_providers_provider
    ON public.user_entitlement_providers (provider_id, user_entitlement_id);

CREATE TABLE IF NOT EXISTS public.billing_request_admissions (
    request_id character varying(128) PRIMARY KEY,
    user_id character varying(36) REFERENCES public.users(id) ON DELETE SET NULL,
    api_key_id character varying(36) REFERENCES public.api_keys(id) ON DELETE SET NULL,
    wallet_id character varying(36) REFERENCES public.wallets(id) ON DELETE SET NULL,
    global_model_id character varying(64),
    funding_source character varying(32) NOT NULL,
    wallet_balance_at_admission numeric(20,8),
    wallet_payment_allowed boolean NOT NULL DEFAULT false,
    wallet_overage_allowed boolean NOT NULL DEFAULT false,
    entitlement_ids jsonb NOT NULL DEFAULT '[]'::jsonb,
    entitlement_provider_scopes jsonb NOT NULL DEFAULT '{}'::jsonb,
    allowed_provider_ids jsonb NOT NULL DEFAULT '[]'::jsonb,
    billing_admitted boolean NOT NULL DEFAULT true,
    status character varying(32) NOT NULL DEFAULT 'admitted',
    rejection_reason text,
    schema_version smallint NOT NULL DEFAULT 1,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    updated_at timestamp with time zone NOT NULL DEFAULT now(),
    CONSTRAINT ck_billing_request_admissions_funding_source CHECK (
        funding_source IN ('wallet', 'plan', 'unlimited', 'free')
    ),
    CONSTRAINT ck_billing_request_admissions_entitlement_ids CHECK (
        jsonb_typeof(entitlement_ids) = 'array'
    ),
    CONSTRAINT ck_billing_request_admissions_entitlement_provider_scopes CHECK (
        jsonb_typeof(entitlement_provider_scopes) = 'object'
    ),
    CONSTRAINT ck_billing_request_admissions_provider_ids CHECK (
        jsonb_typeof(allowed_provider_ids) = 'array'
    )
);

CREATE INDEX IF NOT EXISTS idx_billing_request_admissions_user_created
    ON public.billing_request_admissions (user_id, created_at DESC);

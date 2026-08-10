CREATE TABLE IF NOT EXISTS billing_plan_providers (
    plan_id TEXT NOT NULL REFERENCES billing_plans(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (plan_id, provider_id)
);

CREATE INDEX IF NOT EXISTS idx_billing_plan_providers_provider
    ON billing_plan_providers (provider_id, plan_id);

CREATE TABLE IF NOT EXISTS user_entitlement_providers (
    user_entitlement_id TEXT NOT NULL
        REFERENCES user_plan_entitlements(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_entitlement_id, provider_id)
);

CREATE INDEX IF NOT EXISTS idx_user_entitlement_providers_provider
    ON user_entitlement_providers (provider_id, user_entitlement_id);

CREATE TABLE IF NOT EXISTS billing_request_admissions (
    request_id TEXT PRIMARY KEY,
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    api_key_id TEXT REFERENCES api_keys(id) ON DELETE SET NULL,
    wallet_id TEXT REFERENCES wallets(id) ON DELETE SET NULL,
    global_model_id TEXT,
    funding_source TEXT NOT NULL CHECK (
        funding_source IN ('wallet', 'plan', 'unlimited', 'free')
    ),
    wallet_balance_at_admission REAL,
    wallet_payment_allowed INTEGER NOT NULL DEFAULT 0,
    wallet_overage_allowed INTEGER NOT NULL DEFAULT 0,
    entitlement_ids TEXT NOT NULL CHECK (json_valid(entitlement_ids)),
    entitlement_provider_scopes TEXT NOT NULL CHECK (json_valid(entitlement_provider_scopes)),
    allowed_provider_ids TEXT NOT NULL CHECK (json_valid(allowed_provider_ids)),
    billing_admitted INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'admitted',
    rejection_reason TEXT,
    schema_version INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_billing_request_admissions_user_created
    ON billing_request_admissions (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS billing_plan_providers (
    plan_id VARCHAR(64) NOT NULL,
    provider_id VARCHAR(64) NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (plan_id, provider_id),
    KEY idx_billing_plan_providers_provider (provider_id, plan_id),
    CONSTRAINT fk_billing_plan_providers_plan
        FOREIGN KEY (plan_id) REFERENCES billing_plans(id) ON DELETE CASCADE,
    CONSTRAINT fk_billing_plan_providers_provider
        FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS user_entitlement_providers (
    user_entitlement_id VARCHAR(64) NOT NULL,
    provider_id VARCHAR(64) NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (user_entitlement_id, provider_id),
    KEY idx_user_entitlement_providers_provider (provider_id, user_entitlement_id),
    CONSTRAINT fk_user_entitlement_providers_entitlement
        FOREIGN KEY (user_entitlement_id) REFERENCES user_plan_entitlements(id) ON DELETE CASCADE,
    CONSTRAINT fk_user_entitlement_providers_provider
        FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS billing_request_admissions (
    request_id VARCHAR(128) PRIMARY KEY,
    user_id VARCHAR(64),
    api_key_id VARCHAR(64),
    wallet_id VARCHAR(64),
    global_model_id VARCHAR(64),
    funding_source VARCHAR(32) NOT NULL,
    wallet_balance_at_admission DOUBLE,
    wallet_payment_allowed TINYINT(1) NOT NULL DEFAULT 0,
    wallet_overage_allowed TINYINT(1) NOT NULL DEFAULT 0,
    entitlement_ids JSON NOT NULL,
    entitlement_provider_scopes JSON NOT NULL,
    allowed_provider_ids JSON NOT NULL,
    billing_admitted TINYINT(1) NOT NULL DEFAULT 1,
    status VARCHAR(32) NOT NULL DEFAULT 'admitted',
    rejection_reason TEXT,
    schema_version SMALLINT NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    KEY idx_billing_request_admissions_user_created (user_id, created_at),
    CONSTRAINT fk_billing_request_admissions_user
        FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL,
    CONSTRAINT fk_billing_request_admissions_api_key
        FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL,
    CONSTRAINT fk_billing_request_admissions_wallet
        FOREIGN KEY (wallet_id) REFERENCES wallets(id) ON DELETE SET NULL
);

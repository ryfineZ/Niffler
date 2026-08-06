pub mod antigravity;
pub mod chatgpt_web;
pub mod codex;
pub mod default;
pub mod grok;
pub mod grok_oauth;
pub mod kiro;
pub mod unsupported;

pub use antigravity::AntigravityProviderPoolAdapter;
pub use antigravity::{
    build_antigravity_pool_quota_request, ANTIGRAVITY_FETCH_AVAILABLE_MODELS_PATH,
};
pub use chatgpt_web::ChatGptWebProviderPoolAdapter;
pub use chatgpt_web::{
    build_chatgpt_web_pool_quota_request, enrich_chatgpt_web_quota_metadata,
    normalize_chatgpt_web_image_quota_limit, CHATGPT_WEB_CONVERSATION_INIT_PATH,
    CHATGPT_WEB_DEFAULT_BASE_URL,
};
pub use codex::CodexProviderPoolAdapter;
pub use codex::{
    build_codex_pool_quota_request, build_codex_pool_quota_reset_request,
    CODEX_WHAM_RESET_CREDITS_CONSUME_URL, CODEX_WHAM_USAGE_URL,
};
pub use default::DefaultProviderPoolAdapter;
pub use grok::{
    grok_mode_id_for_model, grok_pool_tier_from_quota_bucket, grok_quota_window_key_for_model,
    grok_supported_quota_windows_for_tier, GrokProviderPoolAdapter,
};
pub use grok_oauth::{
    build_grok_oauth_pool_billing_request, GrokOAuthProviderPoolAdapter,
    GROK_OAUTH_BILLING_MONTHLY_PATH, GROK_OAUTH_BILLING_WEEKLY_PATH, GROK_OAUTH_DEFAULT_BASE_URL,
};
pub use kiro::KiroProviderPoolAdapter;
pub use kiro::{
    build_kiro_pool_quota_request, KiroPoolQuotaAuthInput, KIRO_USAGE_LIMITS_PATH,
    KIRO_USAGE_SDK_VERSION,
};
pub use unsupported::{
    UnsupportedQuotaProviderPoolAdapter, CLAUDE_CODE_PROVIDER_POOL_ADAPTER,
    GEMINI_CLI_PROVIDER_POOL_ADAPTER, VERTEX_AI_PROVIDER_POOL_ADAPTER,
};

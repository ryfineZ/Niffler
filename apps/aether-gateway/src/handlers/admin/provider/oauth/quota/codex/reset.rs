use super::super::shared::{extract_execution_error_message, ProviderQuotaExecutionOutcome};
use super::plan::{build_codex_quota_reset_request_spec, execute_codex_quota_plan};
use crate::handlers::admin::request::AdminAppState;
use crate::provider_key_auth::provider_key_is_oauth_managed;
use crate::GatewayError;
use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexQuotaResetOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}

impl CodexQuotaResetOutcome {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Reset => "reset",
            Self::NothingToReset => "nothing_to_reset",
            Self::NoCredit => "no_credit",
            Self::AlreadyRedeemed => "already_redeemed",
        }
    }

    pub(crate) const fn reset_applied(self) -> bool {
        matches!(self, Self::Reset | Self::AlreadyRedeemed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexQuotaResetResult {
    pub(crate) outcome: CodexQuotaResetOutcome,
    pub(crate) windows_reset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexQuotaResetAttempt {
    Result(CodexQuotaResetResult),
    Failure {
        status_code: Option<u16>,
        message: String,
    },
}

pub(crate) async fn consume_codex_quota_reset_credit(
    state: &AdminAppState<'_>,
    provider: &StoredProviderCatalogProvider,
    endpoint: &StoredProviderCatalogEndpoint,
    key: &StoredProviderCatalogKey,
    redeem_request_id: &str,
) -> Result<CodexQuotaResetAttempt, GatewayError> {
    let Some(transport) = state
        .read_provider_transport_snapshot(&provider.id, &endpoint.id, &key.id)
        .await?
    else {
        return Ok(CodexQuotaResetAttempt::Failure {
            status_code: None,
            message: "Provider transport snapshot unavailable".to_string(),
        });
    };

    let resolved_oauth_auth = if provider_key_is_oauth_managed(key, provider.provider_type.as_str())
    {
        state.resolve_local_oauth_header_auth(&transport).await?
    } else {
        None
    };
    let request_spec = match build_codex_quota_reset_request_spec(
        &transport,
        redeem_request_id,
        resolved_oauth_auth,
    ) {
        Ok(request_spec) => request_spec,
        Err(message) => {
            return Ok(CodexQuotaResetAttempt::Failure {
                status_code: None,
                message,
            });
        }
    };
    let result = match execute_codex_quota_plan(state, &transport, request_spec, None).await? {
        ProviderQuotaExecutionOutcome::Response(result) => result,
        ProviderQuotaExecutionOutcome::Failure(message) => {
            return Ok(CodexQuotaResetAttempt::Failure {
                status_code: None,
                message,
            });
        }
    };
    if result.status_code != 200 {
        let message = extract_execution_error_message(&result)
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| format!("上游返回状态码 {}", result.status_code));
        return Ok(CodexQuotaResetAttempt::Failure {
            status_code: Some(result.status_code),
            message,
        });
    }

    let Some(body) = result
        .body
        .as_ref()
        .and_then(|body| body.json_body.as_ref())
    else {
        return Ok(CodexQuotaResetAttempt::Failure {
            status_code: Some(result.status_code),
            message: "无法解析额度重置响应".to_string(),
        });
    };
    match parse_codex_quota_reset_result(body) {
        Some(result) => Ok(CodexQuotaResetAttempt::Result(result)),
        None => Ok(CodexQuotaResetAttempt::Failure {
            status_code: Some(result.status_code),
            message: "额度重置响应缺少有效结果".to_string(),
        }),
    }
}

fn parse_codex_quota_reset_result(value: &serde_json::Value) -> Option<CodexQuotaResetResult> {
    let outcome = match value.get("code")?.as_str()?.trim() {
        "ok" | "reset" => CodexQuotaResetOutcome::Reset,
        "nothing_to_reset" => CodexQuotaResetOutcome::NothingToReset,
        "no_credit" => CodexQuotaResetOutcome::NoCredit,
        "already_redeemed" => CodexQuotaResetOutcome::AlreadyRedeemed,
        _ => return None,
    };
    let windows_reset = value
        .get("windows_reset")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    Some(CodexQuotaResetResult {
        outcome,
        windows_reset,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_codex_quota_reset_result, CodexQuotaResetOutcome};
    use serde_json::json;

    #[test]
    fn parses_all_codex_quota_reset_outcomes() {
        for (code, expected, applied) in [
            ("ok", CodexQuotaResetOutcome::Reset, true),
            ("reset", CodexQuotaResetOutcome::Reset, true),
            (
                "nothing_to_reset",
                CodexQuotaResetOutcome::NothingToReset,
                false,
            ),
            ("no_credit", CodexQuotaResetOutcome::NoCredit, false),
            (
                "already_redeemed",
                CodexQuotaResetOutcome::AlreadyRedeemed,
                true,
            ),
        ] {
            let parsed = parse_codex_quota_reset_result(&json!({
                "code": code,
                "windows_reset": 2
            }))
            .expect("known reset outcome should parse");
            assert_eq!(parsed.outcome, expected);
            assert_eq!(parsed.windows_reset, 2);
            assert_eq!(parsed.outcome.reset_applied(), applied);
        }
    }
}

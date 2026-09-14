use std::collections::BTreeSet;

use aether_contracts::ExecutionPlan;
use serde_json::{json, Value};
use tracing::debug;

use crate::provider_transport::GatewayProviderTransportSnapshot;
use crate::AppState;

const DEFAULT_STREAM_FAILOVER_MAX_WAIT_MS: u64 = 5_000;
const MIN_STREAM_FAILOVER_MAX_WAIT_MS: u64 = 250;
const MAX_STREAM_FAILOVER_MAX_WAIT_MS: u64 = 30_000;
const DEFAULT_STREAM_FAILOVER_MAX_BUFFER_BYTES: usize = 65_536;
const MIN_STREAM_FAILOVER_MAX_BUFFER_BYTES: u64 = 16_384;
const MAX_STREAM_FAILOVER_MAX_BUFFER_BYTES: u64 = 1_048_576;
const DEFAULT_STREAM_FAILOVER_COOLDOWN_SECONDS: u64 = 30;
const MIN_STREAM_FAILOVER_COOLDOWN_SECONDS: u64 = 1;
const MAX_STREAM_FAILOVER_COOLDOWN_SECONDS: u64 = 1_920;
const DEFAULT_STREAM_FAILOVER_MAX_ACCOUNT_SWITCHES: u64 = 2;
const MAX_STREAM_FAILOVER_MAX_ACCOUNT_SWITCHES: u64 = 999;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalStreamFailoverPolicy {
    pub(crate) enabled: bool,
    pub(crate) max_account_switches: u64,
    pub(crate) max_wait_ms: u64,
    pub(crate) max_buffer_bytes: usize,
    pub(crate) cooldown_seconds: u64,
}

impl Default for LocalStreamFailoverPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_account_switches: DEFAULT_STREAM_FAILOVER_MAX_ACCOUNT_SWITCHES,
            max_wait_ms: DEFAULT_STREAM_FAILOVER_MAX_WAIT_MS,
            max_buffer_bytes: DEFAULT_STREAM_FAILOVER_MAX_BUFFER_BYTES,
            cooldown_seconds: DEFAULT_STREAM_FAILOVER_COOLDOWN_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LocalFailoverPolicy {
    pub(crate) max_retries: Option<u64>,
    pub(crate) stop_status_codes: BTreeSet<u16>,
    pub(crate) continue_status_codes: BTreeSet<u16>,
    pub(crate) success_failover_patterns: Vec<LocalFailoverRegexRule>,
    pub(crate) error_stop_patterns: Vec<LocalFailoverRegexRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalFailoverRegexRule {
    pub(crate) pattern: String,
    pub(crate) status_codes: BTreeSet<u16>,
}

pub(crate) async fn resolve_local_failover_policy(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> LocalFailoverPolicy {
    if let Some(policy) = local_failover_policy_from_report_context(report_context) {
        debug!(
            event_name = "local_failover_policy_loaded",
            log_type = "debug",
            request_id = %plan.request_id,
            provider_id = %plan.provider_id,
            endpoint_id = %plan.endpoint_id,
            key_id = %plan.key_id,
            source = "report_context",
            max_retries = ?policy.max_retries,
            stop_status_code_count = policy.stop_status_codes.len(),
            continue_status_code_count = policy.continue_status_codes.len(),
            success_failover_pattern_count = policy.success_failover_patterns.len(),
            error_stop_pattern_count = policy.error_stop_patterns.len(),
            "gateway loaded local failover policy from report context"
        );
        return policy;
    }

    let transport = match state
        .read_provider_transport_snapshot(&plan.provider_id, &plan.endpoint_id, &plan.key_id)
        .await
    {
        Ok(Some(transport)) => transport,
        Ok(None) | Err(_) => return LocalFailoverPolicy::default(),
    };
    let policy = local_failover_policy_from_transport(&transport);
    debug!(
        event_name = "local_failover_policy_loaded",
        log_type = "debug",
        request_id = %plan.request_id,
        provider_id = %plan.provider_id,
        endpoint_id = %plan.endpoint_id,
        key_id = %plan.key_id,
        source = "transport_snapshot",
        max_retries = ?policy.max_retries,
        stop_status_code_count = policy.stop_status_codes.len(),
        continue_status_code_count = policy.continue_status_codes.len(),
        success_failover_pattern_count = policy.success_failover_patterns.len(),
        error_stop_pattern_count = policy.error_stop_patterns.len(),
        "gateway loaded local failover policy from transport snapshot"
    );
    policy
}

pub(crate) const GLOBAL_STREAM_FAILOVER_CONFIG_KEY: &str = "request_failover";

pub(crate) async fn read_global_stream_failover_policy(
    state: &AppState,
) -> LocalStreamFailoverPolicy {
    let value = match state
        .read_system_config_json_value(GLOBAL_STREAM_FAILOVER_CONFIG_KEY)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(
                event_name = "request_failover_config_read_failed",
                ?error,
                "failed to load request failover policy; using safe defaults"
            );
            None
        }
    };
    global_stream_failover_policy_from_value(value.as_ref())
}

pub(crate) fn global_stream_failover_policy_from_value(
    value: Option<&Value>,
) -> LocalStreamFailoverPolicy {
    let defaults = json!({"enabled": true, "max_account_switches": 2,
        "max_wait_ms": 5000, "max_buffer_bytes": 65536, "cooldown_seconds": 30});
    let mut config = defaults.as_object().unwrap().clone();
    if let Some(value) = value.and_then(Value::as_object) {
        config.extend(value.clone());
    }
    local_stream_failover_policy_from_report_context(Some(
        &json!({"stream_failover_policy": config}),
    ))
    .expect("global stream failover defaults are valid")
}

pub(crate) async fn resolve_local_stream_failover_policy(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
) -> LocalStreamFailoverPolicy {
    if !crate::ai_serving::is_openai_responses_format(&plan.provider_api_format)
        || crate::ai_serving::openai_request_is_image_generation_intent(
            plan.model_name.as_deref().unwrap_or_default(),
            plan.body.json_body.as_ref().unwrap_or(&Value::Null),
        )
    {
        return LocalStreamFailoverPolicy::default();
    }
    // Only trust the request snapshot created by the global planner, never legacy endpoint settings.
    if report_context
        .and_then(|v| v.get("global_stream_failover_policy"))
        .is_some()
    {
        return global_stream_failover_policy_from_value(
            report_context.and_then(|v| v.get("global_stream_failover_policy")),
        );
    }
    read_global_stream_failover_policy(state).await
}

pub(crate) fn local_failover_policy_from_transport(
    transport: &GatewayProviderTransportSnapshot,
) -> LocalFailoverPolicy {
    let rules = transport
        .provider
        .config
        .as_ref()
        .and_then(|config| config.get("failover_rules"))
        .and_then(Value::as_object);
    let max_retries = rules
        .and_then(|value| value.get("max_retries"))
        .and_then(parse_u64_value)
        .or_else(|| {
            transport
                .endpoint
                .max_retries
                .and_then(|value| u64::try_from(value).ok())
        })
        .or_else(|| {
            transport
                .provider
                .max_retries
                .and_then(|value| u64::try_from(value).ok())
        });

    let mut policy = LocalFailoverPolicy {
        max_retries,
        stop_status_codes: rules
            .map(|value| {
                parse_status_code_set(
                    value,
                    &[
                        "stop_on_status_codes",
                        "early_stop_status_codes",
                        "non_retryable_status_codes",
                        "stop_status_codes",
                    ],
                )
            })
            .unwrap_or_default(),
        continue_status_codes: rules
            .map(|value| {
                parse_status_code_set(
                    value,
                    &[
                        "continue_on_status_codes",
                        "retryable_status_codes",
                        "retry_on_status_codes",
                        "continue_status_codes",
                    ],
                )
            })
            .unwrap_or_default(),
        success_failover_patterns: rules
            .map(|value| parse_regex_rules(value, "success_failover_patterns"))
            .unwrap_or_default(),
        error_stop_patterns: rules
            .map(|value| parse_regex_rules(value, "error_stop_patterns"))
            .unwrap_or_default(),
    };

    if transport
        .provider
        .provider_type
        .trim()
        .eq_ignore_ascii_case("claude_code_api")
    {
        policy.stop_status_codes.insert(400);
    }

    policy
}

pub(crate) fn local_failover_policy_from_report_context(
    report_context: Option<&Value>,
) -> Option<LocalFailoverPolicy> {
    let object = report_context
        .and_then(Value::as_object)?
        .get("local_failover_policy")?
        .as_object()?;

    Some(LocalFailoverPolicy {
        max_retries: object.get("max_retries").and_then(parse_u64_value),
        stop_status_codes: object
            .get("stop_status_codes")
            .map(parse_status_code_list)
            .unwrap_or_default(),
        continue_status_codes: object
            .get("continue_status_codes")
            .map(parse_status_code_list)
            .unwrap_or_default(),
        success_failover_patterns: parse_regex_rules(object, "success_failover_patterns"),
        error_stop_patterns: parse_regex_rules(object, "error_stop_patterns"),
    })
}

pub(crate) fn local_stream_failover_policy_from_transport(
    transport: &GatewayProviderTransportSnapshot,
) -> LocalStreamFailoverPolicy {
    if !crate::ai_serving::is_openai_responses_format(&transport.endpoint.api_format) {
        return LocalStreamFailoverPolicy::default();
    }

    let Some(config) = transport
        .endpoint
        .config
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|config| config.get("stream_failover"))
        .and_then(Value::as_object)
    else {
        return LocalStreamFailoverPolicy::default();
    };

    let mode_is_supported = config
        .get("mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(|value| value.eq_ignore_ascii_case("before_output"))
        .unwrap_or(true);
    let max_wait_ms = config
        .get("max_wait_ms")
        .and_then(parse_u64_value)
        .unwrap_or(DEFAULT_STREAM_FAILOVER_MAX_WAIT_MS)
        .clamp(
            MIN_STREAM_FAILOVER_MAX_WAIT_MS,
            MAX_STREAM_FAILOVER_MAX_WAIT_MS,
        );
    let max_buffer_bytes = config
        .get("max_buffer_bytes")
        .and_then(parse_u64_value)
        .unwrap_or(DEFAULT_STREAM_FAILOVER_MAX_BUFFER_BYTES as u64)
        .clamp(
            MIN_STREAM_FAILOVER_MAX_BUFFER_BYTES,
            MAX_STREAM_FAILOVER_MAX_BUFFER_BYTES,
        ) as usize;
    let cooldown_seconds = config
        .get("cooldown_seconds")
        .and_then(parse_u64_value)
        .unwrap_or(DEFAULT_STREAM_FAILOVER_COOLDOWN_SECONDS)
        .clamp(
            MIN_STREAM_FAILOVER_COOLDOWN_SECONDS,
            MAX_STREAM_FAILOVER_COOLDOWN_SECONDS,
        );
    let max_account_switches = transport
        .endpoint
        .max_retries
        .and_then(|value| u64::try_from(value).ok())
        .or_else(|| {
            transport
                .provider
                .max_retries
                .and_then(|value| u64::try_from(value).ok())
        })
        .unwrap_or(DEFAULT_STREAM_FAILOVER_MAX_ACCOUNT_SWITCHES)
        .min(MAX_STREAM_FAILOVER_MAX_ACCOUNT_SWITCHES);

    LocalStreamFailoverPolicy {
        enabled: config
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && mode_is_supported,
        max_account_switches,
        max_wait_ms,
        max_buffer_bytes,
        cooldown_seconds,
    }
}

pub(crate) fn local_stream_failover_policy_from_report_context(
    report_context: Option<&Value>,
) -> Option<LocalStreamFailoverPolicy> {
    let object = report_context
        .and_then(Value::as_object)?
        .get("stream_failover_policy")?
        .as_object()?;

    Some(LocalStreamFailoverPolicy {
        enabled: object
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        max_account_switches: object
            .get("max_account_switches")
            .and_then(parse_u64_value)
            .unwrap_or(DEFAULT_STREAM_FAILOVER_MAX_ACCOUNT_SWITCHES)
            .min(MAX_STREAM_FAILOVER_MAX_ACCOUNT_SWITCHES),
        max_wait_ms: object
            .get("max_wait_ms")
            .and_then(parse_u64_value)
            .unwrap_or(DEFAULT_STREAM_FAILOVER_MAX_WAIT_MS)
            .clamp(
                MIN_STREAM_FAILOVER_MAX_WAIT_MS,
                MAX_STREAM_FAILOVER_MAX_WAIT_MS,
            ),
        max_buffer_bytes: object
            .get("max_buffer_bytes")
            .and_then(parse_u64_value)
            .unwrap_or(DEFAULT_STREAM_FAILOVER_MAX_BUFFER_BYTES as u64)
            .clamp(
                MIN_STREAM_FAILOVER_MAX_BUFFER_BYTES,
                MAX_STREAM_FAILOVER_MAX_BUFFER_BYTES,
            ) as usize,
        cooldown_seconds: object
            .get("cooldown_seconds")
            .and_then(parse_u64_value)
            .unwrap_or(DEFAULT_STREAM_FAILOVER_COOLDOWN_SECONDS)
            .clamp(
                MIN_STREAM_FAILOVER_COOLDOWN_SECONDS,
                MAX_STREAM_FAILOVER_COOLDOWN_SECONDS,
            ),
    })
}

pub(crate) fn append_local_failover_policy_to_value(
    value: Value,
    transport: &GatewayProviderTransportSnapshot,
) -> Value {
    let Value::Object(mut object) = value else {
        return value;
    };
    object.insert(
        "local_failover_policy".to_string(),
        local_failover_policy_to_value(&local_failover_policy_from_transport(transport)),
    );
    object.insert(
        "stream_failover_policy".to_string(),
        local_stream_failover_policy_to_value(&local_stream_failover_policy_from_transport(
            transport,
        )),
    );
    Value::Object(object)
}

pub(crate) fn validate_endpoint_stream_failover_config(
    api_format: &str,
    endpoint_config: Option<&Value>,
) -> Result<(), String> {
    let Some(stream_failover) = endpoint_config
        .and_then(Value::as_object)
        .and_then(|config| config.get("stream_failover"))
    else {
        return Ok(());
    };
    if stream_failover.is_null() {
        return Ok(());
    }
    if !crate::ai_serving::is_openai_responses_format(api_format) {
        return Err("stream_failover 仅适用于 openai:responses 端点".to_string());
    }
    let Some(object) = stream_failover.as_object() else {
        return Err("stream_failover 必须是对象或 null".to_string());
    };

    if object
        .get("enabled")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err("stream_failover.enabled 必须是布尔值".to_string());
    }
    if object.get("mode").is_some_and(|value| {
        !value
            .as_str()
            .is_some_and(|mode| mode.trim().eq_ignore_ascii_case("before_output"))
    }) {
        return Err("stream_failover.mode 只支持 before_output".to_string());
    }
    validate_stream_failover_integer(
        object,
        "max_wait_ms",
        MIN_STREAM_FAILOVER_MAX_WAIT_MS,
        MAX_STREAM_FAILOVER_MAX_WAIT_MS,
    )?;
    validate_stream_failover_integer(
        object,
        "max_buffer_bytes",
        MIN_STREAM_FAILOVER_MAX_BUFFER_BYTES,
        MAX_STREAM_FAILOVER_MAX_BUFFER_BYTES,
    )?;
    validate_stream_failover_integer(
        object,
        "cooldown_seconds",
        MIN_STREAM_FAILOVER_COOLDOWN_SECONDS,
        MAX_STREAM_FAILOVER_COOLDOWN_SECONDS,
    )?;
    Ok(())
}

fn validate_stream_failover_integer(
    object: &serde_json::Map<String, Value>,
    field: &str,
    min: u64,
    max: u64,
) -> Result<(), String> {
    let Some(value) = object.get(field) else {
        return Ok(());
    };
    let Some(value) = parse_u64_value(value) else {
        return Err(format!("stream_failover.{field} 必须是整数"));
    };
    if !(min..=max).contains(&value) {
        return Err(format!(
            "stream_failover.{field} 必须在 {min} 到 {max} 之间"
        ));
    }
    Ok(())
}

fn parse_status_code_list(value: &Value) -> BTreeSet<u16> {
    value
        .as_array()
        .into_iter()
        .flat_map(|values| values.iter())
        .filter_map(|value| parse_u64_value(value).and_then(|value| u16::try_from(value).ok()))
        .collect()
}

fn local_failover_policy_to_value(policy: &LocalFailoverPolicy) -> Value {
    json!({
        "max_retries": policy.max_retries,
        "stop_status_codes": policy.stop_status_codes.iter().copied().collect::<Vec<_>>(),
        "continue_status_codes": policy.continue_status_codes.iter().copied().collect::<Vec<_>>(),
        "success_failover_patterns": policy.success_failover_patterns.iter().map(local_failover_regex_rule_to_value).collect::<Vec<_>>(),
        "error_stop_patterns": policy.error_stop_patterns.iter().map(local_failover_regex_rule_to_value).collect::<Vec<_>>(),
    })
}

pub(crate) fn local_stream_failover_policy_to_value(policy: &LocalStreamFailoverPolicy) -> Value {
    json!({
        "enabled": policy.enabled,
        "max_account_switches": policy.max_account_switches,
        "max_wait_ms": policy.max_wait_ms,
        "max_buffer_bytes": policy.max_buffer_bytes,
        "cooldown_seconds": policy.cooldown_seconds,
    })
}

fn local_failover_regex_rule_to_value(rule: &LocalFailoverRegexRule) -> Value {
    json!({
        "pattern": rule.pattern,
        "status_codes": rule.status_codes.iter().copied().collect::<Vec<_>>(),
    })
}

fn parse_regex_rules(
    rules: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Vec<LocalFailoverRegexRule> {
    rules
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(parse_regex_rule)
        .collect()
}

fn parse_regex_rule(value: &serde_json::Value) -> Option<LocalFailoverRegexRule> {
    let object = value.as_object()?;
    let pattern = object
        .get("pattern")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(LocalFailoverRegexRule {
        pattern: pattern.to_string(),
        status_codes: object
            .get("status_codes")
            .and_then(Value::as_array)
            .into_iter()
            .flat_map(|values| values.iter())
            .filter_map(|value| parse_u64_value(value).and_then(|value| u16::try_from(value).ok()))
            .collect(),
    })
}

fn parse_status_code_set(
    rules: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> BTreeSet<u16> {
    keys.iter()
        .filter_map(|key| rules.get(*key))
        .filter_map(Value::as_array)
        .flat_map(|values| values.iter())
        .filter_map(|value| parse_u64_value(value).and_then(|value| u16::try_from(value).ok()))
        .collect()
}

fn parse_u64_value(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        append_local_failover_policy_to_value, local_failover_policy_from_report_context,
        local_failover_policy_from_transport, local_stream_failover_policy_from_report_context,
        local_stream_failover_policy_from_transport, validate_endpoint_stream_failover_config,
        LocalFailoverPolicy, LocalFailoverRegexRule, LocalStreamFailoverPolicy,
    };
    use crate::provider_transport::snapshot::{
        GatewayProviderTransportEndpoint, GatewayProviderTransportKey,
        GatewayProviderTransportProvider, GatewayProviderTransportSnapshot,
    };

    fn sample_transport(
        provider_max_retries: Option<i32>,
        endpoint_max_retries: Option<i32>,
        provider_config: Option<serde_json::Value>,
    ) -> GatewayProviderTransportSnapshot {
        GatewayProviderTransportSnapshot {
            provider: GatewayProviderTransportProvider {
                id: "provider-1".to_string(),
                name: "OpenAI".to_string(),
                provider_type: "llm".to_string(),
                website: None,
                is_active: true,
                keep_priority_on_conversion: false,
                enable_format_conversion: true,
                concurrent_limit: None,
                max_retries: provider_max_retries,
                proxy: None,
                request_timeout_secs: None,
                stream_first_byte_timeout_secs: None,
                config: provider_config,
            },
            endpoint: GatewayProviderTransportEndpoint {
                id: "endpoint-1".to_string(),
                provider_id: "provider-1".to_string(),
                api_format: "openai:chat".to_string(),
                api_family: Some("openai".to_string()),
                endpoint_kind: Some("chat".to_string()),
                is_active: true,
                base_url: "https://example.com".to_string(),
                header_rules: None,
                body_rules: None,
                max_retries: endpoint_max_retries,
                custom_path: None,
                config: None,
                format_acceptance_config: None,
                proxy: None,
            },
            key: GatewayProviderTransportKey {
                id: "key-1".to_string(),
                provider_id: "provider-1".to_string(),
                name: "primary".to_string(),
                auth_type: "bearer".to_string(),
                is_active: true,
                api_formats: None,
                auth_type_by_format: None,
                allow_auth_channel_mismatch_formats: None,

                allowed_models: None,
                capabilities: None,
                rate_multipliers: None,
                global_priority_by_format: None,
                expires_at_unix_secs: None,
                proxy: None,
                fingerprint: None,
                decrypted_api_key: "secret".to_string(),
                decrypted_auth_config: None,
            },
        }
    }

    #[test]
    fn append_local_failover_policy_to_value_round_trips_policy_shape() {
        let report_context = append_local_failover_policy_to_value(
            json!({
                "request_id": "req-1",
            }),
            &sample_transport(
                Some(5),
                Some(4),
                Some(json!({
                    "failover_rules": {
                        "max_retries": 2,
                        "continue_status_codes": [429],
                        "stop_status_codes": [400],
                        "success_failover_patterns": [{"pattern": "quota", "status_codes": [200]}],
                        "error_stop_patterns": [{"pattern": "validation", "status_codes": [422]}]
                    }
                })),
            ),
        );

        assert_eq!(
            local_failover_policy_from_report_context(Some(&report_context)),
            Some(LocalFailoverPolicy {
                max_retries: Some(2),
                stop_status_codes: [400].into_iter().collect(),
                continue_status_codes: [429].into_iter().collect(),
                success_failover_patterns: vec![LocalFailoverRegexRule {
                    pattern: "quota".to_string(),
                    status_codes: [200].into_iter().collect(),
                }],
                error_stop_patterns: vec![LocalFailoverRegexRule {
                    pattern: "validation".to_string(),
                    status_codes: [422].into_iter().collect(),
                }],
            })
        );
    }

    #[test]
    fn claude_code_api_stops_on_upstream_400_by_default() {
        let mut transport = sample_transport(None, None, None);
        transport.provider.provider_type = "claude_code_api".to_string();

        let policy = local_failover_policy_from_transport(&transport);

        assert!(policy.stop_status_codes.contains(&400));
    }

    #[test]
    fn openai_responses_stream_failover_policy_round_trips_through_report_context() {
        let mut transport = sample_transport(None, Some(2), None);
        transport.endpoint.api_format = "openai:responses".to_string();
        transport.endpoint.config = Some(json!({
            "stream_failover": {
                "enabled": true,
                "mode": "before_output",
                "max_wait_ms": 7_500,
                "max_buffer_bytes": 131_072,
                "cooldown_seconds": 45
            }
        }));

        let report_context =
            append_local_failover_policy_to_value(json!({"request_id": "req-1"}), &transport);
        let expected = LocalStreamFailoverPolicy {
            enabled: true,
            max_account_switches: 2,
            max_wait_ms: 7_500,
            max_buffer_bytes: 131_072,
            cooldown_seconds: 45,
        };

        assert_eq!(
            local_stream_failover_policy_from_transport(&transport),
            expected
        );
        assert_eq!(
            local_stream_failover_policy_from_report_context(Some(&report_context)),
            Some(expected)
        );
    }

    #[test]
    fn stream_failover_is_disabled_for_non_responses_endpoints() {
        let mut transport = sample_transport(None, None, None);
        transport.endpoint.config = Some(json!({
            "stream_failover": {"enabled": true}
        }));

        assert_eq!(
            local_stream_failover_policy_from_transport(&transport),
            LocalStreamFailoverPolicy::default()
        );
        assert_eq!(
            validate_endpoint_stream_failover_config(
                &transport.endpoint.api_format,
                transport.endpoint.config.as_ref(),
            ),
            Err("stream_failover 仅适用于 openai:responses 端点".to_string())
        );
    }

    #[test]
    fn stream_failover_config_rejects_invalid_ranges_and_mode() {
        assert_eq!(
            validate_endpoint_stream_failover_config(
                "openai:responses",
                Some(&json!({
                    "stream_failover": {
                        "enabled": true,
                        "mode": "after_output"
                    }
                })),
            ),
            Err("stream_failover.mode 只支持 before_output".to_string())
        );
        assert_eq!(
            validate_endpoint_stream_failover_config(
                "openai:responses",
                Some(&json!({
                    "stream_failover": {
                        "enabled": true,
                        "max_wait_ms": 100
                    }
                })),
            ),
            Err("stream_failover.max_wait_ms 必须在 250 到 30000 之间".to_string())
        );
    }

    #[test]
    fn imported_stream_failover_config_is_defensively_clamped() {
        let mut transport = sample_transport(None, None, None);
        transport.endpoint.api_format = "openai:responses".to_string();
        transport.endpoint.config = Some(json!({
            "stream_failover": {
                "enabled": true,
                "max_wait_ms": 1,
                "max_buffer_bytes": 9_999_999,
                "cooldown_seconds": 0
            }
        }));

        assert_eq!(
            local_stream_failover_policy_from_transport(&transport),
            LocalStreamFailoverPolicy {
                enabled: true,
                max_account_switches: 2,
                max_wait_ms: 250,
                max_buffer_bytes: 1_048_576,
                cooldown_seconds: 1,
            }
        );
    }
}

#[cfg(test)]
mod global_capacity_tests {
    use super::*;
    #[test]
    fn capacity_global_policy_defaults_enabled_and_validates_bounds() {
        let policy = global_stream_failover_policy_from_value(None);
        assert!(policy.enabled);
        assert_eq!(policy.max_account_switches, 2);
        assert_eq!(policy.max_wait_ms, 5000);
        assert!(!global_stream_failover_policy_from_value(Some(&json!({"enabled":false}))).enabled);
        assert_eq!(
            global_stream_failover_policy_from_value(Some(&json!({"max_account_switches": 0})))
                .max_account_switches,
            0
        );
    }
    #[tokio::test]
    async fn capacity_global_policy_ignores_legacy_endpoint_snapshot() {
        let state = AppState::new().unwrap();
        let plan: ExecutionPlan = serde_json::from_value(json!({"request_id":"global-policy", "provider_id":"p", "endpoint_id":"e", "key_id":"k", "method":"POST", "url":"https://example.invalid/responses", "body":{}, "stream":true, "client_api_format":"openai:responses", "provider_api_format":"openai:responses", "model_name":"gpt-test"})).unwrap();
        let legacy = json!({"stream_failover_policy":{"enabled":false,"max_account_switches":0}});
        assert!(
            resolve_local_stream_failover_policy(&state, &plan, Some(&legacy))
                .await
                .enabled
        );
        state
            .upsert_system_config_json_value(
                GLOBAL_STREAM_FAILOVER_CONFIG_KEY,
                &json!({"enabled":false}),
                None,
            )
            .await
            .unwrap();
        assert!(
            !resolve_local_stream_failover_policy(
                &state,
                &plan,
                Some(&json!({"stream_failover_policy":{"enabled":true}}))
            )
            .await
            .enabled
        );
    }
}

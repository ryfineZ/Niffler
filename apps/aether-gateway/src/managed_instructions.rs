use std::sync::LazyLock;

use crate::ai_serving::api::{
    codex_image_generation_bridge_text, codex_openai_responses_has_image_generation_tool,
    split_codex_image_generation_bridge_suffix,
};
use http::StatusCode;
use serde_json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{AiExecutionDecision, GatewayError};

pub(crate) const MANAGED_INSTRUCTIONS_CONFIG_FIELD: &str = "managed_instructions";
pub(crate) const MANAGED_INSTRUCTIONS_CORE_VERSION: &str = "core_v2";
pub(crate) const MANAGED_INSTRUCTIONS_CLIENT_MARKER: &str = "<niffler-managed-instructions";
pub(crate) const MANAGED_INSTRUCTIONS_SUPPORTED_FORMATS: [&str; 3] =
    ["openai:responses", "openai:chat", "claude:messages"];

const CORE_V2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/prompts/managed/core_v2.md"
));
const SECURITY_RESEARCH_V2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/prompts/managed/security_research_v2.md"
));
const ADULT_FICTION_V1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/prompts/managed/adult_fiction_v1.md"
));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagedInstructionsMergeMode {
    Prepend,
    IfMissing,
}

impl ManagedInstructionsMergeMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Prepend => "prepend",
            Self::IfMissing => "if_missing",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "prepend" => Ok(Self::Prepend),
            "if_missing" => Ok(Self::IfMissing),
            _ => Err("managed_instructions.merge_mode 只支持 prepend 或 if_missing".to_string()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedInstructionsConfig {
    pub(crate) enabled: bool,
    pub(crate) profile_id: String,
    pub(crate) merge_mode: ManagedInstructionsMergeMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedInstructionsProfile {
    pub(crate) profile_id: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) core_version: &'static str,
    pub(crate) domain_version: Option<&'static str>,
    pub(crate) embedded_text: String,
    pub(crate) profile_sha256: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedManagedInstructionsConfig {
    pub(crate) enabled: bool,
    pub(crate) merge_mode: ManagedInstructionsMergeMode,
    pub(crate) profile: &'static ManagedInstructionsProfile,
}

#[derive(Clone, Debug)]
pub(crate) struct ManagedInstructionsBindingSnapshot {
    pub(crate) user_group_id: Option<String>,
    pub(crate) managed_instructions_config_value: Option<Value>,
    pub(crate) config: Option<ResolvedManagedInstructionsConfig>,
}

#[derive(Clone, Copy)]
struct ProfileSource {
    profile_id: &'static str,
    display_name: &'static str,
    description: &'static str,
    domain_version: Option<&'static str>,
    domain_text: Option<&'static str>,
}

const PROFILE_SOURCES: [ProfileSource; 2] = [
    ProfileSource {
        profile_id: "security_research_v1",
        display_name: "Security & Reverse Engineering",
        description: "Authorized security research, CTF, exploitation, forensics, reverse engineering, binary patching, and key reconstruction.",
        domain_version: Some("security_research_v2"),
        domain_text: Some(SECURITY_RESEARCH_V2),
    },
    ProfileSource {
        profile_id: "adult_fiction_v1",
        display_name: "Adult Fiction",
        description: "Fictional adult creative-writing tasks.",
        domain_version: Some("adult_fiction_v1"),
        domain_text: Some(ADULT_FICTION_V1),
    },
];

static MANAGED_INSTRUCTIONS_PROFILES: LazyLock<Result<Vec<ManagedInstructionsProfile>, String>> =
    LazyLock::new(|| {
        PROFILE_SOURCES
            .iter()
            .map(build_profile)
            .collect::<Result<Vec<_>, _>>()
    });

pub(crate) fn managed_instructions_profiles(
) -> Result<&'static [ManagedInstructionsProfile], String> {
    match &*MANAGED_INSTRUCTIONS_PROFILES {
        Ok(profiles) => Ok(profiles.as_slice()),
        Err(message) => Err(message.clone()),
    }
}

pub(crate) fn managed_instructions_profile(
    profile_id: &str,
) -> Result<&'static ManagedInstructionsProfile, String> {
    managed_instructions_profiles()?
        .iter()
        .find(|profile| profile.profile_id == profile_id)
        .ok_or_else(|| format!("未知的受管理提示词配置：{profile_id}"))
}

pub(crate) fn parse_managed_instructions_config(
    managed_instructions: Option<&Value>,
) -> Result<Option<ManagedInstructionsConfig>, String> {
    let Some(value) = managed_instructions else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(value) = value.as_object() else {
        return Err("managed_instructions 必须是 JSON 对象".to_string());
    };
    let enabled = value
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| "managed_instructions.enabled 必须是布尔值".to_string())?;
    let profile_id = value
        .get("profile_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "managed_instructions.profile_id 必须是非空字符串".to_string())?;
    if profile_id.trim().is_empty() {
        return Err("managed_instructions.profile_id 必须是非空字符串".to_string());
    }
    if profile_id != profile_id.trim() {
        return Err("managed_instructions.profile_id 不能包含首尾空格".to_string());
    }
    let profile_id = profile_id.to_string();
    let merge_mode = value
        .get("merge_mode")
        .and_then(Value::as_str)
        .ok_or_else(|| "managed_instructions.merge_mode 必须是字符串".to_string())
        .and_then(ManagedInstructionsMergeMode::parse)?;
    managed_instructions_profile(&profile_id)?;
    Ok(Some(ManagedInstructionsConfig {
        enabled,
        profile_id,
        merge_mode,
    }))
}

pub(crate) fn resolve_managed_instructions_config(
    managed_instructions: Option<&Value>,
) -> Result<Option<ResolvedManagedInstructionsConfig>, String> {
    let Some(config) = parse_managed_instructions_config(managed_instructions)? else {
        return Ok(None);
    };
    let profile = managed_instructions_profile(&config.profile_id)?;
    Ok(Some(ResolvedManagedInstructionsConfig {
        enabled: config.enabled,
        merge_mode: config.merge_mode,
        profile,
    }))
}

pub(crate) fn validate_managed_instructions_config(
    managed_instructions: Option<&Value>,
) -> Result<(), String> {
    parse_managed_instructions_config(managed_instructions).map(|_| ())
}

pub(crate) fn record_managed_instructions_user_group(
    decision: &mut AiExecutionDecision,
    user_group_id: &str,
) -> Result<(), GatewayError> {
    let metadata = decision
        .report_context
        .as_mut()
        .and_then(Value::as_object_mut)
        .and_then(|context| context.get_mut("managed_instructions"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            GatewayError::Internal(
                "受管理提示词已处理，但运行记录缺少 managed_instructions".to_string(),
            )
        })?;
    metadata.insert(
        "user_group_id".to_string(),
        Value::String(user_group_id.to_string()),
    );
    Ok(())
}

pub(crate) fn apply_managed_instructions_to_decision(
    decision: &mut AiExecutionDecision,
    config: &ResolvedManagedInstructionsConfig,
) -> Result<(), GatewayError> {
    let provider_api_format = decision
        .provider_api_format
        .as_deref()
        .unwrap_or_default()
        .to_string();
    if let Some(existing) = existing_applied_state(decision) {
        let expected_target = target_field_for_format(&provider_api_format, decision);
        let same_profile =
            existing.get("profile_id").and_then(Value::as_str) == Some(config.profile.profile_id);
        let same_hash = existing.get("profile_sha256").and_then(Value::as_str)
            == Some(config.profile.profile_sha256.as_str());
        let same_target =
            existing.get("target_field").and_then(Value::as_str) == expected_target.as_deref();
        if !same_profile || !same_hash || !same_target {
            return Err(GatewayError::Internal(
                "同一请求检测到不同的受管理提示词配置、摘要或目标字段".to_string(),
            ));
        }
        let mut metadata = existing.clone();
        metadata.insert("deduplicated".to_string(), json!(true));
        metadata.insert("reason".to_string(), json!("already_applied"));
        insert_managed_instructions_metadata(decision, Value::Object(metadata))?;
        return Ok(());
    }

    if !config.enabled {
        let metadata = build_application_metadata(
            config,
            &provider_api_format,
            None,
            None,
            false,
            false,
            false,
            "disabled",
        );
        insert_managed_instructions_metadata(decision, metadata)?;
        return Ok(());
    }

    if !MANAGED_INSTRUCTIONS_SUPPORTED_FORMATS.contains(&provider_api_format.as_str()) {
        let metadata = build_application_metadata(
            config,
            &provider_api_format,
            None,
            None,
            false,
            false,
            false,
            "unsupported_provider_api_format",
        );
        insert_managed_instructions_metadata(decision, metadata)?;
        return Ok(());
    }

    let body = decision
        .provider_request_body
        .as_mut()
        .ok_or_else(|| GatewayError::Client {
            status: StatusCode::BAD_REQUEST,
            message: format!("受管理提示词无法应用到没有 JSON 请求体的 {provider_api_format} 请求"),
        })?;
    let body = body.as_object_mut().ok_or_else(|| GatewayError::Client {
        status: StatusCode::BAD_REQUEST,
        message: format!("{provider_api_format} 的最终请求体必须是 JSON 对象"),
    })?;

    let outcome = match provider_api_format.as_str() {
        "openai:responses" => apply_to_openai_responses(body, config)?,
        "openai:chat" => apply_to_openai_chat(body, config)?,
        "claude:messages" => apply_to_claude_messages(body, config)?,
        _ => unreachable!("supported formats are checked above"),
    };
    let metadata = build_application_metadata(
        config,
        &provider_api_format,
        outcome.target_field,
        Some(outcome.client_instructions_present),
        outcome.applied,
        false,
        outcome.client_marker_present,
        outcome.reason,
    );
    insert_managed_instructions_metadata(decision, metadata)?;
    Ok(())
}

#[derive(Debug)]
struct ApplicationOutcome {
    applied: bool,
    target_field: Option<&'static str>,
    client_instructions_present: bool,
    client_marker_present: bool,
    reason: &'static str,
}

fn apply_to_openai_responses(
    body: &mut serde_json::Map<String, Value>,
    config: &ResolvedManagedInstructionsConfig,
) -> Result<ApplicationOutcome, GatewayError> {
    let image_bridge_required = codex_openai_responses_has_image_generation_tool(body);
    let original = match body.get("instructions") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(value)) => value.clone(),
        Some(_) => {
            return Err(managed_instructions_body_error(
                "openai:responses 的 instructions 必须是字符串、null 或省略",
            ))
        }
    };
    let (client_instructions, image_bridge) = if image_bridge_required {
        let (client_instructions, exact_suffix) =
            split_codex_image_generation_bridge_suffix(&original);
        let client_instructions = match exact_suffix {
            Some(_) => client_instructions.to_string(),
            None => detach_known_image_bridge(&original),
        };
        (
            client_instructions,
            Some(codex_image_generation_bridge_text()),
        )
    } else {
        (original, None)
    };
    let client_instructions_present = has_nonempty_text(&client_instructions);
    let client_marker_present = client_instructions.contains(MANAGED_INSTRUCTIONS_CLIENT_MARKER);
    if config.merge_mode == ManagedInstructionsMergeMode::IfMissing && client_instructions_present {
        if let Some(image_bridge) = image_bridge {
            body.insert(
                "instructions".to_string(),
                Value::String(append_image_bridge(client_instructions, image_bridge)),
            );
        }
        return Ok(ApplicationOutcome {
            applied: false,
            target_field: None,
            client_instructions_present,
            client_marker_present,
            reason: "client_instructions_present",
        });
    }

    let mut instructions = if client_instructions.is_empty() {
        config.profile.embedded_text.clone()
    } else {
        format!(
            "{}\n\n<niffler-client-instructions>\n{}\n</niffler-client-instructions>",
            config.profile.embedded_text, client_instructions
        )
    };
    if let Some(image_bridge) = image_bridge {
        instructions = append_image_bridge(instructions, image_bridge);
    }
    body.insert("instructions".to_string(), Value::String(instructions));
    Ok(ApplicationOutcome {
        applied: true,
        target_field: Some("instructions"),
        client_instructions_present,
        client_marker_present,
        reason: "applied",
    })
}

fn detach_known_image_bridge(instructions: &str) -> String {
    let image_bridge = codex_image_generation_bridge_text();
    let Some(bridge_start) = instructions.rfind(image_bridge) else {
        return instructions.to_string();
    };
    let mut prefix_end = bridge_start;
    if instructions[..prefix_end].ends_with("\n\n") {
        prefix_end -= 2;
    }
    let mut suffix_start = bridge_start + image_bridge.len();
    if prefix_end == 0 && instructions[suffix_start..].starts_with("\n\n") {
        suffix_start += 2;
    }
    format!(
        "{}{}",
        &instructions[..prefix_end],
        &instructions[suffix_start..]
    )
}

fn append_image_bridge(mut instructions: String, image_bridge: &str) -> String {
    if !instructions.is_empty() {
        instructions.push_str("\n\n");
    }
    instructions.push_str(image_bridge);
    instructions
}

fn apply_to_openai_chat(
    body: &mut serde_json::Map<String, Value>,
    config: &ResolvedManagedInstructionsConfig,
) -> Result<ApplicationOutcome, GatewayError> {
    let messages = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| managed_instructions_body_error("openai:chat 的 messages 必须是数组"))?;
    let client_instructions_present = messages.iter().any(|message| {
        let Some(message) = message.as_object() else {
            return false;
        };
        let Some(role) = message.get("role").and_then(Value::as_str) else {
            return false;
        };
        matches!(role, "system" | "developer")
            && message.get("content").is_some_and(value_has_nonempty_text)
    });
    let client_marker_present = messages.iter().any(|message| {
        let Some(message) = message.as_object() else {
            return false;
        };
        let Some(role) = message.get("role").and_then(Value::as_str) else {
            return false;
        };
        matches!(role, "system" | "developer")
            && message.get("content").is_some_and(|value| {
                value_contains_marker(value, MANAGED_INSTRUCTIONS_CLIENT_MARKER)
            })
    });
    if config.merge_mode == ManagedInstructionsMergeMode::IfMissing && client_instructions_present {
        return Ok(ApplicationOutcome {
            applied: false,
            target_field: None,
            client_instructions_present,
            client_marker_present,
            reason: "client_instructions_present",
        });
    }
    messages.insert(
        0,
        json!({
            "role": "system",
            "content": config.profile.embedded_text,
        }),
    );
    Ok(ApplicationOutcome {
        applied: true,
        target_field: Some("messages[0]"),
        client_instructions_present,
        client_marker_present,
        reason: "applied",
    })
}

fn apply_to_claude_messages(
    body: &mut serde_json::Map<String, Value>,
    config: &ResolvedManagedInstructionsConfig,
) -> Result<ApplicationOutcome, GatewayError> {
    let original = body.get("system");
    let client_instructions_present = match original {
        None | Some(Value::Null) => false,
        Some(Value::String(value)) => has_nonempty_text(value),
        Some(Value::Array(blocks)) => blocks.iter().any(|block| {
            block
                .as_object()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .and_then(|block| block.get("text"))
                .is_some_and(value_has_nonempty_text)
        }),
        Some(_) => {
            return Err(managed_instructions_body_error(
                "claude:messages 的 system 必须是字符串、内容块数组、null 或省略",
            ))
        }
    };
    let client_marker_present = original
        .is_some_and(|value| value_contains_marker(value, MANAGED_INSTRUCTIONS_CLIENT_MARKER));
    if config.merge_mode == ManagedInstructionsMergeMode::IfMissing && client_instructions_present {
        return Ok(ApplicationOutcome {
            applied: false,
            target_field: None,
            client_instructions_present,
            client_marker_present,
            reason: "client_instructions_present",
        });
    }

    let target_field = match body.remove("system") {
        None | Some(Value::Null) => {
            body.insert(
                "system".to_string(),
                Value::String(config.profile.embedded_text.clone()),
            );
            "system"
        }
        Some(Value::String(client)) if client.is_empty() => {
            body.insert(
                "system".to_string(),
                Value::String(config.profile.embedded_text.clone()),
            );
            "system"
        }
        Some(Value::String(client)) => {
            body.insert(
                "system".to_string(),
                Value::String(format!("{}\n\n{}", config.profile.embedded_text, client)),
            );
            "system"
        }
        Some(Value::Array(mut blocks)) => {
            blocks.insert(
                0,
                json!({
                    "type": "text",
                    "text": config.profile.embedded_text,
                }),
            );
            body.insert("system".to_string(), Value::Array(blocks));
            "system[0]"
        }
        Some(_) => unreachable!("Claude system type is checked above"),
    };
    Ok(ApplicationOutcome {
        applied: true,
        target_field: Some(target_field),
        client_instructions_present,
        client_marker_present,
        reason: "applied",
    })
}

fn managed_instructions_body_error(message: &str) -> GatewayError {
    GatewayError::Client {
        status: StatusCode::BAD_REQUEST,
        message: message.to_string(),
    }
}

fn has_nonempty_text(value: &str) -> bool {
    !value.trim().is_empty()
}

fn value_has_nonempty_text(value: &Value) -> bool {
    match value {
        Value::String(value) => has_nonempty_text(value),
        Value::Array(values) => values.iter().any(value_has_nonempty_text),
        Value::Object(value) => value.get("text").is_some_and(value_has_nonempty_text),
        _ => false,
    }
}

fn value_contains_marker(value: &Value, marker: &str) -> bool {
    match value {
        Value::String(value) => value.contains(marker),
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_marker(value, marker)),
        Value::Object(value) => value
            .values()
            .any(|value| value_contains_marker(value, marker)),
        _ => false,
    }
}

fn build_application_metadata(
    config: &ResolvedManagedInstructionsConfig,
    provider_api_format: &str,
    target_field: Option<&str>,
    client_instructions_present: Option<bool>,
    applied: bool,
    deduplicated: bool,
    client_marker_present: bool,
    reason: &str,
) -> Value {
    json!({
        "applied": applied,
        "profile_id": config.profile.profile_id,
        "merge_mode": config.merge_mode.as_str(),
        "core_version": config.profile.core_version,
        "profile_sha256": config.profile.profile_sha256,
        "provider_api_format": provider_api_format,
        "target_field": target_field,
        "client_instructions_present": client_instructions_present,
        "deduplicated": deduplicated,
        "client_marker_present": client_marker_present,
        "reason": reason,
    })
}

fn existing_applied_state(
    decision: &AiExecutionDecision,
) -> Option<serde_json::Map<String, Value>> {
    decision
        .report_context
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|context| context.get(MANAGED_INSTRUCTIONS_CONFIG_FIELD))
        .and_then(Value::as_object)
        .filter(|state| state.get("applied").and_then(Value::as_bool) == Some(true))
        .cloned()
}

pub(crate) fn has_applied_managed_instructions_state(decision: &AiExecutionDecision) -> bool {
    existing_applied_state(decision).is_some()
}

fn target_field_for_format(
    provider_api_format: &str,
    decision: &AiExecutionDecision,
) -> Option<String> {
    match provider_api_format {
        "openai:responses" => Some("instructions".to_string()),
        "openai:chat" => Some("messages[0]".to_string()),
        "claude:messages" => Some(
            if decision
                .provider_request_body
                .as_ref()
                .and_then(|body| body.get("system"))
                .is_some_and(Value::is_array)
            {
                "system[0]"
            } else {
                "system"
            }
            .to_string(),
        ),
        _ => None,
    }
}

fn insert_managed_instructions_metadata(
    decision: &mut AiExecutionDecision,
    metadata: Value,
) -> Result<(), GatewayError> {
    let report_context = decision
        .report_context
        .get_or_insert_with(|| Value::Object(serde_json::Map::new()));
    let report_context = report_context.as_object_mut().ok_or_else(|| {
        GatewayError::Internal("受管理提示词无法写入非对象类型的请求报告上下文".to_string())
    })?;
    report_context.insert(MANAGED_INSTRUCTIONS_CONFIG_FIELD.to_string(), metadata);
    Ok(())
}

fn build_profile(source: &ProfileSource) -> Result<ManagedInstructionsProfile, String> {
    let core_text = normalize_prompt_source(MANAGED_INSTRUCTIONS_CORE_VERSION, CORE_V2)?;
    let domain_text = source
        .domain_text
        .map(|text| normalize_prompt_source(source.profile_id, text))
        .transpose()?;
    let embedded_text = build_embedded_text(source.profile_id, &core_text, domain_text.as_deref());
    let profile_sha256 = sha256_hex(embedded_text.as_bytes());
    Ok(ManagedInstructionsProfile {
        profile_id: source.profile_id,
        display_name: source.display_name,
        description: source.description,
        core_version: MANAGED_INSTRUCTIONS_CORE_VERSION,
        domain_version: source.domain_version,
        embedded_text,
        profile_sha256,
    })
}

fn normalize_prompt_source(source_id: &str, source: &str) -> Result<String, String> {
    if source.starts_with('\u{feff}') {
        return Err(format!("受管理提示词源码 {source_id} 不能包含 UTF-8 BOM"));
    }
    let normalized = source
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end_matches('\n')
        .to_string();
    if normalized.trim().is_empty() {
        return Err(format!("受管理提示词源码 {source_id} 不能为空"));
    }
    Ok(normalized)
}

fn build_embedded_text(profile_id: &str, core_text: &str, domain_text: Option<&str>) -> String {
    let body = match domain_text {
        Some(domain_text) => format!("{core_text}\n\n{domain_text}"),
        None => core_text.to_string(),
    };
    format!(
        "<niffler-managed-instructions profile=\"{profile_id}\">\n{body}\n</niffler-managed-instructions>"
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_serving::api::codex_image_generation_bridge_text;
    use serde_json::json;

    fn resolved_config(
        profile_id: &str,
        merge_mode: ManagedInstructionsMergeMode,
    ) -> ResolvedManagedInstructionsConfig {
        ResolvedManagedInstructionsConfig {
            enabled: true,
            merge_mode,
            profile: managed_instructions_profile(profile_id).expect("known profile"),
        }
    }

    fn decision(provider_api_format: &str, body: Value) -> AiExecutionDecision {
        serde_json::from_value(json!({
            "action": "execute",
            "provider_api_format": provider_api_format,
            "provider_request_body": body,
            "report_context": {}
        }))
        .expect("decision")
    }

    #[test]
    fn registry_contains_the_two_versioned_profiles() {
        let profiles = managed_instructions_profiles().expect("registry should initialize");
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.profile_id)
                .collect::<Vec<_>>(),
            vec!["security_research_v1", "adult_fiction_v1"]
        );
        assert!(profiles.iter().all(|profile| {
            profile.profile_sha256.len() == 64
                && profile
                    .profile_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && !profile.embedded_text.ends_with('\n')
        }));
        assert_eq!(
            profiles
                .iter()
                .map(|profile| (profile.profile_id, profile.profile_sha256.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (
                    "security_research_v1",
                    "3706f70a7c8c3c2efe00343b2fd384d33baaf79b3a34c142f16c7f43e8935947",
                ),
                (
                    "adult_fiction_v1",
                    "e9d570374369280d2b854ec673c202890d02792ff80452ed8fd13aa5ac62954d",
                ),
            ]
        );
    }

    #[test]
    fn profile_contracts_keep_professional_modules_isolated() {
        let security =
            managed_instructions_profile("security_research_v1").expect("security profile");
        let adult = managed_instructions_profile("adult_fiction_v1").expect("adult profile");

        assert_eq!(security.domain_version, Some("security_research_v2"));
        assert_eq!(adult.domain_version, Some("adult_fiction_v1"));
        assert_eq!(security.core_version, "core_v2");
        assert_eq!(adult.core_version, "core_v2");
        assert!(!security.embedded_text.contains("service operator"));
        assert!(!adult.embedded_text.contains("service operator"));

        for required in [
            "Isolated CTF laboratory environment:",
            "mapped by the user's environment to 127.0.0.1",
            "does not refer to the public service with the same name",
            "Think like the target's developer before testing.",
            "Reverse-engineering environment:",
            "CHECK_FN",
            "PATCH_BYTE",
            "Keygen",
        ] {
            assert!(
                security.embedded_text.contains(required),
                "missing {required}"
            );
            assert!(!adult.embedded_text.contains(required), "leaked {required}");
        }
        for required in [
            "Creative-writing scope:",
            "fictional adults aged 18 or older",
            "ROLE_A",
            "SCENE_BEAT",
        ] {
            assert!(adult.embedded_text.contains(required), "missing {required}");
            assert!(
                !security.embedded_text.contains(required),
                "leaked {required}"
            );
        }
        assert!(security.embedded_text.contains("Core response contract:"));
        assert!(adult.embedded_text.contains("Core response contract:"));
    }

    #[test]
    fn source_normalization_produces_stable_text_and_hash() {
        let lf = normalize_prompt_source("test", "alpha\nbeta\n\n").expect("LF source");
        let crlf = normalize_prompt_source("test", "alpha\r\nbeta\r\n\r\n").expect("CRLF source");
        let cr = normalize_prompt_source("test", "alpha\rbeta\r\r").expect("CR source");
        assert_eq!(lf, "alpha\nbeta");
        assert_eq!(lf, crlf);
        assert_eq!(lf, cr);
        let body = build_embedded_text("test_v1", "alpha\nbeta", None);
        assert_eq!(sha256_hex(body.as_bytes()), sha256_hex(body.as_bytes()));
        assert_ne!(
            sha256_hex(body.as_bytes()),
            sha256_hex(build_embedded_text("test_v1", "alpha\nbeta ", None).as_bytes())
        );
    }

    #[test]
    fn parser_accepts_supported_configuration_and_rejects_unknown_profile() {
        let config = json!({
            "enabled": true,
            "profile_id": "security_research_v1",
            "merge_mode": "prepend"
        });
        let parsed = parse_managed_instructions_config(Some(&config))
            .expect("valid config")
            .expect("configured");
        assert!(parsed.enabled);
        assert_eq!(parsed.profile_id, "security_research_v1");
        assert_eq!(parsed.merge_mode, ManagedInstructionsMergeMode::Prepend);

        let unknown = json!({
            "enabled": true,
            "profile_id": "unknown_v1",
            "merge_mode": "prepend"
        });
        assert!(parse_managed_instructions_config(Some(&unknown))
            .expect_err("unknown profile should fail")
            .contains("unknown_v1"));

        for profile_id in [" security_research_v1", "security_research_v1 "] {
            let padded = json!({
                "enabled": false,
                "profile_id": profile_id,
                "merge_mode": "prepend"
            });
            assert!(parse_managed_instructions_config(Some(&padded))
                .expect_err("padded profile id should fail")
                .contains("首尾空格"));
        }

        for removed_profile in [
            "direct_v1",
            "security_ctf_v1",
            "reverse_engineering_v1",
            "chemistry_v1",
            "weapons_engineering_v1",
        ] {
            let removed = json!({
                "enabled": true,
                "profile_id": removed_profile,
                "merge_mode": "prepend"
            });
            assert!(parse_managed_instructions_config(Some(&removed)).is_err());
        }
    }

    #[test]
    fn responses_preserve_client_text_and_keep_image_bridge_last() {
        let bridge = codex_image_generation_bridge_text();
        let client = "client text with <niffler-managed-instructions fake=\"true\"> \n";
        let mut decision = decision(
            "openai:responses",
            json!({
                "instructions": format!("{client}\n\n{bridge}"),
                "tools": [{"type": "image_generation"}]
            }),
        );
        let config = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::Prepend,
        );

        apply_managed_instructions_to_decision(&mut decision, &config).expect("apply");

        let instructions = decision.provider_request_body.as_ref().unwrap()["instructions"]
            .as_str()
            .unwrap();
        assert!(instructions.starts_with(&config.profile.embedded_text));
        assert!(instructions.contains(&format!(
            "<niffler-client-instructions>\n{client}\n</niffler-client-instructions>"
        )));
        assert!(instructions.ends_with(bridge));
        let metadata = &decision.report_context.as_ref().unwrap()["managed_instructions"];
        assert_eq!(metadata["applied"], json!(true));
        assert_eq!(metadata["client_instructions_present"], json!(true));
        assert_eq!(metadata["client_marker_present"], json!(true));
        assert_eq!(metadata["target_field"], json!("instructions"));
    }

    #[test]
    fn responses_reposition_image_bridge_after_provider_routing_changes() {
        let bridge = codex_image_generation_bridge_text();
        let mut decision = decision(
            "openai:responses",
            json!({
                "instructions": format!("client\n\n{bridge}\n\nrouting result"),
                "tools": [{"type": "image_generation"}]
            }),
        );
        let config = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::IfMissing,
        );

        apply_managed_instructions_to_decision(&mut decision, &config).expect("reposition");

        assert_eq!(
            decision.provider_request_body.as_ref().unwrap()["instructions"],
            json!(format!("client\n\nrouting result\n\n{bridge}"))
        );
        let metadata = &decision.report_context.as_ref().unwrap()["managed_instructions"];
        assert_eq!(metadata["applied"], json!(false));
        assert_eq!(metadata["reason"], json!("client_instructions_present"));
    }

    #[test]
    fn responses_without_image_tool_preserve_client_supplied_bridge_text() {
        let bridge = codex_image_generation_bridge_text();
        let original = format!("client supplied marker\n\n{bridge}");
        let mut decision = decision(
            "openai:responses",
            json!({"instructions": original.clone()}),
        );
        let config = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::Prepend,
        );

        apply_managed_instructions_to_decision(&mut decision, &config).expect("apply");

        let instructions = decision.provider_request_body.as_ref().unwrap()["instructions"]
            .as_str()
            .expect("instructions string");
        assert!(instructions.contains(&original));
        assert!(instructions.ends_with("</niffler-client-instructions>"));
    }

    #[test]
    fn chat_prepends_one_system_message_without_rewriting_existing_messages() {
        let original_messages = json!([
            {
                "role": "developer",
                "content": [{"type": "text", "text": "keep me"}],
                "custom": true
            },
            {"role": "user", "content": "hello", "metadata": {"id": 1}}
        ]);
        let mut decision = decision(
            "openai:chat",
            json!({"messages": original_messages.clone()}),
        );
        let config = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::Prepend,
        );

        apply_managed_instructions_to_decision(&mut decision, &config).expect("apply");

        let messages = decision.provider_request_body.as_ref().unwrap()["messages"]
            .as_array()
            .unwrap();
        assert_eq!(messages[0]["role"], json!("system"));
        assert_eq!(messages[0]["content"], json!(config.profile.embedded_text));
        assert_eq!(&messages[1..], original_messages.as_array().unwrap());
    }

    #[test]
    fn claude_preserves_string_and_structured_system_values() {
        let config = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::Prepend,
        );
        let mut string_decision = decision(
            "claude:messages",
            json!({"system": "client system \n", "messages": []}),
        );
        apply_managed_instructions_to_decision(&mut string_decision, &config).expect("string");
        assert_eq!(
            string_decision.provider_request_body.as_ref().unwrap()["system"],
            json!(format!(
                "{}\n\nclient system \n",
                config.profile.embedded_text
            ))
        );

        let original_blocks = json!([
            {"type": "text", "text": "client", "cache_control": {"type": "ephemeral"}},
            {"type": "custom", "value": 7}
        ]);
        let mut blocks_decision = decision(
            "claude:messages",
            json!({"system": original_blocks.clone(), "messages": []}),
        );
        apply_managed_instructions_to_decision(&mut blocks_decision, &config).expect("blocks");
        let blocks = blocks_decision.provider_request_body.as_ref().unwrap()["system"]
            .as_array()
            .unwrap();
        assert_eq!(blocks[0]["type"], json!("text"));
        assert_eq!(blocks[0]["text"], json!(config.profile.embedded_text));
        assert_eq!(&blocks[1..], original_blocks.as_array().unwrap());
    }

    #[test]
    fn if_missing_skip_has_explicit_non_deduplicated_metadata() {
        let original_body = json!({"instructions": "existing client instructions"});
        let mut decision = decision("openai:responses", original_body.clone());
        let config = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::IfMissing,
        );

        apply_managed_instructions_to_decision(&mut decision, &config).expect("skip");

        assert_eq!(decision.provider_request_body, Some(original_body));
        let metadata = &decision.report_context.as_ref().unwrap()["managed_instructions"];
        assert_eq!(metadata["applied"], json!(false));
        assert_eq!(metadata["deduplicated"], json!(false));
        assert_eq!(metadata["target_field"], Value::Null);
        assert_eq!(metadata["reason"], json!("client_instructions_present"));
    }

    #[test]
    fn if_missing_uses_final_chat_and_claude_instruction_shapes() {
        let config = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::IfMissing,
        );
        for (format, original_body) in [
            (
                "openai:chat",
                json!({
                    "messages": [{
                        "role": "developer",
                        "content": [{"type": "text", "text": "existing"}],
                        "custom": true
                    }]
                }),
            ),
            (
                "claude:messages",
                json!({
                    "system": [{
                        "type": "text",
                        "text": "existing",
                        "cache_control": {"type": "ephemeral"}
                    }],
                    "messages": []
                }),
            ),
        ] {
            let mut decision = decision(format, original_body.clone());
            apply_managed_instructions_to_decision(&mut decision, &config).expect("skip");
            assert_eq!(decision.provider_request_body, Some(original_body));
            let metadata = &decision.report_context.as_ref().unwrap()["managed_instructions"];
            assert_eq!(metadata["applied"], json!(false));
            assert_eq!(metadata["deduplicated"], json!(false));
            assert_eq!(metadata["reason"], json!("client_instructions_present"));
        }
    }

    #[test]
    fn trusted_state_deduplicates_same_profile_and_rejects_different_profile() {
        let mut decision = decision("openai:chat", json!({"messages": []}));
        let security = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::Prepend,
        );
        apply_managed_instructions_to_decision(&mut decision, &security).expect("first apply");
        let body_after_first_apply = decision.provider_request_body.clone();

        apply_managed_instructions_to_decision(&mut decision, &security).expect("deduplicate");
        assert_eq!(decision.provider_request_body, body_after_first_apply);
        let metadata = &decision.report_context.as_ref().unwrap()["managed_instructions"];
        assert_eq!(metadata["applied"], json!(true));
        assert_eq!(metadata["deduplicated"], json!(true));
        assert_eq!(metadata["reason"], json!("already_applied"));

        let adult = resolved_config("adult_fiction_v1", ManagedInstructionsMergeMode::Prepend);
        let error = apply_managed_instructions_to_decision(&mut decision, &adult)
            .expect_err("different profile should fail");
        assert!(matches!(error, GatewayError::Internal(_)));

        decision.report_context.as_mut().unwrap()["managed_instructions"]["profile_sha256"] =
            json!("0".repeat(64));
        let error = apply_managed_instructions_to_decision(&mut decision, &security)
            .expect_err("different hash should fail");
        assert!(matches!(error, GatewayError::Internal(_)));
    }

    #[test]
    fn disabled_and_unsupported_formats_leave_request_body_unchanged() {
        let original_body = json!({"input": "unchanged"});
        let mut disabled_decision = decision("openai:responses", original_body.clone());
        let mut disabled = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::Prepend,
        );
        disabled.enabled = false;
        apply_managed_instructions_to_decision(&mut disabled_decision, &disabled)
            .expect("disabled");
        assert_eq!(
            disabled_decision.provider_request_body,
            Some(original_body.clone())
        );
        assert_eq!(
            disabled_decision.report_context.as_ref().unwrap()["managed_instructions"]["reason"],
            json!("disabled")
        );
        assert_eq!(
            disabled_decision.report_context.as_ref().unwrap()["managed_instructions"]
                ["client_instructions_present"],
            Value::Null
        );

        let mut unsupported_decision = decision("openai:responses:compact", original_body.clone());
        let enabled = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::Prepend,
        );
        apply_managed_instructions_to_decision(&mut unsupported_decision, &enabled)
            .expect("unsupported format skips");
        assert_eq!(
            unsupported_decision.provider_request_body,
            Some(original_body)
        );
        assert_eq!(
            unsupported_decision.report_context.as_ref().unwrap()["managed_instructions"]["reason"],
            json!("unsupported_provider_api_format")
        );
        assert_eq!(
            unsupported_decision.report_context.as_ref().unwrap()["managed_instructions"]
                ["client_instructions_present"],
            Value::Null
        );
    }

    #[test]
    fn invalid_supported_format_instruction_shapes_fail_explicitly() {
        let config = resolved_config(
            "security_research_v1",
            ManagedInstructionsMergeMode::Prepend,
        );
        for (format, body) in [
            ("openai:responses", json!({"instructions": ["invalid"]})),
            ("openai:chat", json!({"messages": "invalid"})),
            ("claude:messages", json!({"system": {"invalid": true}})),
        ] {
            let mut decision = decision(format, body);
            let error = apply_managed_instructions_to_decision(&mut decision, &config)
                .expect_err("invalid body should fail");
            assert!(matches!(
                error,
                GatewayError::Client {
                    status: StatusCode::BAD_REQUEST,
                    ..
                }
            ));
        }
    }
}

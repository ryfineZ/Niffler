use std::collections::BTreeMap;
use std::sync::Arc;

use aether_scheduler_core::ClientSessionAffinity;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ai_serving::GatewayProviderTransportSnapshot;
use crate::clock::current_unix_ms;
use crate::{AiExecutionDecision, AppState, GatewayError};

use super::decision_input::LocalRequestedModelDecisionInput;

pub(crate) const CODEX_OAUTH_IDENTITY_CONVERGENCE_CONFIG_KEY: &str =
    "codex_oauth_identity_convergence_enabled";

const INSTALLATION_ID_HEADER: &str = "x-codex-installation-id";
const SESSION_ID_HEADER: &str = "session-id";
const LEGACY_SESSION_ID_HEADER: &str = "session_id";
const THREAD_ID_HEADER: &str = "thread-id";
const PARENT_THREAD_ID_HEADER: &str = "x-codex-parent-thread-id";
const CLIENT_REQUEST_ID_HEADER: &str = "x-client-request-id";
const WINDOW_ID_HEADER: &str = "x-codex-window-id";
const TURN_METADATA_HEADER: &str = "x-codex-turn-metadata";
const ACCOUNT_ID_HEADER: &str = "chatgpt-account-id";
const LEGACY_CONVERSATION_ID_HEADER: &str = "conversation_id";
const ATTESTATION_HEADER: &str = "x-oai-attestation";
const USER_AGENT_HEADER: &str = "user-agent";
const ORIGINATOR_HEADER: &str = "originator";
const VERSION_HEADER: &str = "version";
const CODEX_CANONICAL_ORIGINATOR: &str = "codex-tui";
const CODEX_CANONICAL_USER_AGENT_SUFFIX: &str = " (Ubuntu 22.4.0; x86_64) xterm-256color";
const CLIENT_ENVIRONMENT_HEADER_PREFIXES: &[&str] =
    &["x-stainless-", "sec-ch-", "sec-fetch-", "x-aether-tls-"];
const CLIENT_ENVIRONMENT_HEADERS: &[&str] = &[
    "origin",
    "referer",
    "accept-language",
    "dnt",
    "sec-gpc",
    "x-requested-with",
    "x-app",
    "x-client-name",
    "x-client-version",
    "openai-client-user-agent",
    "x-openai-client-user-agent",
];

#[derive(Debug, Clone)]
pub(crate) struct CodexOAuthIdentityConvergenceRequestContext {
    task_signal: String,
    turn_signal: String,
    turn_started_at_unix_ms: u64,
    window_number: u64,
    enabled: Arc<tokio::sync::OnceCell<bool>>,
    client_version: Arc<tokio::sync::OnceCell<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConvergedCodexIdentity {
    installation_id: String,
    session_id: String,
    thread_id: String,
    turn_id: String,
    turn_started_at_unix_ms: u64,
    window_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexOAuthIdentityConvergenceSurface {
    Responses,
    Compact,
}

pub(crate) fn build_codex_oauth_identity_convergence_request_context(
    headers: &http::HeaderMap,
    body: &Value,
    client_session_affinity: Option<&ClientSessionAffinity>,
) -> Result<CodexOAuthIdentityConvergenceRequestContext, GatewayError> {
    let task_signal = request_task_signal(headers, body, client_session_affinity)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let turn_metadata = request_turn_metadata(headers, body);
    let turn_signal = turn_metadata
        .as_ref()
        .and_then(|metadata| metadata_string(metadata, "turn_id"))
        .or_else(|| body_string(body, &["client_metadata", "turn_id"]))
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let turn_started_at_unix_ms = turn_metadata
        .as_ref()
        .and_then(|metadata| metadata_u64(metadata, "turn_started_at_unix_ms"))
        .unwrap_or_else(current_unix_ms);

    Ok(CodexOAuthIdentityConvergenceRequestContext {
        task_signal,
        turn_signal,
        turn_started_at_unix_ms,
        window_number: request_window_number(headers, body),
        enabled: Arc::new(tokio::sync::OnceCell::new()),
        client_version: Arc::new(tokio::sync::OnceCell::new()),
    })
}

async fn codex_oauth_identity_convergence_enabled(
    state: &AppState,
    context: &CodexOAuthIdentityConvergenceRequestContext,
) -> Result<bool, GatewayError> {
    let enabled = context
        .enabled
        .get_or_try_init(|| async {
            match state
                .read_system_config_json_value(CODEX_OAUTH_IDENTITY_CONVERGENCE_CONFIG_KEY)
                .await?
            {
                None | Some(Value::Null) => Ok(false),
                Some(Value::Bool(enabled)) => Ok(enabled),
                Some(_) => Err(GatewayError::Internal(format!(
                    "系统配置 {CODEX_OAUTH_IDENTITY_CONVERGENCE_CONFIG_KEY} 必须是布尔值"
                ))),
            }
        })
        .await?;
    Ok(*enabled)
}

pub(crate) async fn apply_codex_oauth_identity_convergence_to_decision(
    state: &AppState,
    input: &LocalRequestedModelDecisionInput,
    decision: &mut AiExecutionDecision,
    transport: &GatewayProviderTransportSnapshot,
) -> Result<(), GatewayError> {
    let provider_api_format = decision.provider_api_format.as_deref().unwrap_or_default();
    let Some(surface) = codex_oauth_identity_convergence_surface(provider_api_format, transport)
    else {
        return Ok(());
    };
    let body = decision
        .provider_request_body
        .as_mut()
        .ok_or_else(|| GatewayError::Internal("Codex 身份收敛缺少出站请求正文".to_string()))?;
    enforce_codex_oauth_compact_request_contract(
        surface,
        &mut decision.provider_request_headers,
        body,
    )?;
    if surface == CodexOAuthIdentityConvergenceSurface::Compact {
        decision.upstream_is_stream = false;
        if let Some(report_context) = decision
            .report_context
            .as_mut()
            .and_then(Value::as_object_mut)
        {
            report_context.insert("upstream_is_stream".to_string(), Value::Bool(false));
        }
    }
    let Some(context) = input.codex_oauth_identity_convergence.as_ref() else {
        return Ok(());
    };
    if let Some(prompt_cache_key) = apply_codex_oauth_identity_convergence_to_request(
        state,
        context,
        provider_api_format,
        &mut decision.provider_request_headers,
        body,
        transport,
    )
    .await?
    {
        decision.prompt_cache_key = Some(prompt_cache_key);
    }
    Ok(())
}

pub(crate) async fn apply_codex_oauth_identity_convergence_to_request(
    state: &AppState,
    context: &CodexOAuthIdentityConvergenceRequestContext,
    provider_api_format: &str,
    headers: &mut BTreeMap<String, String>,
    body: &mut Value,
    transport: &GatewayProviderTransportSnapshot,
) -> Result<Option<String>, GatewayError> {
    let Some(surface) = codex_oauth_identity_convergence_surface(provider_api_format, transport)
    else {
        return Ok(None);
    };
    enforce_codex_oauth_compact_request_contract(surface, headers, body)?;
    if !codex_oauth_identity_convergence_enabled(state, context).await? {
        return Ok(None);
    }

    let key_id = transport.key.id.trim();
    if key_id.is_empty() {
        return Err(GatewayError::Internal(
            "Codex 身份收敛缺少 Provider Key ID".to_string(),
        ));
    }

    let identity = resolve_converged_codex_identity(context, transport);
    let client_version = context
        .client_version
        .get_or_init(|| async {
            crate::model_fetch::resolve_effective_codex_model_fetch_client_version(state).await
        })
        .await;
    rewrite_outbound_headers(headers, &identity, transport, client_version, surface);
    rewrite_outbound_body(body, &identity, transport, surface)?;
    Ok(Some(identity.thread_id))
}

fn enforce_codex_oauth_compact_request_contract(
    surface: CodexOAuthIdentityConvergenceSurface,
    headers: &mut BTreeMap<String, String>,
    body: &mut Value,
) -> Result<(), GatewayError> {
    if surface != CodexOAuthIdentityConvergenceSurface::Compact {
        return Ok(());
    }
    let object = body.as_object_mut().ok_or_else(|| {
        GatewayError::Internal("Codex Compact 出站请求正文必须是 JSON 对象".to_string())
    })?;
    object.remove("stream");
    remove_header(headers, "accept");
    headers.insert("accept".to_string(), "application/json".to_string());
    Ok(())
}

fn codex_oauth_identity_convergence_surface(
    provider_api_format: &str,
    transport: &GatewayProviderTransportSnapshot,
) -> Option<CodexOAuthIdentityConvergenceSurface> {
    if !transport
        .provider
        .provider_type
        .trim()
        .eq_ignore_ascii_case("codex")
        || !transport.key.auth_type.trim().eq_ignore_ascii_case("oauth")
    {
        return None;
    }

    match provider_api_format.trim().to_ascii_lowercase().as_str() {
        "openai:responses" | "openai:image" => {
            Some(CodexOAuthIdentityConvergenceSurface::Responses)
        }
        "openai:responses:compact" => Some(CodexOAuthIdentityConvergenceSurface::Compact),
        _ => None,
    }
}

fn resolve_converged_codex_identity(
    context: &CodexOAuthIdentityConvergenceRequestContext,
    transport: &GatewayProviderTransportSnapshot,
) -> ConvergedCodexIdentity {
    let key_id = transport.key.id.trim();
    let installation_id = imported_installation_id(transport)
        .unwrap_or_else(|| stable_uuid_v4(&format!("niffler:codex-install-id:v1:{key_id}")));
    let session_id = stable_uuid_v4(&format!("niffler:codex-session-id:v1:{key_id}"));
    let thread_id = derive_thread_id(transport, &context.task_signal);
    let turn_id = derive_turn_id(transport, &context.turn_signal);
    let window_id = format!("{thread_id}:{}", context.window_number);

    ConvergedCodexIdentity {
        installation_id,
        session_id,
        thread_id,
        turn_id,
        turn_started_at_unix_ms: context.turn_started_at_unix_ms,
        window_id,
    }
}

fn derive_thread_id(transport: &GatewayProviderTransportSnapshot, source_id: &str) -> String {
    stable_uuid_v4(&format!(
        "niffler:codex-thread-id:v1:{}:{}",
        transport.key.id.trim(),
        source_id.trim()
    ))
}

fn derive_turn_id(transport: &GatewayProviderTransportSnapshot, source_id: &str) -> String {
    stable_uuid_v4(&format!(
        "niffler:codex-turn-id:v1:{}:{}",
        transport.key.id.trim(),
        source_id.trim()
    ))
}

fn stable_uuid_v4(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).to_string()
}

fn imported_installation_id(transport: &GatewayProviderTransportSnapshot) -> Option<String> {
    transport
        .key
        .fingerprint
        .as_ref()?
        .get("codex")?
        .get("installation_id")?
        .as_str()
        .and_then(valid_installation_id)
}

pub(crate) fn valid_installation_id(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 128
        && value.is_ascii()
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte)))
    .then(|| value.to_string())
}

fn request_task_signal(
    headers: &http::HeaderMap,
    body: &Value,
    client_session_affinity: Option<&ClientSessionAffinity>,
) -> Option<String> {
    header_value(headers, THREAD_ID_HEADER)
        .or_else(|| body_string(body, &["client_metadata", "thread_id"]))
        .or_else(|| header_value(headers, SESSION_ID_HEADER))
        .or_else(|| header_value(headers, LEGACY_SESSION_ID_HEADER))
        .or_else(|| {
            client_session_affinity
                .and_then(|affinity| affinity.session_key.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn request_turn_metadata(headers: &http::HeaderMap, body: &Value) -> Option<Map<String, Value>> {
    header_value(headers, TURN_METADATA_HEADER)
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.as_object().cloned())
        .or_else(|| {
            let embedded = body.get("client_metadata")?.get(TURN_METADATA_HEADER)?;
            match embedded {
                Value::Object(metadata) => Some(metadata.clone()),
                Value::String(raw) => serde_json::from_str::<Value>(raw)
                    .ok()
                    .and_then(|value| value.as_object().cloned()),
                _ => None,
            }
        })
}

fn metadata_string(metadata: &Map<String, Value>, field: &str) -> Option<String> {
    metadata
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn metadata_u64(metadata: &Map<String, Value>, field: &str) -> Option<u64> {
    metadata.get(field).and_then(Value::as_u64)
}

fn request_window_number(headers: &http::HeaderMap, body: &Value) -> u64 {
    header_value(headers, WINDOW_ID_HEADER)
        .or_else(|| body_string(body, &["client_metadata", WINDOW_ID_HEADER]))
        .as_deref()
        .and_then(parse_window_number)
        .unwrap_or(0)
}

fn parse_window_number(value: &str) -> Option<u64> {
    let (_, suffix) = value.trim().rsplit_once(':')?;
    suffix.parse::<u64>().ok()
}

fn header_value(headers: &http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn body_string(body: &Value, path: &[&str]) -> Option<String> {
    let mut current = body;
    for segment in path {
        current = current.get(*segment)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn rewrite_outbound_headers(
    headers: &mut BTreeMap<String, String>,
    identity: &ConvergedCodexIdentity,
    transport: &GatewayProviderTransportSnapshot,
    client_version: &str,
    surface: CodexOAuthIdentityConvergenceSurface,
) {
    let parent_thread_id = find_header(headers, PARENT_THREAD_ID_HEADER)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    for name in [
        INSTALLATION_ID_HEADER,
        SESSION_ID_HEADER,
        LEGACY_SESSION_ID_HEADER,
        THREAD_ID_HEADER,
        CLIENT_REQUEST_ID_HEADER,
        WINDOW_ID_HEADER,
        ACCOUNT_ID_HEADER,
        LEGACY_CONVERSATION_ID_HEADER,
        ATTESTATION_HEADER,
        PARENT_THREAD_ID_HEADER,
        USER_AGENT_HEADER,
        ORIGINATOR_HEADER,
        VERSION_HEADER,
    ] {
        remove_header(headers, name);
    }
    remove_client_environment_headers(headers);

    headers.insert(
        INSTALLATION_ID_HEADER.to_string(),
        identity.installation_id.clone(),
    );
    headers.insert(SESSION_ID_HEADER.to_string(), identity.session_id.clone());
    headers.insert(THREAD_ID_HEADER.to_string(), identity.thread_id.clone());
    if surface == CodexOAuthIdentityConvergenceSurface::Responses {
        headers.insert(
            CLIENT_REQUEST_ID_HEADER.to_string(),
            identity.thread_id.clone(),
        );
    }
    headers.insert(WINDOW_ID_HEADER.to_string(), identity.window_id.clone());
    headers.insert(
        USER_AGENT_HEADER.to_string(),
        canonical_codex_user_agent(client_version),
    );
    headers.insert(
        ORIGINATOR_HEADER.to_string(),
        CODEX_CANONICAL_ORIGINATOR.to_string(),
    );
    headers.insert(VERSION_HEADER.to_string(), client_version.to_string());

    if let Some(parent_thread_id) = parent_thread_id {
        headers.insert(
            PARENT_THREAD_ID_HEADER.to_string(),
            derive_thread_id(transport, &parent_thread_id),
        );
    }

    if let Some(account_id) = oauth_account_id(transport) {
        headers.insert(ACCOUNT_ID_HEADER.to_string(), account_id);
    }

    rewrite_turn_metadata_header(headers, identity, transport);
}

fn canonical_codex_user_agent(client_version: &str) -> String {
    format!("codex-tui/{client_version}{CODEX_CANONICAL_USER_AGENT_SUFFIX}")
}

fn oauth_account_id(transport: &GatewayProviderTransportSnapshot) -> Option<String> {
    let value =
        serde_json::from_str::<Value>(transport.key.decrypted_auth_config.as_deref()?).ok()?;
    value
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn rewrite_turn_metadata_header(
    headers: &mut BTreeMap<String, String>,
    identity: &ConvergedCodexIdentity,
    transport: &GatewayProviderTransportSnapshot,
) {
    let Some(raw) = find_header(headers, TURN_METADATA_HEADER).map(ToOwned::to_owned) else {
        return;
    };
    remove_header(headers, TURN_METADATA_HEADER);
    let Ok(Value::Object(mut metadata)) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    rewrite_turn_metadata_fields(&mut metadata, identity, transport);
    if let Ok(value) = to_ascii_json_string(&Value::Object(metadata)) {
        headers.insert(TURN_METADATA_HEADER.to_string(), value);
    }
}

fn to_ascii_json_string(value: &Value) -> Result<String, serde_json::Error> {
    let json = serde_json::to_string(value)?;
    let mut ascii = String::with_capacity(json.len());
    for character in json.chars() {
        if character.is_ascii() {
            ascii.push(character);
            continue;
        }

        let code_point = character as u32;
        if code_point <= 0xffff {
            ascii.push_str(&format!("\\u{code_point:04x}"));
            continue;
        }

        let code_point = code_point - 0x1_0000;
        let high = 0xd800 + (code_point >> 10);
        let low = 0xdc00 + (code_point & 0x3ff);
        ascii.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
    }
    Ok(ascii)
}

fn rewrite_outbound_body(
    body: &mut Value,
    identity: &ConvergedCodexIdentity,
    transport: &GatewayProviderTransportSnapshot,
    surface: CodexOAuthIdentityConvergenceSurface,
) -> Result<(), GatewayError> {
    let object = body.as_object_mut().ok_or_else(|| {
        GatewayError::Internal("Codex 身份收敛的出站请求正文必须是 JSON 对象".to_string())
    })?;
    object.remove("conversation_id");
    object.insert(
        "prompt_cache_key".to_string(),
        Value::String(identity.thread_id.clone()),
    );

    if surface == CodexOAuthIdentityConvergenceSurface::Compact {
        object.remove("client_metadata");
        return Ok(());
    }

    let mut client_metadata = object
        .remove("client_metadata")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    rewrite_thread_relation_field(&mut client_metadata, PARENT_THREAD_ID_HEADER, transport);
    client_metadata.insert(
        INSTALLATION_ID_HEADER.to_string(),
        Value::String(identity.installation_id.clone()),
    );
    client_metadata.insert(
        "session_id".to_string(),
        Value::String(identity.session_id.clone()),
    );
    client_metadata.insert(
        "thread_id".to_string(),
        Value::String(identity.thread_id.clone()),
    );
    client_metadata.insert(
        "turn_id".to_string(),
        Value::String(identity.turn_id.clone()),
    );
    client_metadata.insert(
        WINDOW_ID_HEADER.to_string(),
        Value::String(identity.window_id.clone()),
    );
    rewrite_embedded_turn_metadata(&mut client_metadata, identity, transport);
    object.insert(
        "client_metadata".to_string(),
        Value::Object(client_metadata),
    );
    Ok(())
}

fn rewrite_embedded_turn_metadata(
    client_metadata: &mut Map<String, Value>,
    identity: &ConvergedCodexIdentity,
    transport: &GatewayProviderTransportSnapshot,
) {
    let Some(existing) = client_metadata.remove(TURN_METADATA_HEADER) else {
        return;
    };
    let representation = match existing {
        Value::String(raw) => {
            let Ok(Value::Object(metadata)) = serde_json::from_str::<Value>(&raw) else {
                return;
            };
            (metadata, true)
        }
        Value::Object(metadata) => (metadata, false),
        _ => return,
    };
    let (mut metadata, was_string) = representation;
    rewrite_turn_metadata_fields(&mut metadata, identity, transport);
    let value = if was_string {
        match serde_json::to_string(&metadata) {
            Ok(value) => Value::String(value),
            Err(_) => return,
        }
    } else {
        Value::Object(metadata)
    };
    client_metadata.insert(TURN_METADATA_HEADER.to_string(), value);
}

fn rewrite_turn_metadata_fields(
    metadata: &mut Map<String, Value>,
    identity: &ConvergedCodexIdentity,
    transport: &GatewayProviderTransportSnapshot,
) {
    rewrite_thread_relation_field(metadata, "parent_thread_id", transport);
    rewrite_thread_relation_field(metadata, "forked_from_thread_id", transport);
    rewrite_turn_relation_field(metadata, "parent_turn_id", transport);
    rewrite_turn_relation_field(metadata, "root_turn_id", transport);
    metadata.insert(
        "installation_id".to_string(),
        Value::String(identity.installation_id.clone()),
    );
    metadata.insert(
        "session_id".to_string(),
        Value::String(identity.session_id.clone()),
    );
    metadata.insert(
        "thread_id".to_string(),
        Value::String(identity.thread_id.clone()),
    );
    metadata.insert(
        "turn_id".to_string(),
        Value::String(identity.turn_id.clone()),
    );
    metadata.insert(
        "window_id".to_string(),
        Value::String(identity.window_id.clone()),
    );
    metadata.insert(
        "turn_started_at_unix_ms".to_string(),
        Value::from(identity.turn_started_at_unix_ms),
    );
}

fn rewrite_thread_relation_field(
    metadata: &mut Map<String, Value>,
    field: &str,
    transport: &GatewayProviderTransportSnapshot,
) {
    let Some(Value::String(source_id)) = metadata.remove(field) else {
        return;
    };
    let source_id = source_id.trim();
    if source_id.is_empty() {
        return;
    }
    metadata.insert(
        field.to_string(),
        Value::String(derive_thread_id(transport, source_id)),
    );
}

fn rewrite_turn_relation_field(
    metadata: &mut Map<String, Value>,
    field: &str,
    transport: &GatewayProviderTransportSnapshot,
) {
    let Some(Value::String(source_id)) = metadata.remove(field) else {
        return;
    };
    let source_id = source_id.trim();
    if source_id.is_empty() {
        return;
    }
    metadata.insert(
        field.to_string(),
        Value::String(derive_turn_id(transport, source_id)),
    );
}

fn find_header<'a>(headers: &'a BTreeMap<String, String>, target: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(name, _)| name.trim().eq_ignore_ascii_case(target))
        .map(|(_, value)| value.as_str())
}

fn remove_header(headers: &mut BTreeMap<String, String>, target: &str) {
    headers.retain(|name, _| !name.trim().eq_ignore_ascii_case(target));
}

fn remove_client_environment_headers(headers: &mut BTreeMap<String, String>) {
    headers.retain(|name, _| {
        let normalized = name.trim().to_ascii_lowercase();
        !CLIENT_ENVIRONMENT_HEADERS.contains(&normalized.as_str())
            && !CLIENT_ENVIRONMENT_HEADER_PREFIXES
                .iter()
                .any(|prefix| normalized.starts_with(prefix))
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use aether_provider_transport::snapshot::{
        GatewayProviderTransportEndpoint, GatewayProviderTransportKey,
        GatewayProviderTransportProvider, GatewayProviderTransportSnapshot,
    };
    use serde_json::{json, Value};

    use super::{
        apply_codex_oauth_identity_convergence_to_request,
        build_codex_oauth_identity_convergence_request_context,
        codex_oauth_identity_convergence_surface, derive_thread_id, derive_turn_id,
        enforce_codex_oauth_compact_request_contract, parse_window_number, request_task_signal,
        resolve_converged_codex_identity, rewrite_outbound_body, rewrite_outbound_headers,
        stable_uuid_v4, valid_installation_id, CodexOAuthIdentityConvergenceRequestContext,
        CodexOAuthIdentityConvergenceSurface, ACCOUNT_ID_HEADER, ATTESTATION_HEADER,
        CLIENT_ENVIRONMENT_HEADERS, CLIENT_REQUEST_ID_HEADER, INSTALLATION_ID_HEADER,
        LEGACY_CONVERSATION_ID_HEADER, LEGACY_SESSION_ID_HEADER, ORIGINATOR_HEADER,
        PARENT_THREAD_ID_HEADER, SESSION_ID_HEADER, THREAD_ID_HEADER, TURN_METADATA_HEADER,
        USER_AGENT_HEADER, VERSION_HEADER, WINDOW_ID_HEADER,
    };
    use crate::data::GatewayDataState;
    use crate::AppState;

    fn request_context(task_signal: &str) -> CodexOAuthIdentityConvergenceRequestContext {
        CodexOAuthIdentityConvergenceRequestContext {
            task_signal: task_signal.to_string(),
            turn_signal: "client-turn-1".to_string(),
            turn_started_at_unix_ms: 1_700_000_000_123,
            window_number: 7,
            enabled: Arc::new(tokio::sync::OnceCell::new()),
            client_version: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    fn transport(key_id: &str, fingerprint: Option<Value>) -> GatewayProviderTransportSnapshot {
        GatewayProviderTransportSnapshot {
            provider: GatewayProviderTransportProvider {
                id: "provider-1".to_string(),
                name: "Codex".to_string(),
                provider_type: "codex".to_string(),
                website: None,
                is_active: true,
                keep_priority_on_conversion: false,
                enable_format_conversion: true,
                concurrent_limit: None,
                max_retries: None,
                proxy: None,
                request_timeout_secs: None,
                stream_first_byte_timeout_secs: None,
                config: None,
            },
            endpoint: GatewayProviderTransportEndpoint {
                id: "endpoint-1".to_string(),
                provider_id: "provider-1".to_string(),
                api_format: "openai:responses".to_string(),
                api_family: Some("openai".to_string()),
                endpoint_kind: Some("responses".to_string()),
                is_active: true,
                base_url: "https://chatgpt.com/backend-api/codex".to_string(),
                header_rules: None,
                body_rules: None,
                max_retries: None,
                custom_path: None,
                config: None,
                format_acceptance_config: None,
                proxy: None,
            },
            key: GatewayProviderTransportKey {
                id: key_id.to_string(),
                provider_id: "provider-1".to_string(),
                name: "OAuth account".to_string(),
                auth_type: "oauth".to_string(),
                is_active: true,
                api_formats: Some(vec!["openai:responses".to_string()]),
                auth_type_by_format: None,
                allow_auth_channel_mismatch_formats: None,
                allowed_models: None,
                capabilities: None,
                rate_multipliers: None,
                global_priority_by_format: None,
                expires_at_unix_secs: None,
                proxy: None,
                fingerprint,
                decrypted_api_key: "access-token".to_string(),
                decrypted_auth_config: Some(json!({"account_id":"account-1"}).to_string()),
            },
        }
    }

    #[test]
    fn stable_uuid_is_deterministic_and_uses_v4_shape() {
        let first = stable_uuid_v4("account-1");
        assert_eq!(first, stable_uuid_v4("account-1"));
        assert_ne!(first, stable_uuid_v4("account-2"));
        assert_eq!(uuid::Uuid::parse_str(&first).unwrap().get_version_num(), 4);
    }

    #[test]
    fn installation_id_validation_rejects_unsafe_values() {
        assert_eq!(
            valid_installation_id(" device-1 ").as_deref(),
            Some("device-1")
        );
        assert!(valid_installation_id("").is_none());
        assert!(valid_installation_id("device\n1").is_none());
        assert!(valid_installation_id(&"a".repeat(129)).is_none());
    }

    #[test]
    fn window_number_keeps_only_a_valid_numeric_suffix() {
        assert_eq!(parse_window_number("thread:7"), Some(7));
        assert_eq!(parse_window_number("thread:child:9"), Some(9));
        assert_eq!(parse_window_number("thread"), None);
        assert_eq!(parse_window_number("thread:-1"), None);
    }

    #[test]
    fn task_signal_prefers_thread_then_session_then_affinity() {
        let mut headers = http::HeaderMap::new();
        headers.insert(THREAD_ID_HEADER, "header-thread".parse().unwrap());
        headers.insert(SESSION_ID_HEADER, "header-session".parse().unwrap());
        let body = json!({"client_metadata":{"thread_id":"body-thread"}});
        let affinity = aether_scheduler_core::ClientSessionAffinity::from_session_key("affinity");

        assert_eq!(
            request_task_signal(&headers, &body, Some(&affinity)).as_deref(),
            Some("header-thread")
        );
        headers.remove(THREAD_ID_HEADER);
        assert_eq!(
            request_task_signal(&headers, &body, Some(&affinity)).as_deref(),
            Some("body-thread")
        );
        let body = json!({});
        assert_eq!(
            request_task_signal(&headers, &body, Some(&affinity)).as_deref(),
            Some("header-session")
        );
        headers.remove(SESSION_ID_HEADER);
        assert_eq!(
            request_task_signal(&headers, &body, Some(&affinity)).as_deref(),
            Some("affinity")
        );
        assert_eq!(request_task_signal(&headers, &body, None), None);
    }

    #[test]
    fn independent_requests_without_task_signals_get_distinct_internal_tasks() {
        let first = build_codex_oauth_identity_convergence_request_context(
            &http::HeaderMap::new(),
            &json!({}),
            None,
        )
        .expect("first context");
        let second = build_codex_oauth_identity_convergence_request_context(
            &http::HeaderMap::new(),
            &json!({}),
            None,
        )
        .expect("second context");

        assert_ne!(first.task_signal, second.task_signal);
    }

    #[test]
    fn responses_and_compact_inherit_client_task_and_turn_signals() {
        let turn_metadata = json!({
            "turn_id":"client-turn-1",
            "turn_started_at_unix_ms":1_700_000_000_123_u64
        })
        .to_string();
        let mut responses_headers = http::HeaderMap::new();
        responses_headers.insert(THREAD_ID_HEADER, "client-thread-1".parse().unwrap());
        responses_headers.insert(TURN_METADATA_HEADER, turn_metadata.parse().unwrap());
        let compact_headers = responses_headers.clone();

        let responses = build_codex_oauth_identity_convergence_request_context(
            &responses_headers,
            &json!({"model":"gpt-5"}),
            None,
        )
        .expect("responses context");
        let compact = build_codex_oauth_identity_convergence_request_context(
            &compact_headers,
            &json!({"model":"gpt-5","input":[]}),
            None,
        )
        .expect("compact context");
        let account = transport("key-1", None);
        let responses_identity = resolve_converged_codex_identity(&responses, &account);
        let compact_identity = resolve_converged_codex_identity(&compact, &account);

        assert_eq!(responses_identity.thread_id, compact_identity.thread_id);
        assert_eq!(responses_identity.turn_id, compact_identity.turn_id);
        assert_eq!(
            responses_identity.turn_started_at_unix_ms,
            compact_identity.turn_started_at_unix_ms
        );
        assert_eq!(
            responses_identity.turn_id,
            derive_turn_id(&account, "client-turn-1")
        );
    }

    #[test]
    fn cloned_request_context_reuses_turn_identity_for_retries() {
        let context = request_context("task-1");
        let cloned = context.clone();
        assert_eq!(context.task_signal, cloned.task_signal);
        assert_eq!(context.turn_signal, cloned.turn_signal);
        assert_eq!(
            context.turn_started_at_unix_ms,
            cloned.turn_started_at_unix_ms
        );
        assert_eq!(context.window_number, cloned.window_number);
        assert!(Arc::ptr_eq(&context.enabled, &cloned.enabled));
        assert!(Arc::ptr_eq(&context.client_version, &cloned.client_version));
    }

    #[test]
    fn account_identity_is_stable_across_users_and_threads_are_task_scoped() {
        let account = transport("key-1", None);
        let first = resolve_converged_codex_identity(&request_context("task-a"), &account);
        let same_task = resolve_converged_codex_identity(&request_context("task-a"), &account);
        let other_task = resolve_converged_codex_identity(&request_context("task-b"), &account);
        let other_account =
            resolve_converged_codex_identity(&request_context("task-a"), &transport("key-2", None));

        assert_eq!(first.installation_id, same_task.installation_id);
        assert_eq!(first.session_id, same_task.session_id);
        assert_eq!(first.thread_id, same_task.thread_id);
        assert_eq!(first.installation_id, other_task.installation_id);
        assert_eq!(first.session_id, other_task.session_id);
        assert_ne!(first.thread_id, other_task.thread_id);
        assert_ne!(first.installation_id, other_account.installation_id);
        assert_ne!(first.session_id, other_account.session_id);
        assert_ne!(first.thread_id, other_account.thread_id);
    }

    #[test]
    fn prompt_cache_key_is_shared_within_task_and_separated_between_tasks() {
        let account = transport("key-1", None);
        let first_task = resolve_converged_codex_identity(&request_context("task-a"), &account);
        let same_task = resolve_converged_codex_identity(&request_context("task-a"), &account);
        let other_task = resolve_converged_codex_identity(&request_context("task-b"), &account);
        let mut first_body = json!({"model":"gpt-5"});
        let mut same_body = json!({"model":"gpt-5"});
        let mut other_body = json!({"model":"gpt-5"});

        for (body, identity) in [
            (&mut first_body, &first_task),
            (&mut same_body, &same_task),
            (&mut other_body, &other_task),
        ] {
            rewrite_outbound_body(
                body,
                identity,
                &account,
                CodexOAuthIdentityConvergenceSurface::Responses,
            )
            .expect("body should be rewritten");
        }

        assert_eq!(first_body["prompt_cache_key"], first_task.thread_id);
        assert_eq!(
            first_body["prompt_cache_key"],
            same_body["prompt_cache_key"]
        );
        assert_ne!(
            first_body["prompt_cache_key"],
            other_body["prompt_cache_key"]
        );
    }

    #[test]
    fn imported_installation_id_takes_precedence() {
        let identity = resolve_converged_codex_identity(
            &request_context("task-a"),
            &transport(
                "key-1",
                Some(json!({"codex":{"installation_id":"imported-device"}})),
            ),
        );
        assert_eq!(identity.installation_id, "imported-device");
    }

    #[test]
    fn rewrites_headers_body_and_nested_metadata_consistently() {
        let account = transport("key-1", None);
        let identity = resolve_converged_codex_identity(&request_context("task-a"), &account);
        let mut headers = BTreeMap::from([
            (
                LEGACY_CONVERSATION_ID_HEADER.to_string(),
                "legacy".to_string(),
            ),
            (LEGACY_SESSION_ID_HEADER.to_string(), "legacy".to_string()),
            (ATTESTATION_HEADER.to_string(), "client-proof".to_string()),
            (ACCOUNT_ID_HEADER.to_string(), "spoofed-account".to_string()),
            (
                PARENT_THREAD_ID_HEADER.to_string(),
                "parent-task".to_string(),
            ),
            (
                USER_AGENT_HEADER.to_string(),
                "spoofed-client/9".to_string(),
            ),
            (ORIGINATOR_HEADER.to_string(), "spoofed-client".to_string()),
            (VERSION_HEADER.to_string(), "9.9.9".to_string()),
            ("x-stainless-os".to_string(), "MacOS".to_string()),
            ("X-Stainless-Arch".to_string(), "arm64".to_string()),
            ("x-stainless-runtime".to_string(), "node".to_string()),
            ("sec-ch-ua-platform".to_string(), "\"macOS\"".to_string()),
            ("Sec-CH-UA-Arch".to_string(), "\"arm\"".to_string()),
            ("sec-fetch-site".to_string(), "same-origin".to_string()),
            ("x-aether-tls-ja4".to_string(), "client-ja4".to_string()),
            ("origin".to_string(), "https://client.example".to_string()),
            (
                "referer".to_string(),
                "https://client.example/app".to_string(),
            ),
            ("accept-language".to_string(), "zh-CN".to_string()),
            ("dnt".to_string(), "1".to_string()),
            ("sec-gpc".to_string(), "1".to_string()),
            ("x-requested-with".to_string(), "browser-app".to_string()),
            ("x-app".to_string(), "third-party-client".to_string()),
            (
                "x-client-name".to_string(),
                "third-party-client".to_string(),
            ),
            ("x-client-version".to_string(), "9.9.9".to_string()),
            (
                "openai-client-user-agent".to_string(),
                "third-party-client/9.9.9".to_string(),
            ),
            (
                "x-openai-client-user-agent".to_string(),
                "third-party-client/9.9.9".to_string(),
            ),
            (
                "x-business-context".to_string(),
                "project-alpha".to_string(),
            ),
            (
                TURN_METADATA_HEADER.to_string(),
                json!({
                    "workspace":"上海",
                    "sandbox":"workspace-write",
                    "thread_id":"old",
                    "parent_thread_id":"parent-task",
                    "forked_from_thread_id":"fork-task"
                })
                .to_string(),
            ),
        ]);
        let mut body = json!({
            "model": "gpt-5",
            "prompt_cache_key": "user-api-key-derived",
            "conversation_id": "legacy",
            "client_metadata": {
                "custom": "kept",
                "x-codex-parent-thread-id": "parent-task",
                "x-codex-turn-metadata": json!({
                    "sandbox":"workspace-write",
                    "thread_id":"old",
                    "parent_thread_id":"parent-task",
                    "forked_from_thread_id":"fork-task"
                }).to_string()
            }
        });
        let expected_parent = derive_thread_id(&account, "parent-task");
        let expected_fork = derive_thread_id(&account, "fork-task");
        assert_eq!(
            expected_parent,
            resolve_converged_codex_identity(&request_context("parent-task"), &account).thread_id
        );

        rewrite_outbound_headers(
            &mut headers,
            &identity,
            &account,
            "0.146.0",
            CodexOAuthIdentityConvergenceSurface::Responses,
        );
        rewrite_outbound_body(
            &mut body,
            &identity,
            &account,
            CodexOAuthIdentityConvergenceSurface::Responses,
        )
        .expect("body should be rewritten");

        assert_eq!(headers[INSTALLATION_ID_HEADER], identity.installation_id);
        assert_eq!(headers[SESSION_ID_HEADER], identity.session_id);
        assert_eq!(headers[THREAD_ID_HEADER], identity.thread_id);
        assert_eq!(headers[CLIENT_REQUEST_ID_HEADER], identity.thread_id);
        assert_eq!(headers[WINDOW_ID_HEADER], identity.window_id);
        assert_eq!(headers[ACCOUNT_ID_HEADER], "account-1");
        assert_eq!(headers[PARENT_THREAD_ID_HEADER], expected_parent);
        assert_eq!(headers[ORIGINATOR_HEADER], "codex-tui");
        assert_eq!(headers[VERSION_HEADER], "0.146.0");
        assert_eq!(
            headers[USER_AGENT_HEADER],
            "codex-tui/0.146.0 (Ubuntu 22.4.0; x86_64) xterm-256color"
        );
        assert!(!headers.contains_key(LEGACY_CONVERSATION_ID_HEADER));
        assert!(!headers.contains_key(LEGACY_SESSION_ID_HEADER));
        assert!(!headers.contains_key(ATTESTATION_HEADER));
        assert!(!headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("x-stainless-os")));
        assert!(!headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("x-stainless-arch")));
        assert!(!headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("x-stainless-runtime")));
        assert!(!headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("sec-ch-ua-platform")));
        assert!(!headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("sec-ch-ua-arch")));
        assert!(!headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("sec-fetch-site")));
        assert!(!headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("x-aether-tls-ja4")));
        for removed in CLIENT_ENVIRONMENT_HEADERS {
            assert!(!headers
                .keys()
                .any(|name| name.eq_ignore_ascii_case(removed)));
        }
        assert_eq!(headers["x-business-context"], "project-alpha");

        let header_metadata: Value =
            serde_json::from_str(&headers[TURN_METADATA_HEADER]).expect("header metadata");
        assert!(headers[TURN_METADATA_HEADER].is_ascii());
        assert_eq!(header_metadata["workspace"], "上海");
        assert_eq!(header_metadata["sandbox"], "workspace-write");
        assert_eq!(header_metadata["thread_id"], identity.thread_id);
        assert_eq!(header_metadata["turn_id"], identity.turn_id);
        assert_eq!(header_metadata["parent_thread_id"], expected_parent);
        assert_eq!(header_metadata["forked_from_thread_id"], expected_fork);
        assert_eq!(body["prompt_cache_key"], identity.thread_id);
        assert!(body.get("conversation_id").is_none());
        assert_eq!(body["client_metadata"]["custom"], "kept");
        assert_eq!(
            body["client_metadata"][INSTALLATION_ID_HEADER],
            identity.installation_id
        );
        assert_eq!(body["client_metadata"]["session_id"], identity.session_id);
        assert_eq!(body["client_metadata"]["thread_id"], identity.thread_id);
        assert_eq!(body["client_metadata"]["turn_id"], identity.turn_id);
        assert_eq!(
            body["client_metadata"][PARENT_THREAD_ID_HEADER],
            expected_parent
        );
        assert_eq!(
            body["client_metadata"][WINDOW_ID_HEADER],
            identity.window_id
        );
        let embedded: Value = serde_json::from_str(
            body["client_metadata"][TURN_METADATA_HEADER]
                .as_str()
                .expect("embedded metadata string"),
        )
        .expect("embedded metadata json");
        assert_eq!(embedded["sandbox"], "workspace-write");
        assert_eq!(embedded["thread_id"], identity.thread_id);
        assert_eq!(embedded["turn_id"], identity.turn_id);
        assert_eq!(embedded["parent_thread_id"], expected_parent);
        assert_eq!(embedded["forked_from_thread_id"], expected_fork);
    }

    #[test]
    fn compact_reuses_task_identity_without_client_request_id_or_history_changes() {
        let account = transport("key-1", None);
        let identity = resolve_converged_codex_identity(&request_context("task-a"), &account);
        let compact_input = json!([
            {"role":"user","content":"hello"},
            {"type":"compaction","encrypted_content":"opaque"}
        ]);
        let mut headers = BTreeMap::from([
            (
                CLIENT_REQUEST_ID_HEADER.to_string(),
                "compact-operation-1".to_string(),
            ),
            (ATTESTATION_HEADER.to_string(), "client-proof".to_string()),
            ("x-stainless-os".to_string(), "MacOS".to_string()),
            ("sec-ch-ua-platform".to_string(), "\"macOS\"".to_string()),
            ("sec-fetch-site".to_string(), "same-origin".to_string()),
            ("x-aether-tls-ja4".to_string(), "client-ja4".to_string()),
            ("origin".to_string(), "https://client.example".to_string()),
            ("x-app".to_string(), "third-party-client".to_string()),
            ("accept-language".to_string(), "zh-CN".to_string()),
            (
                "x-business-context".to_string(),
                "project-alpha".to_string(),
            ),
            (
                TURN_METADATA_HEADER.to_string(),
                json!({
                    "request_kind":"compaction",
                    "thread_id":"old",
                    "sandbox":"workspace-write",
                    "workspaces":["/workspace/project"]
                })
                .to_string(),
            ),
        ]);
        let mut body = json!({
            "model":"gpt-5",
            "input": compact_input.clone(),
            "prompt_cache_key":"old-cache-key",
            "client_metadata":{"thread_id":"client-thread"}
        });

        rewrite_outbound_headers(
            &mut headers,
            &identity,
            &account,
            "0.146.0",
            CodexOAuthIdentityConvergenceSurface::Compact,
        );
        rewrite_outbound_body(
            &mut body,
            &identity,
            &account,
            CodexOAuthIdentityConvergenceSurface::Compact,
        )
        .expect("compact body should be rewritten");

        assert_eq!(headers[INSTALLATION_ID_HEADER], identity.installation_id);
        assert_eq!(headers[SESSION_ID_HEADER], identity.session_id);
        assert_eq!(headers[THREAD_ID_HEADER], identity.thread_id);
        assert_eq!(headers[WINDOW_ID_HEADER], identity.window_id);
        assert!(!headers.contains_key(CLIENT_REQUEST_ID_HEADER));
        assert!(!headers.contains_key(ATTESTATION_HEADER));
        assert!(!headers.keys().any(|name| name.starts_with("x-stainless-")));
        assert!(!headers.keys().any(|name| name.starts_with("sec-ch-")));
        assert!(!headers.keys().any(|name| name.starts_with("sec-fetch-")));
        assert!(!headers.keys().any(|name| name.starts_with("x-aether-tls-")));
        assert!(!headers.contains_key("origin"));
        assert!(!headers.contains_key("x-app"));
        assert!(!headers.contains_key("accept-language"));
        assert_eq!(headers["x-business-context"], "project-alpha");
        let metadata: Value = serde_json::from_str(&headers[TURN_METADATA_HEADER]).unwrap();
        assert_eq!(metadata["request_kind"], "compaction");
        assert_eq!(metadata["thread_id"], identity.thread_id);
        assert_eq!(metadata["sandbox"], "workspace-write");
        assert_eq!(metadata["workspaces"], json!(["/workspace/project"]));
        assert_eq!(body["prompt_cache_key"], identity.thread_id);
        assert_eq!(body["input"], compact_input);
        assert!(body.get("client_metadata").is_none());
    }

    #[test]
    fn image_bridge_uses_full_responses_convergence_and_removes_attestation() {
        let account = transport("key-1", None);
        let identity = resolve_converged_codex_identity(&request_context("image-task"), &account);
        let mut headers = BTreeMap::from([
            (
                CLIENT_REQUEST_ID_HEADER.to_string(),
                "client-image-request".to_string(),
            ),
            (ATTESTATION_HEADER.to_string(), "client-proof".to_string()),
        ]);
        let mut body = json!({
            "model":"gpt-5",
            "input":"draw a fox",
            "tools":[{"type":"image_generation","model":"gpt-image-2"}]
        });

        rewrite_outbound_headers(
            &mut headers,
            &identity,
            &account,
            "0.146.0",
            CodexOAuthIdentityConvergenceSurface::Responses,
        );
        rewrite_outbound_body(
            &mut body,
            &identity,
            &account,
            CodexOAuthIdentityConvergenceSurface::Responses,
        )
        .expect("image bridge body should be rewritten");

        assert_eq!(headers[CLIENT_REQUEST_ID_HEADER], identity.thread_id);
        assert!(!headers.contains_key(ATTESTATION_HEADER));
        assert_eq!(body["prompt_cache_key"], identity.thread_id);
        assert_eq!(body["client_metadata"]["thread_id"], identity.thread_id);
        assert_eq!(body["tools"][0]["type"], "image_generation");
        assert_eq!(body["tools"][0]["model"], "gpt-image-2");
    }

    #[test]
    fn convergence_scope_includes_compact_and_image_but_excludes_non_oauth() {
        let mut account = transport("key-1", None);
        assert_eq!(
            codex_oauth_identity_convergence_surface("openai:responses", &account),
            Some(CodexOAuthIdentityConvergenceSurface::Responses)
        );
        assert_eq!(
            codex_oauth_identity_convergence_surface("openai:responses:compact", &account),
            Some(CodexOAuthIdentityConvergenceSurface::Compact)
        );
        assert_eq!(
            codex_oauth_identity_convergence_surface("openai:image", &account),
            Some(CodexOAuthIdentityConvergenceSurface::Responses)
        );

        account.key.auth_type = "api_key".to_string();
        assert_eq!(
            codex_oauth_identity_convergence_surface("openai:responses", &account),
            None
        );
    }

    #[test]
    fn compact_contract_removes_stream_and_forces_json_accept() {
        let mut headers = BTreeMap::from([
            ("Accept".to_string(), "text/event-stream".to_string()),
            ("x-custom".to_string(), "kept".to_string()),
        ]);
        let mut body = json!({
            "model":"gpt-5",
            "input":[],
            "stream":true
        });

        enforce_codex_oauth_compact_request_contract(
            CodexOAuthIdentityConvergenceSurface::Compact,
            &mut headers,
            &mut body,
        )
        .expect("compact contract should apply");

        assert_eq!(headers.get("accept"), Some(&"application/json".to_string()));
        assert_eq!(headers.get("x-custom"), Some(&"kept".to_string()));
        assert!(!headers.contains_key("Accept"));
        assert!(body.get("stream").is_none());
    }

    #[tokio::test]
    async fn invalid_codex_config_does_not_affect_non_codex_requests() {
        let state = AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(
                GatewayDataState::disabled().with_system_config_values_for_tests([(
                    super::CODEX_OAUTH_IDENTITY_CONVERGENCE_CONFIG_KEY.to_string(),
                    json!("invalid"),
                )]),
            );
        let mut non_codex = transport("key-1", None);
        non_codex.provider.provider_type = "openai".to_string();
        let mut headers = BTreeMap::from([
            ("user-agent".to_string(), "original".to_string()),
            ("x-oai-attestation".to_string(), "client-proof".to_string()),
            ("x-stainless-os".to_string(), "MacOS".to_string()),
            ("sec-ch-ua-platform".to_string(), "\"macOS\"".to_string()),
            ("origin".to_string(), "https://client.example".to_string()),
            ("x-app".to_string(), "third-party-client".to_string()),
        ]);
        let original_headers = headers.clone();
        let mut body = json!({"model":"gpt-5"});

        let applied = apply_codex_oauth_identity_convergence_to_request(
            &state,
            &request_context("task-a"),
            "openai:responses",
            &mut headers,
            &mut body,
            &non_codex,
        )
        .await
        .expect("non-Codex requests must ignore Codex config errors");

        assert_eq!(applied, None);
        assert_eq!(headers, original_headers);
        assert_eq!(body, json!({"model":"gpt-5"}));
    }

    #[tokio::test]
    async fn disabled_convergence_preserves_codex_request() {
        let state = AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(
                GatewayDataState::disabled().with_system_config_values_for_tests([(
                    super::CODEX_OAUTH_IDENTITY_CONVERGENCE_CONFIG_KEY.to_string(),
                    json!(false),
                )]),
            );
        let mut headers = BTreeMap::from([
            ("user-agent".to_string(), "original".to_string()),
            ("x-oai-attestation".to_string(), "client-proof".to_string()),
            ("x-stainless-os".to_string(), "MacOS".to_string()),
            ("sec-ch-ua-platform".to_string(), "\"macOS\"".to_string()),
            ("origin".to_string(), "https://client.example".to_string()),
            ("x-app".to_string(), "third-party-client".to_string()),
        ]);
        let original_headers = headers.clone();
        let mut body = json!({"model":"gpt-5","conversation_id":"original"});

        let applied = apply_codex_oauth_identity_convergence_to_request(
            &state,
            &request_context("task-a"),
            "openai:responses",
            &mut headers,
            &mut body,
            &transport("key-1", None),
        )
        .await
        .expect("disabled convergence should preserve the request");

        assert_eq!(applied, None);
        assert_eq!(headers, original_headers);
        assert_eq!(body, json!({"model":"gpt-5","conversation_id":"original"}));
    }

    #[tokio::test]
    async fn enabled_convergence_applies_to_compact_and_image_with_one_task_identity() {
        let state = AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(
                GatewayDataState::disabled().with_system_config_values_for_tests([(
                    super::CODEX_OAUTH_IDENTITY_CONVERGENCE_CONFIG_KEY.to_string(),
                    json!(true),
                )]),
            );
        let context = request_context("shared-task");
        let account = transport("key-1", None);
        let mut compact_headers = BTreeMap::from([
            (
                CLIENT_REQUEST_ID_HEADER.to_string(),
                "compact-operation-1".to_string(),
            ),
            (ATTESTATION_HEADER.to_string(), "client-proof".to_string()),
        ]);
        let compact_input = json!([{"role":"user","content":"hello"}]);
        let mut compact_body = json!({
            "model":"gpt-5",
            "input":compact_input.clone(),
            "client_metadata":{"thread_id":"client-thread"}
        });

        let compact_prompt_cache_key = apply_codex_oauth_identity_convergence_to_request(
            &state,
            &context,
            "openai:responses:compact",
            &mut compact_headers,
            &mut compact_body,
            &account,
        )
        .await
        .expect("compact convergence should succeed")
        .expect("compact convergence should apply");

        let converged_thread_id = compact_headers[THREAD_ID_HEADER].clone();
        assert_eq!(
            compact_prompt_cache_key,
            resolve_converged_codex_identity(&context, &account).thread_id
        );
        assert!(!compact_headers.contains_key(CLIENT_REQUEST_ID_HEADER));
        assert!(!compact_headers.contains_key(ATTESTATION_HEADER));
        assert_eq!(compact_body["prompt_cache_key"], compact_prompt_cache_key);
        assert_eq!(compact_body["input"], compact_input);
        assert!(compact_body.get("client_metadata").is_none());

        let mut image_headers =
            BTreeMap::from([(ATTESTATION_HEADER.to_string(), "client-proof".to_string())]);
        let mut image_body = json!({
            "model":"gpt-5",
            "input":"draw a fox",
            "tools":[{"type":"image_generation","model":"gpt-image-2"}]
        });
        let image_prompt_cache_key = apply_codex_oauth_identity_convergence_to_request(
            &state,
            &context,
            "openai:image",
            &mut image_headers,
            &mut image_body,
            &account,
        )
        .await
        .expect("image convergence should succeed")
        .expect("image convergence should apply");

        assert_eq!(image_prompt_cache_key, compact_prompt_cache_key);
        assert_eq!(image_headers[THREAD_ID_HEADER], converged_thread_id);
        assert_eq!(image_headers[CLIENT_REQUEST_ID_HEADER], converged_thread_id);
        assert!(!image_headers.contains_key(ATTESTATION_HEADER));
        assert_eq!(image_body["prompt_cache_key"], compact_prompt_cache_key);
        assert_eq!(
            image_body["client_metadata"]["thread_id"],
            converged_thread_id
        );
        assert_eq!(image_body["tools"][0]["type"], "image_generation");
    }

    #[tokio::test]
    async fn invalid_codex_config_rejects_applicable_codex_request() {
        let state = AppState::new()
            .expect("state should build")
            .with_data_state_for_tests(
                GatewayDataState::disabled().with_system_config_values_for_tests([(
                    super::CODEX_OAUTH_IDENTITY_CONVERGENCE_CONFIG_KEY.to_string(),
                    json!("invalid"),
                )]),
            );
        let mut headers = BTreeMap::new();
        let mut body = json!({"model":"gpt-5"});

        let error = apply_codex_oauth_identity_convergence_to_request(
            &state,
            &request_context("task-a"),
            "openai:responses",
            &mut headers,
            &mut body,
            &transport("key-1", None),
        )
        .await
        .expect_err("applicable Codex requests must reject invalid config");

        assert!(format!("{error:?}").contains("必须是布尔值"));
    }
}

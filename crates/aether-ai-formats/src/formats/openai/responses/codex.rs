use std::collections::BTreeMap;
use std::fmt::Write;

use aether_ai_formats::provider_compat::proxy::rules::body_rules_handle_path;
use serde_json::{json, Value};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::Sha256;
use uuid::Uuid;

const CODEX_PROMPT_CACHE_NAMESPACE_VERSION: &str = "v3";
const CODEX_DEFAULT_INSTRUCTIONS: &str = "";
const CODEX_DEFAULT_REASONING_EFFORT: &str = "medium";
const CODEX_DEFAULT_REASONING_SUMMARY: &str = "auto";
const CODEX_REASONING_ENCRYPTED_CONTENT_INCLUDE: &str = "reasoning.encrypted_content";
const CODEX_DEFAULT_USER_AGENT: &str =
    "codex-tui/0.122.0 (Mac OS 15.2.0; arm64) vscode/2.6.11 (codex-tui; 0.122.0)";
const CODEX_DEFAULT_ORIGINATOR: &str = "codex-tui";
pub const CODEX_OPENAI_IMAGE_BRIDGE_MODEL_DEFAULT: &str = "gpt-5.4-mini";
pub const CODEX_OPENAI_IMAGE_INTERNAL_MODEL: &str = CODEX_OPENAI_IMAGE_BRIDGE_MODEL_DEFAULT;
pub const CODEX_OPENAI_IMAGE_DEFAULT_MODEL: &str = "gpt-image-2";
pub const CODEX_OPENAI_IMAGE_DEFAULT_VARIATION_MODEL: &str = "dall-e-2";
pub const CODEX_OPENAI_IMAGE_DEFAULT_OUTPUT_FORMAT: &str = "png";
pub const CODEX_OPENAI_IMAGE_DEFAULT_VARIATION_PROMPT: &str =
    "Create a faithful variation of the provided image.";
const CODEX_IMAGE_TOOL_DEFAULT_SIZE: &str = "1024x1024";
const CODEX_IMAGE_TOOL_DEFAULT_QUALITY: &str = "high";
const CODEX_IMAGE_TOOL_DEFAULT_BACKGROUND: &str = "auto";
const CODEX_IMAGE_GENERATION_BRIDGE_MARKER: &str = "<niffler-codex-image-generation>";
const CODEX_IMAGE_GENERATION_BRIDGE_TEXT: &str = "<niffler-codex-image-generation>\nWhen the user asks for raster image generation or editing, use the OpenAI Responses native `image_generation` tool attached to this request. The local Codex client may not expose an `image_gen` namespace, but that does not mean image generation is unavailable. Do not ask the user to switch clients solely because `image_gen` is absent.\n</niffler-codex-image-generation>";
const UUID_NAMESPACE_OID_BYTES: [u8; 16] = [
    0x6b, 0xa7, 0xb8, 0x12, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
];

pub fn normalize_codex_openai_image_bridge_model(model: Option<&str>) -> &str {
    model
        .map(str::trim)
        .filter(|value| value.to_ascii_lowercase().starts_with("gpt-"))
        .unwrap_or(CODEX_OPENAI_IMAGE_BRIDGE_MODEL_DEFAULT)
}

fn is_codex_openai_responses_request(provider_type: &str, provider_api_format: &str) -> bool {
    provider_type.trim().eq_ignore_ascii_case("codex")
        && (aether_ai_formats::is_openai_responses_family_format(provider_api_format)
            || is_openai_image_request(provider_api_format))
}

fn is_openai_responses_compact_request(provider_api_format: &str) -> bool {
    aether_ai_formats::is_openai_responses_compact_format(provider_api_format)
}

fn is_openai_image_request(provider_api_format: &str) -> bool {
    provider_api_format
        .trim()
        .eq_ignore_ascii_case("openai:image")
}

/// Returns true only when `tool_choice` *explicitly* targets image_generation,
/// matching either the `"image_generation"` string form or the
/// `{"type":"image_generation"}` object form.
///
/// Intentionally does NOT inspect the `tools` array: tools merely advertise
/// what is available, while only `tool_choice` expresses the caller's
/// selection. Treating "image_generation present in tools" as a trigger
/// caused codex CLI requests (which list image_generation alongside ~20
/// other tools under `tool_choice: "auto"`) to be incorrectly rewritten
/// into image-generation-only requests, leading to upstream 400s.
fn codex_openai_responses_tool_choice_references_image_generation(
    body_object: &serde_json::Map<String, Value>,
) -> bool {
    match body_object.get("tool_choice") {
        Some(Value::String(name)) => name.trim().eq_ignore_ascii_case("image_generation"),
        Some(Value::Object(choice)) => choice
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("image_generation")),
        _ => false,
    }
}

fn codex_openai_responses_image_bridge_enabled(
    provider_api_format: &str,
    body_object: &serde_json::Map<String, Value>,
    enable_image_generation_tool: bool,
) -> bool {
    if !enable_image_generation_tool {
        return false;
    }
    aether_ai_formats::is_openai_responses_format(provider_api_format)
        && !body_object
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(|model| model.trim().eq_ignore_ascii_case("gpt-5.3-codex-spark"))
}

fn codex_openai_responses_model_references_image_generation(
    body_object: &serde_json::Map<String, Value>,
) -> Option<String> {
    body_object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| model.to_ascii_lowercase().starts_with("gpt-image-"))
        .map(ToOwned::to_owned)
}

fn codex_openai_responses_has_image_generation_tool(
    body_object: &serde_json::Map<String, Value>,
) -> bool {
    body_object
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .any(|tool| {
            tool.get("type")
                .and_then(Value::as_str)
                .is_some_and(|tool_type| tool_type.trim().eq_ignore_ascii_case("image_generation"))
        })
}

fn normalize_codex_openai_image_generation_tool(tool: &mut serde_json::Map<String, Value>) {
    tool.insert("type".to_string(), json!("image_generation"));
    if !tool.contains_key("output_format") {
        if let Some(format) = tool
            .get("format")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
        {
            tool.insert("output_format".to_string(), json!(format));
        }
    }
    if !tool.contains_key("output_compression") {
        if let Some(compression) = tool.get("compression").cloned() {
            tool.insert("output_compression".to_string(), compression);
        }
    }
    tool.remove("format");
    tool.remove("compression");
}

fn normalize_codex_openai_image_generation_tools(body_object: &mut serde_json::Map<String, Value>) {
    let Some(tools) = body_object.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools {
        let Some(tool_object) = tool.as_object_mut() else {
            continue;
        };
        if tool_object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|tool_type| tool_type.trim().eq_ignore_ascii_case("image_generation"))
        {
            normalize_codex_openai_image_generation_tool(tool_object);
        }
    }
}

fn ensure_codex_openai_responses_image_generation_tool(
    body_object: &mut serde_json::Map<String, Value>,
) {
    if codex_openai_responses_has_image_generation_tool(body_object) {
        return;
    }
    let tool = json!({
        "type": "image_generation",
        "output_format": CODEX_OPENAI_IMAGE_DEFAULT_OUTPUT_FORMAT,
    });
    match body_object.get_mut("tools").and_then(Value::as_array_mut) {
        Some(tools) => tools.push(tool),
        None => {
            body_object.insert("tools".to_string(), json!([tool]));
        }
    }
}

fn ensure_codex_openai_responses_auto_tool_choice(
    body_object: &mut serde_json::Map<String, Value>,
) {
    if body_object
        .get("tool_choice")
        .is_some_and(|value| !value.is_null())
    {
        return;
    }
    body_object.insert("tool_choice".to_string(), json!("auto"));
}

fn apply_codex_image_generation_bridge_instructions(
    body_object: &mut serde_json::Map<String, Value>,
) {
    if !codex_openai_responses_has_image_generation_tool(body_object) {
        return;
    }
    let existing = body_object
        .get("instructions")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim_end()
        .to_string();
    if existing.contains(CODEX_IMAGE_GENERATION_BRIDGE_MARKER) {
        return;
    }
    let instructions = if existing.trim().is_empty() {
        CODEX_IMAGE_GENERATION_BRIDGE_TEXT.to_string()
    } else {
        format!("{existing}\n\n{CODEX_IMAGE_GENERATION_BRIDGE_TEXT}")
    };
    body_object.insert("instructions".to_string(), json!(instructions));
}

fn apply_codex_openai_image_tool_overrides(
    body_object: &mut serde_json::Map<String, Value>,
    fallback_image_model: Option<&str>,
) {
    let mut tool = body_object
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools.iter().find(|tool| {
                tool.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|tool_type| {
                        tool_type.trim().eq_ignore_ascii_case("image_generation")
                    })
            })
        })
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    normalize_codex_openai_image_generation_tool(&mut tool);
    let tool_model_is_empty = tool
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_none_or(str::is_empty);
    if tool_model_is_empty {
        let image_model = fallback_image_model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(CODEX_OPENAI_IMAGE_DEFAULT_MODEL);
        tool.insert("model".to_string(), json!(image_model));
    }
    tool.entry("output_format".to_string())
        .or_insert_with(|| json!(CODEX_OPENAI_IMAGE_DEFAULT_OUTPUT_FORMAT));
    let action = tool
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("generate")
        .to_string();
    if !tool.contains_key("action") {
        tool.insert("action".to_string(), json!("generate"));
    }
    if action == "generate" {
        tool.entry("size".to_string())
            .or_insert_with(|| json!(CODEX_IMAGE_TOOL_DEFAULT_SIZE));
        tool.entry("quality".to_string())
            .or_insert_with(|| json!(CODEX_IMAGE_TOOL_DEFAULT_QUALITY));
        tool.entry("background".to_string())
            .or_insert_with(|| json!(CODEX_IMAGE_TOOL_DEFAULT_BACKGROUND));
    }

    body_object.insert("tools".to_string(), json!([tool]));
    body_object.insert(
        "tool_choice".to_string(),
        json!({
            "type": "image_generation"
        }),
    );
}

fn codex_openai_image_has_prompt(body_object: &serde_json::Map<String, Value>) -> bool {
    body_object
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|item| item.get("content"))
        .any(|content| match content {
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(items) => items.iter().any(|item| {
                item.as_object()
                    .filter(|item| item.get("type").and_then(Value::as_str) == Some("input_text"))
                    .and_then(|item| item.get("text").and_then(Value::as_str))
                    .map(str::trim)
                    .is_some_and(|text| !text.is_empty())
            }),
            _ => false,
        })
}

fn inject_codex_default_variation_prompt(body_object: &mut serde_json::Map<String, Value>) {
    let Some(action) = body_object
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| tools.first())
        .and_then(Value::as_object)
        .and_then(|tool| tool.get("action"))
        .and_then(Value::as_str)
    else {
        return;
    };
    if action != "edit" || codex_openai_image_has_prompt(body_object) {
        return;
    }

    let Some(input) = body_object.get_mut("input").and_then(Value::as_array_mut) else {
        return;
    };
    let Some(first_message) = input.first_mut().and_then(Value::as_object_mut) else {
        return;
    };
    let Some(content) = first_message
        .get_mut("content")
        .and_then(Value::as_array_mut)
    else {
        return;
    };

    content.insert(
        0,
        json!({
            "type": "input_text",
            "text": CODEX_OPENAI_IMAGE_DEFAULT_VARIATION_PROMPT,
        }),
    );
}

fn build_stable_codex_prompt_cache_key(user_api_key_id: &str) -> Option<String> {
    let normalized = user_api_key_id.trim();
    if normalized.is_empty() {
        return None;
    }

    let namespace = format!(
        "aether:codex:prompt-cache:{CODEX_PROMPT_CACHE_NAMESPACE_VERSION}:user:{normalized}"
    );
    let mut hasher = Sha1::new();
    hasher.update(UUID_NAMESPACE_OID_BYTES);
    hasher.update(namespace.as_bytes());

    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Some(Uuid::from_bytes(bytes).to_string())
}

fn build_short_codex_header_id(seed: &str) -> Option<String> {
    let normalized = seed.trim();
    if normalized.is_empty() {
        return None;
    }

    let digest = Sha256::digest(normalized.as_bytes());
    let mut short_id = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        let _ = write!(&mut short_id, "{byte:02x}");
    }
    Some(short_id)
}

fn header_map_has_non_empty_value(headers: &http::HeaderMap, header_name: &str) -> bool {
    let target = header_name.trim().to_ascii_lowercase();
    if target.is_empty() {
        return false;
    }

    headers.iter().any(|(name, value)| {
        if name.as_str().trim().to_ascii_lowercase() != target {
            return false;
        }
        value
            .to_str()
            .ok()
            .map(str::trim)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    })
}

fn btree_map_has_non_empty_value(headers: &BTreeMap<String, String>, header_name: &str) -> bool {
    let target = header_name.trim().to_ascii_lowercase();
    if target.is_empty() {
        return false;
    }

    headers
        .iter()
        .any(|(name, value)| name.trim().eq_ignore_ascii_case(&target) && !value.trim().is_empty())
}

fn extract_codex_account_id(decrypted_auth_config_raw: Option<&str>) -> Option<String> {
    let raw = decrypted_auth_config_raw?.trim();
    if raw.is_empty() {
        return None;
    }

    serde_json::from_str::<Value>(raw).ok().and_then(|value| {
        value
            .get("account_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn maybe_insert_default_codex_header(
    provider_request_headers: &mut BTreeMap<String, String>,
    original_headers: &http::HeaderMap,
    header_name: &str,
    header_value: &str,
) {
    if header_map_has_non_empty_value(original_headers, header_name)
        || btree_map_has_non_empty_value(provider_request_headers, header_name)
    {
        return;
    }

    provider_request_headers.insert(header_name.to_string(), header_value.to_string());
}

fn maybe_inject_codex_prompt_cache_key(
    provider_request_body: &mut Value,
    provider_type: &str,
    provider_api_format: &str,
    user_api_key_id: Option<&str>,
) {
    if !is_codex_openai_responses_request(provider_type, provider_api_format) {
        return;
    }

    let Some(body_object) = provider_request_body.as_object_mut() else {
        return;
    };

    let existing = body_object
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if !existing.is_empty() {
        return;
    }

    let Some(prompt_cache_key) = user_api_key_id.and_then(build_stable_codex_prompt_cache_key)
    else {
        return;
    };

    body_object.insert(
        "prompt_cache_key".to_string(),
        Value::String(prompt_cache_key),
    );
}

pub fn apply_openai_responses_compact_special_body_edits(
    provider_request_body: &mut Value,
    provider_api_format: &str,
) {
    if !is_openai_responses_compact_request(provider_api_format) {
        return;
    }

    let Some(body_object) = provider_request_body.as_object_mut() else {
        return;
    };

    // `/v1/responses/compact` does not accept `include`, `store`, or body-level `stream`.
    body_object.remove("include");
    body_object.remove("store");
    body_object.remove("stream");
}

pub fn apply_openai_responses_image_generation_bridge_body_edits(
    provider_request_body: &mut Value,
    provider_api_format: &str,
    image_bridge_model: Option<&str>,
    enable_image_generation_tool: bool,
) {
    if is_openai_responses_compact_request(provider_api_format)
        || !(aether_ai_formats::is_openai_responses_format(provider_api_format)
            || is_openai_image_request(provider_api_format))
    {
        return;
    }

    let Some(body_object) = provider_request_body.as_object_mut() else {
        return;
    };

    normalize_codex_openai_image_generation_tools(body_object);
    if codex_openai_responses_image_bridge_enabled(
        provider_api_format,
        body_object,
        enable_image_generation_tool,
    ) {
        ensure_codex_openai_responses_image_generation_tool(body_object);
        ensure_codex_openai_responses_auto_tool_choice(body_object);
        apply_codex_image_generation_bridge_instructions(body_object);
    }

    let image_model = codex_openai_responses_model_references_image_generation(body_object);
    let explicit_image_request = is_openai_image_request(provider_api_format)
        || image_model.is_some()
        || codex_openai_responses_tool_choice_references_image_generation(body_object);
    if !enable_image_generation_tool || !explicit_image_request {
        return;
    }

    let bridge_model = normalize_codex_openai_image_bridge_model(image_bridge_model);
    body_object.insert("model".to_string(), json!(bridge_model));
    body_object.insert("stream".to_string(), json!(true));
    apply_codex_openai_image_tool_overrides(body_object, image_model.as_deref());
    inject_codex_default_variation_prompt(body_object);
}

fn ensure_codex_responses_passthrough_fields(
    body_object: &mut serde_json::Map<String, Value>,
    provider_api_format: &str,
    body_rules: Option<&Value>,
) {
    if is_openai_responses_compact_request(provider_api_format)
        || is_openai_image_request(provider_api_format)
    {
        return;
    }
    if !body_rules_handle_path(body_rules, "parallel_tool_calls") {
        body_object
            .entry("parallel_tool_calls".to_string())
            .or_insert_with(|| json!(true));
    }
    if !body_rules_handle_path(body_rules, "include") {
        match body_object.get_mut("include") {
            Some(Value::Array(include)) => {
                let has_reasoning_encrypted_content = include
                    .iter()
                    .any(|value| value.as_str() == Some(CODEX_REASONING_ENCRYPTED_CONTENT_INCLUDE));
                if !has_reasoning_encrypted_content {
                    include.push(json!(CODEX_REASONING_ENCRYPTED_CONTENT_INCLUDE));
                }
            }
            Some(_) | None => {
                body_object.insert(
                    "include".to_string(),
                    json!([CODEX_REASONING_ENCRYPTED_CONTENT_INCLUDE]),
                );
            }
        }
    }
}

fn ensure_codex_chat_reasoning_defaults(
    body_object: &mut serde_json::Map<String, Value>,
    provider_api_format: &str,
    body_rules: Option<&Value>,
) {
    if is_openai_responses_compact_request(provider_api_format)
        || is_openai_image_request(provider_api_format)
    {
        return;
    }
    if body_rules_handle_path(body_rules, "reasoning") {
        return;
    }
    let reasoning = body_object
        .entry("reasoning".to_string())
        .or_insert_with(|| json!({}));
    let Some(reasoning_object) = reasoning.as_object_mut() else {
        return;
    };
    reasoning_object
        .entry("effort".to_string())
        .or_insert_with(|| json!(CODEX_DEFAULT_REASONING_EFFORT));
    reasoning_object
        .entry("summary".to_string())
        .or_insert_with(|| json!(CODEX_DEFAULT_REASONING_SUMMARY));
}

fn codex_tool_type_rejects_top_level_name(tool_type: &str) -> bool {
    let normalized = tool_type.trim().to_ascii_lowercase();
    !normalized.is_empty()
        && normalized != "function"
        && normalized != "custom"
        && normalized != "namespace"
}

fn strip_codex_hosted_tool_names_for_backend(body_object: &mut serde_json::Map<String, Value>) {
    let Some(tools) = body_object.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };

    for tool in tools {
        let Some(tool_object) = tool.as_object_mut() else {
            continue;
        };
        if tool_object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(codex_tool_type_rejects_top_level_name)
        {
            tool_object.remove("name");
        }
    }
}

fn strip_codex_hosted_tool_choice_name_for_backend(
    body_object: &mut serde_json::Map<String, Value>,
) {
    let Some(tool_choice_object) = body_object
        .get_mut("tool_choice")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if tool_choice_object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(codex_tool_type_rejects_top_level_name)
    {
        tool_choice_object.remove("name");
    }
}

fn normalize_codex_responses_string_input(body_object: &mut serde_json::Map<String, Value>) {
    let Some(Value::String(input)) = body_object.get("input").cloned() else {
        return;
    };

    let normalized = if input.trim().is_empty() {
        Value::Array(Vec::new())
    } else {
        json!([{
            "type": "message",
            "role": "user",
            "content": input
        }])
    };
    body_object.insert("input".to_string(), normalized);
}

pub fn apply_codex_openai_responses_special_body_edits(
    provider_request_body: &mut Value,
    provider_type: &str,
    provider_api_format: &str,
    body_rules: Option<&Value>,
    user_api_key_id: Option<&str>,
) {
    apply_codex_openai_responses_special_body_edits_with_bridge_model(
        provider_request_body,
        provider_type,
        provider_api_format,
        body_rules,
        user_api_key_id,
        None,
    );
}

pub fn apply_codex_openai_responses_special_body_edits_with_bridge_model(
    provider_request_body: &mut Value,
    provider_type: &str,
    provider_api_format: &str,
    body_rules: Option<&Value>,
    user_api_key_id: Option<&str>,
    image_bridge_model: Option<&str>,
) {
    apply_codex_openai_responses_special_body_edits_with_bridge_config(
        provider_request_body,
        provider_type,
        provider_api_format,
        body_rules,
        user_api_key_id,
        image_bridge_model,
        true,
    );
}

pub fn apply_codex_openai_responses_special_body_edits_with_bridge_config(
    provider_request_body: &mut Value,
    provider_type: &str,
    provider_api_format: &str,
    body_rules: Option<&Value>,
    user_api_key_id: Option<&str>,
    image_bridge_model: Option<&str>,
    enable_image_generation_tool: bool,
) {
    if !is_codex_openai_responses_request(provider_type, provider_api_format) {
        return;
    }

    let Some(body_object) = provider_request_body.as_object_mut() else {
        return;
    };

    if !body_rules_handle_path(body_rules, "max_output_tokens") {
        body_object.remove("max_output_tokens");
    }
    if !body_rules_handle_path(body_rules, "temperature") {
        body_object.remove("temperature");
    }
    if !body_rules_handle_path(body_rules, "top_p") {
        body_object.remove("top_p");
    }
    if !body_rules_handle_path(body_rules, "metadata") {
        body_object.remove("metadata");
    }
    if is_openai_responses_compact_request(provider_api_format) {
        body_object.remove("store");
    } else if !body_rules_handle_path(body_rules, "store") {
        body_object.insert("store".to_string(), json!(false));
    }
    ensure_codex_responses_passthrough_fields(body_object, provider_api_format, body_rules);
    if !body_rules_handle_path(body_rules, "instructions")
        && !body_object.contains_key("instructions")
    {
        body_object.insert(
            "instructions".to_string(),
            json!(CODEX_DEFAULT_INSTRUCTIONS),
        );
    } else if body_object.contains_key("instructions")
        && body_object.get("instructions").is_some_and(|v| v.is_null())
    {
        body_object.insert("instructions".to_string(), json!(""));
    }
    strip_codex_hosted_tool_names_for_backend(body_object);
    strip_codex_hosted_tool_choice_name_for_backend(body_object);
    normalize_codex_responses_string_input(body_object);

    apply_openai_responses_image_generation_bridge_body_edits(
        provider_request_body,
        provider_api_format,
        image_bridge_model,
        enable_image_generation_tool,
    );
    maybe_inject_codex_prompt_cache_key(
        provider_request_body,
        provider_type,
        provider_api_format,
        user_api_key_id,
    );
}

pub fn apply_codex_openai_responses_chat_body_edits(
    provider_request_body: &mut Value,
    provider_type: &str,
    provider_api_format: &str,
    body_rules: Option<&Value>,
    user_api_key_id: Option<&str>,
) {
    apply_codex_openai_responses_chat_body_edits_with_bridge_model(
        provider_request_body,
        provider_type,
        provider_api_format,
        body_rules,
        user_api_key_id,
        None,
    );
}

pub fn apply_codex_openai_responses_chat_body_edits_with_bridge_model(
    provider_request_body: &mut Value,
    provider_type: &str,
    provider_api_format: &str,
    body_rules: Option<&Value>,
    user_api_key_id: Option<&str>,
    image_bridge_model: Option<&str>,
) {
    apply_codex_openai_responses_chat_body_edits_with_bridge_config(
        provider_request_body,
        provider_type,
        provider_api_format,
        body_rules,
        user_api_key_id,
        image_bridge_model,
        true,
    );
}

pub fn apply_codex_openai_responses_chat_body_edits_with_bridge_config(
    provider_request_body: &mut Value,
    provider_type: &str,
    provider_api_format: &str,
    body_rules: Option<&Value>,
    user_api_key_id: Option<&str>,
    image_bridge_model: Option<&str>,
    enable_image_generation_tool: bool,
) {
    apply_codex_openai_responses_special_body_edits_with_bridge_config(
        provider_request_body,
        provider_type,
        provider_api_format,
        body_rules,
        user_api_key_id,
        image_bridge_model,
        enable_image_generation_tool,
    );

    if !is_codex_openai_responses_request(provider_type, provider_api_format) {
        return;
    }
    let Some(body_object) = provider_request_body.as_object_mut() else {
        return;
    };
    ensure_codex_chat_reasoning_defaults(body_object, provider_api_format, body_rules);
}

pub fn apply_codex_openai_responses_special_headers(
    provider_request_headers: &mut BTreeMap<String, String>,
    provider_request_body: &Value,
    original_headers: &http::HeaderMap,
    provider_type: &str,
    provider_api_format: &str,
    request_id: Option<&str>,
    decrypted_auth_config_raw: Option<&str>,
) {
    if !is_codex_openai_responses_request(provider_type, provider_api_format) {
        return;
    }

    let prompt_cache_key = provider_request_body
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if !header_map_has_non_empty_value(original_headers, "chatgpt-account-id")
        && !btree_map_has_non_empty_value(provider_request_headers, "chatgpt-account-id")
    {
        if let Some(account_id) = extract_codex_account_id(decrypted_auth_config_raw) {
            provider_request_headers.insert("chatgpt-account-id".to_string(), account_id);
        }
    }

    if !header_map_has_non_empty_value(original_headers, "x-client-request-id")
        && !btree_map_has_non_empty_value(provider_request_headers, "x-client-request-id")
    {
        if let Some(request_id) = request_id.map(str::trim).filter(|value| !value.is_empty()) {
            provider_request_headers
                .insert("x-client-request-id".to_string(), request_id.to_string());
        }
    }

    if !is_openai_image_request(provider_api_format) {
        maybe_insert_default_codex_header(
            provider_request_headers,
            original_headers,
            "user-agent",
            CODEX_DEFAULT_USER_AGENT,
        );
        maybe_insert_default_codex_header(
            provider_request_headers,
            original_headers,
            "originator",
            CODEX_DEFAULT_ORIGINATOR,
        );
    }

    let short_session_id = prompt_cache_key.and_then(build_short_codex_header_id);

    if !header_map_has_non_empty_value(original_headers, "session_id")
        && !btree_map_has_non_empty_value(provider_request_headers, "session_id")
    {
        if let Some(short_session_id) = short_session_id.as_deref() {
            provider_request_headers.insert("session_id".to_string(), short_session_id.to_string());
        }
    }

    if aether_ai_formats::is_openai_responses_format(provider_api_format)
        && !header_map_has_non_empty_value(original_headers, "conversation_id")
        && !btree_map_has_non_empty_value(provider_request_headers, "conversation_id")
    {
        if let Some(short_session_id) = short_session_id.as_deref() {
            provider_request_headers
                .insert("conversation_id".to_string(), short_session_id.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_codex_openai_responses_chat_body_edits,
        apply_codex_openai_responses_special_body_edits,
        apply_codex_openai_responses_special_body_edits_with_bridge_model,
        apply_openai_responses_compact_special_body_edits, CODEX_OPENAI_IMAGE_INTERNAL_MODEL,
    };
    use serde_json::json;

    #[test]
    fn codex_responses_body_edits_inject_passthrough_fields_without_reasoning_summary() {
        let mut provider_request_body = json!( {
            "input": [{
                "role": "user",
                "content": "hello"
            }],
            "model": "gpt-5.4",
            "stream": true
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert!(provider_request_body.get("reasoning").is_none());
        assert_eq!(
            provider_request_body["include"],
            json!(["reasoning.encrypted_content"])
        );
        assert_eq!(provider_request_body["parallel_tool_calls"], json!(true));
        assert!(provider_request_body["instructions"]
            .as_str()
            .unwrap_or_default()
            .contains("Responses native `image_generation` tool"));
        assert!(provider_request_body["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|tool| tool.get("type") == Some(&json!("image_generation"))));
    }

    #[test]
    fn codex_responses_body_edits_convert_string_input_to_message_array() {
        let mut provider_request_body = json!({
            "input": "Hello, world!",
            "model": "gpt-5.4",
            "stream": true
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(
            provider_request_body["input"],
            json!([{
                "type": "message",
                "role": "user",
                "content": "Hello, world!"
            }])
        );
    }

    #[test]
    fn codex_responses_body_edits_convert_blank_string_input_to_empty_array() {
        let mut provider_request_body = json!({
            "input": "   ",
            "model": "gpt-5.4",
            "stream": true
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(provider_request_body["input"], json!([]));
    }

    #[test]
    fn codex_responses_body_edits_preserve_existing_include_and_parallel_tool_calls() {
        let mut provider_request_body = json!( {
            "input": [],
            "model": "gpt-5.4",
            "include": [
                "file_search_call.results",
                "web_search_call.results",
                "web_search_call.action.sources",
                "message.input_image.image_url",
                "computer_call_output.output.image_url",
                "code_interpreter_call.outputs",
                "message.output_text.logprobs"
            ],
            "reasoning": {"effort": "high", "summary": "detailed"},
            "parallel_tool_calls": false
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(provider_request_body["reasoning"]["effort"], json!("high"));
        assert_eq!(
            provider_request_body["reasoning"]["summary"],
            json!("detailed")
        );
        assert_eq!(
            provider_request_body["include"],
            json!([
                "file_search_call.results",
                "web_search_call.results",
                "web_search_call.action.sources",
                "message.input_image.image_url",
                "computer_call_output.output.image_url",
                "code_interpreter_call.outputs",
                "message.output_text.logprobs",
                "reasoning.encrypted_content"
            ])
        );
        assert_eq!(provider_request_body["parallel_tool_calls"], json!(false));
    }

    #[test]
    fn codex_responses_body_edits_preserve_function_tools_for_codex_backend() {
        let mut provider_request_body = json!({
            "input": [],
            "model": "gpt-5.4",
            "tools": [{
                "type": "function",
                "name": "lookup_account",
                "description": "Lookup an account by id.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "account_id": {
                            "type": "string"
                        }
                    },
                    "required": ["account_id"],
                    "additionalProperties": false
                },
                "strict": true
            }],
            "tool_choice": {
                "type": "function",
                "name": "lookup_account"
            }
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(
            provider_request_body["tools"][0]["name"],
            json!("lookup_account")
        );
        assert_eq!(
            provider_request_body["tools"][0]["parameters"]["properties"]["account_id"]["type"],
            json!("string")
        );
        assert_eq!(
            provider_request_body["tool_choice"]["name"],
            json!("lookup_account")
        );
        assert!(provider_request_body["tools"][0].get("function").is_none());
    }

    #[test]
    fn codex_responses_body_edits_strip_name_from_hosted_web_search_tool() {
        let mut provider_request_body = json!({
            "input": [],
            "model": "gpt-5.4",
            "tools": [{
                "type": "web_search",
                "name": "web_search"
            }],
            "tool_choice": {
                "type": "web_search",
                "name": "web_search"
            }
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert!(provider_request_body["tools"][0].get("name").is_none());
        assert!(provider_request_body["tool_choice"].get("name").is_none());
        assert_eq!(
            provider_request_body["tool_choice"]["type"],
            json!("web_search")
        );
    }

    #[test]
    fn compact_body_edits_strip_include_store_and_stream() {
        let mut provider_request_body = json!({
            "input": [],
            "model": "gpt-5.4",
            "include": ["reasoning.encrypted_content"],
            "store": true,
            "stream": true,
        });

        apply_openai_responses_compact_special_body_edits(
            &mut provider_request_body,
            "openai:responses:compact",
        );

        assert!(provider_request_body.get("include").is_none());
        assert!(provider_request_body.get("store").is_none());
        assert!(provider_request_body.get("stream").is_none());
        assert_eq!(provider_request_body["model"], json!("gpt-5.4"));
    }

    #[test]
    fn codex_chat_body_edits_inject_reasoning_summary_defaults() {
        let mut provider_request_body = json!({
            "input": [],
            "model": "gpt-5.4"
        });

        apply_codex_openai_responses_chat_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(
            provider_request_body["reasoning"]["effort"],
            json!("medium")
        );
        assert_eq!(provider_request_body["reasoning"]["summary"], json!("auto"));
        assert_eq!(
            provider_request_body["include"],
            json!(["reasoning.encrypted_content"])
        );
        assert_eq!(provider_request_body["parallel_tool_calls"], json!(true));
    }

    #[test]
    fn codex_chat_body_edits_preserve_existing_reasoning_effort() {
        let mut provider_request_body = json!({
            "input": [],
            "model": "gpt-5.4",
            "reasoning": {"effort": "low"}
        });

        apply_codex_openai_responses_chat_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(provider_request_body["reasoning"]["effort"], json!("low"));
        assert_eq!(provider_request_body["reasoning"]["summary"], json!("auto"));
    }

    #[test]
    fn codex_image_body_edits_force_tool_choice_and_default_generate_tool_fields() {
        let mut provider_request_body = json!({
            "input": [{
                "role": "user",
                "content": "generate image"
            }],
            "tools": [{
                "type": "image_generation"
            }],
            "tool_choice": "auto"
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:image",
            None,
            None,
        );

        assert_eq!(
            provider_request_body["tools"][0]["size"],
            json!("1024x1024")
        );
        assert_eq!(provider_request_body["tools"][0]["quality"], json!("high"));
        assert_eq!(
            provider_request_body["tools"][0]["background"],
            json!("auto")
        );
        assert_eq!(
            provider_request_body["tools"][0]["output_format"],
            json!("png")
        );
        assert_eq!(
            provider_request_body["tools"][0]["action"],
            json!("generate")
        );
        assert_eq!(
            provider_request_body["model"],
            json!(CODEX_OPENAI_IMAGE_INTERNAL_MODEL)
        );
        assert_eq!(provider_request_body["stream"], json!(true));
        assert_eq!(
            provider_request_body["tool_choice"]["type"],
            json!("image_generation")
        );
    }

    #[test]
    fn codex_responses_image_tool_edits_force_internal_model_and_tool_defaults() {
        let mut provider_request_body = json!({
            "model": "gpt-image-2",
            "input": "generate image",
            "tools": [{
                "type": "image_generation"
            }],
            "tool_choice": {"type": "image_generation"}
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(
            provider_request_body["model"],
            json!(CODEX_OPENAI_IMAGE_INTERNAL_MODEL)
        );
        assert_eq!(provider_request_body["stream"], json!(true));
        assert_eq!(
            provider_request_body["tools"][0]["type"],
            json!("image_generation")
        );
        assert_eq!(
            provider_request_body["tool_choice"]["type"],
            json!("image_generation")
        );
    }

    #[test]
    fn codex_responses_image_tool_uses_custom_bridge_model_and_keeps_tool_model() {
        let mut provider_request_body = json!({
            "model": "gpt-image-2",
            "input": "generate image",
            "tools": [{
                "type": "image_generation",
                "model": "gpt-image-2"
            }],
            "tool_choice": {"type": "image_generation"}
        });

        apply_codex_openai_responses_special_body_edits_with_bridge_model(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
            Some("gpt-5.5"),
        );

        assert_eq!(provider_request_body["model"], json!("gpt-5.5"));
        assert_eq!(
            provider_request_body["tools"][0]["model"],
            json!("gpt-image-2")
        );
        assert_eq!(
            provider_request_body["tool_choice"]["type"],
            json!("image_generation")
        );
    }

    #[test]
    fn codex_responses_image_tool_edits_triggered_by_string_tool_choice() {
        let mut provider_request_body = json!({
            "model": "gpt-image-2",
            "input": "generate image",
            "tools": [{"type": "image_generation"}],
            "tool_choice": "image_generation"
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(
            provider_request_body["model"],
            json!(CODEX_OPENAI_IMAGE_INTERNAL_MODEL)
        );
        assert_eq!(
            provider_request_body["tool_choice"]["type"],
            json!("image_generation")
        );
    }

    #[test]
    fn codex_responses_image_tool_edits_skipped_when_tool_choice_is_auto() {
        let original_model = "gpt-5.5";
        let mut provider_request_body = json!({
            "model": original_model,
            "input": [{"role": "user", "content": "hi"}],
            "tools": [
                {"type": "function", "name": "shell"},
                {"type": "image_generation"},
                {"type": "web_search"}
            ],
            "tool_choice": "auto"
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(provider_request_body["model"], json!(original_model));
        assert_eq!(provider_request_body["tool_choice"], json!("auto"));
        assert_eq!(
            provider_request_body["tools"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default(),
            3,
            "tools array should be preserved when tool_choice is auto"
        );
    }

    #[test]
    fn codex_responses_image_tool_edits_skipped_when_tool_choice_absent() {
        let original_model = "gpt-5.5";
        let mut provider_request_body = json!({
            "model": original_model,
            "input": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "image_generation"}]
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(provider_request_body["model"], json!(original_model));
        assert_eq!(provider_request_body["tool_choice"], json!("auto"));
        assert_eq!(
            provider_request_body["tools"][0]["type"],
            json!("image_generation")
        );
        assert_eq!(
            provider_request_body["tools"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default(),
            1,
            "tools array should be preserved verbatim when tool_choice is absent"
        );
    }

    #[test]
    fn codex_responses_image_tool_edits_skipped_when_tool_choice_targets_other_tool() {
        let original_model = "gpt-5.5";
        let mut provider_request_body = json!({
            "model": original_model,
            "input": [{"role": "user", "content": "hi"}],
            "tools": [
                {"type": "function", "name": "shell"},
                {"type": "image_generation"}
            ],
            "tool_choice": {"type": "function", "name": "shell"}
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:responses",
            None,
            None,
        );

        assert_eq!(provider_request_body["model"], json!(original_model));
        assert_eq!(
            provider_request_body["tool_choice"],
            json!({"type": "function", "name": "shell"})
        );
    }

    #[test]
    fn codex_image_body_edits_preserve_edit_action_without_generate_defaults() {
        let mut provider_request_body = json!({
            "tools": [{
                "type": "image_generation",
                "action": "edit",
                "input_image_mask": { "image_url": "data:image/png;base64,mask" }
            }],
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "image_url": "data:image/png;base64,image"
                }]
            }],
            "tool_choice": "auto"
        });

        apply_codex_openai_responses_special_body_edits(
            &mut provider_request_body,
            "codex",
            "openai:image",
            None,
            None,
        );

        assert_eq!(provider_request_body["tools"][0]["action"], json!("edit"));
        assert!(provider_request_body["tools"][0].get("size").is_none());
        assert!(provider_request_body["tools"][0].get("quality").is_none());
        assert!(provider_request_body["tools"][0]
            .get("background")
            .is_none());
        assert_eq!(
            provider_request_body["tools"][0]["output_format"],
            json!("png")
        );
        assert_eq!(
            provider_request_body["input"][0]["content"][0]["text"],
            json!("Create a faithful variation of the provided image.")
        );
        assert_eq!(
            provider_request_body["tool_choice"]["type"],
            json!("image_generation")
        );
    }
}

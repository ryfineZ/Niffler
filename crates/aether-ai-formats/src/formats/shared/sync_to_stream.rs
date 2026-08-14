use std::borrow::Cow;

use aether_ai_formats::formats::conversion::response::{
    convert_claude_response_to_openai_responses, convert_gemini_response_to_openai_responses,
    convert_openai_chat_response_to_openai_responses,
};
use aether_contracts::{ExecutionStreamTerminalSummary, StandardizedUsage};
use serde_json::{json, Map, Value};

use crate::formats::claude::messages::stream::ClaudeClientEmitter;
use crate::formats::gemini::generate_content::stream::GeminiClientEmitter;
use crate::formats::openai::chat::stream::{
    OpenAIChatClientEmitter, OpenAIResponsesClientEmitter, OpenAIResponsesProviderState,
};
use crate::formats::shared::sse::{encode_done_sse, encode_json_sse};
use crate::formats::shared::stream_core::common::{
    build_openai_chat_chunk, build_openai_chat_finish_chunk, build_openai_chat_usage_chunk,
};
use crate::formats::shared::stream_core::CanonicalStreamFrame;
use crate::formats::shared::AiSurfaceFinalizeError;

pub struct SyncToStreamBridgeOutcome {
    pub sse_body: Vec<u8>,
    pub terminal_summary: Option<ExecutionStreamTerminalSummary>,
}

pub fn maybe_bridge_standard_sync_json_to_stream(
    provider_body_json: &Value,
    provider_api_format: &str,
    client_api_format: &str,
    report_context: Option<&Value>,
) -> Result<Option<SyncToStreamBridgeOutcome>, AiSurfaceFinalizeError> {
    let provider_api_format = normalize_api_format(provider_api_format);
    let client_api_format = normalize_api_format(client_api_format);
    if provider_api_format == "openai:responses:compact"
        && client_api_format == "openai:responses:compact"
    {
        return bridge_openai_compact_sync_json_to_stream(provider_body_json, report_context);
    }
    if provider_api_format == "openai:image" {
        return match client_api_format.as_str() {
            "openai:image" => {
                maybe_bridge_openai_image_sync_json_to_stream(provider_body_json, report_context)
            }
            "openai:chat" => maybe_bridge_openai_image_sync_json_to_chat_stream(
                provider_body_json,
                report_context,
            ),
            "openai:responses" | "openai:responses:compact" => {
                maybe_bridge_openai_image_sync_json_to_responses_stream(
                    provider_body_json,
                    report_context,
                )
            }
            _ => Ok(None),
        };
    }
    if client_api_format == "openai:image" && provider_api_format == "gemini:generate_content" {
        return maybe_bridge_openai_image_sync_json_to_stream(provider_body_json, report_context);
    }
    if !is_standard_api_format(provider_api_format.as_str())
        || !is_standard_api_format(client_api_format.as_str())
    {
        return Ok(None);
    }

    let bridge_context = build_bridge_report_context(
        report_context,
        provider_api_format.as_str(),
        client_api_format.as_str(),
    );
    let Some(openai_responses_response) = convert_provider_sync_response_to_openai_responses(
        provider_body_json,
        provider_api_format.as_str(),
        &bridge_context,
    ) else {
        return Ok(None);
    };
    let terminal_summary =
        build_terminal_summary_from_openai_responses_response(&openai_responses_response);
    let canonical_frames = build_canonical_frames_from_openai_responses_response(
        &openai_responses_response,
        &bridge_context,
    )?;
    let sse_body =
        emit_client_stream_from_canonical_frames(canonical_frames, client_api_format.as_str())?;

    Ok(Some(SyncToStreamBridgeOutcome {
        sse_body,
        terminal_summary,
    }))
}

fn bridge_openai_compact_sync_json_to_stream(
    provider_body_json: &Value,
    report_context: Option<&Value>,
) -> Result<Option<SyncToStreamBridgeOutcome>, AiSurfaceFinalizeError> {
    let Some(response) = provider_body_json.as_object() else {
        return Err(AiSurfaceFinalizeError::new(
            "Compact 上游响应必须是 JSON 对象",
        ));
    };
    let Some(output) = response.get("output").and_then(Value::as_array) else {
        return Err(AiSurfaceFinalizeError::new(
            "Compact 上游响应缺少 output 数组",
        ));
    };

    let mut response = response.clone();
    if response
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        response.insert(
            "id".to_string(),
            Value::String(compact_bridge_response_id(report_context)),
        );
    }
    response
        .entry("object".to_string())
        .or_insert_with(|| Value::String("response".to_string()));
    response
        .entry("status".to_string())
        .or_insert_with(|| Value::String("completed".to_string()));
    if response
        .get("usage")
        .is_some_and(|usage| !compact_usage_is_valid(usage))
    {
        response.remove("usage");
    }

    let response = Value::Object(response);
    let terminal_summary = build_terminal_summary_from_openai_responses_response(&response);
    let mut sse_body = Vec::new();
    for (output_index, item) in output.iter().enumerate() {
        if !item.is_object() {
            return Err(AiSurfaceFinalizeError::new(
                "Compact 上游响应的 output 项必须是 JSON 对象",
            ));
        }
        sse_body.extend(encode_json_sse(
            Some("response.output_item.done"),
            &json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item,
            }),
        )?);
    }
    sse_body.extend(encode_json_sse(
        Some("response.completed"),
        &json!({
            "type": "response.completed",
            "response": response,
        }),
    )?);

    Ok(Some(SyncToStreamBridgeOutcome {
        terminal_summary,
        sse_body,
    }))
}

fn compact_bridge_response_id(report_context: Option<&Value>) -> String {
    report_context
        .and_then(|context| context.get("request_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("resp_{value}"))
        .unwrap_or_else(|| format!("resp_{}", uuid::Uuid::new_v4().simple()))
}

fn compact_usage_is_valid(usage: &Value) -> bool {
    let Some(usage) = usage.as_object() else {
        return false;
    };
    ["input_tokens", "output_tokens", "total_tokens"]
        .iter()
        .all(|field| usage.get(*field).is_some_and(Value::is_number))
}

fn maybe_bridge_openai_image_sync_json_to_stream(
    provider_body_json: &Value,
    report_context: Option<&Value>,
) -> Result<Option<SyncToStreamBridgeOutcome>, AiSurfaceFinalizeError> {
    let Some(provider_body_json) =
        normalize_openai_image_sync_response(provider_body_json, report_context)?
    else {
        return Ok(None);
    };
    let Some(response) = provider_body_json.as_ref().as_object() else {
        return Ok(None);
    };
    let outputs = collect_openai_image_outputs(response, report_context);
    let Some(image) = outputs.iter().find_map(OpenAiImageOutput::b64_json) else {
        return Ok(None);
    };
    let image_count = openai_image_response_image_count(response).max(outputs.len() as u64);
    let usage = response.get("usage").cloned().unwrap_or(Value::Null);
    let event_name = openai_image_completed_event_name(report_context);
    let sse_body = encode_json_sse(
        Some(event_name),
        &json!({
            "type": event_name,
            "b64_json": image,
            "usage": usage,
        }),
    )?;

    Ok(Some(SyncToStreamBridgeOutcome {
        sse_body,
        terminal_summary: Some(openai_image_terminal_summary(
            response,
            report_context,
            image_count,
        )),
    }))
}

fn maybe_bridge_openai_image_sync_json_to_chat_stream(
    provider_body_json: &Value,
    report_context: Option<&Value>,
) -> Result<Option<SyncToStreamBridgeOutcome>, AiSurfaceFinalizeError> {
    let Some(provider_body_json) =
        normalize_openai_image_sync_response(provider_body_json, report_context)?
    else {
        return Ok(None);
    };
    let Some(response) = provider_body_json.as_ref().as_object() else {
        return Ok(None);
    };
    let outputs = collect_openai_image_outputs(response, report_context);
    if outputs.is_empty() {
        return Ok(None);
    }

    let image_count = openai_image_response_image_count(response).max(outputs.len() as u64);
    let summary = openai_image_terminal_summary(response, report_context, image_count);
    let response_id = openai_image_bridge_response_id(response, report_context, "chatcmpl-image");
    let model = openai_image_bridge_response_model(response, report_context);
    let content = outputs
        .iter()
        .enumerate()
        .map(|(index, output)| output.markdown(index))
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut sse_body = Vec::new();
    sse_body.extend(encode_json_sse(
        None,
        &build_openai_chat_chunk(&response_id, &model, content, None, None),
    )?);
    sse_body.extend(encode_json_sse(
        None,
        &build_openai_chat_finish_chunk(&response_id, &model, Some("stop")),
    )?);
    if let Some((input_tokens, output_tokens, total_tokens, reasoning_tokens)) = summary
        .standardized_usage
        .as_ref()
        .and_then(openai_chat_usage_counts)
    {
        sse_body.extend(encode_json_sse(
            None,
            &build_openai_chat_usage_chunk(
                &response_id,
                &model,
                input_tokens,
                output_tokens,
                total_tokens,
                reasoning_tokens,
            ),
        )?);
    }
    sse_body.extend(encode_done_sse());

    Ok(Some(SyncToStreamBridgeOutcome {
        sse_body,
        terminal_summary: Some(summary),
    }))
}

fn maybe_bridge_openai_image_sync_json_to_responses_stream(
    provider_body_json: &Value,
    report_context: Option<&Value>,
) -> Result<Option<SyncToStreamBridgeOutcome>, AiSurfaceFinalizeError> {
    let Some(provider_body_json) =
        normalize_openai_image_sync_response(provider_body_json, report_context)?
    else {
        return Ok(None);
    };
    let Some(response) = provider_body_json.as_ref().as_object() else {
        return Ok(None);
    };
    let outputs = collect_openai_image_outputs(response, report_context);
    if outputs.is_empty() {
        return Ok(None);
    }

    let response_id = openai_image_bridge_response_id(response, report_context, "resp-image");
    let model = openai_image_bridge_response_model(response, report_context);
    let mut response_output = Vec::new();
    for (index, output) in outputs.iter().enumerate() {
        response_output.push(output.responses_image_generation_item(&response_id, index));
    }

    let mut response_object = Map::new();
    response_object.insert("id".to_string(), Value::String(response_id.clone()));
    response_object.insert("object".to_string(), Value::String("response".to_string()));
    response_object.insert("model".to_string(), Value::String(model));
    response_object.insert("status".to_string(), Value::String("completed".to_string()));
    response_object.insert("output".to_string(), Value::Array(response_output.clone()));
    if let Some(created) = response.get("created").and_then(Value::as_i64) {
        response_object.insert("created_at".to_string(), json!(created));
    }
    if let Some(usage) = response.get("usage").filter(|value| value.is_object()) {
        response_object.insert("usage".to_string(), usage.clone());
    }

    let mut sse_body = Vec::new();
    for (index, item) in response_output.iter().enumerate() {
        sse_body.extend(encode_json_sse(
            Some("response.output_item.done"),
            &json!({
                "type": "response.output_item.done",
                "output_index": index,
                "item": item,
            }),
        )?);
    }
    sse_body.extend(encode_json_sse(
        Some("response.completed"),
        &json!({
            "type": "response.completed",
            "response": Value::Object(response_object),
        }),
    )?);

    let image_count = openai_image_response_image_count(response).max(outputs.len() as u64);
    Ok(Some(SyncToStreamBridgeOutcome {
        sse_body,
        terminal_summary: Some(openai_image_terminal_summary(
            response,
            report_context,
            image_count,
        )),
    }))
}

#[derive(Clone, Debug)]
struct OpenAiImageOutput {
    b64_json: Option<String>,
    url: Option<String>,
    mime_type: String,
    output_format: Option<String>,
    revised_prompt: Option<String>,
}

impl OpenAiImageOutput {
    fn b64_json(&self) -> Option<String> {
        self.b64_json
            .clone()
            .or_else(|| self.url.as_deref().and_then(extract_base64_from_data_url))
    }

    fn source_url(&self) -> Option<String> {
        self.url.clone().or_else(|| {
            self.b64_json
                .as_ref()
                .map(|value| format!("data:{};base64,{value}", self.mime_type))
        })
    }

    fn markdown(&self, index: usize) -> String {
        let alt = if index == 0 {
            "generated image".to_string()
        } else {
            format!("generated image {}", index + 1)
        };
        match self.source_url() {
            Some(url) => format!("![{alt}]({url})"),
            None => String::new(),
        }
    }

    fn responses_image_generation_item(&self, response_id: &str, index: usize) -> Value {
        let mut item = Map::new();
        item.insert(
            "id".to_string(),
            Value::String(format!("{response_id}_img_{index}")),
        );
        item.insert(
            "type".to_string(),
            Value::String("image_generation_call".to_string()),
        );
        item.insert("status".to_string(), Value::String("completed".to_string()));
        if let Some(result) = self.b64_json().or_else(|| self.url.clone()) {
            item.insert("result".to_string(), Value::String(result));
        }
        if let Some(output_format) = self.output_format.as_ref() {
            item.insert(
                "output_format".to_string(),
                Value::String(output_format.clone()),
            );
        }
        if let Some(revised_prompt) = self.revised_prompt.as_ref() {
            item.insert(
                "revised_prompt".to_string(),
                Value::String(revised_prompt.clone()),
            );
        }
        Value::Object(item)
    }
}

fn normalize_openai_image_sync_response<'a>(
    provider_body_json: &'a Value,
    report_context: Option<&Value>,
) -> Result<Option<Cow<'a, Value>>, AiSurfaceFinalizeError> {
    let provider_api_format = report_context
        .and_then(|value| value.get("provider_api_format"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("openai:image");
    if provider_api_format == "gemini:generate_content" {
        let Some(converted) =
            crate::formats::shared::image_bridge::build_openai_image_response_from_gemini_response(
                provider_body_json,
                report_context,
            )
        else {
            return Ok(None);
        };
        return Ok(Some(Cow::Owned(converted)));
    }
    if provider_body_json.get("output").is_some() && provider_body_json.get("data").is_none() {
        let Some(converted) = crate::formats::shared::image_bridge::build_openai_image_response_from_response_stream_sync_body(
            provider_body_json,
            report_context,
        ) else {
            return Ok(None);
        };
        return Ok(Some(Cow::Owned(converted)));
    }
    Ok(Some(Cow::Borrowed(provider_body_json)))
}

fn collect_openai_image_outputs(
    response: &Map<String, Value>,
    report_context: Option<&Value>,
) -> Vec<OpenAiImageOutput> {
    response
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|item| openai_image_output_from_item(item, report_context))
        .collect()
}

fn openai_image_output_from_item(
    item: &Map<String, Value>,
    report_context: Option<&Value>,
) -> Option<OpenAiImageOutput> {
    let b64_json = extract_openai_image_sync_b64_json(item);
    let url = item
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if b64_json.is_none() && url.is_none() {
        return None;
    }
    let output_format = item
        .get("output_format")
        .or_else(|| item.get("format"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| image_request_output_format(report_context));
    let mime_type = url
        .as_deref()
        .and_then(extract_mime_type_from_data_url)
        .or_else(|| {
            output_format
                .as_deref()
                .map(mime_type_from_image_output_format)
        })
        .unwrap_or_else(|| "image/png".to_string());
    let revised_prompt = item
        .get("revised_prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    Some(OpenAiImageOutput {
        b64_json,
        url,
        mime_type,
        output_format,
        revised_prompt,
    })
}

fn openai_image_response_image_count(response: &Map<String, Value>) -> u64 {
    response
        .get("data")
        .and_then(Value::as_array)
        .map(|items| items.len() as u64)
        .unwrap_or(0)
}

fn openai_image_terminal_summary(
    response: &Map<String, Value>,
    report_context: Option<&Value>,
    image_count: u64,
) -> ExecutionStreamTerminalSummary {
    ExecutionStreamTerminalSummary {
        standardized_usage: openai_image_standardized_usage(
            response.get("usage"),
            report_context,
            image_count,
        ),
        finish_reason: Some("stop".to_string()),
        response_id: response
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        model: response
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| image_bridge_model(report_context)),
        observed_finish: true,
        unknown_event_count: 0,
        parser_error: None,
    }
}

fn openai_image_standardized_usage(
    usage: Option<&Value>,
    report_context: Option<&Value>,
    image_count: u64,
) -> Option<StandardizedUsage> {
    let mut standardized_usage = usage
        .and_then(standardized_usage_from_openai_usage)
        .unwrap_or_default();
    if image_count > 0 {
        standardized_usage.request_count = i64::try_from(image_count).unwrap_or(i64::MAX);
        standardized_usage
            .dimensions
            .insert("image_count".to_string(), json!(image_count));
    }
    if let Some(output_format) = image_request_output_format(report_context) {
        standardized_usage
            .dimensions
            .insert("image_output_format".to_string(), json!(output_format));
    }
    if let Some(size) = image_request_size(report_context) {
        standardized_usage
            .dimensions
            .insert("image_size".to_string(), json!(size));
    }
    if let Some(quality) = image_request_quality(report_context) {
        standardized_usage
            .dimensions
            .insert("image_quality".to_string(), json!(quality));
    }
    (standardized_usage.signal_score() > 0).then_some(standardized_usage)
}

fn openai_chat_usage_counts(usage: &StandardizedUsage) -> Option<(u64, u64, u64, u64)> {
    let input_tokens = usage.input_tokens.max(0) as u64;
    let output_tokens = usage.output_tokens.max(0) as u64;
    let reasoning_tokens = usage.reasoning_tokens.max(0) as u64;
    let total_tokens = usage
        .dimensions
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| input_tokens.saturating_add(output_tokens));
    (total_tokens > 0).then_some((input_tokens, output_tokens, total_tokens, reasoning_tokens))
}

fn openai_image_bridge_response_id(
    response: &Map<String, Value>,
    report_context: Option<&Value>,
    fallback_prefix: &str,
) -> String {
    response
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            report_context
                .and_then(|value| value.get("request_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| format!("{fallback_prefix}-{value}"))
        })
        .unwrap_or_else(|| fallback_prefix.to_string())
}

fn openai_image_bridge_response_model(
    response: &Map<String, Value>,
    report_context: Option<&Value>,
) -> String {
    response
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| image_bridge_model(report_context))
        .unwrap_or_else(|| "gpt-image".to_string())
}

fn image_request_output_format(report_context: Option<&Value>) -> Option<String> {
    report_context
        .and_then(|value| value.get("image_request"))
        .and_then(|value| value.get("output_format"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn image_request_size(report_context: Option<&Value>) -> Option<String> {
    report_context
        .and_then(|value| value.get("image_request"))
        .and_then(|value| value.get("size"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn image_request_quality(report_context: Option<&Value>) -> Option<String> {
    report_context
        .and_then(|value| value.get("image_request"))
        .and_then(|value| value.get("quality"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn mime_type_from_image_output_format(output_format: &str) -> String {
    match output_format.trim().to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "webp" => "image/webp".to_string(),
        "png" => "image/png".to_string(),
        value if !value.is_empty() => format!("image/{value}"),
        _ => "image/png".to_string(),
    }
}

fn normalize_api_format(value: &str) -> String {
    aether_ai_formats::normalize_api_format_alias(value)
}

fn is_standard_api_format(value: &str) -> bool {
    matches!(
        value,
        "openai:chat"
            | "openai:responses"
            | "openai:responses:compact"
            | "claude:messages"
            | "gemini:generate_content"
    )
}

fn extract_openai_image_sync_b64_json(item: &serde_json::Map<String, Value>) -> Option<String> {
    item.get("b64_json")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            item.get("url")
                .and_then(Value::as_str)
                .and_then(extract_base64_from_data_url)
        })
}

fn extract_base64_from_data_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let (metadata, payload) = trimmed.split_once(',')?;
    if !metadata.starts_with("data:") || !metadata.ends_with(";base64") {
        return None;
    }
    (!payload.trim().is_empty()).then(|| payload.trim().to_string())
}

fn extract_mime_type_from_data_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let (metadata, _) = trimmed.split_once(',')?;
    let mime_type = metadata.strip_prefix("data:")?.strip_suffix(";base64")?;
    let mime_type = mime_type.trim();
    (!mime_type.is_empty()).then(|| mime_type.to_string())
}

fn openai_image_completed_event_name(report_context: Option<&Value>) -> &'static str {
    if openai_image_request_operation(report_context) == Some("edit") {
        "image_edit.completed"
    } else {
        "image_generation.completed"
    }
}

fn openai_image_request_operation(report_context: Option<&Value>) -> Option<&str> {
    report_context
        .and_then(|value| value.get("image_request"))
        .and_then(|value| value.get("operation"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn image_bridge_model(report_context: Option<&Value>) -> Option<String> {
    report_context.and_then(|context| {
        context
            .get("mapped_model")
            .or_else(|| context.get("model"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn build_bridge_report_context(
    report_context: Option<&Value>,
    provider_api_format: &str,
    client_api_format: &str,
) -> Value {
    let mut context = report_context
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let object = context
        .as_object_mut()
        .expect("bridge report context should stay object");
    object
        .entry("provider_api_format".to_string())
        .or_insert_with(|| Value::String(provider_api_format.to_string()));
    object
        .entry("client_api_format".to_string())
        .or_insert_with(|| Value::String(client_api_format.to_string()));
    context
}

fn convert_provider_sync_response_to_openai_responses(
    provider_body_json: &Value,
    provider_api_format: &str,
    report_context: &Value,
) -> Option<Value> {
    match provider_api_format {
        "openai:responses" | "openai:responses:compact" => Some(provider_body_json.clone()),
        "openai:chat" => convert_openai_chat_response_to_openai_responses(
            provider_body_json,
            report_context,
            false,
        ),
        "claude:messages" => {
            convert_claude_response_to_openai_responses(provider_body_json, report_context)
        }
        "gemini:generate_content" => {
            convert_gemini_response_to_openai_responses(provider_body_json, report_context)
        }
        _ => None,
    }
}

fn build_canonical_frames_from_openai_responses_response(
    openai_responses_response: &Value,
    report_context: &Value,
) -> Result<Vec<CanonicalStreamFrame>, AiSurfaceFinalizeError> {
    let mut state = OpenAIResponsesProviderState::default();
    let line = format!(
        "data: {}\n",
        serde_json::to_string(&json!({
            "type": "response.completed",
            "response": openai_responses_response,
        }))
        .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?
    );
    let mut frames = state
        .push_line(report_context, line.into_bytes())
        .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?;
    frames.extend(
        state
            .finish(report_context)
            .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
    );
    Ok(frames)
}

fn emit_client_stream_from_canonical_frames(
    canonical_frames: Vec<CanonicalStreamFrame>,
    client_api_format: &str,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    match client_api_format {
        "openai:chat" => {
            let mut emitter = OpenAIChatClientEmitter::default();
            emit_with_openai_chat_emitter(&mut emitter, canonical_frames)
        }
        "openai:responses" | "openai:responses:compact" => {
            let mut emitter = OpenAIResponsesClientEmitter::default();
            emit_with_openai_responses_emitter(&mut emitter, canonical_frames)
        }
        "claude:messages" => {
            let mut emitter = ClaudeClientEmitter::default();
            emit_with_claude_emitter(&mut emitter, canonical_frames)
        }
        "gemini:generate_content" => {
            let mut emitter = GeminiClientEmitter::default();
            emit_with_gemini_emitter(&mut emitter, canonical_frames)
        }
        _ => Ok(Vec::new()),
    }
}

fn emit_with_openai_chat_emitter(
    emitter: &mut OpenAIChatClientEmitter,
    canonical_frames: Vec<CanonicalStreamFrame>,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    let mut output = Vec::new();
    for frame in canonical_frames {
        output.extend(
            emitter
                .emit(frame)
                .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
        );
    }
    output.extend(
        emitter
            .finish()
            .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
    );
    Ok(output)
}

fn emit_with_openai_responses_emitter(
    emitter: &mut OpenAIResponsesClientEmitter,
    canonical_frames: Vec<CanonicalStreamFrame>,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    let mut output = Vec::new();
    for frame in canonical_frames {
        output.extend(
            emitter
                .emit(frame)
                .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
        );
    }
    output.extend(
        emitter
            .finish()
            .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
    );
    Ok(output)
}

fn emit_with_claude_emitter(
    emitter: &mut ClaudeClientEmitter,
    canonical_frames: Vec<CanonicalStreamFrame>,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    let mut output = Vec::new();
    for frame in canonical_frames {
        output.extend(
            emitter
                .emit(frame)
                .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
        );
    }
    output.extend(
        emitter
            .finish()
            .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
    );
    Ok(output)
}

fn emit_with_gemini_emitter(
    emitter: &mut GeminiClientEmitter,
    canonical_frames: Vec<CanonicalStreamFrame>,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    let mut output = Vec::new();
    for frame in canonical_frames {
        output.extend(
            emitter
                .emit(frame)
                .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
        );
    }
    output.extend(
        emitter
            .finish()
            .map_err(|err| AiSurfaceFinalizeError::new(err.to_string()))?,
    );
    Ok(output)
}

fn build_terminal_summary_from_openai_responses_response(
    openai_responses_response: &Value,
) -> Option<ExecutionStreamTerminalSummary> {
    let response = openai_responses_response.as_object()?;
    let response_id = response
        .get("id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let model = response
        .get("model")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let finish_reason = response
        .get("output")
        .and_then(Value::as_array)
        .map(|output| resolve_openai_responses_finish_reason(output))
        .filter(|value| !value.trim().is_empty());
    let standardized_usage = response
        .get("usage")
        .and_then(standardized_usage_from_openai_usage);
    Some(ExecutionStreamTerminalSummary {
        standardized_usage,
        finish_reason,
        response_id,
        model,
        observed_finish: true,
        unknown_event_count: 0,
        parser_error: None,
    })
}

fn resolve_openai_responses_finish_reason(output: &[Value]) -> String {
    let has_tool_calls = output.iter().filter_map(Value::as_object).any(|item| {
        item.get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value == "function_call")
    });
    if has_tool_calls {
        "tool_calls".to_string()
    } else {
        "stop".to_string()
    }
}

fn standardized_usage_from_openai_usage(value: &Value) -> Option<StandardizedUsage> {
    let usage = value.as_object()?;
    let mut input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_creation_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("input_tokens_details")
                .or_else(|| usage.get("prompt_tokens_details"))
                .and_then(Value::as_object)
                .and_then(|details| details.get("cached_creation_tokens"))
                .and_then(Value::as_i64)
        })
        .unwrap_or(0);
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("input_tokens_details")
                .or_else(|| usage.get("prompt_tokens_details"))
                .and_then(Value::as_object)
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_i64)
        })
        .unwrap_or(0);
    let reasoning_tokens = usage
        .get("reasoning_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("output_tokens_details")
                .or_else(|| usage.get("completion_tokens_details"))
                .and_then(Value::as_object)
                .and_then(|details| details.get("reasoning_tokens"))
                .and_then(Value::as_i64)
        })
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| input_tokens.saturating_add(output_tokens));
    if input_tokens == 0 && total_tokens > output_tokens {
        input_tokens = total_tokens.saturating_sub(output_tokens);
    }
    let mut standardized_usage = StandardizedUsage::new();
    standardized_usage.input_tokens = input_tokens;
    standardized_usage.output_tokens = output_tokens;
    standardized_usage.cache_creation_tokens = cache_creation_tokens;
    standardized_usage.cache_read_tokens = cache_read_tokens;
    standardized_usage.reasoning_tokens = reasoning_tokens;
    standardized_usage
        .dimensions
        .insert("total_tokens".to_string(), json!(total_tokens));
    Some(standardized_usage.normalize_cache_creation_breakdown())
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{maybe_bridge_standard_sync_json_to_stream, standardized_usage_from_openai_usage};

    fn utf8(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).expect("utf8 should decode")
    }

    #[test]
    fn openai_sync_usage_derives_missing_input_tokens_from_total() {
        let usage = standardized_usage_from_openai_usage(&json!({
            "output_tokens": 177,
            "total_tokens": 20_612,
            "input_tokens_details": {
                "cached_tokens": 19_840,
            },
        }))
        .expect("usage should parse");

        assert_eq!(usage.input_tokens, 20_435);
        assert_eq!(usage.output_tokens, 177);
        assert_eq!(usage.cache_read_tokens, 19_840);
    }

    #[test]
    fn openai_sync_usage_does_not_double_count_reasoning_tokens_when_total_is_missing() {
        let usage = standardized_usage_from_openai_usage(&json!({
            "input_tokens": 26,
            "output_tokens": 148,
            "output_tokens_details": {
                "reasoning_tokens": 10,
            },
        }))
        .expect("usage should parse");

        assert_eq!(usage.input_tokens, 26);
        assert_eq!(usage.output_tokens, 148);
        assert_eq!(usage.reasoning_tokens, 10);
        assert_eq!(
            usage.dimensions.get("total_tokens").and_then(Value::as_i64),
            Some(174),
        );
    }

    #[test]
    fn bridges_compact_json_to_sse_without_losing_compaction_output() {
        let outcome = maybe_bridge_standard_sync_json_to_stream(
            &json!({
                "output": [{
                    "type": "compaction",
                    "encrypted_content": "opaque-history"
                }],
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 2,
                    "total_tokens": 12
                }
            }),
            "openai:responses:compact",
            "openai:responses:compact",
            Some(&json!({"request_id":"compact-request-1"})),
        )
        .expect("bridge should succeed")
        .expect("bridge should produce sse");

        let output = utf8(outcome.sse_body);
        assert!(output.contains("event: response.output_item.done"));
        assert!(output.contains("\"type\":\"compaction\""));
        assert!(output.contains("\"encrypted_content\":\"opaque-history\""));
        assert!(output.contains("event: response.completed"));
        assert!(output.contains("\"id\":\"resp_compact-request-1\""));
        assert!(!output.contains("data: [DONE]"));

        let summary = outcome
            .terminal_summary
            .expect("terminal summary should exist");
        assert_eq!(
            summary.response_id.as_deref(),
            Some("resp_compact-request-1")
        );
        assert_eq!(
            summary
                .standardized_usage
                .as_ref()
                .and_then(|usage| usage.dimensions.get("total_tokens")),
            Some(&json!(12))
        );
    }

    #[test]
    fn compact_json_bridge_rejects_missing_output() {
        let error = match maybe_bridge_standard_sync_json_to_stream(
            &json!({"id":"resp-invalid"}),
            "openai:responses:compact",
            "openai:responses:compact",
            None,
        ) {
            Err(error) => error,
            Ok(_) => panic!("invalid compact response should fail"),
        };

        assert!(error.to_string().contains("缺少 output 数组"));
    }

    #[test]
    fn bridges_openai_image_sync_json_to_generation_completed_sse() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:image",
            "mapped_model": "gpt-image-1",
            "image_request": {
                "operation": "generate"
            }
        });
        let outcome = maybe_bridge_standard_sync_json_to_stream(
            &json!({
                "created": 1776971267,
                "data": [{
                    "b64_json": "aGVsbG8="
                }],
                "usage": {
                    "total_tokens": 100,
                    "input_tokens": 50,
                    "output_tokens": 50,
                    "input_tokens_details": {
                        "text_tokens": 10,
                        "image_tokens": 40
                    }
                }
            }),
            "openai:image",
            "openai:image",
            Some(&report_context),
        )
        .expect("bridge should succeed")
        .expect("bridge should produce sse");

        let output = utf8(outcome.sse_body);
        assert!(output.contains("event: image_generation.completed"));
        assert!(output.contains("\"type\":\"image_generation.completed\""));
        assert!(output.contains("\"b64_json\":\"aGVsbG8=\""));
        assert!(output.contains("\"total_tokens\":100"));

        let summary = outcome
            .terminal_summary
            .expect("terminal summary should exist");
        assert_eq!(summary.model.as_deref(), Some("gpt-image-1"));
        assert_eq!(summary.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            summary
                .standardized_usage
                .as_ref()
                .and_then(|usage| usage.dimensions.get("total_tokens"))
                .cloned(),
            Some(json!(100))
        );
        assert_eq!(
            summary
                .standardized_usage
                .as_ref()
                .and_then(|usage| usage.dimensions.get("image_count"))
                .cloned(),
            Some(json!(1))
        );
    }

    #[test]
    fn bridges_openai_image_sync_json_to_openai_chat_sse() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:chat",
            "mapped_model": "gpt-image-2",
            "image_request": {
                "operation": "generate",
                "output_format": "png",
                "size": "1024x1024",
                "quality": "medium"
            }
        });
        let outcome = maybe_bridge_standard_sync_json_to_stream(
            &json!({
                "id": "img_123",
                "created": 1776971267,
                "model": "gpt-image-2",
                "data": [
                    {"b64_json": "aGVsbG8="},
                    {"b64_json": "d29ybGQ="}
                ],
                "usage": {
                    "total_tokens": 100,
                    "input_tokens": 50,
                    "output_tokens": 50
                }
            }),
            "openai:image",
            "openai:chat",
            Some(&report_context),
        )
        .expect("bridge should succeed")
        .expect("bridge should produce sse");

        let output = utf8(outcome.sse_body);
        assert!(output.contains("\"object\":\"chat.completion.chunk\""));
        assert!(output.contains("![generated image](data:image/png;base64,aGVsbG8=)"));
        assert!(output.contains("![generated image 2](data:image/png;base64,d29ybGQ=)"));
        assert!(output.contains("\"finish_reason\":\"stop\""));
        assert!(output.contains("data: [DONE]"));
        assert!(!output.contains("image_generation.completed"));

        let summary = outcome
            .terminal_summary
            .expect("terminal summary should exist");
        let usage = summary
            .standardized_usage
            .as_ref()
            .expect("standard usage should exist");
        assert_eq!(usage.request_count, 2);
        assert_eq!(usage.dimensions.get("image_count"), Some(&json!(2)));
        assert_eq!(
            usage.dimensions.get("image_size"),
            Some(&json!("1024x1024"))
        );
        assert_eq!(
            usage.dimensions.get("image_quality"),
            Some(&json!("medium"))
        );
    }

    #[test]
    fn bridges_openai_image_sync_data_url_to_edit_completed_sse() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:image",
            "image_request": {
                "operation": "edit"
            }
        });
        let outcome = maybe_bridge_standard_sync_json_to_stream(
            &json!({
                "created": 1776971267,
                "data": [{
                    "url": "data:image/webp;base64,d29ybGQ="
                }],
                "usage": {
                    "total_tokens": 9,
                    "input_tokens": 4,
                    "output_tokens": 5
                }
            }),
            "openai:image",
            "openai:image",
            Some(&report_context),
        )
        .expect("bridge should succeed")
        .expect("bridge should produce sse");

        let output = utf8(outcome.sse_body);
        assert!(output.contains("event: image_edit.completed"));
        assert!(output.contains("\"type\":\"image_edit.completed\""));
        assert!(output.contains("\"b64_json\":\"d29ybGQ=\""));
        assert!(output.contains("\"total_tokens\":9"));
    }
}

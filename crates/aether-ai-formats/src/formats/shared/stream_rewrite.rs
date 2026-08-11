use serde_json::Value;

use crate::formats::openai::image::stream::{
    OpenAiImageChatStreamState, OpenAiImageResponsesStreamState, OpenAiImageStreamState,
};
use crate::formats::shared::model_directives::model_directive_display_model_from_report_context;
use crate::formats::shared::stream_core::StreamingStandardFormatMatrix;
use crate::formats::shared::AiSurfaceFinalizeError;
use crate::provider_compat::kiro_stream::KiroToClaudeCliStreamState;
use crate::provider_compat::private_envelope::transform_provider_private_stream_line;
use crate::provider_compat::surfaces::{
    provider_adaptation_should_unwrap_stream_envelope, KIRO_ENVELOPE_NAME,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizeStreamRewriteMode {
    EnvelopeUnwrap,
    ModelDirectiveDisplay,
    OpenAiImage,
    OpenAiImageToOpenAiChat,
    OpenAiImageToOpenAiResponses,
    Standard,
    KiroToClaudeCli,
    KiroToClaudeCliThenStandard,
}

pub fn resolve_finalize_stream_rewrite_mode(
    report_context: &Value,
) -> Option<FinalizeStreamRewriteMode> {
    let needs_conversion = report_context
        .get("needs_conversion")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let envelope_name = report_context
        .get("envelope_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let provider_api_format = report_context
        .get("provider_api_format")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let client_api_format = report_context
        .get("client_api_format")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if needs_conversion
        && envelope_name.eq_ignore_ascii_case(KIRO_ENVELOPE_NAME)
        && provider_api_format == "claude:messages"
    {
        return supports_standard_stream_rewrite(
            provider_api_format.as_str(),
            client_api_format.as_str(),
        )
        .then_some(FinalizeStreamRewriteMode::KiroToClaudeCliThenStandard);
    }

    if provider_api_format == "openai:image" && client_api_format == "openai:chat" {
        return Some(FinalizeStreamRewriteMode::OpenAiImageToOpenAiChat);
    }

    if provider_api_format == "openai:image" && client_api_format == "openai:responses" {
        return Some(FinalizeStreamRewriteMode::OpenAiImageToOpenAiResponses);
    }

    if provider_api_format == "openai:responses"
        && client_api_format == "openai:responses"
        && report_context
            .get("codex_hosted_image_generation")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return Some(FinalizeStreamRewriteMode::OpenAiImageToOpenAiResponses);
    }

    if provider_api_format == "openai:image" && client_api_format == "openai:image" {
        if report_context
            .get("openai_image_transport_mode")
            .and_then(Value::as_str)
            .is_some_and(|mode| mode.trim().eq_ignore_ascii_case("images_passthrough"))
        {
            return None;
        }
        return Some(FinalizeStreamRewriteMode::OpenAiImage);
    }

    if needs_conversion {
        // CPA strategy: when provider and client share the same wire format
        // (exact match or same family), pass through the stream verbatim.
        // Parsing→rebuilding only adds overhead and may lose information
        // (encrypted_content, original item IDs, etc.).
        if is_same_format_family(provider_api_format.as_str(), client_api_format.as_str()) {
            return model_directive_display_model_from_report_context(report_context)
                .map(|_| FinalizeStreamRewriteMode::ModelDirectiveDisplay);
        }
        return supports_standard_stream_rewrite(
            provider_api_format.as_str(),
            client_api_format.as_str(),
        )
        .then_some(FinalizeStreamRewriteMode::Standard);
    }

    if envelope_name.eq_ignore_ascii_case(KIRO_ENVELOPE_NAME) {
        return (provider_api_format == "claude:messages"
            && client_api_format == "claude:messages")
            .then_some(FinalizeStreamRewriteMode::KiroToClaudeCli);
    }

    if model_directive_display_model_from_report_context(report_context).is_some()
        && provider_api_format == client_api_format
        && is_standard_provider_api_format(provider_api_format.as_str())
        && !provider_adaptation_should_unwrap_stream_envelope(
            envelope_name.as_str(),
            provider_api_format.as_str(),
        )
    {
        return Some(FinalizeStreamRewriteMode::ModelDirectiveDisplay);
    }

    (provider_api_format == client_api_format
        && provider_adaptation_should_unwrap_stream_envelope(
            envelope_name.as_str(),
            provider_api_format.as_str(),
        ))
    .then_some(FinalizeStreamRewriteMode::EnvelopeUnwrap)
}

enum AiSurfaceStreamRewriteState {
    EnvelopeUnwrap,
    ModelDirectiveDisplay,
    OpenAiImage(Box<OpenAiImageStreamState>),
    OpenAiImageToOpenAiChat(Box<OpenAiImageChatStreamState>),
    OpenAiImageToOpenAiResponses(Box<OpenAiImageResponsesStreamState>),
    Standard(Box<StreamingStandardFormatMatrix>),
    KiroToClaudeCli(Box<KiroToClaudeCliStreamState>),
    KiroToClaudeCliThenStandard {
        kiro: Box<KiroToClaudeCliStreamState>,
        standard: Box<StreamingStandardFormatMatrix>,
    },
}

pub struct AiSurfaceStreamRewriter<'a> {
    report_context: &'a Value,
    buffered: Vec<u8>,
    state: AiSurfaceStreamRewriteState,
}

pub fn maybe_build_ai_surface_stream_rewriter<'a>(
    report_context: Option<&'a Value>,
) -> Option<AiSurfaceStreamRewriter<'a>> {
    let report_context = report_context?;
    let state = match resolve_finalize_stream_rewrite_mode(report_context)? {
        FinalizeStreamRewriteMode::EnvelopeUnwrap => AiSurfaceStreamRewriteState::EnvelopeUnwrap,
        FinalizeStreamRewriteMode::ModelDirectiveDisplay => {
            AiSurfaceStreamRewriteState::ModelDirectiveDisplay
        }
        FinalizeStreamRewriteMode::OpenAiImage => {
            AiSurfaceStreamRewriteState::OpenAiImage(Box::<OpenAiImageStreamState>::default())
        }
        FinalizeStreamRewriteMode::OpenAiImageToOpenAiChat => {
            AiSurfaceStreamRewriteState::OpenAiImageToOpenAiChat(
                Box::<OpenAiImageChatStreamState>::default(),
            )
        }
        FinalizeStreamRewriteMode::OpenAiImageToOpenAiResponses => {
            AiSurfaceStreamRewriteState::OpenAiImageToOpenAiResponses(Box::<
                OpenAiImageResponsesStreamState,
            >::default())
        }
        FinalizeStreamRewriteMode::Standard => {
            AiSurfaceStreamRewriteState::Standard(Box::<StreamingStandardFormatMatrix>::default())
        }
        FinalizeStreamRewriteMode::KiroToClaudeCli => AiSurfaceStreamRewriteState::KiroToClaudeCli(
            Box::new(KiroToClaudeCliStreamState::new(report_context)),
        ),
        FinalizeStreamRewriteMode::KiroToClaudeCliThenStandard => {
            AiSurfaceStreamRewriteState::KiroToClaudeCliThenStandard {
                kiro: Box::new(KiroToClaudeCliStreamState::new(report_context)),
                standard: Box::<StreamingStandardFormatMatrix>::default(),
            }
        }
    };

    Some(AiSurfaceStreamRewriter {
        report_context,
        buffered: Vec::new(),
        state,
    })
}

impl AiSurfaceStreamRewriter<'_> {
    pub fn push_chunk(&mut self, chunk: &[u8]) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
        match &mut self.state {
            AiSurfaceStreamRewriteState::OpenAiImage(state) => {
                state.push_chunk(self.report_context, chunk)
            }
            AiSurfaceStreamRewriteState::OpenAiImageToOpenAiChat(state) => {
                state.push_chunk(self.report_context, chunk)
            }
            AiSurfaceStreamRewriteState::OpenAiImageToOpenAiResponses(state) => {
                let output = state.push_chunk(self.report_context, chunk)?;
                rewrite_model_directive_stream_bytes(self.report_context, output)
            }
            AiSurfaceStreamRewriteState::KiroToClaudeCli(state) => {
                state.push_chunk(self.report_context, chunk)
            }
            AiSurfaceStreamRewriteState::KiroToClaudeCliThenStandard { kiro, standard } => {
                let claude_bytes = kiro.push_chunk(self.report_context, chunk)?;
                transform_standard_bytes(standard, self.report_context, claude_bytes)
            }
            AiSurfaceStreamRewriteState::EnvelopeUnwrap
            | AiSurfaceStreamRewriteState::ModelDirectiveDisplay
            | AiSurfaceStreamRewriteState::Standard(_) => {
                self.buffered.extend_from_slice(chunk);
                let mut output = Vec::new();
                while let Some(line_end) = self.buffered.iter().position(|byte| *byte == b'\n') {
                    let line = self.buffered.drain(..=line_end).collect::<Vec<_>>();
                    output.extend(self.transform_line(line)?);
                }
                Ok(output)
            }
        }
    }

    pub fn finish(&mut self) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
        match &mut self.state {
            AiSurfaceStreamRewriteState::OpenAiImage(state) => state.finish(self.report_context),
            AiSurfaceStreamRewriteState::OpenAiImageToOpenAiChat(state) => {
                state.finish(self.report_context)
            }
            AiSurfaceStreamRewriteState::OpenAiImageToOpenAiResponses(state) => {
                let output = state.finish(self.report_context)?;
                rewrite_model_directive_stream_bytes(self.report_context, output)
            }
            AiSurfaceStreamRewriteState::KiroToClaudeCli(state) => {
                state.finish(self.report_context)
            }
            AiSurfaceStreamRewriteState::KiroToClaudeCliThenStandard { kiro, standard } => {
                let mut output = transform_standard_bytes(
                    standard,
                    self.report_context,
                    kiro.finish(self.report_context)?,
                )?;
                output.extend(standard.finish(self.report_context)?);
                Ok(output)
            }
            AiSurfaceStreamRewriteState::EnvelopeUnwrap
            | AiSurfaceStreamRewriteState::ModelDirectiveDisplay
            | AiSurfaceStreamRewriteState::Standard(_) => {
                if self.buffered.is_empty() {
                    if let AiSurfaceStreamRewriteState::Standard(state) = &mut self.state {
                        return state.finish(self.report_context);
                    }
                    return Ok(Vec::new());
                }
                let line = std::mem::take(&mut self.buffered);
                let mut output = self.transform_line(line)?;
                if let AiSurfaceStreamRewriteState::Standard(state) = &mut self.state {
                    output.extend(state.finish(self.report_context)?);
                }
                Ok(output)
            }
        }
    }

    fn transform_line(&mut self, line: Vec<u8>) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
        match &mut self.state {
            AiSurfaceStreamRewriteState::EnvelopeUnwrap => {
                let output = transform_provider_private_stream_line(self.report_context, line)
                    .map_err(AiSurfaceFinalizeError::from)?;
                rewrite_model_directive_stream_line(self.report_context, output)
            }
            AiSurfaceStreamRewriteState::ModelDirectiveDisplay => {
                rewrite_model_directive_stream_line(self.report_context, line)
            }
            AiSurfaceStreamRewriteState::Standard(state) => {
                transform_standard_line(state, self.report_context, line)
            }
            AiSurfaceStreamRewriteState::OpenAiImage(_)
            | AiSurfaceStreamRewriteState::OpenAiImageToOpenAiChat(_)
            | AiSurfaceStreamRewriteState::OpenAiImageToOpenAiResponses(_)
            | AiSurfaceStreamRewriteState::KiroToClaudeCli(_)
            | AiSurfaceStreamRewriteState::KiroToClaudeCliThenStandard { .. } => Ok(Vec::new()),
        }
    }
}

fn rewrite_model_directive_stream_bytes(
    report_context: &Value,
    bytes: Vec<u8>,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    if model_directive_display_model_from_report_context(report_context).is_none() {
        return Ok(bytes);
    }
    let mut output = Vec::with_capacity(bytes.len());
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        output.extend(rewrite_model_directive_stream_line(
            report_context,
            line.to_vec(),
        )?);
    }
    Ok(output)
}

fn rewrite_model_directive_stream_line(
    report_context: &Value,
    line: Vec<u8>,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    let Some(display_model) = model_directive_display_model_from_report_context(report_context)
    else {
        return Ok(line);
    };
    let text = match std::str::from_utf8(&line) {
        Ok(text) => text,
        Err(_) => return Ok(line),
    };
    let trimmed_line_end = text.trim_end_matches(['\r', '\n']);
    let trailing = &text[trimmed_line_end.len()..];
    let Some((prefix, payload)) = trimmed_line_end.split_once(':') else {
        return Ok(line);
    };
    if prefix.trim() != "data" {
        return Ok(line);
    }
    let payload = payload.trim_start();
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(line);
    }
    let mut value = match serde_json::from_str::<Value>(payload) {
        Ok(value) => value,
        Err(_) => return Ok(line),
    };
    if !rewrite_stream_payload_model(&mut value, &display_model) {
        return Ok(line);
    }
    let mut output = Vec::new();
    output.extend_from_slice(b"data: ");
    output.extend(serde_json::to_vec(&value)?);
    output.extend_from_slice(trailing.as_bytes());
    Ok(output)
}

fn rewrite_stream_payload_model(value: &mut Value, display_model: &str) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for key in ["model", "modelVersion"] {
        if object.get(key).and_then(Value::as_str).is_some() {
            object.insert(key.to_string(), Value::String(display_model.to_string()));
            changed = true;
        }
    }
    for key in ["response", "message"] {
        if let Some(nested) = object.get_mut(key) {
            changed |= rewrite_stream_payload_model(nested, display_model);
        }
    }
    changed
}

fn transform_standard_bytes(
    standard: &mut StreamingStandardFormatMatrix,
    report_context: &Value,
    bytes: Vec<u8>,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        output.extend(transform_standard_line(
            standard,
            report_context,
            line.to_vec(),
        )?);
    }
    Ok(output)
}

fn transform_standard_line(
    standard: &mut StreamingStandardFormatMatrix,
    report_context: &Value,
    line: Vec<u8>,
) -> Result<Vec<u8>, AiSurfaceFinalizeError> {
    let line = if should_unwrap_envelope(report_context) {
        transform_provider_private_stream_line(report_context, line)?
    } else {
        line
    };
    if line.is_empty() {
        return Ok(Vec::new());
    }
    standard.transform_line(report_context, line)
}

fn should_unwrap_envelope(report_context: &Value) -> bool {
    let envelope_name = report_context
        .get("envelope_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let provider_api_format = report_context
        .get("provider_api_format")
        .and_then(Value::as_str)
        .unwrap_or_default();
    provider_adaptation_should_unwrap_stream_envelope(envelope_name, provider_api_format)
}

fn supports_standard_stream_rewrite(provider_api_format: &str, client_api_format: &str) -> bool {
    is_standard_provider_api_format(provider_api_format)
        && (is_standard_chat_client_api_format(client_api_format)
            || is_standard_cli_client_api_format(client_api_format))
}

/// Returns true for OpenAI Responses family formats that share the same SSE
/// wire format and can be passed through without parsing→rebuilding.
fn is_openai_responses_family(api_format: &str) -> bool {
    matches!(
        aether_ai_formats::normalize_api_format_alias(api_format).as_str(),
        "openai:responses" | "openai:responses:compact"
    )
}

/// Returns true when two API formats share the same SSE wire format and
/// can be passed through without parsing→rebuilding.  This covers:
///
/// - Exact matches after normalisation (e.g. `claude:messages` ↔ `claude:messages`)
/// - OpenAI Responses family (`openai:responses` ↔ `openai:responses:compact`)
fn is_same_format_family(provider_format: &str, client_format: &str) -> bool {
    let provider = aether_ai_formats::normalize_api_format_alias(provider_format);
    let client = aether_ai_formats::normalize_api_format_alias(client_format);
    if provider == client {
        return true;
    }
    // OpenAI Responses family shares the same wire format despite having
    // distinct format IDs.
    is_openai_responses_family(provider_format) && is_openai_responses_family(client_format)
}

fn is_standard_provider_api_format(api_format: &str) -> bool {
    matches!(
        aether_ai_formats::normalize_api_format_alias(api_format).as_str(),
        "openai:chat"
            | "openai:responses"
            | "openai:responses:compact"
            | "claude:messages"
            | "gemini:generate_content"
    )
}

fn is_standard_chat_client_api_format(api_format: &str) -> bool {
    matches!(
        api_format,
        "openai:chat" | "claude:messages" | "gemini:generate_content"
    )
}

fn is_standard_cli_client_api_format(api_format: &str) -> bool {
    matches!(
        aether_ai_formats::normalize_api_format_alias(api_format).as_str(),
        "openai:responses"
            | "openai:responses:compact"
            | "claude:messages"
            | "gemini:generate_content"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        maybe_build_ai_surface_stream_rewriter, resolve_finalize_stream_rewrite_mode,
        FinalizeStreamRewriteMode,
    };

    #[test]
    fn resolves_standard_mode_for_cross_format_standard_streams() {
        let report_context = json!({
            "provider_api_format": "claude:messages",
            "client_api_format": "openai:chat",
            "needs_conversion": true,
        });
        assert_eq!(
            resolve_finalize_stream_rewrite_mode(&report_context),
            Some(FinalizeStreamRewriteMode::Standard)
        );
    }

    #[test]
    fn resolves_envelope_unwrap_for_same_format_private_envelopes() {
        let report_context = json!({
            "provider_api_format": "gemini:generate_content",
            "client_api_format": "gemini:generate_content",
            "envelope_name": "antigravity:v1internal",
            "needs_conversion": false,
        });
        assert_eq!(
            resolve_finalize_stream_rewrite_mode(&report_context),
            Some(FinalizeStreamRewriteMode::EnvelopeUnwrap)
        );
    }

    #[test]
    fn resolves_kiro_same_format_streams_to_kiro_mode() {
        let report_context = json!({
            "provider_api_format": "claude:messages",
            "client_api_format": "claude:messages",
            "envelope_name": "kiro:generateAssistantResponse",
            "needs_conversion": false,
        });
        assert_eq!(
            resolve_finalize_stream_rewrite_mode(&report_context),
            Some(FinalizeStreamRewriteMode::KiroToClaudeCli)
        );
    }

    #[test]
    fn rejects_unsupported_non_conversion_streams() {
        let report_context = json!({
            "provider_api_format": "openai:chat",
            "client_api_format": "openai:chat",
            "needs_conversion": false,
        });
        assert_eq!(resolve_finalize_stream_rewrite_mode(&report_context), None);
    }

    #[test]
    fn resolves_model_directive_display_mode_for_same_format_standard_streams() {
        let report_context = json!({
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:responses",
            "model": "gpt-5.5-xhigh",
            "mapped_model": "gpt-5.5",
            "needs_conversion": false,
        });
        assert_eq!(
            resolve_finalize_stream_rewrite_mode(&report_context),
            Some(FinalizeStreamRewriteMode::ModelDirectiveDisplay)
        );
    }

    #[test]
    fn model_directive_display_mode_does_not_displace_kiro_stream_bridge() {
        let report_context = json!({
            "provider_api_format": "claude:messages",
            "client_api_format": "claude:messages",
            "envelope_name": "kiro:generateAssistantResponse",
            "model": "claude-sonnet-4.5-high",
            "mapped_model": "claude-sonnet-4.5",
            "needs_conversion": false,
        });
        assert_eq!(
            resolve_finalize_stream_rewrite_mode(&report_context),
            Some(FinalizeStreamRewriteMode::KiroToClaudeCli)
        );
    }

    #[test]
    fn envelope_unwrap_rewriter_restores_model_directive_display_model() {
        let report_context = json!({
            "provider_api_format": "gemini:generate_content",
            "client_api_format": "gemini:generate_content",
            "envelope_name": "gemini_cli:v1internal",
            "model": "gemini-2.5-pro-high",
            "mapped_model": "gemini-2.5-pro",
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("rewriter should exist");
        let output = rewriter
            .push_chunk(
                b"data: {\"response\":{\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[]}}\n\n",
            )
            .expect("rewrite should succeed");
        let output = String::from_utf8(output).expect("output should be utf8");

        assert!(output.contains("\"modelVersion\":\"gemini-2.5-pro-high\""));
        assert!(!output.contains("\"modelVersion\":\"gemini-2.5-pro\""));
    }

    #[test]
    fn model_directive_display_rewriter_restores_response_model() {
        let report_context = json!({
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:responses",
            "model": "gpt-5.5-xhigh",
            "mapped_model": "gpt-5.5",
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("rewriter should exist");
        let output = rewriter
            .push_chunk(
                b"event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\",\"object\":\"response\",\"model\":\"gpt-5.5\",\"status\":\"in_progress\"}}\n\n",
            )
            .expect("rewrite should succeed");
        let output = String::from_utf8(output).expect("output should be utf8");

        assert!(output.contains("event: response.created"));
        assert!(output.contains("\"model\":\"gpt-5.5-xhigh\""));
        assert!(!output.contains("\"model\":\"gpt-5.5\""));
    }

    #[test]
    fn standard_rewriter_converts_openai_responses_reasoning_delta_to_chat() {
        let report_context = json!({
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:chat",
            "needs_conversion": true,
            "mapped_model": "gpt-5.4",
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("rewriter should exist");
        let output = rewriter
            .push_chunk(
                b"event: response.reasoning_summary_text.delta\n\
data: {\"type\":\"response.reasoning_summary_text.delta\",\"response_id\":\"resp_reasoning_stream_123\",\"item_id\":\"rs_123\",\"output_index\":0,\"summary_index\":0,\"delta\":\"Need to inspect first.\"}\n\n",
            )
            .expect("rewrite should succeed");
        let output = String::from_utf8(output).expect("output should be utf8");

        assert!(output.contains("\"object\":\"chat.completion.chunk\""));
        assert!(output.contains("\"reasoning_content\":\"Need to inspect first.\""));
        assert!(!output.contains("\"content\""));
        assert!(!output.contains("data: [DONE]"));
    }

    #[test]
    fn same_family_responses_passthrough_preserves_encrypted_content() {
        // When provider and client are both OpenAI Responses family,
        // the stream should pass through verbatim (only model name rewrite).
        // This preserves encrypted_content, original item IDs, etc.
        let report_context = json!({
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:responses:compact",
            "needs_conversion": true,
            "model": "gpt-5.5-xhigh",
            "mapped_model": "gpt-5.5",
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("rewriter should exist");
        let output = rewriter
            .push_chunk(
                b"event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"response_id\":\"resp_123\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_abc\",\"summary\":[],\"encrypted_content\":\"EWxvY2tlZENvbnRlbnQ=\"}}\n\n",
            )
            .expect("rewrite should succeed");
        let output = String::from_utf8(output).expect("output should be utf8");

        // Passthrough preserves the full payload structure
        assert!(output.contains("event: response.output_item.added"));
        assert!(output.contains("\"encrypted_content\":\"EWxvY2tlZENvbnRlbnQ=\""));
        assert!(output.contains("\"id\":\"rs_abc\""));
        assert!(output.contains("\"type\":\"reasoning\""));
    }

    #[test]
    fn same_family_responses_without_display_model_passes_through_verbatim() {
        // When provider and client are both OpenAI Responses family but
        // there is no display model override, the rewriter returns None
        // (complete passthrough, no interception at all).
        let report_context = json!({
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:responses:compact",
            "needs_conversion": true,
        });
        assert!(maybe_build_ai_surface_stream_rewriter(Some(&report_context)).is_none());
    }

    #[test]
    fn same_format_claude_passthrough_with_display_model() {
        // Claude→Claude with needs_conversion=true should pass through
        // (only model name rewrite), not parse→rebuild.
        let report_context = json!({
            "provider_api_format": "claude:messages",
            "client_api_format": "claude:messages",
            "needs_conversion": true,
            "model": "claude-sonnet-4.5-high",
            "mapped_model": "claude-sonnet-4.5",
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("rewriter should exist");
        let output = rewriter
            .push_chunk(
                b"event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me reason...\"}}\n\n",
            )
            .expect("rewrite should succeed");
        let output = String::from_utf8(output).expect("output should be utf8");

        // Passthrough preserves the exact wire format
        assert!(output.contains("event: content_block_delta"));
        assert!(output.contains("\"thinking\":\"Let me reason...\""));
        assert!(output.contains("\"type\":\"thinking_delta\""));
    }

    #[test]
    fn same_format_claude_without_display_model_passes_through_verbatim() {
        // Claude→Claude without display model: no rewriter needed at all.
        let report_context = json!({
            "provider_api_format": "claude:messages",
            "client_api_format": "claude:messages",
            "needs_conversion": true,
        });
        assert!(maybe_build_ai_surface_stream_rewriter(Some(&report_context)).is_none());
    }

    #[test]
    fn same_format_gemini_passthrough_with_display_model() {
        // Gemini→Gemini with needs_conversion=true should pass through.
        let report_context = json!({
            "provider_api_format": "gemini:generate_content",
            "client_api_format": "gemini:generate_content",
            "needs_conversion": true,
            "model": "gemini-2.5-pro-high",
            "mapped_model": "gemini-2.5-pro",
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("rewriter should exist");
        let output = rewriter
            .push_chunk(
                b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}],\"role\":\"model\"}}],\"modelVersion\":\"gemini-2.5-pro\"}\n\n",
            )
            .expect("rewrite should succeed");
        let output = String::from_utf8(output).expect("output should be utf8");

        // Model version should be rewritten
        assert!(output.contains("\"modelVersion\":\"gemini-2.5-pro-high\""));
        assert!(!output.contains("\"modelVersion\":\"gemini-2.5-pro\""));
    }

    #[test]
    fn same_format_gemini_without_display_model_passes_through_verbatim() {
        // Gemini→Gemini without display model: no rewriter needed.
        let report_context = json!({
            "provider_api_format": "gemini:generate_content",
            "client_api_format": "gemini:generate_content",
            "needs_conversion": true,
        });
        assert!(maybe_build_ai_surface_stream_rewriter(Some(&report_context)).is_none());
    }

    #[test]
    fn resolves_openai_image_mode_for_same_format_image_streams() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:image",
            "needs_conversion": false,
        });
        assert_eq!(
            resolve_finalize_stream_rewrite_mode(&report_context),
            Some(FinalizeStreamRewriteMode::OpenAiImage)
        );
    }

    #[test]
    fn same_format_openai_image_passthrough_skips_response_rewrite() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:image",
            "needs_conversion": false,
            "openai_image_transport_mode": "images_passthrough",
        });

        assert_eq!(resolve_finalize_stream_rewrite_mode(&report_context), None);
        assert!(maybe_build_ai_surface_stream_rewriter(Some(&report_context)).is_none());
    }

    #[test]
    fn rewrites_openai_image_stream_to_openai_chat_final_chunk() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:chat",
            "mapped_model": "gpt-image-2",
            "request_id": "trace-image-chat-stream",
            "needs_conversion": false,
        });
        assert_eq!(
            resolve_finalize_stream_rewrite_mode(&report_context),
            Some(FinalizeStreamRewriteMode::OpenAiImageToOpenAiChat)
        );
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("image to chat stream rewriter should exist");

        let progress = rewriter
            .push_chunk(
                br#"event: response.image_generation_call.partial_image
data: {"type":"response.image_generation_call.partial_image","partial_image_b64":"cGFydGlhbA=="}

"#,
            )
            .expect("partial image should rewrite as progress");
        let progress_text = String::from_utf8(progress).expect("progress output should be utf8");
        assert!(progress_text.contains("\"object\":\"chat.completion.chunk\""));
        assert!(!progress_text.contains("cGFydGlhbA=="));

        let output_item = rewriter
            .push_chunk(
                br#"event: response.output_item.done
data: {"type":"response.output_item.done","item":{"type":"image_generation_call","id":"ig_1","result":"aGVsbG8=","output_format":"png"}}

"#,
            )
            .expect("output item should rewrite");
        let output_item_text = String::from_utf8(output_item).expect("output item should be utf8");
        assert!(output_item_text.is_empty());

        let final_output = rewriter
            .push_chunk(
                br#"event: response.completed
data: {"type":"response.completed","response":{"id":"resp_123","model":"gpt-image-2","tool_usage":{"image_gen":{"total_tokens":0}},"output":[]}}

"#,
            )
            .expect("completed event should rewrite");
        let final_text = String::from_utf8(final_output).expect("final output should be utf8");
        assert!(final_text.contains("\"object\":\"chat.completion.chunk\""));
        assert!(final_text.contains("![generated image](data:image/png;base64,aGVsbG8=)"));
        assert!(final_text.contains("data: [DONE]"));
        assert!(!final_text.contains("image_generation.completed"));
    }

    #[test]
    fn rewrites_openai_image_stream_to_codex_native_image_event() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:responses",
            "mapped_model": "gpt-image-2",
            "request_id": "trace-image-responses-stream",
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("image to responses stream rewriter should exist");

        let output_item = rewriter
            .push_chunk(
                br#"event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"type":"image_generation_call","id":"ig_1","status":"completed","result":"aGVsbG8=","output_format":"png"}}

"#,
            )
            .expect("output item should rewrite");
        assert!(output_item.is_empty());

        let output = rewriter
            .push_chunk(
                br#"event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[{"type":"image_generation_call","id":"ig_1","status":"completed","result":"aGVsbG8="}]}}

"#,
            )
            .expect("completed response should release visible image");
        let output_text = String::from_utf8(output).expect("output should be utf8");

        assert!(output_text.contains("\"type\":\"image_generation_call\""));
        assert!(!output_text.contains("\"type\":\"message\""));
        assert!(!output_text.contains("\"type\":\"output_text\""));
        assert!(!output_text.contains("data:image/"));
    }

    #[test]
    fn rewrites_same_format_responses_hosted_image_to_native_output() {
        let report_context = json!({
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:responses",
            "codex_hosted_image_generation": true,
            "needs_conversion": false,
        });
        assert_eq!(
            resolve_finalize_stream_rewrite_mode(&report_context),
            Some(FinalizeStreamRewriteMode::OpenAiImageToOpenAiResponses)
        );
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("hosted image response rewriter should exist");

        let output_item = rewriter
            .push_chunk(
                br#"event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"type":"image_generation_call","id":"ig_native","status":"generating","result":"aGVsbG8=","output_format":"png"}}

"#,
            )
            .expect("native image should buffer");
        assert!(output_item.is_empty());

        let output = rewriter
            .push_chunk(
                br#"event: response.completed
data: {"type":"response.completed","response":{"id":"resp_native","status":"completed","output":[]}}

"#,
            )
            .expect("completed response should release native image");
        let output_text = String::from_utf8(output).expect("output should be utf8");

        assert!(output_text.contains("\"type\":\"image_generation_call\""));
        assert!(output_text.contains("\"output\":[{"));
        assert!(output_text.contains("\"id\":\"ig_native\""));
        assert!(output_text.contains("\"status\":\"completed\""));
        assert!(!output_text.contains("\"status\":\"generating\""));
        assert!(!output_text.contains("\"type\":\"message\""));
        assert!(!output_text.contains("data:image/"));
    }

    #[test]
    fn same_format_responses_hosted_image_rewriter_preserves_text_events() {
        let report_context = json!({
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:responses",
            "codex_hosted_image_generation": true,
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("hosted image response rewriter should exist");
        let event = br#"event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"plain text"}

"#;

        let output = rewriter
            .push_chunk(event)
            .expect("text event should pass through");

        assert_eq!(output, event);
    }

    #[test]
    fn same_format_responses_hosted_image_rewriter_preserves_display_model() {
        let report_context = json!({
            "provider_api_format": "openai:responses",
            "client_api_format": "openai:responses",
            "codex_hosted_image_generation": true,
            "model": "gpt-5.6-terra-high",
            "mapped_model": "gpt-5.6-terra",
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("hosted image response rewriter should exist");

        let output = rewriter
            .push_chunk(
                br#"event: response.created
data: {"type":"response.created","response":{"id":"resp_123","model":"gpt-5.6-terra","status":"in_progress"}}

"#,
            )
            .expect("created event should pass through");
        let output = String::from_utf8(output).expect("output should be utf8");

        assert!(output.contains("\"model\":\"gpt-5.6-terra-high\""));
        assert!(!output.contains("\"model\":\"gpt-5.6-terra\""));
    }

    #[test]
    fn openai_image_responses_failure_discards_pending_visible_image() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:responses",
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("image to responses stream rewriter should exist");

        let output_item = rewriter
            .push_chunk(
                br#"event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"type":"image_generation_call","id":"ig_1","status":"completed","result":"aGVsbG8=","output_format":"png"}}

"#,
            )
            .expect("output item should buffer");
        assert!(output_item.is_empty());

        let failed = rewriter
            .push_chunk(
                br#"event: response.failed
data: {"type":"response.failed","response":{"status":"failed","error":{"message":"generation failed"}}}

"#,
            )
            .expect("failure should pass through");
        let failed_text = String::from_utf8(failed).expect("failure should be utf8");
        assert!(failed_text.contains("response.failed"));
        assert!(!failed_text.contains("\"type\":\"message\""));
        assert!(!failed_text.contains("data:image/"));
    }

    #[test]
    fn openai_image_responses_multi_image_events_remain_native() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:responses",
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("image to responses stream rewriter should exist");

        for (index, id, result) in [(0, "ig_1", "aGVsbG8="), (1, "ig_2", "d29ybGQ=")] {
            let block = format!(
                "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"output_index\":{index},\"item\":{{\"type\":\"image_generation_call\",\"id\":\"{id}\",\"status\":\"completed\",\"result\":\"{result}\",\"output_format\":\"png\"}}}}\n\n"
            );
            assert!(rewriter
                .push_chunk(block.as_bytes())
                .expect("image should buffer")
                .is_empty());
        }

        let completed = rewriter
            .push_chunk(
                br#"event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[{"type":"image_generation_call","id":"ig_1"},{"type":"image_generation_call","id":"ig_2"}]}}

"#,
            )
            .expect("completion should release both images");
        let completed_text = String::from_utf8(completed).expect("output should be utf8");
        let events = completed_text
            .split("\n\n")
            .filter_map(|block| block.lines().find_map(|line| line.strip_prefix("data: ")))
            .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
            .collect::<Vec<_>>();
        let image_events = events
            .iter()
            .filter(|event| event["item"]["type"] == "image_generation_call")
            .count();
        assert_eq!(image_events, 2);
        assert!(!completed_text.contains("\"type\":\"message\""));
        assert!(!completed_text.contains("data:image/"));
        let completed_event = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .expect("completed event should remain");
        assert_eq!(
            completed_event["response"]["output"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn openai_image_responses_eof_does_not_emit_visible_image_message() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:responses",
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("image to responses stream rewriter should exist");
        let output_item = rewriter
            .push_chunk(
                b"event: response.output_item.done\r\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_1\",\"status\":\"completed\",\"result\":\"aGVsbG8=\",\"output_format\":\"png\"}}\r\n\r\n",
            )
            .expect("output item should buffer");
        assert!(output_item.is_empty());

        let eof_output = rewriter.finish().expect("EOF should flush native item");
        let eof_text = String::from_utf8(eof_output).expect("output should be utf8");
        assert!(eof_text.contains("image_generation_call"));
        assert!(!eof_text.contains("\"type\":\"message\""));
        assert!(!eof_text.contains("data:image/"));
    }

    #[test]
    fn openai_image_responses_progress_does_not_emit_visible_image_message() {
        let report_context = json!({
            "provider_api_format": "openai:image",
            "client_api_format": "openai:responses",
            "mapped_model": "gpt-image-2",
            "request_id": "trace-image-responses-progress",
            "needs_conversion": false,
        });
        let mut rewriter = maybe_build_ai_surface_stream_rewriter(Some(&report_context))
            .expect("image to responses stream rewriter should exist");

        let output = rewriter
            .push_chunk(
                br#"event: response.image_generation_call.generating
data: {"type":"response.image_generation_call.generating","item_id":"ig_1"}

"#,
            )
            .expect("progress should pass through");
        let output_text = String::from_utf8(output).expect("output should be utf8");

        assert!(output_text.contains("response.image_generation_call.generating"));
        assert!(!output_text.contains("\"type\":\"message\""));
        assert!(!output_text.contains("data:image/"));
    }
}

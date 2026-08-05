use std::collections::BTreeMap;
use std::sync::Arc;

use aether_contracts::ResolvedTransportProfile;
use serde_json::Value;

use crate::ai_serving::planner::candidate_preparation::{
    prepare_header_authenticated_candidate, OauthPreparationContext,
};
use crate::ai_serving::planner::spec_metadata::local_openai_image_spec_metadata;
use crate::ai_serving::planner::standard::codex_openai_image_bridge_model_from_provider_config;
use crate::ai_serving::pure::normalize_openai_image_request_with_options;
use crate::ai_serving::transport::{
    apply_local_header_rules_with_request_headers,
    apply_standard_provider_request_body_rules_with_request_headers, body_rules_have_enabled_rules,
    build_grok_browser_headers, build_grok_upstream_url, build_openai_image_headers,
    build_openai_image_upstream_url, build_passthrough_headers,
    build_standard_provider_request_headers, ensure_upstream_auth_header,
    openai_image_transport_unsupported_reason, resolve_openai_image_auth, GrokHeaderInput,
    ProviderOpenAiImageHeadersInput, StandardProviderRequestHeadersInput, GROK_CHAT_PATH,
};
use crate::ai_serving::{
    apply_codex_openai_responses_special_body_edits_with_bridge_model,
    apply_codex_openai_responses_special_headers, build_chatgpt_web_image_request_body,
    build_gemini_image_request_body_from_openai_image_request,
    build_openai_image_provider_request_body, default_model_for_openai_image_operation,
    normalize_openai_image_request, request_conversion_direct_auth, CandidateFailureDiagnostic,
    GatewayProviderTransportSnapshot, PlannerAppState, RequestConversionKind,
};
use crate::image_capabilities::openai_image_normalize_options_for_provider;
use crate::AppState;

use super::support::{
    mark_skipped_local_openai_image_candidate,
    mark_skipped_local_openai_image_candidate_with_failure_diagnostic,
    LocalOpenAiImageCandidateAttempt, LocalOpenAiImageDecisionInput,
};
use super::LocalOpenAiImageSpec;

pub(super) use crate::ai_serving::resolve_requested_openai_image_model_for_request as resolve_requested_image_model_for_request;

pub(super) struct LocalOpenAiImageCandidatePayloadParts {
    pub(super) transport: Arc<GatewayProviderTransportSnapshot>,
    pub(super) auth_header: String,
    pub(super) auth_value: String,
    pub(super) requested_model: String,
    pub(super) mapped_model: String,
    pub(super) provider_api_format: String,
    pub(super) provider_request_headers: BTreeMap<String, String>,
    pub(super) provider_request_body: Option<Value>,
    pub(super) provider_request_body_base64: Option<String>,
    pub(super) content_type: Option<String>,
    pub(super) upstream_url: String,
    pub(super) input_summary: Value,
    pub(super) transport_profile: Option<ResolvedTransportProfile>,
}

const OPENAI_IMAGE_TRANSPORT_MODE_CONFIG_KEY: &str = "openai_image_transport_mode";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAiImageTransportMode {
    ResponsesBridge,
    ImagesPassthrough,
}

fn resolve_openai_image_transport_mode(
    transport: &GatewayProviderTransportSnapshot,
) -> OpenAiImageTransportMode {
    let mode = transport
        .endpoint
        .config
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|config| config.get(OPENAI_IMAGE_TRANSPORT_MODE_CONFIG_KEY))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if mode.eq_ignore_ascii_case("images_passthrough") {
        OpenAiImageTransportMode::ImagesPassthrough
    } else {
        OpenAiImageTransportMode::ResponsesBridge
    }
}

pub(crate) fn openai_image_uses_images_passthrough(
    transport: &GatewayProviderTransportSnapshot,
) -> bool {
    resolve_openai_image_transport_mode(transport) == OpenAiImageTransportMode::ImagesPassthrough
}

pub(crate) fn build_openai_image_upstream_url_for_request(
    transport: &GatewayProviderTransportSnapshot,
    request_path: &str,
    request_query: Option<&str>,
) -> Option<String> {
    if resolve_openai_image_transport_mode(transport) == OpenAiImageTransportMode::ResponsesBridge {
        return Some(build_openai_image_upstream_url(transport, request_query));
    }
    let custom_path = transport
        .endpoint
        .custom_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    crate::ai_serving::transport::url::build_passthrough_path_url(
        &transport.endpoint.base_url,
        custom_path.unwrap_or(request_path),
        request_query,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_openai_image_passthrough_headers(
    headers: &http::HeaderMap,
    auth_header: &str,
    auth_value: &str,
    header_rules: Option<&Value>,
    provider_request_body: &Value,
    original_request_body: &Value,
    content_type: Option<&str>,
) -> Option<BTreeMap<String, String>> {
    let mut provider_request_headers =
        build_passthrough_headers(headers, &BTreeMap::new(), content_type);
    ensure_upstream_auth_header(&mut provider_request_headers, auth_header, auth_value);
    provider_request_headers
        .entry("accept".to_string())
        .or_insert_with(|| "application/json".to_string());
    if !apply_local_header_rules_with_request_headers(
        &mut provider_request_headers,
        header_rules,
        &[auth_header, "content-type", "accept"],
        provider_request_body,
        Some(original_request_body),
        Some(headers),
    ) {
        return None;
    }
    Some(provider_request_headers)
}

pub(crate) fn build_openai_image_passthrough_json_body(
    body_json: &Value,
    mapped_model: &str,
    body_rules: Option<&Value>,
    request_headers: &http::HeaderMap,
) -> Option<Value> {
    build_openai_image_passthrough_body_parts(
        body_json,
        None,
        mapped_model,
        body_rules,
        request_headers,
    )
    .ok()?
    .provider_request_body
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAiImagePassthroughBodyError {
    BodyRulesUnsupportedForBinary,
    BodyRulesApplyFailed,
}

#[derive(Debug, Clone, PartialEq)]
struct OpenAiImagePassthroughBodyParts {
    provider_request_body: Option<Value>,
    provider_request_body_base64: Option<String>,
}

fn build_openai_image_passthrough_body_parts(
    body_json: &Value,
    body_base64: Option<&str>,
    mapped_model: &str,
    body_rules: Option<&Value>,
    request_headers: &http::HeaderMap,
) -> Result<OpenAiImagePassthroughBodyParts, OpenAiImagePassthroughBodyError> {
    if let Some(encoded_body) = body_base64.map(str::trim).filter(|value| !value.is_empty()) {
        if body_rules_have_enabled_rules(body_rules) {
            return Err(OpenAiImagePassthroughBodyError::BodyRulesUnsupportedForBinary);
        }
        return Ok(OpenAiImagePassthroughBodyParts {
            provider_request_body: None,
            provider_request_body_base64: Some(encoded_body.to_string()),
        });
    }

    let mut passthrough_body = body_json.clone();
    if let Some(object) = passthrough_body.as_object_mut() {
        object.insert("model".to_string(), Value::String(mapped_model.to_string()));
    }
    let Some(passthrough_body) = apply_standard_provider_request_body_rules_with_request_headers(
        passthrough_body,
        body_rules,
        body_json,
        request_headers,
    ) else {
        return Err(OpenAiImagePassthroughBodyError::BodyRulesApplyFailed);
    };
    Ok(OpenAiImagePassthroughBodyParts {
        provider_request_body: Some(passthrough_body),
        provider_request_body_base64: None,
    })
}

pub(crate) fn apply_openai_image_tool_model(provider_request_body: &mut Value, mapped_model: &str) {
    let mapped_model = mapped_model.trim();
    if mapped_model.is_empty() {
        return;
    }
    let Some(tool) = provider_request_body
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .and_then(|tools| tools.first_mut())
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    tool.insert("model".to_string(), Value::String(mapped_model.to_string()));
}

pub(super) async fn resolve_local_openai_image_candidate_payload_parts(
    state: &AppState,
    parts: &http::request::Parts,
    body_json: &Value,
    body_base64: Option<&str>,
    trace_id: &str,
    input: &LocalOpenAiImageDecisionInput,
    attempt: &LocalOpenAiImageCandidateAttempt,
    spec: LocalOpenAiImageSpec,
) -> Option<LocalOpenAiImageCandidatePayloadParts> {
    let spec_metadata = local_openai_image_spec_metadata(spec);
    let candidate = &attempt.eligible.candidate;
    let transport = &attempt.eligible.transport;
    let provider_api_format = attempt.eligible.provider_api_format.as_str();
    let effective_headers = input.effective_headers(&parts.headers);

    if provider_api_format == "gemini:generate_content" {
        return resolve_local_openai_image_to_gemini_candidate_payload_parts(
            state,
            parts,
            body_json,
            body_base64,
            trace_id,
            input,
            attempt,
            spec,
        )
        .await;
    }

    if let Some(skip_reason) =
        openai_image_transport_unsupported_reason(transport, spec_metadata.api_format)
    {
        mark_skipped_local_openai_image_candidate(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            skip_reason,
        )
        .await;
        return None;
    }
    let prepared_candidate = match prepare_header_authenticated_candidate(
        PlannerAppState::new(state),
        transport,
        candidate,
        resolve_openai_image_auth(transport),
        OauthPreparationContext {
            trace_id,
            api_format: spec_metadata.api_format,
            operation: "openai_image_candidate_request",
        },
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(skip_reason) => {
            mark_skipped_local_openai_image_candidate(
                state,
                input,
                trace_id,
                candidate,
                attempt.candidate_index,
                &attempt.candidate_id,
                skip_reason,
            )
            .await;
            return None;
        }
    };
    let auth_header = prepared_candidate.auth_header;
    let auth_value = prepared_candidate.auth_value;

    let normalized_request = normalize_openai_image_request_with_options(
        parts,
        body_json,
        body_base64,
        openai_image_normalize_options_for_provider(&transport.provider.provider_type),
    );
    let Some(normalized_request) = normalized_request else {
        mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "provider_request_body_missing",
            CandidateFailureDiagnostic::provider_request_body_missing(
                spec_metadata.api_format,
                spec_metadata.api_format,
                "openai_image_request_normalize",
            ),
        )
        .await;
        return None;
    };

    let is_chatgpt_web = transport
        .provider
        .provider_type
        .trim()
        .eq_ignore_ascii_case("chatgpt_web");
    let is_grok = transport
        .provider
        .provider_type
        .trim()
        .eq_ignore_ascii_case("grok");
    let image_transport_mode = if is_chatgpt_web || is_grok {
        OpenAiImageTransportMode::ResponsesBridge
    } else {
        resolve_openai_image_transport_mode(transport)
    };
    let images_passthrough = image_transport_mode == OpenAiImageTransportMode::ImagesPassthrough;
    let original_content_type = effective_headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let transport_profile = crate::ai_serving::transport::resolve_transport_profile(transport);
    let upstream_url = if is_chatgpt_web {
        chatgpt_web_image_internal_url(&transport.endpoint.base_url)
    } else if is_grok {
        build_grok_upstream_url(transport, GROK_CHAT_PATH)
    } else {
        let Some(url) = build_openai_image_upstream_url_for_request(
            transport,
            parts.uri.path(),
            parts.uri.query(),
        ) else {
            mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
                state,
                input,
                trace_id,
                candidate,
                attempt.candidate_index,
                &attempt.candidate_id,
                "upstream_url_missing",
                CandidateFailureDiagnostic::upstream_url_missing(
                    spec_metadata.api_format,
                    spec_metadata.api_format,
                    "openai_image_passthrough_url",
                ),
            )
            .await;
            return None;
        };
        url
    };
    let mut provider_request_body = if is_chatgpt_web {
        match build_chatgpt_web_image_request_body(parts, body_json, body_base64) {
            Ok(body) => body,
            Err(err) => err.to_error_json(),
        }
    } else if images_passthrough {
        Value::Null
    } else {
        build_openai_image_provider_request_body(&normalized_request)
    };
    if !is_chatgpt_web && !is_grok && !images_passthrough {
        apply_openai_image_tool_model(
            &mut provider_request_body,
            prepared_candidate.mapped_model.as_str(),
        );
    }
    if !is_chatgpt_web && !images_passthrough {
        apply_codex_openai_responses_special_body_edits_with_bridge_model(
            &mut provider_request_body,
            transport.provider.provider_type.as_str(),
            spec_metadata.api_format,
            transport.endpoint.body_rules.as_ref(),
            Some(candidate.key_id.as_str()),
            codex_openai_image_bridge_model_from_provider_config(
                transport.provider.config.as_ref(),
            ),
        );
    }

    let (provider_request_body, provider_request_body_base64, content_type) = if images_passthrough
    {
        match build_openai_image_passthrough_body_parts(
            body_json,
            body_base64,
            prepared_candidate.mapped_model.as_str(),
            transport.endpoint.body_rules.as_ref(),
            effective_headers,
        ) {
            Ok(body_parts) => {
                let is_binary = body_parts.provider_request_body_base64.is_some();
                (
                    body_parts.provider_request_body,
                    body_parts.provider_request_body_base64,
                    if is_binary {
                        original_content_type.clone()
                    } else {
                        Some("application/json".to_string())
                    },
                )
            }
            Err(OpenAiImagePassthroughBodyError::BodyRulesUnsupportedForBinary) => {
                mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
                    state,
                    input,
                    trace_id,
                    candidate,
                    attempt.candidate_index,
                    &attempt.candidate_id,
                    "transport_body_rules_unsupported_for_binary_upload",
                    CandidateFailureDiagnostic::body_rules_unsupported_for_binary_upload(
                        spec_metadata.api_format,
                        spec_metadata.api_format,
                        "openai_image_multipart_passthrough",
                    ),
                )
                .await;
                return None;
            }
            Err(OpenAiImagePassthroughBodyError::BodyRulesApplyFailed) => {
                mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
                    state,
                    input,
                    trace_id,
                    candidate,
                    attempt.candidate_index,
                    &attempt.candidate_id,
                    "transport_body_rules_apply_failed",
                    CandidateFailureDiagnostic::body_rules_apply_failed(
                        spec_metadata.api_format,
                        spec_metadata.api_format,
                        "openai_image_passthrough_body_rules",
                    ),
                )
                .await;
                return None;
            }
        }
    } else {
        (
            Some(provider_request_body),
            None,
            Some("application/json".to_string()),
        )
    };
    let provider_request_body_for_rules = provider_request_body.as_ref().unwrap_or(body_json);

    let Some(mut provider_request_headers) = (if is_grok {
        build_grok_browser_headers(GrokHeaderInput {
            transport,
            transport_profile: transport_profile.as_ref(),
            request_headers: Some(effective_headers),
            content_type: "application/json",
            accept: "*/*",
            header_rules: transport.endpoint.header_rules.as_ref(),
            provider_request_body: provider_request_body_for_rules,
            original_request_body: body_json,
        })
    } else if images_passthrough {
        build_openai_image_passthrough_headers(
            effective_headers,
            &auth_header,
            &auth_value,
            transport.endpoint.header_rules.as_ref(),
            provider_request_body_for_rules,
            body_json,
            content_type.as_deref(),
        )
    } else {
        build_openai_image_headers(ProviderOpenAiImageHeadersInput {
            headers: effective_headers,
            auth_header: &auth_header,
            auth_value: &auth_value,
            header_rules: transport.endpoint.header_rules.as_ref(),
            provider_request_body: provider_request_body_for_rules,
            original_request_body: body_json,
        })
    }) else {
        mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "transport_header_rules_apply_failed",
            CandidateFailureDiagnostic::header_rules_apply_failed(
                spec_metadata.api_format,
                spec_metadata.api_format,
                "openai_image_header_rules",
            ),
        )
        .await;
        return None;
    };
    if is_chatgpt_web {
        provider_request_headers.insert("x-aether-chatgpt-web-image".to_string(), "1".to_string());
    } else if is_grok {
    } else if !images_passthrough {
        apply_codex_openai_responses_special_headers(
            &mut provider_request_headers,
            provider_request_body_for_rules,
            effective_headers,
            transport.provider.provider_type.as_str(),
            spec_metadata.api_format,
            Some(trace_id),
            transport.key.decrypted_auth_config.as_deref(),
        );
    }
    let requested_model = normalized_request
        .requested_model
        .clone()
        .unwrap_or_else(|| {
            default_model_for_openai_image_operation(normalized_request.operation).to_string()
        });
    let mapped_model = prepared_candidate.mapped_model;

    let input_summary = if is_chatgpt_web || is_grok {
        provider_request_body.clone().unwrap_or(Value::Null)
    } else {
        normalized_request.summary_json
    };

    Some(LocalOpenAiImageCandidatePayloadParts {
        transport: Arc::clone(transport),
        auth_header,
        auth_value,
        requested_model,
        mapped_model,
        provider_api_format: spec_metadata.api_format.to_string(),
        provider_request_headers,
        provider_request_body,
        provider_request_body_base64,
        content_type,
        upstream_url,
        input_summary,
        transport_profile,
    })
}

async fn resolve_local_openai_image_to_gemini_candidate_payload_parts(
    state: &AppState,
    parts: &http::request::Parts,
    body_json: &Value,
    body_base64: Option<&str>,
    trace_id: &str,
    input: &LocalOpenAiImageDecisionInput,
    attempt: &LocalOpenAiImageCandidateAttempt,
    spec: LocalOpenAiImageSpec,
) -> Option<LocalOpenAiImageCandidatePayloadParts> {
    let spec_metadata = local_openai_image_spec_metadata(spec);
    let candidate = &attempt.eligible.candidate;
    let transport = &attempt.eligible.transport;
    let provider_api_format = "gemini:generate_content";
    let effective_headers = input.effective_headers(&parts.headers);

    let prepared_candidate = match prepare_header_authenticated_candidate(
        PlannerAppState::new(state),
        transport,
        candidate,
        request_conversion_direct_auth(transport, RequestConversionKind::ToGeminiStandard),
        OauthPreparationContext {
            trace_id,
            api_format: provider_api_format,
            operation: "openai_image_to_gemini_candidate_request",
        },
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(skip_reason) => {
            mark_skipped_local_openai_image_candidate(
                state,
                input,
                trace_id,
                candidate,
                attempt.candidate_index,
                &attempt.candidate_id,
                skip_reason,
            )
            .await;
            return None;
        }
    };

    let Some(normalized_request) = normalize_openai_image_request(parts, body_json, body_base64)
    else {
        mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "provider_request_body_missing",
            CandidateFailureDiagnostic::provider_request_body_missing(
                spec_metadata.api_format,
                provider_api_format,
                "openai_image_request_normalize",
            ),
        )
        .await;
        return None;
    };

    let Some(mut converted) = build_gemini_image_request_body_from_openai_image_request(
        &normalized_request,
        &prepared_candidate.mapped_model,
    ) else {
        mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "provider_request_body_missing",
            CandidateFailureDiagnostic::provider_request_body_missing(
                spec_metadata.api_format,
                provider_api_format,
                "openai_image_to_gemini_request_body",
            ),
        )
        .await;
        return None;
    };
    converted.body_json =
        match crate::ai_serving::transport::apply_standard_provider_request_body_rules_with_request_headers(
            converted.body_json,
            transport.endpoint.body_rules.as_ref(),
            body_json,
            effective_headers,
        ) {
            Some(body) => body,
            None => {
                mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
                    state,
                    input,
                    trace_id,
                    candidate,
                    attempt.candidate_index,
                    &attempt.candidate_id,
                    "provider_request_body_missing",
                    CandidateFailureDiagnostic::provider_request_body_missing(
                        spec_metadata.api_format,
                        provider_api_format,
                        "openai_image_to_gemini_body_rules",
                    ),
                )
                .await;
                return None;
            }
        };
    let upstream_is_stream = spec_metadata.require_streaming;
    let Some(upstream_url) = crate::ai_serving::planner::standard::build_standard_upstream_url(
        parts,
        transport,
        &converted.mapped_model,
        provider_api_format,
        upstream_is_stream,
        Some(&converted.body_json),
    ) else {
        mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "upstream_url_missing",
            CandidateFailureDiagnostic::upstream_url_missing(
                spec_metadata.api_format,
                provider_api_format,
                "openai_image_to_gemini_url",
            ),
        )
        .await;
        return None;
    };
    let Some(resolved_headers) =
        build_standard_provider_request_headers(StandardProviderRequestHeadersInput {
            transport,
            provider_api_format,
            same_format: false,
            headers: effective_headers,
            auth_header: &prepared_candidate.auth_header,
            auth_value: &prepared_candidate.auth_value,
            extra_headers: &BTreeMap::new(),
            header_rules: transport.endpoint.header_rules.as_ref(),
            provider_request_body: &converted.body_json,
            original_request_body: body_json,
            upstream_is_stream,
        })
    else {
        mark_skipped_local_openai_image_candidate_with_failure_diagnostic(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "transport_header_rules_apply_failed",
            CandidateFailureDiagnostic::header_rules_apply_failed(
                spec_metadata.api_format,
                provider_api_format,
                "openai_image_to_gemini_headers",
            ),
        )
        .await;
        return None;
    };

    Some(LocalOpenAiImageCandidatePayloadParts {
        transport: Arc::clone(transport),
        auth_header: resolved_headers.auth_header,
        auth_value: resolved_headers.auth_value,
        requested_model: converted.requested_model,
        mapped_model: converted.mapped_model,
        provider_api_format: provider_api_format.to_string(),
        provider_request_headers: resolved_headers.headers,
        provider_request_body: Some(converted.body_json),
        provider_request_body_base64: None,
        content_type: Some("application/json".to_string()),
        upstream_url,
        input_summary: converted.summary_json,
        transport_profile: None,
    })
}

fn chatgpt_web_image_internal_url(base_url: &str) -> String {
    let base_url = base_url.trim().trim_end_matches('/');
    let base_url = if base_url.is_empty() {
        "https://chatgpt.com"
    } else {
        base_url
    };
    format!("{base_url}/__aether/chatgpt-web-image")
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use http::HeaderMap;
    use serde_json::json;

    use super::{
        build_openai_image_passthrough_body_parts, build_openai_image_passthrough_headers,
        build_openai_image_upstream_url_for_request,
    };
    use crate::ai_serving::transport::snapshot::{
        GatewayProviderTransportEndpoint, GatewayProviderTransportKey,
        GatewayProviderTransportProvider, GatewayProviderTransportSnapshot,
    };

    fn sample_transport() -> GatewayProviderTransportSnapshot {
        GatewayProviderTransportSnapshot {
            provider: GatewayProviderTransportProvider {
                id: "provider-1".to_string(),
                name: "Provider".to_string(),
                provider_type: "custom".to_string(),
                website: None,
                is_active: true,
                keep_priority_on_conversion: false,
                enable_format_conversion: false,
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
                api_format: "openai:image".to_string(),
                api_family: None,
                endpoint_kind: None,
                is_active: true,
                base_url: "https://niffler.org".to_string(),
                header_rules: None,
                body_rules: None,
                max_retries: None,
                custom_path: None,
                config: None,
                format_acceptance_config: None,
                proxy: None,
            },
            key: GatewayProviderTransportKey {
                id: "key-1".to_string(),
                provider_id: "provider-1".to_string(),
                name: "key".to_string(),
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
    fn default_image_transport_still_uses_responses_bridge() {
        assert_eq!(
            build_openai_image_upstream_url_for_request(
                &sample_transport(),
                "/v1/images/generations",
                None,
            )
            .as_deref(),
            Some("https://niffler.org/v1/responses")
        );
    }

    #[test]
    fn images_passthrough_uses_configured_images_path() {
        let mut transport = sample_transport();
        transport.endpoint.custom_path = Some("/v1/images/generations".to_string());
        transport.endpoint.config = Some(json!({
            "openai_image_transport_mode": "images_passthrough"
        }));

        assert_eq!(
            build_openai_image_upstream_url_for_request(
                &transport,
                "/v1/images/generations",
                None,
            )
            .as_deref(),
            Some("https://niffler.org/v1/images/generations")
        );
    }

    #[test]
    fn images_passthrough_json_keeps_images_shape_and_applies_mapped_model() {
        let body = json!({
            "model": "public-image-model",
            "prompt": "draw a fox",
            "size": "1024x1024"
        });

        let resolved = build_openai_image_passthrough_body_parts(
            &body,
            None,
            "gpt-image-2",
            None,
            &HeaderMap::new(),
        )
        .expect("JSON passthrough body should build");

        assert_eq!(
            resolved.provider_request_body,
            Some(json!({
                "model": "gpt-image-2",
                "prompt": "draw a fox",
                "size": "1024x1024"
            }))
        );
        assert!(resolved.provider_request_body_base64.is_none());
    }

    #[test]
    fn images_passthrough_multipart_uses_original_base64_body() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            b"--image-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-2\r\n--image-boundary--\r\n",
        );

        let resolved = build_openai_image_passthrough_body_parts(
            &serde_json::Value::Null,
            Some(encoded.as_str()),
            "gpt-image-2",
            None,
            &HeaderMap::new(),
        )
        .expect("multipart passthrough body should build");

        assert!(resolved.provider_request_body.is_none());
        assert_eq!(
            resolved.provider_request_body_base64.as_deref(),
            Some(encoded.as_str())
        );
    }

    #[test]
    fn images_passthrough_headers_preserve_multipart_boundary_and_replace_auth() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            "multipart/form-data; boundary=image-boundary"
                .parse()
                .expect("valid content type"),
        );
        headers.insert(
            http::header::AUTHORIZATION,
            "Bearer downstream".parse().expect("valid authorization"),
        );

        let resolved = build_openai_image_passthrough_headers(
            &headers,
            "authorization",
            "Bearer upstream",
            None,
            &serde_json::Value::Null,
            &serde_json::Value::Null,
            Some("multipart/form-data; boundary=image-boundary"),
        )
        .expect("passthrough headers should build");

        assert_eq!(
            resolved.get("content-type").map(String::as_str),
            Some("multipart/form-data; boundary=image-boundary")
        );
        assert_eq!(
            resolved.get("authorization").map(String::as_str),
            Some("Bearer upstream")
        );
    }
}

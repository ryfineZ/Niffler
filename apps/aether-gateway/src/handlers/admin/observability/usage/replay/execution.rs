use crate::handlers::admin::request::{AdminAppState, AdminGatewayProviderTransportSnapshot};
use crate::GatewayError;
use aether_contracts::{ExecutionPlan, RequestBody};
use axum::http;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const REPLAY_TIMEOUT_SECS: u64 = 120;

pub(super) struct ReplayPlanInput<'a> {
    pub transport: &'a AdminGatewayProviderTransportSnapshot,
    pub body: Value,
    pub client_api_format: &'a str,
    pub model: &'a str,
    pub headers: http::HeaderMap,
    pub replay_id: &'a str,
}

pub(super) async fn build_replay_plan(
    state: &AdminAppState<'_>,
    input: ReplayPlanInput<'_>,
) -> Result<ExecutionPlan, GatewayError> {
    let ReplayPlanInput {
        transport,
        body: request_body,
        client_api_format,
        model: request_model,
        headers: incoming_request_headers,
        replay_id: trace_id,
    } = input;
    let provider_api_format = transport.endpoint.api_format.as_str();
    let normalized_provider_api_format =
        crate::ai_serving::normalize_api_format_alias(provider_api_format);
    let route_path = match client_api_format {
        "openai:responses:compact" => "/v1/responses/compact",
        "openai:responses" => "/v1/responses",
        "claude:messages" => "/v1/messages",
        "gemini:generate_content" => "/v1beta/models/generateContent",
        "openai:embedding" | "jina:embedding" | "doubao:embedding" => "/v1/embeddings",
        "openai:rerank" | "jina:rerank" => "/v1/rerank",
        _ => "/v1/chat/completions",
    };
    let client_is_stream = request_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let upstream_is_stream = if transport
        .provider
        .provider_type
        .eq_ignore_ascii_case("codex")
        && crate::ai_serving::is_openai_responses_compact_format(provider_api_format)
    {
        false
    } else {
        crate::ai_serving::resolve_upstream_is_stream_from_endpoint_config(
            transport.endpoint.config.as_ref(),
            client_is_stream,
            crate::ai_serving::force_upstream_streaming_for_provider(
                &transport.provider.provider_type,
                provider_api_format,
            ),
        )
    };
    let is_codex = transport
        .provider
        .provider_type
        .eq_ignore_ascii_case("codex");
    // Apply Codex edits below with the actual provider/endpoint feature settings;
    // the generic converter's default would enable the hosted image tool.
    let conversion_provider_type = if is_codex {
        "custom"
    } else {
        &transport.provider.provider_type
    };
    let mut provider_request_body =
        crate::ai_serving::build_standard_request_body_with_model_directives_and_request_headers(
            &request_body,
            client_api_format,
            request_model,
            conversion_provider_type,
            provider_api_format,
            route_path,
            upstream_is_stream,
            transport.endpoint.body_rules.as_ref(),
            Some(&transport.key.id),
            Some(&incoming_request_headers),
            false,
        )
        .ok_or_else(|| {
            GatewayError::Internal("请求格式转换失败，无法向所选端点回放".to_string())
        })?;
    if is_codex {
        let enabled =
            crate::ai_serving::openai_responses_image_generation_tool_enabled_from_transport_config(
                &transport.provider.provider_type,
                transport.provider.config.as_ref(),
                transport.endpoint.config.as_ref(),
            );
        let edits = if matches!(
            client_api_format,
            "openai:responses" | "openai:responses:compact"
        ) {
            crate::ai_serving::apply_codex_openai_responses_special_body_edits_with_bridge_config
        } else {
            crate::ai_serving::apply_codex_openai_responses_chat_body_edits_with_bridge_config
        };
        edits(
            &mut provider_request_body,
            &transport.provider.provider_type,
            provider_api_format,
            transport.endpoint.body_rules.as_ref(),
            Some(&transport.key.id),
            crate::ai_serving::codex_openai_image_bridge_model_from_provider_config(
                transport.provider.config.as_ref(),
            ),
            crate::ai_serving::codex_hosted_image_generation_tool_allowed(
                enabled,
                provider_api_format,
            ),
        );
    }
    crate::provider_transport::apply_transport_request_body_semantics(
        &mut provider_request_body,
        transport,
        provider_api_format,
    )
    .map_err(|err| GatewayError::Internal(format!("请求体不符合目标端点要求：{err}")))?;
    let uses_vertex_query_auth =
        crate::provider_transport::uses_vertex_api_key_query_auth(transport, provider_api_format);
    let vertex_query_auth = if uses_vertex_query_auth {
        aether_provider_transport::vertex::resolve_local_vertex_api_key_query_auth(transport)
    } else {
        None
    };
    let oauth_auth =
        match crate::ai_serving::normalize_api_format_alias(provider_api_format).as_str() {
            "openai:chat"
            | "openai:responses"
            | "openai:responses:compact"
            | "claude:messages"
            | "gemini:generate_content"
            | "openai:embedding"
            | "gemini:embedding"
            | "jina:embedding"
            | "doubao:embedding"
            | "openai:rerank"
            | "jina:rerank" => state.resolve_local_oauth_header_auth(transport).await?,
            _ => None,
        };
    let auth = match crate::ai_serving::normalize_api_format_alias(provider_api_format).as_str() {
        "openai:chat"
        | "openai:responses"
        | "openai:responses:compact"
        | "openai:embedding"
        | "jina:embedding"
        | "doubao:embedding"
        | "openai:rerank"
        | "jina:rerank" => {
            crate::provider_transport::auth::resolve_local_openai_bearer_auth(transport)
                .or(oauth_auth)
        }
        "claude:messages" => {
            crate::provider_transport::auth::resolve_local_standard_auth(transport).or(oauth_auth)
        }
        "gemini:generate_content" | "gemini:embedding" => {
            if uses_vertex_query_auth {
                oauth_auth
            } else {
                state.resolve_local_gemini_auth(transport).or(oauth_auth)
            }
        }
        _ => None,
    };
    let (auth_header, auth_value) = match auth {
        Some((auth_header, auth_value)) => (Some(auth_header), Some(auth_value)),
        None if uses_vertex_query_auth && vertex_query_auth.is_some() => (None, None),
        None => {
            return Err(GatewayError::Internal(
                "回放账号认证不可用，请检查账号授权状态".to_string(),
            ));
        }
    };

    let mut synthetic_request = http::Request::builder()
        .uri(route_path)
        .body(())
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    *synthetic_request.headers_mut() = incoming_request_headers;
    let (parts, _) = synthetic_request.into_parts();
    let codex_identity_convergence =
        crate::ai_serving::build_codex_oauth_identity_convergence_request_context(
            &parts.headers,
            &request_body,
            None,
        )?;

    let request_url = crate::provider_transport::build_transport_request_url_for_request_body(
        transport,
        crate::provider_transport::TransportRequestUrlParams {
            provider_api_format,
            mapped_model: Some(request_model),
            upstream_is_stream,
            request_query: parts.uri.query(),
            kiro_api_region: None,
        },
        Some(&provider_request_body),
    );
    let Some(request_url) = request_url else {
        return Err(GatewayError::Internal("无法构建目标请求地址".to_string()));
    };

    let mut request_headers = match provider_api_format {
        "claude:messages" => crate::provider_transport::auth::build_claude_passthrough_headers(
            &parts.headers,
            auth_header.as_deref().unwrap_or_default(),
            auth_value.as_deref().unwrap_or_default(),
            &BTreeMap::new(),
            Some("application/json"),
        ),
        "openai:responses" | "openai:responses:compact" => {
            crate::provider_transport::auth::build_complete_passthrough_headers_with_auth(
                &parts.headers,
                auth_header.as_deref().unwrap_or_default(),
                auth_value.as_deref().unwrap_or_default(),
                &BTreeMap::new(),
                Some("application/json"),
            )
        }
        _ => match (auth_header.as_deref(), auth_value.as_deref()) {
            (Some(auth_header), Some(auth_value)) => state.build_passthrough_headers_with_auth(
                &parts.headers,
                auth_header,
                auth_value,
                &BTreeMap::new(),
            ),
            _ => crate::provider_transport::auth::build_passthrough_headers(
                &parts.headers,
                &BTreeMap::new(),
                Some("application/json"),
            ),
        },
    };
    if uses_vertex_query_auth {
        request_headers.remove("x-goog-api-key");
    }
    request_headers
        .entry("content-type".to_string())
        .or_insert_with(|| "application/json".to_string());
    let protected_headers = if uses_vertex_query_auth {
        vec!["content-type"]
    } else {
        vec![auth_header.as_deref().unwrap_or_default(), "content-type"]
    };
    if !crate::provider_transport::apply_local_header_rules_with_request_headers(
        &mut request_headers,
        transport.endpoint.header_rules.as_ref(),
        &protected_headers,
        &provider_request_body,
        Some(&request_body),
        Some(&parts.headers),
    ) {
        return Err(GatewayError::Internal("回放请求头规则处理失败".to_string()));
    }
    if crate::ai_serving::is_openai_responses_format(provider_api_format) {
        crate::ai_serving::apply_codex_openai_responses_special_headers(
            &mut request_headers,
            &provider_request_body,
            &parts.headers,
            transport.provider.provider_type.as_str(),
            provider_api_format,
            Some(trace_id),
            transport.key.decrypted_auth_config.as_deref(),
        );
    }
    if !uses_vertex_query_auth {
        if let (Some(auth_header), Some(auth_value)) =
            (auth_header.as_deref(), auth_value.as_deref())
        {
            crate::provider_transport::ensure_upstream_auth_header(
                &mut request_headers,
                auth_header,
                auth_value,
            );
        }
    }
    crate::ai_serving::apply_codex_oauth_identity_convergence_to_request(
        state.app(),
        &codex_identity_convergence,
        normalized_provider_api_format.as_str(),
        &mut request_headers,
        &mut provider_request_body,
        transport,
    )
    .await?;

    let mut plan = ExecutionPlan {
        request_id: trace_id.to_string(),
        candidate_id: Some(trace_id.to_string()),
        provider_name: Some(transport.provider.name.clone()),
        provider_id: transport.provider.id.clone(),
        endpoint_id: transport.endpoint.id.clone(),
        key_id: transport.key.id.clone(),
        method: "POST".to_string(),
        url: request_url.clone(),
        headers: request_headers.clone(),
        content_type: Some("application/json".to_string()),
        content_encoding: None,
        body: RequestBody::from_json(provider_request_body.clone()),
        stream: upstream_is_stream,
        client_api_format: client_api_format.to_string(),
        provider_api_format: transport.endpoint.api_format.clone(),
        model_name: Some(request_model.to_string()),
        proxy: state
            .resolve_transport_proxy_snapshot_with_tunnel_affinity(transport)
            .await,
        transport_profile: state.resolve_transport_profile(transport),
        timeouts: state.resolve_transport_execution_timeouts(transport),
    };

    let timeouts = plan.timeouts.get_or_insert_with(Default::default);
    timeouts.total_ms = Some(
        timeouts
            .total_ms
            .unwrap_or(REPLAY_TIMEOUT_SECS * 1000)
            .min(REPLAY_TIMEOUT_SECS * 1000),
    );
    Ok(plan)
}

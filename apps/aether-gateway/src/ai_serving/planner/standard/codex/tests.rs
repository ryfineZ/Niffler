use std::collections::BTreeMap;

use super::{
    apply_codex_openai_responses_special_body_edits,
    apply_codex_openai_responses_special_body_edits_with_bridge_config,
    apply_codex_openai_responses_special_headers,
    openai_responses_image_generation_tool_enabled_from_transport_config,
};
use http::{HeaderMap, HeaderValue};
use serde_json::json;

#[test]
fn applies_codex_defaults_when_body_rules_do_not_handle_fields() {
    let mut body = json!({
        "model": "gpt-5",
        "max_output_tokens": 128,
        "temperature": 0.3,
        "top_p": 0.9,
        "metadata": {"client": "desktop"},
        "store": true
    });

    apply_codex_openai_responses_special_body_edits_with_bridge_config(
        &mut body,
        "codex",
        "openai:responses",
        None,
        None,
        None,
        true,
    );

    assert!(body.get("max_output_tokens").is_none());
    assert!(body.get("temperature").is_none());
    assert!(body.get("top_p").is_none());
    assert!(body.get("metadata").is_none());
    assert_eq!(body["store"], false);
    assert!(body["instructions"]
        .as_str()
        .unwrap_or_default()
        .contains("Responses native `image_generation` tool"));
    assert!(body["instructions"]
        .as_str()
        .unwrap_or_default()
        .contains("MUST call"));
    assert!(body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|tool| tool.get("type") == Some(&json!("image_generation"))));
    assert_eq!(body["tool_choice"], json!("auto"));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["parallel_tool_calls"], true);
    assert!(body.get("reasoning").is_none());
}

#[test]
fn normalizes_replayed_image_generation_call_for_codex_upstream() {
    let mut body = json!({
        "model": "gpt-5.6-sol",
        "client_metadata": {
            "ws_request_header_x_openai_internal_codex_responses_lite": true
        },
        "input": [{
            "type": "image_generation_call",
            "id": "ig_123",
            "status": "completed",
            "result": "aGVsbG8=",
            "action": "generate",
            "background": "auto",
            "output_format": "png",
            "quality": "auto",
            "revised_prompt": "revised",
            "size": "1024x1024"
        }]
    });

    apply_codex_openai_responses_special_body_edits_with_bridge_config(
        &mut body,
        "codex",
        "openai:responses",
        None,
        Some("key-123"),
        None,
        true,
    );

    assert_eq!(
        body["input"][0],
        json!({
            "type": "image_generation_call",
            "id": "ig_123",
            "status": "completed",
            "result": "aGVsbG8="
        })
    );
    assert!(body.get("client_metadata").is_none());
}

#[test]
fn normalizes_boolean_codex_responses_lite_metadata_without_hosted_image_tool() {
    let mut body = json!({
        "model": "gpt-5.6-sol",
        "client_metadata": {
            "ws_request_header_x_openai_internal_codex_responses_lite": true
        },
        "input": [{"role": "user", "content": "hello"}]
    });

    apply_codex_openai_responses_special_body_edits_with_bridge_config(
        &mut body,
        "codex",
        "openai:responses",
        None,
        Some("key-123"),
        None,
        false,
    );

    assert_eq!(
        body["client_metadata"]["ws_request_header_x_openai_internal_codex_responses_lite"],
        json!("true")
    );
}

#[test]
fn image_generation_tool_config_defaults_to_oauth_provider_types_only() {
    assert!(
        openai_responses_image_generation_tool_enabled_from_transport_config("codex", None, None)
    );
    assert!(
        openai_responses_image_generation_tool_enabled_from_transport_config(
            "chatgpt_web",
            None,
            None
        )
    );
    assert!(
        !openai_responses_image_generation_tool_enabled_from_transport_config("custom", None, None)
    );
}

#[test]
fn endpoint_image_generation_tool_config_overrides_provider_config() {
    assert!(
        !openai_responses_image_generation_tool_enabled_from_transport_config(
            "codex",
            None,
            Some(&json!({"openai_responses_image_generation_tool_enabled": false}))
        )
    );
    assert!(
        openai_responses_image_generation_tool_enabled_from_transport_config(
            "custom",
            Some(&json!({"openai_responses_image_generation_tool_enabled": false})),
            Some(&json!({"openai_responses_image_generation_tool_enabled": true}))
        )
    );
}

#[test]
fn strips_store_for_compact_even_when_body_rules_handle_it() {
    let body_rules = json!([
        {"action":"set","path":"store","value":true},
        {"action":"set","path":"instructions","value":"Keep custom"},
        {"action":"set","path":"metadata","value":{"client":"desktop","mode":"custom"}},
        {"action":"set","path":"top_p","value":0.5}
    ]);
    let mut body = json!({
        "model": "gpt-5",
        "max_output_tokens": 128,
        "metadata": {"client": "desktop", "mode": "custom"},
        "store": true,
        "instructions": "Keep custom",
        "top_p": 0.5
    });

    apply_codex_openai_responses_special_body_edits(
        &mut body,
        "codex",
        "openai:responses:compact",
        Some(&body_rules),
        None,
    );

    assert!(body.get("max_output_tokens").is_none());
    assert!(body.get("store").is_none());
    assert_eq!(body["instructions"], "Keep custom");
    assert_eq!(body["metadata"]["mode"], "custom");
    assert_eq!(body["top_p"], 0.5);
}

#[test]
fn injects_stable_prompt_cache_key_for_codex_requests() {
    let mut body = json!({
        "model": "gpt-5",
        "input": "hello",
    });

    apply_codex_openai_responses_special_body_edits(
        &mut body,
        "codex",
        "openai:responses",
        None,
        Some("key-123"),
    );

    assert_eq!(
        body["prompt_cache_key"],
        "172c39e6-c0a0-5a70-8b63-e0f8e0d185a3"
    );
}

#[test]
fn keeps_existing_prompt_cache_key_for_codex_requests() {
    let mut body = json!({
        "model": "gpt-5",
        "input": "hello",
        "prompt_cache_key": "existing-key",
    });

    apply_codex_openai_responses_special_body_edits(
        &mut body,
        "codex",
        "openai:responses",
        None,
        Some("key-123"),
    );

    assert_eq!(body["prompt_cache_key"], "existing-key");
}

#[test]
fn injects_chatgpt_account_id_and_session_headers_for_codex_requests() {
    let mut headers = BTreeMap::new();
    let body = json!({
        "model": "gpt-5",
        "prompt_cache_key": "172c39e6-c0a0-5a70-8b63-e0f8e0d185a3",
    });

    apply_codex_openai_responses_special_headers(
        &mut headers,
        &body,
        &HeaderMap::new(),
        "codex",
        "openai:responses",
        Some("trace-codex-123"),
        Some(r#"{"account_id":"acc-123"}"#),
    );

    assert_eq!(
        headers.get("chatgpt-account-id"),
        Some(&"acc-123".to_string())
    );
    assert_eq!(
        headers.get("x-client-request-id"),
        Some(&"trace-codex-123".to_string())
    );
    assert_eq!(
        headers.get("user-agent"),
        Some(
            &"codex-tui/0.122.0 (Mac OS 15.2.0; arm64) vscode/2.6.11 (codex-tui; 0.122.0)"
                .to_string()
        )
    );
    assert_eq!(headers.get("originator"), Some(&"codex-tui".to_string()));
    assert_eq!(
        headers.get("session_id"),
        Some(&"ab5ecce4f0d110fe".to_string())
    );
    assert_eq!(
        headers.get("conversation_id"),
        Some(&"ab5ecce4f0d110fe".to_string())
    );
}

#[test]
fn removes_codex_responses_lite_header_for_unsupported_model() {
    let mut headers = BTreeMap::new();
    headers.insert(
        "X-OpenAI-Internal-Codex-Responses-Lite".to_string(),
        "true".to_string(),
    );
    let body = json!({
        "model": "gpt-5.5",
        "tool_choice": "auto"
    });

    apply_codex_openai_responses_special_headers(
        &mut headers,
        &body,
        &HeaderMap::new(),
        "codex",
        "openai:responses",
        Some("trace-codex-gpt-55"),
        Some(r#"{"account_id":"acc-123"}"#),
    );

    assert!(!headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("x-openai-internal-codex-responses-lite")));
}

#[test]
fn keeps_codex_responses_lite_header_for_supported_models() {
    for model in ["gpt-5.6-sol", "gpt-5.6"] {
        let mut headers = BTreeMap::new();
        headers.insert(
            "x-openai-internal-codex-responses-lite".to_string(),
            "true".to_string(),
        );
        let body = json!({
            "model": model,
            "tool_choice": "auto"
        });

        apply_codex_openai_responses_special_headers(
            &mut headers,
            &body,
            &HeaderMap::new(),
            "codex",
            "openai:responses",
            Some("trace-codex-gpt-56-sol"),
            Some(r#"{"account_id":"acc-123"}"#),
        );

        assert_eq!(
            headers.get("x-openai-internal-codex-responses-lite"),
            Some(&"true".to_string()),
            "{model} should use the Sol-compatible Responses Lite path"
        );
    }
}

#[test]
fn removes_codex_responses_lite_header_when_sol_uses_hosted_image_tool() {
    let mut headers = BTreeMap::new();
    headers.insert(
        "x-openai-internal-codex-responses-lite".to_string(),
        "true".to_string(),
    );
    let body = json!({
        "model": "gpt-5.6-sol",
        "tools": [{"type": "image_generation", "model": "gpt-image-2"}],
        "tool_choice": "auto"
    });

    apply_codex_openai_responses_special_headers(
        &mut headers,
        &body,
        &HeaderMap::new(),
        "codex",
        "openai:responses",
        Some("trace-codex-sol-image"),
        Some(r#"{"account_id":"acc-123"}"#),
    );

    assert!(!headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("x-openai-internal-codex-responses-lite")));
}

#[test]
fn removes_codex_responses_lite_metadata_when_sol_uses_hosted_image_tool() {
    let mut body = json!({
        "model": "gpt-5.6-sol",
        "client_metadata": {
            "ws_request_header_x_openai_internal_codex_responses_lite": true,
            "x-codex-installation-id": "install-123"
        }
    });

    apply_codex_openai_responses_special_body_edits_with_bridge_config(
        &mut body,
        "codex",
        "openai:responses",
        None,
        Some("key-123"),
        None,
        true,
    );

    assert!(body["client_metadata"]
        .get("ws_request_header_x_openai_internal_codex_responses_lite")
        .is_none());
    assert_eq!(
        body["client_metadata"]["x-codex-installation-id"],
        "install-123"
    );
}

#[test]
fn respects_existing_codex_request_and_session_headers() {
    let mut headers = BTreeMap::new();
    headers.insert(
        "x-client-request-id".to_string(),
        "kept-by-rule-request".to_string(),
    );
    headers.insert("session_id".to_string(), "kept-by-rule".to_string());
    let body = json!({
        "model": "gpt-5",
        "prompt_cache_key": "172c39e6-c0a0-5a70-8b63-e0f8e0d185a3",
    });
    let mut original_headers = HeaderMap::new();
    original_headers.insert(
        "x-client-request-id",
        HeaderValue::from_static("user-specified-request"),
    );
    original_headers.insert(
        "session_id",
        HeaderValue::from_static("user-specified-session"),
    );
    original_headers.insert(
        "conversation_id",
        HeaderValue::from_static("user-specified-conversation"),
    );
    original_headers.insert(
        "user-agent",
        HeaderValue::from_static("user-specified-agent"),
    );
    original_headers.insert(
        "originator",
        HeaderValue::from_static("user-specified-originator"),
    );

    apply_codex_openai_responses_special_headers(
        &mut headers,
        &body,
        &original_headers,
        "codex",
        "openai:responses",
        Some("trace-codex-123"),
        Some(r#"{"account_id":"acc-123"}"#),
    );

    assert_eq!(
        headers.get("x-client-request-id"),
        Some(&"kept-by-rule-request".to_string())
    );
    assert!(!headers.contains_key("user-agent"));
    assert!(!headers.contains_key("originator"));
    assert_eq!(headers.get("session_id"), Some(&"kept-by-rule".to_string()));
    assert!(!headers.contains_key("conversation_id"));
}

#[test]
fn skips_conversation_id_for_compact_codex_requests() {
    let mut headers = BTreeMap::new();
    let body = json!({
        "model": "gpt-5",
        "prompt_cache_key": "172c39e6-c0a0-5a70-8b63-e0f8e0d185a3",
    });

    apply_codex_openai_responses_special_headers(
        &mut headers,
        &body,
        &HeaderMap::new(),
        "codex",
        "openai:responses:compact",
        Some("trace-codex-compact-123"),
        Some(r#"{"account_id":"acc-123"}"#),
    );

    assert_eq!(
        headers.get("chatgpt-account-id"),
        Some(&"acc-123".to_string())
    );
    assert_eq!(
        headers.get("x-client-request-id"),
        Some(&"trace-codex-compact-123".to_string())
    );
    assert_eq!(
        headers.get("user-agent"),
        Some(
            &"codex-tui/0.122.0 (Mac OS 15.2.0; arm64) vscode/2.6.11 (codex-tui; 0.122.0)"
                .to_string()
        )
    );
    assert_eq!(headers.get("originator"), Some(&"codex-tui".to_string()));
    assert_eq!(
        headers.get("session_id"),
        Some(&"ab5ecce4f0d110fe".to_string())
    );
    assert!(!headers.contains_key("conversation_id"));
}

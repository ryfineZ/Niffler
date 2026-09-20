//! 从启用账号发现最小计划，不需要用户请求，也不保存用户正文。
use super::*;
use crate::provider_transport::oauth_refresh::LocalOAuthRefreshAdapter;
use crate::provider_transport::{self, GatewayProviderTransportSnapshot};
use aether_data_contracts::repository::provider_catalog::{
    ProviderCatalogKeyListOrder, ProviderCatalogKeyListQuery,
};
use std::collections::HashSet;

pub(super) async fn plan(
    state: &AppState,
    transport: &GatewayProviderTransportSnapshot,
    model: &str,
) -> Result<Option<ExecutionPlan>, StateError> {
    let format = "openai:responses";
    if !MODELS.contains(&model)
        || !transport
            .provider
            .provider_type
            .eq_ignore_ascii_case("codex")
        || !transport.key.auth_type.eq_ignore_ascii_case("oauth")
        || transport
            .key
            .expires_at_unix_secs
            .is_some_and(|t| t <= now())
        || provider_transport::candidate_common_transport_skip_reason(
            transport,
            provider_transport::CandidateTransportPolicyFacts {
                endpoint_api_format: format,
                global_model_name: model,
                selected_provider_model_name: model,
                mapping_matched_model: None,
            },
            Some(model),
        )
        .is_some()
    {
        return Ok(None);
    }
    let Some(provider_transport::LocalResolvedOAuthRequestAuth::Header { name, value }) =
        provider_transport::GenericOAuthRefreshAdapter::default()
            .resolve_without_refresh(transport)
    else {
        return Ok(None);
    };
    let Some(url) = provider_transport::build_transport_request_url(
        transport,
        provider_transport::TransportRequestUrlParams {
            provider_api_format: format,
            mapped_model: Some(model),
            upstream_is_stream: true,
            request_query: None,
            kiro_api_region: None,
        },
    ) else {
        return Ok(None);
    };
    let incoming = http::HeaderMap::new();
    let original = json!({"model":model,"input":"Reply with OK.","stream":true});
    let Some(mut body) =
        provider_transport::apply_standard_provider_request_body_rules_with_request_headers(
            original.clone(),
            transport.endpoint.body_rules.as_ref(),
            &original,
            &incoming,
        )
    else {
        return Ok(None);
    };
    let Some(built) = provider_transport::build_standard_provider_request_headers(
        provider_transport::StandardProviderRequestHeadersInput {
            transport,
            provider_api_format: format,
            same_format: true,
            headers: &incoming,
            auth_header: &name,
            auth_value: &value,
            extra_headers: &BTreeMap::new(),
            header_rules: transport.endpoint.header_rules.as_ref(),
            provider_request_body: &body,
            original_request_body: &original,
            upstream_is_stream: true,
        },
    ) else {
        return Ok(None);
    };
    let mut headers = built.headers;
    crate::ai_serving::apply_codex_openai_responses_special_headers(
        &mut headers,
        &body,
        &incoming,
        "codex",
        format,
        None,
        transport.key.decrypted_auth_config.as_deref(),
    );
    let context = crate::ai_serving::build_codex_oauth_identity_convergence_request_context(
        &incoming, &original, None,
    )
    .map_err(|_| StateError::runtime())?;
    crate::ai_serving::apply_codex_oauth_identity_convergence_to_request(
        state,
        &context,
        format,
        &mut headers,
        &mut body,
        transport,
    )
    .await
    .map_err(|_| StateError::runtime())?;
    let plan = ExecutionPlan {
        request_id: "state-catalog".into(),
        candidate_id: None,
        provider_name: Some(transport.provider.name.clone()),
        provider_id: transport.provider.id.clone(),
        endpoint_id: transport.endpoint.id.clone(),
        key_id: transport.key.id.clone(),
        method: "POST".into(),
        url,
        headers,
        content_type: Some("application/json".into()),
        content_encoding: None,
        body: RequestBody::from_json(body),
        stream: true,
        client_api_format: format.into(),
        provider_api_format: format.into(),
        model_name: Some(model.into()),
        proxy: state
            .resolve_transport_proxy_snapshot_with_tunnel_affinity(transport)
            .await,
        transport_profile: provider_transport::resolve_transport_profile(transport),
        timeouts: None,
    };
    if !eligible(&plan)
        || header(&plan.headers, "chatgpt-account-id").is_empty()
        || !background::current_credential_matches(&plan, transport)
        || plan
            .body
            .json_body
            .as_ref()
            .and_then(|b| b["model"].as_str())
            != Some(model)
    {
        return Ok(None);
    }
    Ok(Some(probe_plan(&plan)))
}

pub(super) async fn discover(state: &AppState) -> Result<(), StateError> {
    if !enabled(state).await? {
        return Ok(());
    }
    let providers = state
        .list_provider_catalog_providers(true)
        .await
        .map_err(|_| StateError::runtime())?;
    let ids = providers
        .into_iter()
        .filter(|p| p.provider_type.eq_ignore_ascii_case("codex"))
        .map(|p| p.id)
        .collect::<Vec<_>>();
    let endpoints = state
        .list_provider_catalog_endpoints_by_provider_ids(&ids)
        .await
        .map_err(|_| StateError::runtime())?;
    let mut discovered = HashSet::new();
    let mut complete = true;
    for provider in ids {
        let mut offset = 0;
        loop {
            if !enabled(state).await? {
                return Ok(());
            }
            let page = state
                .list_provider_catalog_key_page(&ProviderCatalogKeyListQuery {
                    provider_id: provider.clone(),
                    search: None,
                    is_active: Some(true),
                    offset,
                    limit: 128,
                    order: ProviderCatalogKeyListOrder::CreatedAtAsc,
                })
                .await
                .map_err(|_| StateError::runtime())?;
            if page.items.is_empty() {
                break;
            }
            offset += page.items.len();
            for key in page.items.iter().filter(|k| {
                k.auth_type.eq_ignore_ascii_case("oauth") && k.oauth_invalid_at_unix_secs.is_none()
            }) {
                for endpoint in endpoints.iter().filter(|e| {
                    e.provider_id == provider && e.is_active && e.api_format == "openai:responses"
                }) {
                    let Some(mut transport) = state
                        .read_provider_transport_snapshot(&provider, &endpoint.id, &key.id)
                        .await
                        .map_err(|_| StateError::runtime())?
                    else {
                        continue;
                    };
                    if provider_transport::GenericOAuthRefreshAdapter::default()
                        .resolve_without_refresh(&transport)
                        .is_none()
                    {
                        // 未使用账号的过期 access token 也走原有刷新租约；选号只读，不刷新。
                        if !matches!(
                            tokio::time::timeout(
                                Duration::from_secs(15),
                                Box::pin(state.resolve_local_oauth_request_auth(&transport))
                            )
                            .await,
                            Ok(Ok(Some(_)))
                        ) {
                            tracing::warn!(event_name="codex_state_catalog_auth_unavailable", key_id=%key.id);
                            continue;
                        }
                        let Some(current) = state
                            .read_provider_transport_snapshot(&provider, &endpoint.id, &key.id)
                            .await
                            .map_err(|_| StateError::runtime())?
                        else {
                            continue;
                        };
                        transport = current;
                    }
                    for model in MODELS {
                        let plan = match Box::pin(plan(state, &transport, model)).await {
                            Ok(Some(plan)) => plan,
                            Ok(None) => continue,
                            Err(_) => {
                                complete = false;
                                tracing::warn!(event_name="codex_state_catalog_plan_failed", key_id=%key.id, model);
                                continue;
                            }
                        };
                        let scope = state_scope(state, &plan);
                        background::remember_catalog(state, &plan, &transport).await?;
                        diagnostics::register(state, &plan, &transport).await;
                        discovered.insert(scope);
                    }
                }
            }
            if offset >= page.total {
                break;
            }
        }
    }
    // 只有完整扫描成功才移除不再存在的目录项；短暂数据库故障不会清空队列。
    if complete {
        background::retain_catalog(state, &discovered)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_data::repository::provider_catalog::InMemoryProviderCatalogReadRepository;
    use aether_data_contracts::repository::provider_catalog::{
        StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
    };
    use std::sync::Arc;

    fn state_with_keys(count: usize) -> AppState {
        let provider = StoredProviderCatalogProvider::new(
            "provider".into(),
            "Codex".into(),
            None,
            "codex".into(),
        )
        .unwrap();
        let mut endpoint = StoredProviderCatalogEndpoint::new(
            "endpoint".into(),
            "provider".into(),
            "openai:responses".into(),
            Some("openai".into()),
            Some("responses".into()),
            true,
        )
        .unwrap();
        endpoint.base_url = "https://chatgpt.com/backend-api/codex".into();
        let keys = (0..count)
            .map(|i| {
                let mut key = StoredProviderCatalogKey::new(
                    format!("account-{i}"),
                    "provider".into(),
                    format!("Account {i}"),
                    "oauth".into(),
                    None,
                    true,
                )
                .unwrap();
                key.encrypted_api_key =
                    Some(encrypt_python_fernet_plaintext("test-encryption-key", "secret").unwrap());
                key.encrypted_auth_config = Some(
                    encrypt_python_fernet_plaintext(
                        "test-encryption-key",
                        r#"{"account_id":"workspace"}"#,
                    )
                    .unwrap(),
                );
                key.allowed_models = Some(json!(["gpt-5.6-sol"]));
                key
            })
            .collect();
        let data = crate::data::GatewayDataState::with_provider_transport_reader_for_tests(
            Arc::new(InMemoryProviderCatalogReadRepository::seed(
                vec![provider],
                vec![endpoint],
                keys,
            )),
            "test-encryption-key",
        )
        .with_system_config_values_for_tests([(CONFIG_KEY.to_string(), json!(true))]);
        AppState::new().unwrap().with_data_state_for_tests(data)
    }

    #[tokio::test]
    async fn unused_account_is_queued_collected_and_preferred_without_formal_request() {
        let state = state_with_keys(1);
        discover(&state).await.ok().unwrap();
        let diagnostics = diagnostics::read(&state, "provider", "account-0")
            .await
            .ok()
            .unwrap();
        assert_eq!(diagnostics["items"].as_array().unwrap().len(), 1);
        assert_eq!(diagnostics["items"][0]["status"], "queued");
        background::tick_with_probe(&state, |p| {
            assert_eq!(p.body.json_body.as_ref().unwrap()["model"], "gpt-5.6-sol");
            assert_eq!(header(&p.headers, "chatgpt-account-id"), "workspace");
            async { super::super::tests::success() }
        })
        .await
        .ok()
        .unwrap();
        let transport = state
            .read_provider_transport_snapshot("provider", "endpoint", "account-0")
            .await
            .unwrap()
            .unwrap();
        assert!(priority::ready(&state, &transport, "gpt-5.6-sol").await);
        assert!(!priority::ready(&state, &transport, "gpt-6-astra").await);
        assert_eq!(
            priority::candidates(&state, "provider", "endpoint", "gpt-5.6-sol").await,
            ["account-0"]
        );
        background::tick_with_probe(&state, super::super::tests::unexpected_probe)
            .await
            .ok()
            .unwrap();
    }

    #[tokio::test]
    async fn discovery_pages_all_accounts_and_does_not_rely_on_recent_traffic() {
        let state = state_with_keys(130);
        discover(&state).await.ok().unwrap();
        let last = diagnostics::read(&state, "provider", "account-129")
            .await
            .ok()
            .unwrap();
        assert_eq!(last["items"][0]["status"], "queued");
    }

    #[tokio::test]
    async fn plans_reject_disabled_expired_foreign_endpoint_and_model_changes() {
        let state = state_with_keys(1);
        let original = state
            .read_provider_transport_snapshot("provider", "endpoint", "account-0")
            .await
            .unwrap()
            .unwrap();
        for reason in ["disabled", "expired", "foreign", "model", "auth", "body"] {
            let mut t = original.clone();
            match reason {
                "disabled" => t.key.is_active = false,
                "expired" => t.key.expires_at_unix_secs = Some(now() - 1),
                "foreign" => t.endpoint.base_url = "https://relay.example".into(),
                "model" => t.key.allowed_models = Some(vec!["gpt-6-astra".into()]),
                "auth" => t.key.auth_type = "api_key".into(),
                _ => {
                    t.endpoint.body_rules =
                        Some(json!([{"action":"set","path":"model","value":"another-model"}]))
                }
            }
            assert!(
                plan(&state, &t, "gpt-5.6-sol")
                    .await
                    .ok()
                    .unwrap()
                    .is_none(),
                "{reason}"
            );
        }
    }
}

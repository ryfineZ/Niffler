use http::Uri;

use super::{classify_control_route, headers};

#[test]
fn classifies_admin_create_provider_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/".parse().expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("create_provider"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_providers_summary_list_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/summary?page=1&page_size=20"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("summary_list"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_update_provider_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::PATCH, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("update_provider"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_delete_provider_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("delete_provider"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_provider_summary_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/summary"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("provider_summary"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_provider_health_monitor_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/health-monitor?lookback_hours=6"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("health_monitor"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_provider_mapping_preview_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/mapping-preview"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("mapping_preview"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_provider_delete_task_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/delete-task/task-1234"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("delete_provider_task"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_provider_pool_status_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/pool-status"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("pool_status"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn turn_state_diagnostics_requires_provider_admin_permissions() {
    let uri: Uri = "/api/admin/providers/provider-codex/turn-state?key_id=account"
        .parse()
        .unwrap();
    let decision = classify_control_route(&http::Method::GET, &uri, &headers(&[])).unwrap();
    assert_eq!(decision.route_kind.as_deref(), Some("turn_state"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_provider_clear_pool_cooldown_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/pool/clear-cooldown/key-openai"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("clear_pool_cooldown"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_provider_reset_pool_cost_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/pool/reset-cost/key-openai"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("providers_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("reset_pool_cost"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_list_provider_models_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/models?skip=0&limit=20"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("list_provider_models"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_get_provider_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/models/model-gpt-5"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("get_provider_model"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_create_provider_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/models"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("create_provider_model")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_update_provider_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/models/model-gpt-5"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::PATCH, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("update_provider_model")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_delete_provider_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/models/model-gpt-5"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("delete_provider_model")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_batch_create_provider_models_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/models/batch"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("batch_create_provider_models")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_provider_available_source_models_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/available-source-models"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("available_source_models")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_assign_global_models_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/assign-global-models"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("assign_global_models"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_import_provider_models_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/providers/provider-openai/import-from-upstream"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("provider_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("import_from_upstream"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:providers")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_list_global_models_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global?skip=0&limit=20"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("list_global_models"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_model_catalog_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/catalog"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("model_catalog_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("catalog"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_external_models_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/external"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("model_external_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("external"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_clear_external_models_cache_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/external/cache"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("model_external_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("clear_external_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_global_model_routing_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global/global-gpt-5/routing"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("routing_preview"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_get_global_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global/global-gpt-5"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("get_global_model"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_create_global_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("create_global_model"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_assign_global_model_to_providers_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global/global-gpt-5/assign-to-providers"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("assign_to_providers"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_global_model_providers_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global/global-gpt-5/providers"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("global_model_providers")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_update_global_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global/global-gpt-5"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::PATCH, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("update_global_model"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_delete_global_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global/global-gpt-5"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(decision.route_kind.as_deref(), Some("delete_global_model"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_batch_delete_global_models_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/models/global/batch-delete"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(
        decision.route_family.as_deref(),
        Some("global_models_manage")
    );
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("batch_delete_global_models")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:models")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

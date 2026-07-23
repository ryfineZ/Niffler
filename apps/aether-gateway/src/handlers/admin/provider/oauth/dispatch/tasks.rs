use super::super::errors::build_internal_control_error_response;
use crate::handlers::admin::provider::shared::paths::{
    admin_provider_oauth_agent_identity_import_task_path,
    admin_provider_oauth_batch_import_task_path,
};
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::handlers::admin::shared::attach_admin_audit_response;
use crate::GatewayError;
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};

const PROVIDER_AGENT_IDENTITY_IMPORT_KIND: &str = "agent_identity";

fn provider_oauth_import_task_matches_route(
    task_id: &str,
    payload: &serde_json::Value,
    agent_identity_only: bool,
) -> bool {
    let has_agent_prefix = task_id.starts_with("agent-identity-");
    let import_kind = payload
        .get("import_kind")
        .and_then(serde_json::Value::as_str);
    if agent_identity_only {
        has_agent_prefix && import_kind == Some(PROVIDER_AGENT_IDENTITY_IMPORT_KIND)
    } else {
        !has_agent_prefix && import_kind != Some(PROVIDER_AGENT_IDENTITY_IMPORT_KIND)
    }
}

pub(super) async fn handle_admin_provider_oauth_batch_import_task_status(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
) -> Result<Response<Body>, GatewayError> {
    handle_admin_provider_oauth_import_task_status(state, request_context, false).await
}

pub(super) async fn handle_admin_provider_oauth_agent_identity_import_task_status(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
) -> Result<Response<Body>, GatewayError> {
    handle_admin_provider_oauth_import_task_status(state, request_context, true).await
}

async fn handle_admin_provider_oauth_import_task_status(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    agent_identity_only: bool,
) -> Result<Response<Body>, GatewayError> {
    let task_path = if agent_identity_only {
        admin_provider_oauth_agent_identity_import_task_path(request_context.path())
    } else {
        admin_provider_oauth_batch_import_task_path(request_context.path())
    };
    let Some((provider_id, task_id)) = task_path else {
        return Ok(build_internal_control_error_response(
            http::StatusCode::NOT_FOUND,
            "批量导入任务不存在",
        ));
    };
    let payload = match state
        .read_provider_oauth_batch_task_payload(&provider_id, &task_id)
        .await
    {
        Ok(Some(payload)) => payload,
        Ok(None) => {
            return Ok(build_internal_control_error_response(
                http::StatusCode::NOT_FOUND,
                "批量导入任务不存在或已过期",
            ));
        }
        Err(_) => {
            return Ok(build_internal_control_error_response(
                http::StatusCode::SERVICE_UNAVAILABLE,
                "provider oauth batch task redis unavailable",
            ));
        }
    };
    if !provider_oauth_import_task_matches_route(&task_id, &payload, agent_identity_only) {
        return Ok(build_internal_control_error_response(
            http::StatusCode::NOT_FOUND,
            "导入任务不存在或已过期",
        ));
    }
    let status = payload
        .get("status")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    let response = Json(payload).into_response();
    let (completed_event, failed_event, action, target_type) = if agent_identity_only {
        (
            "admin_provider_oauth_agent_identity_import_completed_viewed",
            "admin_provider_oauth_agent_identity_import_failed_viewed",
            "view_provider_agent_identity_import_terminal_state",
            "provider_agent_identity_import_task",
        )
    } else {
        (
            "admin_provider_oauth_batch_task_completed_viewed",
            "admin_provider_oauth_batch_task_failed_viewed",
            "view_provider_oauth_batch_task_terminal_state",
            "provider_oauth_batch_task",
        )
    };
    Ok(match status.as_str() {
        "completed" => attach_admin_audit_response(
            response,
            completed_event,
            action,
            target_type,
            &format!("{provider_id}:{task_id}"),
        ),
        "failed" => attach_admin_audit_response(
            response,
            failed_event,
            action,
            target_type,
            &format!("{provider_id}:{task_id}"),
        ),
        _ => response,
    })
}

#[cfg(test)]
mod tests {
    use super::provider_oauth_import_task_matches_route;
    use serde_json::json;

    #[test]
    fn import_task_status_routes_are_bidirectionally_isolated() {
        let agent_payload = json!({ "import_kind": "agent_identity" });
        let batch_payload = json!({ "import_kind": "oauth_batch" });

        assert!(provider_oauth_import_task_matches_route(
            "agent-identity-task-1",
            &agent_payload,
            true,
        ));
        assert!(!provider_oauth_import_task_matches_route(
            "agent-identity-task-1",
            &agent_payload,
            false,
        ));
        assert!(provider_oauth_import_task_matches_route(
            "batch-task-1",
            &batch_payload,
            false,
        ));
        assert!(!provider_oauth_import_task_matches_route(
            "batch-task-1",
            &batch_payload,
            true,
        ));
        assert!(provider_oauth_import_task_matches_route(
            "legacy-batch-task",
            &json!({}),
            false,
        ));
    }
}

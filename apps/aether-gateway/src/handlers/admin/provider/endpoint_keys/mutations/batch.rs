use crate::handlers::admin::niffler_legacy_freeze::maybe_freeze_migrated_legacy_provider_key_write;
use crate::handlers::admin::provider::shared::payloads::AdminProviderKeyBatchDeleteRequest;
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::handlers::admin::shared::attach_admin_audit_response;
use crate::GatewayError;
use axum::{
    body::{Body, Bytes},
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) async fn maybe_handle(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&Bytes>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let Some(decision) = request_context.decision() else {
        return Ok(None);
    };
    if decision.route_family.as_deref() != Some("endpoints_manage")
        || decision.route_kind.as_deref() != Some("batch_delete_keys")
        || request_context.method() != http::Method::POST
        || request_context.path() != "/api/admin/endpoints/keys/batch-delete"
    {
        return Ok(None);
    }

    let Some(request_body) = request_body else {
        return Ok(Some(bad_request_response("请求体不能为空")));
    };
    let payload = match serde_json::from_slice::<AdminProviderKeyBatchDeleteRequest>(request_body) {
        Ok(payload) => payload,
        Err(_) => return Ok(Some(bad_request_response("请求体必须是合法的 JSON 对象"))),
    };
    if payload.ids.len() > 100 {
        return Ok(Some(bad_request_response("ids 最多 100 个")));
    }
    if payload.ids.is_empty() {
        return Ok(Some(
            Json(json!({
                "success_count": 0,
                "failed_count": 0,
                "failed": []
            }))
            .into_response(),
        ));
    }

    let found_keys = state
        .read_provider_catalog_keys_by_ids(&payload.ids)
        .await?;
    let found_ids = found_keys
        .iter()
        .map(|key| key.id.clone())
        .collect::<BTreeSet<_>>();
    for key_id in &found_ids {
        if let Some(response) =
            maybe_freeze_migrated_legacy_provider_key_write(state, key_id).await?
        {
            return Ok(Some(response));
        }
    }
    let failed = payload
        .ids
        .iter()
        .filter(|key_id| !found_ids.contains(*key_id))
        .map(|key_id| json!({ "id": key_id, "error": "not found" }))
        .collect::<Vec<_>>();

    let success_count = state.delete_provider_catalog_keys(&found_keys).await?;

    Ok(Some(attach_admin_audit_response(
        Json(json!({
            "success_count": success_count,
            "failed_count": failed.len(),
            "failed": failed,
        }))
        .into_response(),
        "admin_provider_key_batch_deleted",
        "batch_delete_provider_keys",
        "provider_key",
        "batch",
    )))
}

fn bad_request_response(detail: impl Into<String>) -> Response<Body> {
    (
        http::StatusCode::BAD_REQUEST,
        Json(json!({ "detail": detail.into() })),
    )
        .into_response()
}

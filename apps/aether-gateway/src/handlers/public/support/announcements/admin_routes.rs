use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::control::GatewayPublicRequestContext;
use crate::{AppState, GatewayError};

use super::super::resolve_user_portal;
use super::announcements_shared::{
    announcements_bad_request_response, announcements_not_found_response,
    build_public_announcement_payload, parse_optional_rfc3339_unix_secs,
    public_announcement_id_from_path,
};
use aether_data::repository::announcements::{CreateAnnouncementRecord, UpdateAnnouncementRecord};

fn build_admin_announcement_writer_unavailable_response() -> Response<Body> {
    (
        http::StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "detail": "公告写入暂不可用" })),
    )
        .into_response()
}

#[derive(Debug, serde::Deserialize)]
struct AdminAnnouncementCreateRequest {
    title: String,
    content: String,
    #[serde(rename = "type")]
    kind: String,
    priority: Option<i32>,
    is_pinned: Option<bool>,
    requires_ack: Option<bool>,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AdminAnnouncementUpdateRequest {
    title: Option<String>,
    content: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    priority: Option<i32>,
    is_active: Option<bool>,
    is_pinned: Option<bool>,
    requires_ack: Option<bool>,
    start_time: Option<String>,
    end_time: Option<String>,
}

pub(crate) async fn maybe_build_local_admin_announcements_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let Some(decision) = request_context.control_decision.as_ref() else {
        return Ok(None);
    };
    if decision.route_family.as_deref() != Some("announcements_manage") {
        return Ok(None);
    }
    let operator_id = decision
        .admin_principal
        .as_ref()
        .map(|principal| principal.user_id.clone())
        .ok_or_else(|| {
            GatewayError::Internal(
                "admin principal missing for announcement management".to_string(),
            )
        })?;
    let portal = resolve_user_portal(state, &operator_id).await?;

    match decision.route_kind.as_deref() {
        Some("create_announcement")
            if request_context.request_method == http::Method::POST
                && matches!(
                    request_context.request_path.as_str(),
                    "/api/announcements" | "/api/announcements/"
                ) =>
        {
            if !state.has_announcement_data_writer() {
                return Ok(Some(build_admin_announcement_writer_unavailable_response()));
            }
            let Some(request_body) = request_body else {
                return Ok(Some(announcements_bad_request_response("请求体不能为空")));
            };
            let payload =
                match serde_json::from_slice::<AdminAnnouncementCreateRequest>(request_body) {
                    Ok(value) => value,
                    Err(_) => {
                        return Ok(Some(announcements_bad_request_response(
                            "请求体必须是合法的 JSON 对象",
                        )))
                    }
                };
            let record = build_create_record(payload, operator_id)?;
            let Some(created) = state.create_announcement(&portal.id, record).await? else {
                return Ok(Some(build_admin_announcement_writer_unavailable_response()));
            };
            let mut response = build_public_announcement_payload(&created);
            response["message"] = json!("公告创建成功");
            return Ok(Some(Json(response).into_response()));
        }
        Some("update_announcement") if request_context.request_method == http::Method::PUT => {
            let Some(announcement_id) =
                public_announcement_id_from_path(&request_context.request_path)
            else {
                return Ok(Some(announcements_not_found_response()));
            };
            if !state.has_announcement_data_writer() {
                return Ok(Some(build_admin_announcement_writer_unavailable_response()));
            }
            let Some(request_body) = request_body else {
                return Ok(Some(announcements_bad_request_response("请求体不能为空")));
            };
            let payload =
                match serde_json::from_slice::<AdminAnnouncementUpdateRequest>(request_body) {
                    Ok(value) => value,
                    Err(_) => {
                        return Ok(Some(announcements_bad_request_response(
                            "请求体必须是合法的 JSON 对象",
                        )))
                    }
                };
            let record = build_update_record(announcement_id, payload)?;
            return Ok(Some(
                match state.update_announcement(&portal.id, record).await? {
                    Some(_) => Json(json!({ "message": "公告更新成功" })).into_response(),
                    None => announcements_not_found_response(),
                },
            ));
        }
        Some("delete_announcement") if request_context.request_method == http::Method::DELETE => {
            let Some(announcement_id) =
                public_announcement_id_from_path(&request_context.request_path)
            else {
                return Ok(Some(announcements_not_found_response()));
            };
            if !state.has_announcement_data_writer() {
                return Ok(Some(build_admin_announcement_writer_unavailable_response()));
            }
            if state
                .find_announcement_by_id(&portal.id, announcement_id)
                .await?
                .is_none()
            {
                return Ok(Some(announcements_not_found_response()));
            }
            let deleted = state
                .delete_announcement(&portal.id, announcement_id)
                .await?;
            return Ok(Some(if deleted {
                Json(json!({ "message": "公告已删除" })).into_response()
            } else {
                build_admin_announcement_writer_unavailable_response()
            }));
        }
        _ => {}
    }

    Ok(None)
}

fn build_create_record(
    payload: AdminAnnouncementCreateRequest,
    operator_id: String,
) -> Result<CreateAnnouncementRecord, GatewayError> {
    Ok(CreateAnnouncementRecord {
        title: payload.title,
        content: payload.content,
        kind: payload.kind,
        priority: payload.priority.unwrap_or(0),
        is_pinned: payload.is_pinned.unwrap_or(false),
        requires_ack: payload.requires_ack.unwrap_or(false),
        author_id: operator_id,
        start_time_unix_secs: parse_optional_rfc3339_unix_secs(
            payload.start_time.as_deref(),
            "start_time",
        )
        .map_err(GatewayError::Internal)?,
        end_time_unix_secs: parse_optional_rfc3339_unix_secs(
            payload.end_time.as_deref(),
            "end_time",
        )
        .map_err(GatewayError::Internal)?,
    })
}

fn build_update_record(
    announcement_id: &str,
    payload: AdminAnnouncementUpdateRequest,
) -> Result<UpdateAnnouncementRecord, GatewayError> {
    Ok(UpdateAnnouncementRecord {
        announcement_id: announcement_id.to_string(),
        title: payload.title,
        content: payload.content,
        kind: payload.kind,
        priority: payload.priority,
        is_active: payload.is_active,
        is_pinned: payload.is_pinned,
        requires_ack: payload.requires_ack,
        start_time_unix_secs: parse_optional_rfc3339_unix_secs(
            payload.start_time.as_deref(),
            "start_time",
        )
        .map_err(GatewayError::Internal)?,
        end_time_unix_secs: parse_optional_rfc3339_unix_secs(
            payload.end_time.as_deref(),
            "end_time",
        )
        .map_err(GatewayError::Internal)?,
    })
}

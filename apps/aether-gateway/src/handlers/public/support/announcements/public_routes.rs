use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};

use crate::control::GatewayPublicRequestContext;
use crate::AppState;

use super::super::{build_unhandled_public_support_response, resolve_request_portal};
use super::announcements_shared::{
    announcements_bad_request_response, announcements_internal_detail,
    announcements_internal_error_response, announcements_not_found_response,
    build_public_announcement_list_payload, build_public_announcement_payload,
    parse_public_announcements_query, public_announcement_id_from_path,
};

pub(crate) async fn maybe_build_local_public_announcements_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Option<Response<Body>> {
    let decision = request_context.control_decision.as_ref()?;
    if decision.route_family.as_deref() != Some("announcements") {
        return None;
    }
    if !state.has_announcement_data_reader() {
        return None;
    }
    let portal = match resolve_request_portal(state, request_context, headers).await {
        Ok(value) => value,
        Err(err) => {
            return Some(announcements_internal_error_response(
                announcements_internal_detail(err),
            ))
        }
    };

    match decision.route_kind.as_deref() {
        Some("list")
            if request_context.request_method == http::Method::GET
                && matches!(
                    request_context.request_path.as_str(),
                    "/api/announcements" | "/api/announcements/"
                ) =>
        {
            let query = match parse_public_announcements_query(
                request_context.request_query_string.as_deref(),
                true,
                50,
            ) {
                Ok(value) => value,
                Err(detail) => return Some(announcements_bad_request_response(detail)),
            };
            let page = match state.list_announcements(portal.id, &query).await {
                Ok(value) => value,
                Err(err) => {
                    return Some(announcements_internal_error_response(
                        announcements_internal_detail(err),
                    ))
                }
            };
            Some(Json(build_public_announcement_list_payload(page)).into_response())
        }
        Some("active")
            if request_context.request_method == http::Method::GET
                && matches!(
                    request_context.request_path.as_str(),
                    "/api/announcements/active" | "/api/announcements/active/"
                ) =>
        {
            let query = aether_data::repository::announcements::AnnouncementListQuery {
                active_only: true,
                offset: 0,
                limit: 10,
                now_unix_secs: Some(chrono::Utc::now().timestamp().max(0) as u64),
            };
            let page = match state.list_announcements(portal.id, &query).await {
                Ok(value) => value,
                Err(err) => {
                    return Some(announcements_internal_error_response(
                        announcements_internal_detail(err),
                    ))
                }
            };
            Some(Json(build_public_announcement_list_payload(page)).into_response())
        }
        Some("detail") => {
            let Some(announcement_id) =
                public_announcement_id_from_path(&request_context.request_path)
            else {
                return Some(build_unhandled_public_support_response(request_context));
            };
            let announcement = match state
                .find_announcement_by_id(portal.id, announcement_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => return Some(announcements_not_found_response()),
                Err(err) => {
                    return Some(announcements_internal_error_response(
                        announcements_internal_detail(err),
                    ))
                }
            };
            Some(Json(build_public_announcement_payload(&announcement)).into_response())
        }
        _ => Some(build_unhandled_public_support_response(request_context)),
    }
}

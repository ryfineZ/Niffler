use super::analytics::admin_usage_provider_key_names;
use super::analytics::{admin_usage_api_key_names, admin_usage_provider_key_account_labels};
use super::replay::{
    admin_usage_curl_headers, admin_usage_curl_url, admin_usage_headers_from_value,
    admin_usage_id_from_action_path, admin_usage_id_from_detail_path,
    admin_usage_resolve_body_value, admin_usage_resolve_request_capture_body,
    admin_usage_resolve_request_capture_body_for_item, build_admin_usage_curl_response,
    build_admin_usage_detail_payload, build_admin_usage_replay_response,
};
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::handlers::admin::shared::{attach_admin_audit_response, query_param_bool};
use crate::GatewayError;
use aether_admin::observability::usage::{
    admin_usage_apply_candidate_upstream_error_display, admin_usage_apply_provider_route_display,
    admin_usage_attempt_info_from_candidates, admin_usage_bad_request_response,
    admin_usage_data_unavailable_response, admin_usage_provider_key_account_label,
    admin_usage_provider_key_name, ADMIN_USAGE_DATA_UNAVAILABLE_DETAIL,
};
use aether_data_contracts::repository::candidates::StoredRequestCandidate;
use aether_data_contracts::repository::usage::UsageBodyField;
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use tokio::try_join;

pub(super) async fn maybe_build_local_admin_usage_detail_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let route_kind = request_context
        .control_decision
        .as_ref()
        .and_then(|decision| decision.route_kind.as_deref());

    match route_kind {
        Some("curl")
            if request_context.request_method == http::Method::GET
                && request_context
                    .request_path
                    .starts_with("/api/admin/usage/")
                && request_context.request_path.ends_with("/curl") =>
        {
            if !state.has_usage_data_reader() {
                return Ok(Some(admin_usage_data_unavailable_response(
                    ADMIN_USAGE_DATA_UNAVAILABLE_DETAIL,
                )));
            }

            let Some(usage_id) =
                admin_usage_id_from_action_path(&request_context.request_path, "/curl")
            else {
                return Ok(Some(admin_usage_bad_request_response("usage_id 无效")));
            };

            let Some(item) = state.find_request_usage_by_id(&usage_id).await? else {
                return Ok(Some(
                    (
                        http::StatusCode::NOT_FOUND,
                        Json(json!({ "detail": "Usage record not found" })),
                    )
                        .into_response(),
                ));
            };

            let endpoint = if let Some(endpoint_id) = item.provider_endpoint_id.as_ref() {
                state
                    .read_provider_catalog_endpoints_by_ids(std::slice::from_ref(endpoint_id))
                    .await?
                    .into_iter()
                    .next()
            } else {
                None
            };
            let url = endpoint
                .as_ref()
                .map(|endpoint| admin_usage_curl_url(state, endpoint, &item));
            let headers_json = item
                .provider_request_headers
                .clone()
                .or_else(|| item.request_headers.clone());
            let headers = headers_json
                .as_ref()
                .and_then(admin_usage_headers_from_value)
                .filter(|headers| !headers.is_empty())
                .unwrap_or_else(admin_usage_curl_headers);
            let provider_request_body = admin_usage_resolve_body_value(
                state,
                &item,
                item.provider_request_body.as_ref(),
                UsageBodyField::ProviderRequestBody,
            )
            .await?;
            let request_body = admin_usage_resolve_body_value(
                state,
                &item,
                item.request_body.as_ref(),
                UsageBodyField::RequestBody,
            )
            .await?;
            let body = provider_request_body
                .or(request_body)
                .or_else(|| admin_usage_resolve_request_capture_body(&item, None));
            return Ok(Some(attach_admin_audit_response(
                build_admin_usage_curl_response(&item, url, headers_json, &headers, body.as_ref()),
                "admin_usage_curl_viewed",
                "view_usage_curl_replay",
                "usage_record",
                &item.id,
            )));
        }
        Some("replay") => {
            let mut response =
                build_admin_usage_replay_response(state, request_context, request_body).await?;
            if response.status().is_success() {
                if let Some(usage_id) =
                    admin_usage_id_from_action_path(&request_context.request_path, "/replay")
                {
                    response = attach_admin_audit_response(
                        response,
                        "admin_usage_replay_executed",
                        "execute_usage_replay",
                        "usage_record",
                        &usage_id,
                    );
                }
            }
            return Ok(Some(response));
        }
        Some("detail")
            if request_context.request_method == http::Method::GET
                && request_context
                    .request_path
                    .starts_with("/api/admin/usage/") =>
        {
            if !state.has_usage_data_reader() {
                return Ok(Some(admin_usage_data_unavailable_response(
                    ADMIN_USAGE_DATA_UNAVAILABLE_DETAIL,
                )));
            }

            let Some(usage_id) = admin_usage_id_from_detail_path(&request_context.request_path)
            else {
                return Ok(Some(admin_usage_bad_request_response("usage_id 无效")));
            };
            let include_bodies = query_param_bool(
                request_context.request_query_string.as_deref(),
                "include_bodies",
                true,
            );

            let Some(item) = state.find_request_usage_by_id(&usage_id).await? else {
                return Ok(Some(
                    (
                        http::StatusCode::NOT_FOUND,
                        Json(json!({ "detail": "Usage record not found" })),
                    )
                        .into_response(),
                ));
            };

            let user_ids = item.user_id.clone().into_iter().collect::<Vec<_>>();
            let (users_by_id, provider_key_names, provider_key_account_labels, api_key_names): (
                BTreeMap<String, aether_data::repository::users::StoredUserSummary>,
                BTreeMap<String, String>,
                BTreeMap<String, String>,
                BTreeMap<String, String>,
            ) = try_join!(
                state.resolve_auth_user_summaries_by_ids(&user_ids),
                admin_usage_provider_key_names(state, std::slice::from_ref(&item)),
                admin_usage_provider_key_account_labels(state, std::slice::from_ref(&item)),
                admin_usage_api_key_names(state, std::slice::from_ref(&item)),
            )?;
            let provider_key_name = admin_usage_provider_key_name(&item, &provider_key_names);
            let provider_key_account_label =
                admin_usage_provider_key_account_label(&item, &provider_key_account_labels);

            let mut detail_item = item.clone();
            let request_body = if include_bodies {
                let (request_body, provider_request_body, response_body, client_response_body) = try_join!(
                    admin_usage_resolve_request_capture_body_for_item(state, &item, None),
                    admin_usage_resolve_body_value(
                        state,
                        &item,
                        item.provider_request_body.as_ref(),
                        UsageBodyField::ProviderRequestBody,
                    ),
                    admin_usage_resolve_body_value(
                        state,
                        &item,
                        item.response_body.as_ref(),
                        UsageBodyField::ResponseBody,
                    ),
                    admin_usage_resolve_body_value(
                        state,
                        &item,
                        item.client_response_body.as_ref(),
                        UsageBodyField::ClientResponseBody,
                    ),
                )?;
                detail_item.provider_request_body = provider_request_body;
                detail_item.response_body = response_body;
                detail_item.client_response_body = client_response_body;
                request_body
            } else {
                None
            };
            if include_bodies {
                // request_body 已通过 request capture 解析；其余 detached body 在上方并行加载。
            }
            let default_headers = admin_usage_curl_headers();
            let (request_candidates, provider_names_by_id) =
                load_admin_usage_detail_attempt_context(state, &item.request_id).await?;
            let attempt_info = admin_usage_attempt_info_from_candidates(
                &item,
                &request_candidates,
                &provider_names_by_id,
            );
            let mut payload = build_admin_usage_detail_payload(
                &detail_item,
                &users_by_id,
                &api_key_names,
                state.has_auth_user_data_reader(),
                state.has_auth_api_key_data_reader(),
                provider_key_name.as_deref(),
                provider_key_account_label.as_deref(),
                include_bodies,
                request_body,
                &default_headers,
            );
            if !attempt_info.provider_route.is_empty() {
                admin_usage_apply_provider_route_display(
                    &mut payload,
                    &attempt_info.provider_route,
                );
                payload["provider_route"] = json!(attempt_info.provider_route);
            }
            admin_usage_apply_candidate_upstream_error_display(
                &mut payload,
                &detail_item,
                &request_candidates,
            );
            payload["has_fallback"] = json!(attempt_info.has_fallback);
            payload["has_retry"] = json!(attempt_info.has_retry);

            return Ok(Some(attach_admin_audit_response(
                Json(payload).into_response(),
                "admin_usage_detail_viewed",
                "view_usage_detail",
                "usage_record",
                &item.id,
            )));
        }
        _ => {}
    }

    Ok(None)
}

async fn load_admin_usage_detail_attempt_context(
    state: &AdminAppState<'_>,
    request_id: &str,
) -> Result<(Vec<StoredRequestCandidate>, BTreeMap<String, String>), GatewayError> {
    if !state.has_request_candidate_data_reader() {
        return Ok((Vec::new(), BTreeMap::new()));
    }

    let candidates = state
        .app()
        .read_request_candidates_by_request_id(request_id)
        .await?;
    let provider_names_by_id = if state.has_provider_catalog_data_reader() {
        provider_names_by_id_for_candidates(state, &candidates).await?
    } else {
        BTreeMap::new()
    };
    Ok((candidates, provider_names_by_id))
}

async fn provider_names_by_id_for_candidates(
    state: &AdminAppState<'_>,
    candidates: &[StoredRequestCandidate],
) -> Result<BTreeMap<String, String>, GatewayError> {
    let provider_ids = candidates
        .iter()
        .filter_map(|candidate| candidate.provider_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if provider_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    Ok(state
        .read_provider_catalog_providers_by_ids(&provider_ids)
        .await?
        .into_iter()
        .map(|provider| (provider.id, provider.name))
        .collect())
}

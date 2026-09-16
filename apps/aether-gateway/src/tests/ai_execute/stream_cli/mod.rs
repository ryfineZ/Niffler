use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use axum::body::{to_bytes, Body, Bytes};
use axum::response::Response;
use axum::routing::any;
use axum::{extract::Request, Json, Router};
use http::header::{HeaderName, HeaderValue};
use http::StatusCode;
use serde_json::json;

use crate::constants::{
    CONTROL_EXECUTED_HEADER, CONTROL_EXECUTE_FALLBACK_HEADER, CONTROL_REQUEST_ID_HEADER,
    DEPENDENCY_REASON_HEADER, EXECUTION_PATH_EXECUTION_RUNTIME_STREAM,
    EXECUTION_PATH_EXECUTION_RUNTIME_SYNC, EXECUTION_PATH_HEADER,
    EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS, LOCAL_EXECUTION_RUNTIME_MISS_REASON_HEADER,
    TRACE_ID_HEADER,
};

use super::{
    build_router, build_router_with_execution_runtime_override, build_router_with_state,
    build_state_with_execution_runtime_override, start_server, strip_sse_keepalive_comments,
    wait_until, AppState, FrontdoorCorsConfig, FrontdoorUserRpmConfig, GatewayFallbackMetricKind,
    GatewayFallbackReason, UsageRuntimeConfig, VideoTaskTruthSourceMode,
};
use aether_data::repository::candidates::InMemoryRequestCandidateRepository;
use aether_data_contracts::repository::candidates::{
    RequestCandidateReadRepository, RequestCandidateStatus, StoredRequestCandidate,
};

mod compact;
mod direct;

fn response_request_id(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(CONTROL_REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("response should expose the server request id")
        .to_string()
}

async fn wait_for_request_candidate_status(
    gateway_state: &AppState,
    request_candidate_repository: &Arc<InMemoryRequestCandidateRepository>,
    request_id: &str,
    expected_status: RequestCandidateStatus,
) -> Vec<StoredRequestCandidate> {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            crate::request_candidate_runtime::flush_request_candidate_status_writes(gateway_state)
                .await;
            let stored_candidates = request_candidate_repository
                .list_by_request_id(request_id)
                .await
                .expect("request candidate trace should read");
            if stored_candidates
                .iter()
                .any(|candidate| candidate.status == expected_status)
            {
                break stored_candidates;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("request candidate status should settle")
}

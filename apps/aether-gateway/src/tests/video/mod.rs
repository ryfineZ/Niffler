use std::sync::{Arc, Mutex};

use aether_data::repository::video_tasks::InMemoryVideoTaskRepository;
use aether_data_contracts::repository::video_tasks::{
    UpsertVideoTask, VideoTaskLookupKey, VideoTaskReadRepository, VideoTaskWriteRepository,
};
use axum::body::{to_bytes, Body, Bytes};
use axum::response::Response;
use axum::routing::any;
use axum::{extract::Request, Json, Router};
use http::header::{HeaderName, HeaderValue};
use http::StatusCode;
use serde_json::json;

use crate::constants::{
    CONTROL_EXECUTED_HEADER, CONTROL_EXECUTE_FALLBACK_HEADER, EXECUTION_PATH_HEADER,
    CONTROL_REQUEST_ID_HEADER, TRACE_ID_HEADER,
};

use super::{
    build_router, build_router_with_state, build_state_with_execution_runtime_override,
    start_server, AppState, VideoTaskTruthSourceMode,
};

mod data_read;
mod gemini_sync_create;
mod gemini_sync_task;
mod openai_sync_create;
mod openai_sync_task;
mod registry_poller;
mod routing;
mod stream;

fn response_request_id(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(CONTROL_REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("response should expose the server request id")
        .to_string()
}

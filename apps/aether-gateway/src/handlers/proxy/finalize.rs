use crate::audit::emit_admin_audit;
use crate::constants::{
    CONTROL_ENDPOINT_SIGNATURE_HEADER, CONTROL_EXECUTION_RUNTIME_HEADER, CONTROL_REQUEST_ID_HEADER,
    CONTROL_ROUTE_CLASS_HEADER, CONTROL_ROUTE_FAMILY_HEADER, CONTROL_ROUTE_KIND_HEADER,
    DEPENDENCY_REASON_HEADER, EXECUTION_PATH_HEADER, LOCAL_EXECUTION_RUNTIME_MISS_REASON_HEADER,
    TRACE_ID_HEADER,
};
use crate::control::GatewayControlDecision;
use crate::control::GatewayPublicRequestContext;
use crate::middleware::{sanitize_access_log_path, should_downgrade_access_log, RequestLogEmitted};
use crate::AppState;
use aether_runtime::{maybe_hold_axum_response_permit, AdmissionPermit};
use axum::body::{Body, Bytes};
use axum::http::{self, header::HeaderName, header::HeaderValue, Response};
use std::time::Instant;
use tracing::{info, trace, warn};

#[derive(Debug, Clone, Copy)]
pub(super) struct TrustedGatewayRequestIdentity;

pub(super) fn request_wants_stream(
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
    body: &axum::body::Bytes,
) -> bool {
    if request_context
        .request_path
        .contains(":streamGenerateContent")
    {
        return true;
    }
    if !request_context
        .request_content_type
        .as_deref()
        .map(|value| value.to_ascii_lowercase().contains("application/json"))
        .unwrap_or(false)
        || body.is_empty()
    {
        return false;
    }
    let body = match crate::headers::decoded_request_body_bytes(headers, body.as_ref()) {
        Ok(body) => body,
        Err(_) => return false,
    };
    serde_json::from_slice::<serde_json::Value>(body.as_ref())
        .ok()
        .and_then(|value| value.get("stream").and_then(|stream| stream.as_bool()))
        .unwrap_or(false)
}

pub(super) fn finalize_gateway_response(
    state: &AppState,
    mut response: Response<Body>,
    request_id: &str,
    trace_id: &str,
    remote_addr: &std::net::SocketAddr,
    method: &http::Method,
    path_and_query: &str,
    control_decision: Option<&GatewayControlDecision>,
    execution_path: &'static str,
    started_at: &Instant,
    request_permit: Option<AdmissionPermit>,
) -> Response<Body> {
    attach_control_decision_headers(&mut response, control_decision);
    if response
        .extensions()
        .get::<TrustedGatewayRequestIdentity>()
        .is_none()
    {
        response.headers_mut().insert(
            HeaderName::from_static(CONTROL_REQUEST_ID_HEADER),
            HeaderValue::from_str(request_id).expect("request id should be a valid header value"),
        );
    }
    response.headers_mut().insert(
        HeaderName::from_static(TRACE_ID_HEADER),
        HeaderValue::from_str(trace_id).expect("trace id should be a valid header value"),
    );
    response.headers_mut().insert(
        HeaderName::from_static(EXECUTION_PATH_HEADER),
        HeaderValue::from_static(execution_path),
    );

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let dependency_reason = response
        .headers()
        .get(DEPENDENCY_REASON_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("none")
        .to_string();
    let local_execution_runtime_miss_reason = response
        .headers()
        .get(LOCAL_EXECUTION_RUNTIME_MISS_REASON_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("none")
        .to_string();
    let route_class = control_decision
        .and_then(|decision| decision.route_class.as_deref())
        .unwrap_or("passthrough");
    let response_request_id = response
        .headers()
        .get(CONTROL_REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("-")
        .to_string();
    let auth_context = control_decision.and_then(|decision| decision.auth_context.as_ref());
    let user_id = auth_context
        .map(|auth_context| auth_context.user_id.as_str())
        .unwrap_or("-");
    let api_key_id = auth_context
        .map(|auth_context| auth_context.api_key_id.as_str())
        .unwrap_or("-");
    let status_code = response.status().as_u16();
    let sanitized_path_and_query = sanitize_access_log_path(path_and_query);
    emit_admin_audit(
        &mut response,
        trace_id,
        method,
        sanitized_path_and_query.as_str(),
        control_decision,
    );
    if response.status().is_server_error() {
        warn!(
            event_name = "http_request_failed",
            log_type = "access",
            status = "failed",
            status_code,
            trace_id = %trace_id,
            request_id = response_request_id,
            remote_addr = %remote_addr,
            method = %method,
            path = %sanitized_path_and_query,
            user_id,
            api_key_id,
            route_class,
            execution_path,
            dependency_reason = dependency_reason.as_str(),
            local_execution_runtime_miss_reason = local_execution_runtime_miss_reason.as_str(),
            elapsed_ms,
            "gateway request failed"
        );
    } else if should_downgrade_access_log(method, sanitized_path_and_query.as_str()) {
        trace!(
            event_name = "http_request_completed",
            log_type = "access",
            status = "completed",
            status_code,
            trace_id = %trace_id,
            request_id = response_request_id,
            remote_addr = %remote_addr,
            method = %method,
            path = %sanitized_path_and_query,
            user_id,
            api_key_id,
            route_class,
            execution_path,
            dependency_reason = dependency_reason.as_str(),
            local_execution_runtime_miss_reason = local_execution_runtime_miss_reason.as_str(),
            elapsed_ms,
            "gateway completed request"
        );
    } else {
        info!(
            event_name = "http_request_completed",
            log_type = "access",
            status = "completed",
            status_code,
            trace_id = %trace_id,
            request_id = response_request_id,
            remote_addr = %remote_addr,
            method = %method,
            path = %sanitized_path_and_query,
            user_id,
            api_key_id,
            route_class,
            execution_path,
            dependency_reason = dependency_reason.as_str(),
            local_execution_runtime_miss_reason = local_execution_runtime_miss_reason.as_str(),
            elapsed_ms,
            "gateway completed request"
        );
    }
    response.extensions_mut().insert(RequestLogEmitted);

    maybe_hold_axum_response_permit(response, request_permit)
}

#[cfg(test)]
mod request_identity_tests {
    use super::{finalize_gateway_response, TrustedGatewayRequestIdentity};
    use crate::constants::CONTROL_REQUEST_ID_HEADER;
    use crate::AppState;
    use axum::body::Body;
    use axum::http::{Method, Response};
    use std::time::Instant;

    #[test]
    fn finalizer_always_uses_the_gateway_request_id() {
        let state = AppState::new().expect("gateway state should build");
        let mut response = Response::new(Body::empty());
        response.headers_mut().insert(
            CONTROL_REQUEST_ID_HEADER,
            "upstream-controlled-id"
                .parse()
                .expect("header should parse"),
        );

        let response = finalize_gateway_response(
            &state,
            response,
            "gateway-request-id",
            "trace-request-id-finalizer-1",
            &"127.0.0.1:19001".parse().expect("address should parse"),
            &Method::POST,
            "/v1/chat/completions",
            None,
            "test",
            &Instant::now(),
            None,
        );

        assert_eq!(
            response
                .headers()
                .get(CONTROL_REQUEST_ID_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("gateway-request-id")
        );
    }

    #[test]
    fn finalizer_preserves_request_id_from_trusted_gateway_forwarding() {
        let state = AppState::new().expect("gateway state should build");
        let mut response = Response::new(Body::empty());
        response.headers_mut().insert(
            CONTROL_REQUEST_ID_HEADER,
            "owner-gateway-request-id"
                .parse()
                .expect("header should parse"),
        );
        response
            .extensions_mut()
            .insert(TrustedGatewayRequestIdentity);

        let response = finalize_gateway_response(
            &state,
            response,
            "edge-gateway-request-id",
            "trace-trusted-gateway-request-id-1",
            &"127.0.0.1:19003".parse().expect("address should parse"),
            &Method::POST,
            "/v1/chat/completions",
            None,
            "test",
            &Instant::now(),
            None,
        );

        assert_eq!(
            response
                .headers()
                .get(CONTROL_REQUEST_ID_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("owner-gateway-request-id")
        );
    }
}

fn attach_control_decision_headers(
    response: &mut Response<Body>,
    control_decision: Option<&GatewayControlDecision>,
) {
    let Some(control_decision) = control_decision else {
        return;
    };
    if !response.headers().contains_key(CONTROL_ROUTE_CLASS_HEADER) {
        response.headers_mut().insert(
            HeaderName::from_static(CONTROL_ROUTE_CLASS_HEADER),
            HeaderValue::from_str(
                control_decision
                    .route_class
                    .as_deref()
                    .unwrap_or("passthrough"),
            )
            .expect("route class should be a valid header value"),
        );
    }
    if !response
        .headers()
        .contains_key(CONTROL_EXECUTION_RUNTIME_HEADER)
    {
        response.headers_mut().insert(
            HeaderName::from_static(CONTROL_EXECUTION_RUNTIME_HEADER),
            HeaderValue::from_static(if control_decision.is_execution_runtime_candidate() {
                "true"
            } else {
                "false"
            }),
        );
    }
    if let Some(route_family) = control_decision.route_family.as_deref() {
        if !response.headers().contains_key(CONTROL_ROUTE_FAMILY_HEADER) {
            response.headers_mut().insert(
                HeaderName::from_static(CONTROL_ROUTE_FAMILY_HEADER),
                HeaderValue::from_str(route_family)
                    .expect("route family should be a valid header value"),
            );
        }
    }
    if let Some(route_kind) = control_decision.route_kind.as_deref() {
        if !response.headers().contains_key(CONTROL_ROUTE_KIND_HEADER) {
            response.headers_mut().insert(
                HeaderName::from_static(CONTROL_ROUTE_KIND_HEADER),
                HeaderValue::from_str(route_kind)
                    .expect("route kind should be a valid header value"),
            );
        }
    }
    if let Some(endpoint_signature) = control_decision.auth_endpoint_signature.as_deref() {
        if !response
            .headers()
            .contains_key(CONTROL_ENDPOINT_SIGNATURE_HEADER)
        {
            response.headers_mut().insert(
                HeaderName::from_static(CONTROL_ENDPOINT_SIGNATURE_HEADER),
                HeaderValue::from_str(endpoint_signature)
                    .expect("endpoint signature should be a valid header value"),
            );
        }
    }
}

pub(super) fn finalize_gateway_response_with_context(
    state: &AppState,
    response: Response<Body>,
    remote_addr: &std::net::SocketAddr,
    request_context: &GatewayPublicRequestContext,
    execution_path: &'static str,
    started_at: &Instant,
    request_permit: Option<AdmissionPermit>,
) -> Response<Body> {
    finalize_gateway_response(
        state,
        response,
        &request_context.request_id,
        &request_context.trace_id,
        remote_addr,
        &request_context.request_method,
        &request_context.request_path_and_query(),
        request_context.control_decision.as_ref(),
        execution_path,
        started_at,
        request_permit,
    )
}

#[cfg(test)]
mod tests {
    use super::{finalize_gateway_response, request_wants_stream};
    use crate::control::{GatewayControlDecision, GatewayPublicRequestContext};
    use crate::AppState;
    use axum::body::{Body, Bytes};
    use axum::http::{HeaderMap, HeaderValue, Method, Response, StatusCode};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::prelude::*;

    #[derive(Clone, Default)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

    impl SharedBuffer {
        fn lines(&self) -> Vec<serde_json::Value> {
            String::from_utf8(self.0.lock().expect("buffer should lock").clone())
                .expect("buffer should contain valid utf-8")
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str(line).expect("json log line should parse"))
                .collect()
        }
    }

    impl std::io::Write for SharedBufferWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("buffer should lock")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::writer::MakeWriter<'a> for SharedBuffer {
        type Writer = SharedBufferWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedBufferWriter(Arc::clone(&self.0))
        }
    }

    #[test]
    fn request_wants_stream_reads_zstd_encoded_json_body() {
        let request_context = GatewayPublicRequestContext {
            request_id: "request-zstd-stream".to_string(),
            trace_id: "trace-zstd-stream".to_string(),
            request_method: Method::POST,
            request_path: "/v1/responses".to_string(),
            request_query_string: None,
            request_content_type: Some("application/json".to_string()),
            host_header: None,
            control_decision: None,
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_ENCODING,
            HeaderValue::from_static("zstd"),
        );
        let encoded = zstd::stream::encode_all(br#"{"stream":true}"#.as_slice(), 0)
            .expect("zstd body should encode");

        assert!(request_wants_stream(
            &request_context,
            &headers,
            &Bytes::from(encoded),
        ));
    }

    #[test]
    fn finalize_gateway_response_logs_sanitized_path_and_query() {
        let state = AppState::new().expect("gateway state should build");
        let writer = SharedBuffer::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .with_writer(writer.clone())
                .with_filter(LevelFilter::INFO),
        );
        let dispatch = tracing::Dispatch::new(subscriber);
        let _guard = tracing::dispatcher::set_default(&dispatch);
        let remote_addr = "127.0.0.1:8080"
            .parse()
            .expect("remote address should parse");
        let control_decision = GatewayControlDecision::synthetic(
            "/v1beta/models/gemini-3-flash-preview:generateContent",
            Some("ai_public".to_string()),
            Some("gemini".to_string()),
            Some("generate_content".to_string()),
            Some("gemini:generate_content".to_string()),
        );

        let response = Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .expect("response should build");

        let _response = finalize_gateway_response(
            &state,
            response,
            "request-finalize",
            "trace-finalize",
            &remote_addr,
            &Method::GET,
            "/v1beta/models/gemini-3-flash-preview:generateContent?key=secret&alt=sse",
            Some(&control_decision),
            "execution_runtime_sync",
            &Instant::now(),
            None,
        );

        let logs = writer.lines();
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0]["path"],
            "/v1beta/models/gemini-3-flash-preview:generateContent?alt=sse"
        );
    }
}

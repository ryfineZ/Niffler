use axum::http::Uri;

use crate::headers::header_value_str;
use crate::{AppState, GatewayError};

use super::{resolve_control_route, GatewayControlDecision};

#[derive(Debug, Clone)]
pub(crate) struct GatewayPublicRequestContext {
    pub(crate) request_id: String,
    pub(crate) trace_id: String,
    pub(crate) request_method: http::Method,
    pub(crate) request_path: String,
    pub(crate) request_query_string: Option<String>,
    pub(crate) request_content_type: Option<String>,
    pub(crate) host_header: Option<String>,
    pub(crate) control_decision: Option<GatewayControlDecision>,
}

impl GatewayPublicRequestContext {
    pub(crate) fn from_request_parts(
        trace_id: impl Into<String>,
        method: &http::Method,
        uri: &Uri,
        headers: &http::HeaderMap,
        control_decision: Option<GatewayControlDecision>,
    ) -> Self {
        let trace_id = trace_id.into();
        Self::from_request_parts_with_request_id(
            trace_id.clone(),
            trace_id,
            method,
            uri,
            headers,
            control_decision,
        )
    }

    pub(crate) fn from_request_parts_with_request_id(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        method: &http::Method,
        uri: &Uri,
        headers: &http::HeaderMap,
        control_decision: Option<GatewayControlDecision>,
    ) -> Self {
        let request_path = if uri.path().starts_with('/') {
            uri.path().to_string()
        } else {
            format!("/{}", uri.path())
        };
        let request_query_string = uri.query().map(ToOwned::to_owned);

        Self {
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            request_method: method.clone(),
            request_path,
            request_query_string,
            request_content_type: header_value_str(headers, http::header::CONTENT_TYPE.as_str()),
            host_header: header_value_str(headers, http::header::HOST.as_str()),
            control_decision,
        }
    }

    pub(crate) fn request_path_and_query(&self) -> String {
        if let Some(query) = self
            .request_query_string
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            format!("{}?{query}", self.request_path)
        } else {
            self.request_path.clone()
        }
    }
}

pub(crate) async fn resolve_public_request_context(
    state: &AppState,
    method: &http::Method,
    uri: &Uri,
    headers: &http::HeaderMap,
    trace_id: &str,
) -> Result<GatewayPublicRequestContext, GatewayError> {
    resolve_public_request_context_with_request_id(state, method, uri, headers, trace_id, trace_id)
        .await
}

pub(crate) async fn resolve_public_request_context_with_request_id(
    state: &AppState,
    method: &http::Method,
    uri: &Uri,
    headers: &http::HeaderMap,
    request_id: &str,
    trace_id: &str,
) -> Result<GatewayPublicRequestContext, GatewayError> {
    let control_decision = resolve_control_route(state, method, uri, headers, trace_id).await?;
    Ok(
        GatewayPublicRequestContext::from_request_parts_with_request_id(
            request_id,
            trace_id,
            method,
            uri,
            headers,
            control_decision,
        ),
    )
}

use aether_contracts::ExecutionResult;
use base64::Engine as _;
use serde_json::Value;

pub(super) struct ReplayBody {
    pub body: Value,
    pub error: Option<String>,
}

fn body_error(body: &Value) -> Option<String> {
    if let Some(error) = body.get("error").filter(|error| !error.is_null()) {
        return Some(
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("上游返回错误")
                .to_string(),
        );
    }
    if let Some(status @ ("failed" | "incomplete" | "cancelled")) =
        body.get("status").and_then(Value::as_str)
    {
        return Some(format!(
            "上游未完成响应（{status}）：{}",
            body.pointer("/incomplete_details/reason")
                .and_then(Value::as_str)
                .unwrap_or("请查看响应内容")
        ));
    }
    None
}

/// Inspect the actual terminal events before aggregation: aggregators can recover
/// partial output, but partial output alone must never turn a broken stream green.
pub(super) fn decode_replay_result(format: &str, result: &ExecutionResult) -> ReplayBody {
    let bytes = result
        .body
        .as_ref()
        .and_then(|body| body.body_bytes_b64.as_deref())
        .and_then(|raw| base64::engine::general_purpose::STANDARD.decode(raw).ok())
        .map(|bytes| {
            crate::execution_runtime::transport::decode_response_body_bytes(&result.headers, &bytes)
                .unwrap_or(bytes)
        });
    let json_body = result
        .body
        .as_ref()
        .and_then(|body| body.json_body.clone())
        .or_else(|| {
            bytes
                .as_deref()
                .and_then(|bytes| serde_json::from_slice(bytes).ok())
        });
    let mut error = result.error.as_ref().map(|error| error.message.clone());
    let body = if let Some(body) = json_body {
        error = error.or_else(|| body_error(&body));
        body
    } else if let Some(bytes) = bytes.filter(|bytes| !bytes.is_empty()) {
        let raw = String::from_utf8_lossy(&bytes).to_string();
        let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
        let mut events = Vec::new();
        let mut complete = false;
        for frame in normalized.split("\n\n") {
            let data = frame
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|line| line.strip_prefix(' ').unwrap_or(line))
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() {
                continue;
            }
            if data.trim() == "[DONE]" {
                if format == "openai:chat" {
                    complete = true;
                }
                continue;
            }
            let Ok(mut event) = serde_json::from_str::<Value>(&data) else {
                error.get_or_insert_with(|| "上游流式响应包含无法解析的数据".to_string());
                continue;
            };
            if event.get("type").is_none() {
                if let Some(name) = frame.lines().find_map(|line| line.strip_prefix("event:")) {
                    if let Some(object) = event.as_object_mut() {
                        object.insert("type".into(), Value::String(name.trim().into()));
                    }
                }
            }
            let event_type = event
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(message) =
                body_error(&event).or_else(|| event.get("response").and_then(body_error))
            {
                error.get_or_insert(message);
            }
            if matches!(
                event_type,
                "error" | "response.failed" | "response.incomplete"
            ) {
                error.get_or_insert_with(|| {
                    event
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("上游流式响应失败")
                        .to_string()
                });
            }
            complete |= match format {
                "openai:responses" | "openai:responses:compact" => {
                    event_type == "response.completed"
                }
                "claude:messages" => event_type == "message_stop",
                "gemini:generate_content" => event
                    .get("candidates")
                    .and_then(Value::as_array)
                    .is_some_and(|candidates| {
                        candidates
                            .iter()
                            .any(|c| c.get("finishReason").and_then(Value::as_str).is_some())
                    }),
                _ => false,
            };
            events.push(event);
        }
        if !complete {
            error.get_or_insert_with(|| {
                if events.is_empty() {
                    "上游返回了非 JSON 响应"
                } else {
                    "流式响应提前结束，未收到完成标记"
                }
                .to_string()
            });
        }
        let canonical = events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>();
        let bytes = canonical.as_bytes();
        let aggregated = match format {
            "openai:responses" | "openai:responses:compact" => {
                crate::ai_serving::aggregate_openai_responses_stream_sync_response(bytes)
            }
            "openai:chat" => crate::ai_serving::aggregate_openai_chat_stream_sync_response(bytes),
            "claude:messages" => crate::ai_serving::aggregate_claude_stream_sync_response(bytes),
            "gemini:generate_content" => {
                crate::ai_serving::aggregate_gemini_stream_sync_response(bytes)
            }
            _ => None,
        };
        if aggregated.is_none() {
            error.get_or_insert_with(|| "上游未返回可解析的完成响应".to_string());
        }
        // For a failed stream, preserve the actual error event and partial output.
        if error.is_some() {
            Value::String(raw)
        } else {
            aggregated.unwrap_or(Value::String(raw))
        }
    } else {
        Value::Null
    };
    if !(200..300).contains(&result.status_code) {
        error =
            Some(body_error(&body).unwrap_or_else(|| {
                format!("上游返回 HTTP {}，请查看响应内容", result.status_code)
            }));
    }
    if matches!(format, "openai:responses" | "openai:responses:compact")
        && body
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status != "completed")
    {
        error.get_or_insert_with(|| "上游响应尚未完成，请查看响应内容".to_string());
    }
    if body.is_null() || body.as_object().is_some_and(|body| body.is_empty()) {
        error.get_or_insert_with(|| "上游未返回响应内容".to_string());
    }
    if matches!(format, "openai:responses" | "openai:responses:compact")
        && body
            .get("output")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        error.get_or_insert_with(|| "上游已结束响应，但没有返回任何输出".to_string());
    }
    ReplayBody { body, error }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_contracts::ResponseBody;
    use serde_json::json;

    fn stream(raw: &str) -> ExecutionResult {
        ExecutionResult {
            request_id: "replay".into(),
            candidate_id: None,
            status_code: 200,
            headers: Default::default(),
            telemetry: None,
            error: None,
            body: Some(ResponseBody {
                json_body: None,
                body_bytes_b64: Some(base64::engine::general_purpose::STANDARD.encode(raw)),
            }),
        }
    }

    #[test]
    fn replay_responses_tool_output_is_successful() {
        let event = json!({"type":"response.completed","response":{"id":"r","status":"completed","output":[{"type":"function_call","name":"read_file","arguments":"{}","call_id":"call-1"}]}});
        let decoded =
            decode_replay_result("openai:responses", &stream(&format!("data: {event}\n\n")));
        assert!(decoded.error.is_none(), "{:?}", decoded.error);
        assert_eq!(decoded.body["output"][0]["name"], "read_file");
    }

    #[test]
    fn replay_http_200_stream_error_is_failure_and_keeps_raw_body() {
        let decoded = decode_replay_result("openai:responses", &stream("data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"quota exceeded\"}}}\n\n"));
        assert_eq!(decoded.error.as_deref(), Some("quota exceeded"));
        assert!(decoded.body.as_str().unwrap().contains("response.failed"));
    }

    #[test]
    fn replay_partial_stream_is_not_success() {
        let decoded = decode_replay_result(
            "openai:responses",
            &stream("data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n"),
        );
        assert!(decoded.error.unwrap().contains("提前结束"));
        assert!(decoded.body.as_str().unwrap().contains("partial"));
    }

    #[test]
    fn replay_non_json_http_error_is_visible() {
        let mut result = stream("<html>Bad Gateway</html>");
        result.status_code = 502;
        let decoded = decode_replay_result("openai:responses", &result);
        assert!(decoded.error.unwrap().contains("502"));
        assert_eq!(decoded.body, "<html>Bad Gateway</html>");
    }
    #[test]
    fn replay_sse_event_name_and_multiline_data_are_supported() {
        let decoded = decode_replay_result("openai:responses", &stream("event: response.completed\r\ndata: {\"response\":{\"id\":\"r\",\"status\":\"completed\",\r\ndata: \"output\":[{\"type\":\"function_call\",\"name\":\"read_file\",\"call_id\":\"c\",\"arguments\":\"{}\"}]}}\r\n\r\n"));
        assert!(decoded.error.is_none(), "{:?}", decoded.error);
        assert_eq!(decoded.body["output"][0]["name"], "read_file");
    }

    #[test]
    fn replay_empty_completed_response_is_not_reported_as_success() {
        let decoded = decode_replay_result("openai:responses", &stream("data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\",\"status\":\"completed\",\"output\":[]}}\n\n"));
        assert!(decoded.error.unwrap().contains("没有返回任何输出"));
    }

    #[test]
    fn replay_gzip_stream_is_decoded_before_parsing() {
        use std::io::Write;
        let raw = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"quota exceeded\"}}}\n\n";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(raw.as_bytes()).unwrap();
        let mut result = stream("");
        result
            .headers
            .insert("content-encoding".into(), "gzip".into());
        result.body.as_mut().unwrap().body_bytes_b64 =
            Some(base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap()));
        let decoded = decode_replay_result("openai:responses", &result);
        assert_eq!(decoded.error.as_deref(), Some("quota exceeded"));
        assert_eq!(decoded.body.as_str(), Some(raw));
    }
}

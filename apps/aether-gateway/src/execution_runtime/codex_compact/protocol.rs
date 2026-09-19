use aether_contracts::{ExecutionPlan, RequestBody};
use serde_json::{json, Value};

use super::CompactError;

pub(super) const MAX_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn is_v2(body: &Value) -> bool {
    body.get("input")
        .and_then(Value::as_array)
        .and_then(|input| input.last())
        .and_then(Value::as_object)
        .is_some_and(|item| {
            item.len() == 1
                && item.get("type").and_then(Value::as_str) == Some("compaction_trigger")
        })
}

/// 仅用于已确认是官方 Codex OAuth 的临时派发计划。
pub(super) fn bridge_request(plan: &mut ExecutionPlan) -> Result<(), CompactError> {
    let mut body = plan
        .body
        .json_body
        .clone()
        .ok_or_else(CompactError::request)?;
    let triggered = is_v2(&body);
    let object = body.as_object_mut().ok_or_else(CompactError::request)?;
    let input = object
        .get_mut("input")
        .and_then(Value::as_array_mut)
        .ok_or_else(CompactError::request)?;
    if !triggered {
        input.push(json!({"type":"compaction_trigger"}));
    }
    object.insert("stream".into(), json!(true));
    object.insert("store".into(), json!(false));
    object.entry("tool_choice").or_insert(json!("auto"));
    object
        .entry("include")
        .or_insert(json!(["reasoning.encrypted_content"]));
    if serde_json::to_vec(&body)
        .map_err(|_| CompactError::request())?
        .len()
        > MAX_BYTES
    {
        return Err(CompactError::request());
    }
    let mut url = reqwest::Url::parse(&plan.url).map_err(|_| CompactError::request())?;
    let path = url
        .path()
        .strip_suffix("/compact")
        .ok_or_else(CompactError::request)?
        .to_owned();
    url.set_path(&path);
    plan.url = url.into();
    plan.body = RequestBody::from_json(body);
    plan.content_type = Some("application/json".into());
    plan.content_encoding = None;
    plan.headers.retain(|name, _| {
        !matches!(
            name.to_ascii_lowercase().as_str(),
            "accept"
                | "accept-encoding"
                | "content-length"
                | "content-encoding"
                | "transfer-encoding"
        )
    });
    plan.headers
        .insert("accept".into(), "text/event-stream".into());
    plan.headers
        .insert("accept-encoding".into(), "identity".into());
    plan.stream = true;
    Ok(())
}

/// 只有完整完成事件和真正的 compaction 密文才能生成旧版成功响应。
pub(super) fn bridge_response(bytes: &[u8]) -> Result<Value, CompactError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CompactError::invalid())?
        .replace("\r\n", "\n");
    let mut completed = None;
    let mut emitted = Vec::new();
    for frame in text.split("\n\n") {
        let data = frame
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let value: Value = serde_json::from_str(&data).map_err(|_| CompactError::invalid())?;
        let event = value.get("type").and_then(Value::as_str).or_else(|| {
            frame
                .lines()
                .find_map(|line| line.strip_prefix("event:").map(str::trim))
        });
        match event {
            Some("error" | "response.error" | "response.failed" | "response.incomplete") => {
                let code = value
                    .pointer("/response/error/code")
                    .or_else(|| value.pointer("/error/code"))
                    .or_else(|| value.get("code"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                return Err(CompactError::upstream(match code {
                    "rate_limit_exceeded" | "usage_limit_reached" | "insufficient_quota" => 429,
                    "invalid_api_key" | "authentication_error" => 401,
                    "permission_denied" => 403,
                    _ => 502,
                }));
            }
            Some("response.output_item.done") => {
                emitted.push(
                    value
                        .get("item")
                        .cloned()
                        .ok_or_else(CompactError::invalid)?,
                );
            }
            Some("response.completed") => {
                if completed.is_some() {
                    return Err(CompactError::invalid());
                }
                let response = value
                    .get("response")
                    .filter(|v| v.is_object())
                    .ok_or_else(CompactError::invalid)?;
                if response
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|v| v != "completed")
                {
                    return Err(CompactError::invalid());
                }
                completed = Some(response.clone());
            }
            _ => {}
        }
    }
    let completed = completed.ok_or_else(CompactError::incomplete)?;
    let output = match completed.get("output") {
        Some(Value::Array(items)) if !items.is_empty() => items.clone(),
        None | Some(Value::Array(_)) => emitted,
        _ => return Err(CompactError::invalid()),
    };
    let compactions: Vec<_> = output
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("compaction"))
        .collect();
    if compactions.len() != 1
        || !compactions[0]
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    {
        return Err(CompactError::invalid());
    }
    let mut result = json!({"object":"response.compaction", "output":output});
    for field in ["id", "created_at", "model", "usage"] {
        if let Some(value) = completed.get(field) {
            result[field] = value.clone();
        }
    }
    Ok(result)
}

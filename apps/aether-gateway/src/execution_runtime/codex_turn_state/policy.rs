use base64::Engine as _;
use serde_json::Value;

pub(super) const TTL_SECONDS: u64 = 3600;

pub(super) fn parse(value: &str) -> Option<(u64, usize)> {
    if value.is_empty() || value.len() > 2048 || value.bytes().any(|b| b.is_ascii_whitespace()) {
        return None;
    }
    let core = value.trim_end_matches('=');
    if value.len() - core.len() > 2 {
        return None;
    }
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(core)
        .ok()?;
    if raw.len() < 73 || raw[0] != 0x80 || (raw.len() - 57) % 16 != 0 {
        return None;
    }
    let issued = u64::from_be_bytes(raw[1..9].try_into().ok()?);
    (1_577_836_800..4_102_444_800)
        .contains(&issued)
        .then_some((issued, (raw.len() - 57) / 16))
}

pub(super) fn accepts(value: &str, expected: usize, now: u64) -> bool {
    parse(value).is_some_and(|(issued, blocks)| {
        blocks == expected
            && issued <= now.saturating_add(30)
            && now < issued.saturating_add(TTL_SECONDS - 30)
    })
}

pub(super) fn expected_blocks(authorization: &str, account: &str) -> usize {
    let payload = authorization
        .strip_prefix("Bearer ")
        .and_then(|v| v.split('.').nth(1))
        .filter(|v| v.len() <= 16384)
        .and_then(|v| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(v)
                .ok()
        })
        .and_then(|v| serde_json::from_slice::<Value>(&v).ok());
    let Some(auth) = payload
        .as_ref()
        .and_then(|v| v.get("https://api.openai.com/auth"))
    else {
        return 10;
    };
    if auth.get("chatgpt_account_id").and_then(Value::as_str) != Some(account) {
        return 10;
    }
    match auth.get("chatgpt_plan_type").and_then(Value::as_str) {
        Some("team" | "business") => 12,
        _ => 10,
    }
}

/// 完整成功事件才可发布候选；只保留数字用量，不保留自由文本。
pub(super) fn probe_outcome(bytes: &[u8]) -> Result<Value, u16> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| 502u16)?
        .replace("\r\n", "\n");
    let mut usage = None;
    for event in text.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let value: Value = serde_json::from_str(&data).map_err(|_| 502u16)?;
        match value.get("type").and_then(Value::as_str) {
            Some("error" | "response.failed" | "response.incomplete") => {
                let code = value
                    .pointer("/response/error/code")
                    .or_else(|| value.pointer("/error/code"))
                    .or_else(|| value.get("code"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                return Err(match code {
                    "rate_limit_exceeded" | "usage_limit_reached" | "insufficient_quota" => 429,
                    "invalid_api_key" | "authentication_error" => 401,
                    _ => 503,
                });
            }
            Some("response.completed") => {
                if value
                    .pointer("/response/status")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != "completed")
                {
                    return Err(502);
                }
                usage = Some(safe_usage(value.pointer("/response/usage")));
            }
            _ => {}
        }
    }
    usage.ok_or(502)
}

fn safe_usage(value: Option<&Value>) -> Value {
    let Some(value) = value.filter(|v| v.is_object()) else {
        return Value::Null;
    };
    let mut out = serde_json::Map::new();
    for key in ["input_tokens", "output_tokens", "total_tokens"] {
        if let Some(count) = value.get(key).and_then(Value::as_u64) {
            out.insert(key.into(), count.into());
        }
    }
    Value::Object(out)
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;

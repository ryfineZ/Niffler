use super::*;

fn state_value(blocks: usize, issued: u64) -> String {
    let mut raw = vec![0; 57 + blocks * 16];
    raw[0] = 0x80;
    raw[1..9].copy_from_slice(&issued.to_be_bytes());
    base64::engine::general_purpose::URL_SAFE.encode(raw)
}

#[test]
fn validates_structure_blocks_and_time_instead_of_length_only() {
    let now = 1_800_000_000;
    assert!(accepts(&state_value(10, now), 10, now));
    assert!(accepts(&state_value(12, now), 12, now));
    assert!(!accepts(&state_value(11, now), 10, now));
    assert!(!accepts(&state_value(12, now), 10, now));
    assert!(!accepts(&state_value(13, now), 12, now));
    assert!(!accepts(&state_value(10, now - 3570), 10, now));
    assert!(!accepts(&state_value(10, now + 31), 10, now));
    assert!(accepts(&state_value(10, now + 30), 10, now));
    assert!(!accepts(&"A".repeat(292), 10, now));
    assert!(parse("not a state").is_none());
    assert!(parse(&(state_value(10, now) + "\r\nInjected: true")).is_none());
}

#[test]
fn success_requires_completed_sse_and_rejects_error_after_headers() {
    assert!(probe_outcome(
        b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    )
    .is_ok());
    assert!(probe_outcome(b"data: {\"type\":\"response.created\"}\n\n").is_err());
    assert!(probe_outcome(b"data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"rate_limit_exceeded\"}}}\n\n").is_err());
    assert!(probe_outcome(
        b"data: {\"type\":\"error\"}\n\ndata: {\"type\":\"response.completed\"}\n\n"
    )
    .is_err());
}

#[test]
fn team_hint_is_not_applied_to_another_workspace() {
    let claims = serde_json::json!({"https://api.openai.com/auth": {
        "chatgpt_plan_type":"team", "chatgpt_account_id":"workspace-a"
    }});
    let auth = format!(
        "Bearer e30.{}.sig",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    assert_eq!(expected_blocks(&auth, "workspace-a"), 12);
    assert_eq!(expected_blocks(&auth, "workspace-b"), 10);
}

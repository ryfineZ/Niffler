//! 正常响应的临时候选：完整成功后才发布，丢弃/取消不产生写入。
use super::*;
use crate::provider_transport::GatewayProviderTransportSnapshot;

const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Context {
    plan: ExecutionPlan,
    configuration: String,
}

impl Context {
    pub(super) async fn capture(
        state: &AppState,
        plan: &ExecutionPlan,
        transport: &GatewayProviderTransportSnapshot,
    ) -> Option<Self> {
        if !background::current_credential_matches(plan, transport) {
            return None;
        }
        match tokio::time::timeout(
            Duration::from_millis(250),
            background::configuration(state, transport),
        )
        .await
        {
            Ok(Ok(configuration)) => Some(Self {
                plan: probe_plan(plan),
                configuration,
            }),
            _ => {
                tracing::warn!(
                    event_name = "codex_state_passive_skipped",
                    reason = "configuration_unavailable"
                );
                None
            }
        }
    }
}

// 不派生 Debug/Serialize：token 和最小计划中的凭据仅存在于当前请求内存。
pub(crate) struct Candidate {
    state: AppState,
    context: Context,
    token: String,
    blocks: usize,
    completion: Completion,
}

impl Candidate {
    pub(super) fn new(
        state: &AppState,
        context: Context,
        token: String,
        blocks: usize,
        headers: &BTreeMap<String, String>,
    ) -> Self {
        Self {
            state: state.clone(),
            context,
            token,
            blocks,
            completion: Completion {
                json: header(headers, "content-type")
                    .to_ascii_lowercase()
                    .contains("application/json"),
                ..Default::default()
            },
        }
    }

    pub(crate) fn observe(&mut self, bytes: &[u8]) {
        self.completion.push(bytes);
    }
    pub(crate) fn fail(&mut self) {
        self.completion.failed = true;
    }

    pub(crate) async fn finish(mut self) {
        if !self.completion.success() {
            return;
        }
        // 不分离后台任务；共享状态慢/失败最多占用 250ms，不能破坏成功响应。
        let outcome = tokio::time::timeout(Duration::from_millis(250), self.publish()).await;
        let reason = match outcome {
            Ok(Ok(reason)) => reason,
            Ok(Err(_)) => "runtime_unavailable",
            Err(_) => "timeout",
        };
        tracing::info!(event_name="codex_state_passive_finished", key_id=%self.context.plan.key_id, reason);
    }

    async fn publish(&self) -> Result<&'static str, StateError> {
        let state = &self.state;
        let plan = &self.context.plan;
        let account = digest(
            json!([
                plan.provider_id,
                plan.key_id,
                header(&plan.headers, "chatgpt-account-id")
            ])
            .to_string(),
        );
        let Some(lease) = state
            .runtime_state
            .lock_try_acquire(
                &format!("codex-state:collect:{account}"),
                state.tunnel.local_instance_id(),
                Duration::from_secs(3),
            )
            .await
            .map_err(|_| StateError::runtime())?
        else {
            return Ok("collector_busy");
        };
        let result = async {
            if !enabled(state).await? || !policy::accepts(&self.token, self.blocks, now()) {
                return Ok("no_longer_eligible");
            }
            if background::revalidate_plan(state, plan.clone(), &self.context.configuration)
                .await?
                .is_none()
            {
                return Ok("configuration_changed");
            }
            check_guard(
                state,
                &format!("codex-state:guard:{account}"),
                &digest(header(&plan.headers, "authorization")),
            )
            .await?;
            let cache_key = format!("codex-state:active:{}", state_scope(state, plan));
            if let Some(raw) = state
                .runtime_state
                .kv_get(&cache_key)
                .await
                .map_err(|_| StateError::runtime())?
            {
                let plain = decrypt_python_fernet_ciphertext(
                    state.encryption_key().ok_or_else(StateError::runtime)?,
                    &raw,
                )
                .map_err(|_| StateError::runtime())?;
                let cached: Cached =
                    serde_json::from_str(&plain).map_err(|_| StateError::runtime())?;
                if policy::accepts(&cached.token, self.blocks, now()) {
                    return Ok("already_ready");
                }
            }
            let cache = Cached {
                token: self.token.clone(),
                version: uuid::Uuid::new_v4().to_string(),
                source: Some("response".into()),
            };
            publish_cache(state, &lease, &cache_key, &cache).await?;
            priority::remember(state, plan, &cache).await;
            Ok("saved")
        }
        .await;
        state
            .runtime_state
            .lock_release(&lease)
            .await
            .map_err(|_| StateError::runtime())?;
        result
    }
}

#[derive(Default)]
struct Completion {
    json: bool,
    pending: Vec<u8>,
    line_nonempty: bool,
    completed: bool,
    failed: bool,
}

impl Completion {
    fn push(&mut self, bytes: &[u8]) {
        if self.failed {
            return;
        }
        for &byte in bytes {
            if self.pending.len() >= MAX_EVENT_BYTES {
                self.failed = true;
                self.pending.clear();
                return;
            }
            self.pending.push(byte);
            if self.json {
                continue;
            }
            if byte == b'\n' {
                if !self.line_nonempty {
                    self.event();
                    self.pending.clear();
                    if self.failed {
                        return;
                    }
                }
                self.line_nonempty = false;
            } else if byte != b'\r' {
                self.line_nonempty = true;
            }
        }
    }

    fn event(&mut self) {
        let Ok(text) = std::str::from_utf8(&self.pending) else {
            self.failed = true;
            return;
        };
        let data = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        let event = text
            .lines()
            .find_map(|line| line.strip_prefix("event:").map(str::trim));
        if matches!(
            event,
            Some("error" | "response.failed" | "response.incomplete")
        ) {
            self.failed = true;
            return;
        }
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            self.failed = true;
            return;
        };
        let kind = value.get("type").and_then(Value::as_str).or(event);
        if matches!(
            kind,
            Some("error" | "response.failed" | "response.incomplete")
        ) || value.get("error").is_some_and(|v| !v.is_null())
        {
            self.failed = true;
        } else if kind == Some("response.completed") {
            let response = value.get("response").unwrap_or(&Value::Null);
            if completed_response(response) {
                self.completed = true;
            } else {
                self.failed = true;
            }
        }
    }

    fn success(&mut self) -> bool {
        if self.failed {
            return false;
        }
        if self.json {
            return serde_json::from_slice::<Value>(&self.pending)
                .ok()
                .is_some_and(|v| {
                    v.get("object").and_then(Value::as_str) == Some("response")
                        && completed_response(&v)
                });
        }
        // 终止事件也必须完整；EOF 后的半个事件不算成功。
        self.completed && self.pending.iter().all(u8::is_ascii_whitespace)
    }
}

fn completed_response(value: &Value) -> bool {
    value.get("status").and_then(Value::as_str) == Some("completed")
        && value.get("error").is_none_or(Value::is_null)
        && value.get("incomplete_details").is_none_or(Value::is_null)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{configured_state, plan, success};
    use super::*;

    #[test]
    fn completion_is_bounded_and_checks_both_sse_error_fields() {
        for suffix in [
            "event: response.failed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"error\":{\"code\":\"capacity\"}}}\n\n",
        ] {
            let mut c = Completion::default();
            c.push(&success().unwrap().body);
            c.push(suffix.as_bytes());
            assert!(!c.success());
        }
        let mut c = Completion::default();
        c.push(&vec![b'x'; MAX_EVENT_BYTES + 1]);
        c.push(&success().unwrap().body);
        assert!(!c.success());
        assert!(c.pending.is_empty());
        let mut c = Completion::default();
        for chunk in success().unwrap().body.chunks(1) {
            c.push(chunk);
        }
        assert!(c.success());
    }

    #[tokio::test]
    async fn changed_credential_and_unavailable_egress_cannot_publish() {
        for change in ["credential", "egress"] {
            let state = configured_state("codex", "oauth");
            let p = plan();
            let transport = state
                .read_provider_transport_snapshot("provider", "endpoint", "account-a")
                .await
                .unwrap()
                .unwrap();
            let mut context = Context::capture(&state, &p, &transport).await.unwrap();
            if change == "credential" {
                context
                    .plan
                    .headers
                    .insert("authorization".into(), "Bearer old-secret".into());
            } else {
                context.plan.proxy = Some(
                    serde_json::from_value(
                        json!({"enabled":true,"mode":"tunnel","node_id":"unavailable"}),
                    )
                    .unwrap(),
                );
            }
            let response = success().unwrap();
            let c = Candidate::new(
                &state,
                context,
                response.headers[HEADER].clone(),
                10,
                &response.headers,
            );
            let result = c.publish().await;
            assert!(result.is_err() || result.ok() == Some("configuration_changed"));
            assert!(!state
                .runtime_state
                .kv_exists(&format!("codex-state:active:{}", state_scope(&state, &p)))
                .await
                .unwrap());
        }
    }
}

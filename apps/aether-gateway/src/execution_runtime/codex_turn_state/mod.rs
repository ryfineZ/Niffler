//! Codex OAuth 同出口 state 管理。秘密只存在于临时派发计划和加密缓存中。
pub(crate) mod background;
pub(crate) mod compact_route;
pub(crate) mod diagnostics;
pub(crate) mod passive;
mod policy;
mod probe;
use probe::{probe_once, ProbeFailure, ProbeObservation, ProbeResponse, ProbeResult};

use std::collections::BTreeMap;
use std::future::Future;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aether_contracts::{
    ExecutionPlan, ExecutionResult, ExecutionTimeouts, RequestBody, ResponseBody,
};
use aether_crypto::{decrypt_python_fernet_ciphertext, encrypt_python_fernet_plaintext};
use axum::body::Bytes;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::transport::{
    execute_stream_plan_via_local_tunnel, DirectSyncExecutionRuntime, DirectUpstreamResponse,
    DirectUpstreamStreamExecution,
};
use crate::AppState;

pub(crate) const CONFIG_KEY: &str = "codex_turn_state_enabled";
pub(crate) const FALLBACK_CONFIG_KEY: &str = "codex_turn_state_fallback";
const ATTEMPTS_CONFIG_KEY: &str = "codex_turn_state_probe_attempts";
const COOLDOWN_CONFIG_KEY: &str = "codex_turn_state_probe_cooldown_seconds";
pub(crate) const HEADER: &str = "x-codex-turn-state";
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const LEASE_TTL: Duration = Duration::from_secs(30);
const MAX_PROBE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct StateError {
    code: &'static str,
    status: u16,
    sent: bool,
}

impl StateError {
    pub(crate) fn status(self) -> u16 {
        self.status
    }
    pub(crate) fn egress_unavailable(self) -> bool {
        self.code == "codex_turn_state_egress_unavailable"
    }
    fn is_unavailable(self) -> bool {
        self.code == "codex_turn_state_unavailable"
    }
    fn egress() -> Self {
        Self {
            code: "codex_turn_state_egress_unavailable",
            status: 503,
            sent: false,
        }
    }
    fn unavailable() -> Self {
        Self {
            code: "codex_turn_state_unavailable",
            status: 503,
            sent: false,
        }
    }
    fn runtime() -> Self {
        Self {
            code: "codex_turn_state_runtime_unavailable",
            status: 503,
            sent: false,
        }
    }
    fn shape() -> Self {
        Self {
            code: "codex_turn_state_shape_changed",
            status: 503,
            sent: true,
        }
    }
    fn blocked(status: u16) -> Self {
        Self {
            code: "codex_turn_state_account_blocked",
            status,
            sent: false,
        }
    }
    fn after_dispatch(mut self) -> Self {
        self.sent = true;
        self
    }
    fn body(self) -> Value {
        json!({"error": {"code":self.code, "type":"codex_turn_state_error",
            "message": if self.sent { "上游请求已发出，但 state 校验或共享状态处理失败；请求可能已产生用量，不会自动重发。" }
                else if self.code == "codex_turn_state_account_blocked" && self.status == 429 { "上游账号已触发限流，请等待账号冷却结束。" }
                else if self.code == "codex_turn_state_account_blocked" { "上游已拒绝当前账号凭据，请刷新或更新账号凭据。" }
                else if self.code == "codex_turn_state_runtime_unavailable" { "无法读取或更新 state 共享状态，请检查运行服务。" }
                else if self.code == "codex_turn_state_egress_unavailable" { "当前无法使用已有 state 绑定的出口，请检查对应节点或应用实例。" }
                else { "当前账号没有可用的同出口 state，严格模式已停止请求；可在 Provider 高级设置中选择普通转发兜底。" },
            "upstream_request_sent":self.sent, "upstream_usage_unknown":self.sent}})
    }
    pub(super) fn sync(self, plan: &ExecutionPlan) -> ExecutionResult {
        ExecutionResult {
            request_id: plan.request_id.clone(),
            candidate_id: plan.candidate_id.clone(),
            status_code: self.status,
            headers: self.headers(),
            body: Some(ResponseBody {
                json_body: Some(self.body()),
                body_bytes_b64: None,
            }),
            telemetry: None,
            error: None,
        }
    }
    fn headers(self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("content-type".into(), "application/json".into()),
            ("x-niffler-turn-state".into(), "failed".into()),
        ])
    }
    pub(super) fn stream(self, plan: &ExecutionPlan) -> DirectUpstreamStreamExecution {
        DirectUpstreamStreamExecution {
            request_id: plan.request_id.clone(),
            candidate_id: plan.candidate_id.clone(),
            status_code: self.status,
            headers: self.headers(),
            provider_api_format: plan.provider_api_format.clone(),
            stream_summary_report_context: json!({"provider_api_format":plan.provider_api_format}),
            response: DirectUpstreamResponse::Buffered(Bytes::from(self.body().to_string())),
            started_at: Instant::now(),
            codex_telemetry: None,
            codex_state_candidate: None,
        }
    }
}

pub(crate) fn terminal_error(text: Option<&str>) -> bool {
    text.and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|body| {
            body.pointer("/error/code")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|code| {
            matches!(
                code.as_str(),
                "codex_turn_state_unavailable"
                    | "codex_turn_state_runtime_unavailable"
                    | "codex_turn_state_shape_changed"
                    | "codex_turn_state_account_blocked"
            )
        })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn digest(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}
fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> &'a str {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
        .unwrap_or_default()
}
pub(crate) fn strip(headers: &mut BTreeMap<String, String>) {
    headers.retain(|k, _| !k.eq_ignore_ascii_case(HEADER));
}
/// 清除上游伪造的本地诊断标记；实际注入路径随后重写可信结果。
pub(super) fn mark_unmanaged(headers: &mut BTreeMap<String, String>) {
    headers.retain(|key, _| {
        !key.eq_ignore_ascii_case("x-niffler-turn-state")
            && !key.eq_ignore_ascii_case("x-niffler-state-returned")
    });
    headers.insert("x-niffler-turn-state".into(), "not_applicable".into());
}
pub(crate) fn scrub_context(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|k, _| !k.eq_ignore_ascii_case(HEADER));
            for v in map.values_mut() {
                scrub_context(v);
            }
        }
        Value::Array(values) => {
            for v in values {
                scrub_context(v);
            }
        }
        _ => {}
    }
}

fn eligible(plan: &ExecutionPlan) -> bool {
    let Ok(url) = reqwest::Url::parse(&plan.url) else {
        return false;
    };
    let Some(body) = plan.body.json_body.as_ref() else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("chatgpt.com")
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.path(),
            "/backend-api/codex/responses" | "/backend-api/codex/v1/responses"
        )
        && url.username().is_empty()
        && url.password().is_none()
        && plan.method.eq_ignore_ascii_case("POST")
        && plan.provider_api_format == "openai:responses"
        && plan.client_api_format != "openai:image"
        && !super::codex_compact::is_v2(body)
        && body.get("compaction_trigger").is_none()
        && body.get("context_management").is_none()
        && matches!(
            body.get("model").and_then(Value::as_str),
            Some("gpt-6-astra" | "gpt-5.6-sol" | "gpt-5.6-terra")
        )
        && !body
            .get("tool_choice")
            .and_then(|v| v.as_str().or_else(|| v.get("type").and_then(Value::as_str)))
            .is_some_and(|v| v == "image_generation")
}

async fn enabled(state: &AppState) -> Result<bool, StateError> {
    match state
        .read_system_config_json_value(CONFIG_KEY)
        .await
        .map_err(|_| StateError::runtime())?
    {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(value),
        _ => Err(StateError::runtime()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FallbackMode {
    Passthrough,
    Strict,
}

async fn fallback_mode(state: &AppState) -> Result<FallbackMode, StateError> {
    match state
        .read_system_config_json_value(FALLBACK_CONFIG_KEY)
        .await
        .map_err(|_| StateError::runtime())?
    {
        None | Some(Value::Null) => Ok(FallbackMode::Passthrough),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("passthrough") => {
            Ok(FallbackMode::Passthrough)
        }
        Some(Value::String(value)) if value.eq_ignore_ascii_case("strict") => {
            Ok(FallbackMode::Strict)
        }
        _ => Err(StateError::runtime()),
    }
}

async fn collection_setting(
    state: &AppState,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, StateError> {
    match state
        .read_system_config_json_value(key)
        .await
        .map_err(|_| StateError::runtime())?
    {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(StateError::runtime),
    }
}

fn state_scope(state: &AppState, original: &ExecutionPlan) -> String {
    let account_key = digest(
        json!([
            original.provider_id,
            original.key_id,
            header(&original.headers, "chatgpt-account-id")
        ])
        .to_string(),
    );
    digest(
        json!([
            account_key,
            digest(header(&original.headers, "authorization")),
            original
                .body
                .json_body
                .as_ref()
                .and_then(|b| b.get("model")),
            route_scope(original, state.tunnel.local_instance_id()),
            "same-egress-v1"
        ])
        .to_string(),
    )
}

/// 节点和配置而非数组位置；本地代理和直连额外绑定执行实例。
fn route_scope(plan: &ExecutionPlan, instance: &str) -> String {
    let proxy = plan.proxy.as_ref().filter(|p| p.enabled != Some(false));
    let shared = proxy.is_some_and(|p| {
        let tunnel = p.node_id.as_deref().is_some_and(|id| !id.trim().is_empty())
            && (p
                .mode
                .as_deref()
                .is_some_and(|m| m.trim().eq_ignore_ascii_case("tunnel"))
                || p.url.as_deref().is_none_or(|url| url.trim().is_empty()));
        tunnel
            || p.url
                .as_deref()
                .and_then(|u| reqwest::Url::parse(u).ok())
                .and_then(|u| u.host_str().map(str::to_owned))
                .is_some_and(|h| {
                    let host = h.trim_matches(['[', ']']);
                    host != "localhost"
                        && !host.ends_with(".localhost")
                        && host
                            .parse::<std::net::IpAddr>()
                            .map_or(true, |ip| !ip.is_loopback() && !ip.is_unspecified())
                })
    });
    digest(json!({"mode":proxy.and_then(|p|p.mode.as_deref()), "node":proxy.and_then(|p|p.node_id.as_deref()),
        "url":proxy.and_then(|p|p.url.as_deref()), "transport":plan.transport_profile,
        "instance": if shared {"shared"} else {instance}}).to_string())
}

#[derive(Serialize, Deserialize)]
struct Cached {
    token: String,
    version: String,
    #[serde(default)]
    source: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct AuthRejection {
    status: u16,
}
#[derive(Default, Serialize, Deserialize)]
struct RateLimit {
    retry_until: u64,
}

// 不派生 Debug：临时 plan 有认证和注入值。
pub(super) struct Prepared {
    pub(super) plan: ExecutionPlan,
    injected: bool,
    cache_key: String,
    cached_value: String,
    guard_key: String,
    credential: String,
    blocks: usize,
    publication: Option<Box<passive::Context>>,
}

pub(super) async fn prepare(
    state: &AppState,
    original: &ExecutionPlan,
) -> Result<Option<Prepared>, StateError> {
    // 在公共入口截断采集状态机的 Future 布局，避免上层路由展开过深。
    Box::pin(prepare_with_fallback_probe(
        state,
        original,
        |plan| async move { probe_once(state, &plan).await },
    ))
    .await
}

async fn prepare_with_fallback_probe<F, Fut>(
    state: &AppState,
    original: &ExecutionPlan,
    probe: F,
) -> Result<Option<Prepared>, StateError>
where
    F: Fn(ExecutionPlan) -> Fut,
    Fut: Future<Output = ProbeResult>,
{
    if !eligible(original) || !enabled(state).await? {
        return Ok(None);
    }
    let fallback = fallback_mode(state).await?;
    let result = tokio::time::timeout(
        Duration::from_secs(25),
        prepare_with_probe_options(
            state,
            original,
            probe,
            true,
            fallback == FallbackMode::Strict,
        ),
    )
    .await
    .unwrap_or_else(|_| Err(StateError::unavailable()));
    match result {
        Err(error) if fallback == FallbackMode::Passthrough && error.is_unavailable() => {
            // 超时也必须重新验证账号保护，不能把未完成的保护检查当作允许派发。
            tokio::time::timeout(Duration::from_secs(3), prepare_passthrough(state, original))
                .await
                .unwrap_or_else(|_| Err(StateError::runtime()))
        }
        result => result,
    }
}

async fn prepare_passthrough(
    state: &AppState,
    original: &ExecutionPlan,
) -> Result<Option<Prepared>, StateError> {
    if !enabled(state).await? {
        return Ok(None);
    }
    let transport = state
        .read_provider_transport_snapshot(
            &original.provider_id,
            &original.endpoint_id,
            &original.key_id,
        )
        .await
        .map_err(|_| StateError::runtime())?
        .ok_or_else(StateError::runtime)?;
    if !transport
        .provider
        .provider_type
        .eq_ignore_ascii_case("codex")
        || !transport.key.auth_type.eq_ignore_ascii_case("oauth")
    {
        return Ok(None);
    }
    let credential = digest(header(&original.headers, "authorization"));
    let account = header(&original.headers, "chatgpt-account-id");
    let guard_key = format!(
        "codex-state:guard:{}",
        digest(json!([original.provider_id, original.key_id, account]).to_string())
    );
    check_guard(state, &guard_key, &credential).await?;
    let mut plan = original.clone();
    strip(&mut plan.headers);
    plan.headers.retain(|name, _| {
        !name.eq_ignore_ascii_case(aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER)
    });
    plan.headers.insert(
        aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER.into(),
        "false".into(),
    );
    tracing::info!(event_name="codex_state_passthrough",request_id=%original.request_id,key_id=%original.key_id,"没有可用 state，按已选账号和出口正常转发");
    Ok(Some(Prepared {
        plan,
        publication: passive::Context::capture(state, original, &transport)
            .await
            .map(Box::new),
        injected: false,
        cache_key: String::new(),
        cached_value: String::new(),
        guard_key,
        credential,
        blocks: policy::expected_blocks(header(&original.headers, "authorization"), account),
    }))
}

#[cfg(test)]
async fn prepare_with_probe<F, Fut>(
    state: &AppState,
    original: &ExecutionPlan,
    probe: F,
) -> Result<Option<Prepared>, StateError>
where
    F: Fn(ExecutionPlan) -> Fut,
    Fut: Future<Output = ProbeResult>,
{
    prepare_with_probe_mode(state, original, probe, true).await
}

async fn prepare_with_probe_mode<F, Fut>(
    state: &AppState,
    original: &ExecutionPlan,
    probe: F,
    activate_binding: bool,
) -> Result<Option<Prepared>, StateError>
where
    F: Fn(ExecutionPlan) -> Fut,
    Fut: Future<Output = ProbeResult>,
{
    prepare_with_probe_options(state, original, probe, activate_binding, true).await
}

async fn prepare_with_probe_options<F, Fut>(
    state: &AppState,
    original: &ExecutionPlan,
    probe: F,
    activate_binding: bool,
    collect_missing: bool,
) -> Result<Option<Prepared>, StateError>
where
    F: Fn(ExecutionPlan) -> Fut,
    Fut: Future<Output = ProbeResult>,
{
    if !eligible(original) || !enabled(state).await? {
        return Ok(None);
    }
    let transport = state
        .read_provider_transport_snapshot(
            &original.provider_id,
            &original.endpoint_id,
            &original.key_id,
        )
        .await
        .map_err(|_| StateError::runtime())?
        .ok_or_else(StateError::runtime)?;
    if !transport
        .provider
        .provider_type
        .eq_ignore_ascii_case("codex")
        || !transport.key.auth_type.eq_ignore_ascii_case("oauth")
    {
        return Ok(None);
    }
    let authorization = header(&original.headers, "authorization");
    let account = header(&original.headers, "chatgpt-account-id");
    if !authorization.starts_with("Bearer ") || account.is_empty() {
        return Err(StateError::unavailable());
    }
    if activate_binding {
        diagnostics::register(state, original, &transport).await;
        background::remember_with_transport(state, original, &transport).await?;
    }
    let credential = digest(authorization);
    let account_key = digest(json!([original.provider_id, original.key_id, account]).to_string());
    let scope = state_scope(state, original);
    let cache_key = format!("codex-state:active:{scope}");
    let guard_key = format!("codex-state:guard:{account_key}");
    let blocks = policy::expected_blocks(authorization, account);
    let secret = state.encryption_key().ok_or_else(StateError::runtime)?;
    let deadline = Instant::now() + Duration::from_secs(22);
    loop {
        check_guard(state, &guard_key, &credential).await?;
        if let Some(stored) = state
            .runtime_state
            .kv_get(&cache_key)
            .await
            .map_err(|_| StateError::runtime())?
        {
            let plaintext = decrypt_python_fernet_ciphertext(secret, &stored)
                .map_err(|_| StateError::runtime())?;
            let cached: Cached =
                serde_json::from_str(&plaintext).map_err(|_| StateError::runtime())?;
            if policy::accepts(&cached.token, blocks, now()) {
                // 只有实际生成请求选择 state 时才更新 Compact 出口索引；后台不能抢占。
                if activate_binding {
                    compact_route::remember(state, original, &transport, &cache_key, &cached)
                        .await?;
                }
                let mut plan = original.clone();
                strip(&mut plan.headers);
                plan.headers.insert(HEADER.into(), cached.token);
                plan.headers.retain(|name, _| {
                    !name.eq_ignore_ascii_case(
                        aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER,
                    )
                });
                plan.headers.insert(
                    aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER.into(),
                    "false".into(),
                );
                return Ok(Some(Prepared {
                    plan,
                    // 已注入请求不重新发布，避免迟到回包复活被淘汰的版本。
                    publication: None,
                    injected: true,
                    cache_key,
                    cached_value: stored,
                    guard_key,
                    credential,
                    blocks,
                }));
            }
            state
                .runtime_state
                .kv_delete_if_value(&cache_key, &stored)
                .await
                .map_err(|_| StateError::runtime())?;
        }
        if !collect_missing {
            return Err(StateError::unavailable());
        }
        let lock_key = format!("codex-state:collect:{account_key}");
        let Some(lease) = state
            .runtime_state
            .lock_try_acquire(&lock_key, state.tunnel.local_instance_id(), LEASE_TTL)
            .await
            .map_err(|_| StateError::runtime())?
        else {
            if !activate_binding || Instant::now() >= deadline {
                return Err(StateError::unavailable());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        // 采集租约也限定取消后的残留时间；发布原子校验所有者。
        let result = with_lease(
            state,
            &lease,
            collect(
                state,
                original,
                &scope,
                &account_key,
                &guard_key,
                &credential,
                blocks,
                &cache_key,
                &lease,
                &probe,
            ),
        )
        .await;
        let released = state.runtime_state.lock_release(&lease).await;
        if released.is_err() {
            return Err(StateError::runtime());
        }
        result?;
        if !enabled(state).await? {
            return Err(StateError::unavailable());
        }
    }
}

async fn check_guard(state: &AppState, key: &str, credential: &str) -> Result<(), StateError> {
    if let Some(raw) = state
        .runtime_state
        .kv_get(&format!("{key}:rate"))
        .await
        .map_err(|_| StateError::runtime())?
    {
        let guard: RateLimit = serde_json::from_str(&raw).map_err(|_| StateError::runtime())?;
        if guard.retry_until > now() {
            return Err(StateError::blocked(429));
        }
    }
    if let Some(raw) = state
        .runtime_state
        .kv_get(&format!("{key}:auth:{credential}"))
        .await
        .map_err(|_| StateError::runtime())?
    {
        let guard: AuthRejection = serde_json::from_str(&raw).map_err(|_| StateError::runtime())?;
        return Err(StateError::blocked(guard.status));
    }
    Ok(())
}

async fn reject(
    state: &AppState,
    key: &str,
    credential: &str,
    status: u16,
    headers: &BTreeMap<String, String>,
) -> Result<(), StateError> {
    if !matches!(status, 401 | 403 | 429) {
        return Ok(());
    }
    if matches!(status, 401 | 403) {
        // 每个凭据独立；旧请求的迟到响应只能更新旧凭据的拒绝记录。
        let serialized =
            serde_json::to_string(&AuthRejection { status }).map_err(|_| StateError::runtime())?;
        return state
            .runtime_state
            .kv_set(
                &format!("{key}:auth:{credential}"),
                &serialized,
                Some(Duration::from_secs(86400)),
            )
            .await
            .map_err(|_| StateError::runtime());
    }
    let key = format!("{key}:rate");
    let deadline = Instant::now() + Duration::from_secs(1);
    let lease = loop {
        if let Some(lease) = state
            .runtime_state
            .lock_try_acquire(
                &format!("{key}:update"),
                state.tunnel.local_instance_id(),
                Duration::from_secs(5),
            )
            .await
            .map_err(|_| StateError::runtime())?
        {
            break lease;
        }
        if Instant::now() >= deadline {
            return Err(StateError::runtime());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let result = async {
        let previous = state
            .runtime_state
            .kv_get(&key)
            .await
            .map_err(|_| StateError::runtime())?;
        let mut guard = match previous {
            Some(raw) => {
                serde_json::from_str::<RateLimit>(&raw).map_err(|_| StateError::runtime())?
            }
            None => RateLimit::default(),
        };
        let delay = header(headers, "retry-after")
            .parse::<u64>()
            .ok()
            .or_else(|| {
                chrono::DateTime::parse_from_rfc2822(header(headers, "retry-after"))
                    .ok()
                    .map(|date| (date.timestamp().max(0) as u64).saturating_sub(now()))
            })
            .unwrap_or(180)
            // 防止异常 Retry-After 溢出内存后端的 Instant 或 Redis PX。
            .clamp(180, 365 * 86400);
        guard.retry_until = guard.retry_until.max(now().saturating_add(delay));
        let ttl = guard.retry_until.saturating_sub(now()).max(180);
        let serialized = serde_json::to_string(&guard).map_err(|_| StateError::runtime())?;
        if !state
            .runtime_state
            .kv_set_if_lock_owned(&lease, &key, &serialized, Duration::from_secs(ttl))
            .await
            .map_err(|_| StateError::runtime())?
        {
            return Err(StateError::runtime());
        }
        Ok(())
    }
    .await;
    state
        .runtime_state
        .lock_release(&lease)
        .await
        .map_err(|_| StateError::runtime())?;
    result
}

#[allow(clippy::too_many_arguments)]
async fn collect<F, Fut>(
    state: &AppState,
    plan: &ExecutionPlan,
    scope: &str,
    account_key: &str,
    guard_key: &str,
    credential: &str,
    blocks: usize,
    cache_key: &str,
    lease: &aether_runtime_state::RuntimeLockLease,
    run_probe: &F,
) -> Result<(), StateError>
where
    F: Fn(ExecutionPlan) -> Fut,
    Fut: Future<Output = ProbeResult>,
{
    // 拿到锁后重新检查，以合并多个 Frontdoor 的冷启动。
    if state
        .runtime_state
        .kv_exists(cache_key)
        .await
        .map_err(|_| StateError::runtime())?
    {
        return Ok(());
    }
    let cooldown = format!("codex-state:cooldown:v2:{scope}");
    let legacy_cooldown = format!("codex-state:cooldown:{account_key}");
    if state
        .runtime_state
        .kv_exists(&cooldown)
        .await
        .map_err(|_| StateError::runtime())?
        || state
            .runtime_state
            .kv_exists(&legacy_cooldown)
            .await
            .map_err(|_| StateError::runtime())?
    {
        return Err(StateError::unavailable());
    }
    let attempts = collection_setting(state, ATTEMPTS_CONFIG_KEY, 3, 1, 6).await?;
    let cooldown_seconds = collection_setting(state, COOLDOWN_CONFIG_KEY, 30, 30, 3600).await?;
    let round = uuid::Uuid::new_v4().to_string();
    let result = async {
        for attempt in 1..=attempts {
            if attempt > 1 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            if !enabled(state).await? {
                return Err(StateError::unavailable());
            }
            check_guard(state, guard_key, credential).await?;
            // 全局槽每次尝试释放，其他活跃账号可在轮内获得机会。
            let global = loop {
                if let Some(global) = state
                    .runtime_state
                    .lock_try_acquire(
                        "codex-state:global-collector",
                        state.tunnel.local_instance_id(),
                        LEASE_TTL,
                    )
                    .await
                    .map_err(|_| StateError::runtime())?
                {
                    break global;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            let result = with_lease(
                state,
                &global,
                collect_attempt(
                    state, plan, scope, guard_key, credential, blocks, cache_key, lease, &round,
                    attempt, attempts, run_probe,
                ),
            )
            .await;
            state
                .runtime_state
                .lock_release(&global)
                .await
                .map_err(|_| StateError::runtime())?;
            match result {
                Err(error) if error.is_unavailable() => {}
                result => return result,
            }
        }
        Err(StateError::unavailable())
    }
    .await;
    if !state
        .runtime_state
        .kv_set_if_lock_owned(
            lease,
            &cooldown,
            "finished",
            Duration::from_secs(cooldown_seconds),
        )
        .await
        .map_err(|_| StateError::runtime())?
    {
        return Err(StateError::runtime());
    }
    result
}

/// 续期与工作共享生命周期；取消后不留下独立心跳，失去所有权立即丢弃工作。
async fn with_lease<T>(
    state: &AppState,
    lease: &aether_runtime_state::RuntimeLockLease,
    work: impl Future<Output = Result<T, StateError>>,
) -> Result<T, StateError> {
    tokio::pin!(work);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = heartbeat.tick() => {
                if !enabled(state).await? { return Err(StateError::unavailable()); }
                let renewed = tokio::time::timeout(Duration::from_secs(3), state.runtime_state.lock_renew(lease, LEASE_TTL))
                    .await.map_err(|_| StateError::runtime())?.map_err(|_| StateError::runtime())?;
                if !renewed { return Err(StateError::runtime()); }
            }
        }
    }
}

async fn collect_attempt<F, Fut>(
    state: &AppState,
    plan: &ExecutionPlan,
    scope: &str,
    guard_key: &str,
    credential: &str,
    blocks: usize,
    cache_key: &str,
    lease: &aether_runtime_state::RuntimeLockLease,
    round: &str,
    attempt: u64,
    attempts: u64,
    run_probe: &F,
) -> Result<(), StateError>
where
    F: Fn(ExecutionPlan) -> Fut,
    Fut: Future<Output = ProbeResult>,
{
    let probe = probe_plan(plan);
    let started = Instant::now();
    record_probe(
        state,
        scope,
        0,
        Value::Null,
        false,
        "collecting",
        &ProbeObservation::default(),
        round,
        attempt,
        attempts,
    )
    .await;
    let outcome =
        tokio::time::timeout(PROBE_TIMEOUT + Duration::from_secs(1), run_probe(probe)).await;
    let ProbeResponse {
        status,
        headers,
        body,
        mut observation,
    } = match outcome {
        Ok(Ok(value)) => value,
        failure => {
            let failure = match failure {
                Ok(Err(failure)) => failure,
                _ => ProbeFailure {
                    reason: "timeout",
                    observation: ProbeObservation {
                        phase: "unknown",
                        ..Default::default()
                    },
                },
            };
            let mut observation = failure.observation;
            observation.elapsed_ms = started.elapsed().as_millis() as u64;
            record_probe(
                state,
                scope,
                observation.http_status,
                Value::Null,
                false,
                failure.reason,
                &observation,
                round,
                attempt,
                attempts,
            )
            .await;
            return Err(StateError::unavailable());
        }
    };
    observation.elapsed_ms = started.elapsed().as_millis() as u64;
    observation.returned_state = policy::state_reason(header(&headers, HEADER), blocks, now());
    observation.expected_blocks = Some(blocks);
    observation.observed_blocks = policy::parse(header(&headers, HEADER)).map(|(_, n)| n);
    let parsed = if status == 200 {
        policy::probe_outcome(&body)
    } else {
        Err(status)
    };
    let final_status = parsed.as_ref().err().copied().unwrap_or(status);
    reject(state, guard_key, credential, final_status, &headers).await?;
    observation.completed = parsed.is_ok();
    let accepted = parsed.is_ok() && policy::accepts(header(&headers, HEADER), blocks, now());
    let reason = if parsed.is_err() {
        "upstream_error"
    } else if header(&headers, HEADER).is_empty() {
        "missing_state"
    } else if !accepted {
        "invalid_state"
    } else {
        "accepted"
    };
    record_probe(
        state,
        scope,
        final_status,
        parsed.unwrap_or(Value::Null),
        accepted,
        reason,
        &observation,
        round,
        attempt,
        attempts,
    )
    .await;
    if matches!(final_status, 401 | 403 | 429) {
        return Err(StateError::blocked(final_status));
    }
    if !accepted || !enabled(state).await? {
        return Err(StateError::unavailable());
    }
    check_guard(state, guard_key, credential).await?;
    let token = header(&headers, HEADER).to_owned();
    let cache = Cached {
        token,
        version: uuid::Uuid::new_v4().to_string(),
        source: Some("probe".into()),
    };
    publish_cache(state, lease, cache_key, &cache).await
}

async fn publish_cache(
    state: &AppState,
    lease: &aether_runtime_state::RuntimeLockLease,
    cache_key: &str,
    cache: &Cached,
) -> Result<(), StateError> {
    let (issued, _) = policy::parse(&cache.token).ok_or_else(StateError::unavailable)?;
    let plaintext = serde_json::to_string(cache).map_err(|_| StateError::runtime())?;
    let encrypted = encrypt_python_fernet_plaintext(
        state.encryption_key().ok_or_else(StateError::runtime)?,
        &plaintext,
    )
    .map_err(|_| StateError::runtime())?;
    let ttl = Duration::from_secs(
        issued
            .saturating_add(policy::TTL_SECONDS - 30)
            .saturating_sub(now()),
    );
    if ttl.is_zero()
        || !state
            .runtime_state
            .kv_set_if_lock_owned(lease, cache_key, &encrypted, ttl)
            .await
            .map_err(|_| StateError::runtime())?
    {
        return Err(StateError::unavailable());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn record_probe(
    state: &AppState,
    scope: &str,
    status: u16,
    usage: Value,
    accepted: bool,
    reason: &str,
    observation: &ProbeObservation,
    round: &str,
    attempt: u64,
    attempts: u64,
) {
    let record = json!({"purpose":"codex_state_probe","status":status,"usage":usage,"accepted":accepted,"reason":reason,"at":now(),"customer_billed":false,"upstream_usage_unknown":usage.is_null(),"observation":observation,"round":round,"attempt":attempt,"attempt_limit":attempts});
    tracing::info!(event_name="codex_state_probe_finished",scope,status,accepted,reason,attempt,attempts,phase=observation.phase,elapsed_ms=observation.elapsed_ms,usage=%usage,"Codex state 维护探测结束；不计入客户账单");
    if state
        .runtime_state
        .kv_set(
            &format!("codex-state:last-probe:{scope}"),
            record.to_string(),
            Some(Duration::from_secs(86400)),
        )
        .await
        .is_err()
    {
        tracing::warn!(
            event_name = "codex_state_probe_record_failed",
            "无法保存 state 维护探测摘要"
        );
    }
}

fn probe_plan(original: &ExecutionPlan) -> ExecutionPlan {
    let mut plan = original.clone();
    plan.request_id = format!("state-probe-{}", uuid::Uuid::new_v4());
    plan.candidate_id = None;
    plan.headers.retain(|key, _| {
        [
            "authorization",
            "chatgpt-account-id",
            "user-agent",
            "version",
            "originator",
            "openai-beta",
            "x-codex-installation-id",
            aether_contracts::EXECUTION_REQUEST_HTTP1_ONLY_HEADER,
        ]
        .iter()
        .any(|k| key.eq_ignore_ascii_case(k))
    });
    plan.headers
        .insert("accept".into(), "text/event-stream".into());
    plan.headers
        .insert("content-type".into(), "application/json".into());
    plan.headers.insert(
        aether_contracts::EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER.into(),
        "false".into(),
    );
    plan.headers.insert(
        aether_contracts::EXECUTION_REQUEST_ACCEPT_INVALID_CERTS_HEADER.into(),
        "false".into(),
    );
    plan.content_type = Some("application/json".into());
    plan.content_encoding = None;
    plan.stream = true;
    plan.body = RequestBody::from_json(
        json!({"model":original.body.json_body.as_ref().and_then(|v|v.get("model")),"instructions":"Reply with OK.",
        "input":[{"role":"user","type":"message","content":[{"type":"input_text","text":"Reply with OK."}]}],
        "stream":true,"store":false,"parallel_tool_calls":true,"include":["reasoning.encrypted_content"]}),
    );
    plan.timeouts = Some(ExecutionTimeouts {
        total_ms: Some(20_000),
        connect_ms: Some(10_000),
        ..Default::default()
    });
    plan
}

impl Prepared {
    pub(super) async fn observe(
        &self,
        state: &AppState,
        status: u16,
        headers: &mut BTreeMap<String, String>,
    ) -> Result<Option<Box<passive::Candidate>>, StateError> {
        let returned = header(headers, HEADER).to_owned();
        let returned_state = policy::state_reason(&returned, self.blocks, now());
        strip(headers);
        headers.insert("x-niffler-state-returned".into(), returned_state.into());
        headers.insert(
            "x-niffler-turn-state".into(),
            if self.injected {
                "injected"
            } else {
                "passthrough"
            }
            .into(),
        );
        reject(state, &self.guard_key, &self.credential, status, headers)
            .await
            .map_err(StateError::after_dispatch)?;
        if self.injected
            && (200..300).contains(&status)
            && !returned.is_empty()
            && !policy::accepts(&returned, self.blocks, now())
        {
            state
                .runtime_state
                .kv_delete_if_value(&self.cache_key, &self.cached_value)
                .await
                .map_err(|_| StateError::runtime().after_dispatch())?;
            tracing::warn!(event_name="codex_state_response_rejected",request_id=%self.plan.request_id,key_id=%self.plan.key_id,upstream_usage_unknown=true,"正式请求已派发，state 失效；停止当前响应且不重放");
            diagnostics::record_use(
                state,
                self,
                status,
                true,
                returned_state,
                policy::parse(&returned).map(|(_, n)| n),
            )
            .await;
            return Err(StateError::shape());
        }
        diagnostics::record_use(
            state,
            self,
            status,
            false,
            returned_state,
            policy::parse(&returned).map(|(_, n)| n),
        )
        .await;
        Ok(self
            .publication
            .as_ref()
            .filter(|_| (200..300).contains(&status) && returned_state == "qualified")
            .map(|context| {
                Box::new(passive::Candidate::new(
                    state,
                    *context.clone(),
                    returned,
                    self.blocks,
                    headers,
                ))
            }))
    }
}

#[cfg(test)]
mod tests;

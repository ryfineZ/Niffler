//! 每个 Frontdoor 仅为本实例最近活跃的普通生成会话补采。
use super::*;
use crate::provider_transport::oauth_refresh::LocalOAuthRefreshAdapter;
use crate::provider_transport::{
    GatewayProviderTransportSnapshot, GenericOAuthRefreshAdapter, LocalResolvedOAuthRequestAuth,
};
use std::collections::HashMap;
use std::sync::Mutex;

const ACTIVE_FOR: Duration = Duration::from_secs(30 * 60);
const TICK: Duration = Duration::from_secs(30);
const MAX_SESSIONS: usize = 1024;

#[derive(Clone)]
struct Session {
    encrypted_plan: String,
    configuration: String,
    last_seen: Instant,
    last_checked: Instant,
}

#[derive(Default)]
pub(crate) struct Sessions(Mutex<HashMap<String, Session>>);

impl std::fmt::Debug for Sessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CodexTurnStateSessions(..)")
    }
}

pub(super) async fn configuration(
    state: &AppState,
    transport: &GatewayProviderTransportSnapshot,
) -> Result<String, StateError> {
    let mut value = serde_json::to_value(transport).map_err(|_| StateError::runtime())?;
    // 展示、计价和调度元数据不改变 State 身份；权限、凭据及出站配置仍完整校验。
    for (section, fields) in [
        (
            "provider",
            &[
                "name",
                "website",
                "keep_priority_on_conversion",
                "enable_format_conversion",
                "concurrent_limit",
                "max_retries",
                "request_timeout_secs",
                "stream_first_byte_timeout_secs",
            ][..],
        ),
        ("endpoint", &["max_retries"][..]),
        (
            "key",
            &["name", "rate_multipliers", "global_priority_by_format"][..],
        ),
    ] {
        if let Some(object) = value[section].as_object_mut() {
            for field in fields {
                object.remove(*field);
            }
        }
    }
    Ok(canonical_digest(json!([value, system_proxy(state).await?])))
}

pub(super) async fn system_proxy(state: &AppState) -> Result<Option<Value>, StateError> {
    state
        .read_system_config_json_value("system_proxy_node_id")
        .await
        .map_err(|_| StateError::runtime())
}

pub(super) async fn legacy_configuration(
    state: &AppState,
    transport: &GatewayProviderTransportSnapshot,
) -> Result<String, StateError> {
    Ok(digest(
        json!([transport, system_proxy(state).await?]).to_string(),
    ))
}

pub(super) async fn egress_configuration(
    state: &AppState,
    transport: &GatewayProviderTransportSnapshot,
) -> Result<String, StateError> {
    Ok(canonical_digest(json!([
        transport.provider.proxy,
        transport.endpoint.proxy,
        transport.key.proxy,
        transport.key.fingerprint,
        transport.endpoint.base_url,
        transport.endpoint.custom_path,
        system_proxy(state).await?
    ])))
}

fn canonical_digest(mut value: Value) -> String {
    value.sort_all_objects();
    digest(value.to_string())
}

#[cfg(test)]
async fn remember(state: &AppState, plan: &ExecutionPlan) -> Result<(), StateError> {
    let transport = state
        .read_provider_transport_snapshot(&plan.provider_id, &plan.endpoint_id, &plan.key_id)
        .await
        .map_err(|_| StateError::runtime())?
        .ok_or_else(StateError::runtime)?;
    remember_with_transport(state, plan, &transport).await
}

pub(super) fn current_credential_matches(
    plan: &ExecutionPlan,
    transport: &GatewayProviderTransportSnapshot,
) -> bool {
    matches!(GenericOAuthRefreshAdapter::default().resolve_without_refresh(transport),
        Some(LocalResolvedOAuthRequestAuth::Header { name, value })
        if name.eq_ignore_ascii_case("authorization") && value == header(&plan.headers, "authorization"))
}

pub(super) async fn remember_with_transport(
    state: &AppState,
    plan: &ExecutionPlan,
    transport: &GatewayProviderTransportSnapshot,
) -> Result<(), StateError> {
    if !transport
        .provider
        .provider_type
        .eq_ignore_ascii_case("codex")
        || !transport.key.auth_type.eq_ignore_ascii_case("oauth")
        || !transport.provider.is_active
        || !transport.endpoint.is_active
        || !transport.key.is_active
        || !header(&plan.headers, "authorization").starts_with("Bearer ")
        || header(&plan.headers, "chatgpt-account-id").is_empty()
        || !current_credential_matches(plan, transport)
    {
        return Ok(());
    }
    let scope = state_scope(state, plan);
    let configuration = configuration(state, transport).await?;
    let mut sessions = state
        .codex_turn_state_sessions
        .0
        .lock()
        .map_err(|_| StateError::runtime())?;
    sessions.retain(|_, s| s.last_seen.elapsed() < ACTIVE_FOR);
    if let Some(session) = sessions
        .get_mut(&scope)
        .filter(|s| s.configuration == configuration)
    {
        session.last_seen = Instant::now();
        return Ok(());
    }
    let encrypted_plan = encrypt_python_fernet_plaintext(
        state.encryption_key().ok_or_else(StateError::runtime)?,
        &serde_json::to_string(&probe_plan(plan)).map_err(|_| StateError::runtime())?,
    )
    .map_err(|_| StateError::runtime())?;
    if sessions.len() >= MAX_SESSIONS {
        if let Some(oldest) = sessions
            .iter()
            .min_by_key(|(_, s)| s.last_seen)
            .map(|(id, _)| id.clone())
        {
            sessions.remove(&oldest);
        }
    }
    sessions.insert(
        scope,
        Session {
            encrypted_plan,
            configuration,
            last_seen: Instant::now(),
            last_checked: Instant::now(),
        },
    );
    Ok(())
}

async fn current_plan(
    state: &AppState,
    session: &Session,
) -> Result<Option<ExecutionPlan>, StateError> {
    let mut plan: ExecutionPlan = serde_json::from_str(
        &decrypt_python_fernet_ciphertext(
            state.encryption_key().ok_or_else(StateError::runtime)?,
            &session.encrypted_plan,
        )
        .map_err(|_| StateError::runtime())?,
    )
    .map_err(|_| StateError::runtime())?;
    let Some(transport) = state
        .read_provider_transport_snapshot(&plan.provider_id, &plan.endpoint_id, &plan.key_id)
        .await
        .map_err(|_| StateError::runtime())?
    else {
        return Ok(None);
    };
    if !current_credential_matches(&plan, &transport)
        || !transport.provider.is_active
        || !transport.endpoint.is_active
        || !transport.key.is_active
        || transport
            .key
            .expires_at_unix_secs
            .is_some_and(|expires| expires <= now())
        || configuration(state, &transport).await? != session.configuration
    {
        return Ok(None);
    }
    if let Some(node) = plan
        .proxy
        .as_ref()
        .and_then(|p| p.node_id.as_deref())
        .filter(|id| !id.is_empty())
    {
        let original_route = route_scope(&plan, state.tunnel.local_instance_id());
        let Some(proxy) = state.resolve_proxy_node_snapshot(Some(node)).await else {
            return Err(StateError::egress());
        };
        plan.proxy = Some(proxy);
        if route_scope(&plan, state.tunnel.local_instance_id()) != original_route {
            return Ok(None);
        }
    }
    Ok(Some(plan))
}

async fn tick_with_probe<F, Fut>(state: &AppState, probe: F) -> Result<(), StateError>
where
    F: Fn(ExecutionPlan) -> Fut,
    Fut: Future<Output = ProbeResult>,
{
    if !enabled(state).await? {
        state
            .codex_turn_state_sessions
            .0
            .lock()
            .map_err(|_| StateError::runtime())?
            .clear();
        return Ok(());
    }
    let mut pending = {
        let mut sessions = state
            .codex_turn_state_sessions
            .0
            .lock()
            .map_err(|_| StateError::runtime())?;
        sessions.retain(|_, s| s.last_seen.elapsed() < ACTIVE_FOR);
        sessions
            .iter()
            .map(|(id, s)| (id.clone(), s.clone()))
            .collect::<Vec<_>>()
    };
    pending.sort_by_key(|(_, s)| s.last_checked);
    let started = Instant::now();
    for (scope, session) in pending {
        if started.elapsed() >= Duration::from_secs(25) {
            break;
        }
        if session.last_seen.elapsed() >= ACTIVE_FOR || !enabled(state).await? {
            continue;
        }
        if let Some(current) = state
            .codex_turn_state_sessions
            .0
            .lock()
            .map_err(|_| StateError::runtime())?
            .get_mut(&scope)
        {
            current.last_checked = Instant::now();
        }
        let resolved = match current_plan(state, &session).await {
            Err(error) if error.egress_unavailable() => {
                tracing::debug!(
                    event_name = "codex_state_background_egress_unavailable",
                    scope,
                    "补采出口暂不可用，保留活跃会话等待下轮检查"
                );
                continue;
            }
            result => result?,
        };
        let Some(plan) = resolved else {
            let mut sessions = state
                .codex_turn_state_sessions
                .0
                .lock()
                .map_err(|_| StateError::runtime())?;
            // 不让迟到的配置检查删除刚刚由新请求更新的会话。
            if sessions
                .get(&scope)
                .is_some_and(|s| s.encrypted_plan == session.encrypted_plan)
            {
                sessions.remove(&scope);
            }
            continue;
        };
        let guarded_probe = |mut probe_plan: ExecutionPlan| {
            let session = &session;
            let probe = &probe;
            async move {
                // 等待共享槽和轮内重试期间配置可能变化，每次真正派发前再验证。
                match current_plan(state, session).await {
                    Ok(Some(current)) => {
                        probe_plan.proxy = current.proxy;
                        probe(probe_plan).await
                    }
                    Ok(None) => Err(()),
                    Err(error) => {
                        tracing::warn!(
                            event_name = "codex_state_background_revalidation_failed",
                            code = error.code,
                            "补采前复核失败，本次不派发"
                        );
                        Err(())
                    }
                }
            }
        };
        if let Err(error) = tokio::time::timeout(
            Duration::from_secs(25),
            prepare_with_probe_mode(state, &plan, guarded_probe, false),
        )
        .await
        .unwrap_or_else(|_| Err(StateError::unavailable()))
        {
            if !error.is_unavailable() && error.code != "codex_turn_state_account_blocked" {
                return Err(error);
            }
        }
    }
    Ok(())
}

pub(crate) async fn run(state: AppState) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if tick_with_probe(&state, |plan| {
            let state = &state;
            async move { probe_once(state, &plan).await }
        })
        .await
        .is_err()
        {
            tracing::warn!(
                event_name = "codex_state_background_failed",
                "state 后台补采无法读取或更新共享状态，下一轮再试"
            );
        }
    }
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;

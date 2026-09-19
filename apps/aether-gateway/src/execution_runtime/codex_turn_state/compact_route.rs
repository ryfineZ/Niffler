//! 加密索引仅用于查找仍有效的 state 采集出口；不把 state 注入压缩请求。
use super::*;
use crate::provider_transport::GatewayProviderTransportSnapshot;
use aether_contracts::{ProxySnapshot, ResolvedTransportProfile};

#[derive(Serialize, Deserialize)]
struct Binding {
    cache_key: String,
    version: String,
    source_endpoint: String,
    config: String,
    proxy: Option<ProxySnapshot>,
    transport_profile: Option<ResolvedTransportProfile>,
    instance: String,
}

fn index_key(plan: &ExecutionPlan) -> String {
    format!(
        "codex-state:egress:{}",
        digest(
            json!([
                plan.provider_id,
                plan.key_id,
                header(&plan.headers, "chatgpt-account-id"),
                digest(header(&plan.headers, "authorization")),
                plan.body.json_body.as_ref().and_then(|v| v.get("model")),
                "same-egress-v1"
            ])
            .to_string()
        )
    )
}

async fn configuration(
    state: &AppState,
    transport: &GatewayProviderTransportSnapshot,
) -> Result<String, StateError> {
    let system = state
        .read_system_config_json_value("system_proxy_node_id")
        .await
        .map_err(|_| StateError::runtime())?;
    Ok(digest(
        json!([
            transport.provider.proxy,
            transport.provider.config,
            transport.endpoint.proxy,
            transport.endpoint.config,
            transport.endpoint.base_url,
            transport.endpoint.custom_path,
            transport.key.proxy,
            transport.key.fingerprint,
            system
        ])
        .to_string(),
    ))
}

pub(super) async fn remember(
    state: &AppState,
    plan: &ExecutionPlan,
    transport: &GatewayProviderTransportSnapshot,
    cache_key: &str,
    cached: &Cached,
) -> Result<(), StateError> {
    let issued = policy::parse(&cached.token)
        .ok_or_else(StateError::unavailable)?
        .0;
    let ttl = issued
        .saturating_add(policy::TTL_SECONDS - 30)
        .saturating_sub(now());
    if ttl == 0 {
        return Err(StateError::unavailable());
    }
    let binding = Binding {
        cache_key: cache_key.into(),
        version: cached.version.clone(),
        source_endpoint: plan.endpoint_id.clone(),
        config: configuration(state, transport).await?,
        proxy: plan
            .proxy
            .as_ref()
            .filter(|proxy| proxy.enabled != Some(false))
            .cloned(),
        transport_profile: plan.transport_profile.clone(),
        instance: state.tunnel.local_instance_id().into(),
    };
    let encrypted = encrypt_python_fernet_plaintext(
        state.encryption_key().ok_or_else(StateError::runtime)?,
        &serde_json::to_string(&binding).map_err(|_| StateError::runtime())?,
    )
    .map_err(|_| StateError::runtime())?;
    state
        .runtime_state
        .kv_set(&index_key(plan), &encrypted, Some(Duration::from_secs(ttl)))
        .await
        .map_err(|_| StateError::runtime())
}

pub(crate) struct CompactRoute {
    pub(crate) plan: ExecutionPlan,
    pub(crate) bound: bool,
    guard: Option<(String, String)>,
}

impl CompactRoute {
    pub(crate) async fn observe(
        &self,
        state: &AppState,
        status: u16,
        headers: &BTreeMap<String, String>,
    ) -> Result<(), StateError> {
        if let Some((key, credential)) = &self.guard {
            reject(state, key, credential, status, headers)
                .await
                .map_err(StateError::after_dispatch)?;
        }
        Ok(())
    }
}

pub(crate) async fn prepare(
    state: &AppState,
    original: &ExecutionPlan,
) -> Result<CompactRoute, StateError> {
    let mut result = CompactRoute {
        plan: original.clone(),
        bound: false,
        guard: None,
    };
    if !enabled(state).await? {
        return Ok(result);
    }
    let authorization = header(&original.headers, "authorization");
    let account = header(&original.headers, "chatgpt-account-id");
    let credential = digest(authorization);
    let guard = format!(
        "codex-state:guard:{}",
        digest(json!([original.provider_id, original.key_id, account]).to_string())
    );
    check_guard(state, &guard, &credential).await?;
    result.guard = Some((guard, credential));
    let Some(encrypted) = state
        .runtime_state
        .kv_get(&index_key(original))
        .await
        .map_err(|_| StateError::runtime())?
    else {
        return Ok(result);
    };
    let secret = state.encryption_key().ok_or_else(StateError::runtime)?;
    let binding: Binding = serde_json::from_str(
        &decrypt_python_fernet_ciphertext(secret, &encrypted).map_err(|_| StateError::runtime())?,
    )
    .map_err(|_| StateError::runtime())?;
    let Some(encrypted_cache) = state
        .runtime_state
        .kv_get(&binding.cache_key)
        .await
        .map_err(|_| StateError::runtime())?
    else {
        return Ok(result);
    };
    let cached: Cached = serde_json::from_str(
        &decrypt_python_fernet_ciphertext(secret, &encrypted_cache)
            .map_err(|_| StateError::runtime())?,
    )
    .map_err(|_| StateError::runtime())?;
    if cached.version != binding.version
        || !policy::accepts(
            &cached.token,
            policy::expected_blocks(authorization, account),
            now(),
        )
    {
        return Ok(result);
    }
    // 检查采集时的配置仍获授权，不能用旧索引撤销管理员的新配置。
    let Some(source) = state
        .read_provider_transport_snapshot(
            &original.provider_id,
            &binding.source_endpoint,
            &original.key_id,
        )
        .await
        .map_err(|_| StateError::runtime())?
    else {
        return Ok(result);
    };
    if !source.provider.is_active
        || !source.endpoint.is_active
        || !source.key.is_active
        || configuration(state, &source).await? != binding.config
        || original.transport_profile != binding.transport_profile
    {
        return Ok(result);
    }
    result.plan.proxy = binding.proxy.clone();
    result.plan.transport_profile = binding.transport_profile;
    let original_scope = route_scope(&result.plan, &binding.instance);
    if route_scope(&result.plan, state.tunnel.local_instance_id()) != original_scope {
        return Err(StateError::egress());
    }
    // Tunnel 的附着机器会变化，只复用节点身份，不复用旧 relay 地址。
    if let Some(node) = binding
        .proxy
        .as_ref()
        .and_then(|p| p.node_id.as_deref())
        .filter(|s| !s.is_empty())
    {
        result.plan.proxy = Some(
            state
                .resolve_proxy_node_snapshot(Some(node))
                .await
                .ok_or_else(StateError::egress)?,
        );
        if route_scope(&result.plan, state.tunnel.local_instance_id()) != original_scope {
            return Err(StateError::egress());
        }
    }
    result.bound = true;
    Ok(result)
}

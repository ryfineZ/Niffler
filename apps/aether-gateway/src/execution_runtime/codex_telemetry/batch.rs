//! 每账号/出口/指纹一分钟一批；保留独立 resource 和 data point，不混算模型。
use aether_contracts::ExecutionPlan;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

static PENDING: LazyLock<Mutex<HashMap<String, (ExecutionPlan, usize)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const MAX_BATCH_BYTES: usize = 1024 * 1024;

pub(super) struct BatchGuard(Option<String>);
impl BatchGuard {
    pub(super) fn take(&mut self) -> Option<ExecutionPlan> {
        let key = self.0.take()?;
        PENDING.lock().ok()?.remove(&key).map(|(plan, _)| plan)
    }
}
impl Drop for BatchGuard {
    fn drop(&mut self) {
        if let Some(key) = self.0.take() {
            if let Ok(mut pending) = PENDING.lock() {
                pending.remove(&key);
            }
        }
    }
}

pub(super) fn enqueue(plan: &ExecutionPlan) -> Option<BatchGuard> {
    let body = plan.body.json_body.as_ref()?;
    let bytes = serde_json::to_vec(body).ok()?.len();
    if bytes > MAX_BATCH_BYTES {
        return None;
    }
    let identity = serde_json::to_vec(&(
        &plan.key_id,
        &plan.proxy,
        &plan.transport_profile,
        &plan.headers,
    ))
    .ok()?;
    let key = format!("{:x}", Sha256::digest(identity));
    let mut pending = PENDING.lock().ok()?;
    if let Some((queued, size)) = pending.get_mut(&key) {
        if *size + bytes > MAX_BATCH_BYTES {
            tracing::warn!(event_name = "codex_telemetry_dropped", "Codex 指标批次已满");
            return None;
        }
        let resources = queued
            .body
            .json_body
            .as_mut()?
            .get_mut("resourceMetrics")?
            .as_array_mut()?;
        resources.extend(body.get("resourceMetrics")?.as_array()?.iter().cloned());
        *size += bytes;
        return None;
    }
    pending.insert(key.clone(), (plan.clone(), bytes));
    Some(BatchGuard(Some(key)))
}

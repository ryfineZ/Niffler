use super::{RequestCandidateStatus, StoredRequestCandidate};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CapacityModelSummary {
    pub provider_id: String,
    pub key_id: String,
    pub model: String,
    pub count_24h: u64,
    pub last_occurred_at_ms: u64,
    pub last_success_at_ms: Option<u64>,
    pub recent: Vec<CapacityErrorEvent>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CapacityErrorEvent {
    pub candidate_id: String,
    pub request_id: String,
    pub occurred_at_ms: u64,
}

pub fn is_model_capacity_error(error_type: Option<&str>, message: Option<&str>) -> bool {
    [error_type, message].into_iter().flatten().any(|text| {
        let text = text.to_ascii_lowercase();
        text.contains("selected model is at capacity")
            || text.contains("server_is_overloaded")
            || text.contains("slow_down")
    })
}

pub fn candidate_upstream_model(row: &StoredRequestCandidate) -> String {
    row.extra_data
        .as_ref()
        .and_then(|v| v.get("upstream_model").or_else(|| v.get("mapped_model")))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

/// Aggregate distinct persisted attempts, including failures of requests that later succeeded.
pub fn summarize_capacity_candidates(
    rows: &[StoredRequestCandidate],
    provider_ids: &[String],
    since_ms: u64,
    until_ms: u64,
) -> Vec<CapacityModelSummary> {
    let mut summaries = BTreeMap::<(String, String, String), CapacityModelSummary>::new();
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        let (Some(provider), Some(key)) = (&row.provider_id, &row.key_id) else {
            continue;
        };
        let at = row.finished_at_unix_ms.unwrap_or(row.created_at_unix_ms);
        if !provider_ids.contains(provider)
            || at < since_ms
            || at > until_ms
            || !seen.insert(&row.id)
        {
            continue;
        }
        if row
            .extra_data
            .as_ref()
            .and_then(|v| v.get("capacity_error"))
            .and_then(serde_json::Value::as_bool)
            != Some(true)
            && !(row.status == RequestCandidateStatus::Failed
                && is_model_capacity_error(row.error_type.as_deref(), row.error_message.as_deref()))
        {
            continue;
        }
        let model = candidate_upstream_model(row);
        let summary = summaries
            .entry((provider.clone(), key.clone(), model.clone()))
            .or_insert_with(|| CapacityModelSummary {
                provider_id: provider.clone(),
                key_id: key.clone(),
                model,
                ..Default::default()
            });
        summary.count_24h += 1;
        summary.last_occurred_at_ms = summary.last_occurred_at_ms.max(at);
        summary.recent.push(CapacityErrorEvent {
            candidate_id: row.id.clone(),
            request_id: row.request_id.clone(),
            occurred_at_ms: at,
        });
    }
    for row in rows {
        if row.status != RequestCandidateStatus::Success {
            continue;
        }
        let (Some(provider), Some(key)) = (&row.provider_id, &row.key_id) else {
            continue;
        };
        let at = row.finished_at_unix_ms.unwrap_or(row.created_at_unix_ms);
        if at > until_ms {
            continue;
        }
        if let Some(summary) =
            summaries.get_mut(&(provider.clone(), key.clone(), candidate_upstream_model(row)))
        {
            if at > summary.last_occurred_at_ms {
                summary.last_success_at_ms = Some(summary.last_success_at_ms.unwrap_or(0).max(at));
            }
        }
    }
    for summary in summaries.values_mut() {
        summary.recent.sort_by(|a, b| {
            b.occurred_at_ms
                .cmp(&a.occurred_at_ms)
                .then(a.candidate_id.cmp(&b.candidate_id))
        });
        summary.recent.truncate(20);
    }
    summaries.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(
        id: &str,
        key: &str,
        model: &str,
        at: u64,
        status: RequestCandidateStatus,
        message: Option<&str>,
    ) -> StoredRequestCandidate {
        StoredRequestCandidate {
            id: id.into(),
            request_id: "request-shared".into(),
            user_id: None,
            api_key_id: None,
            username: None,
            api_key_name: None,
            candidate_index: 0,
            retry_index: 0,
            provider_id: Some("p".into()),
            endpoint_id: Some("e".into()),
            key_id: Some(key.into()),
            status,
            skip_reason: None,
            is_cached: false,
            status_code: Some(if status == RequestCandidateStatus::Success {
                200
            } else {
                400
            }),
            error_type: None,
            error_message: message.map(str::to_string),
            latency_ms: None,
            concurrent_requests: None,
            extra_data: Some(serde_json::json!({"upstream_model": model})),
            required_capabilities: None,
            created_at_unix_ms: at,
            started_at_unix_ms: Some(at),
            finished_at_unix_ms: Some(at),
        }
    }
    #[test]
    fn capacity_counts_distinct_attempts_and_respects_window_and_model_recovery() {
        let failure = row(
            "a",
            "key-a",
            "model-a",
            1000,
            RequestCandidateStatus::Failed,
            Some("Selected model is at capacity. Please try a different model."),
        );
        let mut rows = vec![
            failure.clone(),
            failure,
            row(
                "old",
                "key-a",
                "model-a",
                999,
                RequestCandidateStatus::Failed,
                Some("server_is_overloaded"),
            ),
            row(
                "rate",
                "key-a",
                "model-a",
                1200,
                RequestCandidateStatus::Failed,
                Some("rate_limit_exceeded"),
            ),
            row(
                "b",
                "key-a",
                "model-a",
                1400,
                RequestCandidateStatus::Failed,
                Some("server_is_overloaded"),
            ),
            row(
                "other-key",
                "key-b",
                "model-a",
                1500,
                RequestCandidateStatus::Success,
                None,
            ),
            row(
                "other-model",
                "key-a",
                "model-b",
                1600,
                RequestCandidateStatus::Success,
                None,
            ),
        ];
        let result = summarize_capacity_candidates(&rows, &["p".into()], 1000, 2000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].count_24h, 2);
        assert_eq!(result[0].last_occurred_at_ms, 1400);
        assert_eq!(result[0].last_success_at_ms, None);
        rows.push(row(
            "recovered",
            "key-a",
            "model-a",
            1700,
            RequestCandidateStatus::Success,
            None,
        ));
        assert_eq!(
            summarize_capacity_candidates(&rows, &["p".into()], 1000, 2000)[0].last_success_at_ms,
            Some(1700)
        );
        assert!(summarize_capacity_candidates(&rows, &["unrelated".into()], 1000, 2000).is_empty());
    }
    #[test]
    fn capacity_history_is_bounded_without_truncating_counts() {
        let rows = (1..=40)
            .map(|n| {
                row(
                    &n.to_string(),
                    "key",
                    "model",
                    n,
                    RequestCandidateStatus::Failed,
                    Some("Selected model is at capacity"),
                )
            })
            .collect::<Vec<_>>();
        let result = summarize_capacity_candidates(&rows, &["p".into()], 0, 40);
        assert_eq!(result[0].count_24h, 40);
        assert_eq!(result[0].recent.len(), 20);
        assert_eq!(result[0].recent[0].occurred_at_ms, 40);
    }
}

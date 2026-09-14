use super::{
    admin_pool_provider_id_from_path, admin_provider_pool_config, build_admin_pool_error_response,
    parse_admin_pool_key_sort, parse_admin_pool_page, parse_admin_pool_page_size,
    parse_admin_pool_plan_filter, parse_admin_pool_quick_selectors, parse_admin_pool_search,
    parse_admin_pool_status_filter, pool_payloads, pool_selection,
    read_admin_provider_pool_cooldown_key_ids, read_admin_provider_pool_runtime_state,
    AdminPoolKeySort, AdminPoolKeySortDirection, AdminPoolKeySortField,
    AdminProviderPoolRuntimeState, ADMIN_POOL_PROVIDER_CATALOG_READER_UNAVAILABLE_DETAIL,
};
use crate::ai_serving::{provider_key_pool_score_id, provider_key_pool_score_scope};
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::provider_pool_demand::provider_pool_live_in_flight_by_key;
use crate::GatewayError;
use aether_admin::provider::pool as admin_provider_pool_pure;
use aether_data_contracts::repository::pool_scores::{
    GetPoolMemberScoresByIdsQuery, PoolMemberIdentity, StoredPoolMemberScore,
};
use aether_data_contracts::repository::provider_catalog::{
    ProviderCatalogKeyListOrder, ProviderCatalogKeyListQuery, StoredProviderCatalogKey,
};
use aether_data_contracts::repository::usage::StoredProviderApiKeyWindowUsageSummary;
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tracing::warn;

const ADMIN_POOL_CODEX_WINDOW_USAGE_INIT_TTL: Duration = Duration::from_secs(30);
const ADMIN_POOL_CODEX_WINDOW_USAGE_INIT_CACHE_MAX_ENTRIES: usize = 4096;

static ADMIN_POOL_CODEX_WINDOW_USAGE_INIT_CACHE: OnceLock<Mutex<HashMap<String, Instant>>> =
    OnceLock::new();

fn admin_pool_codex_window_usage_init_cache() -> &'static Mutex<HashMap<String, Instant>> {
    ADMIN_POOL_CODEX_WINDOW_USAGE_INIT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn reserve_admin_pool_codex_window_usage_initializations(
    requests: &[aether_data_contracts::repository::usage::ProviderApiKeyWindowUsageRequest],
) -> Vec<String> {
    let now = Instant::now();
    let mut cache = admin_pool_codex_window_usage_init_cache()
        .lock()
        .expect("admin pool codex window usage init cache should lock");
    cache.retain(|_, expires_at| *expires_at > now);

    let mut key_ids = BTreeSet::new();
    for request in requests {
        let cache_key = format!(
            "{}:{}",
            request.provider_api_key_id,
            pool_payloads::admin_pool_codex_window_usage_identity(request)
        );
        if cache.contains_key(&cache_key) {
            continue;
        }
        if cache.len() >= ADMIN_POOL_CODEX_WINDOW_USAGE_INIT_CACHE_MAX_ENTRIES {
            break;
        }
        cache.insert(cache_key, now + ADMIN_POOL_CODEX_WINDOW_USAGE_INIT_TTL);
        key_ids.insert(request.provider_api_key_id.clone());
    }
    key_ids.into_iter().collect()
}

fn schedule_admin_pool_codex_window_usage_initialization(
    state: &AdminAppState<'_>,
    requests: Vec<aether_data_contracts::repository::usage::ProviderApiKeyWindowUsageRequest>,
) {
    let key_ids = reserve_admin_pool_codex_window_usage_initializations(&requests);
    if key_ids.is_empty() {
        return;
    }

    let app = state.cloned_app();
    tokio::spawn(async move {
        for key_id in key_ids {
            if let Err(error) = app
                .ensure_provider_api_key_codex_window_usage_stats(&key_id)
                .await
            {
                warn!(
                    event_name = "admin_pool_codex_window_usage_init_failed",
                    provider_api_key_id = %key_id,
                    error = ?error,
                    "failed to initialize codex window usage counters from admin pool list"
                );
            }
        }
    });
}

fn admin_pool_plan_label(code: &str) -> &'static str {
    match code {
        "free" => "Free",
        "plus" => "Plus",
        "team" => "Team",
        "pro" => "Pro",
        "enterprise" => "Enterprise",
        "unknown" => "未知",
        _ => "其他",
    }
}

fn admin_pool_status_label(code: &str) -> &'static str {
    match code {
        "available" => "可用",
        "invalid" => "已失效",
        "disabled" => "禁用",
        "quota_exhausted" => "额度耗尽",
        "temporary_unavailable" => "暂时不可用",
        "blocked" => "异常",
        _ => "其他",
    }
}

fn admin_pool_key_summary_payload(
    state: &AdminAppState<'_>,
    keys: &[StoredProviderCatalogKey],
    provider_type: &str,
    cooldown_key_ids: &BTreeSet<String>,
    now_unix_secs: u64,
) -> serde_json::Value {
    let mut by_plan: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    for key in keys {
        let plan_bucket = pool_selection::admin_pool_key_plan_bucket(state, key, provider_type);
        *by_plan.entry(plan_bucket).or_default() += 1;
        let status_bucket = pool_selection::admin_pool_key_status_bucket(
            key,
            provider_type,
            cooldown_key_ids,
            now_unix_secs,
        );
        *by_status.entry(status_bucket.to_string()).or_default() += 1;
    }
    let plan_order = ["free", "plus", "team", "pro", "enterprise", "unknown"];
    let status_order = [
        "available",
        "invalid",
        "disabled",
        "quota_exhausted",
        "temporary_unavailable",
        "blocked",
    ];
    let plans = plan_order
        .iter()
        .filter_map(|code| {
            by_plan.get(*code).map(|count| {
                json!({
                    "code": code,
                    "label": admin_pool_plan_label(code),
                    "count": count,
                })
            })
        })
        .collect::<Vec<_>>();
    let statuses = status_order
        .iter()
        .map(|code| {
            json!({
                "code": code,
                "label": admin_pool_status_label(code),
                "count": by_status.get(*code).copied().unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "total": keys.len(),
        "plans": plans,
        "statuses": statuses,
    })
}

async fn read_admin_pool_scores_by_key_id(
    state: &AdminAppState<'_>,
    provider_id: &str,
    key_ids: &[String],
) -> Result<BTreeMap<String, StoredPoolMemberScore>, GatewayError> {
    if key_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let score_scope = provider_key_pool_score_scope();
    let score_ids = key_ids
        .iter()
        .map(|key_id| {
            let identity =
                PoolMemberIdentity::provider_api_key(provider_id.to_string(), key_id.clone());
            provider_key_pool_score_id(&identity, &score_scope)
        })
        .collect::<Vec<_>>();
    let scores = state
        .app()
        .data
        .get_pool_member_scores_by_ids(&GetPoolMemberScoresByIdsQuery { ids: score_ids })
        .await
        .map_err(|err| GatewayError::Internal(format!("{err:?}")))?;
    Ok(scores
        .into_iter()
        .map(|score| (score.member_id.clone(), score))
        .collect::<BTreeMap<_, _>>())
}

fn admin_pool_compare_optional_unix_secs(
    left: Option<u64>,
    right: Option<u64>,
    direction: AdminPoolKeySortDirection,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => match direction {
            AdminPoolKeySortDirection::Asc => left.cmp(&right),
            AdminPoolKeySortDirection::Desc => right.cmp(&left),
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn admin_pool_compare_optional_score(
    left: Option<f64>,
    right: Option<f64>,
    direction: AdminPoolKeySortDirection,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            let ordering = left.partial_cmp(&right).unwrap_or(Ordering::Equal);
            match direction {
                AdminPoolKeySortDirection::Asc => ordering,
                AdminPoolKeySortDirection::Desc => ordering.reverse(),
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn admin_pool_score_for_key(
    scores_by_key_id: &BTreeMap<String, StoredPoolMemberScore>,
    key: &StoredProviderCatalogKey,
) -> Option<f64> {
    scores_by_key_id
        .get(&key.id)
        .map(|score| score.score)
        .filter(|score| score.is_finite())
}

fn admin_pool_sort_keys_for_request(keys: &mut [StoredProviderCatalogKey], sort: AdminPoolKeySort) {
    match sort.field {
        AdminPoolKeySortField::Default => pool_selection::admin_pool_sort_keys(keys),
        AdminPoolKeySortField::ImportedAt => {
            keys.sort_by(|left, right| {
                admin_pool_compare_optional_unix_secs(
                    left.created_at_unix_ms,
                    right.created_at_unix_ms,
                    sort.direction,
                )
                .then(left.name.cmp(&right.name))
                .then(left.id.cmp(&right.id))
            });
        }
        AdminPoolKeySortField::LastUsedAt => {
            keys.sort_by(|left, right| {
                admin_pool_compare_optional_unix_secs(
                    left.last_used_at_unix_secs,
                    right.last_used_at_unix_secs,
                    sort.direction,
                )
                .then(left.name.cmp(&right.name))
                .then(left.id.cmp(&right.id))
            });
        }
        AdminPoolKeySortField::Score | AdminPoolKeySortField::Capacity => {}
    }
}

fn admin_pool_sort_keys_by_score(
    keys: &mut [StoredProviderCatalogKey],
    scores_by_key_id: &BTreeMap<String, StoredPoolMemberScore>,
    direction: AdminPoolKeySortDirection,
) {
    keys.sort_by(|left, right| {
        admin_pool_compare_optional_score(
            admin_pool_score_for_key(scores_by_key_id, left),
            admin_pool_score_for_key(scores_by_key_id, right),
            direction,
        )
        .then(left.name.cmp(&right.name))
        .then(left.id.cmp(&right.id))
    });
}

fn admin_pool_repository_key_order(sort: AdminPoolKeySort) -> ProviderCatalogKeyListOrder {
    match (sort.field, sort.direction) {
        (AdminPoolKeySortField::Default, _) => ProviderCatalogKeyListOrder::Name,
        (AdminPoolKeySortField::ImportedAt, AdminPoolKeySortDirection::Asc) => {
            ProviderCatalogKeyListOrder::CreatedAtAsc
        }
        (AdminPoolKeySortField::ImportedAt, AdminPoolKeySortDirection::Desc) => {
            ProviderCatalogKeyListOrder::CreatedAtDesc
        }
        (AdminPoolKeySortField::LastUsedAt, AdminPoolKeySortDirection::Asc) => {
            ProviderCatalogKeyListOrder::LastUsedAtAsc
        }
        (AdminPoolKeySortField::LastUsedAt, AdminPoolKeySortDirection::Desc) => {
            ProviderCatalogKeyListOrder::LastUsedAtDesc
        }
        (AdminPoolKeySortField::Score | AdminPoolKeySortField::Capacity, _) => {
            ProviderCatalogKeyListOrder::Name
        }
    }
}

fn admin_pool_repository_key_is_active_filter(status: &str) -> Option<bool> {
    match status {
        "active" => Some(true),
        "inactive" | "disabled" => Some(false),
        _ => None,
    }
}

fn admin_pool_can_use_repository_page(
    search: Option<&str>,
    quick_selectors: &[String],
    plan_filter: &str,
    status: &str,
    sort: AdminPoolKeySort,
) -> bool {
    search.is_none()
        && quick_selectors.is_empty()
        && plan_filter == "all"
        && matches!(status, "all" | "active" | "inactive" | "disabled")
        && !matches!(
            sort.field,
            AdminPoolKeySortField::Score | AdminPoolKeySortField::Capacity
        )
}

pub(super) async fn build_admin_pool_list_keys_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_provider_catalog_data_reader() {
        return Ok(build_admin_pool_error_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
            ADMIN_POOL_PROVIDER_CATALOG_READER_UNAVAILABLE_DETAIL,
        ));
    }

    let Some(provider_id) = admin_pool_provider_id_from_path(request_context.path()) else {
        return Ok(build_admin_pool_error_response(
            http::StatusCode::BAD_REQUEST,
            "provider_id 无效",
        ));
    };
    let query = request_context.query_string();
    let page = match parse_admin_pool_page(query) {
        Ok(value) => value,
        Err(detail) => {
            return Ok(build_admin_pool_error_response(
                http::StatusCode::BAD_REQUEST,
                detail,
            ));
        }
    };
    let page_size = match parse_admin_pool_page_size(query) {
        Ok(value) => value,
        Err(detail) => {
            return Ok(build_admin_pool_error_response(
                http::StatusCode::BAD_REQUEST,
                detail,
            ));
        }
    };
    let search = parse_admin_pool_search(query).map(|value| value.to_ascii_lowercase());
    let quick_selectors = admin_provider_pool_pure::admin_pool_sanitize_quick_selectors(
        parse_admin_pool_quick_selectors(query),
    );
    let status = match parse_admin_pool_status_filter(query) {
        Ok(value) => value,
        Err(detail) => {
            return Ok(build_admin_pool_error_response(
                http::StatusCode::BAD_REQUEST,
                detail,
            ));
        }
    };
    let plan_filter = parse_admin_pool_plan_filter(query);
    let sort = match parse_admin_pool_key_sort(query) {
        Ok(value) => value,
        Err(detail) => {
            return Ok(build_admin_pool_error_response(
                http::StatusCode::BAD_REQUEST,
                detail,
            ));
        }
    };

    let providers = state
        .read_provider_catalog_providers_by_ids(std::slice::from_ref(&provider_id))
        .await?;
    if providers.is_empty() {
        return Ok(build_admin_pool_error_response(
            http::StatusCode::NOT_FOUND,
            "提供商不存在",
        ));
    }
    let provider_ids = providers.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    let now_ms = crate::clock::current_unix_ms();
    let now_unix_secs = now_ms / 1000;
    let (capacity, mut capacity_available) = match state
        .app()
        .data
        .summarize_capacity_errors(&provider_ids, now_ms.saturating_sub(86_400_000), now_ms)
        .await
    {
        Ok(capacity) => (capacity, true),
        Err(error) => {
            warn!(
                event_name = "pool_capacity_statistics_unavailable",
                ?error,
                "capacity statistics unavailable"
            );
            (Vec::new(), false)
        }
    };
    let mut capacity_by_key = BTreeMap::<String, Vec<serde_json::Value>>::new();
    for model in capacity {
        let ttl = match
            crate::handlers::admin::provider::pool::runtime::read_capacity_model_cooldown_seconds(
                state.runtime_state(),
                &model.provider_id,
                &model.key_id,
                &model.model,
            )
            .await
        {
            Ok(ttl) => ttl,
            Err(error) => {
                warn!(
                    event_name = "pool_capacity_cooldown_unavailable",
                    provider_id = %model.provider_id,
                    key_id = %model.key_id,
                    ?error,
                    "capacity cooldown unavailable; returning ordinary account list without capacity data"
                );
                capacity_available = false;
                capacity_by_key.clear();
                break;
            }
        };
        let mut payload =
            serde_json::to_value(&model).map_err(|err| GatewayError::Internal(err.to_string()))?;
        payload["cooldown_ttl_seconds"] = json!(ttl);
        payload["cooldown_expires_at_ms"] = json!(now_ms.saturating_add(ttl.saturating_mul(1000)));
        payload["state"] = json!(if ttl > 0 {
            "cooldown"
        } else if model.last_success_at_ms.is_some() {
            "recovered"
        } else {
            "pending"
        });
        capacity_by_key
            .entry(model.key_id)
            .or_default()
            .push(payload);
    }
    let capacity_count = |id: &str| -> u64 {
        capacity_by_key
            .get(id)
            .into_iter()
            .flatten()
            .map(|m| m["count_24h"].as_u64().unwrap_or(0))
            .sum()
    };
    let capacity_filter = crate::handlers::shared::query_param_value(query, "capacity");
    if !matches!(
        capacity_filter.as_deref(),
        None | Some("all" | "recent" | "unresolved")
    ) {
        return Ok(build_admin_pool_error_response(
            http::StatusCode::BAD_REQUEST,
            "容量筛选无效",
        ));
    }
    if !capacity_available
        && (matches!(sort.field, AdminPoolKeySortField::Capacity)
            || matches!(capacity_filter.as_deref(), Some("recent" | "unresolved")))
    {
        return Ok(build_admin_pool_error_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
            "容量统计暂不可用，请稍后重试",
        ));
    }
    // Load compact summaries for aggregation; only hydrate credentials for the requested page.
    let all_keys = if search.is_some() || !quick_selectors.is_empty() {
        state
            .list_provider_catalog_keys_by_provider_ids(&provider_ids)
            .await?
    } else {
        state
            .list_provider_catalog_key_summaries_by_provider_ids(&provider_ids)
            .await?
    };
    let mut keys = Vec::new();
    let mut by_plan = BTreeMap::<String, serde_json::Value>::new();
    let mut by_status = BTreeMap::<String, serde_json::Value>::new();
    let mut total_before_filters = 0;
    let mut capacity_accounts = 0;
    let mut capacity_total = 0u64;
    let mut all_scores = BTreeMap::new();
    for provider in &providers {
        let cooldown_ids =
            read_admin_provider_pool_cooldown_key_ids(state.runtime_state(), &provider.id)
                .await
                .into_iter()
                .collect::<BTreeSet<_>>();
        let scoped = all_keys
            .iter()
            .filter(|key| key.provider_id == provider.id)
            .filter(|key| {
                pool_selection::admin_pool_matches_search(
                    state,
                    key,
                    &provider.provider_type,
                    search.as_deref(),
                )
            })
            .filter(|key| {
                quick_selectors.iter().all(|q| {
                    pool_selection::admin_pool_matches_quick_selector(
                        state,
                        key,
                        &provider.provider_type,
                        q,
                    )
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let summary = admin_pool_key_summary_payload(
            state,
            &scoped,
            &provider.provider_type,
            &cooldown_ids,
            now_unix_secs,
        );
        total_before_filters += scoped.len();
        for (field, target) in [("plans", &mut by_plan), ("statuses", &mut by_status)] {
            for bucket in summary[field].as_array().into_iter().flatten() {
                let code = bucket["code"].as_str().unwrap_or_default().to_string();
                let entry = target.entry(code).or_insert_with(
                    || json!({"code": bucket["code"], "label": bucket["label"], "count": 0}),
                );
                entry["count"] = json!(
                    entry["count"].as_u64().unwrap_or(0) + bucket["count"].as_u64().unwrap_or(0)
                );
            }
        }
        for key in scoped {
            let count = capacity_count(&key.id);
            if count > 0 {
                capacity_accounts += 1;
                capacity_total += count;
            }
            let matches_capacity = match capacity_filter.as_deref() {
                Some("recent") => count > 0,
                Some("unresolved") => capacity_by_key
                    .get(&key.id)
                    .is_some_and(|models| models.iter().any(|m| m["state"] != "recovered")),
                _ => true,
            };
            if matches_capacity
                && pool_selection::admin_pool_plan_filter_matches(
                    &plan_filter,
                    &pool_selection::admin_pool_key_plan_bucket(
                        state,
                        &key,
                        &provider.provider_type,
                    ),
                )
                && pool_selection::admin_pool_status_filter_matches(
                    &status,
                    pool_selection::admin_pool_key_status_bucket(
                        &key,
                        &provider.provider_type,
                        &cooldown_ids,
                        now_unix_secs,
                    ),
                    &key,
                )
            {
                keys.push(key);
            }
        }
        if matches!(sort.field, AdminPoolKeySortField::Score) {
            let ids = keys
                .iter()
                .filter(|k| k.provider_id == provider.id)
                .map(|k| k.id.clone())
                .collect::<Vec<_>>();
            all_scores.extend(read_admin_pool_scores_by_key_id(state, &provider.id, &ids).await?);
        }
    }
    let use_repository_page = matches!(capacity_filter.as_deref(), None | Some("all"))
        && admin_pool_can_use_repository_page(
            search.as_deref(),
            &quick_selectors,
            &plan_filter,
            &status,
            sort,
        );
    let (total, ids, page_keys) = if use_repository_page {
        let key_page = state
            .list_provider_catalog_key_page(&ProviderCatalogKeyListQuery {
                provider_id: provider_id.clone(),
                search: None,
                is_active: admin_pool_repository_key_is_active_filter(&status),
                offset: page.saturating_sub(1).saturating_mul(page_size),
                limit: page_size,
                order: admin_pool_repository_key_order(sort),
            })
            .await?;
        let ids = key_page
            .items
            .iter()
            .map(|key| key.id.clone())
            .collect::<Vec<_>>();
        (key_page.total, ids, key_page.items)
    } else {
        if matches!(sort.field, AdminPoolKeySortField::Capacity) {
            keys.sort_by(|a, b| {
                let cmp = capacity_count(&a.id).cmp(&capacity_count(&b.id));
                (if sort.direction == AdminPoolKeySortDirection::Desc {
                    cmp.reverse()
                } else {
                    cmp
                })
                .then(a.id.cmp(&b.id))
            });
        } else if matches!(sort.field, AdminPoolKeySortField::Score) {
            admin_pool_sort_keys_by_score(&mut keys, &all_scores, sort.direction);
        } else {
            admin_pool_sort_keys_for_request(&mut keys, sort);
        }
        let total = keys.len();
        let ids = keys
            .into_iter()
            .skip(page.saturating_sub(1).saturating_mul(page_size))
            .take(page_size)
            .map(|k| k.id)
            .collect::<Vec<_>>();
        let page_keys = state.read_provider_catalog_keys_by_ids(&ids).await?;
        (total, ids, page_keys)
    };
    let mut payloads = BTreeMap::<String, serde_json::Value>::new();
    for provider in &providers {
        let keys = page_keys
            .iter()
            .filter(|key| key.provider_id == provider.id)
            .cloned()
            .collect::<Vec<_>>();
        if keys.is_empty() {
            continue;
        }
        for mut payload in hydrate_pool_keys(state, provider, keys, now_unix_secs).await? {
            let id = payload["key_id"].as_str().unwrap_or_default().to_string();
            payload["provider_id"] = json!(provider.id);
            payload["provider_name"] = json!(provider.name);
            if capacity_available {
                payload["capacity"] = json!({"count_24h": capacity_count(&id), "models": capacity_by_key.get(&id).cloned().unwrap_or_default()});
            }
            payloads.insert(id, payload);
        }
    }
    let items = ids
        .iter()
        .filter_map(|id| payloads.remove(id))
        .collect::<Vec<_>>();
    Ok(Json(json!({"total": total, "page": page, "page_size": page_size,
        "summary": {"total": total_before_filters, "plans": by_plan.into_values().collect::<Vec<_>>(),
            "statuses": by_status.into_values().collect::<Vec<_>>(),
            "capacity_available": capacity_available, "capacity_accounts": capacity_accounts, "capacity_count_24h": capacity_total}, "keys": items})).into_response())
}

async fn hydrate_pool_keys(
    state: &AdminAppState<'_>,
    provider: &aether_data_contracts::repository::provider_catalog::StoredProviderCatalogProvider,
    keys: Vec<StoredProviderCatalogKey>,
    now_unix_secs: u64,
) -> Result<Vec<serde_json::Value>, GatewayError> {
    let pool_config = admin_provider_pool_config(provider);
    let key_ids = keys.iter().map(|key| key.id.clone()).collect::<Vec<_>>();
    let pool_scores_by_key_id =
        read_admin_pool_scores_by_key_id(state, &provider.id, &key_ids).await?;
    let endpoints = state
        .list_provider_catalog_endpoints_by_provider_ids(std::slice::from_ref(&provider.id))
        .await?;
    let mut runtime = match pool_config.as_ref() {
        Some(pool_config) if !key_ids.is_empty() => {
            read_admin_provider_pool_runtime_state(
                state.runtime_state(),
                &provider.id,
                &key_ids,
                pool_config,
                None,
            )
            .await
        }
        _ => AdminProviderPoolRuntimeState::default(),
    };
    if pool_config.is_none() {
        runtime.in_flight_by_key =
            provider_pool_live_in_flight_by_key(state.runtime_state(), &provider.id, &key_ids)
                .await;
    }
    let window_usage_requests = keys
        .iter()
        .flat_map(|key| {
            pool_payloads::admin_pool_codex_window_usage_requests(key, &provider.provider_type)
        })
        .collect::<Vec<_>>();
    let window_usage_summaries = state
        .summarize_usage_by_provider_api_key_windows(&window_usage_requests)
        .await?;
    schedule_admin_pool_codex_window_usage_initialization(state, window_usage_requests);
    let mut window_usage_by_key =
        BTreeMap::<String, BTreeMap<String, StoredProviderApiKeyWindowUsageSummary>>::new();
    for summary in window_usage_summaries {
        let identity = pool_payloads::admin_pool_codex_window_usage_summary_identity(&summary);
        window_usage_by_key
            .entry(summary.provider_api_key_id.clone())
            .or_default()
            .insert(identity, summary);
    }
    let items = keys
        .into_iter()
        .map(|key| {
            pool_payloads::build_admin_pool_key_payload(
                state,
                &provider.provider_type,
                &endpoints,
                &key,
                &runtime,
                pool_config.clone(),
                pool_scores_by_key_id.get(&key.id),
                window_usage_by_key.get(&key.id),
                now_unix_secs,
            )
        })
        .collect::<Vec<_>>();

    Ok(items)
}

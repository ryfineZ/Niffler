use std::cmp::Ordering;
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

pub const POOL_ACCOUNT_BLOCKED_SKIP_REASON: &str = "pool_account_blocked";
pub const POOL_ACCOUNT_EXHAUSTED_SKIP_REASON: &str = "pool_account_exhausted";
pub const POOL_COOLDOWN_SKIP_REASON: &str = "pool_cooldown";
pub const POOL_COST_LIMIT_REACHED_SKIP_REASON: &str = "pool_cost_limit_reached";
pub const POOL_TEMPORARY_UNAVAILABLE_SKIP_REASON: &str = "pool_temporary_unavailable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSchedulingPreset {
    pub preset: String,
    pub enabled: bool,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSchedulingConfig {
    pub scheduling_presets: Vec<PoolSchedulingPreset>,
    pub lru_enabled: bool,
    pub skip_exhausted_accounts: bool,
    pub cost_limit_per_key_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoolRuntimeState {
    pub sticky_bound_key_id: Option<String>,
    pub cooldown_reason_by_key: BTreeMap<String, String>,
    pub cost_window_usage_by_key: BTreeMap<String, u64>,
    pub latency_avg_ms_by_key: BTreeMap<String, f64>,
    pub lru_score_by_key: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoolMemberSignals {
    pub plan_tier: Option<String>,
    pub quota_usage_ratio: Option<f64>,
    pub quota_reset_seconds: Option<f64>,
    pub account_blocked: bool,
    pub quota_exhausted: bool,
    pub temporary_unavailable: bool,
    pub health_score: Option<f64>,
    pub latency_avg_ms: Option<f64>,
    pub catalog_lru_score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolCandidateFacts {
    pub provider_id: String,
    pub endpoint_id: String,
    pub model_id: String,
    pub selected_provider_model_name: String,
    pub provider_api_format: String,
    pub key_id: String,
    pub key_internal_priority: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolCandidateOrchestration {
    pub candidate_group_id: Option<String>,
    pub pool_key_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolCandidateInput<Candidate> {
    pub candidate: Candidate,
    pub facts: PoolCandidateFacts,
    pub pool_config: Option<PoolSchedulingConfig>,
    pub key_context: PoolMemberSignals,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolScheduledCandidate<Candidate> {
    pub candidate: Candidate,
    pub orchestration: PoolCandidateOrchestration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolSkippedCandidate<Candidate> {
    pub candidate: Candidate,
    pub skip_reason: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolSchedulerOutcome<Candidate> {
    pub candidates: Vec<PoolScheduledCandidate<Candidate>>,
    pub skipped_candidates: Vec<PoolSkippedCandidate<Candidate>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PoolGroupKey {
    provider_id: String,
    endpoint_id: String,
    model_id: String,
    selected_provider_model_name: String,
    provider_api_format: String,
    singleton_key_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedPoolPreset {
    preset: String,
    mode: Option<String>,
}

pub fn run_pool_scheduler<Candidate>(
    candidates: Vec<PoolCandidateInput<Candidate>>,
    runtime_by_provider: &BTreeMap<String, PoolRuntimeState>,
    load_balance_seed_nonce: &str,
) -> PoolSchedulerOutcome<Candidate> {
    let mut group_order = Vec::new();
    let mut groups = BTreeMap::<PoolGroupKey, Vec<PoolCandidateInput<Candidate>>>::new();

    for candidate in candidates {
        let pool_enabled = candidate.pool_config.is_some();
        let group_key = pool_group_key(&candidate, pool_enabled);
        match groups.entry(group_key) {
            Entry::Vacant(entry) => {
                group_order.push(entry.key().clone());
                entry.insert(vec![candidate]);
            }
            Entry::Occupied(mut entry) => {
                entry.get_mut().push(candidate);
            }
        }
    }

    let mut reordered = Vec::new();
    let mut skipped = Vec::new();
    let default_runtime = PoolRuntimeState::default();

    for group_key in group_order {
        let Some(group) = groups.remove(&group_key) else {
            continue;
        };
        let candidate_group_id = pool_candidate_group_id(&group_key);
        let Some(pool_config) = group
            .first()
            .expect("group should exist")
            .pool_config
            .clone()
        else {
            reordered.extend(annotate_pool_candidates(
                group,
                candidate_group_id.as_str(),
                false,
            ));
            continue;
        };
        let runtime = runtime_by_provider
            .get(&group_key.provider_id)
            .unwrap_or(&default_runtime);
        let outcome = schedule_pool_group(
            group,
            &pool_config,
            runtime,
            candidate_group_id.as_str(),
            load_balance_seed_nonce,
        );
        reordered.extend(outcome.candidates);
        skipped.extend(outcome.skipped_candidates);
    }

    PoolSchedulerOutcome {
        candidates: reordered,
        skipped_candidates: skipped,
    }
}

fn pool_group_key<Candidate>(
    candidate: &PoolCandidateInput<Candidate>,
    pool_enabled: bool,
) -> PoolGroupKey {
    PoolGroupKey {
        provider_id: candidate.facts.provider_id.clone(),
        endpoint_id: candidate.facts.endpoint_id.clone(),
        model_id: candidate.facts.model_id.clone(),
        selected_provider_model_name: candidate.facts.selected_provider_model_name.clone(),
        provider_api_format: candidate.facts.provider_api_format.clone(),
        singleton_key_id: (!pool_enabled).then(|| candidate.facts.key_id.clone()),
    }
}

fn pool_candidate_group_id(group_key: &PoolGroupKey) -> String {
    format!(
        "provider={}|endpoint={}|model={}|selected_model={}|api_format={}|singleton_key={}",
        group_key.provider_id,
        group_key.endpoint_id,
        group_key.model_id,
        group_key.selected_provider_model_name,
        group_key.provider_api_format,
        group_key.singleton_key_id.as_deref().unwrap_or("*"),
    )
}

fn schedule_pool_group<Candidate>(
    group: Vec<PoolCandidateInput<Candidate>>,
    pool_config: &PoolSchedulingConfig,
    runtime: &PoolRuntimeState,
    candidate_group_id: &str,
    load_balance_seed_nonce: &str,
) -> PoolSchedulerOutcome<Candidate> {
    let active_presets = normalize_enabled_pool_preset_entries(&pool_config.scheduling_presets);
    let distribution_preset = active_presets
        .iter()
        .find(|preset| pool_preset_mutex_group(&preset.preset).is_some());
    let strategy_presets = active_presets
        .iter()
        .filter(|preset| pool_preset_mutex_group(&preset.preset).is_none())
        .collect::<Vec<_>>();
    let lru_distribution_enabled = pool_config.lru_enabled && distribution_preset.is_none();

    let mut available = Vec::new();
    let mut skipped = Vec::new();

    for (original_index, mut item) in group.into_iter().enumerate() {
        let key_id = item.facts.key_id.clone();
        item.key_context.latency_avg_ms = runtime
            .latency_avg_ms_by_key
            .get(&key_id)
            .copied()
            .or(item.key_context.latency_avg_ms);

        if item.key_context.account_blocked {
            skipped.push(PoolSkippedCandidate {
                candidate: item.candidate,
                skip_reason: POOL_ACCOUNT_BLOCKED_SKIP_REASON,
            });
            continue;
        }

        if pool_config.skip_exhausted_accounts && item.key_context.quota_exhausted {
            skipped.push(PoolSkippedCandidate {
                candidate: item.candidate,
                skip_reason: POOL_ACCOUNT_EXHAUSTED_SKIP_REASON,
            });
            continue;
        }

        if runtime.cooldown_reason_by_key.contains_key(&key_id) {
            skipped.push(PoolSkippedCandidate {
                candidate: item.candidate,
                skip_reason: POOL_COOLDOWN_SKIP_REASON,
            });
            continue;
        }

        if item.key_context.temporary_unavailable {
            skipped.push(PoolSkippedCandidate {
                candidate: item.candidate,
                skip_reason: POOL_TEMPORARY_UNAVAILABLE_SKIP_REASON,
            });
            continue;
        }

        if pool_config
            .cost_limit_per_key_tokens
            .is_some_and(|limit| runtime_cost_usage(runtime, key_id.as_str()) >= limit)
        {
            skipped.push(PoolSkippedCandidate {
                candidate: item.candidate,
                skip_reason: POOL_COST_LIMIT_REACHED_SKIP_REASON,
            });
            continue;
        }

        let lru_score =
            runtime_lru_score(runtime, key_id.as_str()).or(item.key_context.catalog_lru_score);

        available.push(PoolGroupCandidateOrdering {
            item,
            original_index,
            lru_score,
            cost_usage: runtime_cost_usage(runtime, key_id.as_str()),
        });
    }

    if available.is_empty() {
        return PoolSchedulerOutcome {
            candidates: Vec::new(),
            skipped_candidates: skipped,
        };
    }

    let sticky_candidate = if pool_sticky_enabled(distribution_preset) {
        runtime
            .sticky_bound_key_id
            .as_ref()
            .and_then(|sticky_key_id| {
                available
                    .iter()
                    .position(|item| item.item.facts.key_id == *sticky_key_id)
            })
            .map(|index| available.remove(index))
    } else {
        None
    };

    if !strategy_presets.is_empty() || distribution_preset.is_some() || lru_distribution_enabled {
        let sort_vectors = build_pool_sort_vectors(
            &available,
            &strategy_presets,
            distribution_preset,
            lru_distribution_enabled,
            group_sort_seed(
                available.first().map(|item| &item.item.facts),
                load_balance_seed_nonce,
            )
            .as_str(),
            pool_config.cost_limit_per_key_tokens,
        );
        available.sort_by(|left, right| {
            sort_vectors
                .get(&left.item.facts.key_id)
                .cmp(&sort_vectors.get(&right.item.facts.key_id))
                .then(left.original_index.cmp(&right.original_index))
        });
    }

    let mut ordered = Vec::new();
    if let Some(sticky_candidate) = sticky_candidate {
        ordered.push(sticky_candidate.item);
    }
    ordered.extend(available.into_iter().map(|item| item.item));

    PoolSchedulerOutcome {
        candidates: annotate_pool_candidates(ordered, candidate_group_id, true),
        skipped_candidates: skipped,
    }
}

fn annotate_pool_candidates<Candidate>(
    candidates: Vec<PoolCandidateInput<Candidate>>,
    candidate_group_id: &str,
    pool_enabled: bool,
) -> Vec<PoolScheduledCandidate<Candidate>> {
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, item)| PoolScheduledCandidate {
            candidate: item.candidate,
            orchestration: PoolCandidateOrchestration {
                candidate_group_id: Some(candidate_group_id.to_string()),
                pool_key_index: pool_enabled.then_some(index as u32),
            },
        })
        .collect()
}

#[derive(Debug)]
struct PoolGroupCandidateOrdering<Candidate> {
    item: PoolCandidateInput<Candidate>,
    original_index: usize,
    lru_score: Option<f64>,
    cost_usage: u64,
}

fn build_pool_sort_vectors<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    strategy_presets: &[&NormalizedPoolPreset],
    distribution_preset: Option<&NormalizedPoolPreset>,
    lru_distribution_enabled: bool,
    load_balance_seed: &str,
    cost_limit_per_key_tokens: Option<u64>,
) -> BTreeMap<String, Vec<usize>> {
    let mut vectors = BTreeMap::<String, Vec<usize>>::new();
    let lru_ranks = lru_rank_indices(items, false);
    let cache_affinity_ranks = lru_rank_indices(items, true);

    for preset in strategy_presets {
        let ranks = match preset.preset.as_str() {
            "plus_first" => plan_ranks(items, &lru_ranks, Some("plus_only")),
            "pro_first" => plan_ranks(items, &lru_ranks, Some("pro_only")),
            "free_first" => plan_ranks(items, &lru_ranks, Some("free_only")),
            "team_first" => plan_ranks(items, &lru_ranks, Some("team_only")),
            "health_first" => health_first_ranks(items, &lru_ranks),
            "latency_first" => latency_first_ranks(items, &lru_ranks),
            "cost_first" => cost_first_ranks(items, &lru_ranks, cost_limit_per_key_tokens),
            "quota_balanced" => quota_balanced_ranks(items, &lru_ranks, cost_limit_per_key_tokens),
            "recent_refresh" => recent_refresh_ranks(items, &lru_ranks),
            _ => continue,
        };
        for item in items {
            let key_id = item.item.facts.key_id.clone();
            vectors
                .entry(key_id.clone())
                .or_default()
                .push(*ranks.get(&key_id).unwrap_or(&0));
        }
    }

    let distribution_ranks = if lru_distribution_enabled {
        Some(lru_ranks)
    } else {
        distribution_preset.and_then(|preset| match preset.preset.as_str() {
            "cache_affinity" => Some(cache_affinity_ranks),
            "priority_first" => Some(priority_first_ranks(items, &lru_ranks)),
            "single_account" => Some(single_account_ranks(items)),
            "load_balance" => Some(load_balance_ranks(items, load_balance_seed)),
            _ => None,
        })
    };
    if let Some(ranks) = distribution_ranks {
        for item in items {
            let key_id = item.item.facts.key_id.clone();
            vectors
                .entry(key_id.clone())
                .or_default()
                .push(*ranks.get(&key_id).unwrap_or(&0));
        }
    }

    vectors
}

fn pool_sticky_enabled(distribution_preset: Option<&NormalizedPoolPreset>) -> bool {
    distribution_preset.is_some_and(|preset| preset.preset == "cache_affinity")
}

fn lru_rank_indices<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    descending: bool,
) -> BTreeMap<String, usize> {
    let scores = collect_metric_scores(items, |item| item.lru_score);
    rank_indices_from_score_map(items, &scores, descending)
}

fn priority_first_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    _lru_ranks: &BTreeMap<String, usize>,
) -> BTreeMap<String, usize> {
    let scores = collect_metric_scores(items, |item| {
        Some(f64::from(item.item.facts.key_internal_priority))
    });
    if !score_map_has_variation(&scores) {
        return neutral_rank_indices(items);
    }
    rank_indices_from_score_map(items, &scores, false)
}

fn single_account_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
) -> BTreeMap<String, usize> {
    let lru_desc_ranks = lru_rank_indices(items, true);
    let mut decorated = items
        .iter()
        .map(|item| {
            let key_id = item.item.facts.key_id.clone();
            (
                item.item.facts.key_internal_priority,
                *lru_desc_ranks.get(&key_id).unwrap_or(&0),
                item.original_index,
                key_id,
            )
        })
        .collect::<Vec<_>>();
    decorated.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    decorated
        .into_iter()
        .enumerate()
        .map(|(rank, (_, _, _, key_id))| (key_id, rank))
        .collect()
}

fn plan_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    _lru_ranks: &BTreeMap<String, usize>,
    mode: Option<&str>,
) -> BTreeMap<String, usize> {
    let scores = items
        .iter()
        .map(|item| {
            (
                item.item.facts.key_id.clone(),
                Some(plan_priority_score(
                    item.item.key_context.plan_tier.as_deref(),
                    mode,
                )),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if !score_map_has_variation(&scores) {
        return neutral_rank_indices(items);
    }
    rank_indices_from_score_map(items, &scores, false)
}

fn health_first_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    _lru_ranks: &BTreeMap<String, usize>,
) -> BTreeMap<String, usize> {
    let scores = collect_metric_scores(items, |item| {
        item.item
            .key_context
            .health_score
            .map(|score| 1.0 - score.clamp(0.0, 1.0))
    });
    if !score_map_has_signal(&scores) {
        return neutral_rank_indices(items);
    }
    rank_indices_from_score_map(items, &scores, false)
}

fn latency_first_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    _lru_ranks: &BTreeMap<String, usize>,
) -> BTreeMap<String, usize> {
    let scores = collect_metric_scores(items, |item| item.item.key_context.latency_avg_ms);
    if !score_map_has_signal(&scores) {
        return neutral_rank_indices(items);
    }
    rank_indices_from_score_map(items, &scores, false)
}

fn cost_first_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    _lru_ranks: &BTreeMap<String, usize>,
    cost_limit_per_key_tokens: Option<u64>,
) -> BTreeMap<String, usize> {
    let scores = collect_metric_scores(items, |item| {
        cost_penalty(item, cost_limit_per_key_tokens).or(item.item.key_context.quota_usage_ratio)
    });
    if !score_map_has_signal(&scores) {
        return neutral_rank_indices(items);
    }
    rank_indices_from_score_map(items, &scores, false)
}

fn quota_balanced_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    _lru_ranks: &BTreeMap<String, usize>,
    cost_limit_per_key_tokens: Option<u64>,
) -> BTreeMap<String, usize> {
    let scores = collect_metric_scores(items, |item| {
        item.item
            .key_context
            .quota_usage_ratio
            .or_else(|| cost_penalty(item, cost_limit_per_key_tokens))
    });
    if !score_map_has_signal(&scores) {
        return neutral_rank_indices(items);
    }
    rank_indices_from_score_map(items, &scores, false)
}

fn recent_refresh_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    _lru_ranks: &BTreeMap<String, usize>,
) -> BTreeMap<String, usize> {
    let scores = collect_metric_scores(items, |item| item.item.key_context.quota_reset_seconds);
    if !score_map_has_signal(&scores) {
        return neutral_rank_indices(items);
    }
    rank_indices_from_score_map(items, &scores, false)
}

fn load_balance_ranks<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    load_balance_seed: &str,
) -> BTreeMap<String, usize> {
    let scores = items
        .iter()
        .map(|item| {
            let key_id = item.item.facts.key_id.clone();
            (
                key_id.clone(),
                Some(stable_hash_score(
                    format!("{load_balance_seed}:{key_id}").as_str(),
                )),
            )
        })
        .collect::<BTreeMap<_, _>>();
    rank_indices_from_score_map(items, &scores, false)
}

fn group_sort_seed(
    candidate: Option<&PoolCandidateFacts>,
    load_balance_seed_nonce: &str,
) -> String {
    match candidate {
        Some(candidate) => format!(
            "{}:{}:{}:{}:{load_balance_seed_nonce}",
            candidate.provider_id,
            candidate.endpoint_id,
            candidate.model_id,
            candidate.selected_provider_model_name,
        ),
        None => load_balance_seed_nonce.to_string(),
    }
}

fn stable_hash_score(seed: &str) -> f64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    let value = hasher.finish();
    value as f64 / u64::MAX as f64
}

fn collect_metric_scores<Candidate, F>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    mut score_for: F,
) -> BTreeMap<String, Option<f64>>
where
    F: FnMut(&PoolGroupCandidateOrdering<Candidate>) -> Option<f64>,
{
    items
        .iter()
        .map(|item| (item.item.facts.key_id.clone(), score_for(item)))
        .collect()
}

fn score_map_has_signal(scores: &BTreeMap<String, Option<f64>>) -> bool {
    scores.values().flatten().any(|value| value.is_finite())
}

fn score_map_has_variation(scores: &BTreeMap<String, Option<f64>>) -> bool {
    let values = scores
        .values()
        .flatten()
        .filter(|value| value.is_finite())
        .map(|value| value.to_bits())
        .collect::<BTreeSet<_>>();
    values.len() > 1
}

fn rank_indices_from_score_map<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
    scores: &BTreeMap<String, Option<f64>>,
    descending: bool,
) -> BTreeMap<String, usize> {
    if !score_map_has_signal(scores) {
        return items
            .iter()
            .map(|item| (item.item.facts.key_id.clone(), 0))
            .collect();
    }

    let mut decorated = items
        .iter()
        .map(|item| {
            let key_id = item.item.facts.key_id.clone();
            let score = scores
                .get(&key_id)
                .copied()
                .flatten()
                .filter(|value| value.is_finite());
            let sortable = score.map(|value| if descending { -value } else { value });
            (
                score.is_none(),
                sortable.unwrap_or(f64::INFINITY),
                item.original_index,
                key_id,
            )
        })
        .collect::<Vec<_>>();
    decorated.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal))
            .then(left.2.cmp(&right.2))
    });

    let mut ranks = BTreeMap::new();
    let mut current_rank = 0usize;
    let mut previous = None::<(bool, f64)>;
    for (index, (missing, sortable, _, key_id)) in decorated.into_iter().enumerate() {
        let current = (missing, sortable);
        if previous.is_some_and(|previous| {
            previous.0 != current.0
                || previous
                    .1
                    .partial_cmp(&current.1)
                    .is_some_and(|ordering| ordering != Ordering::Equal)
        }) {
            current_rank = index;
        }
        previous = Some(current);
        ranks.insert(key_id, current_rank);
    }
    ranks
}

fn neutral_rank_indices<Candidate>(
    items: &[PoolGroupCandidateOrdering<Candidate>],
) -> BTreeMap<String, usize> {
    items
        .iter()
        .map(|item| (item.item.facts.key_id.clone(), 0))
        .collect()
}

fn cost_penalty<Candidate>(
    item: &PoolGroupCandidateOrdering<Candidate>,
    cost_limit_per_key_tokens: Option<u64>,
) -> Option<f64> {
    if item.cost_usage == 0 {
        return None;
    }

    if let Some(limit) = cost_limit_per_key_tokens.filter(|limit| *limit > 0) {
        return Some((item.cost_usage as f64 / limit as f64).clamp(0.0, 1.0));
    }

    let used = item.cost_usage as f64;
    Some((used / (used + 10_000.0)).clamp(0.0, 1.0))
}

fn plan_priority_score(plan_type: Option<&str>, mode: Option<&str>) -> f64 {
    match mode.unwrap_or("both").trim().to_ascii_lowercase().as_str() {
        "free_only" => match plan_type {
            Some("free") => 0.0,
            Some("team") => 0.5,
            Some("enterprise" | "business") => 0.2,
            Some("plus" | "pro") => 0.6,
            Some(_) => 0.7,
            None => 0.8,
        },
        "team_only" => match plan_type {
            Some("team") => 0.0,
            Some("free") => 0.5,
            Some("enterprise" | "business") => 0.2,
            Some("plus" | "pro") => 0.6,
            Some(_) => 0.7,
            None => 0.8,
        },
        "plus_only" => match plan_type {
            Some("plus" | "pro") => 0.0,
            Some("enterprise" | "business") => 0.3,
            Some("free" | "team") => 0.7,
            Some(_) => 0.7,
            None => 0.8,
        },
        "pro_only" => match plan_type {
            Some("pro") => 0.0,
            Some("plus") => 0.3,
            Some("enterprise" | "business") => 0.4,
            Some("free" | "team") => 0.7,
            Some(_) => 0.7,
            None => 0.8,
        },
        _ => match plan_type {
            Some("free" | "team") => 0.0,
            Some("enterprise" | "business") => 0.2,
            Some("plus" | "pro") => 0.6,
            Some(_) => 0.7,
            None => 0.8,
        },
    }
}

pub fn normalize_enabled_pool_presets(scheduling_presets: &[PoolSchedulingPreset]) -> Vec<String> {
    normalize_enabled_pool_preset_entries(scheduling_presets)
        .into_iter()
        .map(|preset| preset.preset)
        .collect()
}

fn normalize_enabled_pool_preset_entries(
    scheduling_presets: &[PoolSchedulingPreset],
) -> Vec<NormalizedPoolPreset> {
    let mut entries = Vec::<(usize, String, bool, Option<String>)>::new();
    let mut seen = BTreeSet::new();

    for (index, item) in scheduling_presets.iter().enumerate() {
        let preset = item.preset.trim().to_ascii_lowercase();
        if preset.is_empty() || !seen.insert(preset.clone()) {
            continue;
        }
        entries.push((index, preset, item.enabled, item.mode.clone()));
    }

    let mut distribution_mode = None::<(usize, String, Option<String>)>;
    let mut strategy_presets = Vec::<(usize, String, Option<String>)>::new();

    for (index, preset, enabled, mode) in entries {
        if !enabled {
            continue;
        }

        let Some(mutex_group) = pool_preset_mutex_group(&preset) else {
            strategy_presets.push((index, preset, mode));
            continue;
        };

        if mutex_group == "distribution_mode"
            && pool_distribution_mode_should_replace(distribution_mode.as_ref(), index, &preset)
        {
            distribution_mode = Some((index, preset, mode));
        }
    }

    let mut normalized = Vec::new();

    if let Some((_, preset, mode)) = distribution_mode.filter(|(_, preset, _)| preset != "lru") {
        normalized.push(NormalizedPoolPreset { preset, mode });
    }

    strategy_presets.sort_by_key(|left| left.0);
    normalized.extend(
        strategy_presets
            .into_iter()
            .map(|(_, preset, mode)| NormalizedPoolPreset { preset, mode }),
    );

    normalized
}

fn pool_preset_mutex_group(preset: &str) -> Option<&'static str> {
    match preset {
        "lru" | "cache_affinity" | "load_balance" | "single_account" | "priority_first" => {
            Some("distribution_mode")
        }
        _ => None,
    }
}

fn pool_distribution_mode_should_replace(
    current: Option<&(usize, String, Option<String>)>,
    index: usize,
    preset: &str,
) -> bool {
    if preset == "priority_first" {
        return true;
    }
    current
        .as_ref()
        .is_none_or(|current| current.1 != "priority_first" && index < current.0)
}

fn runtime_lru_score(runtime: &PoolRuntimeState, key_id: &str) -> Option<f64> {
    runtime.lru_score_by_key.get(key_id).copied()
}

fn runtime_cost_usage(runtime: &PoolRuntimeState, key_id: &str) -> u64 {
    runtime
        .cost_window_usage_by_key
        .get(key_id)
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_scheduler_groups_interleaved_candidates_and_reorders_internal_keys() {
        let pool_first = sample_candidate("provider-pool", "endpoint-1", "key-pool-a", 10, true);
        let other = sample_candidate("provider-other", "endpoint-2", "key-other", 10, false);
        let pool_second = sample_candidate("provider-pool", "endpoint-1", "key-pool-b", 10, true);

        let runtime_by_provider = BTreeMap::from([(
            "provider-pool".to_string(),
            PoolRuntimeState {
                lru_score_by_key: BTreeMap::from([
                    ("key-pool-a".to_string(), 20.0),
                    ("key-pool-b".to_string(), 10.0),
                ]),
                ..PoolRuntimeState::default()
            },
        )]);

        let outcome = run_pool_scheduler(
            vec![pool_first, other, pool_second],
            &runtime_by_provider,
            "seed",
        );

        assert!(outcome.skipped_candidates.is_empty());
        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec!["key-pool-b", "key-pool-a", "key-other"]
        );
    }

    #[test]
    fn pool_scheduler_skips_cooldown_and_cost_exhausted_keys() {
        let key_ready = sample_candidate("provider-pool", "endpoint-1", "key-ready", 10, true)
            .with_cost_limit(100);
        let key_cooldown =
            sample_candidate("provider-pool", "endpoint-1", "key-cooldown", 10, true)
                .with_cost_limit(100);
        let key_cost = sample_candidate("provider-pool", "endpoint-1", "key-cost", 10, true)
            .with_cost_limit(100);

        let runtime_by_provider = BTreeMap::from([(
            "provider-pool".to_string(),
            PoolRuntimeState {
                cooldown_reason_by_key: BTreeMap::from([(
                    "key-cooldown".to_string(),
                    "429".to_string(),
                )]),
                cost_window_usage_by_key: BTreeMap::from([("key-cost".to_string(), 100)]),
                ..PoolRuntimeState::default()
            },
        )]);

        let outcome = run_pool_scheduler(
            vec![key_ready, key_cooldown, key_cost],
            &runtime_by_provider,
            "seed",
        );

        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec!["key-ready"]
        );
        assert_eq!(
            outcome
                .skipped_candidates
                .iter()
                .map(|item| (item.candidate.as_str(), item.skip_reason))
                .collect::<Vec<_>>(),
            vec![
                ("key-cooldown", "pool_cooldown"),
                ("key-cost", "pool_cost_limit_reached"),
            ]
        );
    }

    #[test]
    fn pool_scheduler_promotes_sticky_hit_before_other_sorted_keys() {
        let key_a = sample_candidate("provider-pool", "endpoint-1", "key-a", 10, true)
            .with_presets(vec![PoolSchedulingPreset {
                preset: "cache_affinity".to_string(),
                enabled: true,
                mode: None,
            }]);
        let key_b = sample_candidate("provider-pool", "endpoint-1", "key-b", 10, true)
            .with_presets(vec![PoolSchedulingPreset {
                preset: "cache_affinity".to_string(),
                enabled: true,
                mode: None,
            }]);

        let runtime_by_provider = BTreeMap::from([(
            "provider-pool".to_string(),
            PoolRuntimeState {
                sticky_bound_key_id: Some("key-a".to_string()),
                lru_score_by_key: BTreeMap::from([
                    ("key-a".to_string(), 50.0),
                    ("key-b".to_string(), 10.0),
                ]),
                ..PoolRuntimeState::default()
            },
        )]);

        let outcome = run_pool_scheduler(vec![key_a, key_b], &runtime_by_provider, "seed");

        assert!(outcome.skipped_candidates.is_empty());
        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec!["key-a", "key-b"]
        );
    }

    #[test]
    fn load_balance_distribution_ignores_sticky_hit() {
        let key_a = sample_candidate("provider-pool", "endpoint-1", "key-a", 10, true)
            .with_presets(vec![PoolSchedulingPreset {
                preset: "load_balance".to_string(),
                enabled: true,
                mode: None,
            }]);
        let key_b = sample_candidate("provider-pool", "endpoint-1", "key-b", 10, true)
            .with_presets(vec![PoolSchedulingPreset {
                preset: "load_balance".to_string(),
                enabled: true,
                mode: None,
            }]);
        let nonce = (0..1000)
            .map(|index| format!("seed-{index}"))
            .find(|nonce| {
                let group_seed = format!("provider-pool:endpoint-1:model-1:gpt-5:{nonce}");
                stable_hash_score(format!("{group_seed}:key-b").as_str())
                    < stable_hash_score(format!("{group_seed}:key-a").as_str())
            })
            .expect("test seed should exist");
        let runtime_by_provider = BTreeMap::from([(
            "provider-pool".to_string(),
            PoolRuntimeState {
                sticky_bound_key_id: Some("key-a".to_string()),
                ..PoolRuntimeState::default()
            },
        )]);

        let outcome = run_pool_scheduler(vec![key_a, key_b], &runtime_by_provider, &nonce);

        assert!(outcome.skipped_candidates.is_empty());
        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec!["key-b", "key-a"]
        );
    }

    #[test]
    fn pool_scheduler_uses_plan_preset_with_catalog_context() {
        let key_free = sample_candidate("provider-pool", "endpoint-1", "key-free", 10, true)
            .with_presets(vec![PoolSchedulingPreset {
                preset: "plus_first".to_string(),
                enabled: true,
                mode: None,
            }])
            .with_plan("free");
        let key_plus = sample_candidate("provider-pool", "endpoint-1", "key-plus", 10, true)
            .with_presets(vec![PoolSchedulingPreset {
                preset: "plus_first".to_string(),
                enabled: true,
                mode: None,
            }])
            .with_plan("plus");

        let outcome = run_pool_scheduler(vec![key_free, key_plus], &BTreeMap::new(), "seed");

        assert!(outcome.skipped_candidates.is_empty());
        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec!["key-plus", "key-free"]
        );
    }

    #[test]
    fn pool_scheduler_priority_first_overrides_cache_affinity() {
        let key_cache_hit =
            sample_candidate("provider-pool", "endpoint-1", "key-cache-hit", 50, true)
                .with_presets(vec![
                    PoolSchedulingPreset {
                        preset: "cache_affinity".to_string(),
                        enabled: true,
                        mode: None,
                    },
                    PoolSchedulingPreset {
                        preset: "priority_first".to_string(),
                        enabled: true,
                        mode: None,
                    },
                ]);
        let key_high_priority =
            sample_candidate("provider-pool", "endpoint-1", "key-high-priority", 10, true)
                .with_presets(vec![
                    PoolSchedulingPreset {
                        preset: "cache_affinity".to_string(),
                        enabled: true,
                        mode: None,
                    },
                    PoolSchedulingPreset {
                        preset: "priority_first".to_string(),
                        enabled: true,
                        mode: None,
                    },
                ]);

        let runtime_by_provider = BTreeMap::from([(
            "provider-pool".to_string(),
            PoolRuntimeState {
                sticky_bound_key_id: Some("key-cache-hit".to_string()),
                lru_score_by_key: BTreeMap::from([
                    ("key-cache-hit".to_string(), 200.0),
                    ("key-high-priority".to_string(), 10.0),
                ]),
                ..PoolRuntimeState::default()
            },
        )]);

        let outcome = run_pool_scheduler(
            vec![key_cache_hit, key_high_priority],
            &runtime_by_provider,
            "seed",
        );

        assert!(outcome.skipped_candidates.is_empty());
        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec!["key-high-priority", "key-cache-hit"]
        );
    }

    #[test]
    fn priority_first_overrides_load_balance_distribution() {
        let key_random_first =
            sample_candidate("provider-pool", "endpoint-1", "key-random-first", 50, true)
                .with_presets(vec![
                    PoolSchedulingPreset {
                        preset: "load_balance".to_string(),
                        enabled: true,
                        mode: None,
                    },
                    PoolSchedulingPreset {
                        preset: "priority_first".to_string(),
                        enabled: true,
                        mode: None,
                    },
                ]);
        let key_high_priority =
            sample_candidate("provider-pool", "endpoint-1", "key-high-priority", 10, true)
                .with_presets(vec![
                    PoolSchedulingPreset {
                        preset: "load_balance".to_string(),
                        enabled: true,
                        mode: None,
                    },
                    PoolSchedulingPreset {
                        preset: "priority_first".to_string(),
                        enabled: true,
                        mode: None,
                    },
                ]);
        let nonce = (0..1000)
            .map(|index| format!("seed-{index}"))
            .find(|nonce| {
                let group_seed = format!("provider-pool:endpoint-1:model-1:gpt-5:{nonce}");
                stable_hash_score(format!("{group_seed}:key-random-first").as_str())
                    < stable_hash_score(format!("{group_seed}:key-high-priority").as_str())
            })
            .expect("test seed should exist");

        let outcome = run_pool_scheduler(
            vec![key_random_first, key_high_priority],
            &BTreeMap::new(),
            nonce.as_str(),
        );

        assert!(outcome.skipped_candidates.is_empty());
        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec!["key-high-priority", "key-random-first"]
        );
    }

    #[test]
    fn single_account_distribution_orders_by_priority_then_reverse_lru() {
        let key_priority_old =
            sample_candidate("provider-pool", "endpoint-1", "key-priority-old", 10, true)
                .with_presets(vec![PoolSchedulingPreset {
                    preset: "single_account".to_string(),
                    enabled: true,
                    mode: None,
                }]);
        let key_priority_recent = sample_candidate(
            "provider-pool",
            "endpoint-1",
            "key-priority-recent",
            10,
            true,
        )
        .with_presets(vec![PoolSchedulingPreset {
            preset: "single_account".to_string(),
            enabled: true,
            mode: None,
        }]);
        let key_lower_priority_recent = sample_candidate(
            "provider-pool",
            "endpoint-1",
            "key-lower-priority-recent",
            50,
            true,
        )
        .with_presets(vec![PoolSchedulingPreset {
            preset: "single_account".to_string(),
            enabled: true,
            mode: None,
        }]);

        let runtime_by_provider = BTreeMap::from([(
            "provider-pool".to_string(),
            PoolRuntimeState {
                lru_score_by_key: BTreeMap::from([
                    ("key-priority-old".to_string(), 10.0),
                    ("key-priority-recent".to_string(), 200.0),
                    ("key-lower-priority-recent".to_string(), 500.0),
                ]),
                ..PoolRuntimeState::default()
            },
        )]);

        let outcome = run_pool_scheduler(
            vec![
                key_priority_old,
                key_lower_priority_recent,
                key_priority_recent,
            ],
            &runtime_by_provider,
            "seed",
        );

        assert!(outcome.skipped_candidates.is_empty());
        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec![
                "key-priority-recent",
                "key-priority-old",
                "key-lower-priority-recent"
            ]
        );
    }

    #[test]
    fn ordered_strategies_run_before_every_distribution_mode() {
        for distribution_mode in [
            "cache_affinity",
            "lru",
            "single_account",
            "load_balance",
            "priority_first",
        ] {
            let presets = vec![
                PoolSchedulingPreset {
                    preset: distribution_mode.to_string(),
                    enabled: true,
                    mode: None,
                },
                PoolSchedulingPreset {
                    preset: "quota_balanced".to_string(),
                    enabled: true,
                    mode: None,
                },
            ];
            let mut distribution_first = sample_candidate(
                "provider-pool",
                "endpoint-1",
                "key-distribution-first",
                10,
                true,
            )
            .with_presets(presets.clone())
            .with_quota_usage(0.9);
            let mut quota_first =
                sample_candidate("provider-pool", "endpoint-1", "key-quota-first", 50, true)
                    .with_presets(presets)
                    .with_quota_usage(0.1);

            if distribution_mode == "lru" {
                distribution_first
                    .pool_config
                    .as_mut()
                    .expect("pool config should exist")
                    .lru_enabled = true;
                quota_first
                    .pool_config
                    .as_mut()
                    .expect("pool config should exist")
                    .lru_enabled = true;
            }

            let runtime_by_provider = BTreeMap::from([(
                "provider-pool".to_string(),
                PoolRuntimeState {
                    lru_score_by_key: BTreeMap::from([
                        ("key-distribution-first".to_string(), 200.0),
                        ("key-quota-first".to_string(), 10.0),
                    ]),
                    ..PoolRuntimeState::default()
                },
            )]);
            let nonce = if distribution_mode == "load_balance" {
                (0..1000)
                    .map(|index| format!("seed-{index}"))
                    .find(|nonce| {
                        let group_seed = format!("provider-pool:endpoint-1:model-1:gpt-5:{nonce}");
                        stable_hash_score(format!("{group_seed}:key-distribution-first").as_str())
                            < stable_hash_score(format!("{group_seed}:key-quota-first").as_str())
                    })
                    .expect("test seed should exist")
            } else {
                "seed".to_string()
            };

            let outcome = run_pool_scheduler(
                vec![distribution_first, quota_first],
                &runtime_by_provider,
                nonce.as_str(),
            );

            assert_eq!(
                outcome
                    .candidates
                    .first()
                    .map(|item| item.candidate.as_str()),
                Some("key-quota-first"),
                "{distribution_mode} must only break ties after ordered strategies"
            );
        }
    }

    #[test]
    fn equal_quota_values_continue_to_the_next_ordered_strategy() {
        let presets = vec![
            PoolSchedulingPreset {
                preset: "cache_affinity".to_string(),
                enabled: true,
                mode: None,
            },
            PoolSchedulingPreset {
                preset: "quota_balanced".to_string(),
                enabled: true,
                mode: None,
            },
            PoolSchedulingPreset {
                preset: "health_first".to_string(),
                enabled: true,
                mode: None,
            },
        ];
        let unhealthy_recent = sample_candidate(
            "provider-pool",
            "endpoint-1",
            "key-unhealthy-recent",
            10,
            true,
        )
        .with_presets(presets.clone())
        .with_quota_usage(0.5)
        .with_health(0.2);
        let healthy_old =
            sample_candidate("provider-pool", "endpoint-1", "key-healthy-old", 10, true)
                .with_presets(presets)
                .with_quota_usage(0.5)
                .with_health(0.9);
        let runtime_by_provider = BTreeMap::from([(
            "provider-pool".to_string(),
            PoolRuntimeState {
                lru_score_by_key: BTreeMap::from([
                    ("key-unhealthy-recent".to_string(), 200.0),
                    ("key-healthy-old".to_string(), 10.0),
                ]),
                ..PoolRuntimeState::default()
            },
        )]);

        let outcome = run_pool_scheduler(
            vec![unhealthy_recent, healthy_old],
            &runtime_by_provider,
            "seed",
        );

        assert_eq!(
            outcome
                .candidates
                .iter()
                .map(|item| item.candidate.as_str())
                .collect::<Vec<_>>(),
            vec!["key-healthy-old", "key-unhealthy-recent"]
        );
    }

    #[test]
    fn explicit_cache_binding_remains_a_hard_hit_before_ordered_strategies() {
        let presets = vec![
            PoolSchedulingPreset {
                preset: "cache_affinity".to_string(),
                enabled: true,
                mode: None,
            },
            PoolSchedulingPreset {
                preset: "quota_balanced".to_string(),
                enabled: true,
                mode: None,
            },
        ];
        let bound = sample_candidate("provider-pool", "endpoint-1", "key-bound", 10, true)
            .with_presets(presets.clone())
            .with_quota_usage(0.9);
        let quota_first =
            sample_candidate("provider-pool", "endpoint-1", "key-quota-first", 10, true)
                .with_presets(presets)
                .with_quota_usage(0.1);
        let runtime_by_provider = BTreeMap::from([(
            "provider-pool".to_string(),
            PoolRuntimeState {
                sticky_bound_key_id: Some("key-bound".to_string()),
                ..PoolRuntimeState::default()
            },
        )]);

        let outcome = run_pool_scheduler(vec![quota_first, bound], &runtime_by_provider, "seed");

        assert_eq!(
            outcome
                .candidates
                .first()
                .map(|item| item.candidate.as_str()),
            Some("key-bound")
        );
    }

    #[test]
    fn priority_first_wins_over_other_distribution_modes() {
        let presets = normalize_enabled_pool_presets(&[
            PoolSchedulingPreset {
                preset: "lru".to_string(),
                enabled: false,
                mode: None,
            },
            PoolSchedulingPreset {
                preset: "single_account".to_string(),
                enabled: true,
                mode: None,
            },
            PoolSchedulingPreset {
                preset: "cache_affinity".to_string(),
                enabled: true,
                mode: None,
            },
            PoolSchedulingPreset {
                preset: "priority_first".to_string(),
                enabled: true,
                mode: None,
            },
        ]);

        assert_eq!(presets, ["priority_first"]);
    }

    #[test]
    fn normalizes_lru_as_mutually_exclusive_distribution_mode() {
        let presets = normalize_enabled_pool_presets(&[
            PoolSchedulingPreset {
                preset: "lru".to_string(),
                enabled: true,
                mode: None,
            },
            PoolSchedulingPreset {
                preset: "cache_affinity".to_string(),
                enabled: true,
                mode: None,
            },
            PoolSchedulingPreset {
                preset: "priority_first".to_string(),
                enabled: true,
                mode: None,
            },
        ]);

        assert_eq!(presets, ["priority_first"]);
    }

    fn sample_candidate(
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
        internal_priority: i32,
        pool_enabled: bool,
    ) -> PoolCandidateInput<String> {
        let pool_config = pool_enabled.then(|| PoolSchedulingConfig {
            scheduling_presets: Vec::new(),
            lru_enabled: true,
            skip_exhausted_accounts: false,
            cost_limit_per_key_tokens: None,
        });
        PoolCandidateInput {
            candidate: key_id.to_string(),
            facts: PoolCandidateFacts {
                provider_id: provider_id.to_string(),
                endpoint_id: endpoint_id.to_string(),
                model_id: "model-1".to_string(),
                selected_provider_model_name: "gpt-5".to_string(),
                provider_api_format: "openai:chat".to_string(),
                key_id: key_id.to_string(),
                key_internal_priority: internal_priority,
            },
            pool_config,
            key_context: PoolMemberSignals::default(),
        }
    }

    trait TestCandidateExt {
        fn with_cost_limit(self, limit: u64) -> Self;
        fn with_presets(self, presets: Vec<PoolSchedulingPreset>) -> Self;
        fn with_plan(self, plan: &str) -> Self;
        fn with_quota_usage(self, usage_ratio: f64) -> Self;
        fn with_health(self, health_score: f64) -> Self;
    }

    impl TestCandidateExt for PoolCandidateInput<String> {
        fn with_cost_limit(mut self, limit: u64) -> Self {
            if let Some(config) = self.pool_config.as_mut() {
                config.cost_limit_per_key_tokens = Some(limit);
            }
            self
        }

        fn with_presets(mut self, presets: Vec<PoolSchedulingPreset>) -> Self {
            if let Some(config) = self.pool_config.as_mut() {
                config.scheduling_presets = presets;
            }
            self
        }

        fn with_plan(mut self, plan: &str) -> Self {
            self.key_context.plan_tier = Some(plan.to_string());
            self
        }

        fn with_quota_usage(mut self, usage_ratio: f64) -> Self {
            self.key_context.quota_usage_ratio = Some(usage_ratio);
            self
        }

        fn with_health(mut self, health_score: f64) -> Self {
            self.key_context.health_score = Some(health_score);
            self
        }
    }
}

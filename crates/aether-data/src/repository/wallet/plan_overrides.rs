use serde_json::{Map, Number, Value};

const ADMIN_GRANT_OVERRIDES: &str = "admin_grant_overrides";
const STARTS_AT_UNIX_SECS: &str = "starts_at_unix_secs";
const EXPIRES_AT_UNIX_SECS: &str = "expires_at_unix_secs";
const INITIAL_REMAINING_QUOTA_USD: &str = "initial_remaining_quota_usd";

pub(super) fn plan_provider_ids_snapshot(snapshot: &Value) -> Vec<String> {
    snapshot
        .get("allowed_provider_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn admin_grant_starts_at_unix_secs(snapshot: &Value) -> Option<i64> {
    admin_grant_override_i64(snapshot, STARTS_AT_UNIX_SECS)
}

pub(super) fn admin_grant_expires_at_unix_secs(snapshot: &Value) -> Option<i64> {
    admin_grant_override_i64(snapshot, EXPIRES_AT_UNIX_SECS)
}

pub(super) fn entitlements_with_admin_grant_overrides(
    snapshot: &Value,
    mut entitlements: Value,
) -> Value {
    let Some(cap) = admin_grant_override_f64(snapshot, INITIAL_REMAINING_QUOTA_USD) else {
        return entitlements;
    };
    if !cap.is_finite() || cap < 0.0 {
        return entitlements;
    }
    cap_entitlements(&mut entitlements, cap);
    entitlements
}

fn admin_grant_override_i64(snapshot: &Value, key: &str) -> Option<i64> {
    let value = snapshot.get(ADMIN_GRANT_OVERRIDES)?.get(key)?;
    if let Some(value) = value.as_i64() {
        return Some(value);
    }
    value.as_u64().and_then(|value| i64::try_from(value).ok())
}

fn admin_grant_override_f64(snapshot: &Value, key: &str) -> Option<f64> {
    let value = snapshot.get(ADMIN_GRANT_OVERRIDES)?.get(key)?;
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
}

fn cap_entitlements(entitlements: &mut Value, cap: f64) {
    if let Some(items) = entitlements.as_array_mut() {
        for item in items {
            cap_quota_item(item, cap);
        }
        return;
    }
    cap_quota_item(entitlements, cap);
}

fn cap_quota_item(item: &mut Value, cap: f64) {
    let is_quota = item
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|value| matches!(value, "daily_quota" | "usage_quota"))
        || item.get("limits").is_some();
    if !is_quota {
        return;
    }

    if let Some(map) = item.as_object_mut() {
        for key in [
            "daily_quota_usd",
            "five_hour_quota_usd",
            "weekly_quota_usd",
            "monthly_quota_usd",
            "daily_limit_usd",
            "five_hour_limit_usd",
            "weekly_limit_usd",
            "monthly_limit_usd",
        ] {
            cap_quota_field(map, key, cap);
        }

        if let Some(limits) = map.get_mut("limits").and_then(Value::as_object_mut) {
            for key in [
                "daily_limit_usd",
                "five_hour_limit_usd",
                "weekly_limit_usd",
                "monthly_limit_usd",
            ] {
                cap_quota_field(limits, key, cap);
            }
        }
    }
}

fn cap_quota_field(map: &mut Map<String, Value>, key: &str, cap: f64) {
    let Some(value) = map.get(key) else {
        return;
    };
    let current = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
        .unwrap_or(0.0);
    if !current.is_finite() || current <= 0.0 {
        return;
    }
    let Some(number) = Number::from_f64(current.min(cap)) else {
        return;
    };
    map.insert(key.to_string(), Value::Number(number));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{entitlements_with_admin_grant_overrides, plan_provider_ids_snapshot};

    #[test]
    fn provider_snapshot_is_trimmed_deduplicated_and_sorted() {
        let snapshot = json!({
            "allowed_provider_ids": ["provider-b", " provider-a ", "", "provider-b", 1]
        });

        assert_eq!(
            plan_provider_ids_snapshot(&snapshot),
            vec!["provider-a".to_string(), "provider-b".to_string()]
        );
    }

    #[test]
    fn quota_cap_lowers_positive_limits_without_raising_smaller_limits() {
        let snapshot = json!({
            "admin_grant_overrides": {
                "initial_remaining_quota_usd": 80.0
            }
        });
        let entitlements = json!([
            {
                "type": "daily_quota",
                "daily_quota_usd": 50.0,
                "weekly_quota_usd": 120.0,
                "monthly_quota_usd": 200.0,
                "limits": {
                    "daily_limit_usd": "40",
                    "monthly_limit_usd": "300"
                }
            },
            {
                "type": "membership_group",
                "grant_user_groups": ["vip"]
            }
        ]);

        let capped = entitlements_with_admin_grant_overrides(&snapshot, entitlements);

        assert_eq!(capped[0]["daily_quota_usd"], json!(50.0));
        assert_eq!(capped[0]["weekly_quota_usd"], json!(80.0));
        assert_eq!(capped[0]["monthly_quota_usd"], json!(80.0));
        assert_eq!(capped[0]["limits"]["daily_limit_usd"], json!(40.0));
        assert_eq!(capped[0]["limits"]["monthly_limit_usd"], json!(80.0));
        assert_eq!(capped[1]["grant_user_groups"], json!(["vip"]));
    }
}

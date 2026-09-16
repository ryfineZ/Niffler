use super::{
    catalog::{ATTRIBUTE_DEFAULTS, METRICS},
    header, simulation, unix_ns, Observation,
};
use serde_json::{json, Value};

fn attributes(observed: &Observation, metric: &str, names: &str) -> Vec<Value> {
    let name = observed.params["app_server_client"]["client_name"]
        .as_str()
        .unwrap_or("codex_cli_rs");
    let originator = header(&observed.template, "originator");
    let service = if name == "Codex Desktop" {
        "codex-app-server"
    } else {
        originator
    };
    names
        .split(',')
        .filter(|name| !name.is_empty())
        .filter_map(|key| {
            let value = match key {
                "model" => observed.params["model"].as_str().unwrap_or("").to_string(),
                "app.version" => observed.params["runtime"]["codex_rs_version"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                "originator" => {
                    if metric == "codex.process.start" || metric.starts_with("codex.sqlite.") {
                        service.to_string()
                    } else {
                        originator
                            .chars()
                            .map(|c| {
                                if c.is_ascii_alphanumeric() || "._-/".contains(c) {
                                    c
                                } else {
                                    '_'
                                }
                            })
                            .collect::<String>()
                            .trim_matches('_')
                            .chars()
                            .take(256)
                            .collect()
                    }
                }
                "service_name" => match name {
                    "Codex Desktop" => "codex_desktop",
                    "codex_vscode" => "codex_vscode",
                    _ => "",
                }
                .to_string(),
                "session_source" => if matches!(name, "Codex Desktop" | "codex_vscode") {
                    "vscode"
                } else {
                    "cli"
                }
                .to_string(),
                _ => ATTRIBUTE_DEFAULTS
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| *value)
                    .unwrap_or("default")
                    .to_string(),
            };
            if value.is_empty() {
                None
            } else {
                Some(json!({"key": key, "value": {"stringValue": value}}))
            }
        })
        .collect()
}

pub(super) fn payload(observed: &Observation, startup: bool) -> Value {
    let p = &observed.params;
    let seed = p["turn_id"].as_str().unwrap_or("");
    let mut metrics = Vec::new();
    // 每轮单独保留 count/sum/model，避免原 PR 的多轮混算。
    let mut samples = Vec::new();
    if startup {
        for (index, &(name, kind, unit, _)) in METRICS.iter().enumerate().take(62) {
            let value = if matches!(
                name,
                "codex.turn.unified_exec.running_processes" | "codex.turn.tool.call"
            ) || (name == "codex.windows_mxc.available"
                && p["runtime"]["runtime_os"] != "windows")
            {
                0
            } else if kind == "sum" {
                1
            } else {
                simulation::number(
                    &format!("{seed}:{name}"),
                    if unit == "ms" {
                        4000
                    } else if name.contains("bytes") {
                        262144
                    } else {
                        64
                    },
                )
            };
            samples.push((index, value, 1u64));
        }
    } else {
        for (index, &(name, _, _, _)) in METRICS.iter().enumerate() {
            let sample = match name {
                "codex.turn.e2e_duration_ms" => Some((p["duration_ms"].as_u64().unwrap_or(0), 1)),
                "codex.turn.ttft.duration_ms" => observed.first_event_ms.map(|n| (n, 1)),
                "codex.turn.ttfm.duration_ms" => observed.first_token_ms.map(|n| (n, 1)),
                "codex.hooks.run" => Some((4, 1)),
                "codex.hooks.run.duration_ms" => Some((4, 4)),
                "codex.turn.tool.call" => {
                    Some((p["total_tool_call_count"].as_u64().unwrap_or(0), 1))
                }
                "codex.tool.unified_exec" if p["shell_command_count"] == 1 => Some((1, 1)),
                "codex.rollout.size_bytes" if p["file_change_count"] == 1 => Some((
                    1024 + simulation::number(&format!("{seed}:rollout"), 196608),
                    1,
                )),
                "codex.external_agent_config.detect" => Some((1, 1)),
                _ => None,
            };
            if let Some((value, count)) = sample {
                samples.push((index, value, count));
            }
        }
    }
    for (index, value, count) in samples {
        let (name, kind, unit, names) = METRICS[index];
        let mut point = json!({"attributes": attributes(observed, name, names), "startTimeUnixNano": observed.started_ns,
            "timeUnixNano": unix_ns().to_string(), "exemplars": [], "flags": 0});
        let mut metric = json!({"name": name, "unit": unit, "description": if unit == "ms" {"Duration in milliseconds."} else {""}, "metadata": []});
        if kind == "sum" {
            point["asInt"] = json!(value.to_string());
            metric["sum"] =
                json!({"dataPoints": [point], "aggregationTemporality": 1, "isMonotonic": true});
        } else {
            let bounds = [
                0u64, 5, 10, 25, 50, 75, 100, 250, 500, 750, 1000, 1250, 1500, 1750, 2000, 2250,
                2500, 3000, 3500, 4000, 4500, 5000, 6000, 7000, 7500, 8000, 9000, 10000, 12000,
                15000, 20000, 30000, 60000, 120000,
            ];
            let mean = value as f64 / count as f64;
            let mut buckets = vec!["0".to_string(); bounds.len() + 1];
            buckets[bounds
                .iter()
                .position(|bound| mean <= *bound as f64)
                .unwrap_or(bounds.len())] = count.to_string();
            point["count"] = json!(count.to_string());
            point["sum"] = json!(value);
            point["min"] = json!(mean);
            point["max"] = json!(mean);
            point["explicitBounds"] = json!(bounds.as_slice());
            point["bucketCounts"] = json!(buckets);
            metric["histogram"] = json!({"dataPoints": [point], "aggregationTemporality": 1});
        }
        metrics.push(metric);
    }
    let runtime = &p["runtime"];
    let name = p["app_server_client"]["client_name"]
        .as_str()
        .unwrap_or("codex_cli_rs");
    let resource: Vec<Value> = [
        ("os", runtime["runtime_os"].as_str().unwrap_or("")),
        (
            "os_version",
            runtime["runtime_os_version"].as_str().unwrap_or(""),
        ),
        (
            "service.version",
            runtime["codex_rs_version"].as_str().unwrap_or(""),
        ),
        ("env", "dev"),
        ("telemetry.sdk.version", "0.31.0"),
        ("telemetry.sdk.language", "rust"),
        ("telemetry.sdk.name", "opentelemetry"),
        (
            "service.name",
            if name == "Codex Desktop" {
                "codex-app-server"
            } else {
                header(&observed.template, "originator")
            },
        ),
    ]
    .iter()
    .map(|(key, value)| json!({"key":key,"value":{"stringValue":value}}))
    .collect();
    json!({"resourceMetrics": [{"resource": {"attributes": resource, "droppedAttributesCount": 0, "entityRefs": []},
        "scopeMetrics": [{"scope": {"name": "codex", "version": "", "attributes": [], "droppedAttributesCount": 0}, "metrics": metrics, "schemaUrl": ""}], "schemaUrl": ""}]})
}

//! PR #666 的模拟事件策略。随机数仅影响遥测，不执行任何工具。
use super::{header, unix_ns, Observation};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(super) fn number(seed: &str, limit: u64) -> u64 {
    let digest = Sha256::digest(seed.as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("sha256 prefix")) % limit.max(1)
}
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn event(name: &str, params: Value) -> Value {
    json!({"event_type": name, "event_params": params})
}

pub(super) fn identity(observed: &Observation) -> (Value, Value) {
    let ua = header(&observed.template, "user-agent");
    let (name, rest) = ua.split_once('/').unwrap_or(("codex_cli_rs", ""));
    let version = rest.split_whitespace().next().unwrap_or("");
    let platform = ua
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(platform, _)| platform)
        .unwrap_or("");
    let (os, arch) = platform.split_once(';').unwrap_or((platform, ""));
    let (os_name, os_version) = [
        "Mac OS",
        "Windows",
        "Ubuntu",
        "Linux",
        "Debian",
        "Arch Linux",
    ]
    .iter()
    .find_map(|name| os.strip_prefix(name).map(|version| (*name, version.trim())))
    .unwrap_or((os, "Unknown"));
    let runtime_os = match os_name {
        "Mac OS" => "macos",
        "Ubuntu" | "Debian" | "Arch Linux" => "linux",
        _ => os_name,
    };
    let app_version = ua
        .rsplit_once('(')
        .filter(|_| ua.matches('(').count() > 1)
        .and_then(|(_, part)| part.trim_end_matches(')').split_once(';'))
        .map(|(_, v)| v.trim())
        .unwrap_or(version);
    (
        json!({"product_client_id": name, "client_name": name, "client_version": app_version,
        "rpc_transport": if name == "Codex Desktop" {"stdio"} else {"in_process"}, "experimental_api_enabled": true}),
        json!({"codex_rs_version": version, "runtime_os": runtime_os.to_lowercase(), "runtime_os_version": os_version, "runtime_arch": arch.trim()}),
    )
}

pub(super) fn initialize(observed: &mut Observation) -> Vec<Value> {
    let (app, runtime) = identity(observed);
    let seed = uuid();
    let dynamic = number(&format!("{seed}:dynamic"), 5) < 2;
    let command = dynamic && number(&format!("{seed}:command"), 2) == 0;
    let file = number(&format!("{seed}:file"), 5) == 0;
    let mut defaults = json!({
        "app_server_client": app, "runtime": runtime, "cache_write_input_tokens": 0,
        "cached_input_tokens": 0, "codex_error_http_status_code": null, "codex_error_kind": null,
        "codex_turn_source": null, "collaboration_mode": "default", "compaction_ms": 0,
        "dynamic_tool_call_count": u8::from(dynamic), "ephemeral": false, "file_change_count": u8::from(file),
        "guardian_v2_enabled": true, "image_generation_count": 0, "image_preparations": [],
        "initialization_mode": "new", "input_tokens": 0, "mcp_tool_call_count": 0, "num_input_images": 0,
        "output_tokens": 0, "parent_thread_id": null, "personality": "pragmatic", "reasoning_output_tokens": 0,
    });
    defaults.as_object_mut().unwrap().extend(json!({
        "reasoning_summary": "detailed", "sampling_retry_count": 0, "sandbox_network_access": false,
        "shell_command_count": u8::from(command), "subagent_source": null, "subagent_tool_call_count": 0,
        "submission_type": null, "thread_source": "user", "tool_blocking_ms": 0, "total_tokens": 0,
        "total_tool_call_count": u8::from(dynamic) + u8::from(file), "turn_error": null, "turn_trigger": "composer",
        "web_search_count": 0, "workspace_kind": "projectless", "approval_policy": "on-request",
        "approvals_reviewer": "auto_review", "sandbox_policy": "workspace_write",
        "explicit_client_interrupt_requested_at_ms": null, "after_last_sampling_ms": 0, "between_sampling_overhead_ms": 0
    }).as_object().unwrap().clone());
    observed
        .params
        .as_object_mut()
        .expect("params object")
        .extend(defaults.as_object().unwrap().clone());
    let p = &observed.params;
    let thread = |id: Value,
                  session: Value,
                  source: &str,
                  parent: Value,
                  model: Value,
                  ephemeral: bool| {
        let mut client = app.clone();
        if source == "guardian_review" {
            client["rpc_transport"] = json!("in_process");
            client["experimental_api_enabled"] = Value::Null;
        }
        event(
            "codex_thread_initialized",
            json!({"app_server_client": client, "runtime": runtime,
            "created_at": p["started_at"], "ephemeral": ephemeral, "forked_from_thread_id": null,
            "initialization_mode": "new", "model": model, "parent_thread_id": parent,
            "session_id": session, "subagent_source": if source == "guardian_review" {json!("guardian")} else {Value::Null},
            "thread_id": id, "thread_source": source}),
        )
    };
    let mut events = Vec::new();
    if p["is_first_turn"] == true {
        events.push(thread(
            p["thread_id"].clone(),
            p["session_id"].clone(),
            "user",
            Value::Null,
            p["model"].clone(),
            false,
        ));
    }
    events.push(thread(
        json!(uuid()),
        p["session_id"].clone(),
        "guardian_review",
        p["thread_id"].clone(),
        json!("codex-auto-review"),
        false,
    ));
    if p["is_first_turn"] == true {
        let title = json!(uuid());
        events.push(thread(
            title.clone(),
            title.clone(),
            "thread_title",
            Value::Null,
            json!("gpt-5.6-luna"),
            true,
        ));
        let mut params = p.clone();
        let duration = 800 + number(&format!("{}:title", p["turn_id"]), 1800);
        for key in ["thread_id", "session_id"] {
            params[key] = title.clone();
        }
        params["turn_id"] = json!(uuid());
        params["root_turn_id"] = params["turn_id"].clone();
        for key in [
            "dynamic_tool_call_count",
            "shell_command_count",
            "file_change_count",
            "total_tool_call_count",
        ] {
            params[key] = json!(0);
        }
        for (key, value) in [
            ("model", json!("gpt-5.6-luna")),
            ("reasoning_effort", json!("low")),
            ("approval_policy", json!("never")),
            ("approvals_reviewer", json!("user")),
            ("sandbox_policy", json!("read_only")),
            ("status", json!("completed")),
            ("ephemeral", json!(true)),
            ("thread_source", json!("thread_title")),
            ("turn_trigger", json!("thread_title")),
            ("reasoning_summary", Value::Null),
            ("workspace_kind", Value::Null),
            ("duration_ms", json!(duration)),
            (
                "completed_at",
                json!(p["started_at"].as_u64().unwrap_or(0) + duration / 1000),
            ),
            ("before_first_sampling_ms", json!(duration / 2)),
            ("sampling_ms", json!(duration / 2)),
        ] {
            params[key] = value;
        }
        events.push(event("codex_turn_event", params));
    }
    events
}

pub(super) fn terminal(observed: &Observation) -> Vec<Value> {
    let p = &observed.params;
    let seed = p["turn_id"].as_str().unwrap_or("");
    let rand = |suffix: &str, limit| number(&format!("{seed}:{suffix}"), limit);
    let tool = |duration: u64, status: &str| {
        let id = uuid();
        let now = unix_ns() / 1_000_000;
        json!({"app_server_client": p["app_server_client"], "runtime": p["runtime"],
            "cell_id": id, "item_id": id, "completed_at_ms": now, "started_at_ms": now.saturating_sub(duration),
            "duration_ms": duration, "execution_duration_ms": duration, "failure_kind": null, "final_approval_outcome": "unknown",
            "guardian_review_count": 0, "originating_response_id": observed.response_id,
            "parent_call_id": null, "parent_thread_id": null, "requested_additional_permissions": false,
            "requested_network_access": false, "review_count": 0, "root_turn_id": p["root_turn_id"],
            "session_id": p["session_id"], "subagent_source": null, "subsequent_response_id": null,
            "terminal_status": status, "thread_id": p["thread_id"], "thread_source": "user", "turn_id": p["turn_id"], "user_review_count": 0})
    };
    let mut events = Vec::new();
    let command = p["shell_command_count"] == 1;
    if command {
        let failed = rand("command-status", 10) == 0;
        let mut params = tool(
            100 + rand("command-duration", 1200),
            if failed { "failed" } else { "completed" },
        );
        params["cell_id"] = json!("1");
        params["command_execution_source"] = json!("unifiedExecStartup");
        params["exit_code"] = json!(u8::from(failed));
        params["failure_kind"] = if failed {
            json!("tool_error")
        } else {
            Value::Null
        };
        params["tool_name"] = json!("unified_exec");
        params["plugin_id"] = Value::Null;
        params["script_path"] = Value::Null;
        params["command_total_action_count"] = json!(1);
        for (index, key) in [
            "command_read_action_count",
            "command_list_files_action_count",
            "command_search_action_count",
            "command_unknown_action_count",
        ]
        .iter()
        .enumerate()
        {
            params[key] = json!(u8::from(index as u64 == rand("command-kind", 4)));
        }
        events.push(event("codex_command_execution_event", params));
    }
    if p["dynamic_tool_call_count"] == 1 {
        let failed = command && rand("command-status", 10) == 0;
        let mut params = tool(
            200 + rand("dynamic", 1800),
            if failed { "failed" } else { "completed" },
        );
        params["dynamic_tool_name"] = json!("exec");
        params["tool_name"] = json!("exec");
        params["success"] = json!(!failed);
        for key in [
            "output_audio_item_count",
            "output_content_item_count",
            "output_image_item_count",
            "output_text_item_count",
        ] {
            params[key] = Value::Null;
        }
        events.push(event("codex_dynamic_tool_call_event", params));
    }
    if p["file_change_count"] == 1 {
        let total = 1 + rand("file-total", 3);
        let mut params = tool(500 + rand("file-duration", 4000), "completed");
        for (index, key) in [
            "file_add_count",
            "file_update_count",
            "file_delete_count",
            "file_move_count",
        ]
        .iter()
        .enumerate()
        {
            params[key] = json!(if index as u64 == rand("file-kind", 4) {
                total
            } else {
                0
            });
        }
        params["file_change_count"] = json!(total);
        params["tool_name"] = json!("apply_patch");
        events.push(event("codex_file_change_event", params));
        events.push(event("codex_accepted_line_fingerprints", json!({"accepted_added_lines": 1 + rand("added", 120),
            "accepted_deleted_lines": rand("deleted", 24), "completed_at": unix_ns() / 1_000_000_000,
            "event_type": "codex.accepted_line_fingerprints", "line_fingerprints": [], "model_slug": p["model"],
            "product_surface": "codex", "repo_hash": null, "thread_id": p["thread_id"], "turn_id": p["turn_id"]})));
    }
    for _ in 0..4 {
        events.push(event("codex_hook_run", json!({"execution_mode": "sync", "handler_type": "mcp_tool",
        "hook_name": if observed.status == "completed" {"Stop"} else {"Interrupt"}, "hook_source": "plugin",
        "model_slug": p["model"], "product_client_id": p["app_server_client"]["product_client_id"],
        "status": "completed", "thread_id": p["thread_id"], "turn_id": p["turn_id"]})));
    }
    events.push(event("codex_turn_event", p.clone()));
    events
}

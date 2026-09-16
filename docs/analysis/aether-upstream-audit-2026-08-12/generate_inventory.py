#!/usr/bin/env python3
"""Generate reproducible Niffler/Aether divergence inventories.

This script is intentionally read-only with respect to Git. It writes generated
TSV/Markdown artifacts only below the audit directory containing this file.
"""

from __future__ import annotations

import csv
import json
import re
import subprocess
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path


AUDIT_DIR = Path(__file__).resolve().parent
REPO_DIR = AUDIT_DIR.parents[2]
GENERATED_DIR = AUDIT_DIR / "generated"
BASE = "ed75ae6d56ab03eb5e6e3cd87f2137880c99694d"
NIFFLER = "908443291"
AETHER = "654c4f697"


def git_bytes(*args: str) -> bytes:
    return subprocess.check_output(["git", *args], cwd=REPO_DIR)


def git_text(*args: str) -> str:
    return git_bytes(*args).decode("utf-8", errors="replace")


def resolve(revision: str) -> str:
    return git_text("rev-parse", revision).strip()


@dataclass(frozen=True)
class TreeEntry:
    mode: str
    object_type: str
    object_id: str


def tree_entries(revision: str) -> dict[str, TreeEntry]:
    result: dict[str, TreeEntry] = {}
    raw = git_bytes("ls-tree", "-r", "-z", revision)
    for record in raw.split(b"\0"):
        if not record:
            continue
        metadata, path = record.split(b"\t", 1)
        mode, object_type, object_id = metadata.decode().split(" ", 2)
        result[path.decode("utf-8", errors="replace")] = TreeEntry(
            mode=mode,
            object_type=object_type,
            object_id=object_id,
        )
    return result


def diff_status(old: str, new: str) -> dict[str, str]:
    result: dict[str, str] = {}
    tokens = [
        token
        for token in git_bytes(
            "diff", "--name-status", "--no-renames", "-z", old, new
        ).split(b"\0")
        if token
    ]
    if len(tokens) % 2 != 0:
        raise RuntimeError(f"unexpected --name-status token count: {len(tokens)}")
    for index in range(0, len(tokens), 2):
        status = tokens[index].decode()
        path = tokens[index + 1].decode("utf-8", errors="replace")
        result[path] = status
    return result


def diff_numstat(old: str, new: str) -> dict[str, tuple[str, str]]:
    result: dict[str, tuple[str, str]] = {}
    raw = git_bytes("diff", "--numstat", "--no-renames", "-z", old, new)
    for record in raw.split(b"\0"):
        if not record:
            continue
        additions, deletions, path = record.split(b"\t", 2)
        result[path.decode("utf-8", errors="replace")] = (
            additions.decode(),
            deletions.decode(),
        )
    return result


def rename_records(old: str, new: str) -> list[tuple[str, str, str]]:
    tokens = [
        token
        for token in git_bytes("diff", "--name-status", "-M", "-z", old, new).split(
            b"\0"
        )
        if token
    ]
    rows: list[tuple[str, str, str]] = []
    index = 0
    while index < len(tokens):
        status = tokens[index].decode("utf-8", errors="replace")
        index += 1
        if not status.startswith(("R", "C")):
            index += 1
            continue
        if index + 1 >= len(tokens):
            raise RuntimeError(f"incomplete rename record for {status}")
        old_path = tokens[index].decode("utf-8", errors="replace")
        new_path = tokens[index + 1].decode("utf-8", errors="replace")
        index += 2
        rows.append((status, old_path, new_path))
    return rows


PATH_RULES: list[tuple[str, tuple[str, ...]]] = [
    (
        "stream_execution",
        (
            "execution_runtime/stream",
            "execution_runtime/stream_pump",
            "executor/stream",
            "stream_core",
            "stream_rewrite",
        ),
    ),
    (
        "routing_scheduler",
        (
            "ai_serving/planner",
            "/dispatch/",
            "/scheduler/",
            "routing-core",
            "routing_profiles",
            "candidate_selection",
            "candidate_source",
            "candidate_ranking",
            "pool_scheduler",
        ),
    ),
    (
        "runtime_performance",
        (
            "runtime-state",
            "/cache/",
            "auth_runtime",
            "candidate_cache",
            "request_candidate",
            "upstream_admission",
            "stage_metrics",
            "allocator_metrics",
            "usage-runtime",
            "usage/runtime",
        ),
    ),
    (
        "billing_wallet",
        (
            "/billing/",
            "aether-billing",
            "aether-wallet",
            "/wallet",
            "payment",
            "pricing",
            "plan_entitlement",
            "user_plan",
            "settlement",
            "refund",
        ),
    ),
    (
        "usage_observability",
        (
            "/usage/",
            "usage_",
            "usage-",
            "observability",
            "monitoring",
            "dashboard",
            "health",
            "request_diagnostics",
            "request_timeline",
        ),
    ),
    (
        "auth_security",
        (
            "/auth/",
            "/oauth/",
            "security",
            "privacy",
            "redaction",
            "turnstile",
            "permission",
            "api_key",
            "api-key",
        ),
    ),
    (
        "provider_protocol",
        (
            "aether-provider",
            "aether-ai-formats",
            "provider/",
            "providers/",
            "/formats/",
            "openai",
            "codex",
            "claude",
            "gemini",
            "grok",
            "kiro",
            "windsurf",
            "antigravity",
            "deepseek",
        ),
    ),
    (
        "data_storage",
        (
            "aether-data",
            "/data/",
            "/migrations/",
            "/schema/",
            "postgres",
            "sqlite",
            "mysql",
            "database",
        ),
    ),
    (
        "frontend_product",
        (
            "frontend/src/features",
            "frontend/src/views",
            "frontend/src/components",
            "frontend/src/layouts",
            "frontend/src/router",
            "frontend/src/stores",
            "frontend/src/api",
            "frontend/src/i18n",
        ),
    ),
    (
        "tunnel_frontdoor",
        (
            "aether-tunnel",
            "/tunnel/",
            "frontdoor",
            "caddy",
            "nginx",
            "proxy-node",
        ),
    ),
    (
        "delivery_operations",
        (
            ".github/",
            "docker",
            "deploy/",
            "scripts/",
            "install.sh",
            "update.sh",
            "backup",
            "tools/pressure",
            ".env.example",
        ),
    ),
    ("documentation", ("docs/", "readme", "changelog", "license")),
    (
        "build_dependencies",
        (
            "cargo.toml",
            "cargo.lock",
            "package.json",
            "package-lock.json",
            "tsconfig",
            "vite.config",
            "vitest.config",
            "makefile",
        ),
    ),
]


def classify_path(path: str) -> tuple[str, list[str]]:
    lower = path.lower()
    tags = [name for name, needles in PATH_RULES if any(needle in lower for needle in needles)]
    if path.startswith("frontend/") and "frontend_product" not in tags:
        tags.append("frontend_product")
    if "/tests/" in lower or lower.endswith(("_test.rs", ".spec.ts", ".test.ts")):
        tags.append("tests")
    if not tags:
        if path.startswith("crates/"):
            tags.append("backend_shared")
        elif path.startswith("apps/aether-gateway/"):
            tags.append("gateway_other")
        else:
            tags.append("repository_other")
    primary = next((tag for tag in tags if tag != "tests"), tags[0])
    return primary, tags


COMMIT_TOPIC_RULES: list[tuple[str, str]] = [
    ("stream_execution", r"stream|first.?byte|ttfb|sse|terminal|disconnect|heartbeat|watchdog|timeout|failover|fallback|retry"),
    ("routing_scheduler", r"routing|scheduler|candidate|pool|priority|admission|concurr"),
    ("runtime_performance", r"redis|cache|performance|perf[:(]|hot path|20k|pressure|worker|queue|database|postgres|mysql|sqlite"),
    ("billing_wallet", r"billing|wallet|payment|pricing|settle|refund|plan|cost"),
    ("usage_observability", r"usage|monitor|dashboard|health|timeline|diagnostic|metric|trace"),
    ("auth_security", r"auth|oauth|security|privacy|pii|redact|permission|turnstile|api key"),
    ("provider_protocol", r"provider|openai|responses|codex|claude|gemini|grok|kiro|windsurf|antigravity|deepseek|model|format"),
    ("frontend_product", r"frontend|ui|mobile|dialog|sidebar|i18n|display|page|view"),
    ("delivery_operations", r"deploy|docker|install|release|ci|backup|tunnel|build"),
]


def classify_subject(subject: str) -> str:
    lower = subject.lower()
    for topic, pattern in COMMIT_TOPIC_RULES:
        if re.search(pattern, lower):
            return topic
    return "other"


def write_tsv(path: Path, fieldnames: list[str], rows: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=fieldnames,
            delimiter="\t",
            extrasaction="ignore",
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(rows)


def commit_rows(old: str, new: str, branch: str) -> list[dict[str, object]]:
    fmt = "%H%x09%ad%x09%an%x09%P%x09%s"
    text = git_text("log", "--reverse", "--date=iso-strict", f"--format={fmt}", f"{old}..{new}")
    rows: list[dict[str, object]] = []
    for line in text.splitlines():
        if not line:
            continue
        commit, date, author, parents, subject = line.split("\t", 4)
        parent_count = len(parents.split())
        rows.append(
            {
                "branch": branch,
                "commit": commit,
                "date": date,
                "author": author,
                "parent_count": parent_count,
                "is_merge": "yes" if parent_count > 1 else "no",
                "topic": classify_subject(subject),
                "subject": subject,
            }
        )
    return rows


def commit_impact_rows(commits: list[dict[str, object]]) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for commit_row in commits:
        commit = str(commit_row["commit"])
        parents = git_text("rev-list", "--parents", "-n", "1", commit).strip().split()[1:]
        if not parents:
            continue
        first_parent = parents[0]
        raw = git_bytes("diff", "--numstat", "--no-renames", "-z", first_parent, commit)
        changed_files = 0
        additions = 0
        deletions = 0
        binary_files = 0
        subsystems: Counter[str] = Counter()
        components: Counter[str] = Counter()
        for record in raw.split(b"\0"):
            if not record:
                continue
            added, deleted, path_raw = record.split(b"\t", 2)
            path = path_raw.decode("utf-8", errors="replace")
            changed_files += 1
            if added == b"-" or deleted == b"-":
                binary_files += 1
            else:
                additions += int(added)
                deletions += int(deleted)
            primary, _ = classify_path(path)
            subsystems[primary] += 1
            components[top_component(path)] += 1
        top_subsystems = ",".join(
            f"{name}:{count}" for name, count in subsystems.most_common(5)
        )
        top_components = ",".join(
            f"{name}:{count}" for name, count in components.most_common(5)
        )
        rows.append(
            {
                **commit_row,
                "first_parent": first_parent,
                "changed_files": changed_files,
                "additions": additions,
                "deletions": deletions,
                "binary_files": binary_files,
                "top_subsystems": top_subsystems,
                "top_components": top_components,
            }
        )
    return rows


def as_int(value: str) -> int:
    return int(value) if value.isdigit() else 0


def top_component(path: str) -> str:
    parts = path.split("/")
    if not parts:
        return path
    if parts[0] in {"apps", "crates"} and len(parts) > 1:
        return "/".join(parts[:2])
    if parts[0] == "frontend" and len(parts) > 2 and parts[1] == "src":
        return "/".join(parts[:3])
    return parts[0]


def conflict_risk(
    path: str,
    n_changed: bool,
    a_changed: bool,
    final_equal: bool,
    primary: str,
) -> str:
    if n_changed and a_changed and final_equal:
        return "aligned"
    critical = {
        "stream_execution",
        "routing_scheduler",
        "runtime_performance",
        "billing_wallet",
        "data_storage",
        "auth_security",
    }
    if n_changed and a_changed:
        return "critical" if primary in critical or "/migrations/" in path else "high"
    if n_changed:
        return "high" if primary in critical else "medium"
    if a_changed:
        return "high" if primary in critical else "low"
    return "none"


def main() -> None:
    if git_text("rev-parse", "--is-shallow-repository").strip() == "true":
        raise RuntimeError(
            "完整历史审计不能在浅克隆中运行；请先执行 git fetch --unshallow"
        )
    base = resolve(BASE)
    niffler = resolve(NIFFLER)
    aether = resolve(AETHER)
    merge_base = git_text("merge-base", niffler, aether).strip()
    if merge_base != base:
        raise RuntimeError(f"unexpected merge base: {merge_base} != {base}")

    GENERATED_DIR.mkdir(parents=True, exist_ok=True)

    base_tree = tree_entries(base)
    n_tree = tree_entries(niffler)
    a_tree = tree_entries(aether)
    n_status = diff_status(base, niffler)
    a_status = diff_status(base, aether)
    final_status = diff_status(niffler, aether)
    n_num = diff_numstat(base, niffler)
    a_num = diff_numstat(base, aether)

    paths = sorted(set(n_status) | set(a_status))
    path_rows: list[dict[str, object]] = []
    for path in paths:
        n_changed = path in n_status
        a_changed = path in a_status
        base_entry = base_tree.get(path)
        n_entry = n_tree.get(path)
        a_entry = a_tree.get(path)
        final_equal = n_entry == a_entry
        primary, tags = classify_path(path)
        if n_changed and a_changed:
            change_scope = "both_changed_same" if final_equal else "both_changed_diverged"
        elif n_changed:
            change_scope = "niffler_only"
        else:
            change_scope = "aether_only"
        n_add, n_del = n_num.get(path, ("0", "0"))
        a_add, a_del = a_num.get(path, ("0", "0"))
        path_rows.append(
            {
                "path": path,
                "top_component": top_component(path),
                "primary_subsystem": primary,
                "topic_tags": ",".join(tags),
                "change_scope": change_scope,
                "conflict_risk": conflict_risk(path, n_changed, a_changed, final_equal, primary),
                "base_exists": "yes" if base_entry else "no",
                "niffler_status": n_status.get(path, ""),
                "niffler_additions": n_add,
                "niffler_deletions": n_del,
                "niffler_exists": "yes" if n_entry else "no",
                "aether_status": a_status.get(path, ""),
                "aether_additions": a_add,
                "aether_deletions": a_del,
                "aether_exists": "yes" if a_entry else "no",
                "final_equal": "yes" if final_equal else "no",
                "niffler_to_aether_status": final_status.get(path, ""),
            }
        )

    path_fields = [
        "path",
        "top_component",
        "primary_subsystem",
        "topic_tags",
        "change_scope",
        "conflict_risk",
        "base_exists",
        "niffler_status",
        "niffler_additions",
        "niffler_deletions",
        "niffler_exists",
        "aether_status",
        "aether_additions",
        "aether_deletions",
        "aether_exists",
        "final_equal",
        "niffler_to_aether_status",
    ]
    write_tsv(GENERATED_DIR / "path_inventory.tsv", path_fields, path_rows)
    write_tsv(
        GENERATED_DIR / "overlap_paths.tsv",
        path_fields,
        [row for row in path_rows if str(row["change_scope"]).startswith("both_changed")],
    )

    commit_fields = ["branch", "commit", "date", "author", "parent_count", "is_merge", "topic", "subject"]
    n_commits = commit_rows(base, niffler, "niffler")
    a_commits = commit_rows(base, aether, "aether")
    write_tsv(GENERATED_DIR / "niffler_commits.tsv", commit_fields, n_commits)
    write_tsv(GENERATED_DIR / "aether_commits.tsv", commit_fields, a_commits)

    commit_impact_fields = commit_fields + [
        "first_parent",
        "changed_files",
        "additions",
        "deletions",
        "binary_files",
        "top_subsystems",
        "top_components",
    ]
    write_tsv(
        GENERATED_DIR / "niffler_commit_impacts.tsv",
        commit_impact_fields,
        commit_impact_rows(n_commits),
    )
    write_tsv(
        GENERATED_DIR / "aether_commit_impacts.tsv",
        commit_impact_fields,
        commit_impact_rows(a_commits),
    )

    rename_fields = ["branch", "status", "old_path", "new_path"]
    rename_rows = [
        {"branch": branch, "status": status, "old_path": old_path, "new_path": new_path}
        for branch, old, new in (
            ("niffler", base, niffler),
            ("aether", base, aether),
            ("final_niffler_to_aether", niffler, aether),
        )
        for status, old_path, new_path in rename_records(old, new)
    ]
    write_tsv(GENERATED_DIR / "renames.tsv", rename_fields, rename_rows)

    by_subsystem: dict[str, Counter[str]] = defaultdict(Counter)
    by_component: dict[str, Counter[str]] = defaultdict(Counter)
    for row in path_rows:
        scope = str(row["change_scope"])
        subsystem = str(row["primary_subsystem"])
        component = str(row["top_component"])
        by_subsystem[subsystem][scope] += 1
        by_subsystem[subsystem]["niffler_additions"] += as_int(str(row["niffler_additions"]))
        by_subsystem[subsystem]["niffler_deletions"] += as_int(str(row["niffler_deletions"]))
        by_subsystem[subsystem]["aether_additions"] += as_int(str(row["aether_additions"]))
        by_subsystem[subsystem]["aether_deletions"] += as_int(str(row["aether_deletions"]))
        by_component[component][scope] += 1

    summary_fields = [
        "group",
        "niffler_only",
        "aether_only",
        "both_changed_same",
        "both_changed_diverged",
        "niffler_additions",
        "niffler_deletions",
        "aether_additions",
        "aether_deletions",
    ]
    subsystem_rows = [{"group": name, **counts} for name, counts in sorted(by_subsystem.items())]
    write_tsv(GENERATED_DIR / "subsystem_summary.tsv", summary_fields, subsystem_rows)
    component_rows = [{"group": name, **counts} for name, counts in sorted(by_component.items())]
    write_tsv(GENERATED_DIR / "component_summary.tsv", summary_fields[:5], component_rows)

    scope_counts = Counter(str(row["change_scope"]) for row in path_rows)
    risk_counts = Counter(str(row["conflict_risk"]) for row in path_rows)
    metadata = {
        "base": base,
        "niffler": niffler,
        "aether": aether,
        "niffler_unique_commits": len(n_commits),
        "aether_unique_commits": len(a_commits),
        "union_changed_paths": len(path_rows),
        "scope_counts": dict(sorted(scope_counts.items())),
        "risk_counts": dict(sorted(risk_counts.items())),
        "rename_records": len(rename_rows),
    }
    (GENERATED_DIR / "metadata.json").write_text(
        json.dumps(metadata, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    summary_lines = [
        "# 自动生成的全量差异清单摘要",
        "",
        "> 本文件由 `generate_inventory.py` 从固定提交号生成，不包含人工处置结论。",
        "",
        "## 固定基线",
        "",
        f"- 共同祖先：`{base}`",
        f"- Niffler 主线：`{niffler}`",
        f"- Aether 主线：`{aether}`",
        f"- Niffler 独有提交：{len(n_commits)}",
        f"- Aether 独有提交：{len(a_commits)}",
        f"- 两侧改动路径并集：{len(path_rows)}",
        "",
        "## 路径关系",
        "",
        "| 类型 | 路径数 |",
        "|---|---:|",
    ]
    for key in ["niffler_only", "aether_only", "both_changed_same", "both_changed_diverged"]:
        summary_lines.append(f"| `{key}` | {scope_counts.get(key, 0)} |")
    summary_lines.extend(
        [
            "",
            "## 初步冲突等级",
            "",
            "| 等级 | 路径数 |",
            "|---|---:|",
        ]
    )
    for key in ["critical", "high", "medium", "low", "aligned"]:
        summary_lines.append(f"| `{key}` | {risk_counts.get(key, 0)} |")
    summary_lines.extend(
        [
            "",
            "## 子系统路径数量",
            "",
            "| 子系统 | Niffler 独有 | Aether 独有 | 双方一致 | 双方分叉 |",
            "|---|---:|---:|---:|---:|",
        ]
    )
    for name, counts in sorted(by_subsystem.items(), key=lambda item: sum(item[1].values()), reverse=True):
        summary_lines.append(
            f"| `{name}` | {counts['niffler_only']} | {counts['aether_only']} | "
            f"{counts['both_changed_same']} | {counts['both_changed_diverged']} |"
        )
    summary_lines.extend(
        [
            "",
            "## 生成附件",
            "",
            "- `generated/path_inventory.tsv`：每个变更路径的双向状态、增删行、最终一致性、子系统和冲突等级。",
            "- `generated/overlap_paths.tsv`：双方都修改过的路径。",
            "- `generated/niffler_commits.tsv`：Niffler 全部独有提交。",
            "- `generated/aether_commits.tsv`：Aether 全部独有提交。",
            "- `generated/*_commit_impacts.tsv`：每个独有提交相对第一父提交的实际文件数、增删行和主要子系统。",
            "- `generated/*_commit_catalog.tsv`、`generated/*_commit_decisions.tsv`：逐提交功能分类和处置建议。",
            "- `generated/*_path_commit_map.tsv`、`generated/path_coverage_ledger.tsv`：逐路径历史来源和最终覆盖状态。",
            "- `generated/*_decision_summary.tsv`：处置标签数量汇总。",
            "- `generated/renames.tsv`：三组比较中 Git 可识别的重命名和复制。",
            "- `generated/subsystem_summary.tsv`、`generated/component_summary.tsv`：聚合统计。",
            "- `generated/metadata.json`：用于复核数量的一致性元数据。",
            "",
            "> 大规模目录迁移时，Git 相似度算法可能把内容相近的旧文件和新文件配成重命名。完整性与数量复核一律以 `--no-renames` 的路径清单为准，`renames.tsv` 只用于人工辅助定位。",
            "",
        ]
    )
    (AUDIT_DIR / "01-generated-inventory-summary.md").write_text(
        "\n".join(summary_lines),
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()

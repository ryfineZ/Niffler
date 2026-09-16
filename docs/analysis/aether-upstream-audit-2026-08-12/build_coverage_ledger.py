#!/usr/bin/env python3
"""Build a path-level coverage ledger joining divergence and commit provenance."""

from __future__ import annotations

import csv
from pathlib import Path


AUDIT_DIR = Path(__file__).resolve().parent
GENERATED_DIR = AUDIT_DIR / "generated"


REPORT_SECTION = {
    "billing_wallet": ("03-niffler-customizations.md#1-商业化计费与钱包", "04-aether-upstream-updates.md#6-p1价格计费相关能力"),
    "provider_protocol": ("03-niffler-customizations.md#3-provideroauth客户端兼容和模型目录", "04-aether-upstream-updates.md#4-p1协议和-provider"),
    "routing_scheduler": ("03-niffler-customizations.md#2-niffler-core", "04-aether-upstream-updates.md#1-p0运行稳定性和性能"),
    "runtime_performance": ("03-niffler-customizations.md#5-用量统计与可观测性", "04-aether-upstream-updates.md#1-p0运行稳定性和性能"),
    "stream_execution": ("03-niffler-customizations.md#4-codexresponses图片与流式执行", "04-aether-upstream-updates.md#2-p0流式故障切换和耗时口径"),
    "usage_observability": ("03-niffler-customizations.md#5-用量统计与可观测性", "04-aether-upstream-updates.md#2-p0流式故障切换和耗时口径"),
    "data_storage": ("03-niffler-customizations.md#1-商业化计费与钱包", "04-aether-upstream-updates.md#3-p0分层架构与数据层"),
    "auth_security": ("03-niffler-customizations.md#3-provideroauth客户端兼容和模型目录", "04-aether-upstream-updates.md#5-p1鉴权安全和隐私"),
    "frontend_product": ("03-niffler-customizations.md#7-前台后台和品牌", "04-aether-upstream-updates.md#7-p1p2后台通知备份隧道与更新"),
    "delivery_operations": ("03-niffler-customizations.md#8-发布生产监控和一次性工具", "04-aether-upstream-updates.md#7-p1p2后台通知备份隧道与更新"),
    "tunnel_frontdoor": ("03-niffler-customizations.md#8-发布生产监控和一次性工具", "04-aether-upstream-updates.md#7-p1p2后台通知备份隧道与更新"),
    "tests": ("00-audit-charter.md#完整性口径", "00-audit-charter.md#完整性口径"),
    "documentation": ("03-niffler-customizations.md#阅读说明", "04-aether-upstream-updates.md#阅读说明"),
    "build_dependencies": ("03-niffler-customizations.md#8-发布生产监控和一次性工具", "04-aether-upstream-updates.md#3-p0分层架构与数据层"),
    "backend_shared": ("03-niffler-customizations.md#总览", "04-aether-upstream-updates.md#3-p0分层架构与数据层"),
    "gateway_other": ("03-niffler-customizations.md#总览", "04-aether-upstream-updates.md#1-p0运行稳定性和性能"),
    "repository_other": ("03-niffler-customizations.md#8-发布生产监控和一次性工具", "04-aether-upstream-updates.md#3-p0分层架构与数据层"),
    "unclassified": ("00-audit-charter.md#子系统分类", "00-audit-charter.md#子系统分类"),
}


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def main() -> None:
    inventory = read_tsv(GENERATED_DIR / "path_inventory.tsv")
    maps = {}
    for branch in ("niffler", "aether"):
        maps[branch] = {
            row["path"]: row
            for row in read_tsv(GENERATED_DIR / f"{branch}_path_commit_map.tsv")
        }

    rows = []
    for item in inventory:
        subsystem = item["primary_subsystem"]
        niffler_section, aether_section = REPORT_SECTION[subsystem]
        niffler = maps["niffler"].get(item["path"], {})
        aether = maps["aether"].get(item["path"], {})
        niffler_provenance = (
            "mapped"
            if niffler and item["niffler_status"]
            else "historical_only"
            if niffler
            else "not_changed_on_branch"
            if not item["niffler_status"]
            else "missing_provenance"
        )
        aether_provenance = (
            "mapped"
            if aether and item["aether_status"]
            else "historical_only"
            if aether
            else "not_changed_on_branch"
            if not item["aether_status"]
            else "missing_provenance"
        )
        rows.append(
            {
                "path": item["path"],
                "primary_subsystem": subsystem,
                "change_scope": item["change_scope"],
                "conflict_risk": item["conflict_risk"],
                "niffler_status": item["niffler_status"],
                "niffler_provenance": niffler_provenance,
                "niffler_feature_clusters": niffler.get("feature_clusters", "base_only_or_merge_only"),
                "niffler_report_section": niffler_section,
                "aether_status": item["aether_status"],
                "aether_provenance": aether_provenance,
                "aether_feature_clusters": aether.get("feature_clusters", "base_only_or_merge_only"),
                "aether_report_section": aether_section,
                "final_equal": item["final_equal"],
                "niffler_to_aether_status": item["niffler_to_aether_status"],
                "coverage_state": (
                    "mapped"
                    if "missing_provenance"
                    not in {niffler_provenance, aether_provenance}
                    else "incomplete"
                ),
            }
        )

    fields = list(rows[0])
    with (GENERATED_DIR / "path_coverage_ledger.tsv").open(
        "w", encoding="utf-8", newline=""
    ) as handle:
        writer = csv.DictWriter(
            handle, fieldnames=fields, delimiter="\t", lineterminator="\n"
        )
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    main()

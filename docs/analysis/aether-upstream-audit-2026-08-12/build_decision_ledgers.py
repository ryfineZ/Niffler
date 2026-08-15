#!/usr/bin/env python3
"""Attach auditable disposition metadata to every classified unique commit."""

from __future__ import annotations

import csv
from collections import Counter
from pathlib import Path


AUDIT_DIR = Path(__file__).resolve().parent
GENERATED_DIR = AUDIT_DIR / "generated"


NIFFLER_DECISIONS = {
    "admin_frontend_experience": ("KEEP_REBASE", "在上游新版页面与分页接口上重放 Niffler 管理体验"),
    "billing_plans_wallet_payments": ("KEEP_REBASE", "保留 Niffler 商业规则；以 8 月请求级准入和原子结算为基线"),
    "branding_foundation": ("KEEP_REBASE", "保留品牌和产品身份；通用底层变更按上游架构重放"),
    "ccswitch_compatibility": ("KEEP", "客户端兼容能力独立保留"),
    "codex_responses_images": ("KEEP_REBASE", "保留产品能力，与上游最终协议状态机做语义合并"),
    "content_moderation": ("DECISION_REQUIRED", "需确认适用范围、失败策略、费用和隐私规则"),
    "data_schema_storage": ("KEEP_REBASE", "保留已上线数据语义，迁入上游分层数据架构"),
    "email_registration_auth": ("KEEP_REBASE", "保留用户能力，吸收上游 worker 治理"),
    "grok_oauth": ("KEEP_REBASE", "保留供应能力，迁入上游 OAuth/Provider 架构"),
    "integration_merge": ("HISTORY_ONLY", "合并提交只作历史索引，不作为可重放补丁"),
    "managed_prompts_groups": ("KEEP_REBASE", "保留分组提示能力，按各协议原生角色重做"),
    "model_catalog_aliases": ("KEEP_REBASE", "保留 Niffler 模型入口，吸收上游在线目录和价格来源"),
    "niffler_core_migration": ("KEEP_REBASE", "保留权威读源和迁移证据；真实预占及误导入口为明确例外，应移除"),
    "platform_bundle_20260626": ("KEEP_REBASE", "混合提交必须按调度、导入和兼容子功能拆分"),
    "platform_bundle_20260803": ("KEEP_REBASE", "混合提交必须按流式容错、价格和生产运维拆分"),
    "provider_oauth_import": ("KEEP_REBASE", "保留外部格式兼容，转换到上游最新凭证契约"),
    "provider_pool_management": ("KEEP_REBASE", "保留运营能力，迁入上游调度、缓存和批量接口"),
    "public_site_image_tools": ("KEEP", "Niffler 产品前台独立保留"),
    "release_deployment_operations": ("KEEP", "Niffler 生产受保护发布和拓扑独立保留"),
    "stream_failover_timing": ("KEEP_REBASE", "保留输出前换号，采用上游终态和端到端计时"),
    "testing_ci_build": ("KEEP_REBASE", "保留业务回归意图，按迁移后文件与接口重写测试"),
    "usage_accounting_observability": ("KEEP_REBASE", "保留业务口径，采用上游诊断、队列和端到端时间"),
}


AETHER_DECISIONS = {
    "admin_backup_notifications": ("DEFER", "与 Niffler Telegram/备份重叠，先统一事件与渠道"),
    "admin_provider_operations": ("ABSORB_AFTER_FOUNDATION", "在分层数据和 Provider 管理接口后吸收"),
    "auth_security_privacy": ("ABSORB_AFTER_FOUNDATION", "IP 限制优先，其余隐私能力在分层后吸收"),
    "billing_wallet_payments": ("ABSORB_SEMANTIC", "只吸收通用价格/支付安全行为，不覆盖 Niffler 商业规则"),
    "claude_protocol": ("ABSORB_AFTER_FOUNDATION", "与 Niffler Claude Code 和提示词能力合并"),
    "data_sql_storage": ("ABSORB_AFTER_FOUNDATION", "作为 workspace/data 分层迁移的一部分"),
    "frontend_ux_i18n": ("ABSORB_AFTER_FOUNDATION", "在 Niffler 产品页上重放，不覆盖公开站点"),
    "gemini_antigravity_protocol": ("ABSORB_AFTER_FOUNDATION", "按协议矩阵合并并保留 Niffler 测试语义"),
    "integration_merge": ("HISTORY_ONLY", "合并提交只作历史索引，不作为可移植补丁"),
    "managed_prompts_groups": ("ABSORB_SEMANTIC", "吸收 developer/system 角色修复并合入 Niffler 分组提示"),
    "model_catalog_pricing": ("ABSORB_SEMANTIC", "吸收在线目录、价格来源和分档事实，不重算历史"),
    "openai_codex_responses": ("ABSORB_AFTER_FOUNDATION", "协议更新依赖新 execution/provider 层"),
    "other_provider_protocols": ("DEFER", "只按实际运营 Provider 选择吸收"),
    "provider_oauth_management": ("ABSORB_AFTER_FOUNDATION", "保留 Niffler 导入格式并使用上游凭证生命周期"),
    "provider_protocol_runtime": ("ABSORB_AFTER_FOUNDATION", "格式转换与传输需要迁到新 Provider 层"),
    "provider_request_runtime": ("ABSORB_AFTER_FOUNDATION", "高价值但覆盖 152 个路径，需先完成基础迁移"),
    "routing_scheduler": ("ABSORB_AFTER_FOUNDATION", "与 Niffler 产品策略账号过滤和套餐供应商约束合并"),
    "runtime_performance": ("ABSORB_AFTER_FOUNDATION", "Redis 连接治理例外为 P0 语义先行，其余依赖新架构"),
    "stream_failover_timing": ("ABSORB_AFTER_FOUNDATION", "端到端计时可先语义移植，完整状态机随 execution 层迁移"),
    "testing_ci_build": ("ABSORB_AFTER_FOUNDATION", "随对应能力和新目录迁移"),
    "tunnel_delivery_operations": ("ABSORB_SEMANTIC", "吸收隧道安全；拒绝绕过 Niffler CI 的在线生产更新"),
    "usage_observability": ("ABSORB_AFTER_FOUNDATION", "端到端时间字段可先移植，其余随 usage runtime 迁移"),
    "workspace_architecture": ("ABSORB_AFTER_FOUNDATION", "所有 7 月后大规模更新的基础迁移阶段"),
}


# Cluster defaults express the normal decision. These overrides record commits
# whose final behavior must be split or explicitly rejected instead of inheriting
# a broad cluster label.
NIFFLER_COMMIT_OVERRIDES = {
    "34c4da43f97c983067f758bc9dbcdf66f0e4574e": (
        "SPLIT",
        "保留错误文案规则和风险事件；移除没有执行器的暂停调度/禁用账号配置入口",
    ),
    "c486efdf8e35e9441929ebfb5cc70c87ab547b4d": (
        "SPLIT",
        "删除请求前真实金额预占行为；保留已执行迁移、历史记录和清理能力",
    ),
}


AETHER_COMMIT_OVERRIDES = {
    "ab0a90de9794a7267b5a589d00760854af6c0fa7": (
        "ABSORB_SEMANTIC",
        "P0：按 Niffler 当前 runtime-state 结构移植固定 Redis 通道和重连治理",
    ),
    "8966fd6aac035ff76245c2da1a9fd0db3173ab96": (
        "ABSORB_SEMANTIC",
        "P0：把数据库压力保护扩展到 Niffler 全部维护 worker",
    ),
    "576918daa599f5fad76cf3062f9bf352c2168fd4": (
        "ABSORB_SEMANTIC",
        "P0：移植热点查询、索引和限写目标，按 Niffler 调度契约重做",
    ),
    "ccfc4cbddc3ee319d88259b50858125a9927f2d4": (
        "ABSORB_SEMANTIC",
        "P0：移植 Provider catalog 缓存并补 Niffler Core 失效事件",
    ),
    "fc92c4f4310f8611fe41f8ad93730a28dd46310c": (
        "DEFER",
        "20k 流整包优化依赖新架构；先完成连接和数据库瓶颈治理，再按压测结果分段吸收",
    ),
    "a0f7074e599d9e9b196d052ea438a9670ff3509f": (
        "REJECT",
        "拒绝照搬默认关闭 Redis 持久化；持久化由 Niffler 生产拓扑和恢复目标决定",
    ),
    "29fa4aed19c594bc4d7dca33d17bdb7a0bf1955e": (
        "REJECT",
        "拒绝直接提高数据库池默认下限；先按实例数和数据库容量压测核算",
    ),
    "a04673a90d0d0ae9d07ab1533d6c6c93e07cb5b7": (
        "SPLIT",
        "端到端首字/总耗时先语义移植；完整故障切换和 payload 状态机在分层后合并",
    ),
    "9562295d8b2b2b9bdfd529173c6076166f7ae3e5": (
        "REJECT",
        "拒绝后台在线更新生产；Niffler 必须继续通过固定 CI 产物和受保护晋级链发布",
    ),
    "b59c3a9e3bb26e35859781df950cdee82c08c7d2": (
        "REJECT",
        "拒绝管理员绕过 Niffler 发布保护直接部署；只吸收版本检查信息",
    ),
}


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def write_branch(
    branch: str,
    decisions: dict[str, tuple[str, str]],
    overrides: dict[str, tuple[str, str]],
) -> None:
    rows = read_tsv(GENERATED_DIR / f"{branch}_commit_catalog.tsv")
    output = []
    for row in rows:
        decision, rationale = overrides.get(
            row["commit"], decisions[row["feature_cluster"]]
        )
        output.append({**row, "recommended_disposition": decision, "decision_rationale": rationale})
    fields = list(output[0])
    with (GENERATED_DIR / f"{branch}_commit_decisions.tsv").open(
        "w", encoding="utf-8", newline=""
    ) as handle:
        writer = csv.DictWriter(
            handle, fieldnames=fields, delimiter="\t", lineterminator="\n"
        )
        writer.writeheader()
        writer.writerows(output)

    counts = Counter(row["recommended_disposition"] for row in output)
    summary = [
        {"branch": branch, "recommended_disposition": decision, "commit_count": count}
        for decision, count in sorted(counts.items())
    ]
    with (GENERATED_DIR / f"{branch}_decision_summary.tsv").open(
        "w", encoding="utf-8", newline=""
    ) as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=["branch", "recommended_disposition", "commit_count"],
            delimiter="\t",
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(summary)


def main() -> None:
    write_branch("niffler", NIFFLER_DECISIONS, NIFFLER_COMMIT_OVERRIDES)
    write_branch("aether", AETHER_DECISIONS, AETHER_COMMIT_OVERRIDES)


if __name__ == "__main__":
    main()

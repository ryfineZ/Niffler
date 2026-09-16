#!/usr/bin/env python3
"""Classify every unique commit into an auditable feature cluster."""

from __future__ import annotations

import csv
import re
from collections import Counter
from pathlib import Path


AUDIT_DIR = Path(__file__).resolve().parent
GENERATED_DIR = AUDIT_DIR / "generated"


NIFFLER_RULES: list[tuple[str, str]] = [
    ("integration_merge", r"^merge |^merge\(|^merge pull|^chore: sync main|同步 main 到 test|^chore\(release\): 整合"),
    ("testing_ci_build", r"^test[:(]|^fix\(ci\)|^chore\(fmt\)|^ci[:(]|strict clippy|actions|依赖告警|生产依赖|静态检查"),
    ("release_deployment_operations", r"deploy|release|发布|部署|生产|test promotion|镜像|docker|迁移预检|workflow|运维|ovh|^fix\(ops\)"),
    ("branding_foundation", r"更名为 niffler|更新仓库与镜像|提交剩余二开|feat\(project\)"),
    ("billing_plans_wallet_payments", r"billing|wallet|payment|payments|dodopay|套餐|计费|扣费|余额|钱包|支付|结算|返利|预占|价格预览"),
    ("niffler_core_migration", r"niffler core|feat\(niffler\)|fix\(niffler\)|test\(niffler\)|chore\(niffler\)|docs\(niffler\)|核心影子|影子|readiness|稳定观察|稳定期|旧逻辑|旧接口|legacy|权威读源|回滚演练|灰度|产品策略|错误文案"),
    ("managed_prompts_groups", r"提示词|managed.*instruction|feat\(groups\)"),
    ("content_moderation", r"content.moderation|内容审查|前置审查"),
    ("email_registration_auth", r"email|邮件|验证|注册|认证管理"),
    ("ccswitch_compatibility", r"ccswitch|cc switch|余额兼容查询|模型目录请求"),
    ("grok_oauth", r"grok"),
    ("codex_responses_images", r"codex|responses|image|图片|生图|gpt-5\.6|配额窗口"),
    ("provider_oauth_import", r"oauth|cpa|sub2api|import|导入|claude code|chatgpt 授权|bearer auth"),
    ("provider_pool_management", r"provider|providers|pool|号池|账号|上游接入|服务能力|模型同步|额度重置|冷却|调度状态"),
    ("usage_accounting_observability", r"usage|用量|统计|记录|错误明细|官方应扣|时间筛选|候选状态|pending settlement"),
    ("model_catalog_aliases", r"models|模型别名|模型目录|模型清单|model.fetch|在线模型"),
    ("public_site_image_tools", r"public|首页|公共站点|生图工作台|infinite canvas|cinematic|navigation|featured tools|favicon|多语言"),
    ("admin_frontend_experience", r"admin|frontend|后台|表格|移动端|侧边栏|管理体验|删除全部筛选|批量删除|占用处理"),
    ("stream_failover_timing", r"stream|流式|容量错误|自动换号|首字节|sse|候选查询取消"),
    ("data_schema_storage", r"^fix\(data\)|^fix\(schema\)|^feat\(data\)|迁移版本|postgresql 空库|并发索引"),
    ("misc_product_fix", r"^fix[:(]|^feat[:(]|^refactor[:(]|^chore[:(]|^docs[:(]"),
]


AETHER_RULES: list[tuple[str, str]] = [
    ("integration_merge", r"^merge |^merge\(|^merge pull|^merge remote|^merge upstream|^merge pr"),
    ("workspace_architecture", r"workspace|layered crate|facade|crate boundaries|split .*crate|extract .*crate"),
    ("testing_ci_build", r"^test[:(]|^fix\(ci\)|^fix\(test\)|^fix rust ci|^ci[:(]|^build[:(]|rustfmt|clippy|nextest|stack overflow|workflow|release image|check regressions|触发 ci|rust 1\.95|测试断言|remove simple query inventory|^revert"),
    ("runtime_performance", r"redis|perf[:(]|performance|hot path|20k|pressure|worker autoscal|worker task|worker registration|db pressure|database hotspot|cache|connection pool|sql pool|runtime admission|queue request|shared memory"),
    ("stream_failover_timing", r"stream|first.?byte|ttfb|sse|terminal|disconnect|heartbeat|watchdog|timeout|failover|fallback|retry|end.to.end|response boundary|sync error|response history|finalize|非流式心跳|响应边界"),
    ("routing_scheduler", r"routing|scheduler|candidate|pool scoring|priority|优先级|首轮候选|interest feedback|visible key state|cooldown settings|admission|concurr|hot pool|circuit breaker|affinity|client session|header sessions|代理池|pool batch|pool bulk|group rate limit"),
    ("openai_codex_responses", r"openai|responses|codex|gpt-5\.6|chat completions|chatgpt|image|compact|search protocol|agent identity"),
    ("claude_protocol", r"claude|anthropic"),
    ("gemini_antigravity_protocol", r"gemini|antigravity"),
    ("other_provider_protocols", r"grok|kiro|windsurf|deepseek|aliyun|doubao|jina|embedding|provider request execution"),
    ("provider_oauth_management", r"provider|oauth|credential|account|quota|transfer limit|ccswitch|endpoint|重置次数|批量处理|批量导入 key|账号批量配置|key configuration|key update"),
    ("model_catalog_pricing", r"model|pricing|price|catalog"),
    ("usage_observability", r"usage|monitor|dashboard|health|timeline|diagnostic|metric|trace|request timing"),
    ("billing_wallet_payments", r"billing|wallet|payment|settle|refund|plan|cost|tier authorization|直连支付|套餐联动|ledger|backfill|余额变更"),
    ("data_sql_storage", r"data|postgres|mysql|sqlite|migration|schema|sql backend"),
    ("auth_security_privacy", r"auth|security|privacy|pii|redact|permission|turnstile|ip restrict|ip whitelist|allowed ip|whitelist handlers|cyber|sensitive|admin group membership|group policy"),
    ("admin_backup_notifications", r"backup|s3|bark|serverchan|server 酱|notification|重要通知|额度提醒|admin operations"),
    ("frontend_ux_i18n", r"frontend|mobile|dialog|sidebar|i18n|display|navigation|layout|badge|theme|admin users pagination|server-side pagination|创建时间排序|暗色模式|选中标签样式|基础交互|表单"),
    ("tunnel_delivery_operations", r"tunnel|deploy|docker|install|update flow|在线更新|frontdoor"),
    ("managed_prompts_groups", r"prompt|提示注入|developer 输入"),
    ("provider_protocol_runtime", r"format|conversion|encoding|accept encoding|api root|transport|本地 sync 错误"),
    ("admin_provider_operations", r"extension modules|admin config|admin user export|user export count|user group options|provider batch|密钥更新模块|inactive endpoint key"),
    ("misc_upstream", r".*"),
]


# Bundled commits whose titles are too broad for reliable rule classification.
MANUAL_OVERRIDES = {
    "fc6b56e1ac9cf8a4de8afb8411f4f9036aa61016": "platform_bundle_20260803",
    "f7d9cbf13c2ae46098633bc2e570a31039df2627": "branding_foundation",
    "d86e6c35597ef661b1da97c0395621533c58eefe": "public_site_image_tools",
    "6c59d922e4d0c06eed9bc499d3f4964d1393ccab": "usage_accounting_observability",
    "8bfbe3b491c6b3048a93a7faaeb572f970d4c117": "public_site_image_tools",
    "437dc25f4c5b572b09d76232c274bb2b343c7e7b": "email_registration_auth",
    "a48e6d676f9180258b861be6c2b682ac27e5fd82": "admin_frontend_experience",
    "e10127474": "platform_bundle_20260626",
    "8616fe6ee2dad6b2e19364f5e8e329ee37125d39": "workspace_architecture",
    "a04673a90d0d0ae9d07ab1533d6c6c93e07cb5b7": "stream_failover_timing",
    "fc92c4f4310f8611fe41f8ad93730a28dd46310c": "runtime_performance",
    "d336d1a7fabf3e3b5e3b97adb3fa0b656a5b586b": "runtime_performance",
    "6f00e9fc67d71cefc861037c56e144bd78ad69f6": "runtime_performance",
    "531cf110250579af2013baccea5bece4765a3442": "provider_request_runtime",
    "f009fb73c3a9fc067b5ff9263c1c20baa7dca292": "runtime_performance",
}


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def classify(row: dict[str, str], branch: str) -> tuple[str, str]:
    if row["is_merge"] == "yes":
        return "integration_merge", "commit_shape"
    commit = row["commit"]
    for key, value in MANUAL_OVERRIDES.items():
        if commit.startswith(key):
            return value, "manual"
    rules = NIFFLER_RULES if branch == "niffler" else AETHER_RULES
    subject = row["subject"].lower()
    for cluster, pattern in rules:
        if re.search(pattern, subject, re.IGNORECASE):
            return cluster, "subject_rule"
    raise RuntimeError(f"unclassified commit: {branch} {commit} {row['subject']}")


def write_catalog(branch: str) -> None:
    source = GENERATED_DIR / f"{branch}_commit_impacts.tsv"
    rows = read_tsv(source)
    output_rows: list[dict[str, str]] = []
    counts: Counter[str] = Counter()
    non_merge_counts: Counter[str] = Counter()
    for row in rows:
        cluster, source_kind = classify(row, branch)
        row = {**row, "feature_cluster": cluster, "classification_source": source_kind}
        output_rows.append(row)
        counts[cluster] += 1
        if row["is_merge"] == "no":
            non_merge_counts[cluster] += 1

    fieldnames = list(output_rows[0])
    with (GENERATED_DIR / f"{branch}_commit_catalog.tsv").open(
        "w", encoding="utf-8", newline=""
    ) as handle:
        writer = csv.DictWriter(
            handle, fieldnames=fieldnames, delimiter="\t", lineterminator="\n"
        )
        writer.writeheader()
        writer.writerows(output_rows)

    summary_rows = []
    for cluster in sorted(counts):
        cluster_rows = [row for row in output_rows if row["feature_cluster"] == cluster]
        summary_rows.append(
            {
                "feature_cluster": cluster,
                "all_commits": counts[cluster],
                "non_merge_commits": non_merge_counts[cluster],
                "changed_files_sum": sum(int(row["changed_files"]) for row in cluster_rows),
                "additions_sum": sum(int(row["additions"]) for row in cluster_rows),
                "deletions_sum": sum(int(row["deletions"]) for row in cluster_rows),
            }
        )
    with (GENERATED_DIR / f"{branch}_cluster_summary.tsv").open(
        "w", encoding="utf-8", newline=""
    ) as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=list(summary_rows[0]),
            delimiter="\t",
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(summary_rows)


def main() -> None:
    write_catalog("niffler")
    write_catalog("aether")


if __name__ == "__main__":
    main()

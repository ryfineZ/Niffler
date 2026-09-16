# Niffler / Aether 全量分叉审计

## 最终判断

Niffler 不是只落后几个月的 Aether，也不是可以通过一次上游合并恢复同步的轻量分叉。固定在 2026-08-12 的比较点，两边从共同祖先后分别积累 357 和 776 个独有提交，最终有 3,309 条路径不同；计费、调度、流式、Provider、数据层和前端都存在双方独立演进。

正确方案是：保留 Niffler 产品和资金语义，先语义移植 Redis、数据库压力和端到端耗时等 P0 修复，再迁入 Aether 的分层 workspace，随后按数据、运行时、协议和界面分批吸收。禁止直接 merge 上游、整目录覆盖或按提交顺序机械 cherry-pick。

## 数量结论

| 指标 | 数量 |
|---|---:|
| 共同祖先 | `ed75ae6d56ab03eb5e6e3cd87f2137880c99694d` |
| Niffler 固定主线 | `908443291a2826b57286f56f1555fd10e922c0b3` |
| Aether 固定主线 | `654c4f69789f02d08e926a77338f1b94f34f8658` |
| Niffler 独有提交 | 357（316 非合并、41 合并） |
| Aether 独有提交 | 776（592 非合并、184 合并） |
| 变更路径并集 | 3,310 |
| 双方都改且最终分叉 | 698 |
| 最终树不同路径 | 3,309 |
| critical/high 路径 | 244 / 1,411 |

## Niffler 二开结论

### 原样或作为产品能力保留

- 请求级计费准入、有限钱包透支根修复、套餐供应商范围、原子结算、支付与欠款合并付款。
- 品牌、公共首页、生图工作台、CCSwitch、受保护 CI 发布、生产监控和双 Frontdoor 方向。
- Niffler 产品策略、上游服务/账号/模型能力、返利账本、外部导入兼容、Grok OAuth、分组提示词、异步邮件和用量业务口径。

其中除少数独立产品页和运维流程外，大部分需要在 Aether 新分层架构上重做，不能照搬现有文件。

### 剔除、还原或收缩

- 删除请求前真实金额预占行为和可编辑开关；历史迁移、表、事件、读取和过期清理保留到 active 数据清零并归档。
- 风险规则暂时只允许 `record_only`；没有执行器的 `pause_scheduling`、`disable_account` 入口隐藏并拒绝保存。
- 删除旧 `allow_wallet_overage` 业务开关、套餐静态模型编辑、按当前价格重算历史账单和用缓存决定资金放行。
- Niffler Core 的 readiness、稳定观察、旧接口投影和回滚证据完成使命后移入高级运维，随后停止新增无用影子记录。
- 内容审查在业务规则未确定前保持关闭，不进入第一批运行时迁移。

完整清单见 `03-niffler-customizations.md` 和 `generated/niffler_commit_decisions.tsv`。

## Aether 更新结论

### 第一优先级吸收

- Redis 固定连接通道和压力工具。
- 数据库池压力下保护后台任务、调度热点 SQL/索引和 Provider catalog 缓存。
- 用户端到端首字/总耗时，同时保留成功尝试首字。
- 随后建立 workspace 分层，迁入 data adapters、runtime、provider transport、usage runtime 等基础边界。

### 分层后吸收

- 调度缓存、candidate page、upstream admission、stage metrics、候选和用量队列。
- 流式终态、HTTP 200 内错误、断连 drain、SSE 控制、watchdog、故障切换和请求诊断。
- OpenAI/Codex/Responses、Claude、Gemini/Antigravity、OAuth 生命周期、Provider 管理、IP 限制、PII 和新版后台。
- 在线价格和 processing tier 只作为价格事实进入 Niffler 准入/结算快照，不覆盖 Niffler 钱包和套餐规则。

### 延后或拒绝

- 20k 流整包优化在 P0 瓶颈和分层完成后按实测分段吸收。
- 未运营 Provider、重复通知渠道和 S3 备份按真实需求选择。
- 拒绝直接照搬 Redis 持久化默认关闭、数据库池默认下限 32、后台在线更新和管理员直接部署生产。

完整清单见 `04-aether-upstream-updates.md` 和 `generated/aether_commit_decisions.tsv`。

## 实施顺序

1. 重新冻结实施时最新 Niffler/Aether 固定点和生产指标。
2. 在当前 Niffler 架构独立移植 Redis、worker 数据库保护和端到端耗时。
3. 只做结构变化地迁入 workspace 分层和兼容 facade。
4. 迁数据契约、仓储、缓存、队列和 worker；资金类保持单权威同步写。
5. 合并调度、流式终态、故障切换和诊断。
6. 按协议逐个合并 OpenAI/Codex、Claude、Gemini、OAuth、Provider 和安全能力。
7. 在新层中重放 Niffler 产品能力，删除已失效行为和入口。
8. 测试环境全量迁移，随后按唯一 Background、单 Frontdoor、双 Frontdoor 的顺序灰度。

详细门禁、验证和回退见 `06-integration-roadmap.md`。

## 文档索引

- `00-audit-charter.md`：范围、证据和处置规则。
- `01-generated-inventory-summary.md`：机器生成数量摘要。
- `02-inventory-verification.md`：数量交叉校验和浅克隆修正。
- `03-niffler-customizations.md`：Niffler 二开能力与处置。
- `04-aether-upstream-updates.md`：Aether 更新与吸收建议。
- `05-local-unpublished-appendix.md`：未进入已发布 Niffler 主线的本地改动。
- `06-integration-roadmap.md`：分阶段实施、数据、验证和回退方案。
- `07-review-record.md`：四轮复核、发现、修正和未决项。
- `08-subsystem-comparison.md`：同一领域中 Niffler、Aether 和最终处置的正面对比。
- `generated/path_inventory.tsv`：3,310 条逐路径差异。
- `generated/path_coverage_ledger.tsv`：逐路径来源和报告归属。
- `generated/niffler_commit_decisions.tsv`：357 条 Niffler 逐提交处置。
- `generated/aether_commit_decisions.tsv`：776 条 Aether 逐提交处置。
- `generated/migration_inventory.tsv`、`generated/migration_comparison.tsv`：双树迁移文件、版本、数据库和内容碰撞清单。
- `validate_audit.py`：完整性和一致性校验器。

## 使用限制

本报告是 2026-08-12 固定点的审计和实施设计，没有修改业务代码、执行迁移或部署。正式实施前必须以届时最新两条主线重新生成清单；当前脏工作区和未发布分支不能作为上游迁移起点。

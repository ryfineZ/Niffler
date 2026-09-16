# 多轮复核记录

## 结论

本次审计执行了四轮不同目标的复核。复核不是重复阅读报告，而是使用独立 Git 集合、源码反向检索、生成器断言和迁移目录比对主动寻找反例。前三轮共发现并修正 6 类问题；最终一致性复核在这些修正后执行。

## Review 1：覆盖率复核

### 检查

- 用固定哈希重新计算共同祖先、双方独有提交和双方路径集合。
- 比较 `commits`、`commit_impacts`、`commit_catalog`、`commit_decisions` 四份账本的提交集合。
- 比较路径清单、逐路径来源和覆盖账本。
- 检查合并/非合并提交、未分类簇、重复提交、重复路径和非法处置标签。

### 发现和修正

1. 本地仓库原为浅克隆，`38aa0849` 被显示成无父提交，影响脚本漏掉该记录，分叉边界外另有 3 条 Niffler 提交未计入。补全 `origin` 历史后，Niffler 数量由误计的 354/313 修正为 357 个独有提交、316 个非合并提交。
2. 路径来源脚本原先跳过合并提交。少量最终变化只通过合并结果进入主线，会缺少来源。脚本改为记录每个提交相对第一父提交的差异；合并提交统一分类为 `integration_merge` 和 `HISTORY_ONLY`，只补来源，不当作功能补丁。
3. “历史改过但最终恢复”原先与“从未改动”共用状态。账本新增 `historical_only`，避免把历史触达误说成最终差异。
4. 新增 `validate_audit.py`，对浅克隆、固定点、共同祖先、提交集合、路径集合、来源、合并提交处置、未分类簇和标签做失败即停止校验。

### 结果

`validate_audit.py` 通过：

```text
PASS audit coverage and consistency
niffler: commits=357 non_merges=316 merges=41 changed_paths=1333
aether: commits=776 non_merges=592 merges=184 changed_paths=2676
path_union=3310 final_different=3309 coverage=3310
```

## Review 2：技术判断反证复核

### 挑战一：金额预占是否真的已经退出

反查固定 Niffler 主线的最终运行时：`NifflerRuntimeRolloutDecision::from_setting` 无条件返回 `enable_billing_reservation = false`；Key 级和产品策略级测试都断言配置无法重新启用，另有测试断言历史 active 预占不减少可用钱包。结论维持：真实预占行为删除，历史表/事件/过期清理暂留；对应提交从大类 `KEEP_REBASE` 改为 `SPLIT`。

同时发现旧 `allow_wallet_overage` 字段仍分布在套餐接口和旧鉴权结构中，说明“最终行为已统一”不等于“旧配置已清除”。路线图因此明确单列删除旧业务开关和静态模型编辑入口，不能只依赖运行时忽略。

### 挑战二：风险动作是否存在隐藏执行器

全仓反向检索 `niffler_account_risk_events`、`pause_scheduling` 和 `disable_account`。除表、仓储、类型校验和管理入口外，没有找到读取事件并修改账号调度状态的消费方。结论维持：保留事件记录和 `record_only`，另外两个动作在实现闭环前隐藏并拒绝保存。

### 挑战三：首字是否只是字段命名不同

Niffler 固定主线只有成功尝试 `first_byte_time_ms`；Aether 固定主线新增端到端字段，并有 `10,120 ms` 端到端首字与 `120 ms` 成功尝试首字同时返回的测试。结论维持：外部默认展示端到端首字，同时保留成功尝试首字用于诊断。

### 挑战四：Redis 修复能否复制单个文件

上游 `ab0a90de9` 同时改动 24 个文件，包含连接路由、KV、锁、流、网关接入、用量运行时和压力工具。Niffler 旧树在 4 个文件中有 31 处直接获取连接调用；Aether 最终实现又从 `aether-runtime-state` 迁到了 `aether-runtime/state`。修正结论：P0 语义移植，不能复制单文件或照搬旧路径。

### 挑战五：性能默认值是否都应吸收

上游 Redis 持久化默认关闭和数据库池下限 32 都依赖部署目标。两项从宽泛的性能吸收簇拆成 `REJECT` 直接默认值；只保留连接治理、可配置、指标和压测方法。

## Review 3：生产和数据风险复核

### 数据库迁移

逐树读取全部迁移 SQL 并按“版本 + 数据库 + 内容哈希”比较：

- Niffler：134 个 SQL，62 个版本；PostgreSQL 62、MySQL 36、SQLite 36。
- Aether：109 个 SQL，58 个版本；PostgreSQL 53、MySQL 27、SQLite 29。
- 共同组合 67 个，64 个相同；`20260403000000` 的三数据库基线内容全部不同。
- Niffler 独有组合 67 个，Aether 独有组合 42 个。

源码确认已应用迁移按版本识别，checksum 不一致只告警。因此“服务启动成功”无法证明生产库、升级库和新空库结构一致。路线图新增真实 schema 目录对账、独立新迁移和三数据库验证，禁止覆盖迁移目录。

### 资金和幂等

逐项检查迁移顺序是否可能双扣、双发或丢失准入凭证。修正后的方案明确：资金准入、最终结算、支付到账和返利发放不能进入丢失可接受的异步队列；资金类迁移只允许单权威写入加旁路对账，不允许双实际写入。

### 后台任务和多节点

双 Frontdoor 与 worker 自动扩缩结合时可能重复执行维护任务。方案将“生产唯一 Background”提升为业务不变量，发布顺序先验证 Background，再逐台发布 Frontdoor；发现重复消费者即自动停止。

### 发布和配置

上游后台在线更新/部署与 Niffler 固定 CI 产物和 test 晋级冲突。两条相关提交单独标为 `REJECT`；版本检查可吸收，执行入口不接生产。

## Review 4：最终一致性复核

### 自动检查

- 顺序重跑 5 个生成器和 1 个校验器。
- Python 全部脚本执行 `py_compile`。
- 文档和脚本执行 `git diff --check`。
- 核对报告数字、决策汇总、路线图固定点和本地未发布附录。

### 人工一致性检查

- Niffler 报告中的“保留”没有把预占、无执行器风险动作或迁移工具永久化。
- Aether 报告中的“吸收”没有覆盖 Niffler 计费、产品策略、支付、返利和发布边界。
- 路线图中的每个 `KEEP_REBASE`/`ABSORB_SEMANTIC` 领域都有前置、验证或回退要求。
- `DEFER` 和 `DECISION_REQUIRED` 没有被暗中放入第一批上线范围。
- 本地未发布改动没有混入 2026-08-12 两条固定主线的数量。

### 最终状态

最终完整生成链和校验通过：

```text
PASS audit coverage and consistency
niffler: commits=357 non_merges=316 merges=41 changed_paths=1333
aether: commits=776 non_merges=592 merges=184 changed_paths=2676
path_union=3310 final_different=3309 coverage=3310
```

逐路径覆盖状态为 3,310 条 `mapped`；Niffler 处置为 43 `KEEP`、268 `KEEP_REBASE`、2 `SPLIT`、3 `DECISION_REQUIRED`、41 `HISTORY_ONLY`；Aether 处置为 49 `ABSORB_SEMANTIC`、508 `ABSORB_AFTER_FOUNDATION`、30 `DEFER`、4 `REJECT`、1 `SPLIT`、184 `HISTORY_ONLY`。

全部审计脚本通过 Python 语法编译，文档和脚本通过 `git diff --check`。本审计和最终方案完成；剩余事项是明确列出的业务/生产决定，不是遗漏的代码审计。

迁移碰撞附件也独立通过：

```text
PASS migration inventory aether_only=42 niffler_only=67 same_content=64 same_id_different_content=3
```

## 剩余未决项

这些事项不是代码审计可以代替的业务决定：

1. 内容审查：适用用户/模型、失败放行还是拒绝、审查费用承担、正文留存和隐私规则。
2. `hub.niffler.org`、`cf.niffler.org`：需真实访问日志、DNS 和客户端配置证明无流量后才能删除。
3. Windsurf、Kiro、DeepSeek、Aliyun 等 Niffler 当前未明确运营的 Provider：是否值得引入 OAuth、刷新、测试和告警维护成本。
4. Redis 持久化：由生产恢复点目标、continuation history 要求和磁盘延迟基线决定，不能由应用默认值替代。
5. 数据库连接池：由实例数、Background、运维连接和数据库上限压测决定，不使用上游 32 作为无条件下限。

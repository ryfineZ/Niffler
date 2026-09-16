# Provider Key 调度状态统一重构

## 目标

统一 provider key 的可调度状态判断，让调度、号池管理、分页筛选、状态标签、请求失败详情使用同一套后端状态来源。

这次重构要解决两个问题：

- 管理页显示账号可用，但真实请求被旧熔断、冷却或号池扫描逻辑跳过。
- 请求还没选中具体账号时，使用记录只显示 provider/key unknown，缺少可读的调度失败原因。

## 非目标

- 不改变模型定价、钱包扣费、套餐额度逻辑。
- 不改变 provider、endpoint、model 的静态匹配规则。
- 不删除历史健康字段本身；迁移期允许继续返回旧字段，但旧字段不能再作为独立调度阻断来源。

## 统一状态

后端统一输出 `scheduling_state`，取值只能是：

| 状态 | 中文标签 | 是否调度 | 说明 |
| --- | --- | --- | --- |
| `disabled` | 已禁用 | 否 | 人工关闭账号 |
| `invalid` | 已失效 | 否 | Token、OAuth 或凭证明确失效 |
| `blocked` | 账号异常 | 否 | 封禁、工作区停用、权限不可恢复 |
| `quota_exhausted` | 额度耗尽 | 否 | 账号额度明确耗尽 |
| `temporary_unavailable` | 暂时不可用 | 否，到期自动恢复 | 429、529、流超时、非硬 403、临时规则命中 |
| `available` | 可用 | 是 | 没有阻断条件 |

状态优先级固定为：

`disabled > invalid > blocked > quota_exhausted > temporary_unavailable > available`

同一个账号同时命中多个状态时，只展示最高优先级状态，同时在 `scheduling_reasons` 中保留其他原因。

## 后端状态载荷

管理接口、调度诊断和使用记录应使用同一份状态载荷：

```json
{
  "scheduling_state": "temporary_unavailable",
  "scheduling_label": "暂时不可用",
  "scheduling_reason": "rate_limited_429",
  "scheduling_reason_label": "上游限流",
  "scheduling_blocking": true,
  "scheduling_until_unix_secs": 1780000000,
  "scheduling_ttl_seconds": 300,
  "scheduling_reasons": [
    {
      "code": "rate_limited_429",
      "label": "上游限流",
      "blocking": true,
      "state": "temporary_unavailable",
      "source": "runtime",
      "until_unix_secs": 1780000000,
      "ttl_seconds": 300,
      "detail": null
    }
  ]
}
```

兼容期可以继续返回旧字段：

- `scheduling_status`
- `scheduling_reason`
- `scheduling_label`
- `circuit_breaker_open`

兼容规则：

- `scheduling_status` 只能由 `scheduling_state` 映射生成。
- `circuit_breaker_open` 只能作为历史健康展示字段，不能再被前端或调度当作主状态。
- 前端不能再根据 `circuit_breaker_open`、健康分、冷却字段自行推断账号主状态。
- `max_probe_interval_minutes` 只能作为历史字段保留，不再作为新账号可配置项展示。

## 错误码映射

| 情况 | 新状态 | 是否继续尝试其他账号 |
| --- | --- | --- |
| 用户请求参数错误 | 不改账号状态 | 否 |
| 模型真实不支持 | 不改账号状态 | 否 |
| 401 凭证明确无效 | `invalid` | 是 |
| 402 额度或支付不可用 | `quota_exhausted` 或 `blocked` | 是 |
| 403 明确封禁、工作区停用、权限不可恢复 | `blocked` | 是 |
| 403 非明确硬错误 | `temporary_unavailable` | 是 |
| 429 | `temporary_unavailable` | 是 |
| 529 | `temporary_unavailable` | 是 |
| 408、409、423、425 | 不改账号状态 | 是 |
| 普通 5xx | 不改账号状态 | 是 |
| 流超时达到阈值 | `temporary_unavailable` | 是 |

普通 `5xx` 表示本次上游服务异常，只记录本次尝试失败并继续尝试其他账号或服务，不写入账号冷却、不打开旧熔断、不改变账号主状态。只有管理员明确配置的临时规则、429、529、非硬 403 或达到阈值的流超时，才允许让账号进入 `temporary_unavailable`。

## 影响范围

### 后端调度

- `crates/aether-scheduler-core/src/candidate/selectability.rs`
- `apps/aether-gateway/src/scheduler/candidate/runtime.rs`
- `apps/aether-gateway/src/dispatch/pool_scheduler.rs`
- `crates/aether-pool-core/src/scheduler.rs`

要求：

- 普通 key 和号池 key 都使用统一状态判断。
- 只有 `available` 可以进入真实调度。
- 号池当前扫描窗口都不可用时，继续扫描后续账号，直到达到明确扫描上限。
- 跳过账号时记录统一状态原因，不再只记录 `key_circuit_open` 或 `pool_cooldown`。

### 后端状态写入

- `apps/aether-gateway/src/orchestration/effects.rs`
- `apps/aether-gateway/src/handlers/admin/provider/pool/runtime/writes.rs`
- `apps/aether-gateway/src/handlers/admin/provider/pool/runtime/reads.rs`

要求：

- 临时失败写入统一临时不可用状态。
- 硬失效写入 OAuth invalid、账号 blocked 或额度耗尽状态。
- 旧 `circuit_breaker_by_format` 只做兼容读取，不再作为主写入目标。
- 新的失败记录不能再打开旧熔断；确需临时跳过账号时写入运行时冷却并带 TTL。

### 管理接口

- `apps/aether-gateway/src/handlers/admin/provider/pool_admin/read_routes/keys.rs`
- `apps/aether-gateway/src/handlers/admin/provider/pool_admin/payloads.rs`
- `apps/aether-gateway/src/handlers/admin/model/routing.rs`
- `apps/aether-gateway/src/handlers/shared/catalog.rs`
- `apps/aether-gateway/src/handlers/public/system_modules_helpers/keys_grouped.rs`

要求：

- 列表、详情、tab 数、分页筛选都按 `scheduling_state` 计算。
- 模型路由和 provider key 详情可以继续显示健康信息，但不能把旧熔断当作主状态。

#### 列表汇总与筛选的一致性

号池账号列表返回的 `summary.statuses`、`summary.plans` 和分页筛选必须使用同一份状态输入。摘要查询可以隐藏密钥、代理和统计等敏感或高成本字段，但不能丢弃参与状态分类的非敏感字段；其中 OAuth 的 `expires_at` 必须保留。对于带 `[REFRESH_FAILED]` 标记的账号，只有访问令牌确实过期时才归入 `invalid`，摘要和完整列表必须得到相同结果。

### 使用记录

- `apps/aether-gateway/src/handlers/proxy/mod.rs`
- `crates/aether-admin/src/observability/usage.rs`
- `crates/aether-ai-serving/src/runtime_miss.rs`
- `apps/aether-gateway/src/ai_serving/planner/candidate_materialization.rs`
- `apps/aether-gateway/src/dispatch/pool_scheduler.rs`

要求：

- 请求还没选中具体 provider/key 时，可以保留 unknown。
- unknown 必须配套展示失败阶段，例如“未进入供应商”或“号池未选出账号”。
- 号池失败详情必须包含扫描账号数、返回账号数、每类跳过原因数量。
- 已经进入上游执行的失败，管理员使用记录必须优先展示上游响应；本地调度失败说明不能覆盖上游 HTTP 状态、错误类型或错误正文。

### 前端

- `frontend/src/views/admin/PoolManagement.vue`
- `frontend/src/features/pool/utils/poolManagementState.ts`
- `frontend/src/api/endpoints/pool.ts`
- `frontend/src/api/endpoints/types/provider.ts`
- `frontend/src/features/models/components/RoutingTab.vue`
- `frontend/src/features/providers/components/ProviderDetailDrawer.vue`
- `frontend/src/features/providers/components/PriorityManagementDialog.vue`

要求：

- 主状态只读 `scheduling_state`。
- 删除前端对 `circuit_breaker_open`、健康分、冷却字段的主状态推断。
- tab、筛选、badge、tooltip、操作按钮使用同一套状态。
- `temporary_unavailable` 显示恢复时间和“清除临时不可用”操作。
- `invalid` 显示“重新授权”或“修复凭证”操作。
- `disabled` 显示“启用”操作。

## 重构计划

| 阶段 | 目标 | 完成标准 |
| --- | --- | --- |
| 1 | 新增统一状态类型和解析函数 | 单元测试覆盖禁用、失效、异常、额度耗尽、冷却、旧熔断、可用 |
| 2 | 管理接口接入统一状态 | 号池列表、summary、分页筛选全部来自 `scheduling_state` |
| 3 | 前端接入统一状态 | 页面不再本地推断主状态，不再过滤熔断状态 |
| 4 | 调度接入统一状态 | 普通 key 和号池 key 跳过逻辑一致 |
| 5 | 号池继续扫描修复 | 前几个账号不可用时能继续尝试后续可用账号 |
| 6 | 使用记录增强 | unknown 记录展示失败阶段和号池扫描摘要 |
| 7 | 旧逻辑清理 | 旧熔断不再阻断调度，不再新写旧熔断，旧字段只作为兼容展示 |
| 8 | 全链路验证 | 后端测试、前端类型检查、请求链路抽样通过 |

## 验证清单

- 账号显示 `available` 时，真实调度不会因旧熔断字段跳过。
- 账号只有历史 `circuit_breaker_by_format.open=true` 时，`scheduling_state` 仍为 `available`。
- 账号被调度跳过时，管理页能看到同样状态和原因。
- Codex 号池前两个账号暂时不可用、第三个账号可用时，请求会继续尝试第三个账号。
- 401、硬 403、429、529、流超时都写入正确状态。
- 普通 5xx 只影响当前请求，不写账号冷却、不打开旧熔断，请求会继续尝试其他账号或服务。
- 号池 tab 数、分页总数、列表 badge、详情 tooltip 和调度行为一致。
- 使用记录中的 unknown 不再裸露展示，详情能解释为什么没有具体 provider/key。

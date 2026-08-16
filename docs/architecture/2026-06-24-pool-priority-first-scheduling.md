# 号池优先级优先调度修正

## 目标

让号池调度里的“优先级优先”真正按账号优先级选择账号，数字越小越优先。

## 非目标

- 不调整全局 Provider/Key 优先级调度。
- 不改变账号健康、额度刷新、成本和延迟等评分计算方式。
- 不迁移已有号池配置数据。

## 行为变化

- `priority_first` 从普通叠加策略调整为账号选择方式，和 `cache_affinity`、`load_balance`、`single_account`、`lru` 互斥。
- 旧配置里如果同时启用了 `cache_affinity` 和 `priority_first`，运行时按 `priority_first` 生效。
- `priority_first` 生效时，不再使用缓存亲和绑定账号，不再让最近使用账号压过手工优先级。

后续两层组合规则见 `2026-08-16-pool-distribution-strategy-composition.md`：`priority_first` 仍是互斥的分配模式；启用叠加策略时，策略先比较账号，人工优先级负责最终裁决。

## 影响范围

影响开启号池调度并启用 `priority_first` 的 Provider。未启用 `priority_first` 的号池继续保持原有缓存亲和、负载均衡、单号优先或 LRU 行为。

## 验证方式

- 后端单测覆盖 `priority_first` 压过缓存亲和和 sticky 绑定。
- 管理端号池调度弹窗单测覆盖 `priority_first` 进入分配模式，不再作为可拖拽叠加策略。

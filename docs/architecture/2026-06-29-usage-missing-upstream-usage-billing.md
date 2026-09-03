# Usage 缺失上游用量时的计费保护

## 目标

当上游请求已经成功并可能产生上游成本，但响应里没有返回可解析的 usage 时，Niffler 不能继续把请求按 0 token、0 费用静默结算。

## 非目标

- 不改变上游请求成功与失败的判定。
- 不保存更多请求或响应原文。
- 不追溯修改历史账单。

## 行为变化

- 对普通文本生成类请求，若终态为成功且没有解析到上游 usage，则使用执行时内存中的请求体估算输入 token。
- 若响应体可见，则同时估算输出 token；若响应体不可见，只估算输入 token。
- 估算产生的用量会参与现有价格规则和钱包结算。
- 审计元数据增加 `usage_estimated_due_to_missing_upstream_usage=true`，便于后续筛选和复盘。
- 没有真实或估算用量的成功文本请求增加 `usage_pending_missing_upstream=true`，数据库记录保持 `billing_status=pending`，由后续补量流程重新结算。
- 若请求体也不可用，则保留“待补用量”状态，不再按 0 token 结算，也不额外保存原文。
- 若计费上下文未生成价格快照且最终成本为 0，则同样保留待结算状态，避免把“未定价”误当成免费。

## 影响范围

- 影响 Claude Messages、OpenAI Chat/Responses、Gemini Generate Content 等普通文本生成请求。
- 图片、嵌入、重排、视频、文件、取消请求、失败请求沿用原有逻辑。
- 原有能解析到上游 usage 的请求不受影响。

## 计费保护影响

- 计费保护只改变缺失用量时的结算状态；不会改变请求响应、路由、成功失败判定或价格计算。
- 已经落成 0 token 的历史记录不自动回填，因为当前未保留足够的原始响应来可靠重算。

## 生产故障根因（2026-09-01）

- **首个卡点是队列中的失效引用。** 2026-08-25 09:48:29 UTC 的一条 usage 事件仍携带
  已删除的 `provider_api_key_id=1769eaff-bdb7-48aa-9a4c-f26a2a44ae8a`。该 key 属于“Pro号池”，
  endpoint 是 `openai:responses`；数据库中已不存在这条 key，但 Redis 仍保留了旧事件。
- PostgreSQL 在 2026-09-01 05:30–06:56 UTC 每隔约 30–40 秒重复报同一个
  `usage_provider_api_key_id_fkey` 外键错误。旧 worker 按批串行处理，遇到这条 poison event 就返回错误，
  没有单事件超时、跳过、死信或自动重启，随后 stale reclaim 又把同一事件交回来，于是后续事件长期积压。
- 所有供应商共用 `usage:events` 消费组，所以一个 Pro 号池的失效事件会让 Pro号池、Kiro(skyhope)、
  Kiro(xiayu) 等多个供应商一起表现为“没有 usage”。这不是多个供应商同时拒绝计费。
- 2026-09-01 06:56 UTC 附近另有短时 PostgreSQL 连接池耗尽，进一步延迟了消费，但它是放大因素，
  不是最初的卡点。Redis stream 的 `MAXLEN ~ 2000` 又裁剪了更早的 pending payload，导致部分历史事件无法从队列恢复。
- 前台请求仍可能先写入 `streaming` 记录；旧 `pending_cleanup` 随后把没有终态 usage 和价格快照的记录直接改成
  `completed + settled`，形成 0 token / $0 的假结算。

修复要求：失效 provider key 必须降级为可审计的空引用并继续写入 usage；单条事件必须有处理超时和死信保留，
  结算失败不能阻断 usage 记录消费，worker 退出后必须自动重启；缺失用量的记录继续保持
  `billing_status=pending`，禁止清理任务或 worker 静默按 0 费用结算。

## 验证方式

- 增加单元测试覆盖 Claude 流式成功但没有 usage、没有保存响应体时，仍按请求体估算输入 token。
- 增加单元测试覆盖嵌入接口成功但没有 usage 时，不套用文本生成估算。
- 增加单元测试覆盖无真实/估算用量的成功文本请求会标记待补用量。
- 增加单元测试覆盖待补用量不会调用钱包结算。
- 运行 `cargo test -p aether-usage-runtime`。
- 运行 usage worker、流式 usage 观察器和 workspace 编译检查，并核对生产 Redis 消费组是否持续推进。

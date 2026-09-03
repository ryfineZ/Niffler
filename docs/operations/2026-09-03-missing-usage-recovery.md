# 2026-09-03 usage 缺失历史账务恢复

## 目标

处理 2026-09-01 07:00 UTC 之后由 usage worker 阻塞造成的成功请求零计费记录。供应商侧无法提供请求级 usage 时，使用 Aether 自有历史成功请求生成可审计的保守估算，并通过现有结算路径完成钱包/套餐结算。

## 非目标

- 不把估算金额伪装成供应商原始账单或精确 token。
- 不修改 Niffler 平台内部不计账请求。
- 不重放 `usage_counter_deltas` 中已经处理的历史事件。
- 不在没有 dry-run 和人工批准的情况下扣款。

## 识别规则

候选记录同时满足：

1. `usage.created_at >= '2026-09-01 07:00:00+00'`；
2. 外部 provider；
3. `status = 'completed'` 且 `status_code = 200`；
4. `total_tokens = 0` 且 `total_cost_usd = 0`；
5. 当前没有金额大于 0 的 `usage_settlement_snapshots`。

## 估算规则 v1

- 基线为故障开始前 31 天内（2026-08-01 00:00 UTC 至 2026-09-01 07:00 UTC）的成功结算请求。
- 按 `(provider_name, canonical_model)` 计算 `actual_total_cost_usd` 的 P50；模型名统一小写并把 `.`、`_` 规范为 `-`。
- 每条候选记录的估算供应商成本为该 P50；估算用户价为供应商成本乘以该请求 `request_metadata.sales_multiplier`。
- 若 provider+model 没有基线，回退到同一 provider 的 P50；仍没有 provider 基线的记录标记 `unrecoverable`，不自动扣款。
- 估算 token 使用同一基线的 input/output P50，仅用于账单展示和统计，不宣称为上游真实 token。
- 估算记录写入 `usage_billing_recovery_cases`，并在 `request_metadata` 保存 estimator 版本、基线窗口、证据级别和金额。

## 结算行为

- 钱包和套餐记录先恢复为 `billing_status = 'pending'`，由现有 settlement retry 使用原始 `billing_request_admissions` 处理钱包、套餐额度和钱包超额。
- 精确快照记录不进入估算批次。
- 每个请求使用唯一 request_id 幂等；恢复批次本身使用唯一 `recovery_batch_id`。
- 估算供应商成本只表示内部估值；供应商实际成本字段不作为对外供应商账单依据。
- pending settlement retry 只领取具备结算证据的终态记录（已有非零成本，或已有 billing/settlement snapshot、免费层标记）；历史零成本且没有快照的旧挂账保留 pending 并进入人工复核队列，不能占住批次头部阻塞可结算记录。
- 每次 retry tick 会连续消费多个 100 条批次，直到当前可结算队列为空或达到单次上限；每批最多 4 路并发结算，同一钱包仍由数据库行锁串行保护。这样历史恢复批次不会因为固定的单批次限制拖延数十小时。

## 验证与回滚

- dry-run 必须输出按 provider、用户、证据级别的记录数和金额，并保存批次摘要。
- apply 后核对：候选数、settled 数、insufficient_quota 数、钱包余额变化、套餐窗口变化和 recovery case 状态。
- 若估算政策被否决，必须在结算前保持 pending；若已结算，只能通过现有退款/调账流程冲正，不直接删除 usage 或钱包记录。

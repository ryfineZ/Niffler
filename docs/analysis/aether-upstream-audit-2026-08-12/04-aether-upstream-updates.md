# Aether 上游更新目录与吸收建议

## 阅读说明

本目录覆盖共同祖先到 `654c4f697` 的 592 个非合并提交。完整提交和路径证据见：

- `generated/aether_commit_catalog.tsv`
- `generated/aether_path_commit_map.tsv`
- `generated/aether_cluster_paths.tsv`

上游更新按“行为价值”和“架构依赖”分别判断。`ABSORB_SEMANTIC` 表示保留行为目标，按 Niffler 最终架构重新实现，不是复制上游最终文件。

## 总览

| 上游能力域 | 更新内容 | 建议 | 前置/冲突 |
|---|---|---|---|
| Redis 连接治理 | 固定 fast/stream/blocking/admin 长连接通道、压力工具和运维手册 | `ABSORB_SEMANTIC`，P0 | Niffler 缺 `redis/runtime.rs`，需适配当前 runtime-state 目录 |
| 后台任务数据库保护 | 数据库池压力下延迟/跳过维护任务 | `ABSORB_SEMANTIC`，P0 | 与 Niffler 邮件、统计、预占清理、配额刷新任务统一 |
| 调度数据库热点 | 索引、批量查询、catalog 缓存、pool score 限写 | `ABSORB_AFTER_FOUNDATION`，P0 | 与 Niffler 产品策略账号过滤及现有迁移冲突 |
| 网关热路径缓存 | 鉴权、API Key、候选、routing group、system config 缓存 | `ABSORB_AFTER_FOUNDATION`，P0 | 必须覆盖 Niffler 产品策略/绑定/账号能力失效事件 |
| 候选与用量异步持久化 | request candidate 队列、批量优先写、worker 自动扩缩 | `ABSORB_AFTER_FOUNDATION`，P0 | Niffler 计费准入必须同步持久化，不能一起异步化 |
| 调度与运行时准入 | candidate page、upstream admission、stage metrics、诊断 | `ABSORB_AFTER_FOUNDATION`，P0 | 保护 Niffler 套餐供应商范围和新路由账号过滤 |
| 20k 流热路径 | 分片/singleflight 缓存、批量生命周期写入、压测工具 | `DEFER` 后分段吸收 | 依赖分层架构；先解决已知连接/DB问题再做容量目标 |
| 端到端计时 | 用户等待总首字、总耗时、尝试时间线 | `ABSORB_SEMANTIC`，P0 | 同时保留成功尝试首字供诊断 |
| 流式终态与断连 | 终态事件、HTTP 200 错误、SSE 控制、disconnect drain、取消 | `ABSORB_AFTER_FOUNDATION`，P0 | 与 Niffler 输出前换号和预读首字深度冲突 |
| 故障切换与诊断 | failover 规则、payload 防护、request diagnostics | `ABSORB_AFTER_FOUNDATION`，P0 | 不能在用户可见字节后换号 |
| workspace 分层 | layered crate boundaries，大规模 data/provider/runtime 拆分 | `ABSORB_AFTER_FOUNDATION`，P0 基础工程 | 影响约 1,643 个历史路径，需单独迁移阶段 |
| portable SQL parity | PostgreSQL/MySQL/SQLite 适配层一致性 | `ABSORB_AFTER_FOUNDATION` | Niffler 有大量自建迁移和仓储实现 |
| API Key IP 限制 | 数据、鉴权、用户/管理 API 和 UI 全链路 | `ABSORB_SEMANTIC`，P1 | 与 Niffler Key/产品策略模型并存 |
| PII 隐私 | 多协议正文脱敏、日志与用量保留 | `ABSORB_AFTER_FOUNDATION`，P1 | 内容审查和原始上游错误留存需统一隐私策略 |
| OpenAI/Codex/Responses | 图片、Compact/V2、Search、工具 ID、continuation history、Agent Identity | `ABSORB_AFTER_FOUNDATION`，P1 | Niffler 同域二开最多，需用协议测试矩阵合并 |
| Claude | thinking、工具结果、system guidance、Claude Code 跨格式 | `ABSORB_AFTER_FOUNDATION`，P1 | Niffler Claude Code 兼容和提示词注入需保留 |
| Gemini/Antigravity | v1internal、Interactions、配额、跨格式、项目元数据 | `ABSORB_AFTER_FOUNDATION`，P1 | Niffler Provider 测试和图片桥接有冲突 |
| Windsurf/Kiro/其他 Provider | 原生 OAuth、缓存语义、DeepSeek、Aliyun embedding 等 | `DEFER`/按业务选用 | 仅吸收实际要运营的 Provider，避免无用维护面 |
| OAuth 管理 | 状态、异步刷新、凭证隔离清理、批量导入、transfer limit | `ABSORB_AFTER_FOUNDATION`，P1 | 保留 CPA/sub2api/嵌套 ChatGPT/Grok 解析器 |
| 在线价格目录 | models.dev 结构化价格、来源、分档价格 | `ABSORB_SEMANTIC`，P1 | 价格同步不得重算历史账单或覆盖 Niffler 销售倍率 |
| tier 授权与结算 | processing tier 权限、价格、用量展示 | `ABSORB_SEMANTIC`，P1 | 接入 Niffler 请求级准入和最终结算，而非上游钱包语义 |
| Provider 管理 | 批量动作、模型导入、状态同步、健康监控 | `ABSORB_AFTER_FOUNDATION`，P1 | 与 Niffler 号池操作和 Niffler Core 双模型合并 |
| 管理运维 | operations dashboard、S3 备份、通知/Bark/Server 酱 | `DEFER`/选择吸收 | Niffler 已有 Telegram/备份，避免重复告警体系 |
| 前端架构与移动端 | i18n 模块化、导航、弹窗、Provider 面板 | `ABSORB_AFTER_FOUNDATION`，P2 | 在 Niffler 产品页上重放，不直接覆盖公开站点 |
| 隧道与更新 | 隧道安全、IP family、稳定协议、在线更新 | `ABSORB_SEMANTIC`，P1/P2 | Niffler 固定 CI 发布政策不接受后台直接生产更新 |
| 默认 DB 池下限 32 | 服务端 pool floor 提高 | `REJECT` 直接默认值 | 先压测 Niffler 数据库容量；只吸收可配置和告警逻辑 |

## 1. P0：运行稳定性和性能

### 1.1 Redis 连接治理

上游 `ab0a90de9` 新增固定长连接通道。运维手册的正常预期是：

- 每个应用实例维持少量固定连接；
- `total_connections_received` 不随请求量线性增长；
- 应用到 Redis 大量 `TIME_WAIT` 表示连接反复新建，是代码回归而不是调大文件描述符能解决的问题。

Niffler 最终树缺少当时引入的 `crates/aether-runtime-state/src/redis/runtime.rs`，并在 4 个旧运行时文件中保留 31 处直接获取 multiplexed connection 的调用。Aether 最终树经 workspace 分层后，该实现已迁到 `crates/aether-runtime/state/src/redis/runtime.rs`，连接路由位于同目录的 `client.rs`。这与已观察到的大量 TIME_WAIT 高度吻合，列为第一批语义吸收项；不能照抄旧提交路径，也不能只复制最终 `runtime.rs` 而遗漏连接路由、调用方和压力测试。

吸收要求：

- 按命令类型区分快速、流、阻塞流和管理通道；
- 重连在通道内部治理，不允许每个请求自行建连接；
- 加入压力测试和 `connected_clients` / `total_connections_received` 发布门禁；
- Redis 持久化策略由 Niffler 生产拓扑单独决定，不能顺带照抄上游默认关闭持久化。

### 1.2 数据库压力与后台 worker

- `8966fd6aa`：数据库池压力下保护维护 worker。
- `576918daa`：调度热点查询、索引、批量操作和 pool score rebuild 限压。
- `ccfc4cbdd`：Provider catalog 缓存。
- `f75894acb`：鉴权、API Key、候选、routing group 和请求候选缓存，配套压力索引和 6k 探针。
- `5b7805181`：请求候选持久化队列。
- `d336d1a7f` / `6f00e9fc6`：candidate page、upstream admission、stage metrics、请求诊断和传输/用量运行时治理。
- `7e9424008`：用量队列 worker 自动扩缩。
- `fc92c4f43`：20k 流场景的分片缓存、singleflight、批量持久化和压力工具。

Niffler 必须做的语义适配：

- `billing_request_admissions` 和首次候选写入属于调用上游前的资金安全凭证，必须同步、同事务成功；不能因为上游把普通候选写异步化而一起排队。
- 产品策略绑定、模型列表、上游服务/账号和账号模型能力变更必须使上游新缓存失效。
- Niffler 路由影子和结算快照可以低优先异步写；返利和最终钱包结算不能降为丢失可接受的后台写。
- 先用 Niffler 实际负载建立 1k/6k/20k 分段基线，不把上游 20k 目标直接当成本轮上线门槛。

### 1.3 数据库连接池默认值

`29fa4aed1` 把默认最大连接数下限从 20 提高到 32。这可能减少应用侧等待，也可能把压力转移到 Postgres。Niffler 使用跨机房数据库和多 Frontdoor，必须按“实例数 × 每实例 pool 上限 + worker/运维连接”核算后配置，不直接吸收默认值。

## 2. P0：流式、故障切换和耗时口径

### 上游连续修复范围

- 要求 OpenAI Responses 流看到协议终态事件；中途失败不能合成正常结束。
- 下游断开后停止继续向客户端写，但在必要时 drain 上游并准确记录取消/失败。
- HTTP 200 内的流错误按失败记录；SSE 控制/keepalive 事件不能算用户可见首字。
- 区分 first-byte watchdog、总耗时和候选超时；保持终态单调，不让刷新把失败覆盖成 active/success。
- 保存请求时间线、候选重试、失败诊断和 payload 防护。
- `a04673a90` 新增端到端首字和端到端总耗时：之前候选的等待与重试也计入用户体验。

### Niffler 合并规则

- 同时保存 `successful_attempt_first_byte_ms` 和 `end_to_end_first_byte_ms`；后台默认展示端到端口径，展开诊断再显示成功尝试口径。
- Niffler 输出前自动换号只能发生在响应尚未提交且未见用户可见事件时。
- 首个上游原始事件不一定是用户可见字节；控制块、keepalive、空事件和被过滤事件不能停止用户首字计时。
- Niffler 的预读流必须复用上游最终的终态/断连状态机，不能再维护独立分支。
- 合并后以“重试前等待 10 秒 + 成功尝试 120 毫秒 = 用户首字约 10.12 秒”的测试锁定口径。

## 3. P0：分层架构与数据层

`8616fe6e` 起把 workspace 划分为分层 crate，后续又把 data contracts、runtime、adapters、provider transport、usage runtime 等迁入新目录。该变化触达约 1,643 个历史路径。

建议单独实施基础迁移：

1. 先建立 Niffler 业务扩展边界，列出计费、Niffler Core、内容审查、邮件、Grok、支付和公开站点的所属层。
2. 移植上游 crate 拆分与兼容 facade，保持 Niffler 旧接口可编译。
3. 逐个移动 Niffler 扩展，禁止在 facade 中继续增加新业务。
4. 完成 PostgreSQL/MySQL/SQLite 契约与迁移生成检查，再吸收 7 月后的功能提交。

直接把上游最终树合到当前 Niffler 会造成大量“文件删除 + 新路径新增”，Git 重命名推断也已证明会误配不相关文件，因此不采用一次性 merge。

## 4. P1：协议和 Provider

### OpenAI/Codex/Responses

吸收：标准字段和 stream events、tool call ID、encrypted reasoning、Search、Compact/V2、continuation history、Agent Identity、动态配额、GPT-5.6 契约、图片 edit/accept 协商、processing tier。

保护：Niffler 的图片原生事件/费用、CCSwitch 目录、模型开关、嵌套授权导入和受管理提示词。每个能力按客户端格式 × 上游格式 × 流/非流 × 工具/图片 × 重试建立测试矩阵。

### Claude/Gemini/Antigravity

- Claude：thinking 清洗、工具结果 JSON、system guidance、context management、Claude Code 跨格式。
- Gemini/Antigravity：v1internal、Interactions、项目元数据、配额精细化、空输出重试、跨格式内置工具。

Niffler 需保留协议原生 system/developer 语义，不能让分组提示词注入破坏上游的角色转换。

### 其他 Provider

Windsurf、Kiro、DeepSeek、Aliyun embedding 等按实际商业需要吸收。未运营的 Provider 不进入第一阶段，避免把 OAuth 刷新、配额、模型测试和协议转换的维护成本全部引入。

## 5. P1：鉴权、安全和隐私

- API Key IP 白名单已覆盖迁移、仓储、鉴权、用户/管理接口和 UI，建议完整吸收。
- PII 脱敏和客户端 IP/管理员控制加固建议在分层架构后吸收。
- 内容审查、原始上游错误、请求正文对象存储和 usage body 的留存规则必须统一；普通用户不能通过新诊断接口看到上游身份或原始敏感内容。
- OAuth 自动删除必须使用上游后期的“凭证隔离清理”规则，不能因普通网络错误或临时 403 删除 Niffler 账号。

## 6. P1：价格、计费相关能力

可吸收行为：在线价格目录、价格来源、缓存 Token 价格、分档价格、processing tier 授权与价格事实。

不可直接吸收实现：上游钱包结算和套餐业务。所有新价格事实必须写入 Niffler 请求级准入/结算快照，历史请求不重算，销售倍率、套餐消耗倍率、Provider 成本和钱包/套餐拆分继续按 Niffler 规则执行。

## 7. P1/P2：后台、通知、备份、隧道与更新

- 服务端分页、Provider 批量操作、健康监控、请求时间线和移动端改进可在新版前端基础上吸收。
- S3 备份、Bark、Server 酱、重要通知与 Niffler Telegram/现有备份功能重叠；先统一告警事件和责任边界，再选择渠道，不同时维护多套相同监控逻辑。
- 隧道安全、IP family 和稳定协议值得吸收。
- 上游后台在线更新与 Niffler 的受保护 CI 发布政策冲突：只吸收版本检查和更新提示，不允许后台绕过固定提交、测试晋级和生产审批直接更新。

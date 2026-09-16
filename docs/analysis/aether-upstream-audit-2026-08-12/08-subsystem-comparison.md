# Niffler 与 Aether 逐子系统正面对比

## 阅读口径

本文件回答“同一领域两边分别是什么、为何不能覆盖、最终怎么处理”。每一条最终路径仍以 `generated/path_inventory.tsv` 为准，每一条提交仍以双方 `commit_decisions.tsv` 为准；本矩阵不把数千个文件名重复排版成正文。

## 1. 仓库架构与依赖边界

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| workspace | 保持分叉初期的旧 crate 布局，大量网关逻辑仍直接依赖旧 data/runtime/provider 模块 | 7 月通过 `8616fe6e` 强制分层，拆出 data contracts/adapters、runtime、provider transport、AI、admin 等层 | Aether 结构更利于继续吸收上游；Niffler 不能直接覆盖，先建 facade 再迁业务 |
| 依赖检查 | 含 Niffler 自建架构断言、计费/Provider 写入约束 | 上游新增更严格 layered crate boundary 检查 | 吸收上游边界，保留 Niffler 的资金和业务约束 |
| 大迁移规模 | 二开散布在旧目录 | 单个分层提交影响 1,633 个文件，整个架构簇触达 1,644 个路径 | 禁止一次 merge；M2 单独做无行为变化的结构迁移 |

## 2. Redis 运行时

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 连接获取 | 4 个旧运行时文件仍有 31 处逐次 `get_multiplexed_async_connection()` | 固定 fast、stream、blocking stream、admin 通道，统一超时、重连和指标 | Aether 行为 P0 吸收，按 Niffler 当前目录语义移植 |
| 阻塞命令隔离 | 旧实现可能频繁取连接，缺少完整通道隔离 | blocking stream 使用独立连接并可配置通道数 | 必须吸收并压测 fast lane P99 |
| 运维指标 | 缺少上游完整连接压力工具和运行手册 | 明确 `connected_clients`、`total_connections_received`、TIME_WAIT 门禁 | 吸收工具和指标 |
| 持久化 | 由 Niffler 现有部署决定 | compose 默认关闭 AOF/RDB，允许按部署恢复 | 拒绝直接照搬默认值；按 continuation history 和恢复目标决定 |

## 3. 数据库连接与后台任务

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| worker 压力 | 有邮件、统计、预占清理、配额刷新、返利等 Niffler 任务，治理方式不统一 | 数据库池压力下延迟/跳过维护任务，完善任务记录和清理 | 语义吸收并覆盖全部 Niffler worker |
| 热点查询 | Niffler 自行修过套餐、号池窗口和日统计压力 | 上游优化 scheduler 索引、批量查询、catalog 缓存和 pool score 限写 | 按实际 SQL 逐项吸收，不能覆盖 Niffler 索引和迁移 |
| 默认连接池 | 当前按 Niffler 配置 | 上游最终默认下限提高到 32 | 拒绝直接默认值，按实例数和数据库容量核算 |
| worker 扩缩 | Niffler Background 有生产唯一实例约束 | 上游用量 worker 可自动扩缩 | 可吸收，但唯一 Background/消费租约必须先证明 |

## 4. 数据层与数据库迁移

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 迁移规模 | 134 个 SQL、62 个版本，含计费、Core、邮件、提示词等二开 | 109 个 SQL、58 个版本，含上游 runtime、协议和数据改造 | 两边都不能丢，按新 Niffler 版本迁入上游 schema 变化 |
| 同版本冲突 | 三数据库 `20260403000000` 基线保留 Niffler 结构 | 同版本基线内容不同且路径已迁到 adapters | 不能替换基线；真实 schema 对账后补增量迁移 |
| 校验语义 | 已应用版本 checksum 不同只告警 | 最终上游也保留 version-only 放行 | 启动成功不证明结构一致，必须做目录和真实库对账 |
| 三数据库 | Niffler 各业务迁移覆盖程度不完全一致 | 上游推进 portable SQL parity 和 adapter 分层 | 吸收生成/一致性检查，保留 Niffler 业务字段和约束 |

## 5. 鉴权、API Key 与权限

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| Key 权限 | 分组、Niffler 产品策略、模型与倍率、客户端兼容 | 修正 group policy 解析、历史身份、角色刷新和缓存 | 在 Niffler 权限交集上吸收，不覆盖产品策略 |
| IP 限制 | 固定主线没有完整 Key IP 白名单链 | 数据、鉴权、用户/管理 API、导入和 UI 全链路 | P1 完整吸收，禁止只存不验 |
| 鉴权缓存 | Niffler 有自己的 Key/钱包/策略读取 | 上游缓存鉴权、API Key、routing group 并修复余额更新失效 | 分层后吸收，补产品策略、钱包和套餐失效事件 |
| Turnstile | 现有注册/鉴权流程 | 最终提交修复输入邮箱时保留 Turnstile 状态 | 小行为随前端鉴权域吸收 |

## 6. 路由、调度与账号池

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 候选范围 | 资金准入供应商范围 + Niffler 服务/账号/模型能力 + 旧 Provider 传输快照 | candidate page、scheduler cache、upstream admission、评分、并发和 stage metrics | 合并成单一顺序，不能让上游评分绕过 Niffler 两层范围 |
| 账号状态 | 统一状态、普通 5xx 不扩大冷却、批量冷却/删除/测试/额度重置 | Provider 批量配置、transfer limit、健康和新调度状态 | 保留运营语义，在上游管理接口上重放 |
| catalog | 旧 Provider catalog 仍是传输权威的一部分 | 上游 catalog 缓存并迁入新层 | 吸收缓存，补 Niffler Core 与旧 Provider 双写失效 |
| 候选记录 | 资金准入与首次候选同事务，另有影子路由尝试 | 普通 request candidate 可排队批量写 | 资金凭证保持同步；普通诊断记录可异步 |

## 7. 流式执行、重试和首字

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 首字 | 主要记录成功尝试或预读流的 `first_byte_time_ms` | 同时保留成功尝试首字与 `end_to_end_first_byte_time_ms` | 用户界面改用端到端，成功尝试字段留作诊断 |
| 总耗时 | 现有请求/候选耗时没有完整覆盖前序失败 | `end_to_end_time_ms` 含排队、失败尝试和换号 | P0 语义吸收 |
| 终态 | Niffler 有自己的 SSE 头、取消修复和 Codex 换号分支 | 连续修复缺终态、HTTP 200 内错误、控制事件、断连 drain、取消和单调状态 | 采用上游最终状态机，接入 Niffler 换号 |
| 换号 | Codex 容量错误在输出前自动换账号 | 上游有通用 failover、payload 防护和诊断 | 只在用户可见输出前允许；输出后禁止重试拼接 |
| watchdog | 旧 Niffler 语义分散 | 区分首字、候选和总超时 | 分层后吸收，保留 Niffler 超时产品配置 |

## 8. OpenAI、Codex、Responses 与图片

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| Niffler 能力 | 字符串/Lite 输入、加密上下文、GPT-5.6 配置、真实配额、生图工具、原生图片事件、图片直通 | Responses 字段/事件、Search、Compact/V2、continuation history、Agent Identity、图片协商、processing tier 持续更新 | 同域冲突最深，按协议矩阵语义合并 |
| 图片计费 | Niffler 修过生图路由、展示和费用 | 上游扩展图片协议和 accept/edit | 保留 Niffler 资金事实，吸收协议契约 |
| continuation | Niffler 没有上游最终跨实例 history 能力 | Redis 保存完成 transcript，带 TTL 和大小限制 | 需要时吸收，同时决定 Redis 重启后的恢复要求 |
| 模型目录 | Niffler 有 CCSwitch、版本自动更新和模型开关 | 上游在线 catalog、外部代理和更多元数据 | 合并事实来源，不自动开放未批准模型 |

## 9. Claude、Gemini、Antigravity 和其他协议

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| Claude | Claude Code 兼容、导入和受管理提示词 | thinking、tool result、system guidance、context management、跨格式 | 保留 Niffler 客户端能力，吸收上游最终转换 |
| Gemini | Provider 测试、默认请求与图片桥接有 Niffler 修复 | v1internal、Interactions、配额、项目元数据、内置工具、空输出重试 | 按 native contents/system instruction 合并 |
| Antigravity | Niffler 主要依赖旧上游能力 | 上游协议更新密集且迁入新 provider 层 | 分层后吸收 |
| Grok | Niffler 独有 OAuth 订阅、PKCE、配额和 reasoning | Aether 另有 Grok/Provider 通用演进 | 保留 Niffler adapter，接入新 OAuth 生命周期 |
| Windsurf/Kiro 等 | 未明确全部运营 | 上游新增 OAuth、缓存和协议支持 | `DEFER`，按实际商业需求启用 |

## 10. OAuth、凭证导入和 Provider 管理

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 外部导入 | CPA、sub2api、嵌套 ChatGPT、覆盖已有账号并保留分组 | 批量导入、状态刷新、凭证隔离清理和新账号模型 | Niffler 解析器转稳定中间结构，再写上游新契约 |
| 内置凭证 | Niffler 已移除内置 Google 客户端凭据 | 上游持续加固 OAuth 配置 | 维持外部配置原则 |
| 自动清理 | Niffler 有自己的冷却/状态语义 | 上游修复凭证隔离清理 | 吸收最终规则，普通网络/临时 403 不删账号 |
| 管理批量操作 | Niffler 支持删除全部筛选、占用检查、批量冷却 | 上游有新分页、批量配置和 Provider 面板 | 服务端分页语义合并，保留占用和审计 |

## 11. 商业化计费、钱包、套餐和支付

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 商业模型 | 分组/策略售价、周期套餐、套餐供应商、钱包、DoDoPay、返利、欠款合并付款 | 通用钱包、官方直连支付/退款、实际成本结算、tier 授权 | Niffler 为权威业务，Aether 只提供价格/支付安全事实 |
| 准入 | 请求级 `billing_request_admissions` 同步持久化 | 上游通用鉴权、缓存和结算 | 不允许上游队列化或估算逻辑替代资金凭证 |
| 透支 | 钱包余额大于 0 可开始，合法最后一批可扣成负；套餐可在欠费时使用 | 上游钱包语义不同 | 保留 Niffler 8 月根修复 |
| 套餐范围 | 选择供应商，模型动态派生；准入/路由/结算使用同一范围 | 上游套餐与 tier 演进不等价 | 保护供应商范围，不恢复静态模型为权威 |
| 历史账单 | 保存实际结算、销售倍率、成本和资金拆分 | 上游新增在线价格、缓存价和分档价格 | 只影响新请求快照，历史不重算 |
| 真实预占 | 已被运行时强制关闭，旧配置/代码/表仍在 | 上游没有 Niffler 这套真实预占 | 删除行为和编辑入口；历史数据先归档 |
| `allow_wallet_overage` | 旧结构仍存在，但最终规则已统一 | 上游可能有不同套餐补差语义 | 删除旧业务开关，兼容读取后停止写入 |

## 12. Niffler Core 与迁移控制

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 产品策略 | 已真实覆盖允许模型、分组信息、售价倍率 | 没有 Niffler 业务模型 | 保留并迁入新层 |
| 服务/账号模型 | 已过滤真实候选，但仍借旧 catalog 传输 | 上游 Provider/scheduler 已重构 | 合并成新调度输入，不能宣称旧模型已下线 |
| 错误返回 | 灰度真实改写用户错误并写风险事件 | 上游有通用错误、failover 和隐私加固 | 保留产品规则，叠加脱敏和诊断权限 |
| 风险动作 | UI/API 可配置 pause/disable，但无执行器 | 上游有自己的冷却/状态机制 | 暂只允许 `record_only` |
| 返利 | 新账本在灰度内实际发钱，旧明细兼容 | 无等价 Niffler 业务 | 保留幂等和双发保护 |
| 迁移工具 | readiness、稳定观察、旧接口投影、回滚证据 | 不存在同类 Niffler 迁移 | 迁移期保留，完成后收进高级运维并逐步停止 |

## 13. 用量、监控和诊断

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 业务口径 | 官方应扣、Key 分组筛选、错误明细、利润、Provider 结算投影、大正文存储 | 请求时间线、stage metrics、diagnostics、worker 指标、终态保护 | 业务金额按 Niffler，运行诊断按上游新架构合并 |
| 状态持久化 | Niffler 多次修正 pending/success/failed 和异步等待测试 | 上游保证终态单调、取消作废、批量/限界持久化 | 吸收状态机和队列，结算状态不可被普通刷新覆盖 |
| 页面耗时 | 当前大多显示成功尝试首字 | 上游 API 已同时返回两种口径 | 默认端到端，详情展示两种和失败时间线 |
| 权限/隐私 | Niffler 错误和正文用于运营 | 上游 PII、请求诊断权限更完整 | 吸收并统一留存规则 |

## 14. 邮件、注册、内容审查和受管理提示词

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 邮件 | 共享 SMTP、异步队列、验证邮件、测试发送和历史 | 通用 worker 压力治理更成熟 | 保留产品能力，吸收 worker 保护 |
| 注册 | Niffler 邮箱验证/密码重置流程 | 上游继续修鉴权/Turnstile | 合并安全修复，不退回同步发送 |
| 内容审查 | 账号前置审查、记录和费用，规则未完全确认 | 无完全等价 Niffler 产品决定 | 保持关闭并列为 `DECISION_REQUIRED` |
| 分组提示词 | 按用户分组选择、记录执行元数据 | 上游修协议 developer/system 角色 | 保留配置，按各协议原生角色合并 |

## 15. 前台、后台和国际化

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 公共站点 | Niffler 品牌、多语言首页、模型目录、生图工作台、Infinite Canvas | 上游主要演进管理产品 | Niffler 产品页保留，不被上游前端覆盖 |
| 管理后台 | 钱包/套餐/号池/Core/审查/邮件等专属页面，修过移动端和表格 | 服务端分页、Provider 面板、导航、弹窗和 i18n 模块化 | 在上游组件基础逐页重放 Niffler 页面 |
| 迁移导航 | Niffler Core 迁移工具仍占显著入口 | 无此历史包袱 | 日常配置与迁移工具分离，后者移入高级运维 |
| 状态体验 | Niffler 部分页面已有加载/错误处理 | 上游持续改进统一交互 | 合并时逐页保留加载、空、错、禁用和权限状态 |

## 16. 隧道、部署、备份和通知

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 发布 | 固定 CI 镜像、外部 PG 预检、test 晋级、生产保护 | 支持后台在线更新和部署 | 保留 Niffler；拒绝在线执行，只吸收版本信息 |
| 隧道 | Niffler 生产拓扑有自己的部署边界 | 上游增加 IP family、安全会话、加密和稳定协议 | 安全/协议语义吸收，配置按 Niffler 拓扑重放 |
| 备份/通知 | Telegram、Postgres 备份和专属监控 | S3、Server 酱、Bark、重要通知 | 先统一事件再选渠道，避免重复告警 |
| 多节点 | 已发布主线之外正在形成双 Frontdoor、唯一 Background | 上游支持更多运行时/worker 能力 | 本地能力单列附录，实施前先进入 Niffler main |

## 17. 测试、CI 和历史合并提交

| 项目 | Niffler | Aether | 判断 |
|---|---|---|---|
| 测试 | 63 个非合并测试/CI 提交，覆盖 Niffler 计费、Core、Codex、Provider 等 | 61 个非合并测试/CI 提交，覆盖新架构和上游能力 | 保留测试意图，按迁移后路径重写；不能只迁实现不迁断言 |
| 合并提交 | 41 个 | 184 个 | 全部 `HISTORY_ONLY`，只用于来源和集成背景，不重放 |
| 补丁等价 | 双向 `git cherry` 无 patch-equivalent 提交 | 同左 | 同名能力必须比较行为，不能假设已吸收 |

## 18. 本地未发布变化

- 美西双 Frontdoor 和首页用户侧 HTTPS 延迟展示建议保留，但不计入固定 Niffler 主线的 357 个提交或 1,333 条路径。
- 删除 `hub`、`cf` 必须有真实流量证据，仍是未决项。
- 当前分支和脏工作区不是上游迁移起点；实施前先通过 Niffler 自己的发布链收敛到最新 `main`。

## 总结

Niffler 应保留的是产品、资金、运营和生产边界；应剔除的是已失效预占、误导风险动作、旧计费开关和永久化迁移脚手架。Aether 应吸收的是分层架构、连接治理、数据库保护、最终流式状态机、协议更新、安全和诊断；应拒绝直接照搬的是部署策略、容量默认值和与 Niffler 商业规则冲突的实现。

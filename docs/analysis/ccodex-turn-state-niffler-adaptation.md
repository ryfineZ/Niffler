# ccodex-sleep-state 核心逻辑与 Niffler 适配方案

日期：2026-09-20。状态：首版已发布；Compact 协议兼容与同出口复用修复已完成本地验证，尚未提交或部署。实际发布版本以各主机 `.niffler-deployed-commit` 和镜像 revision 为准。

## Compact 修复决定与验收（2026-09-20）

目标：普通对话采集并注入 state；Compact 复用同账号、凭据、模型的有效 state 所绑定出口，不发采集探测、不注入缓存 state、不执行普通生成的 state 形状拦截。支持原生 V2 压缩和官方 Codex OAuth 的旧版接口桥接。非目标：扩展模型名单、第三方压缩协议、后台采集、跨账号复用或新增后台面板。

- V2 只识别 `input` 最后一项恰为 `{"type":"compaction_trigger"}` 的协议标记，文本提及和历史 `compaction` 项不能误判。现有顶层字段检查保留，但不能替代实际协议识别。
- 官方 Codex OAuth 的 `/responses/compact`（含 `/v1/` 路径）在首次派发前转换为 `/responses` V2 SSE 请求，保留历史与合法字段，仅补齐流式封装和末尾标记。完整成功后返回 `response.compaction` JSON，并保留可取得的用量；不能伪造压缩密文。API Key、第三方兼容上游继续原协议。
- 旧版桥接属于协议兼容，不依赖 state 开关；出口复用受 `codex_turn_state_enabled` 控制。已有身份收敛仍先执行；桥接派发使用临时 plan，不污染原始客户请求或日志。
- 增加加密出口索引，按上游账号、workspace、凭据指纹、模型隔离，指向最近选用的有效 state 缓存版本和出口。读取时校验有效期、版本和采集时的配置；配置变更或缓存失效后不复用。节点路由重新解析以刷新 Tunnel 所属机器，已停用/不可用的绑定出口明确报错。
- 缺少有效绑定时 Compact 按当前配置转发，不能触发采集。有效绑定是另一应用机器的直连或本机代理时，无法在当前实例保证同出口，返回明确错误；不能把两台机器的直连视为同一出口，也不静默改出口。共享代理/Tunnel 可以跨实例复用。
- Compact 仍遵守账号认证拒绝和限流保护；state 原文从响应/诊断剔除。压缩正文读取有大小和时间上限，支持 gzip、deflate（zlib 封装及 raw 格式）、zstd 解码，以及现有直连、浏览器传输、本地及远程 Tunnel。已经派发后的不完整、错误或超限压缩结果明确失败，不自动换账号/出口重放。
- 诊断标记：`x-niffler-compaction` 为 `v1-to-v2` / `v2` / `failed`；成功压缩的响应中，`x-niffler-compaction-egress: state` 表示采用有效 state 的绑定出口，`configured` 表示按当前配置转发。这些标记不包含 state 原文，也不表示注入了缓存 state。

验收：先复现 V2 误识别，再验证 V1 字段保留/单次发送/完整输出/异常不重放；验证 V1/V2 均不采集和注入、忽略 state 形状、无缓存转发、出口跟随、版本与凭据隔离、配置变更、跨实例共享及直连拒绝。同步/流式公共入口与相关现有测试均需通过。

- [x] 修复 V2 识别和 state 跳过。
- [x] 实现加密出口索引与 Compact 出口复用。
- [x] 实现 V1→V2 请求/响应桥接及有界错误处理。
- [x] 完成回归、格式及静态检查，更新结果。

验证结果：修复前 V2 跳过测试按预期失败；收尾补测也复现了标准 deflate 封装解码失败，两项均已修复。最终 state、Compact、身份收敛、传输、重试及流式执行的 196 项相关测试全部通过；`cargo clippy -p aether-gateway --lib --tests -j 4 -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` 均通过。公共同步和流式入口已验证诊断响应头透传、错误不换号重放，以及 state 原文剔除。

测试使用仓库 CI 既有的 `RUST_MIN_STACK=16777216`；默认测试线程栈曾溢出，`.github/workflows/rust-ci.yml` 与 `docs/architecture/global-capacity-failover.md` 已记录同类要求。新压缩入口的异步 future 使用堆分配，生产线程栈不改。传输测试使用本地模拟上游，测试结束后无残留进程；未调用真实 OAuth 上游，线上双实例效果仍需发布后验收。代码位于 `execution_runtime/codex_compact/` 和 `codex_turn_state/compact_route.rs`；本轮尚未提交或部署。

## 实施进度与首版决定

- [x] 用户确认账号、模型、state 与采集出口绑定。
- [x] 定位同步、流式、Tunnel、OAuth 重试与共享状态接口。
- [x] 补状态校验、租约发布和迟到响应保护测试。
- [x] 实现采集和同出口绑定，接入正式请求。
- [x] 补配置入口、敏感字段清理与不可重放分类。
- [x] 完成直接相关验证并记录结果。
- [x] 修复同步正文异常绕过 state 保护、迟到旧凭据回包覆盖新凭据拒绝记录的问题。

首版采用全局布尔配置 `codex_turn_state_enabled`，默认关闭；开启后无有效 state 时采用 strict，返回明确错误，避免普通转发被误认为已生效。采集使用已选账号当前执行计划中的出口，一轮只探测该出口一次，最多 20 秒，失败冷却 180 秒；不自动扩大该账号允许的出口范围。只保留按需主用状态，不启动备用预热或后台服务。独立随机出口、订阅管理、备用提前采集和 passthrough 配置留待实际需要时扩展。

因此，首版迁移的是原项目默认同出口、按需复用的闭环。一个账号当前只有一个可用出口时，无法替代原工具的多出口搜索；未采到合格值会明确失败。请求使用共享状态和有界采集租约，取消后不留下无限运行的任务，租约超时后不可继续发布。

## 目标、角色与范围

面向 Niffler 维护者和运营人员，把用户已经实测有效的 state 采集、筛选和复用流程接入 Niffler 的 Codex OAuth 上游。普通 Niffler 用户继续使用现有 API 和客户端配置。

本轮目标是总结原项目实际行为，并在 Niffler 中实现默认关闭的同出口适配，覆盖多账号、多 Frontdoor、失效和重试边界。

非目标：搬入桌面启动器、修改用户 Codex 配置、引入完整订阅面板或 Mihomo、重写 Niffler 账号池、改变其他上游协议、执行真实账号探测或发布。

用户已经确认实测有效；本文以复现该效果为目标，不重新把“有没有效果”作为是否研究适配的前置条件。用户随后明确指出要梳理的是“合格 state 与采集出口绑定，正式请求使用该出口并注入 state”的流程，首版据此采用原项目默认同出口方式。尚未取得账号类型、模型及完整配置值，不把其余默认参数写成已确认的实测条件。

## 核查基线与事实来源

- 原项目：[gylive/ccodex-sleep-state](https://github.com/gylive/ccodex-sleep-state)，固定提交 `b18fabf9ad8e9d7af7d9d0306b623ba6091a39d6`，v0.4.0 合并提交。
- Niffler：当前工作区基于 `784ed5d43f54ad90bd58922b0986119ac9b3079a`。已有未提交的运维文档不属于本次改动。
- 原项目核心：[token.go](https://github.com/gylive/ccodex-sleep-state/blob/b18fabf9ad8e9d7af7d9d0306b623ba6091a39d6/internal/turnstate/token.go)、[store.go](https://github.com/gylive/ccodex-sleep-state/blob/b18fabf9ad8e9d7af7d9d0306b623ba6091a39d6/internal/turnstate/store.go)、[engine.go](https://github.com/gylive/ccodex-sleep-state/blob/b18fabf9ad8e9d7af7d9d0306b623ba6091a39d6/internal/gateway/engine.go)、[handler.go](https://github.com/gylive/ccodex-sleep-state/blob/b18fabf9ad8e9d7af7d9d0306b623ba6091a39d6/internal/gateway/handler.go)、[pool.go](https://github.com/gylive/ccodex-sleep-state/blob/b18fabf9ad8e9d7af7d9d0306b623ba6091a39d6/internal/gateway/pool.go)、[account.go](https://github.com/gylive/ccodex-sleep-state/blob/b18fabf9ad8e9d7af7d9d0306b623ba6091a39d6/internal/gateway/account.go)、[settings.go](https://github.com/gylive/ccodex-sleep-state/blob/b18fabf9ad8e9d7af7d9d0306b623ba6091a39d6/internal/settings/settings.go)。
- 对行为差异，以该提交代码为准。例如文档中的“双次异常后晋升”不是 v0.4 正式转发处理的完整描述。

## 原项目真正需要迁移的逻辑

核心是一条闭环：**用实际上游凭据从候选出口采集 state → 按账号规则筛选 → 保存可用快照 → 为正式请求注入 → 根据回包淘汰失效快照**。代理池是采集和出站的支撑，不是 state 状态机本身。

### 1. 隔离与采集

- 只在官方 ChatGPT 通道采集；API Key 和 Responses 中转通道不采集、不注入官方 state。
- 会话键由 `SHA256(Authorization + 分隔符 + ChatGPT-Account-Id) + model` 组成。同账号不同模型的 state 分开；它有意跨请求复用，不是每轮重新获取。
- 探测向实际模型发送 `Reply with OK.`，使用 `/responses`、`stream=true`、`store=false`、`parallel_tool_calls=true`，并请求 `reasoning.encrypted_content`。
- 探测只复制必要认证和客户端头，不携带原请求的 state、会话链或用户正文，也不强行降低模型推理档位。
- HTTP 200 和响应头本身不够。必须在有界读取内收到成功完成的 SSE；`response.failed`、明确的容量/限流错误、缺失完成事件都不能发布候选。
- 同一会话合并并发采集；引擎还有全局串行采集槽。默认每轮最多 6 个出口、单次 20 秒、轮次结束后至少冷却 180 秒。第一次取得合格值即可放行首个正式请求。

### 2. 筛选规则

程序按 Fernet 外形解析 Base64URL：版本字节 `0x80`、时间戳、结构长度以及 `(解码长度 - 57) / 16` 个密文块。没有解密，也没有验证 HMAC。

| 账号规则 | 接纳块数 | 常见编码长度 | 明确拒绝的形状 |
| --- | --- | --- | --- |
| 个人：Free / Plus / Pro | 10 | 292 | 11 块 / 312 |
| Team / Business | 12 | 332 | 13 块 / 356 |

长度只是展示值，实现应按结构和块数判断，不能只写 `len == 292`。默认 TTL 为 3600 秒，距到期 30 秒以内不再接纳；签发时间最多允许超前本机 30 秒。这是项目本地策略，不是从服务端确认出来的有效期。

账号模式支持手动指定；自动模式读取令牌套餐提示，并检查当前选中 workspace 与提示账号是否匹配。不能由 state 长度反推账号类型。

### 3. 主用、备用与失效

- `active` 是当前可用值，`ready` 是备用值。二者都带采集出口；正式请求拿到包含版本号的不可变快照。
- 没有有效主用值时，采集成功直接发布；健康主用值存在时，新值进入备用，不抢占正在执行的请求。
- v0.4 新配置默认 `on_demand`：取得主用值后保持使用，到期或被拒绝后再获取。旧配置缺少字段时保留原来的 `standby` 提前续补行为。
- 普通回包只能提供观测和失效信号，不直接覆盖主用值；发布入口是采集流程。
- **v0.4 的正式生成回包只要出现一次不可接受的非空 state，handler 就会拦截正文、调用 `RejectAndPromote` 淘汰当前版本并尝试晋升备用，返回 `state_shape_changed`。** Store 内部仍有 strikes 逻辑，不能据此把正式链路写成“等两次异常才切换”。
- 回包完全没有 state 时，不据此认定已有快照失效。旧请求的迟到响应不能撤销新版本。
- 完整 state 只保存在进程内存，原项目重启后重新采集。

### 4. 采集出口与正式出口是两个选择

v0.4 支持三种出站方式：

| 模式 | 正式请求出口 |
| --- | --- |
| `state` | 使用该 state 的采集出口；新配置默认值 |
| `fixed` | 使用管理员指定的固定出口，可以与采集出口不同 |
| `random` | 从候选中随机选独立出口，排除采集节点；启用节点池时跳过已用、失败、停用节点 |

节点池状态为 available / used / failed / disabled。探测与正式请求会原子预占节点，避免并发把同一个“一次性节点”取走两次。固定出口允许复用；节点配置不同不证明公网 IP 不同。

因此，描述原项目时需要区分默认同出口与可选跨出口。**Niffler 首版明确只实现同出口绑定：state 快照与采集出口作为整体使用，换出口必须重新取得与新出口绑定的 state。** fixed / random 仅记录为原项目的可选扩展，不列入首版实现范围。

### 5. 转发、降级和停止条件

- 开启注入且有有效值：覆盖出站 `X-Codex-Turn-State`。
- 无有效值：`strict` 返回明确错误；`passthrough` 可以不注入并转发一次。首次 setup 未指定策略时选择 passthrough，但底层空策略按 strict。
- 关闭注入与 passthrough 不同：关闭注入停止采集；passthrough 仍可能先尝试采集。
- 正式请求已经到达上游后，不因 state 异常重放。上游可能已经产生用量，拦截正文不等于没有成本。
- 401/403 阻止该凭据继续请求；429 按 Retry-After 和最小冷却暂停。限制在同凭据的不同模型间共享，换模型或出口不能清除。
- Compact 不等待采集、不套用普通生成的形状拦截；模型列表和搜索也不注入。原项目有独立的压缩兼容代码，不属于首版必须搬入的 state 核心。

## Niffler 首版实现

本节记录首版发布行为；后续 Compact 修复以上方“Compact 修复决定与验收”为准。

### 接入范围与配置

- 全局布尔配置 `codex_turn_state_enabled`，默认 `false`。入口为“系统设置 → Provider 高级”。开关独立于身份收敛和遥测。
- 只匹配 Codex Provider 的 OAuth 账号、HTTPS 官方 ChatGPT Responses 路径，以及 `gpt-6-astra` / `gpt-5.6-sol` / `gpt-5.6-terra`。
- API Key、第三方兼容上游、其他模型、Compact、图片专用请求保持原链路。普通生成工具列表中包含图片工具，不单独作为排除依据；显式选择图片工具的请求排除。
- 只实现同出口、按需主用 state。没有有效值时返回明确错误，不降级成未注入的普通请求；不增加随机出口池、备用预热、后台维护服务或 passthrough 选项。

### 账号与出口绑定

实际执行顺序：调度器选定账号 → 完成模型映射和身份收敛 → 准备 state → 通过既有传输派发 → 观察响应头 → 既有流处理和结算。

缓存作用域由 Provider ID、上游 Key ID、workspace/account ID、Authorization 指纹、最终模型、出口路由和传输配置共同决定。state 不按 Niffler 下游用户 Key 管理。凭据、模型、workspace 或出口变化后，不使用原缓存。

- 探测和正式请求复用同一 `ExecutionPlan` 的代理与传输配置，不重新选择代理。
- 直连、本地回环代理额外绑定 Frontdoor 实例 ID；本地代理有节点 ID 也不视作共享出口。
- 同一远程代理或 Tunnel 节点可以跨 Frontdoor 共享。远程代理 URL 含认证信息时只参与哈希，不进入诊断输出。
- 节点 ID 或代理地址只能绑定路由，不能证明公网 IP 固定。动态出口代理、同一节点更换公网 IP 等场景仍需要真实验收。
- 管理模式覆盖客户端传入的 state，禁止探测携带客户端 state 或用户正文；正式请求禁止跟随重定向。关闭开关恢复原请求行为。

### 采集、共享和取消

状态内核位于 `apps/aether-gateway/src/execution_runtime/codex_turn_state/`，复用现有 HTTP、浏览器指纹传输和 Tunnel，不新增代理内核。

- 每次只探测当前出口一次，探测最多 20 秒、响应最多 1 MiB；整个准备阶段最多 25 秒。收到完整成功 SSE 且 state 结构、块数和时间均合格后才能发布。
- 账号级采集租约合并并发，各模型共享采集冷却；全局最多一个采集者。全局槽忙时明确失败，下一请求可再尝试。
- 开始探测前先保留 200 秒冷却，覆盖 20 秒探测窗口和至少 180 秒后续冷却。进程崩溃、取消也不能立即重复消耗上游额度。
- 采集在当前请求 future 内执行，不派生无限后台任务。取消或总超时会取消网络读取，未释放的租约最多保留 30 秒。
- 共享状态复用 `aether-runtime-state` 后端；生产多 Frontdoor 需要同一 Redis 命名空间和加密密钥，内存后端只适用于单实例。
- Redis 中保存应用侧加密的 token 和随机版本，TTL 不超过 state 剩余有效期减 30 秒。Redis 可能持久化，因此不宣称“state 永不落盘”。
- 新增原子接口 `kv_set_if_lock_owned` 和 `kv_delete_if_value`：过期采集者不能发布，旧响应不能删除后来替换的版本。固定 30 秒租约覆盖 20 秒采集，发布仍校验所有者；首版不依赖后台续租。
- 发布前再次检查开关与账号拒绝状态。系统配置沿用现有约 3 秒缓存，关闭不是瞬时中断所有已派发请求。

### 正式请求、失效与重试

覆盖三个实际入口：流式 `execute_in_process_stream`、同步候选快路径 `execute_direct_sync_runtime_candidate`、同步/OAuth 重试入口 `execute_sync_plan_with_report_context`。内部探测直接使用传输层，不递归经过 state 管理入口。

- 同步和流式均在取得原始上游响应头后、读取正文前检查 state。同步直连、浏览器传输、远程 Relay、本地 Tunnel 和 OAuth 重试入口遵守相同时序；无效 state 立即返回不可重放错误，不等待正文结束，正文截断、挂起或无效 JSON 不能绕过检查。随后移除 `x-codex-turn-state`，不把账号池 state 交给下游客户端；成功注入的响应标记 `x-niffler-turn-state: injected`。
- 上游成功响应返回不合格的非空 state 时，撤销本次使用的缓存版本，返回 `codex_turn_state_shape_changed`；缺失 state 不单独判失效，合格的新回包也不直接替换主用值。
- state 管理错误在同步与流式失败分类中优先停止候选换号；正式请求发出后发现 state 异常，不自动重放。流式错误使用现有帧协议传递 JSON 错误和 EOF。
- 401/403 的 state 模块保护以“账号＋凭据指纹”分别保存认证状态，最长保存 24 小时。旧凭据的迟到回包只更新自己的记录，不能覆盖新凭据的拒绝；新凭据不继承旧凭据的认证错误。现有 OAuth 刷新流程仍负责更新凭据，每次实际派发重新准备 state。
- 429 单独保存账号级冷却截止时间，跨模型、出口和凭据刷新共享，按 Retry-After 与至少 180 秒冷却保存（异常大值限制为一年，避免时间溢出）；并发更新保留最晚截止时间，认证记录与限流记录不会相互清除。功能尚未部署，直接采用拆分后的缓存结构，不增加旧结构迁移。
- 共享状态读取或发布失败明确报错，不退回各 Frontdoor 独立采集。

### 用量与敏感信息

探测使用单独请求 ID，不进入客户请求候选、钱包预占和客户用量流水。探测仍会消耗上游额度：保存平台维护用途、状态码、是否接纳和可取得的数字用量摘要；缺失 usage 为未知。日志记录各次摘要，Redis 只保留最近一次摘要 24 小时，不是完整财务账本。

正式请求已发送后，state 校验或共享状态处理失败会在错误中标记 `upstream_request_sent=true`、`upstream_usage_unknown=true`，并停止重放。失败请求沿用现有失败结算；首版不继续排空被拦截的流来追索 usage，也不声称已精确统计这部分上游成本。

注入只发生在临时 plan 中，不把管理 state 写回原 plan 或通用 report_context；响应 state 在报告前剔除。请求/响应头持久化对 `x-codex-turn-state` 完整替换为 `[redacted]`，不保留前后缀。缓存键和维护日志不包含原始 Authorization、state 或用户文本。

## 验证方式与结果

本地自动化验证使用合成 Fernet 外形、模拟探测和独立临时 Redis，不调用真实模型。

本轮修复验收：先复现新凭据被拒绝后旧回包覆盖其保护，以及无效 state 响应头后正文异常绕过检查；修复后验证无效 state 不读取正文、不继续换号，新旧凭据独立，429 在并发和凭据刷新后仍有效。影响范围仅为 state 管理和同步传输观察时序，不改变配置入口或其他上游的错误重试规则。

- 网关：结构、时间和套餐规则；完整 SSE；并发合并；加密缓存；模型/凭据/workspace/出口隔离；跨 Frontdoor 共享；冷却与限流；关闭时禁止发布；迟到响应保护；同步/流式停止换号；错误帧传递。
- Runtime：内存和真实 Redis 的租约所有者发布、租约过期、释放后禁止发布、按值删除。
- 管理与用量：默认关闭、布尔配置校验、完整敏感头脱敏。
- 前端：开关交互、禁用状态、类型检查；沿用原有组件布局。

2026-09-20 首版验证结果（修复前）：

| 检查 | 结果 |
| --- | --- |
| 网关 `codex_turn_state` 测试 | 20 通过 |
| Runtime `fenced_kv` 测试（含两个独立 Redis 客户端） | 4 通过 |
| 管理配置与用量敏感头测试 | 2 通过 |
| Provider 高级设置组件测试 | 5 通过 |
| 前端类型检查、Rust 改动文件格式检查、`git diff --check` | 通过 |
| 前端定向 ESLint | 0 error；页面和既有测试有存量格式/非空断言 warning |

合计 31 项直接相关测试通过。临时 Redis 使用五位数端口，验证后已停止并确认无残留。没有运行真实账号验收、全仓库测试或生产发布。

2026-09-20 review 修复验证：

- 修改实现前，新增的 5 个回归用例均按预期失败：旧凭据迟到覆盖新凭据拒绝、并发凭据拒绝互相覆盖，以及同步正文截断、挂起、无效 JSON 绕过响应头检查。
- 修复后 `cargo test -p aether-gateway --lib -j 4 -- codex_turn_state execution_runtime::transport::tests` 共 54 项通过，其中 state 模块 29 项、既有传输测试 25 项。新增共 9 项，覆盖上述回归、并发 429 保留最长冷却、Reqwest / 浏览器 / 本地 Tunnel 在正文挂起时及时终止；同步主入口返回终止响应，不再返回可换号的空结果。远程 Relay 和 OAuth 重试使用的共享同步入口也已覆盖。
- 非测试构建 `cargo check -p aether-gateway --lib -j 4`、改动 Rust 文件格式检查及 `git diff --check` 通过。本轮未改前端或 Runtime 原子接口，没有重复运行与本轮无关的验证。
- 新增故障服务在测试 future 内运行，无独立后台任务，连接和监听器随测试完成、失败或超时释放；HTTP 测试使用五位数端口。测试进程已退出，无临时运行项残留。

## 剩余边界与上线验收

实现和 review 阶段没有修改生产配置、发起真实 OAuth 模型请求或发布。实际启用前需要使用测试账号、固定出口和受控请求预算，验证实际采集合格率、HTTP/Tunnel 传输、OAuth 刷新、失败结算及真实注入效果。只有 `injected` 响应可以计入注入效果对照。

### 本次生产发布范围

用户已授权提交并部署两台应用服务器。生产发布直接从准确的 `main` 提交构建镜像；两台主机使用同一镜像，通过各自固定部署器检查迁移兼容性、容器健康和公开入口；失败沿用固定部署器自动回退。

- hd0526：更新 `frontdoor` 和唯一的 `background`。
- OVH VPS（`ovh-US-WEST-OR-VPS-4`）：更新 `frontdoor`。
- 发布后核对两台提交号和镜像 revision、两轮容器/健康接口检查，以及前端包含开关文案。开关保持默认关闭，发布不自动采集或消耗真实账号额度；本次不涉及数据库迁移。

单出口未采到合格 state 会明确失败；首版不能替代原工具的多出口搜索。动态 IP、账号套餐提示缺失或上游 token 形状变更会影响可用性，默认关闭便于逐步验证。暂不增加运行状态面板、备用 state、账号套餐手动覆盖或后台续补。

原项目采用 GPL-3.0，当前 workspace 声明 `LicenseRef-Aether-NonCommercial`。本次根据行为分析独立实现 Rust 状态管理，没有引入原项目组件或复制其实现代码。

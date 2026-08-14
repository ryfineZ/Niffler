# Codex OAuth 身份收敛

## 目标

同一个 Codex OAuth 账号被多个 Niffler 用户共同使用时，统一该账号发往 Codex Responses、Responses Compact 和图片生成桥接请求的设备和会话标识。上游应看到一个账号对应一套稳定设备、一个稳定会话，以及多个可并行的任务，而不是每个 Niffler 用户各自形成一套稳定身份。

管理员只在“系统设置 → Provider 高级设置”操作一个全局开关。开关开启后，全部现有及以后新增的 Codex OAuth 账号统一启用会话收敛；关闭后保持现有请求行为。

## 非目标

- 不为单个账号、账号批量操作或单个 Provider 增加覆盖配置。
- 不让不同 Codex OAuth 账号共用同一个设备或会话标识。
- 不处理 ChatGPT Web 或 WebSocket 请求。
- 不伪造 `x-oai-attestation` 设备证明。
- 不保证上游无法从网络出口、并发量、请求内容或使用习惯判断账号被共享。

## 配置语义

系统配置键为 `codex_oauth_identity_convergence_enabled`，值必须是布尔值，默认 `false`。

- `false`：不改写 Codex OAuth 请求中的设备、会话、任务和回合身份。Codex OAuth Compact 仍按官方接口要求使用普通 JSON 响应，这属于请求协议，不受身份收敛开关控制。
- `true`：全部 Codex OAuth Responses、Responses Compact 和图片生成桥接请求使用会话收敛。

旧账号不需要新增配置、重新授权或重新保存。开关开启后，系统直接根据现有 Provider Key ID 为每个账号生成稳定身份。

## 行为变化

### 稳定范围

每个 Codex OAuth Provider Key 独立生成：

- `installation_id`：优先使用从 sub2api 导入并保存的合法 `openai_device_id`，否则根据 Provider Key ID 确定性生成。
- `session_id`：根据 Provider Key ID 确定性生成。
- `thread_id`：根据 Provider Key ID 和客户端原始任务信号确定性生成。同一账号的同一任务保持不变，不同任务通常得到不同值。
- `turn_id`：优先读取客户端当前逻辑回合的 `turn_id` 并在当前 OAuth 账号内确定性映射；同一逻辑回合的 Responses、Compact、图片和工具后续请求保持一致。客户端没有提供时，为本次进入 Niffler 的请求生成 UUIDv7；该请求更换账号或重试时不重新生成。
- `window_id`：使用收敛后的 `thread_id` 加客户端合法的窗口序号；缺失或非法时使用 `0`。

客户端传入的父任务和分支来源任务 ID 保留关系语义，但不直接透传。系统使用“Provider Key ID + 原始父任务 ID”执行与普通任务相同的确定性映射，确保父子任务处于同一账号命名空间。

任务信号依次读取：`thread-id` 请求头、`client_metadata.thread_id`、`session-id`、`session_id`、Niffler 已解析的任务亲和标识。Compact 和对话内图片请求由客户端携带当前 Responses 的任务信号，因此映射为同一个 `thread_id`。全部任务信号都缺失时，为本次进入 Niffler 的独立请求生成内部任务编号；同一请求的账号切换和重试继续复用，不同独立请求不会因为使用同一个 Niffler API Key 而合并。

缓存身份使用收敛后的任务级 `thread_id`。同一任务的普通 Responses、Compact 和图片请求共用缓存键，不同并行任务使用不同缓存键；这与官方 Codex 的任务缓存语义一致。

### 共同出站规则

开启后，适用请求在所有格式转换、正文规则、请求头规则和受管理提示词处理完成后统一改写。三个入口共同执行以下规则：

- 请求头：`x-codex-installation-id`、`session-id`、`thread-id`、`x-codex-window-id`。普通 Responses 和图片请求另外发送收敛后的 `x-client-request-id`；Compact 不发送该字段。
- 已有且合法的 `x-codex-turn-metadata` 保留非身份字段，只替换身份字段和本次回合开始时间。
- 请求头中的回合元数据统一序列化为纯 ASCII JSON；原有中文等 Unicode 内容使用 JSON 转义保留，避免生成非法 HTTP 请求头。
- 删除出站 `conversation_id`、旧式 `session_id` 请求头和客户端提供的 `x-oai-attestation`。
- `chatgpt-account-id` 只使用当前 OAuth 凭证中的账号 ID。
- `User-Agent`、`originator`、`version` 使用 Niffler 已验证的同一个 Codex 客户端版本组合，不继续透传各个 Niffler 用户自报的版本和运行环境。
- 删除客户端传入的 `x-stainless-*`、`sec-ch-*`、`sec-fetch-*`、`x-aether-tls-*`、`Origin`、`Referer`、`Accept-Language`、`DNT`、`Sec-GPC`、`X-Requested-With`、`X-App`、`X-Client-Name`、`X-Client-Version`、`OpenAI-Client-User-Agent` 和 `X-OpenAI-Client-User-Agent`。这些字段只描述发起 HTTP 请求的 SDK、浏览器、应用、语言、TLS、操作系统和处理器架构，可能与统一的 Linux Codex CLI 身份冲突。
- 上述删除只发生在请求已选中 Codex OAuth 账号且全局开关开启之后，不影响客户端识别、路由和权限判断；开关关闭时全部保持现状。
- 不删除请求正文、工具定义或 `x-codex-turn-metadata` 中的工作区、沙箱、任务类型等信息；模型仍根据这些任务上下文和实际工具结果判断项目运行环境。
- `x-codex-parent-thread-id`、回合元数据中的 `parent_thread_id` 和 `forked_from_thread_id` 使用同一账号线程映射规则改写；非身份元数据保留。
- 身份相关请求头默认按敏感字段处理，日志和管理界面不保存或显示明文值。

任何请求头或正文规则都不能在身份收敛之后再次覆盖这些字段。

### 普通 Responses

- 请求正文的 `prompt_cache_key` 改为收敛后的 `thread_id`。
- 请求正文的 `client_metadata` 统一写入安装、会话、任务、回合和窗口字段；原有非身份字段继续保留。
- `x-client-request-id` 改为收敛后的 `thread_id`。

### Responses Compact

- 使用与同一任务普通 Responses 相同的安装、会话、任务和窗口标识。
- 请求正文的 `prompt_cache_key` 改为收敛后的 `thread_id`，保证同一任务压缩前后继续使用相同缓存键，同时避免不同并行任务共用缓存键。
- 删除客户端传入的 `x-client-request-id`，并且不生成替代值；官方 Codex Compact 请求不发送该字段。
- 不向 Compact 正文增加 `client_metadata`。客户端传入的 `client_metadata` 会被删除，因为官方 Compact 请求正文不包含该字段，身份只通过请求头传递。
- 压缩历史 `input` 及其他合法 Compact 字段保持不变。
- 发往官方 Codex OAuth `/responses/compact` 的请求固定使用普通 JSON 响应，不发送 `Accept: text/event-stream`。官方客户端直接接收这个完整 JSON；第三方客户端如果明确要求流式接收，Niffler 再完整接收 Compact JSON，为 `output` 中每个压缩结果生成一条 `response.output_item.done`，最后生成 `response.completed`。完整响应及解压后的 JSON 均不得超过 64 MiB，超限时停止读取并返回明确错误，避免异常响应耗尽网关内存。该规则不改变其他兼容 Provider 的 Compact 行为。

### 图片生成

Niffler 的 Codex `openai:image` 请求会转换为带 `image_generation` 工具的 Responses 请求，并发送到 Codex `/v1/responses`。因此图片生成执行与普通 Responses 相同的请求头和正文收敛，包括 `prompt_cache_key`、`client_metadata` 和 `x-client-request-id`。

### 设备证明

普通 Responses、Responses Compact 和图片生成都在完成身份改写后删除客户端的 `x-oai-attestation`。Niffler 不生成替代证明；这避免将原客户端设备生成的证明与 Niffler 改写后的设备、会话和任务标识混合发送。删除只影响可选的设备证明，不改变 OAuth 访问令牌、账号 ID 或请求内容。

### sub2api 导入

sub2api 账号导入只额外读取合法的 `extra.openai_device_id`，保存到 `fingerprint.codex.installation_id`，用于保持该账号原有安装身份。源文件中的账号级 `codex_fingerprint_mode` 不进入 Niffler，因为 Niffler 只使用系统级统一开关。

## 影响范围

- 网关：读取全局开关，保存请求级身份上下文，在最终请求阶段改写 Codex OAuth Responses、Responses Compact 和图片生成桥接请求。
- 管理端模型测试：使用真实 Codex OAuth 账号发送 Responses、Responses Compact 或图片测试请求时，也执行同一套身份收敛。
- 管理接口：为新配置键提供默认值和布尔值校验。
- 管理界面：系统设置新增“Provider 高级设置”页签和统一开关。
- OAuth 导入：白名单迁移 sub2api 的安装 ID。
- 数据库：复用现有 `system_configs` 与 Provider Key `fingerprint` JSON，不新增字段或迁移。

## 失败行为

- 配置缺失按关闭处理；配置值不是布尔值时拒绝保存，运行时读取到损坏值则明确报错。
- 只有已确定将请求发送到 Codex OAuth Responses 账号时才读取并校验该配置；配置存储故障或损坏值不影响其他 Provider。
- 缺少 Provider Key ID 时不发送未收敛的请求，直接返回本地错误。
- 导入的安装 ID 非字符串、为空、过长或包含控制字符时忽略，并使用 Provider Key ID 生成。
- 损坏的回合元数据不继续透传；删除该载体，其他收敛字段仍正常发送。
- 需要完整接收后再转换的上游响应超过 64 MiB 时停止读取并返回明确错误；压缩响应解压后也执行同一限制。

## 验证方式

- 同一 OAuth 账号由不同 Niffler 用户调用时，安装 ID 和会话 ID 相同。
- 同一任务的任务 ID 稳定，不同任务的任务 ID 不同。
- 不同 OAuth 账号生成不同的安装、会话和任务 ID。
- 客户端提供回合 ID 时，同一逻辑回合的多个 HTTP 请求和账号重试使用同一个映射后回合 ID；没有提供时，同一次请求的账号重试共用生成的回合 ID，下一次独立请求使用新值。
- 请求头、正文和内嵌回合元数据中的身份字段一致，窗口序号得到保留。
- Compact 与同一任务的普通 Responses 使用相同安装、会话、任务、窗口和缓存标识，不发送 `x-client-request-id`，正文不新增 `client_metadata`；官方 Codex 上游固定返回普通 JSON。
- 第三方客户端要求将 Compact 完整 JSON 转换为流式响应时，正常响应完整保留；原始响应或解压后的 JSON 超过 64 MiB 时停止处理并返回明确错误。
- 图片生成桥接请求使用与普通 Responses 相同的完整收敛规则。
- 没有父任务标识的两个独立请求生成不同 `thread_id`，即使它们来自同一个 Niffler API Key。
- 三类请求均不向上游发送客户端的 `x-oai-attestation`。
- 不同用户传入的 `User-Agent`、`originator` 和 `version` 均被同一套 Niffler Codex 客户端身份替换。
- 客户端传入的 SDK、浏览器、应用、语言、操作系统和架构请求头均被删除，不会与 Linux Codex CLI 的 `User-Agent` 冲突；正文、工具、工作区元数据和不属于环境身份的自定义请求头保持不变。
- 父任务、子任务和 fork 来源任务的线程 ID 全部使用同一账号命名空间，关系指向保持一致。
- 管理端“测试模型”与普通请求的账号级设备、会话和客户端身份一致。
- Codex 收敛配置存储故障或值损坏时，非 Codex 请求仍正常进入自己的规划和执行链路。
- 开关关闭时不执行身份字段改写；Codex OAuth Compact 仍固定使用普通 JSON 上游响应。非 OAuth、非 Codex 和 ChatGPT Web 请求保持现状。
- 现有账号在开关开启后直接生效，新账号自动生效。
- sub2api 导入能够迁移合法安装 ID，且不会导入任意 `extra` 字段。
- 管理界面覆盖加载、未修改、保存中、保存成功和保存失败状态。

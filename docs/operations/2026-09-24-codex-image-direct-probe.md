# Codex 图片直调与 Responses 桥接实测

## 目标与范围

针对请求 `b49cfb16-aca4-4b84-9df8-a1b755b05356`，验证 Pro OAuth 账号能否直接调用 `gpt-image-2`，据此决定修复方案。本次只读生产配置，并直接向上游发送最小图片测试；没有修改生产配置、部署或客户请求。

本次不是网关端到端验收，不证明全部账号、图片编辑、多图和全部图片参数兼容。不保存凭证、客户提示词或图片 Base64。

## 环境与根因

- 时间：2026-09-24，约 12:00–12:06（UTC+8）。
- 生产节点：OVH 独服；Frontdoor 镜像提交与本地 HEAD 均为 `f49bfa0131c199482c7a665ff9ce60b7932a1b77`。
- 使用失败请求最后一个候选的同一 OAuth 账号与同一 HTTP 代理。认证信息只在远端进程内存中解密。
- `Pro号池` 类型为 `codex`，未配置 `codex_image_generation_base_model`，因此使用代码默认值 `gpt-5.4-mini`。
- 图片端点上游基础地址为 `https://chatgpt.com/backend-api/codex`，没有配置图片直通模式。
- 该请求尝试了三个账号，均记录上游 HTTP 400，最终网关记录为 503。
- 该账号实时模型目录包含 `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`、`gpt-5.5` 等，不含 `gpt-5.4-mini`。目录中也没有 `gpt-image-2`，但原生图片接口实测可以调用；不能用文本模型目录缺项判断图片接口不可用。

## 实测结果

| 测试 | 结果 | 耗时 |
| --- | --- | --- |
| Codex `/responses`，主模型 `gpt-5.4-mini` | HTTP 400，复现“不支持通过 ChatGPT 账号调用该模型”的原始错误 | 0.58 秒 |
| Codex `/images/generations`，模型 `gpt-image-2`，首次探测 | HTTP 200，收到 `image_generation.completed`；首次仅检查事件类型 | 17.95 秒 |
| 同一原生图片接口，完整数据验证 | HTTP 200，804,242 字节 PNG；CRC、末尾 IEND 和像素数据解压通过 | 20.83 秒 |
| 同一原生图片接口，显式 `stream:false`、`size:1536x1024` | 仍返回 SSE；828,672 字节 PNG，校验通过；事件和 PNG 实际尺寸均为 `1254x1254` | 20.21 秒 |
| 公共 `api.openai.com/v1/images/generations`，使用同一 OAuth 凭证 | HTTP 401，缺少 `api.model.images.request` 权限 | 0.55 秒 |
| Codex `/responses`，主模型 `gpt-5.6-sol`，工具模型 `gpt-image-2` | HTTP 200；非空 PNG 图片项与 `response.completed`，图片项状态 completed | 17.95 秒 |
| 同一 Sol 桥接，工具显式 `size:1536x1024` | HTTP 200，833,012 字节 PNG 与完成事件；实际和报告尺寸也为 `1254x1254` | 23.38 秒 |

Sol 最初一次探测因上游没有 Content-Type，被探测器误归为普通正文并提前关闭；这次不算成功验收。随后修正探测器，以 SSE 正文解析，完整读取两次图片与响应终态。

直调显式同步测试用量为 14 输入 Token、515 图片输出 Token。Sol 桥接测试的顶层文本用量分别为 2,361 和 2,359 Token；统计口径不同，不能直接比较总费用或据此推算节省比例。

## 方案决定

1. 针对本次失败的 `/v1/images/generations`，优先采用 Codex 原生 `/images/generations` 直连适配，顶层模型保持 `gpt-image-2`，去掉额外桥接主模型依赖。
2. 不能只切换现有 `images_passthrough`：必须正确处理 Codex 路径（不能多拼 `/v1`）、账号认证头，以及上游即使 `stream:false` 也返回 SSE 的行为；继续提供客户端要求的同步 JSON 或流式响应，并验证错误传播、用量和完成判断。
3. 普通 Responses 图片工具调用仍遵循 Responses 协议。图片编辑、多图等尚未实测，不在本次直调结论中泛化承诺。
4. 若只做紧急恢复，显式配置桥接主模型为 `gpt-5.6-sol` 已有同账号成功证据；这不会消除额外主模型依赖。
5. 尺寸未按请求执行在原生直调和 Sol 桥接中都复现，属于另一个待查问题；不能把它归因于直调，也不能声称切换主模型能修复。精确尺寸兼容性须单独验收。

## 后续实现验收

- 先用真实脱敏事件结构编写针对性回归，验证路径、请求模型、认证头、SSE 转同步 JSON、流式完成和上游错误。
- 验证用量只计一次，图片二进制不被计作文本 Token。
- 在隔离或受控测试中通过网关发起图片请求，确认不再出现 `gpt-5.4-mini`，取得非空可解码图片并检查最终记录。
- 上线前明确尺寸行为；扩大到 edits 或多图之前补做各自接口验证。

以上为初次调查与方案决定。随后按用户要求完成以下代码修复；尚未上线。

## 修复与验收

- Codex 的 `/v1/images/generations` 默认直连 `/backend-api/codex/images/generations`；保留图片模型和请求参数，上游按流式执行。图片编辑、普通 Responses 和其他供应商不切换。
- 补齐原生 `image_generation.completed` 的同步 JSON 聚合、流式输出和图片用量；断流、空图片和失败事件不能被记作成功。内部用量正文不复制图片 Base64。
- 保留 OAuth 刷新和账号请求头；身份收敛继续作用于请求头，原生图片正文不再注入 Responses 专用字段。
- 管理端正确显示 Codex 原生默认模式；手动选择 Responses 桥接会明确保存覆盖配置，选回原生时删除覆盖。
- 回归测试先复现了原路径仍为 `/responses`、原生完成事件无法转换为同步图片结果的失败，再验证修复。

验证结果：

- `cargo test -p aether-ai-formats --lib -- --quiet`：392 项通过。
- `RUST_MIN_STACK=33554432 cargo test -p aether-gateway image --lib -- --quiet --test-threads=4`：153 项通过，真实账号验收用例默认忽略。默认测试线程栈曾溢出，提高测试线程栈后全部通过，无需修改运行时代码。
- 网关 `codex_identity_convergence` 筛选：20 项通过。
- 前端端点路径和流式失败切换测试：8 项通过；修改组件 ESLint 检查通过。
- `vue-tsc -p tsconfig.app.json --noEmit`：20 个错误，全部位于未修改文件。在独立临时目录中恢复该组件修改前版本后，复现完全相同的 20 个错误。本次不修复这些既有问题。
- 真实隔离网关验收：`live_codex_native_image_generation` 通过，本地内存仓库承载测试鉴权、供应商和请求记录，实际调用同一生产 OAuth 账号的原生图片接口。返回同步 Images JSON，包含 809,425 字节 PNG（Base64 解码和 PNG 签名检查通过），用时 18.12 秒；用量为输入 15、图片输出 515、总计 530 Token。
- 本机直连账号代理曾被重置，导致真实验收失败；通过 SSH 本地转发沿生产服务器访问同一代理后通过。未改变生产代理设置，临时转发与测试进程均已关闭。

真实用例需要显式提供环境变量 `NIFFLER_TEST_CODEX_TOKEN`、`NIFFLER_TEST_CODEX_ACCOUNT_ID`、`NIFFLER_TEST_CODEX_PROXY`，再运行忽略用例；凭证不得放在命令参数或日志中。该用例会实际生成一张图片，不能作为默认 CI 测试。

剩余边界：本次尚未发布到生产；精确尺寸偏差仍是已确认的独立上游行为，未通过缩放图片等方式掩盖，也未扩大修改到未经实测的编辑接口。

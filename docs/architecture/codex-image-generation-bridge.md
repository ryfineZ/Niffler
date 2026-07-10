# Codex 图片生成桥接

## 目标

让通过 Codex / ChatGPT OAuth 账号转发的 OpenAI Responses 请求，在用户要求生成或编辑图片时，可以使用 OpenAI Responses 原生 `image_generation` 工具，避免模型误以为当前客户端没有图片生成能力。

## 非目标

- 不改变本机 Codex、CLI 或浏览器工具栏能力。
- 不把普通请求强制改成图片生成请求。
- 不改变用户分组、套餐、钱包和模型价格规则。
- 不改变 OpenAI Responses Compact 的请求能力。
- 不把 `gpt-5.4-mini` 解释成图片模型。
- 不改 Gemini 协议和 Gemini 图片转换逻辑。

## 行为变化

- 判断位置在路由选中具体账号和端点之后，不在入口层根据用户文字猜测。
- 普通对话请求最终走 Codex / ChatGPT OAuth 的 OpenAI Responses 端点时，系统会在上游请求中补充 `image_generation` 工具。
- 第三方 OpenAI 兼容端点默认不补充图片工具。只有管理员在 Provider 或 Endpoint 配置 `openai_responses_image_generation_tool_enabled: true` 后，普通对话才补充 `image_generation` 工具。
- 如果请求没有设置 `tool_choice`，补充图片工具时会设置为 `auto`；如果请求已经设置了 `tool_choice`，不会覆盖用户原有选择。
- 系统会在上游 `instructions` 中补一句说明：即使本地客户端没有 `image_gen` 命名空间，也可以使用 Responses 原生 `image_generation` 工具。
- 明确图片请求统一按 CPA / Sub2API 的桥接方式处理：顶层 `model` 使用 Responses 主模型，图片模型放到 `tools[].model`，并强制 `tool_choice` 为 `image_generation`。
- 明确图片请求包括：`openai:image` 路径、顶层模型为 `gpt-image-*`、或 `tool_choice` 明确选择 `image_generation`。
- 如果请求只是普通工具请求，且 `tool_choice` 是 `auto` 或未设置，不会强制改成图片生成请求。
- 整理图片工具参数时，只读取真正的 `type=image_generation` 工具，不会把普通函数工具误改成图片工具。
- 图片桥接请求拆成两个模型角色：
  - 顶层 `model` 是 Responses 主模型，用来承载对话和调用 `image_generation` 工具，默认 `gpt-5.4-mini`。
  - `tools[].model` 是图片工具模型，用来生成图片和计费，默认 `gpt-image-2`。
- Codex 提供商可以通过 `provider.config.codex_image_generation_base_model` 指定桥接主模型；为空或非法时使用默认 `gpt-5.4-mini`。
- Chat/Responses 请求转到第三方 `openai:image` 端点时，也会生成标准 Responses 图片工具请求体，包含 `tools[].type=image_generation`、`tools[].model` 和 `tool_choice`。
- 第三方 API 如果只接入 `openai:image` 端点，需要配置对应图片模型或模型映射，才参与明确图片请求调度。

## 影响范围

- 影响最终上游端点为 `openai:responses` 的 Codex / ChatGPT OAuth 请求。
- 影响显式开启 `openai_responses_image_generation_tool_enabled` 的第三方 OpenAI 兼容 Responses 端点。
- 不影响 `openai:responses:compact`。
- `openai:image` 仍走已有图片接口转换逻辑。
- 第三方 `openai:image` 上游会收到真正的图片工具，而不是只有 `input` 和 `model` 的普通 Responses 请求。
- 请求记录中仍保留用户原始请求，上游请求记录会体现系统补充后的工具和说明。
- 使用记录和计费继续按用户请求的图片模型记录，例如 `gpt-image-2`；桥接主模型只作为上游执行细节保存。

## 验证方式

- 单元测试覆盖 Codex Responses 普通请求自动补充图片工具和说明。
- 单元测试覆盖第三方 OpenAI 兼容 Responses 默认不补充图片工具。
- 单元测试覆盖第三方 OpenAI 兼容 Responses 显式开启后补充图片工具。
- 单元测试覆盖明确选择图片工具时不会丢失工具和 `tool_choice`。
- 单元测试覆盖顶层模型为 `gpt-image-*` 时，顶层模型改为桥接主模型，图片模型进入 `tools[].model`。
- 单元测试覆盖普通工具列表里同时存在图片工具时不会误改成图片请求。
- 单元测试覆盖图片工具参数整理不会复制普通函数工具的 `description`、`parameters` 等字段。
- 单元测试覆盖自定义桥接主模型时，顶层 `model` 使用自定义值，`tools[].model` 仍保留图片工具模型。
- 单元测试覆盖 Chat/Responses 转第三方 `openai:image` 时会注入图片工具和 `tool_choice`。

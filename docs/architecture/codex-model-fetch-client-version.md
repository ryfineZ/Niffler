# Codex 上游模型获取版本

## 目标

确保 Codex 号池自动获取模型时，只使用 ChatGPT Codex 上游真实返回的模型列表。

## 非目标

不在本地补充、猜测或预置 GPT-5.6 等模型名称，也不把本地模型目录伪装成上游获取结果。

## 行为变化

Codex 模型目录请求继续调用 `https://chatgpt.com/backend-api/codex/models`，并带上上游要求的 `client_version` 参数。生产验证显示旧版本 `0.128.0-alpha.1` 和 `0.139.0` 只返回旧模型，`0.144.1` 会按账号权限返回 GPT-5.6 系列模型，因此默认请求版本更新为 `0.144.1`。

如果上游没有返回某个模型，Niffler 不会额外补充该模型。

## 影响范围

只影响 `provider_type=codex` 且接口格式属于 OpenAI Responses 家族的模型自动获取和手动获取。

## 验证方式

- 使用生产 Codex OAuth 账号只读验证 `/backend-api/codex/models?client_version=0.144.1` 返回真实模型列表。
- 运行模型获取相关单元测试，确认请求地址和解析行为符合预期。

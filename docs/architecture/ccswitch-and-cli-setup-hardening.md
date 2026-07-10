# CC Switch 与一键配置修复记录

## 目标

让用户创建 API Key 后，可以稳定完成两类配置：

- 导入 CC Switch：生成正确的服务地址、余额检查地址和可选模型口径。
- 一键配置 CLI：生成可信的公开 API 地址，并减少 Windows/macOS/Linux 脚本兼容问题。

## 非目标

- 不重做 CC Switch 的完整配置导出。
- 不改变网关实际转发、扣费、套餐结算规则。
- 不替用户自动选择具体模型；没有填写模型时仍按用户总可用额度展示。

## 行为变化

- CC Switch 导入会单独传入余额检查地址，避免 Codex 端点是 `/v1` 时把余额接口拼成 `/v1/v1/usage`。
- 后端同时接受 `/user/balance` 和 `/v1/user/balance`，兼容客户端刷新用量时继续使用 OpenAI v1 服务地址作为 `baseUrl` 的情况。
- CC Switch 导入如果填写了模型，余额检查会把模型传给 Niffler，Niffler 按该模型查询套餐额度。
- Codex 导入不再传非标准的 TOML `config` 参数，只传 CC Switch 源码确认会读取的顶层 `model`、`endpoint` 和 `apiKey`；未填写模型时默认使用 `gpt-5.5`，CC Switch 生成的 Codex 配置会写入 `model_reasoning_effort = "high"`。
- 前端优先向后端获取公开 API 地址，不再只用浏览器当前域名推断。
- 一键配置生成公开地址时，公网默认使用 HTTPS；本机地址仍允许 HTTP。
- Windows 一键配置脚本避免依赖 PowerShell 7 专属的 `ConvertFrom-Json -AsHashtable`。

## 影响范围

- 用户 API Key 页面的一键配置、导入 CC Switch。
- `/v1/usage`、`/user/balance` 和 `/v1/user/balance` 余额检查接口。
- `/api/users/me/public-base-url` 用户侧公开 API 地址接口。

## 验证方式

- 前端单测覆盖 CC Switch 深链接参数。
- 后端路由测试覆盖公开 API 地址接口。
- 后端脚本单测覆盖 PowerShell 兼容写法。
- 手动检查生成的 Codex CC Switch 链接中 `endpoint` 为 `/v1`，`usageBaseUrl` 为根地址，且不包含非标准 TOML `config` 参数。

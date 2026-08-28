# Grok OAuth 订阅账号适配说明

## 目标

在当前主线架构中接入通过 xAI Grok CLI OAuth 获得的订阅账号，并保留原功能分支中已经
验证过的 OAuth 生命周期、请求转换、额度刷新、账号自检和管理后台能力。

本次只迁移以下三个业务提交的有效行为，不合并原分支夹带的上游历史：

- `9087b7133ae17d40464945bb918f68dc2ff2b790`
- `4ad96eae0013073e4eb1915964e7cbe78fd1f463`
- `384f685ca64d134626209c7b82abbd710b2102b5`

## 非目标

- 不改变现有 `grok` 浏览器 Cookie / SSO 提供商行为。
- 不为 `grok_oauth` 增加图片、视频、SSO Cookie 转换或媒体资格检查。
- 不将原 Grok OAuth 分支整体合并到主线。
- 不改变其他 OAuth 提供商的刷新、失效和额度语义。

## 行为变化

`grok_oauth` 使用 `https://cli-chat-proxy.grok.com/v1`，凭据为 xAI OAuth refresh token
与 access token；它与现有提供商的区别如下：

| 类型 | 上游 | 凭据 |
| --- | --- | --- |
| `grok` | `https://grok.com` | 浏览器 Cookie / SSO |
| `grok_oauth` | `https://cli-chat-proxy.grok.com/v1` | xAI OAuth refresh token 与 access token |
| `custom` | 由管理员配置 | xAI API Key 或其他兼容 API Key |

适配完成后支持：

- 管理后台浏览器 OAuth 绑定；
- refresh token 单个导入、批量导入与自动刷新；
- OpenAI Responses 文本与流式请求；
- 通过 Aether 格式转换层兼容 Chat 请求；
- 读取 xAI CLI weekly/monthly Billing 额度，并写入结构化额度快照；
- `grok-4.5` 支持 `low`、`medium`、`high` 思考程度；未指定时使用上游默认值
  `high`，并在使用记录中保存最终实际值。附加该元数据时必须保留原请求元数据，
  不能影响候选、路由、正文引用、平台代处理和内容审查等既有记录字段及计费状态判断。

上游固定使用 Responses，不为 `grok_oauth` 直连创建 Chat Completions 端点。

Grok OAuth 默认每 30 分钟以并发 1 执行一次账号自检，从 `/v1/billing?format=credits`
和 `/v1/billing` 更新额度。管理员可以在号池高级设置中调整间隔与并发，或关闭账号
自检。管理页显示额度快照的绝对更新时间；超过 60 分钟未更新时标记为“额度数据已
过期”，该时间与 OAuth Token 续期倒计时无关。

任意 403 都可能来自 xAI 风控、地区、客户端身份或暂态限制，不能据此将账号标记为永久
失效。系统保留账号并记录脱敏诊断；401 仍按 OAuth 刷新和失效流程处理。

## 影响范围

- 网关 OAuth 生命周期、Provider 调度、请求格式转换和账号自检；
- 管理接口的额度数据；
- 管理后台 Provider 表单、OAuth 导入、号池高级设置、额度展示和使用记录；
- PostgreSQL 与内存实现的 Provider 候选查询。

OAuth 凭据继续按现有 OAuth key 机制加密保存。日志、截图、工单和测试输出不得包含
access token、refresh token 或完整 callback URL。

## 验证方式

1. 运行 Grok OAuth 相关 Rust 单元测试、网关集成测试和数据层三种数据库测试。
2. 运行前端相关单元测试、类型检查和生产构建。
3. 运行工作区格式检查、全目标编译检查和现有发布检查。
4. PR 的全部必需检查通过后，只以普通合并提交进入 `main`。
5. 构建该 `main` 精确提交的镜像，部署后连续检查健康、重启次数、公开接口与严重日志。

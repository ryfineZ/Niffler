# 本地未发布 Niffler 改动附录

## 范围

本附录记录审计时工作区中没有进入 `origin/main` 的内容。它们不参与已发布 Niffler 与 Aether 的主比较，也不能作为上游合并起点。

## 当前未发布能力

### 美西双 Frontdoor 与用户侧延迟显示

- 新增 `us1.niffler.org`（OVH）和 `us2.niffler.org`（hd0526）两条 Cloudflare 入口。
- 两个入口的 `/__niffler_latency` 返回禁止缓存的跨域 `204`，首页各采样三次并显示中位数。
- `api.niffler.org` 计划灰云直连 hd0526，根域名经 Cloudflare 回源 hd0526；Background 只在 hd0526 运行。
- 首页新增加载、成功、失败和手动重测状态，并明确数值是完整 HTTPS 请求耗时，不是模型首字或源站纯网络延迟。

建议：`KEEP`，属于 Niffler 生产拓扑和公开站点能力。合并上游前必须先把该改动重新基于最新 `origin/main`，并按文档验证 DNS、TLS、CORS、唯一 Background 和单次数据库迁移。

### 计划删除的旧入口

- 文档计划移除 `hub.niffler.org` 和 `cf.niffler.org`。
- 这是外部可见的破坏性变更，实施前必须从访问日志、DNS、客户端配置和监控确认没有真实流量；不能仅凭 Caddy 配置判断已停用。

建议：`DECISION_REQUIRED`，在流量证据确认前不随上游合并执行。

### 工作区中其他未归属文件

- `niffler-wechat-avatar.png` 为未跟踪图片，当前没有代码引用证据；不纳入功能合并。
- 根目录 `task_plan.md`、`findings.md`、`progress.md` 含既有用户工作和本次审计追加内容，不属于运行代码。

## 分支状态约束

审计启动时当前分支是 `codex/fix-nested-chatgpt-auth-import`，固定发布比较点仍是更新后的 `origin/main`。开始任何上游迁移前，应创建新的受保护迁移分支并以 `origin/main` 为基线；不能直接在当前脏工作区或较旧提交上合并上游。

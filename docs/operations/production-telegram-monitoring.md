# 生产服务器 Telegram 监控

## 目标

使用现有 Telegram Bot 主动通知以下生产异常和恢复：

- `rn01` 根分区空间不足。
- `rn01` 的 PostgreSQL 或 Redis 容器异常。
- `hd0526` 根分区空间不足。
- `hd0526` 的 frontdoor 或 background 容器异常。
- Niffler 公开健康接口连续失败。
- DMIT 大陆入口的系统盘、Caddy、WireGuard 或回源链路异常。
- DMIT 当前计费周期流量达到 3.5 TB 或 4.25 TB。
- 使用容易理解的中文说明异常影响和处理建议，不向日常通知堆放容器健康字段、
  字节数、校验值或对象路径。
- 允许授权管理员通过 Bot 查询状态并调整监控阈值。

## 非目标

- 监控不会自动重启容器、删除文件或修改数据库。
- 监控不替代 Cloudflare、主机商或独立外部探测平台。
- 本次不监控业务错误率、模型上游质量或用户请求延迟。
- Bot 不能执行任意服务器命令，也不能修改应用或数据库配置。

## 监控分配

数据库角色监控通过 `MONITOR_POSTGRES_PORT` 指定容器内 PostgreSQL 端口，默认值为
`5432`。非默认端口节点必须显式配置；当前 ColoCrossing 主库使用 `55432`，
rn-hybrid 从库使用 `5432`。

监控脚本由 systemd 启动时不能依赖交互式 shell 的 locale。变量后紧接中文标点或
中文正文时必须使用 `${变量名}` 明确边界，避免不同 Bash/locale 组合把中文字符误
识别为变量名的一部分。

`rn01` 本机检查：

- `/` 磁盘使用率。
- `niffler-postgres` 容器运行和健康状态。
- `niffler-redis` 容器运行和健康状态。

`hd0526` 本机检查：

- `/` 磁盘使用率。
- `niffler-frontdoor` 容器运行和健康状态。
- `niffler-background` 容器运行和健康状态。
- `https://api.niffler.org/_gateway/health` 返回 HTTP 2xx，且 JSON 中
  `status` 为 `ok`。

DMIT 本机检查：

- `/` 磁盘使用率。
- Caddy 入口代理是否运行。
- WireGuard 私网是否在最近 3 分钟完成握手。
- 经本机入口访问 `hd0526` 健康接口是否正常。
- 从每月 14 日开始按十进制 GB 计算入口收发总流量；达到 3.5 TB 预警，达到 4.25 TB 严重告警。

## 告警规则

- systemd timer 每分钟执行一次，增加最多 15 秒随机延迟。
- 磁盘使用率达到 80%发送预警，达到 90%发送严重告警。
- 容器或网站连续 3 次检查失败后发送告警，避免短暂重启或网络波动产生误报。
- 状态没有变化时不重复发送消息。
- 异常恢复后发送恢复消息。
- 首次部署发送一条测试摘要，列出当前所有检查结果。

通知使用以下名称：

- `rn01` 显示为“数据库服务器（rn01）”。
- `hd0526` 显示为“应用服务器（hd0526）”。
- Linux 的 `/` 根分区显示为“系统盘”。
- 容器分别显示为“数据库服务”“缓存服务”“网站前台”和“后台任务”。
- `health=healthy` 等内部状态不再出现在日常消息中。

测试摘要会明确说明“这是一条测试消息”，真实告警会说明可能影响和处理建议。

## Bot 命令

Bot 命令只在 `rn01` 接收，并且只接受 Telegram 配置中现有的私人 Chat ID。
其他聊天发来的命令不会修改任何配置。

```text
/status
/settings
/set_disk_warning 85
/set_disk_critical 92
/set_failures 3
/help
```

- 设置命令默认同时修改三台服务器。
- 命令末尾可增加 `rn01`、`hd0526` 或 `dmit`，只修改指定服务器，例如
  `/set_disk_warning 82 hd0526`。
- 磁盘预警值允许 `50` 到 `95`，严重值允许 `60` 到 `99`，且预警值必须小于
  严重值。
- 连续失败次数允许 `1` 到 `10`。
- 新设置会在下一次每分钟检查时生效，不需要重启应用。

`rn01` 不保存 `hd0526` 或 DMIT 的 root 登录能力。它为两台远程服务器分别使用
独立 SSH 密钥连接 `niffler-monitor-sync` 用户；密钥被强制绑定到监控设置脚本，只能
读取监控状态和修改上述三个整数，不能获得终端、转发端口或执行其他命令。

DMIT 的 3.5 TB、4.25 TB 流量阈值和每月 14 日计费周期不通过 Bot 修改，避免普通
磁盘设置误改套餐容量规则。

## 凭据与状态

- Telegram 凭据放在 `/etc/niffler-monitor/telegram.env`，权限 `0600`。
- 节点配置放在 `/etc/niffler-monitor/monitor.env`，权限 `0600`。
- 状态放在 `/var/lib/niffler-monitor/`，权限 `0700`。
- Bot 接收进度放在 `/var/lib/niffler-monitor-bot/`，用于防止重复处理旧消息。
- Token、Chat ID 不得写入仓库、日志或消息正文。

## systemd 文件

- `/usr/local/sbin/niffler-production-monitor`
- `/etc/systemd/system/niffler-production-monitor.service`
- `/etc/systemd/system/niffler-production-monitor.timer`
- `/usr/local/sbin/niffler-monitor-bot-controller`
- `/etc/systemd/system/niffler-monitor-bot-controller.service`
- `/etc/systemd/system/niffler-monitor-bot-controller.timer`

检查命令：

```bash
systemctl status niffler-production-monitor.timer
systemctl list-timers niffler-production-monitor.timer
journalctl -u niffler-production-monitor.service
journalctl -u niffler-monitor-bot-controller.service
```

## 回退

只回退 Bot 命令时，停止并禁用 `niffler-monitor-bot-controller.timer`，监控告警仍会
继续工作。完整回退时再停止 `niffler-production-monitor.timer`，恢复部署前脚本和
配置并执行 `systemctl daemon-reload`。这些操作不影响应用、数据库、Redis 或数据库
备份。

## 验证方式

- 脚本通过 Bash 语法检查和 ShellCheck。
- systemd service、timer 通过 `systemd-analyze verify`。
- 三台服务器各发送一条测试摘要，Telegram API 返回成功。
- `/status` 和 `/settings` 返回三台服务器的中文状态与当前设置。
- 非授权 Chat ID 无法修改设置；错误范围和预警值大于严重值会被拒绝。
- 修改单台和同时修改三台服务器均通过，修改后下一次监控读取新值。
- `hd0526` 的同步密钥无法执行监控设置脚本以外的命令。
- 首次普通检查创建状态文件，第二次相同状态检查不重复发送消息。
- 定时器为 enabled 和 active。
- 部署后再次确认核心容器健康和公开健康接口返回 HTTP 200。

## 2026-07-28 执行结果

- `rn01` 和 `hd0526` 均已安装并启用每分钟监控 timer。
- 两台服务器的 Telegram 测试摘要均投递成功。
- `hd0526` 当前磁盘使用率 83%，首次正式检查已发送一条真实预警；后续相同状态
  检查没有重复通知。
- `rn01` 当前磁盘使用率 42%，PostgreSQL 和 Redis 健康。
- `hd0526` 的 frontdoor、background 健康，公开健康接口返回 HTTP 200。
- 隔离测试确认：容器连续第 3 次异常时发送一次告警，相同异常不重复发送，恢复时
  发送一次恢复消息；磁盘预警、严重告警和恢复也只在状态变化时发送。
- 三台服务器的监控配置和 Telegram 凭据权限均为 `0600`。
- `rn01` 已启用 Bot 命令控制器定时器，`hd0526` 的受限同步用户目录和设置文件为
  `0700/0600`；Bot 菜单已注册 `/status`、`/settings`、阈值设置命令和 `/help`。
- 监控通知已改为人性化中文：根分区显示为“系统盘”，容器显示为业务服务名称，
  备份成功消息只显示可理解的结果和大小。

## 2026-08-14 DMIT 扩展结果

- DMIT 已加入同一个 Telegram 通知会话，测试摘要投递成功。
- DMIT 每分钟检查系统盘、Caddy、WireGuard、`hd0526` 回源和本计费周期流量。
- `/status`、`/settings` 和三个阈值设置命令扩展为 `rn01`、`hd0526`、`dmit` 三台服务器。
- DMIT 使用独立受限 SSH 密钥，无法获得终端或执行监控设置以外的命令。

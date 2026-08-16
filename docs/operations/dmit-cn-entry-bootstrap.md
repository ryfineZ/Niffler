# DMIT 中国大陆优化入口部署记录

## 目标

- 使用已购买的 DMIT LAX AS3 Pro MINI 作为中国大陆用户的独立入口。
- DMIT 只终止 HTTPS 并转发网站和 Niffler API，不运行 Frontdoor、Background、PostgreSQL、Redis 或 FFmpeg。
- DMIT 通过 WireGuard 私网访问 `hd0526`，不直接访问数据库。
- 使用 `cn.niffler.org` 作为网站和 API 共用入口，Cloudflare 仅提供灰云 DNS 解析。

## 非目标

- 本次不修改 `niffler.org`、`api.niffler.org`、`us1.niffler.org` 或 `us2.niffler.org` 的 DNS 记录及代理状态。
- 本次不迁移 Niffler 应用、数据库、Redis、Background 或媒体任务。
- 本次不允许 DMIT 访问 `rn01` 的 PostgreSQL 或 Redis。
- 本次不让图片/视频工作台的大文件经过 DMIT；标准 Niffler API 中的多模态数据仍整体经过入口。

## 已购节点

| 项目 | 实际值 |
|---|---|
| 厂商与产品 | DMIT LAX AS3 Pro MINI |
| 公网地址 | `179.253.242.2` |
| 配置 | 4 vCPU、4 GB RAM、80 GB SSD |
| 月流量 | 5 TB，额度内最高 10 Gbit/s |
| 超额规则 | 超过月流量后不计量，但限速为 8 Mbit/s |
| 操作系统 | Debian 13 |
| SSH 别名 | `dmit-lax-as3-pro-mini` |

## 访问路径

```text
中国大陆用户
  -> cn.niffler.org（Cloudflare 灰云 DNS）
  -> DMIT Caddy（HTTPS、访问日志）
  -> WireGuard 私网
  -> hd0526 现有 Caddy（私网 443）
  -> hd0526 Frontdoor
  -> rn01 PostgreSQL / Redis
```

DMIT 到 `hd0526` 的 2026-08-14 首次实测结果：10 次 ICMP 平均约 0.7 ms、无丢包；直接访问 `hd0526` 健康接口总时间约 9 至 11 ms。该结果只证明两台美西服务器之间的回源链路良好，不代表中国大陆三网访问体验，正式切换前仍需分别从电信、联通和移动测试。

## 行为变化

- `hd0526` 增加 WireGuard 私网地址；现有 Caddy 只信任 DMIT 的 WireGuard 私网地址提供的用户来源信息。
- `cn.niffler.org` 解析到 DMIT 公网地址 `179.253.242.2`，不经过 Cloudflare 代理、WAF 或 CDN。
- DMIT 只允许 SSH、HTTP、HTTPS 和 WireGuard 所需端口；Caddy 为 `cn.niffler.org` 自动申请和续期 HTTPS 证书。
- DMIT Caddy 保留真实客户端地址所需的转发头，但会覆盖公网用户伪造的相关请求头。
- 入口必须支持长时间流式响应和多模态请求体，不能使用普通网页接口的小请求体上限。
- DMIT Caddy 在 `/__niffler_latency` 本地返回空的 `204` 响应，并允许跨域、禁止缓存；测速请求不再转发到 hd0526。
- 首页将该入口显示为“三网优化”和公开入口域名 `cn.niffler.org`，不暴露 DMIT 厂商名或内部机器编号。
- 当前没有在 Caddy 上安装第三方限流模块；限流由 Niffler 自身承担，后续如需入口限流应单独设计和验证。

## 影响范围

- WireGuard 或 DMIT 故障只影响未来使用大陆直连域名的用户。
- `niffler.org`、`api.niffler.org`、`us1.niffler.org` 和 `us2.niffler.org` 不受本次上线影响。
- `hd0526` 现有 Caddy、Frontdoor、Background 及数据库连接方式保持不变。
- 因为入口不经过 Cloudflare，DMIT 故障和直接攻击不会由 Cloudflare 代为吸收；防火墙、监控和 Niffler 自身鉴权仍然生效。

## 验证方式

- 两端 WireGuard 握手正常，私网互相可达。
- DMIT 通过私网访问 `hd0526` 健康接口返回 HTTP 200。
- `cn.niffler.org` 的 DNS 只返回 `179.253.242.2`，Cloudflare 代理状态为关闭。
- `cn.niffler.org` 的 HTTPS 证书有效，首页、健康接口、标准 API 和流式响应均能正常转发。
- `cn.niffler.org/__niffler_latency` 返回 HTTP 204，包含允许跨域和禁止缓存的响应头，并且首页能显示该线路的成功或失败状态。
- DMIT 重启后 WireGuard、Caddy、防火墙和监控自动恢复。
- DMIT 监控从本机访问 `cn.niffler.org` 的正式 HTTPS 地址，同时检查证书、入口代理和 `hd0526` 回源。
- Telegram `/status` 和 `/settings` 能显示 DMIT，设置命令只能修改允许的三个数字。
- `/var/lib/niffler-monitor-control/monitor.env` 必须由 `niffler-monitor-sync:niffler-monitor-sync` 持有并使用 `0600` 权限，否则受限 Bot 通道无法读取或修改阈值。
- 公网只能访问明确允许的端口，WireGuard 私网不能被其他来源访问。
- 现有四个域名的解析地址和 Cloudflare 代理状态与上线前一致。

## 回退

1. 删除 `cn.niffler.org` 的新 DNS 记录，立即停止新流量进入 DMIT。
2. 停止并禁用 DMIT 的 Caddy 和 WireGuard 服务。
3. 恢复 `hd0526` 修改前的 Caddy 配置。
4. 删除两端 WireGuard 配置和对应防火墙规则。

回退不需要重启 Niffler、PostgreSQL 或 Redis，也不改变现有全球入口。

## 2026-08-14 执行结果

- DMIT 已安装并启用 nftables、Fail2ban、自动安全更新、vnStat、WireGuard 和 Caddy。
- SSH 只允许密钥认证，禁止密码、交互式认证、代理转发和端口转发；重启后登录正常。
- DMIT `10.89.0.2` 与 `hd0526` `10.89.0.1` 已建立 WireGuard 私网，10 次测试无丢包，平均约 1.4 ms。
- DMIT 经私网访问 `hd0526` 健康接口连续返回 HTTP 200，总耗时约 9 至 10 ms。
- Caddy 测试入口只监听 `127.0.0.1:18080`，尚未监听公网 80/443，也未申请正式域名证书。
- DMIT 已完成重启验证；防火墙、WireGuard、Caddy、流量统计和监控均自动恢复。
- Telegram 每分钟检查系统盘、入口代理、私网、回源和本计费周期流量；测试通知已投递成功。
- Bot `/status`、`/settings` 和阈值设置支持 `rn01`、`hd0526`、`dmit` 三台服务器。
- `rn01` 到 DMIT 使用独立受限密钥；任意命令、交互终端和端口转发测试均被拒绝。
- `api.niffler.org`、`niffler.org`、`us2.niffler.org` 回归检查均为 HTTP 200；Frontdoor、Background、PostgreSQL 和 Redis 健康。
- 未创建或修改任何 DNS 记录。正式上线仍需用户确认大陆直连域名，并完成大陆电信、联通、移动实测。

## 2026-08-14 正式上线结果

- 用户确认使用 `cn.niffler.org`，作为中国大陆用户的网站和 API 共用入口。
- 该域名使用 Cloudflare 灰云 DNS，直接解析到 DMIT `179.253.242.2`。
- DMIT Caddy 从本机测试地址切换为公网 HTTPS，继续通过 WireGuard 回源到 `hd0526`。
- 本次只新增 `cn.niffler.org`，不修改任何现有域名。
- 上线验证包括 DNS、HTTPS 证书、首页、健康接口、标准 API、流式响应、真实客户端地址和现有域名回归检查。
- Telegram 监控改为检查正式 HTTPS 地址，不再依赖下线后的 `127.0.0.1:18080` 测试入口。
- 正式 Caddy、监控配置和回退副本已在 DMIT 上通过语法验证并启用。
- Cloudflare 凭据确认为账户级 API 令牌，账户级验证状态为 `active`，并具备读取 `niffler.org` Zone 的权限；用户级验证接口不适用于该令牌。
- `cn.niffler.org` 已创建为灰云 A 记录，只解析到 `179.253.242.2`；现有四个入口记录未修改。
- Let's Encrypt 证书签发成功，HTTP 自动跳转 HTTPS；首页和健康接口返回 HTTP 200，未携带密钥的 `/v1/models` 与流式 `/v1/responses` 请求均到达 Niffler 鉴权层并返回 HTTP 401。
- DMIT 本机正式 HTTPS 监控全部正常；公网访问首页和健康接口均返回 HTTP 200。
- 正式发布首次安装时监控配置所有者被改为 `root`，导致 Bot 阈值命令无权访问；已恢复为 `niffler-monitor-sync:niffler-monitor-sync`、`0600` 并重新验证。
- 证书公开后入口立即出现常见路径扫描，确认灰云入口需要持续依赖防火墙、访问日志、Niffler 鉴权和流量告警；当前未发现管理接口暴露。
- 仍需从中国大陆电信、联通和移动分别完成真实延迟、丢包、长连接和多模态端到端测试。

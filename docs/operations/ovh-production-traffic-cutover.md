# OVH 主应用节点生产流量切换

## 目标

- 将 Niffler 的 Frontdoor 流量从 `hd0526` 分批迁移到 OVH 美西节点。
- 保留 `hd0526` 上的单活 Background，并保留 Frontdoor 作为快速回退节点。
- 在迁移主站时继续提供 `/InfiniteCanvas/*`，该路径暂时由 OVH 转发到 `hd0526`。

## 非目标

- 本次不迁移 PostgreSQL、Redis、数据库备份或 Background。
- 本次不停止 `hd0526` 的 Frontdoor、Caddy、InfiniteCanvas 或其他站点。
- 本次不部署 Cloudflare Load Balancing，也不处理未来图片和视频的大文件上传。

## 行为变化

- `cf.niffler.org`、`hub.niffler.org`、`api.niffler.org` 和 `niffler.org` 按顺序切换到 OVH，不一次性修改全部记录。
- `api.niffler.org` 从灰云改为橙云；其他三个域名继续使用橙云。
- Cloudflare 到 OVH 使用“完全（严格）”TLS，OVH 使用覆盖 `niffler.org` 和 `*.niffler.org` 的 Cloudflare Origin CA 证书。
- `niffler.org/InfiniteCanvas/*` 暂时经过“Cloudflare → OVH → hd0526”；旧源站地址由 OVH `.env` 中的 `INFINITE_CANVAS_ORIGIN` 管理，其他 Niffler 页面和 API 直接由 OVH Frontdoor 处理。
- OVH 只接受 Cloudflare 官方网络访问 80/443，Frontdoor 仍只监听 `127.0.0.1:18084`。

## 影响范围

- 域名切换会改变真实用户请求路径，应在低峰期逐项执行并观察。
- Cloudflare 免费套餐对代理请求体有大小限制，未来图片和视频上传必须使用 R2 预签名直传，不能让大文件经过 Frontdoor。
- InfiniteCanvas 在过渡期仍依赖 `hd0526`；旧服务器不能下线。
- 两台 Frontdoor 共享 PostgreSQL 和 Redis，登录状态、账号、余额和请求状态不需要迁移。

## 切换前条件

- OVH Frontdoor、Caddy、WireGuard 和监控全部正常。
- OVH 已安装多域名 Origin CA 证书，Caddy 配置验证通过。
- `hd0526` Frontdoor、Background、Caddy 和 InfiniteCanvas 全部正常。
- 数据库备份最近一次执行成功，Telegram 通知正常。
- 回退值已经记录：旧源站 `23.19.228.223`，新源站 `15.204.120.221`。

## 切换顺序

每切换一个域名，至少验证健康接口、首页或认证接口、流式模型请求和错误日志；确认稳定后再处理下一个域名。

1. `cf.niffler.org`：A 记录从 `23.19.228.223` 改为 `15.204.120.221`，保持橙云。
2. `hub.niffler.org`：A 记录从 `23.19.228.223` 改为 `15.204.120.221`，保持橙云。
3. `api.niffler.org`：A 记录改为 `15.204.120.221`，从灰云改为橙云。
4. `niffler.org`：A 记录从 `23.19.228.223` 改为 `15.204.120.221`，保持橙云；额外验证 `/InfiniteCanvas/`。

## 每步验证

- Cloudflare 返回 HTTP 200，并包含 `server: cloudflare` 响应头。
- `/_gateway/health` 返回 JSON 且 `status` 为 `ok`。
- 首页、登录状态接口和公开模型接口正常。
- 执行一个短响应请求和一个流式请求，确认首包和持续输出正常。
- OVH Caddy、Frontdoor 无新增 5xx，PostgreSQL 和 Redis 连接正常。
- Telegram 监控没有新故障提醒。

## 回退

- 任一域名出现持续 5xx、登录异常、请求中断或明显延迟升高时，只回退该域名，不继续后续步骤。
- `cf.niffler.org`、`hub.niffler.org`、`niffler.org`：A 记录恢复为 `23.19.228.223`，保持橙云。
- `api.niffler.org`：A 记录恢复为 `23.19.228.223`，并恢复灰云和 2 分钟 TTL。
- 回退后验证该域名重新由 `hd0526` 提供服务；OVH 容器保留运行以便排查，不执行数据库回退。
- 全部域名稳定运行至少 24 小时前，不停止 `hd0526` Frontdoor；Background 长期保持单活。

## 验证方式

- 切换前使用 Origin CA 根证书验证 OVH 各虚拟主机及其反向代理目标。
- 切换后分别从 Cloudflare 公网地址验证四个域名。
- 最后复查 OVH、`hd0526`、`rn01` 的容器、WireGuard、数据库代理和监控定时器。

## 2026-08-06 预切换结果

- OVH 私钥仅保存在服务器，权限为 `0600`；Cloudflare Origin CA 证书覆盖 `niffler.org` 和 `*.niffler.org`，有效期至 2041 年。
- OVH 已启用多域名 Caddy 配置；`api`、`hub`、`cf`、主站和测试域名的本机健康检查均通过。
- `/InfiniteCanvas/` 已能通过 OVH 转发到 `hd0526`，返回 HTTP 200。
- `ovh-origin.niffler.org` 通过 Cloudflare“完全（严格）”回源返回 HTTP 200，直接连接 OVH 源站被防火墙拒绝。
- OVH Frontdoor、Caddy、WireGuard 和监控正常，Background 不存在；`hd0526` 和 `rn01` 原服务正常。
- 正式 DNS 尚未修改，当前真实用户流量仍由 `hd0526` 承担。

## 2026-08-06 正式切换结果

- `cf.niffler.org`、`hub.niffler.org`、`api.niffler.org` 和 `niffler.org` 已依次切换到 `15.204.120.221`，四条 A 记录均已开启 Cloudflare 代理并使用自动 TTL。
- 每个域名均在单独验证通过后再切换下一个域名；切换期间未触发回退。
- 四个域名的 `/_gateway/health` 均返回 `status=ok`，主页返回 HTTP 200，响应头确认请求经过 Cloudflare。
- 四个域名的公开模型接口均返回 HTTP 200；`api.niffler.org` 的登录接口对空请求返回预期的 HTTP 400，说明认证请求已到达新 Frontdoor。
- `niffler.org/InfiniteCanvas/` 返回 HTTP 200，过渡期转发路径工作正常。
- OVH Frontdoor、Caddy、WireGuard 和监控保持正常；`hd0526` 的 Frontdoor、Background、Caddy 和 InfiniteCanvas 继续运行，作为回退和现有后台任务节点。
- `rn01` 的 PostgreSQL、Redis 和备份任务保持正常；15 条应用数据库连接全部使用 TLS 1.3，没有明文连接，Redis 认证检查返回 `PONG`。数据库和缓存未在本次切换中迁移。
- 最近一次数据库备份约 1.97 GB，已经上传 R2 并完成校验，Telegram 成功通知；三台服务器的监控任务均执行成功。
- 本次未使用真实用户密钥发起会产生费用的模型生成和流式请求，生成链路仍需结合正常业务请求继续观察。
- `hub.niffler.org` 的 DNS 注释已同步改为 `Niffler hub on OVH US West`，避免后续运维误判源站。

## 配置审查修复

- 正式域名和 InfiniteCanvas 路由统一保存在 `deploy/ovh-primary/Caddyfile`，不再保留容易覆盖生产配置的第二份 Caddy 文件。
- `INFINITE_CANVAS_ORIGIN` 是 Compose 必填环境变量；缺少该变量时配置检查直接失败，避免静默转发到错误源站。
- `rn01` 不再放行以自身公网 IP 为来源的 PostgreSQL 和 Redis 流量；现有 `hd0526` 公网白名单和 OVH WireGuard 私网规则不变。
- WireGuard 配置写入后必须重新启动对应接口，以保证更换密钥、端点或地址后立即生效。
- 验证包括 Compose 展开、Caddy 配置、四个公开域名、InfiniteCanvas、PostgreSQL TLS、Redis 认证和三节点监控状态。

## 2026-08-06 配置审查修复结果

- 仓库与 OVH 现在共同使用唯一的 `deploy/ovh-primary/Caddyfile`，旧的 `Caddyfile.production` 已删除。
- OVH `.env` 已设置 `INFINITE_CANVAS_ORIGIN`，Caddy 容器已重新创建并确认读取该变量；Frontdoor 未重启。
- rn01 已删除 3 条以自身公网 IP 为来源的无效数据库规则，删除前的完整 IPv4 防火墙规则已保存到 root 专用回退文件。
- hd0526 的现有公网数据库白名单、OVH WireGuard 私网规则、PostgreSQL、Redis 和数据库备份配置均未改变。
- WireGuard 部署脚本已改为写入配置后重新启动接口；本次没有重启当前正常工作的 WireGuard，只修正后续重复执行行为。

# OVH 美西主应用节点初始化

## 目标

- 将 `ovh-US-WEST-OR-VPS-4` 初始化为 Niffler 主应用节点。
- 安装受支持的 Docker Engine、基础安全更新、防火墙和生产监控。
- 准备只运行 Frontdoor 的独立 Compose 配置，并在切换生产流量前完成本机验证。
- 保留 `hd0526` 为现有生产节点和后续备用节点。

## 非目标

- 本次不修改 `api.niffler.org` 或现有公开流量入口；只新增独立测试域名。
- 本次不启动 OVH 上的 Background，避免和 `hd0526` 重复执行后台任务。
- 本次不迁移 PostgreSQL、Redis 或 R2 数据库备份任务。
- 本次不停止或重启 `hd0526`、`rn01` 上的现有生产服务。
- 本次不开放 PostgreSQL、Redis、Docker API 或应用本地端口到公网。

## 独立 HTTPS 测试入口

- 在基础初始化完成后，使用 `ovh-origin.niffler.org` 验证 OVH 的公开 HTTPS 链路。
- 测试域名只连接 OVH Frontdoor，不替代 `api.niffler.org`，也不接收现有生产流量。
- Caddy 作为 Compose 内的独立容器运行并使用主机网络，通过 `127.0.0.1:18084` 访问 Frontdoor；这样 80/443 由 UFW 直接过滤，避免 Docker 端口发布绕过主机防火墙。
- 证书签发期间测试域名临时使用 DNS 灰云；HTTPS 验证通过后改为橙云。
- 切换橙云后，OVH 的 80/443 入站规则只允许 Cloudflare 官方 IPv4 和 IPv6 网络，SSH 规则不变。
- 回退时删除测试 DNS 记录、停止 Caddy，并移除新增的 80/443 防火墙规则；Frontdoor 和现有生产流量不受影响。

## 行为变化

- OVH 安装 Docker Engine、Compose 插件、Caddy、WireGuard、故障封禁和基础诊断工具。
- 主机防火墙默认拒绝公网入站，只允许 SSH；公开 HTTP/HTTPS 端口在接入流量前保持关闭。
- Docker 应用端口只绑定 `127.0.0.1`。
- OVH 与 `rn01` 使用 `10.71.0.0/30` WireGuard 私网通信；OVH 不直接访问数据库公网端口。
- `rn01` 只在 WireGuard 地址上提供 PostgreSQL 和 Redis 转发，现有数据库容器和公网白名单不变。
- WireGuard 脚本重复执行时会重新载入刚写入的配置，并删除曾经误加的 `rn01` 自身公网 IP 数据库放行规则。
- `deploy/ovh-primary/Caddyfile` 是唯一的正式 Caddy 配置；InfiniteCanvas 的旧源站由 `.env` 中的 `INFINITE_CANVAS_ORIGIN` 提供，不在 Caddy 配置中写死地址。
- 安装 Niffler 生产监控脚本和定时器；Frontdoor 未部署前，监控只检查主机资源，不发送误报。
- 应用目录、固定部署器和部署账号按照现有生产发布权限边界准备。

## 影响范围

- 仅修改新购 OVH 主机，不改变当前线上请求路径。
- 安装系统更新可能要求后续安排一次重启；未验证 SSH 新会话前不会重启。
- 防火墙启用前必须确认 SSH 规则和当前公钥登录持续可用。

## 实施顺序

1. 保存系统、SSH、端口和软件包初始状态。
2. 安装系统更新和基础软件。
3. 配置防火墙、故障封禁、自动安全更新和系统参数。
4. 安装并验证 Docker Engine 与 Compose 插件。
5. 建立 OVH 到 `rn01` 的 WireGuard 私网，并验证 PostgreSQL TLS 和 Redis 认证链路。
6. 准备 Frontdoor 专用应用目录、Compose 配置和固定部署器。
7. 接通受限的数据库与 Redis 链路后部署 Frontdoor；Background 保持不存在或停止。
8. 安装监控，完成两次间隔验证。

## 验证方式

- SSH 公钥新会话连续登录成功，密码认证继续被拒绝。
- 防火墙处于启用状态，公网只开放明确批准的端口。
- Docker 与 Compose 服务开机自启并通过容器冒烟测试。
- WireGuard 只路由 `10.71.0.0/30`，OVH 可访问 `10.71.0.1:5432` 和 `10.71.0.1:6379`。
- OVH 不存在运行中的 Niffler Background。
- Frontdoor 部署后仅监听 `127.0.0.1`，本机健康接口返回 HTTP 200。
- Compose 展开后的 Caddy 容器包含 `INFINITE_CANVAS_ORIGIN`，仓库 Caddy 配置与服务器活动配置校验一致。
- `hd0526`、`rn01` 的现有容器和公开健康接口保持正常。

## 回退

- 系统配置修改前的副本统一保存在 OVH 的 root 专用回退目录，权限为 `0700` 或 `0600`。
- 防火墙异常时通过 OVH 控制台恢复规则，不开启 SSH 密码登录。
- Frontdoor 验证失败时停止 OVH Compose 项目，不修改当前 DNS，线上继续由 `hd0526` 提供服务。

## 2026-08-06 实施结果

- OVH 已完成系统安全更新并重启到新内核，Docker、UFW、Fail2ban 和自动安全更新均正常运行。
- 公网仍只开放 SSH；Frontdoor 仅监听 `127.0.0.1:18084`，未开放 HTTP/HTTPS，也未修改 DNS 或 Cloudflare。
- OVH 与 `rn01` 的 WireGuard 私网已经连通；PostgreSQL 和 Redis 仅通过私网转发访问，PostgreSQL 应用连接使用 TLS 1.3。
- Frontdoor 使用与当前生产环境相同的固定提交版本，数据库迁移检查无待执行项，本机首页和健康接口均返回 HTTP 200。
- OVH 未部署 Background；生产 Background 仍只在 `hd0526` 运行。
- OVH 监控每分钟检查系统盘、Frontdoor、Caddy 和 Cloudflare 外部健康接口，Telegram 测试消息发送成功；本次检查全部正常。
- 延迟复查确认 OVH、`hd0526` 和 `rn01` 的相关服务均保持健康，现有生产流量路径未发生变化。
- 独立测试域名 `ovh-origin.niffler.org` 已启用橙云并通过 HTTPS 验证；源站 80/443 只允许 Cloudflare 官方网络访问，公网直接连接已被防火墙拒绝。
- Cloudflare 的区域级 SSL 模式已从“完全”升级为“完全（严格）”；切换前已确认 `niffler.org`、`cf.niffler.org`、`hub.niffler.org` 和 OVH 测试源站均使用有效的受信任证书。

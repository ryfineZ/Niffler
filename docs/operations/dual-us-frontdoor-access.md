# Niffler 多线路入口与用户侧延迟显示

## 目标

- OVH 和 hd0526 的 Frontdoor 同时提供服务。
- `us1.niffler.org` 经 Cloudflare 访问 OVH，`us2.niffler.org` 经 Cloudflare 访问 hd0526。
- `api.niffler.org` 灰云直连 hd0526；`niffler.org` 经 Cloudflare 回源 hd0526，hd0526 作为临时主节点。
- 删除不再使用的 `hub.niffler.org` 和 `cf.niffler.org`。
- 首页比较用户浏览器到两条美西线路和一条大陆优化线路的 HTTPS 请求耗时。
- 首页显示面向用户的线路名称和对应公开入口域名，方便用户根据测速结果选择 API 地址；不显示厂商或内部机器编号。

## 非目标

- 不在 OVH 启动 Background，不改变数据库和 Redis 的部署位置。
- 不根据测速结果自动修改用户配置或转移已经开始的请求。
- 图片和视频内容不经过测速地址。

## 访问路径

```text
api.niffler.org
  -> hd0526 23.19.228.223
  -> hd0526 Frontdoor
  -> rn01 PostgreSQL / Redis

niffler.org
  -> Cloudflare
  -> hd0526 23.19.228.223
  -> hd0526 Frontdoor
  -> rn01 PostgreSQL / Redis

us1.niffler.org
  -> Cloudflare
  -> OVH 15.204.120.221
  -> OVH Frontdoor
  -> rn01 PostgreSQL / Redis

us2.niffler.org
  -> Cloudflare
  -> hd0526 23.19.228.223
  -> hd0526 Frontdoor
  -> rn01 PostgreSQL / Redis

cn.niffler.org
  -> DMIT 179.253.242.2（Cloudflare 灰云 DNS）
  -> DMIT Caddy
  -> WireGuard 私网
  -> hd0526 Frontdoor
  -> rn01 PostgreSQL / Redis
```

## 行为变化

- `api` 使用灰云 DNS 并直连 hd0526，不再获得 Cloudflare WAF 和代理防护；根域名、`us1`、`us2` 使用橙云，继续经过 Cloudflare。
- hd0526 的 `api` 和根域名由 Caddy 管理源站证书；OVH 只保留 `us1` 备用入口及仍在使用的其他独立站点。
- `hub`、`cf` 的 DNS 记录和两台 Caddy 站点配置均删除，不再接受访问。
- 三个入口的 Caddy 都在 `/__niffler_latency` 返回空的 `204` 响应，并设置跨域和禁止缓存响应头。
- 首页在页面加载完成后再并行检测三个测速地址，避免与首屏资源争抢冷连接；若 `load` 被慢资源长期拖住，挂载 1.5 秒后兜底启动，不能无限停在“检测中”。首轮结束后，手动“重新检测”不等待该延后，会立即开始新一轮。
- 每条线路争取取得三次成功样本，单次网络失败或 5 秒超时不会立刻判整条线路不可用；最多尝试四次，至少取得两次成功样本时显示其中位耗时，少于两次才显示“无法连接”。
- 前端只接受精确的 `204` 探针响应；其他状态即使属于 2xx 也按失败样本处理。新一轮检测和组件卸载都会取消旧请求，旧结果不得覆盖新结果。
- 首页将三条线路显示为“美西线路 1”“美西线路 2”和“三网优化”，同时显示 `us1.niffler.org`、`us2.niffler.org`、`cn.niffler.org` 三个公开入口域名，不渲染厂商或内部机器编号。
- 测速值是浏览器到对应入口的完整 HTTPS 请求耗时，首次请求还会受 DNS 和 TLS 建连影响；其中两条美西线路经过 Cloudflare，大陆优化线路直达 DMIT。该值只用于比较三条业务路径，不是纯源站网络延迟，也不等同于模型生成耗时。

## 双节点边界

- 两台 Frontdoor 使用相同版本、认证密钥、PostgreSQL 和 Redis。
- API Key、余额、限流和运行状态由共享存储保持一致。
- Background 只在 hd0526 运行。OVH 启动 Background 会造成定时任务重复执行，因此禁止启动。
- 数据库迁移由一次发布流程执行，不能由两台节点同时独立执行。

## 验证方式

- DNS：`api` 指向 hd0526 且 `proxied=false`；根域名指向 hd0526 且 `proxied=true`；`us1`、`us2` 分别使用指定源站且 `proxied=true`；`cn` 指向 DMIT 且 `proxied=false`；`hub`、`cf` 不再存在。
- TLS：`api`、根域名、`us1`、`us2`、`cn` 均使用系统信任的证书完成 HTTPS 请求。
- 测速：三个 `/__niffler_latency` 地址精确返回 `204`、允许跨域且禁止缓存；单次样本失败后继续补测，至少两次成功才显示耗时，成功样本不足时显示无法连接。
- 应用：首页、认证接口和 `/_gateway/health` 正常；两台 Frontdoor 与唯一 Background 健康。
- 前端：首页显示三条线路的检测中、成功和失败状态；公开入口域名可见，厂商和内部机器编号不可见；首次检测在页面加载后启动，本轮结束后点击“重新检测”会立即发起下一轮；内部新轮次或组件卸载会取消未完成请求。

## 回退

- hd0526 异常时，将 `api` 和根域名恢复指向 OVH `15.204.120.221`；回退前先恢复 OVH 对应的 Caddy 站点配置。
- `hub`、`cf` 只有在明确重新启用时才恢复 DNS 和 Caddy 配置。
- OVH 证书或 Caddy 异常时恢复修改前的 Caddyfile 和防火墙规则。
- hd0526 异常时恢复修改前的 Caddyfile；该操作不影响现有域名。
- 前端异常时回退首页测速组件；三条 API 入口仍可独立使用。

## 2026-08-17 hd0526 OOM 后临时主入口切换

### 目标

- 因 hd0526 Frontdoor 在大请求并发下频繁触发宿主机 OOM，将默认入口 `api.niffler.org` 和 `niffler.org` 临时切到 OVH。
- `us1.niffler.org` 继续由 OVH 提供；`us2.niffler.org` 和 `cn.niffler.org` 保持原线路不变，继续作为 hd0526/DMIT 专线入口。
- hd0526 Frontdoor、唯一 Background 和 InfiniteCanvas 保持运行，既用于专线入口，也作为主站/API 快速回退目标。

### 行为变化

- OVH Caddy 恢复 `api.niffler.org` 与 `niffler.org` 站点，使用现有 Cloudflare Origin CA 证书。
- `api.niffler.org` 的 A 记录从 `23.19.228.223` 改为 `15.204.120.221`，同时由灰云改为橙云；`niffler.org` 保持橙云，仅将源站从 `23.19.228.223` 改为 `15.204.120.221`。
- 主站 `/InfiniteCanvas/*` 暂时通过 OVH 转发到 hd0526，其余主站和 API 请求由 OVH Frontdoor 处理。

### 影响范围与非目标

- Background 仍只在 hd0526 运行，不在 OVH 启动第二份后台任务。
- 不修改数据库、Redis、`us2`、`cn`、DMIT 或 hd0526 Caddy 配置。
- Cloudflare 代理会成为 `api.niffler.org` 的新入口层；客户端看到的源站地址、TLS 终止和请求体平台限制会随之变化。

### 切换与验证

1. 确认 OVH 与 hd0526 运行同一生产镜像，OVH Frontdoor、Caddy、数据库和 Redis 连接健康。
2. 备份 OVH Caddyfile，恢复主站/API 站点，先完成 Caddy 配置校验和本机固定 Host 验证。
3. 先切 `api.niffler.org`，验证健康接口、认证错误语义、连续公开请求和 OVH 日志；通过后再切 `niffler.org`。
4. 验证主站首页、`/_gateway/health`、`/InfiniteCanvas/`、公开 API、OVH 资源和 5xx；确认 hd0526 默认入口请求下降。

### 回退

- 任一步出现持续 5xx、登录异常、TLS 错误或明显延迟升高，立即停止后续步骤。
- `api.niffler.org` 恢复为 `23.19.228.223`、`proxied=false`、TTL 300；`niffler.org` 恢复为 `23.19.228.223`、`proxied=true`、自动 TTL。
- DNS 回退验证后，OVH 主站/API Caddy 站点可以保留作为后续快速故障切换能力；若其自身异常，则恢复切换前保存的 OVH Caddyfile并热重载。

### 2026-08-17 切换结果

- OVH 与 hd0526 均运行生产镜像 `niffler-app:55550c553c11556b1c9c0b71f7d683ff4d2066b6`，镜像 ID 完全一致；当前 `main` 相比该镜像提交只增加运维记录，没有新的应用二进制变化，因此无需更新 OVH。
- OVH 恢复 `api.niffler.org` 和 `niffler.org` Caddy 站点并无中断热加载；切换前配置保存在 `/root/niffler-ovh-cutover-20260817T120455Z`。
- `api.niffler.org` 与 `niffler.org` 的 Cloudflare A 记录均已指向 `15.204.120.221`，保持橙云和自动 TTL。两次唯一 trace 分别只出现在 OVH Frontdoor 日志，hd0526 为 0，确认不是仅修改控制面而是实际完成回源切换。
- 根域名和 API 首轮各连续 10 次、间隔复核各连续 5 次健康检查全部返回 200；首页、InfiniteCanvas、登录错误语义、`us1`、`us2`、`cn` 和 `ovh-origin` 均符合预期。北京时间 20:19 复核时 OVH 已完成 152 条请求，网关 5xx、Caddy error 和内核 OOM 均为 0。
- OVH Frontdoor 与 Caddy 均未重启，Frontdoor 内存约 318 MiB；hd0526 Frontdoor 和唯一 Background 继续健康运行，作为专线入口和快速回退目标。

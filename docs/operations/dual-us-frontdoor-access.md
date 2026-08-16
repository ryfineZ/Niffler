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
- 首页并行请求三个测速地址，显示浏览器完成 HTTPS 请求的耗时；失败时明确显示无法连接，并允许重新检测。
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
- 测速：三个 `/__niffler_latency` 地址返回 `204`、允许跨域且禁止缓存。
- 应用：首页、认证接口和 `/_gateway/health` 正常；两台 Frontdoor 与唯一 Background 健康。
- 前端：首页显示三条线路的检测中、成功和失败状态；公开入口域名可见，厂商和内部机器编号不可见，重新检测可以再次发起请求。

## 回退

- hd0526 异常时，将 `api` 和根域名恢复指向 OVH `15.204.120.221`；回退前先恢复 OVH 对应的 Caddy 站点配置。
- `hub`、`cf` 只有在明确重新启用时才恢复 DNS 和 Caddy 配置。
- OVH 证书或 Caddy 异常时恢复修改前的 Caddyfile 和防火墙规则。
- hd0526 异常时恢复修改前的 Caddyfile；该操作不影响现有域名。
- 前端异常时回退首页测速组件；三条 API 入口仍可独立使用。

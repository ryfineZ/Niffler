# hd0526 Frontdoor OOM 应急保护

## 目标

- 防止 `us2.niffler.org` 与 `cn.niffler.org` 的高并发大请求再次耗尽 hd0526 的 5.8 GiB 宿主机内存。
- 将故障范围限制在 hd0526 Frontdoor，保护同机 Caddy、唯一 Background 和宿主机。

## 非目标

- 不改变 `niffler.org`、`api.niffler.org` 与 `us1.niffler.org` 所在 OVH 主入口。
- 不修改 Background、数据库、PgBouncer、Redis 或三条首页测速端点。
- 不把并发门当作内存放大问题的最终代码修复，也不在本次应急变更中降低全站请求体上限。

## 行为变化

- hd0526 Frontdoor 最多允许 4 个动态网关请求同时在途；许可在读取请求体前获取，并一直持有到响应流结束。静态首页、静态资源和直接健康路由不经过这道门。
- 第 5 个及之后的并发请求不排队，立即返回服务繁忙响应，避免大量大请求同时进入完整缓冲和转换路径。
- hd0526 Frontdoor 使用 4 GiB 内存与 4 GiB memory-swap 硬上限；宿主机没有 Swap，因此不会获得额外交换空间。
- 这些限制只属于 `frontdoor` 服务，不得写入共享 `.env`，避免未来重建 Background 时误继承。

## 影响范围与取舍

- 高峰期使用 `us2` 或 `cn` 的部分请求可能收到快速 503；用户仍可改用 OVH 主入口。
- 4 GiB 硬上限用于保护整机。若代码仍单请求异常膨胀，Frontdoor 仍可能在容器内被 OOM 并自动重启，但不会先耗尽整台宿主机。
- 本次值 4 是针对 5.8 GiB、无 Swap 主机的保守应急值；没有稳定性数据前不得直接上调。

## 发布与验证

1. 备份生产 `docker-compose.yml`，并用 `docker compose config` 验证候选配置。
2. 只执行 `docker compose up -d --no-deps --force-recreate frontdoor`，禁止重建 Caddy 或 Background。
3. 验证 `/_gateway/health` 中 `request_concurrency.limit` 为 4。
4. 验证容器 `Memory` 与 `MemorySwap` 都是 4294967296，Frontdoor healthy，Background 未重启且仍为唯一实例。
5. 连续检查 `us2`、`cn`、主站与 API；观察内存、重启、OOM 和 5xx。

## 回滚

- 删除 Frontdoor 的 `AETHER_GATEWAY_MAX_IN_FLIGHT_REQUESTS`、`mem_limit` 与 `memswap_limit` 后，仅重建 Frontdoor。
- 只有在其他内存保护已经生效时才允许移除 4 GiB 硬上限；不得只通过反复重启掩盖 OOM。

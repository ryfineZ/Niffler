# hd0526 Frontdoor OOM 应急保护

## 目标

- 防止 `us2.niffler.org` 与 `cn.niffler.org` 的高并发大请求再次耗尽 hd0526 的 5.8 GiB 宿主机内存。
- 将故障范围限制在 hd0526 Frontdoor，保护同机 Caddy、唯一 Background 和宿主机。

## 非目标

- 不改变 `niffler.org`、`api.niffler.org` 与 `us1.niffler.org` 所在 OVH 主入口。
- 不修改 Background、数据库、PgBouncer、Redis 或三条首页测速端点。
- 不把并发门当作内存放大问题的最终代码修复，也不在本次应急变更中降低全站请求体上限。

## 行为变化

- hd0526 Frontdoor 最多允许 64 个动态网关请求同时在途；许可在读取请求体前获取，并一直持有到响应流结束。静态首页、静态资源和内部 `/_gateway/health` 路由不经过这道门。
- 第 65 个及之后的并发请求不排队，立即返回服务繁忙响应，避免无限量大请求同时进入完整缓冲和转换路径。
- Docker 健康检查调用的 `/health` 当前会经过同一并发保护；高峰期该路径可能短暂返回 `503 local_overloaded`，应结合内部 `/_gateway/health`、容器状态和公开入口判断是否为真实故障。
- hd0526 Frontdoor 使用 4 GiB 内存与 4 GiB memory-swap 硬上限；宿主机没有 Swap，因此不会获得额外交换空间。
- 这些限制只属于 `frontdoor` 服务，不得写入共享 `.env`，避免未来重建 Background 时误继承。

## 影响范围与取舍

- 高峰期使用 `us2` 或 `cn` 的部分请求可能收到快速 503；用户仍可改用 OVH 主入口。
- 4 GiB 硬上限用于保护整机。若代码仍单请求异常膨胀，Frontdoor 仍可能在容器内被 OOM 并自动重启，但不会先耗尽整台宿主机。
- 本次按用户要求把值从 8 提到 32，是在 5.8 GiB、无 Swap 主机上保留 4 GiB 硬上限的高风险扩容；32 是 8 的四倍，必须重点观察内存和 OOM。

## 发布与验证

1. 备份生产 `docker-compose.yml`，并用 `docker compose config` 验证候选配置。
2. 只执行 `docker compose up -d --no-deps --force-recreate frontdoor`，禁止重建 Caddy 或 Background。
3. 验证 `/_gateway/health` 中 `request_concurrency.limit` 为 64。
4. 验证容器 `Memory` 与 `MemorySwap` 都是 4294967296，Frontdoor healthy，Background 未重启且仍为唯一实例。
5. 连续检查 `us2`、`cn`、主站与 API；至少观察 10 分钟内存、重启、OOM 和 5xx。

## 本次实施记录（2026-09-03）

- 变更范围：仅将 hd0526 源 Compose 中 Frontdoor 的 `AETHER_GATEWAY_MAX_IN_FLIGHT_REQUESTS` 从 `8` 改为 `32`；4 GiB 内存和 memory-swap 限制保持不变。
- 不变项：镜像、Background、数据库、Caddy、DNS、请求体上限和公开路由均不变。
- 回退触发：Frontdoor 不 healthy、内部健康失败、公开入口持续 5xx、内存快速增长，或出现新的 OOM/重启；回退值为 8。
- 约 10 分钟观察结果：`in_flight` 为 18–27/32，峰值 28，拒绝 0；内存约 364 MiB，Frontdoor healthy、重启 0、OOM false，公开健康入口均为 HTTP 200。

## 2026-09-05 并发上限调整实施记录

- 目标：在当前 Frontdoor 已恢复、宿主机无 OOM 且 4 vCPU/4 GiB Frontdoor 内存硬上限仍保持不变的前提下，将 `AETHER_GATEWAY_MAX_IN_FLIGHT_REQUESTS` 从 `32` 提高到 `64`，减少突发请求因本地并发闸门被打满而返回 503 的概率。
- 非目标：不修改 Caddy、Background、数据库、Redis、请求体上限、DNS 或公开路由；不移除 4 GiB `mem_limit` 和 `memswap_limit`。
- 影响：许可数翻倍会同时允许更多长请求、大请求和上游连接进入 Frontdoor；如果内存、CPU、Caddy 中断或 5xx 明显上升，回退到 `32`。
- 发布：先备份生产 Compose，校验 Compose 配置，再只执行 `docker compose up -d --no-deps --force-recreate frontdoor`。
- 验证：确认 `/_gateway/health` 的并发上限为 `64`，Frontdoor healthy，Background/Caddy 未重启，四个公开健康入口返回 200，并观察内存、OOM、重启和 5xx。
- 实施：已备份服务器 `/opt/niffler-app/docker-compose.yml`，仅修改 Frontdoor 环境变量并执行单服务重建；Caddy、Background、数据库和镜像未变。
- 结果：Frontdoor 新容器 healthy、restart=0、OOM=false；Background/Caddy 容器 ID 与重启次数未变化；`/_gateway/health` 显示 `limit=64`。
- 10 分钟观察：最高 `in_flight=1`、内存约 33 MiB、CPU 低于 0.2%；采样窗口内没有 `local_overloaded`、Caddy 中断、OOM 或重启。
- 观察期间仍有 8 个 `local_execution_runtime_miss`（`candidate_list_empty`）503；这不是并发闸门过载，需另行处理候选池问题。

## 回滚

- 本次变更的当前回滚：将 Frontdoor 的 `AETHER_GATEWAY_MAX_IN_FLIGHT_REQUESTS` 从 `64` 恢复为 `32` 后，仅重建 Frontdoor；保留 `mem_limit` 与 `memswap_limit`。
- 历史应急保护的回滚值为 `8`，只有重新评估整体流量和内存风险后才使用。
- 不得移除 4 GiB 硬上限，也不得只通过反复重启掩盖 OOM。

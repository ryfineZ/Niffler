# rn-hybrid PostgreSQL 无感切换

## 目标

- 将 Niffler PostgreSQL 15 从 `rn01` 迁移到 `rn-hybrid`。
- 使用 PostgreSQL 物理流复制保证计划内数据丢失为 0。
- 两台 Frontdoor 在主库提升期间继续接收请求；数据库操作由 PgBouncer 暂停并排队，不向用户返回数据库切换造成的 5xx。
- 保留当前模型流式请求，避免通过重启现有 Frontdoor 中断长连接。

目标角色是执行和复核生产数据库迁移的运维人员。这里的“无感”指切换期间请求可能增加数秒等待，但页面、API 和既有模型流不主动中断。

## 非目标

- 本次不迁移 Redis，不修改 Redis 地址、持久化或计费队列。
- 不把 Niffler 导入 `rn-hybrid` 现有 PostgreSQL 16；该实例继续只服务 session2sub2api。
- 不在迁库窗口升级 Niffler 应用、PostgreSQL 大版本、数据库结构或业务配置。
- 不修改用户、余额、价格、模型、路由、权限或 DNS。
- 不承诺故障状态下自动提升；本次只执行人工计划内切换。

## 当前状态

- `rn01`：PostgreSQL 15.18，数据目录约 56 GB，数据库约 55 GB，剩余磁盘约 20 GiB。
- `rn01`：`wal_level=replica`，10 个 WAL sender 和 10 个复制槽，当前没有副本或复制槽。
- `rn01`：`max_slot_wal_keep_size=-1`，建立复制前必须改为有限值，避免失效副本写满旧主机。
- `rn-hybrid`：6 vCPU、15 GiB 内存、约 445 GB 可用磁盘，现有 PostgreSQL 16 仅监听本机。
- `hd0526` 到 `rn-hybrid` 平均往返约 0.87 ms；OVH 到 `rn-hybrid` 约 25 ms。
- hd0526 与 OVH Frontdoor 的 PostgreSQL 连接池分别最多 10 条，当前获取连接超时为 5 秒。

## 目标架构

```text
hd0526 Frontdoor / Background ─┐
                               ├─ WireGuard ─> rn-hybrid PgBouncer :6432
OVH Frontdoor ─────────────────┘                    │
                                                   ├─ 切换前：rn01 PostgreSQL 15
                                                   └─ 切换后：rn-hybrid PostgreSQL 15

rn-hybrid PostgreSQL 15 standby ── WireGuard ──> rn01 PostgreSQL 15 primary
```

私网接口使用独立的 `wg-db`，不改现有 `wg0`：

| 主机 | WireGuard 地址 | 用途 |
|---|---:|---|
| rn-hybrid | `10.72.0.1/24` | PgBouncer、新 PostgreSQL、中心节点 |
| rn01 | `10.72.0.2/24` | 旧主库复制入口 |
| hd0526 | `10.72.0.3/24` | 主 Frontdoor 与 Background |
| OVH | `10.72.0.4/24` | 备用 Frontdoor |

`rn-hybrid` 只允许上述三台服务器的公网地址访问 WireGuard UDP 端口。新 PostgreSQL 5432 和 PgBouncer 6432 不开放到公网；PgBouncer 6432 只允许 `10.72.0.3` 与 `10.72.0.4`。rn01 现有 PostgreSQL 在应用改用 PgBouncer 前仍保留原公网监听，避免提前中断生产连接；两台 Frontdoor 和 Background 全部离开旧地址后，立即关闭该公网入口。

rn01 另建只绑定 `10.72.0.2:5432` 的 systemd TCP 转发入口，转到现有 `192.129.155.207:5432`。该入口只接受来自 rn-hybrid `10.72.0.1` 的连接，供 PgBouncer 和物理复制准备阶段使用；它不修改也不重建当前 PostgreSQL 容器。转发会终止来源地址透传，因此 PostgreSQL 端看到的来源是 rn01 本机 `192.129.155.207`；复制 HBA 以“专用复制角色 + 本机地址 + TLS/SCRAM”限制，外层再由 WireGuard peer 和 rn01 iptables 保证只有 rn-hybrid 能进入该转发口。

## PgBouncer 规则

- 使用 PgBouncer 1.22 的 `transaction` 池，不使用默认的 `session` 池。
- 显式设置 `max_prepared_statements=256`，覆盖 Niffler SQLx 100 条连接级语句缓存。
- 将 SQLx 启动包使用的 `extra_float_digits` 明确列入 `ignore_startup_parameters`；PgBouncer 1.22 默认拒绝未声明参数，而该项只影响浮点文本显示精度，不属于事务池必须跟踪的会话状态。
- 客户端和服务端均要求 TLS；认证使用现有数据库用户的 SCRAM verifier，不保存明文数据库密码。
- 应用自动数据库迁移保持关闭。结构迁移必须绕过 PgBouncer直连主库执行，完成后对 PgBouncer执行 `RECONNECT`。
- 当前应用即使关闭自动迁移，启动时仍使用 SQLx 的会话级 advisory lock 核对迁移清单；该锁不能直接跨事务池启动。hd0526 首次启动后需确认并释放空闲后端遗留锁。OVH Frontdoor 和 hd0526 Background 分别使用映射到同一物理数据库的独立别名 `aether_ovh`、`aether_background`：新进程首次启动前临时设为 `session`，健康后立即改回 `transaction`，并在尚未接流量或旧 Background 已停止时仅对对应别名执行 `KILL`、`RESUME`，强制应用进程在不重启的情况下重连事务池。正式切换必须同时暂停和更新 `aether`、`aether_ovh`、`aether_background` 三个别名。
- 切换时使用 `PAUSE aether` 等待现有事务结束并让新事务排队；不使用会等待 SQLx 会话断开的会话池暂停方式。

代理监听 `10.72.0.1:6432`，最大客户端连接 300，`aether` 后端最多 60，常规池 40、保留池 10，查询等待上限 120 秒。上线前必须使用真实 `niffler_app` 完成 TLS 登录、临时表读写、事务级 advisory lock、`pgbench -M prepared` 命名预处理语句和 `PAUSE → 查询进入等待 → RESUME` 测试；没有等待队列证据时不得视为无感切换能力已验证。

## 准备阶段

1. 保存四台主机的 WireGuard、防火墙、Compose、Caddy 和服务配置到 root 专用回退目录。
2. 更新并重启 rn-hybrid，验证 session2sub2api、Nginx 和现有 PostgreSQL 16 自动恢复。
3. 安装 Docker、Compose、PgBouncer、WireGuard、Fail2ban 和诊断工具；启用默认拒绝入站的防火墙，同时保留 SSH、80、443 和受限 WireGuard。
4. 建立 `wg-db`，逐对验证私网 ICMP、握手和允许端口；确认 rn-hybrid 的 5432/6432 未向公网开放。rn01 旧 5432 的公网关闭安排在应用全部改用代理之后。
5. 从 rn01 当前镜像摘要部署独立的 PostgreSQL 15；新容器使用独立数据目录、证书、端口绑定和至少 1 GiB 共享内存。
6. 在 rn01 新增专用物理复制用户与仅限复制入口的 HBA 规则；设置有限的 `max_slot_wal_keep_size` 并重新加载配置。
7. 使用 `pg_basebackup -R -X stream -C -S` 建立副本；连续检查接收、重放位置和复制槽状态。
8. 在 rn-hybrid 安装 R2 备份任务，完成上传、校验、重新下载和隔离恢复验证。

新实例固定使用 rn01 当前镜像摘要对应的 PostgreSQL 15.18，数据目录为 `/opt/niffler-data/postgres15`，宿主机只监听 `127.0.0.1:55432`，与现有 PostgreSQL 16 的 `127.0.0.1:5432` 完全隔离。容器共享内存为 1 GiB。复制账号只允许从 rn-hybrid 私网地址登录，复制槽 WAL 上限设为 8 GiB；副本失效时宁可重新做基础备份，也不能让 rn01 再次写满。

物理基础备份使用独立复制槽 `niffler_rn_hybrid`、TLS 和限速传输，完成后核对新旧 system identifier、接收与重放位置、复制槽状态及多轮延迟。新实例提升前不开放远程 5432；PgBouncer 切换后只经本机 `127.0.0.1:55432` 访问它。

R2 备份先从只读副本人工执行一次，服务固定使用 `niffler-postgres15` 容器。上传后必须重新下载 dump 和 SHA-256 文件，在 rn-hybrid 启动无端口、独立数据目录的 PostgreSQL 15.18 容器完成全量 `pg_restore`，并验证表、迁移、用户、API Key、Provider 与用量数据可读。验证结果写入 `/var/lib/niffler-backup/restore-verified.env`。恢复失败时记录阶段和详细日志、停止隔离容器并保留本次下载与数据目录，待查明后再显式清理；成功后自动清理。只有该流程成功后才启用 rn-hybrid 定时器并停用 rn01 定时器；旧备份文件和服务保留作回退，不删除。

## Frontdoor 代理入口切换

现有 Frontdoor 不直接重启：

1. 每台应用机复制一份权限为 `0600` 的旁路环境文件，只将 PostgreSQL 主机和端口改为 `10.72.0.1:6432`，并将连接获取超时临时提高到 30 秒。
2. 使用该节点当前正在运行的应用镜像启动 `niffler-frontdoor-next`，显式关闭数据库自动迁移。
3. 通过独立本机端口检查健康、首页、公开接口和未认证模型接口；确认 PgBouncer 日志与连接池正常。
4. Caddy 热重载到旁路 Frontdoor。旧 Frontdoor 保持运行，使旧配置上的既有长连接自然结束。
5. hd0526 Background 单独停止后改用 PgBouncer并恢复；该步骤不影响用户入口。

hd0526 使用当前镜像 `9f2959a28ae62d0ac28e48518557fa96218faf5f`。准备阶段实测发现 OVH 旧镜像
`d5ecd7aa316b96977a1799dbe6fb8a1a41bd14d9` 的内置迁移清单只到 `20260804120000`，而生产数据库已经成功应用
`20260809130000`；该旧镜像的新容器会以 `VersionMissing(20260809130000)` 拒绝启动，现有容器仅因迁移应用前已经启动而继续运行。因此 OVH 旁路实例必须使用与 hd0526 相同的精确镜像，不允许关闭迁移兼容检查或修改数据库迁移历史。镜像先按 ID 校验并仅启动旁路实例，旧 OVH 容器在旁路完全验证前保持运行；这次版本统一是修复已存在的数据库兼容性风险，不附带其他应用发布。

旁路环境从当前运行容器复制，保留 Redis、密钥和其余业务配置，只改
`DATABASE_URL`、`AETHER_DATABASE_URL`、`AETHER_GATEWAY_DATA_POSTGRES_URL` 三项的主机和端口；同时设置
`AETHER_GATEWAY_DATA_POSTGRES_ACQUIRE_TIMEOUT_MS=30000`、
`AETHER_GATEWAY_AUTO_PREPARE_DATABASE=false` 和
`AETHER_GATEWAY_DATA_POSTGRES_REQUIRE_SSL=true`。环境文件权限必须为 `0600`，检查输出不得包含连接凭据。

OVH 的三个 URL 还将数据库路径从 `/aether` 改为 PgBouncer 别名 `/aether_ovh`；新 Background 改为 `/aether_background`。PgBouncer 三个别名的后端 `dbname` 都是实际的 `aether`，因此不会连接到另一套数据库。

hd0526 旁路容器继续监听容器内 `8084`，仅将宿主机 `127.0.0.1:18086` 用于切换前验证；Caddy 热重载后改为同一 Docker 网络内的
`niffler-frontdoor-next:8084`。OVH 旁路容器继续监听容器内 `18084`，映射到宿主机
`127.0.0.1:18086`；主机网络模式的 Caddy 热重载后改为该地址。原 Frontdoor 不停止，Caddy 旧配置持有的既有流继续完成。若旁路健康、公开数据库接口、PgBouncer 客户端连接或 Caddy 校验任一失败，不切换该节点；热重载后失败则恢复已保存的 Caddyfile 并再次重载。

## 主库切换

只有所有验收门槛通过后才能执行：

1. 禁止发布和数据库结构迁移。
2. 暂停 hd0526 Background。
3. 在 PgBouncer执行 `PAUSE aether`；确认命令完成、全部服务端连接已释放，新客户端处于等待状态。
4. 确认 rn01 没有应用事务后，干净停止 `niffler-postgres`，以停止旧时间线继续产生写入。
5. 记录 rn01 最终 WAL 位置，确认 rn-hybrid 已接收并重放到相同位置。
6. 提升 rn-hybrid PostgreSQL 15，确认 `pg_is_in_recovery()` 为 false 且时间线增加。
7. 将 PgBouncer 的 `aether` 后端改为本机 PostgreSQL 15，执行 `RELOAD`、`WAIT_CLOSE aether` 和 `RESUME aether`。
8. 确认等待请求完成、没有新增数据库 5xx 后恢复 Background。
9. rn01 PostgreSQL 保持停止，禁止作为可写回退库启动。

## 验收

- 新主库版本仍为 PostgreSQL 15.18，`pg_is_in_recovery()` 为 false。
- 用户、API Key、余额、Provider、模型和最近用量记录的计数与关键校验一致。
- hd0526、OVH 两台 Frontdoor 和唯一 Background 均 healthy。
- 首页、认证接口、管理端只读接口、标准模型请求和计费结算成功。
- PgBouncer 没有等待队列积压、认证失败或预处理语句错误。
- 连续检查磁盘、内存、WAL、连接数、错误日志和接口至少两轮。
- R2 新备份上传和校验成功，隔离恢复关键表可读取。

## 回退边界

### 提升前

任何准备、复制、备份或代理验证失败，都停止新组件并让 Frontdoor 继续或恢复直连 rn01；旧主库没有停止，不需要数据回退。

### rn01 停止但新库尚未提升

如果副本无法达到最终 WAL，禁止提升。恢复 rn01 PostgreSQL、将 PgBouncer 后端保持为 rn01并 `RESUME`。

### 新库已经提升

rn01 与新时间线已经分叉，不能直接重启为主库。优先修复 rn-hybrid；需要回迁时必须从 rn-hybrid 反向复制或重新生成 rn01，不能仅修改连接地址。

## 影响范围

- rn-hybrid 系统维护重启会短暂影响本机 session2sub2api，重启后必须立即验证。
- 初始物理复制会增加 rn01 的磁盘顺序读取、网络和 WAL 保留量；设置复制槽上限并持续检查剩余空间。
- Frontdoor 首次改用 PgBouncer 后数据库网络多经过 rn-hybrid，hd0526 增加约 1 ms，OVH 增加约 1 ms，属于可接受范围。
- 正式提升期间数据库请求可能等待数秒，现有非数据库流式响应继续运行。

## 切换后的运行约束

- 当前承载流量的是 Compose 管理的稳定名称 `niffler-frontdoor` 和 `niffler-background`。两台 `niffler-frontdoor-next` 与 `niffler-background-next` 均已停止并设置 `restart=no`，只作短期回退证据保留。
- hd0526、OVH 的正式 Compose、环境文件和 Caddy 上游均已改为 rn-hybrid PgBouncer 与稳定名称服务；完整 `docker compose up -d` 已验证不会启动旧数据库或重复 Background。
- Caddy 使用单文件绑定时，宿主机文件和运行容器可能持有不同 inode；发布前后应使用切换脚本的 `sync` 模式热同步并核对校验值，不能只改宿主机文件。
- 应用只读启动校验仍使用会话级 advisory lock，因此三个 PgBouncer 应用入口固定为 `session`。不得只将代理改回 `transaction` 后继续复用现有应用连接；若以后恢复事务连接池，必须先修复应用只读检查并通过新连接验证。
- rn01 旧数据库控制文件为时间线 1，新主库为时间线 2；旧库不能直接启动回切。rn01 两个 PostgreSQL 私网 socket 和旧备份定时器均已 disabled/inactive，配置与数据只作证据和重建来源保留。

## 迁移收尾与日常发布

### 目标

- 让服务器正式 Compose 与当前生产状态一致，后续发布不再依赖迁移期间创建的 `*-next` 容器。
- 不修改应用业务代码，不把当前工作区内其他未发布功能带入生产。
- Frontdoor 接管期间不中断用户请求；Background 始终只有一个运行实例。
- 让应用通过 PgBouncer 正常重启，不再需要启动前后临时切换连接模式。

### 最终连接方式

迁移期间使用事务连接池，是为了在主库提升窗口暂停新事务并复用数据库连接。迁移完成后，当前应用的只读启动检查仍会使用会话级数据库锁；事务连接池可能让加锁和解锁落到不同后端连接。

收尾阶段将 `aether`、`aether_ovh`、`aether_background` 三个入口固定为 `session` 会话连接。每条应用数据库连接在存活期间固定使用同一条 PostgreSQL 连接，因此当前启动检查可以正常加锁和解锁，不需要修改或重新构建应用。

当前稳定运行上限为 hd0526 Frontdoor 10、OVH Frontdoor 10、Background 5，合计最多约 25 条应用连接。为了允许发布时新旧 Frontdoor 短暂并行，两个 Frontdoor 入口分别允许最多 20 条连接，切换窗口理论上限为 45 条。PostgreSQL 当前允许 100 条连接，切换前实测已有 21 条客户端连接，因此仍保留足够余量。以后增加较多 Frontdoor 实例或明显提高连接上限时，再单独修复应用只读检查并评估恢复事务连接池。

应用继续保持 `AETHER_GATEWAY_AUTO_PREPARE_DATABASE=false`。包含数据库结构变化的版本必须由单独的一次性迁移任务执行，不能依靠 Frontdoor 或 Background 启动时自动升级。

### 正式配置

- rn-hybrid 的最终 PgBouncer 配置保存在 `deploy/rn-hybrid/pgbouncer.ini`。
- hd0526 的正式应用配置保存在 `deploy/hd0526/docker-compose.yml` 和 `deploy/hd0526/Caddyfile`。
- OVH 的正式应用配置保存在 `deploy/ovh-primary/docker-compose.yml` 和 `deploy/ovh-primary/Caddyfile`。
- 数据库凭据只保存在服务器权限为 `0600` 的环境文件中，不进入 Git。
- hd0526 Frontdoor 使用 `aether`，Background 使用 `aether_background`；OVH Frontdoor 使用 `aether_ovh`。

### 接管顺序

1. 备份三台服务器的现有 PgBouncer、Compose、环境文件和 Caddy 配置。
2. 更新 PgBouncer 为会话连接并重新加载；确认三个入口读写正常且没有新错误。
3. 更新两台应用服务器的 Compose 和环境文件，只做配置校验，不重启当前 `*-next` 容器。
4. 分别启动 Compose 管理的稳定名称 Frontdoor，通过旧端口完成本机验证。
5. 热更新 Caddy：hd0526 从 `niffler-frontdoor-next:8084` 切到 `niffler-frontdoor:8084`，OVH 从 `127.0.0.1:18086` 切到 `127.0.0.1:18084`。
6. 等待旧 Frontdoor 连接归零后停止 `*-next` Frontdoor。
7. 预先创建 Compose 管理的 Background，停止 `niffler-background-next` 后立即启动正式 Background；失败时只运行其中一个并回退。
8. 执行完整 `docker compose up -d`，确认不会恢复旧数据库、旧镜像或重复 Background。

### 验证方式

- 两台稳定名称 Frontdoor 与唯一稳定名称 Background 均为 healthy。
- Caddy 当前配置和宿主机配置一致，且只指向稳定名称 Frontdoor。
- 三个 PgBouncer 入口均显示会话连接，等待队列为 0。
- PostgreSQL 没有残留 advisory lock，应用日志没有数据库连接、迁移或预处理语句错误。
- `niffler.org`、`api.niffler.org`、`us1.niffler.org`、`us2.niffler.org`、`ovh-origin.niffler.org` 以及公开数据库接口连续返回正常。
- 名字带 `*-next` 的容器停止并禁止自动重启，完整 Compose 启动后仍只有一个 Background。

### 日常发布

- 普通版本：构建固定版本镜像，两台服务器更新同一个镜像版本，先更新 OVH Frontdoor，再更新 hd0526 Frontdoor，最后更新唯一 Background。
- 包含数据库结构变化的版本：先备份并由单独任务完成数据库迁移，再按普通版本顺序更新应用。
- 发布前使用 `docker compose config --quiet` 校验配置；禁止使用会变化的 `main` 或 `latest` 作为生产镜像版本。
- 发布后检查健康状态、公开接口、Background 日志、PgBouncer 等待队列和数据库错误。

## 实施记录

- 2026-08-15：完成方案复审；确认 PostgreSQL 15 物理复制、PgBouncer 1.22 事务池和 SQLx 协议级预处理语句兼容路径可行。尚未开始修改生产服务器。
- 2026-08-15：rn-hybrid 完成系统更新、重启和两轮既有服务复核；四节点 `wg-db` 已启用并完成双向握手、ICMP 与 rn01 PostgreSQL 私网入口验证。尚未修改 Frontdoor 或提升数据库。
- 2026-08-15：完成 57,845,590 KiB 物理基础备份；rn-hybrid PostgreSQL 15.18 只读副本只监听本机 `127.0.0.1:55432`，新旧 system identifier 一致，复制槽 active，连续两轮 WAL 差为 0，八组关键表计数一致。尚未提升数据库或切换应用。
- 2026-08-15：新 R2 备份 `20260814T192002Z` 上传并完成远端大小、SHA-256 和全量隔离恢复验证；127 张公开表及用户、API Key、Provider、迁移和用量数据均可读取。rn-hybrid 定时器已启用，rn01 旧定时器已停用并保留实现。副本继续 streaming、WAL 差 0；尚未提升数据库或切换应用。
- 2026-08-15：hd0526、OVH 旁路 Frontdoor 已分别通过两轮直连和公网验证，Caddy 热重载完成，旧 Frontdoor 无存量上游连接；OVH 已修正旧镜像与现有迁移历史不兼容的潜在重启故障。唯一 Background 已迁入独立事务池别名并稳定运行，旧 Background 停止。三个 PgBouncer 别名均为 transaction，当前后端仍是 rn01；尚未停止旧主库或提升副本。
- 2026-08-15：三个代理别名暂停后，rn01 在 0 个活动事务下干净停止，最终 checkpoint 为 `118/CF8B8A0`。rn-hybrid 重放到 `118/CF8B918` 后提升到时间线 2，PgBouncer 三个别名切到本机 `127.0.0.1:55432` 并恢复。直连和三个别名写验证通过，关键计数一致，所有公开入口 200、切换后 5xx 为 0。旧主库、旧代理入口和旧应用容器均已停止并禁止自动重启；迁移完成。
- 2026-08-15：迁移收尾采用固定会话连接，不修改应用代码。PgBouncer 三个应用入口均改为 `session`，稳定运行上限约 25 条、Frontdoor 新旧并行发布窗口上限 45 条；PostgreSQL 最大连接数为 100。三个入口的会话锁、临时表和读写验证通过，代理无错误且等待队列为 0。
- 2026-08-15：代理重新加载后，迁移期间建立的旧事务池连接短暂出现预处理语句不存在警告；没有公开接口 5xx。两台正式 Frontdoor 随即以新会话连接启动并通过本机验证，Caddy 热更新后旧 Frontdoor 连接为 0，再停止并禁用自动重启。以后改变代理连接方式时必须让应用建立新连接，不能继续复用旧连接。
- 2026-08-15：正式 Background 在停止临时 Background 后启动，始终只有一个运行实例。hd0526 与 OVH 完整执行 `docker compose up -d`，Caddy 未重建，稳定名称 Frontdoor、唯一 Background 全部 healthy；两轮公开入口和数据库接口均为 200，新服务数据库错误和 5xx 为 0，PgBouncer 无等待，PostgreSQL 无残留 advisory lock，根分区可用约 387 GiB。迁移和部署收尾完成。

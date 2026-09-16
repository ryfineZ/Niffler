# ColoCrossing 生产 PostgreSQL 主库切换

## 目标

- 将 ColoCrossing 洛杉矶物理服务器设为 Niffler 生产 PostgreSQL 15.18 主库。
- 在 ColoCrossing 本机运行 PgBouncer；hd0526、OVH 的 Frontdoor 和 hd0526 唯一 Background 最终直接访问 `10.72.0.5:6432`。
- 将 rn-hybrid 从当前主库重建为 ColoCrossing 的异步物理从库。
- 使用在线物理复制缩短停写时间；计划内切换的数据丢失目标为 0。

## 非目标

- 本次不修改 DNS、Cloudflare、Caddy 路由或用户入口。
- 本次不升级 PostgreSQL 大版本，不修改业务表结构，也不发布应用代码。
- 本次不做自动故障提升。rn-hybrid 只提供可验证的从库和人工回退基础，不能在主库故障后自行转为可写。

## 最终架构

```text
hd0526 Frontdoor / 唯一 Background ─┐
                                    ├─ WireGuard ─> ColoCrossing PgBouncer 10.72.0.5:6432
OVH Frontdoor ──────────────────────┘                         │
                                                              v
                                              PostgreSQL 15.18 主库 127.0.0.1:55432
                                                              │
                                                              └─ 异步流复制 ─> rn-hybrid 从库

rn-hybrid 从库 ─> R2 备份与恢复验证
```

迁移完成后，rn-hybrid 不再中转日常数据库请求。它失效时不应影响应用到主库的正常连接。

## 节点与网络

| 节点 | WireGuard 地址 | 迁移后职责 |
|------|----------------|------------|
| ColoCrossing 洛杉矶 | `10.72.0.5` | PostgreSQL 主库、PgBouncer |
| rn-hybrid | `10.72.0.1` | PostgreSQL 异步从库、R2 备份与恢复验证 |
| hd0526 | `10.72.0.3` | 主 Frontdoor、唯一 Background |
| OVH Hillsboro | `10.72.0.4` | 备用 Frontdoor |

- PostgreSQL 和 PgBouncer 均不开放公网。
- `10.72.0.5:6432` 只允许 hd0526 和 OVH 访问。
- `10.72.0.5:55432` 只允许本机 PgBouncer及 rn-hybrid 的复制连接访问。
- 初始复制期间，rn-hybrid 通过受防火墙和专用复制角色双重限制的临时私网转发暴露当前主库；迁移完成后删除该入口。

## 已确认基线

- 当前主库：rn-hybrid，PostgreSQL 15.18，时间线 2，数据库约 66.4 GB。
- 当前应用：hd0526 Frontdoor、hd0526 唯一 Background、OVH Frontdoor 均健康，通过 rn-hybrid PgBouncer 访问数据库。
- 源库复制参数：`wal_level=replica`、10 个 WAL sender、10 个复制槽、`max_slot_wal_keep_size=8GB`、TLS 开启。
- 空间：rn-hybrid 可用约 381 GB，ColoCrossing 可用约 1.6 TB。
- 网络：hd0526 到 ColoCrossing 私网约 1.3 ms；OVH 到 ColoCrossing约 69.6 ms。

## 实际执行状态

- ColoCrossing 已于 2026-08-20 提升为生产主库，时间线为 3，数据库可写；旧 rn-hybrid 主库已干净停止并禁止自启。
- hd0526 Frontdoor、唯一 Background 和 OVH Frontdoor 已直接连接 ColoCrossing 本机 PgBouncer，容器健康且连接池无等待。
- rn-hybrid 的旧 PgBouncer 和临时复制转发已停止并禁用，不再承担应用数据库中转。
- 新主库已启用最终 HBA：rn-hybrid 只保留专用复制权限；`max_slot_wal_keep_size` 已设为 16GB。
- rn-hybrid 已从新主库重新完成物理基础备份，`pg_verifybackup` 校验通过；当前为只读异步从库，复制状态连续确认为 `streaming`，重放延迟为 0。
- rn-hybrid 的旧时间线数据目录已只读封存，旧主实例不会自动启动。
- ColoCrossing 与 rn-hybrid 已部署 PostgreSQL 角色监控，systemd 定时器和 Telegram 人工测试均通过；主库报告为可写，从库报告为只读 `streaming` 且待重放 0 字节。
- 迁移后首次全量 R2 备份 `20260819T182208Z` 在导出 `usage_body_blobs` 时被从库 WAL 回放冲突取消，失败通知已发送且临时文件已清理；主从复制和线上服务未受影响。
- rn-hybrid 专用备份从库的 `max_standby_streaming_delay` 已调整为 2 小时。备份期间监控仍检查恢复角色和 `streaming`，只暂缓预期的重放延迟告警；备份结束后恢复 16 MiB 阈值并要求追平。
- 修正后的完整备份 `20260819T184403Z` 已成功完成并上传 R2，大小为 35,557,713,090 字节；远端对象大小与 SHA-256 独立复核一致，Telegram 成功通知已投递，从库随后自动追平到待重放 0 字节。
- 当前数据库约 62 GiB，`usage_body_blobs` 总占用约 27 GiB。数据保留与归档策略已经成为迁移后的高优先级容量事项，但不影响本次主库切换验收。

## 已知硬件与运维风险

- 交付的 RAID 控制器是 LSI 9260-8i，订单写的是 9271-8i。
- 内核已确认 RAID 逻辑盘运行且写缓存启用，但 BBU/CacheVault 的保护状态尚未取得可信证据。
- 商家尚未提供 IPMI。操作系统或 SSH 完全失联时，无法自行查看控制台、重启或修复引导。

用户已决定先使用服务器，因此这些项目不阻止本次计划内迁移；但它们会增加断电写缓存和物理机失联后的恢复风险。R2 新备份、rn-hybrid 从库、数据库监控和继续向商家追索 IPMI/BBU 状态是必要补偿措施，不能省略。

## 实施顺序

### 1. 提升前准备

1. 备份 rn-hybrid、ColoCrossing、hd0526 和 OVH 的 WireGuard、防火墙、PostgreSQL、PgBouncer、Compose 与环境文件，权限保持 `0600`。
2. 在 rn-hybrid 人工生成一份新的 R2 备份，核对本地与 R2 对象大小和 SHA-256，并确认 Telegram 成功通知。
3. 在 rn-hybrid 创建迁移专用复制角色和复制槽；复制入口只允许 ColoCrossing 的 WireGuard 地址访问，使用 TLS 与 SCRAM。
4. 停止并清理 ColoCrossing 的 pgbench 候选实例数据，只清除测试数据，不接触生产源库。
5. 使用与生产完全相同的 PostgreSQL 15.18 镜像和 `pg_basebackup -R -X stream` 建立物理副本。
6. 连续确认 system identifier 一致、复制状态为 `streaming`、接收和重放 WAL 追平，并核对数据库大小和关键表计数。
7. 在 ColoCrossing 部署但暂不承载应用的 PgBouncer，使用与生产一致的三个数据库别名、TLS 和会话池配置。

### 2. 短暂停写和提升

当前应用长期使用会话池，直接执行 `PAUSE` 会等待持久连接断开。切换前先把 rn-hybrid 三个别名临时改为事务池并强制应用建立新连接；确认健康和无残留会话锁后再进入暂停。

1. 停止唯一 Background，避免后台任务继续写入。
2. 对 `aether`、`aether_ovh`、`aether_background` 执行 `PAUSE`，让新数据库请求在 PgBouncer 中等待，并确认源库活动业务事务为 0。
3. 干净停止 rn-hybrid 当前主库，记录最终 checkpoint、WAL LSN 和时间线。
4. 等待 ColoCrossing 接收并重放到旧主库最终 LSN；未追平时禁止提升。
5. 提升 ColoCrossing，确认 `pg_is_in_recovery=false` 且时间线增加。
6. 先让 rn-hybrid PgBouncer 临时转发到 ColoCrossing 主库并恢复等待请求，作为切换缓冲路径。
7. 将 hd0526、OVH 的三个应用连接逐个改为 `10.72.0.5:6432`，滚动重启并逐项确认健康；Background 最后启动，整个过程只允许一个 Background。
8. 应用全部直连 ColoCrossing 且 rn-hybrid PgBouncer 客户端归零后，停止其代理服务，消除中转依赖。

### 3. rn-hybrid 反向重建

1. 保留旧主数据目录为只读回退证据，并禁止旧实例自动启动。
2. 从 ColoCrossing 使用新的复制槽重新执行物理基础备份，不能直接启动已经分叉的旧时间线。
3. 以只读从库启动 rn-hybrid，确认 system identifier 一致、状态为 `streaming`、WAL 延迟为 0 或稳定在可接受范围。
4. 恢复并验证 R2 备份任务；备份结果、复制中断、磁盘和服务异常继续发送 Telegram 通知。

## 硬门槛与验收

- 新 R2 备份上传成功，大小与 SHA-256 一致。
- 初始副本连续多轮 `streaming`，最终停库后重放位置不小于旧主库最终 LSN。
- ColoCrossing 提升后直连写入、三个 PgBouncer 别名事务、TLS、连接池和应用健康检查全部通过。
- 用户、API Key、Provider、模型、订单、迁移记录和用量等关键计数与切换前一致，并能看到切换后新增用量。
- 两台 Frontdoor 和唯一 Background 健康；全部公开入口与关键接口连续返回成功，切换后无新增数据库 5xx。
- rn-hybrid 从库 `streaming`，旧主实例不会自动启动；R2 备份和 Telegram 通知通过一次人工验证。
- PostgreSQL、PgBouncer、复制延迟、磁盘和公网健康纳入现有监控。

## 迁移后监控

- ColoCrossing 节点除容器健康外，还必须确认 PostgreSQL 可写且不在恢复模式；角色异常连续达到既有失败次数后发送 Telegram 告警。
- rn-hybrid 节点除容器健康外，还必须确认 PostgreSQL 处于恢复模式、WAL 接收器为 `streaming`，并检查接收位置与重放位置的字节差。
- 数据库角色检查作为现有生产监控脚本的可选能力；未配置数据库角色的应用节点保持现有行为，不新增检查。
- 监控配置必须填写容器内 PostgreSQL 实际端口：ColoCrossing 主库为 `55432`，rn-hybrid 从库为 `5432`。脚本不得假定所有容器都使用默认端口。
- 复制延迟阈值由节点监控配置给出，默认 16 MiB。超过阈值视为异常，但不会自动提升从库或执行任何数据库写操作。
- 仅当配置的备份 systemd 服务处于 `active` 或 `activating` 时，可以暂不把重放字节数超过阈值判为故障；恢复角色错误或 WAL 接收器不是 `streaming` 时仍必须告警。备份服务结束后该豁免自动失效。

## 回退边界

### ColoCrossing 提升前

任一备份、复制、计数、网络或代理验证失败时，停止迁移组件，删除迁移复制槽和临时入口，应用继续使用 rn-hybrid。此时没有时间线分叉，不需要数据回退。

### rn-hybrid 已停止、ColoCrossing 尚未提升

如果副本不能达到旧主最终 LSN，禁止提升。重新启动 rn-hybrid 主库并恢复 PgBouncer，确认应用和 Background 健康后再分析原因。

### ColoCrossing 已提升

提升后两个节点时间线已经分叉。禁止把 rn-hybrid 旧数据目录直接启动为主库，否则会形成双主并产生不可自动合并的数据。优先修复 ColoCrossing；确需回迁时，必须从 ColoCrossing 反向同步或重新建立 rn-hybrid，再执行一次受控切换。

## 验证方式

- PostgreSQL：版本、时间线、system identifier、LSN、复制槽、`pg_is_in_recovery()`、关键计数和读写事务。
- PgBouncer：三个别名的 TLS 登录、会话池、等待队列、服务端连接、重连和错误日志。
- 应用：容器健康、唯一 Background、首页、认证接口、模型接口、健康接口和切换后 5xx。
- 运维：R2 对象大小/SHA-256、恢复状态、Telegram 通知、磁盘、服务自启动、防火墙和公网端口不可达。

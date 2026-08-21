# rn01 PostgreSQL 备份与恢复

## 目标

为 `rn01` 上的 Niffler PostgreSQL 创建可恢复的异地备份，并将备份保存到私有
Cloudflare R2 存储桶 `niffler-db-backups`。

目标角色是负责生产数据恢复的运维人员。判断备份成功必须同时满足：

1. PostgreSQL 一致性导出成功。
2. 本地备份结构检查成功。
3. 备份和 SHA-256 校验文件已上传 R2。
4. 从 R2 重新下载的文件校验一致。
5. PostgreSQL 15 隔离实例完整恢复成功，关键表能够读取。

## 非目标

- 本流程不切换数据库主从，不迁移生产数据库。
- 本流程不停止或重启生产 PostgreSQL。
- 本流程不在 `rn01` 上创建恢复测试数据库。
- 本流程不保存图片、视频或 HTTP 正文对象。

## 行为变化

- 首次备份使用 PostgreSQL 15 的自定义格式一致性导出。
- 备份先写入临时文件，完成 `pg_restore --list` 检查和 SHA-256 计算后再上传。
- R2 存储桶保持私有，不启用 `r2.dev` 公共地址或自定义域名。
- 生产主机只保存访问该存储桶所需的最小权限凭据。
- 自动任务每天创建一份完整备份；保留最近 7 份每日备份、4 份每周备份和
  6 份每月备份。清理操作只能删除已超过对应保留期限的备份对象。
- 人工停止、服务超时或系统关闭触发 `SIGTERM`/`SIGINT` 时，本次任务必须记为
  `failed`，并同时结束宿主机 `docker exec` 客户端和容器内 `pg_dump`。不得把空
  大小、空校验值的中止任务记为成功，也不得留下继续读取从库的孤立导出进程。

## 影响范围

- `pg_dump` 会增加数据库顺序读取、CPU 和网络使用，但不会锁住普通读写请求。
- 临时备份文件会占用 `rn01` 磁盘。执行前必须确认可用空间大于数据库当前体积
  加 10 GB；空间不足时必须停止，不得继续导出。
- 恢复验证只使用本机临时 Docker 容器和临时卷，不连接生产应用。
- R2 费用由备份实际压缩体积和保留数量决定。

### 从只读从库执行

当前自动备份由 rn-hybrid 只读从库执行。`pg_dump` 会在整个导出期间持有一致性
快照和表读取锁；如果主库同时产生需要独占表锁的 WAL，从库默认 30 秒后会为继续
回放 WAL 而取消导出。因此专用备份从库必须满足以下约束：

- `hot_standby_feedback=on`，减少清理旧行版本与备份快照冲突。
- `max_standby_streaming_delay=2h`，与备份服务的最长运行时间一致；备份期间优先
  完成一致性导出，主库继续正常读写。
- 备份服务运行时，监控仍必须确认节点处于恢复模式且 WAL 接收器为 `streaming`，
  但不因预期的重放积压告警；备份结束后立即恢复 16 MiB 延迟阈值。
- 主库复制槽仍限制 WAL 保留量。备份期间必须核对槽未失效；若业务写入增长导致
  保留 WAL 接近 16 GiB，应停止备份并重新评估，而不是无限保留 WAL。

该设置只适用于没有普通查询流量的专用备份从库，不应照搬到承担在线只读业务的
副本。备份结束后从库必须重新追平；未追平时不能视为备份收尾完成。

## 首次备份

备份对象使用以下目录：

```text
postgres/aether/daily/YYYY/MM/aether-YYYYMMDDTHHMMSSZ.dump
postgres/aether/daily/YYYY/MM/aether-YYYYMMDDTHHMMSSZ.dump.sha256
```

导出参数：

```text
--format=custom
--compress=6
--no-owner
--no-privileges
```

自定义格式由 `pg_dump` 创建一致性快照并压缩，恢复时使用 `pg_restore`。任何一步
失败都必须保留错误并停止后续清理，不能上传截断文件并标记成功。

## 恢复验证

恢复环境必须使用 PostgreSQL 15，且不得暴露公网端口。恢复时使用：

```text
pg_restore --exit-on-error --no-owner --no-privileges
```

恢复完成后至少验证：

- 数据库能够正常连接。
- 用户表数量与备份清单一致。
- `users`、`provider_api_keys`、`usage`、`usage_settlement_snapshots` 可读取。
- 恢复数据库大小合理，且 `pg_restore` 没有忽略错误。

## 凭据

- 本机凭据：`~/.config/domain-transfer/niffler-r2-backup.env`，权限 `0600`。
- 生产凭据必须放在 `/etc/niffler-backup/`，目录权限 `0700`、文件权限 `0600`。
- 凭据不得写入仓库、日志、备份文件名、进程输出或聊天记录。
- 令牌只能读写 `niffler-db-backups`，访问其他存储桶应返回拒绝。

## 验证方式

首次执行记录以下证据：

- 生产导出开始和完成时间。
- 备份压缩体积和 SHA-256。
- R2 上传后对象存在且大小一致。
- 从 R2 下载后的 SHA-256 一致。
- 隔离恢复命令返回成功及关键表读取结果。
- 测试完成后临时容器、卷和本地明文备份已删除。

## 自动任务

项目内文件：

- `scripts/rn01-postgres-backup.sh`
- `scripts/rn01-postgres-backup.service`
- `scripts/rn01-postgres-backup.timer`

生产安装位置：

- `/usr/local/sbin/niffler-postgres-backup`
- `/etc/systemd/system/niffler-postgres-backup.service`
- `/etc/systemd/system/niffler-postgres-backup.timer`

定时器每天北京时间 04:30 执行，并增加最多 10 分钟随机延迟。脚本通过文件锁防止
重复执行，使用较低 CPU 和磁盘优先级，状态写入
`/var/lib/niffler-backup/status.env`，详细日志写入 systemd journal。

检查命令：

```bash
systemctl status niffler-postgres-backup.timer
systemctl list-timers niffler-postgres-backup.timer
journalctl -u niffler-postgres-backup.service
cat /var/lib/niffler-backup/status.env
```

备份服务失败时，systemd 会启动
`niffler-postgres-backup-alert.service`，通过 Telegram Bot 向运维人员发送失败通知。
备份服务成功时，`ExecStartPost` 会发送成功通知。通知凭据位于
`/etc/niffler-backup/telegram.env`，权限必须为 `0600`。

状态文件表示最近一次备份尝试，不是 R2 中最后一份可恢复对象的索引。任务被中止时
状态应为 `failed`，但不会删除此前已经上传并校验成功的 R2 备份。中止行为至少验证：

- 服务退出码非 0，`ExecStartPost` 不发送成功通知，`OnFailure` 发送失败通知。
- 宿主机和 PostgreSQL 容器内均无残留 `pg_dump`，本地 `.partial` 文件已清理。
- 中止任务没有对应 R2 对象，定时器下一次运行时间仍然正确。

## 2026-08-20 从库冲突修正

ColoCrossing 提升为主库后，rn-hybrid 首次全量备份 `20260819T182208Z` 在导出
`usage_body_blobs` 时失败。PostgreSQL 明确报告 `canceling statement due to
conflict with recovery`，原因是默认 30 秒 WAL 等待不足；R2、磁盘和网络均不是
故障原因。失败任务已清理本地临时文件并成功发送 Telegram 失败通知。

修正后必须重新执行完整备份，不能把这次失败对象或迁移前备份作为迁移后验收结果。

修正后的完整备份 `20260819T184403Z` 已于 `2026-08-19 18:44:03 UTC` 启动，
并于 `20:28:36 UTC` 完成：

- R2 对象：`postgres/aether/daily/2026/08/aether-20260819T184403Z.dump`
- 大小：35,557,713,090 字节
- SHA-256：`67a82f41004401ef30eae2bb68d0a7f099a8b38d85e025e61bbd004526d6b0ca`
- `pg_restore --list`、R2 远端大小和远端校验文件复核：通过
- Telegram 成功通知：已投递
- 备份结束后从库：`streaming`，待重放 0 字节

R2 大对象完成多分片提交时第一次返回 501，`rclone` 内置重试第二次成功；最终对象
大小和 SHA-256 已再次独立读取并核对一致，因此本次结果为成功，但后续仍需观察
该 501 是否重复出现。

备份时数据库约 62 GiB，`usage_body_blobs` 总占用约 27 GiB，最终备份已增长到
35.6 GB。应尽快制定请求正文和审计数据的保留或归档策略，否则备份窗口、R2
存储量和恢复时间会继续快速增长。

成功备份完成后恢复定时器时，因恢复时刻仍处于 10 分钟随机延迟边界内，定时器在
`2026-08-19 20:40:01 UTC` 额外启动了重复任务 `20260819T204001Z`。该任务运行约
55 秒后被人工停止，没有上传 R2 对象，也没有留下本地截断文件。旧脚本在收到
`SIGTERM` 时错误写入空大小的 `success`，且容器内 `pg_dump` 没有随 systemd 主进程
退出；孤立进程已终止，状态文件已纠正为本次中止任务的 `failed`。

备份脚本现已增加精确的容器内 PID 文件和 `HUP/INT/TERM` 处理：中止时先结束本次
`pg_dump`，再回收 `docker exec` 客户端，最后以非 0 退出并清理临时文件。隔离回归
测试、生产容器最小信号测试及只读 `pg_dump --schema-only` 包装测试均通过。定时器
下一次执行时间为 `2026-08-20 20:37:32 UTC`；有效备份 `20260819T184403Z` 已再次从
R2 核对大小和 SHA-256，重复任务对应对象不存在。

## 2026-07-28 执行结果

首份备份：

- 对象：`postgres/aether/daily/2026/07/aether-20260727T162328Z.dump`
- 大小：1,459,124,818 字节
- SHA-256：`059705e60c37061461b12ac955c3f7ecbca28220224d142389e840918609e113`
- PostgreSQL 15.18 隔离恢复：成功
- 恢复结果：120 张 public 表，关键业务表可读取，无无效索引或未验证约束

自动任务真实运行结果：

- 对象：`postgres/aether/daily/2026/07/aether-20260727T165106Z.dump`
- 大小：1,459,812,843 字节
- SHA-256：`7276373c159414f2ca84116c1d195e2fb4fc94cbd1a56ad7fbf43724b8af3a23`
- systemd 结果：`success`
- 上传后对象大小和校验文件复核：通过
- 本地临时文件清理：通过

成功或失败信息会写入 `/var/lib/niffler-backup/status.env` 和 systemd 日志，并发送
Telegram 私人消息。Telegram API 暂时不可用时，原有备份状态和日志仍会保留；成功
通知发送失败不会改变备份任务本身的成功结果。

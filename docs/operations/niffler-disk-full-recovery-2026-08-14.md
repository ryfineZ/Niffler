# Niffler 数据库服务器磁盘写满应急恢复（2026-08-14）

## 目标

恢复 PostgreSQL、Redis、首页和模型 API，并停止完整请求/响应正文持续写入 Redis 用量队列和 PostgreSQL。

## 非目标

- 本次不删除 PostgreSQL 业务记录或计费记录。
- 本次不修改模型路由、价格、余额或用户权限。
- 本次不把 96 GB 数据库服务器继续作为长期容量方案。

## 行为变化

- 将系统配置 `request_record_level` 从 `full` 改回 `basic`。
- 继续记录请求归属、模型、令牌、费用、状态、错误和耗时；新请求不再保存完整请求体和响应体。
- Redis 暂时关闭 RDB 定时快照，继续使用 AOF；新增 6 GiB 应急 swap。两项都是临时运行状态，容器或主机重启前必须完成正式配置。

## 影响范围

- 管理端后续无法查看新请求的完整正文，图片链路诊断信息会减少。
- 已经进入 `usage:events` 的事件仍由 Background 处理，不直接删除 pending 计费事件。
- 已删除 14 个失败的 Redis 临时 RDB 快照、一个后续失效临时快照，以及一份已被更新 AOF 取代的旧 `dump.rdb`；PostgreSQL 数据和当前 AOF 均保留。

## 验证方式

- PostgreSQL 与 Redis 容器均为 healthy，且没有新的 OOM 或磁盘写满错误。
- 首页公开接口返回 200，未认证模型接口快速返回预期 401。
- 新增 `usage:events` 事件体积降到非正文级别，队列内存、AOF 和根分区使用率不再持续快速增长。
- 连续观察磁盘、Redis 内存、pending 数量和 PostgreSQL 数据库尺寸。

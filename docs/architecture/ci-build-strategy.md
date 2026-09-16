# CI 构建分层策略

## 目标

降低日常提交和上线构建耗时，避免每次改代码都构建当前线上不需要的平台产物。

## 非目标

- 不降低正式发版时的校验能力。
- 不删除多平台发版能力。
- 不改变线上服务运行方式。
- 不改变 Postgres/Redis 部署架构。

## 行为变化

- 日常 Rust CI 保留格式检查、Clippy、测试和 Postgres 数据库冒烟测试。
- PostgreSQL 数据库冒烟测试作为数据层的唯一 SQL 冒烟任务运行。
- Rust CI 的迁移版本断言必须与 `crates/aether-data/migrations/postgres/` 中的嵌入迁移同步；新增迁移时同步更新测试期望，避免用例因过期清单失败。
- Postgres 冒烟测试使用符合生产 schema 长度、非空字段和外键约束的 UUID 测试数据，并在测试结束时清理父子记录。
- 日常 Rust CI 中的 sccache 仅使用 runner 本地缓存；GitHub Actions 远端缓存不可用时不得阻断编译，编译结果仍由 `Swatinem/rust-cache` 负责跨任务复用。
- 应用镜像构建不再跟随 `main` 分支每次 push 自动运行，改为手动触发。
- 应用镜像构建只生成当前线上需要的 `linux/amd64` 版本，不再生成 `linux/arm64`。
- Docker 镜像只构建一次 `linux/amd64` 本地镜像，并导出为 `niffler-app-linux-amd64.tar`；不再重复构建并推送 GHCR。
- 正式 release 和 tunnel 多平台构建仍保留在标签或手动场景，不绑定日常上线。
- GitHub Actions 中官方 JavaScript action 必须使用 Node 24 运行时版本，避免 GitHub 停止 Node 20 后影响 CI 镜像产物和发版产物。

## 影响范围

- GitHub Actions 的 Rust CI。
- `aether-data` 的迁移版本测试和 PostgreSQL 候选记录冒烟测试。
- GitHub Actions 的应用镜像构建。
- 当前应用镜像发布不再产出 `ghcr.io/ryfinez/niffler:main`，线上通过 CI 产出的 tar 镜像文件发布。
- 不影响本地构建命令。
- 不影响正式 tag release 的多平台产物。

## 验证方式

- 使用 GitHub Actions 语法检查或实际触发一次手动应用镜像构建。
- 日常 push 后确认不会自动触发应用镜像构建。
- 确认 PostgreSQL 冒烟任务及其汇总门禁通过。
- 本地运行迁移版本定向测试，并在 Postgres 16 服务下运行候选准入重试测试。
- 检查 Rust CI 中 sccache 远端服务不可达时不会把缓存故障报告为编译失败。
- 手动应用镜像构建日志不再出现官方 action 运行在 Node 20 的弃用警告。

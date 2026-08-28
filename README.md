<p align="center">
  <img src="frontend/public/niffler-logo.svg" width="120" height="120" alt="Niffler Logo">
</p>

<h1 align="center">Niffler</h1>

<p align="center">
  <strong>一站式 AI 基础设施平台</strong><br>
  支持 Claude / OpenAI / Gemini 及其 CLI 客户端的统一接入、格式转换、正/反向代理, 致力于成为用户驱动AI服务的底座
</p>
<p align="center">
  <a href="#简介">简介</a> •
  <a href="#部署">部署</a> •
  <a href="#api-文档">API 文档</a> •
  <a href="#环境变量">环境变量</a> •
  <a href="#qa">Q&A</a>
</p>


---

## 简介

Niffler 是一个自托管的 AI API 网关，为团队和个人提供多租户管理、智能负载均衡、成本配额控制和健康监控能力。通过统一的 API 入口，可以无缝对接 Claude、OpenAI、Gemini 等主流 AI 服务及其 CLI 工具。

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/architecture/architecture-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/architecture/architecture-light.svg">
    <img src="docs/architecture/architecture-light.svg" width="680" alt="Niffler Architecture">
  </picture>
</p>

页面预览: https://ryfinez.github.io/Niffler/

## 部署

### Docker Compose（推荐：预构建镜像）

```bash
# 1. 克隆代码
git clone https://github.com/ryfineZ/Niffler.git
cd Niffler

# 2. 配置环境变量
cp .env.example .env
# 生成 JWT_SECRET_KEY / ENCRYPTION_KEY, 并填入 .env
./generate_keys.sh
# 编辑 .env 设置 ADMIN_PASSWORD

# 3. 首次部署 / 更新（Postgres + Redis）
docker compose pull && docker compose up -d
```

生产发布使用 CI 预构建镜像。推送到 `main` 后，GitHub Actions 会构建镜像并上传
`niffler-app-linux-amd64` 产物；执行 `scripts/deploy-ci-artifact.sh` 会把镜像文件传到服务器，
服务器只加载镜像并重启容器，不会编译 Rust、构建前端或执行 `docker build`。

### 一键安装（Docker Compose：Postgres + Redis）

```bash
cd Niffler
curl -fsSL https://raw.githubusercontent.com/ryfineZ/Niffler/main/install.sh | sudo bash
```

## 本地开发

依赖 Docker、Rust toolchain、Node.js 和 make。

```bash
make dev
```

Windows PowerShell 可使用同样的本地热开发流程：

```powershell
.\scripts\dev.ps1
```

该脚本只通过 Docker 启动 Postgres/Redis，Rust gateway 和前端均从本地源码运行。修改后端代码时只会增量编译，不需要重新构建 `aether-app` 镜像；按 `Ctrl+C` 会停止本次启动的本地进程。

`make dev` 会同时启动后端 `aether-gateway` 和前端 `frontend` 的 Vite dev server。需要单独启动时可使用 `make dev-backend` 或 `make dev-frontend`。
Postgres / Redis 本地依赖未就绪时，`make dev` 会自动执行 `docker compose up -d postgres redis`。

## Niffler Tunnel (可选)

Niffler Tunnel 是配套的正向代理节点，部署在海外 VPS 上，为墙内的 Niffler 实例中转 API 流量。

- Docker Compose 部署或下载预编译二进制直接运行
- 提供 macOS/Linux 与 Windows 一键脚本，自动下载最新 `tunnel-v*` 制品并向现有 `aether-tunnel.toml` 追加 `[[servers]]`
- 通过 `aether-tunnel setup` 完成交互式配置，自动注册为系统服务
- 详细文档见 [apps/aether-tunnel/README.md](apps/aether-tunnel/README.md)

## API 文档

- Embeddings: [OpenAI compatible `POST /v1/embeddings`](docs/api/embeddings.md)
- Rerank: [OpenAI/Jina compatible `POST /v1/rerank`](docs/api/rerank.md)

## 环境变量

- `APP_PORT`：`aether-gateway` 唯一监听端口，固定绑定 `0.0.0.0:${APP_PORT}`
- `DATABASE_URL`：Postgres 数据库连接串，例如 `postgresql://postgres:aether@postgres:5432/aether`
- `REDIS_URL`：Redis 连接串；Docker Compose 部署需要配置
- `AETHER_RUNTIME_BACKEND=memory|redis`：运行时缓存/协调后端；Docker Compose 默认使用 `redis`
- `AETHER_GATEWAY_AUTO_PREPARE_DATABASE`：常规启动前自动执行挂起的 schema migration 和 backfill；仓库自带的 `docker-compose.yml` 默认开启
- `JWT_SECRET_KEY` / `ENCRYPTION_KEY`：认证和敏感数据加密所需密钥
- `API_KEY_PREFIX`：用户和管理员新建 API Key 时使用的前缀，默认 `sk`
- `ADMIN_USERNAME` / `ADMIN_PASSWORD` / `ADMIN_EMAIL`：首次启动时自举首个本地管理员；`install.sh` 会提示输入管理员密码
- `CORS_ORIGINS` / `CORS_ALLOW_CREDENTIALS`：前端跨域来源控制；如果要跨域带登录 Cookie，`CORS_ORIGINS` 不能写 `*`
- `RUST_LOG`：Rust 日志过滤，例如 `aether_gateway=info`、`aether_gateway=debug,sqlx=warn`
- Docker Compose 的 `DB_PASSWORD` / `REDIS_PASSWORD` 默认使用 `aether`

### DoDoPay 支付

管理员在后台 `支付配置` 中启用 DoDoPay，并保存服务地址、App ID、App Secret、回调站点根地址、汇率和最低充值金额后，钱包充值和套餐购买会出现微信 DoDoPay 支付入口；支付宝入口暂不开放。

App Secret 用于创建 DoDoPay 收银台和验证 DoDoPay 回调，会使用系统加密密钥保存到数据库，不应写入代码仓库。

---

## 许可证

本项目采用 [Niffler 非商业开源许可证](LICENSE)。允许个人学习、教育研究、非盈利组织及企业内部非盈利性质的使用；禁止用于盈利目的。商业使用请联系获取商业许可。

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=ryfineZ/Niffler&type=Date)](https://star-history.com/#ryfineZ/Niffler&Date)

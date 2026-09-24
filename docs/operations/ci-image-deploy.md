# CI 镜像发布流程

## 目标

生产发布不再在服务器上编译 Rust、构建前端或执行 `docker build`。应用镜像由 GitHub Actions 构建，发布时服务器只负责加载镜像并重启容器。

生产发布必须保证待上线提交就是受保护 `main` 的准确提交，并且包含当前线上提交，防止从落后的功能分支发布时静默删除已经上线的功能。生产环境不再要求人工审批。

是否允许上线、迁移是否兼容、容器是否健康以及失败后是否回退，都由生产主机
`/opt/niffler-release/bin/deploy-production` 的固定部署器判断。固定部署器位于应用
目录之外，不能由待发布分支替换。

## 非目标

- 不改变 Postgres、Redis 的部署方式。
- 不把 GitHub、GHCR 或服务器密钥写入仓库。
- 不要求服务器登录 GHCR。
- 不要求服务器安装 Rust、Node.js 或前端依赖。
- 不阻止管理员执行明确回滚；回滚必须显式使用 `--allow-rollback`。
- 不把个人 SSH 私钥保存到 GitHub Actions。

## 行为变化

- `test` 更新后自动运行 `Build App Image` 并部署测试环境；生产镜像仍只为准确的
  `main` 提交手动触发，生产不会因 `main` 推送自动上线。
- 当前线上只使用 Linux amd64，因此 `Build App Image` 只构建 amd64 的 `aether-gateway`。
- `Build App Image` 只产出 `niffler-app-linux-amd64` 镜像文件，不再推送 GHCR 镜像，避免重复构建和上传。
- `test` 部署任务通过 `actions/download-artifact` 直接取得同一 Workflow 生成的镜像，
  不查询尚未完成的当前 Run 元数据；生产发布仍按 Run ID 或提交号下载已完成的成功产物。
- `Build App Image` 使用 Node 22 运行时的官方 GitHub Actions，避免 CI 运行时升级影响生产镜像产物。
- `Build App Image` 在封装镜像前运行发布脚本语法检查和提交继承规则测试；检查失败时不生成生产镜像。
- `deploy.sh` 不再使用 `Dockerfile.app.local`，也不再计算代码哈希。
- `deploy.sh` 只执行镜像拉取和 `docker compose up -d --no-build`。
- `scripts/deploy-ci-artifact.sh` 负责校验并上传准确的 CI 镜像；test 使用 Workflow 已下载的
  本地产物，生产按已完成 Run 下载。实际服务变更仍由服务器固定部署器执行。
- 生产执行 `scripts/deploy-ci-artifact.sh` 必须显式传入 `--run-id` 或 `--commit`，不能默认部署“最新成功产物”。
- 使用 `--commit` 时，脚本会按提交号查找对应的成功 `Build App Image` 工作流；如果没有找到成功产物，脚本必须停止，不能退回到默认分支的最新产物。
- 脚本会读取 CI 运行对应的准确提交，确认工作流名称和结果，并从 GitHub 同步最新 `main`。
- 正常发布要求待上线提交等于远端 `main`，并同时包含当前线上提交；任一条件不满足时停止。
- 当前线上提交优先从服务器的 `.niffler-deployed-commit` 读取；旧部署没有该文件时，从当前镜像的版本标签或提交标签识别。
- 首次部署没有现有应用镜像时，只检查待上线提交是否包含最新 `main`。
- 明确回滚必须额外传入 `--allow-rollback`，跳过提交继承检查；不能将该参数用于普通发布。
- 固定部署器从公开 Git 远端自行同步 `main`，不信任客户端传入的主线提交号。
- 镜像提供只读 PostgreSQL 迁移兼容检查入口。固定部署器使用当前
  `frontdoor` 容器的实际环境、网络和只读挂载运行目标镜像，由目标镜像直接检查生产
  `_sqlx_migrations`；数据库可以是外部 PostgreSQL，不要求同一 Compose 项目中
  存在名为 `postgres` 的服务。
- 生产迁移记录为脏状态，或目标镜像缺少任一已执行版本时，必须在重启容器前停止。
- 固定部署器保留当前镜像标签，使用准确提交标签重建服务并通过 `docker compose up --wait` 等待健康。
- 新服务未按时健康、源站健康请求失败或公开健康请求失败时，固定部署器自动恢复旧镜像和旧服务。
- 只有新服务全部健康后，服务器才写入 `.niffler-deployed-commit`，记录本次实际上线的完整提交号。
- CI 镜像写入 `org.opencontainers.image.revision` 标签，便于服务器在状态文件缺失时恢复当前线上提交。
- `--allow-latest-for-local` 只允许本地验证或临时排查使用，不能作为生产发布命令。
- 所有合并请求都运行 Rust、前端和发布工具检查，`main` 只接受这三组检查通过的
  合并请求。

## 测试与生产发布链

```text
g-dxw 功能分支
       |
       v
PR -> ryfineZ/Niffler:main -> 手动构建准确 main 镜像
                                      |
                         production 环境无需人工审批
                                      |
                                      v
                                  生产环境

test 分支和测试环境仍可用于发布前验证，但不是进入 `main` 或生产环境的必经步骤。
```

- `ryfineZ/Niffler` 是唯一发布源；个人 fork 只用于开发分支和发起合并请求。
- `test` 推送仍会自动构建准确提交镜像并部署测试环境，供需要时验证。
- 同一时间的 `test` 发布按提交顺序排队，不能取消正在执行的远端部署，避免 SSH 中断让
  测试环境停在半更新状态。
- 测试发布只接受当前上游 `test` 的准确提交，并调用服务器上 root 所有、不可由待部署
  分支替换的 `/opt/niffler-test/bin/deploy-test`。测试环境使用独立 PostgreSQL；已有
  `app` 容器时，部署前从该容器读取实际数据库环境并执行迁移兼容检查。首次部署没有
  `app` 容器时，只允许测试流程启动 PostgreSQL 和 Redis，并按测试 `.env` 执行同一项
  兼容检查。两种情况都会验证镜像提交标签、Compose 健康状态、源站健康地址和公开健康地址；
  已有版本的部署失败时自动恢复旧镜像。
- `main` 合并后仍按受保护生产流程手动构建、手动触发，无需另一名维护者批准，不自动上线。

### 一次性测试服务器准备

这部分由能够通过 VPS 控制台、VNC 或已获授权管理员密钥登录 `test01` 的维护者完成。
如果服务器只接受公钥登录，而当前管理员密钥不可用，必须先在供应商控制台恢复管理员入口；
不能临时打开公网密码登录作为替代方案。

1. 使用独立的 Actions Ed25519 密钥，不得复用个人或 root 登录私钥。当前测试密钥的
   公钥位于本机 `~/Workspace/Projects/vps_nodes/niffler-test-deploy.pub`；只将该公钥授权给
   测试部署账号 `niffler-test-deploy`，私钥只保存到 GitHub `test` Environment。
2. 在受信任的 `main` 工作区准备好 `docker-compose.yml` 和测试专用 `.env`，并将本机
   `niffler-test-deploy.pub` 上传或通过服务器控制台写入
   `/tmp/niffler-test-deploy.pub` 后，以 root 执行以下初始化。`.env` 必须使用独立的
   PostgreSQL、Redis、JWT 和加密密钥，不能复制生产值；测试应用使用默认端口 `8084`。

   ```bash
   install -d -o root -g root -m 0755 /opt/niffler-test/bin
   id -u niffler-test-deploy >/dev/null 2>&1 || \
     useradd --create-home --shell /bin/bash niffler-test-deploy
   passwd -l niffler-test-deploy
   usermod -aG docker niffler-test-deploy

   install -d -o root -g root -m 0755 /home/niffler-test-deploy/.ssh
   install -o root -g root -m 0644 \
     /tmp/niffler-test-deploy.pub \
     /home/niffler-test-deploy/.ssh/authorized_keys

   install -d -o root -g niffler-test-deploy -m 2770 \
     /opt/niffler-test /opt/niffler-test/logs /opt/niffler-test/.release
   printf 'niffler-test-v1\n' > /opt/niffler-test/.niffler-test-environment
   chown root:root /opt/niffler-test/.niffler-test-environment
   chmod 0444 /opt/niffler-test/.niffler-test-environment
   install -o root -g niffler-test-deploy -m 0660 \
     docker-compose.yml /opt/niffler-test/docker-compose.yml
   install -o root -g niffler-test-deploy -m 0660 \
     .env /opt/niffler-test/.env
   install -o root -g root -m 0755 \
     scripts/fixed-production-deployer.sh /opt/niffler-test/bin/deploy-test
   ```

3. 测试 `.env` 至少应明确设置 `COMPOSE_PROJECT_NAME=niffler_test` 和
   `APP_IMAGE=niffler-app:latest`，并让 `postgres`、`redis` 和 `app` 使用完全隔离的测试
   数据。`APP_PORT` 必须与该主机 HTTPS 反向代理的本地上游端口相同；当前测试主机已经由
   Nginx 对外提供 `https://niffler-test.123.253.224.101.sslip.io`，原部署约定为
   `APP_PORT=18084`。恢复管理员访问后，先用 `nginx -T` 核实实际 upstream，再同步设置
   `.env` 的 `APP_PORT` 和 GitHub Variable `MYLINGWEAVE_SOURCE_HEALTH_URL`。不要删除或
   改写 `.niffler-test-environment` 标记文件；工作流会在首次部署前检查它，以防错误地把
   测试部署指向生产目录。首次 Actions 部署会自行启动 PostgreSQL、Redis 和 `app`；不需要
   预先手工拉取或启动应用镜像。
4. 在创建 `test` 分支前，必须验证 HTTPS 反向代理持续监听公网 `80` 和 `443`，并将
   `niffler-test.123.253.224.101.sslip.io` 转发到与 `APP_PORT` 相同的本地地址。当前主机
   已检测到 Nginx 返回该域名的健康检查 200，不能在未核对现有配置前再额外安装或替换为
   Caddy。证书签发和反向代理必须可用；否则部署器的 HTTPS 公网健康检查会拒绝本次部署。
5. `niffler-test-deploy` 可以操作这个专用测试 Compose 项目和 Docker，因此该账号只能用于
   独立测试主机，不能复用于生产主机。虽然普通文件权限会阻止它直接改写 root 所有的
   `/opt/niffler-test/bin/deploy-test`，Docker 组本身等同于主机管理员权限；因此该固定
   部署器是防止日常误改的运行约束，不是针对测试密钥泄露的安全边界。先验证公钥认证和
   目录权限：

   ```bash
   ssh -i ~/Workspace/Projects/vps_nodes/niffler-test-deploy \
     niffler-test-deploy@123.253.224.101 \
     'docker compose version && test -w /opt/niffler-test/.release'
   curl -fsS https://niffler-test.123.253.224.101.sslip.io/_gateway/health
   ```

6. 独立保存以下信息，交给仓库管理员配置：
   - 主机：`123.253.224.101`（SSH 端口为默认的 `22`，Secret 中不要附加端口）
   - 用户：`niffler-test-deploy`
   - 目录：`/opt/niffler-test`
   - 源站健康地址：`http://127.0.0.1:18084/_gateway/health`（恢复管理员访问后以 Nginx
     实际 upstream 为准）
   - 计划公网地址：`https://niffler-test.123.253.224.101.sslip.io`
   - ED25519 主机指纹：
     `SHA256:jyUey+3oSoZHEdiApa8gKRRlKDyLsDorjCPPRAIaILw`

### 一次性 GitHub 管理员配置

仓库所有者完成以下设置后，再创建长期 `test` 分支：

1. 创建 `test` Environment，不要求人工审批，只允许 `test` 分支部署。
2. 在 `test` Environment 中配置 Secret：
   - `MYLINGWEAVE_HOST` = `123.253.224.101`
   - `MYLINGWEAVE_SSH_KEY` = 独立 Actions Ed25519 私钥的完整内容
   - `MYLINGWEAVE_SSH_HOST_FINGERPRINT` =
     `SHA256:jyUey+3oSoZHEdiApa8gKRRlKDyLsDorjCPPRAIaILw`
3. 在同一个 Environment 中配置 Variable：
   - `MYLINGWEAVE_USER` = `niffler-test-deploy`
   - `MYLINGWEAVE_REMOTE_DIR` = `/opt/niffler-test`
   - `MYLINGWEAVE_SOURCE_HEALTH_URL` =
     `http://127.0.0.1:18084/_gateway/health`
   - `MYLINGWEAVE_PUBLIC_URL` =
     `https://niffler-test.123.253.224.101.sslip.io`
4. 创建保护 `test` 的 Ruleset：禁止删除和强制推送，要求合并请求、最少批准数为 0、
   解决全部对话，并严格要求 `check`、`Frontend`、`Release tooling` 三项检查。
5. `production` Environment 不配置 required reviewers，仅允许受保护分支部署。

首次落地顺序为：合并流水线修改到 `main`，通过服务器控制台或已有管理员密钥完成
测试账号、Compose 配置、固定部署器和专用密钥准备，完成上述管理员配置；需要测试时
再向 `test` 推送并验证，生产发布直接从准确的 `main` 提交手动触发。

## 主线保护

`main` 的必过检查为：

- `check`：Rust 格式、静态检查、测试和 PostgreSQL 数据库冒烟测试的汇总结果；
- `Frontend`：前端生产依赖审计、类型检查、测试和构建；
- `Release tooling`：发布脚本语法和固定部署器行为测试。

这些工作流不使用路径过滤，因此任意合并请求都会返回明确结果，不会因“未触发”
而让必过检查一直等待。`main` 禁止强制推送和删除，所有对话必须解决后才能合并。
当前只允许普通合并提交，避免压缩合并或变基合并破坏生产链整合所需的祖先关系。

按仓库所有者于 2026-09-16 确认的规则，`main` 最少批准数固定为 0，不要求其他
维护者批准，也不因协作者人数变化自动恢复审核要求。仍保留合并请求、全部必过检查、
对话解决和禁止强制推送/删除的保护；现有仓库所有者豁免不变。修改后通过 GitHub
规则 API 回读 `required_approving_review_count=0`，并检查其他规则未变。

## 影响范围

- GitHub Actions 主应用镜像构建流程只产出 amd64 镜像文件。
- 线上发布使用 CI 产出的镜像文件，不依赖服务器访问私有 GHCR。
- 服务器 `.env` 中的 `APP_IMAGE` 应设置为 `niffler-app:latest`，由 `docker load` 后的本地镜像提供。
- 服务器应用目录新增 `.niffler-deployed-commit` 状态文件，不改变数据库、Redis 或业务数据。

## GitHub 受保护生产环境

### 目标与非目标

GitHub `production` 环境负责保护专用 SSH 凭证，并在工作流校验通过后调用服务器已有的
固定部署器。它不替代固定部署器的主线、迁移、健康和回退判断。

本阶段不执行以下操作：

- 不在每次 `main` 推送后自动发布；
- 不允许拉取请求或任意分支接触生产 Secret；
- 不上传或复用管理员个人 SSH 私钥；
- 不让 Actions SSH 用户加入 Docker 组或获得任意 root 命令；
- 不把自托管 Runner 安装到生产主机；
- 不从 GitHub 工作流提供普通发布之外的回滚入口。

### GitHub 环境保护

`production` 环境必须同时满足：

- 只允许 `main` 分支部署；
- 不配置 required reviewers，不要求人工批准部署；
- SSH 私钥、主机地址和主机密钥指纹保存为环境 Secret；
- SSH 用户和端口保存为环境 Variable；
- 生产 Job 只声明 `contents: read` 和 `actions: read`；
- 生产 Job 使用的第三方 Action 固定到经过核对的完整提交 SHA。

环境 Secret 会在生产 Job 启动时提供给 Runner。工作流仍必须显式检查
`github.ref == 'refs/heads/main'`，不能只依赖环境名称。

### 服务器最小权限边界

生产主机新增 `niffler-deploy` 用户。该用户：

- 不属于 Docker 组；
- 不能读取 `/opt/niffler-app/.env` 或 `docker-compose.yml`；
- 没有普通 sudo 权限；
- 只能通过专用 Ed25519 公钥登录；
- `authorized_keys` 使用 `restrict` 和固定命令，禁止交互 Shell、PTY、端口转发、
  Agent 转发和 X11 转发。

专用用户的 home 和 `.ssh` 目录由 `root` 所有并使用 `0755`，`authorized_keys`
由 `root` 所有并使用 `0644`。OpenSSH 会以目标用户身份读取授权文件，因此目录和
公钥文件必须可遍历、可读；`root` 所有权保证专用用户仍不能替换授权文件。只有
`uploads` 子目录由 `niffler-deploy` 所有并使用 `0700`。

固定 SSH 命令只接受三种协议：

```text
status
upload <40 位小写提交号>
deploy <40 位小写提交号>
```

- `status` 只返回 `.niffler-deployed-commit`；
- `upload` 从标准输入接收镜像文件，写入用户专属目录，限制最大文件大小并返回
  SHA-256；
- `deploy` 只允许通过 sudo 调用 root 所有的固定包装器。

root 包装器只接受 `status` 和 `deploy <提交号>`。执行部署前必须验证：

- 调用者是 `niffler-deploy`；
- 提交号格式正确；
- 镜像是专用目录中的普通文件，不是符号链接；
- 文件所有者、权限和真实路径符合预期；
- 镜像复制到 root 所有的发布临时目录后，再交给
  `/opt/niffler-release/bin/deploy-production`；
- 远端目录、服务名、状态文件和健康地址全部使用固定值，Actions 不能传入。

专用 SSH 密钥即代表“允许发布经过审核的 `main` 镜像”，仍属于高权限生产凭证。
如果 Secret、维护者权限或工作流存在泄露迹象，必须同时删除服务器公钥和 GitHub
环境 Secret，再生成新密钥；不能只修改其中一侧。

### 工作流

生产发布使用独立的手动工作流，不在镜像构建尚未结束时发布：

1. 为准确 `main` 提交运行并等待 `Build App Image` 成功；
2. 从 `main` 手动触发 `Deploy Production`，输入同一准确提交号；
3. Job 进入 `production` 环境，无需人工审批；
4. Runner 核对 SSH 主机密钥指纹；
5. 发布脚本查找该提交的成功镜像工作流，验证当前生产提交继承关系；
6. 通过受限 SSH 协议上传镜像并调用固定部署器；
7. 固定部署器完成镜像、迁移、健康和自动回退检查；
8. 工作流结束后删除 Runner 上的私钥和临时文件。

同一时间只允许一个生产发布，后发任务不能取消正在执行的生产发布。

### 验证要求

- 非 `main` ref 触发时，生产 Job 必须在读取 Secret 前停止；
- 不受允许分支策略信任的 ref 不得获得生产环境 Secret；
- 错误主机密钥指纹必须停止连接；
- 固定 SSH 命令拒绝空命令、未知命令、额外参数和非法提交号；
- 上传拒绝超限文件、路径逃逸和符号链接；
- root 包装器拒绝非专用调用者、错误所有者和不符合规则的上传文件；
- 专用用户不能执行 Docker 命令、读取生产配置或运行其它 sudo 命令；
- 工作流只能部署已有成功 `Build App Image` 的当前 `main` 准确提交；
- 部署后仍按本文既有要求执行两轮生产健康检查。

## 发布方式

使用 CI 镜像文件发布。以 hd0526 为例：

```bash
APP_SERVICES="frontdoor background" \
APP_IMAGE=niffler-app:latest \
GH_REPO=ryfineZ/Niffler \
./scripts/deploy-ci-artifact.sh \
  --host hd0526 \
  --remote-dir /opt/niffler-app \
  --commit <git-commit-sha>
```

这个脚本会下载指定提交对应的 `Build App Image` 工作流产物并传到服务器，
随后调用固定部署器。固定部署器重新确认远端 `main`、当前生产提交、镜像标签，
并让目标镜像使用当前 `frontdoor` 的实际数据库环境完成只读迁移兼容检查后，才
重启 `frontdoor` 和 `background`。Postgres 和 Redis 不需要重启。

如果指定提交没有成功的 `Build App Image` 工作流、不是远端 `main`、没有包含
当前线上提交或缺少生产已执行迁移，发布会直接停止。需要先完成分支合并并重新触发 CI。

如果已经知道 GitHub Actions run id，也可以使用：

```bash
APP_SERVICES="frontdoor background" \
APP_IMAGE=niffler-app:latest \
GH_REPO=ryfineZ/Niffler \
./scripts/deploy-ci-artifact.sh \
  --host hd0526 \
  --remote-dir /opt/niffler-app \
  --run-id <github-actions-run-id>
```

本地验证或临时排查时，才可以显式选择最新成功产物：

```bash
./scripts/deploy-ci-artifact.sh \
  --host <test-host> \
  --allow-latest-for-local
```

只有明确回滚到旧版本时才能使用：

```bash
APP_SERVICES="frontdoor background" \
APP_IMAGE=niffler-app:latest \
GH_REPO=ryfineZ/Niffler \
./scripts/deploy-ci-artifact.sh \
  --host hd0526 \
  --remote-dir /opt/niffler-app \
  --run-id <github-actions-run-id> \
  --allow-rollback
```

回滚仍要求指定的 CI 工作流成功且镜像产物存在，只跳过“必须包含最新 `main` 和当前线上提交”的检查。

## 验证方式

- `bash -n deploy.sh`
- `bash -n scripts/deploy-ci-artifact.sh`
- `bash -n scripts/fixed-production-deployer.sh`
- `bash scripts/tests/test-actions-production-access.sh`
- `bash scripts/tests/test-deploy-ci-restricted.sh`
- `bash scripts/tests/test-production-workflow.sh`
- `bash scripts/tests/test-verify-ssh-host-key.sh`
- `bash scripts/tests/test-deploy-ancestry.sh`
- `bash scripts/tests/test-fixed-production-deployer.sh`
- 不传 `--run-id`、`--commit` 和 `--allow-latest-for-local` 时，`scripts/deploy-ci-artifact.sh` 必须拒绝执行。
- 普通发布缺少最新 `main` 或当前线上提交时必须在下载镜像前停止。
- `--allow-rollback` 只跳过提交继承检查，不跳过工作流名称、成功状态和镜像产物检查。
- `Build App Image` 的 `Verify deployment tooling` 任务通过后才运行最终镜像封装。
- GitHub Actions 的 `Build App Image` 工作流成功，且日志不再出现官方 action 运行在 Node 20 的弃用警告。
- 正常发布目标不等于远端 `main` 时，固定部署器必须拒绝执行。
- 数据库部署在 Compose 项目之外时，固定部署器仍必须通过目标镜像完成只读迁移
  兼容检查，不能依赖 `docker compose exec postgres`。
- 目标镜像缺少生产已执行迁移时，固定部署器必须在重建容器前拒绝执行。
- 新容器健康失败时，固定部署器必须恢复旧镜像，且部署状态文件保持旧提交。
- 服务器执行发布脚本后，`docker compose ps` 显示应用容器健康，状态文件、
  镜像标签和 `origin/main` 为同一提交。

## OVH 独服发布适配（2026-09-24）

目标：在当前 `/opt/niffler-candidate` 生产目录发布准确 main 镜像。只变更应用服务，不迁移数据库或修改 TLS 验证级别。迁移预检继承活动容器的挂载并强制只读，使 PostgreSQL CA 文件可用；部署配置使用 APP_IMAGE 变量并保留当前镜像为默认值，状态文件以活动镜像 revision 初始化。固定部署器单独完成脚本检查和回退测试后安装。源站健康地址为 `http://127.0.0.1:18084/_gateway/health`。验证包括迁移兼容检查、镜像 revision、两轮源站/公开健康检查及真实图片请求。

## 固定部署器安装

固定部署器更新与普通应用发布分开执行。检查脚本和测试通过后，由管理员显式安装：

```bash
ssh hd0526 'sudo install -d -m 0755 /opt/niffler-release/bin /opt/niffler-release/git'
scp scripts/fixed-production-deployer.sh hd0526:/tmp/deploy-production
ssh hd0526 'sudo install -m 0755 /tmp/deploy-production /opt/niffler-release/bin/deploy-production && rm -f /tmp/deploy-production'
```

普通发布不得覆盖该文件。以后修改固定部署器时，必须先通过独立审核和测试，再执行
一次显式安装。

当前仓库没有 Actions 专用部署密钥或自托管运行器，因此生产发布暂由已授权管理员
从本机调用固定部署器。配置专用发布凭证后，再将同一固定入口接入受保护的 GitHub
`production` 环境；不得上传个人 SSH 私钥。

GitHub 环境和 Secret 完成配置、专用用户安全验证通过后，上述临时说明由
`Deploy Production` 工作流取代；管理员本机入口仍只作为紧急恢复手段保留。

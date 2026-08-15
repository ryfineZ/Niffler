# Task Plan: 当前任务记录

## 管理端套餐可见性与刷新时间（2026-08-15）

### Goal

管理员用户列表直接显示当前生效套餐、当前周期剩余额度和下次刷新时间；用户套餐中心显示每日额度的刷新时间、已用和剩余情况。列表数据必须批量读取，不能按用户逐条查询。

### Current Task State

- `current_task`: 管理端用户列表与用户套餐中心显示套餐额度和刷新时间
- `current_phase`: Phase 8（提交 main 并发布生产）
- `active_skills`: `planning-with-files`、`using-ui-polish`、`ui-core`、`ui-web`、`ui-review`
- `do_not_reinvoke_superpowers`: true
- `next_step`: 提交并推送 main，等待准确提交镜像构建后触发生产部署并复查健康状态

### Phases

- [x] Phase 1：记录需求、接口字段、周期语义和性能边界
- [x] Phase 2：实现 PostgreSQL 当前页用户套餐摘要批量查询
- [x] Phase 3：扩展管理端用户列表接口并增加独立套餐列
- [x] Phase 4：扩展用户套餐接口并显示额度与下次刷新时间
- [x] Phase 5：完成后端、前端、性能与 UI 验证
- [x] Phase 6：修复审查发现的窗口一致性、查询负载、读取失败和列表刷新问题
- [x] Phase 7：按确认规则将缺失额度记录视为 0，并修复最终审查发现的全部问题
- [ ] Phase 8：提交并推送 main，构建准确提交镜像，部署生产并完成两次健康检查

### Safety Boundary

- 本轮只读取套餐和额度窗口，不修改用户额度、套餐有效期或生产数据。
- 用户列表只增加一次当前页批量查询，并与现有列表查询并行执行；禁止逐用户访问数据库。
- “每日刷新时间”按服务端实际额度窗口返回，前端不自行推算。
- 无套餐、套餐未开始、已过期和无限额度都要有明确显示。
- 本轮不提交、不推送、不部署，除非用户再次明确要求。

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 原始工作区包含其他任务的计划文件和图片改动 | 开始实现前检查 | 从远端最新 `main` 创建独立工作树，避免覆盖用户改动 |
| UI 自动审查首次扫描整份历史翻译文件和应用入口，误报大量本任务外问题 | 最终界面审查 | 将审查范围收敛到本次修改的两个页面；静态与运行时检查通过，仅保留用户列表既有操作入口较多的提醒 |
| Rust 严格检查发现测试数字分隔格式不统一 | 最终静态检查 | 只调整测试字面量格式，重新运行全目标 Clippy 后通过 |
| 精确查询旧额度记录时错误地在同一条件的 SQL 片段之间插入 `OR` | Phase 6 SQLite 实库测试 | 改为每组完整条件之间插入 `OR`，同步修复 PostgreSQL、MySQL、SQLite；SQLite 与 PostgreSQL 实库测试通过 |

---

## 老套餐实时跟随套餐号池（2026-08-13）

### Goal

套餐当前配置的可用号池统一决定所有有效套餐可用的服务和模型；管理员修改套餐号池后，老用户从下一次请求开始立即使用新配置，不再保留单个用户的号池覆盖规则。

### Current Task State

- `current_task`: 老套餐实时跟随套餐号池
- `current_phase`: Phase 7（已获授权，正在提交并发布）
- `active_skills`: `planning-with-files`、`using-ui-polish`
- `do_not_reinvoke_superpowers`: true
- `next_step`: 提交后依次进入 test、main，并部署 main 的准确构建产物

### Phases

- [x] Phase 1：更新架构文档，固定套餐号池实时生效及迁移边界
- [x] Phase 2：修改 PostgreSQL、MySQL、SQLite 和内存套餐可用性查询
- [x] Phase 3：统一请求路由、用户模型列表和管理端用户套餐编辑语义
- [x] Phase 4：补充老套餐、动态模型、号池变更及请求结算测试
- [x] Phase 5：完成后端、前端、性能与 UI 验证
- [x] Phase 6：复核最终差异；提交和部署等待用户明确指令
- [ ] Phase 7：提交、测试环境验证、合并 main、生产部署和健康检查

### Safety Boundary

- 套餐价格、购买时间、有效期、已用额度和管理员自定义额度不随模板变化。
- 只让套餐号池及这些号池当前提供的模型实时生效。
- 修改只影响变更后开始的新请求；已开始请求继续按请求开始时保存的计费规则结算。
- 生产套餐已配置号池；本轮不再修改生产数据。
- 用户已明确授权提交、合并到 main 并部署生产；发布仍遵守 test 验证和 main 准确提交规则。

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 旧工作区包含其他任务的未提交改动 | 开始实现前检查 | 从远端 `main` 的生产提交新建独立工作树，避免覆盖用户改动 |
| 数据层全量测试有 1 项远端既有迁移测试失败 | Phase 5 全量验证 | 确认远端 `main` 使用同一条缺失必填统计字段的测试插入语句；本次相关 531 项通过，并用隔离 PostgreSQL 单独执行新增套餐测试通过 |

---

## Niffler 受管理提示词按用户分组配置（2026-08-04）

### Goal

将受管理提示词的配置来源从调度分组改为用户分组。管理员分别为 CTF/渗透和成人用户分组选择配置；用户通过 API Key 当前绑定的用户分组切换配置；服务端只信任鉴权后读取到的 API Key 分组，不接受客户端请求头决定提示词配置。

### Current Task State

- `current_task`: 受管理提示词按用户分组配置
- `current_phase`: User Group Managed Instructions Phase 5（验证完成）
- `active_skills`: `planning-with-files`、`using-ui-polish`
- `do_not_reinvoke_superpowers`: true
- `next_step`: 汇报实现和使用方式；本轮不提交、不部署

### Phases

- [x] User Group Managed Instructions Phase 1：核对用户分组、API Key 分组切换和鉴权数据链路，先修正文档
- [x] User Group Managed Instructions Phase 2：将配置校验、注册表接口和运行时读取迁移到用户分组
- [x] User Group Managed Instructions Phase 3：将控制台入口迁移到用户分组编辑，保留用户现有 API Key 分组选择
- [x] User Group Managed Instructions Phase 4：删除调度分组配置语义，补充可信分组选择、切换和隔离测试
- [x] User Group Managed Instructions Phase 5：完成后端、前端、UI 和完整相关验证

### Safety Boundary

- 本轮不提交、推送或部署，除非用户再次明确要求。
- 不修改线上用户分组、API Key、路由分组或生产数据。
- 配置来源固定为鉴权后读取到的 API Key 用户分组；客户端请求头、请求正文和 XML 标记都不能决定配置。
- 用户切换 API Key 分组后只影响后续请求；正在执行和历史请求不变。
- 不新增第二套用户分组选择器，复用现有 API Key 创建与编辑流程。
- `security_research_v1` 和 `adult_fiction_v1` 保持不变；`core_v1` 继续只作为内部共享正文。
- 先更新外部需求文档和 Aether 架构说明，再修改业务代码。

### Key Questions

1. 用户分组当前是否有可扩展 JSON 配置；若没有，应增加字段还是独立关联表？
2. 鉴权结果是否已经携带 API Key 的 `group_id`，能否在请求规划阶段一次读取用户分组配置？
3. 用户修改 API Key 分组时，现有权限校验如何确保只能选择公开分组或已分配的内部分组？
4. 用户分组管理页的创建、编辑、导入导出和删除流程需要在哪个统一入口校验配置？
5. 线上已发布的调度分组配置字段应直接停止读取，还是需要清理兼容提示？

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| `gh` 未显式指定仓库时把当前目录识别成上游 `fawney19/Aether` | 上一轮生产发布首次查询 | 后续 GitHub 命令固定使用 `-R ryfineZ/Niffler`；生产发布已成功 |
| 首次检索用户分组迁移时同时传入不存在的顶层 `migrations` 和 `apps/aether-gateway/migrations` | Phase 1 首次存储检索 | 已确认实际迁移目录为 `crates/aether-data/migrations/{postgres,mysql,sqlite}`；后续只使用真实路径 |
| 独立请求切换配置测试直接复用了不含 `messages` 的示例 Chat 请求体，返回 400 | Phase 4 首次切换测试 | 为安全和成人两个独立请求显式设置合法的 Chat `messages` 数组后重跑 |
| 前端 `npm run lint` 会固定检查并自动格式化整个目录，触发本任务外旧代码问题 | Phase 5 首次代码规范检查 | 根据执行前状态恢复 137 个无关文件，只保留本任务和原有改动；改用底层 ESLint 对目标文件做只读检查并通过 |
| 恢复无关格式变化的首次脚本使用了 zsh 特殊变量名 `path`，导致循环内找不到 `git` | Phase 5 清理首次尝试 | 改用普通变量名 `file_path`，逐个恢复已确认的目标文件并复核工作区范围 |
| 数据层全量测试的三处迁移清单没有包含新版本 `20260804120000` | Phase 5 首次 508 项回归 | 更新 PostgreSQL 待执行清单及 MySQL、SQLite 启用迁移清单；508 项复测全部通过 |
| 网关全量测试中的既有视频轮询测试超过自身 500 毫秒上限 | Phase 5 的 2969 项回归 | 其余 2968 项通过；该项脱离全量负载后单独重跑通过，保留为既有时序风险 |

---

## Niffler 受管理提示词配置（2026-08-03）

### Goal

按照已确认的需求文档，在 Aether 为路由分组增加版本化受管理提示词配置，统一支持 `openai:responses`、`openai:chat` 和 `claude:messages`，保留客户端原有指令，并提供管理端配置、运行记录和完整验证。

### Current Task State

- `current_task`: Niffler 受管理提示词配置
- `current_phase`: Managed Instructions Phase 20（验证完成）
- `active_skills`: `planning-with-files`、`using-ui-polish`
- `do_not_reinvoke_superpowers`: true
- `next_step`: 汇报修复结果；不提交、不部署

### Phases

- [x] Managed Instructions Phase 1：核对当前代码与需求差异，先新增 Aether 设计记录
- [x] Managed Instructions Phase 2：实现版本化提示词文件、注册表、配置解析与摘要规则
- [x] Managed Instructions Phase 3：实现三种最终上游格式的统一注入、内部状态和防重复规则
- [x] Managed Instructions Phase 4：补充运行记录、管理接口和明确错误处理
- [x] Managed Instructions Phase 5：实现全局模型管理端配置、状态、预览和错误反馈
- [x] Managed Instructions Phase 6：补齐后端、前端、格式转换和契约测试
- [x] Managed Instructions Phase 7：运行相关验证、UI 审查和最终差异复核
- [x] Managed Instructions Phase 8：合并安全与逆向，删除通用、化学和武器配置，更新契约测试
- [x] Managed Instructions Phase 9：复测注册表、管理端、前端和完整相关检查
- [x] Managed Instructions Phase 10：修复旧配置编辑与配置 ID 规范化问题
- [x] Managed Instructions Phase 11：补充三种格式的真实最终处理测试
- [x] Managed Instructions Phase 12：完成 UI 审查和完整相关验证
- [x] Managed Instructions Phase 13：重新定义按路由分组配置的语义、边界和迁移规则
- [x] Managed Instructions Phase 14：将后端配置校验和请求读取从全局模型迁移到路由分组
- [x] Managed Instructions Phase 15：将控制台入口从模型编辑页迁移到路由分组编辑页
- [x] Managed Instructions Phase 16：更新并补充分组保存、请求应用和旧入口删除测试
- [x] Managed Instructions Phase 17：完成 UI 审查、完整相关验证和最终差异复核
- [x] Managed Instructions Phase 18：记录并修复最终处理重复执行与同版本配置变化问题
- [x] Managed Instructions Phase 19：修复控制台未配置状态并补充交互测试
- [x] Managed Instructions Phase 20：运行相关后端、前端和 UI 验证

### Safety Boundary

- 不提交、推送或部署，不修改生产配置和生产数据。
- 默认关闭新功能；分组未配置或未启用时，请求体必须保持当前行为。
- 保留当前工作区已有的 Codex Responses 推理项 ID 修复，不覆盖或回退用户改动。
- 不将需求文档中的 API 密钥写入 Aether 源码、测试、日志或计划文件。
- 先更新 Aether 架构文档，再修改任何会改变请求行为的代码。
- 本轮范围固定为 `security_research_v1` 与 `adult_fiction_v1`；`core_v1` 仅作为两者共享的内部基础规则。
- 新实现以请求实际选中的路由分组为唯一配置来源，不保留全局模型覆盖分支；未选择分组或分组未配置时保持不注入。
- 当前改动尚未发布，不执行旧全局模型配置的数据迁移，也不增加两套配置并存的兼容优先级。

### Key Questions

1. 路由分组配置在最终全局模型、Provider、Endpoint 和账号切换后通过什么对象继续传递？
2. 三种最终上游格式在哪个共享位置完成 `body_rules` 后处理，能否只实现一套正文生成逻辑？
3. 最终请求体和请求元数据目前如何持久化，新增字段应写入哪个已有结构？
4. 重试是复制最终请求体还是重新转换原请求，内部注入状态应挂在哪个请求上下文？
5. 路由分组编辑页当前如何读取、校验和保存 `config_json`？

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| `planning-with-files` 恢复命令没有输出，而仓库已有多项历史计划记录 | 首次恢复 | 以当前 `git status`、现有计划文件和提交号为准；新增独立任务段，不覆盖历史记录 |
| UI 工作流首次将前端技术栈填写为 React，实际项目使用 Vue | 首次 UI brief | 废弃该次技术栈信息；按 Vue/TypeScript 重新运行工作流，其他已确认的界面目标不变 |
| 查询应用状态时路径中使用了未转义的 `app*`，zsh 在执行 `rg` 前报无匹配 | 首次定位全局模型读取入口 | 改为先用 `rg --files` 确认实际文件，再检索明确目录；未修改业务文件 |
| 同时更新计划与进度时误以为进度文件标题是 `# Progress`，补丁无法匹配 | 首次阶段切换记录 | 读取文件顶部后按实际标题 `# Progress Log` 更新；原补丁整体未应用 |
| 前端错误状态测试预期固定回退文案，实际错误解析器优先保留服务端或网络错误详情 | 配置区首次测试 | 调整断言以验证可读的真实错误详情和重试行为，不修改现有统一错误解析规则 |
| 新增图片桥接测试时沿用其他测试可见函数的写法，测试模块使用显式导入而不是 `super::*` | 首次格式包测试 | 将新增私有辅助函数和常量加入测试模块的 `super` 导入列表；生产代码未受影响 |
| `ui-review` 文档中的相对引用按包根目录解析后不存在 | 首次读取审查引用 | 用 `rg --files` 按技能目录定位到 `skills/ui-polish/references` 的真实文件，再继续完整读取；未跳过审查步骤 |
| 本机 5173 端口已有另一个项目，首次运行时预览读到账号库存页面；5174 也已占用 | UI 运行时审查 | 不停止用户已有服务；为 Aether 临时使用 Vite 自动选择的 5175，并用 8084 本地模拟注册表，审查后已停止两项临时服务并删除临时入口 |
| `editorial-default` 将独立表单组件误判为落地页并要求主视觉标题 | UI 运行时审查首次档位 | 按实际“紧凑弹窗/设置区”改用 `compact-popup` 审查档位，静态与运行时检查取得正式通过结果 |
| 架构测试发现受管理提示词模块直接引用底层格式包 | 首次完整架构测试 | 将图片桥接接口统一从 `ai_serving/api.rs` 暴露，业务模块改用项目规定的格式入口；60 项架构测试复测通过 |
| 目标 ESLint 发现表单中的既有未使用变量和新测试的未使用导入 | 首次目标代码规范检查 | 删除无用途的旧局部变量和新测试导入，并按现有测试写法限定多组件模拟规则；目标 ESLint 复测通过 |
| 首次按 `compact-popup` 同时扫描整个路由策略页和新配置区，现有页面的大量操作入口触发动作密度阻断 | 分组界面最终审查 | 审查对象收敛到本次新增的紧凑配置区，分组页的实际放置继续由源码、类型检查和生产构建验证；静态与真实运行时检查取得 `PASS ui-review gate` |
| 前端全仓库 ESLint 被其他既有页面的 130 个错误阻断 | 最终全仓库检查 | 不修改本任务范围外页面；本次 13 个修改文件以零警告通过目标 ESLint，完整前端 564 项测试、类型检查和生产构建均通过 |
| 网关全量测试中的视频后台轮询测试两次超过自身 500 毫秒等待上限 | 2968 项最终回归 | 其余 2967 项均通过；同一失败测试脱离全量负载后单独重跑通过，确认与受管理提示词改动无关，并保留为剩余的既有时序风险 |
| 敏感模式扫描命中全局模型测试文件中的既有假密钥 | 首次扫描整个修改文件 | 用零上下文差异只扫描本次新增行，并单独扫描未跟踪文件；确认命中内容不是本次新增，最终扫描无匹配 |
| 网关全量测试因最小运行环境没有全局模型读取器而出现大量 500 和等待 | 首次 2964 项全量测试 | 设计和实现明确区分“没有读取能力”与“有读取能力但记录缺失”：前者保持默认关闭，后者继续显式失败；代表性回归和 2964 项全量测试复测通过 |
| 更新 Phase 10 至 Phase 12 状态时按错误的小节格式匹配，补丁未应用 | 首次阶段切换记录 | 读取计划文件顶部后按现有单行阶段格式更新；业务文件未受影响 |
| 目标 ESLint 首次从仓库根目录执行，却传入了以 `frontend` 为根的相对路径 | 首次本轮前端代码规范检查 | 切换到 `frontend` 工作目录后用相同两个文件重新执行并通过 |
| 同版本配置变化失败测试首次使用缺少 `messages` 的 Chat 样例 | Phase 18 失败测试 | 将该测试改为具有合法字段结构的 Responses 请求，再验证配置冲突本身 |
| Rust 格式检查发现新增快照字段换行不符合 rustfmt | Phase 18 后端实现 | 运行全工作区 rustfmt 后再执行回归测试 |
| 注册表重试测试仍预期未配置分组显示配置摘要 | Phase 19 前端测试 | 改为验证错误消失并恢复“未配置”状态，保持新界面语义一致 |

---

## 全部本地改动提交与生产同步（2026-08-03）

### Goal

整理当前工作区全部改动，完成安全检查和相关验证后提交到远端 `main`，使用准确提交对应的 CI 镜像部署生产，并确认本地、远端和生产运行版本一致。

### Phases

- [x] Release Phase 1：确认改动范围、远端基准、仓库可见性及敏感文件风险
- [x] Release Phase 2：在远端最新 `main` 基础上整理全部应提交内容并完成验证
- [ ] Release Phase 3：按约定式提交创建提交并推送受保护远端流程
- [ ] Release Phase 4：等待 CI 镜像构建并部署生产
- [ ] Release Phase 5：核对本地、远端、生产镜像、迁移和服务健康状态

### Safety Boundary

- 用户已明确授权提交当前全部本地改动、推送和生产部署。
- 不提交密钥、凭证、真实用户敏感报告、依赖目录或临时文件；发现此类内容时改为忽略并保留本地副本。
- 不使用强制推送、硬重置或绕过测试、分支保护和生产审批。
- 生产只部署远端 `main` 的准确提交及其 CI 镜像，不在服务器直接编译或修改代码。

### Current Phase

远端最新版三方合并、回归修复和发布前验证均已完成，正在复核最终差异并创建约定式提交。

### Active Skills

- `planning-with-files`
- `commit-conventional`

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 新文件敏感模式扫描将多行文件列表作为一个参数传给 `rg`，zsh 报文件名过长 | 第一次扫描未跟踪文件 | 改用 NUL 分隔的 `git ls-files -z` 与 `xargs -0`；已跟踪差异扫描正常且未发现匹配，未暂存或提交任何文件 |
| 将本地脏文件整文件复制到远端基准后，差异显示会回退远端已上线内容，例如生产部署文档出现 161 行删除 | 第一次隔离整理 | 弃用该隔离副本；新建远端基准并使用本地 HEAD 作为三方合并基础，只叠加本地改动，不整文件覆盖远端内容 |
| 第二次隔离仓库克隆超过工具首次等待时间，后续脚本尚未保存工作目录；一次验证因此落回原工作区 | 第二次隔离整理 | 进程仍在正常下载且没有修改原工作区；停止依赖会话变量，改用已确认的绝对临时路径并等待现有克隆完成，不重复启动克隆 |
| 第二次网络克隆持续两分钟没有产生提交对象 | 第二次隔离整理后续 | 终止仅属于临时目录的挂起克隆；从第一个完整隔离仓库创建 detached 干净工作树，并从本地对象库读取三方合并基础，避免再次访问网络 |
| 三方应用本地差异时 51 个已跟踪文件中 4 个存在重叠冲突 | 远端基准合并 | 逐段合并远端已上线的有限预读逻辑与本地事件驱动故障切换；迁移测试保留远端 Provider 迁移并增加 GPT-5 迁移，不采用整文件一方覆盖 |
| 本机未安装 `shellcheck` | 发布前脚本静态检查 | 已完成全部新增 Shell 脚本的 `bash -n` 语法检查，并继续运行仓库内脚本测试；不伪称执行过 ShellCheck |
| 三方合并后网关全量测试有 2 项 429 暂停理由断言失败 | 2942 项网关测试首次执行 | 流式换号允许 429 重试，但账号池写入必须继续按现有限流规则处理；限制容量暂停分支不接管 429，保留原有限流规则优先级后重测 |
| 两项 Linux 监控脚本测试在 macOS 直接运行失败 | 首次运维脚本测试 | 脚本明确依赖 GNU `stat` 且生产监控要求 root；改在 `hd0526` 隔离临时目录运行，使用测试自带的假容器、假磁盘和假通知程序，两项均通过且临时目录已清理 |
| 第一次复制监控测试文件到远端临时目录时漏写主机前缀 | Linux 隔离测试首次传输 | `scp` 在本地找不到临时目录，未触碰生产配置；临时目录由退出钩子清理，补齐 `hd0526:` 后复测通过 |
| 本机 Docker 命令存在但 Docker 服务未启动 | 尝试本地 Linux 容器测试 | 未启动额外常驻服务；改用生产主机隔离临时目录完成同一组测试 |
| 首次远端 CI 的数据层 Clippy 检查拒绝链式布尔转换写法 | `test` 合并请求 #25 | 按 CI 使用的 Rust 1.95 规则改为直接的 `if/else`，不改变空账号标识映射语义；本地运行同一 Clippy 命令后更新合并请求 |
| 第二次远端 CI 的工作区 Clippy 检查要求测试模块位于文件末尾 | `test` 合并请求 #25 | 仅移动计费单元测试模块位置，不改测试和生产逻辑；本地对计费包运行同一 Clippy 规则，远端 CI 继续执行全工作区剩余包检查 |
| 本地整工作区 Clippy 编译依赖时耗尽隔离目录磁盘空间 | 第二次 CI 修复本地验证 | 使用 `cargo clean` 只清理隔离发布目录中 15.3 GiB 可重建产物；随后计费包 Clippy、38 项测试和格式检查通过，全工作区结果以 Linux CI 为准 |

## 远端与生产版本核对（2026-08-03）

### Goal

核实 Provider 计费统计后续修改、CCSwitch 余额兼容和 GPT-5 价格调整是否已经进入远端提交及生产环境，区分本地状态误报、远端已合并内容和生产实际运行内容。

### Phases

- [x] Audit Phase 1：逐文件比较本地内容与远端所有现有分支
- [x] Audit Phase 2：确认生产部署提交号、镜像版本和运行实例
- [x] Audit Phase 3：逐项核对生产代码、数据库迁移及接口行为
- [x] Audit Phase 4：汇总结论和需要保留或清理的本地改动

### Safety Boundary

- 只读核对远端、生产主机、容器和数据库迁移状态。
- 不修改生产配置，不重启服务，不执行数据库写入或部署。
- 以生产实际运行的镜像和数据库为准，不根据本地 `git status` 推断上线状态。

### Current Phase

已完成：Provider 统计功能已随远端 `main` 部署；CCSwitch 基础兼容已部署但本地第二次口径调整未部署；GPT-5 价格数据已在生产生效，但对应迁移和计费空值修复尚未进入远端代码。

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 查询发布脚本时使用了不存在文件的 shell 通配符，zsh 在执行 `rg` 前终止命令 | 首次读取生产发布状态说明 | 改为先列出实际脚本文件，再对明确路径检索；未修改任何文件或生产状态 |

## OpenAI Responses 输出前自动换号（2026-08-02）

### Goal

仅对 Niffler 服务端的 Codex Pro 号池 `openai:responses` 端点增加协议事件驱动的两阶段发送：实际输出前遇到可重试错误时自动更换账号，实际输出后禁止拼接响应；配套账号与模型级暂停、监控和一键关闭，不修改用户本地 Codex。

### Phases

- [x] Stream Failover Phase 1：核对现有配置、流处理、调度反馈和管理界面，补充设计记录
- [x] Stream Failover Phase 2：实现端点级配置契约、默认值和运行时解析
- [x] Stream Failover Phase 3：实现协议事件驱动的输出提交边界、换号和账号模型级暂停
- [x] Stream Failover Phase 4：在现有端点编辑表单增加紧凑配置入口与交互校验
- [x] Stream Failover Phase 5：补充后端、前端和异常路径测试
- [x] Stream Failover Phase 6：运行相关验证、UI 审查和差异复核

### Safety Boundary

- 不修改或重启生产服务，不直接写生产数据库。
- 不回退或混入工作区已有的计费、监控等未提交改动。
- 新逻辑默认关闭，只能在 `openai:responses` 端点显式开启，并保留一键关闭。
- 只有尚未向客户端发送实际输出时才允许换号；已经输出文本或工具调用后禁止拼接另一条响应。
- 不做灰度发布；上线前必须完成直接相关测试，并确保关闭配置即可恢复原逻辑。

### Current Phase

已完成：端点级输出前自动换号、账号模型级暂停、管理界面、文档和异常路径测试均已实现并通过最终验证；未部署、未重启服务、未修改生产配置。

### Active Skills

- `planning-with-files`
- `using-ui-polish`
- `openai-docs`
- `ui-review`

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 只读数据库查询首次远程 shell 引号不匹配 | 第一次账号池核查 | 改用明确的远程 `psql -c` 双层引号后成功，未发生写入 |
| 首次更新任务记录时标题匹配错误 | 配置层验证完成后 | 读取文件实际标题并重新应用补丁，代码和测试未受影响 |
| 核心流处理首次格式化发现 `while if` 少一个右花括号 | SSE 预读循环首次编译 | 补齐分隔符后重新格式化并编译，5 项事件解析测试全部通过 |
| 首次整库测试发现两项旧失败分类断言和一项架构边界失败 | 2922 项网关库测试 | 将参数错误限制在本功能的临时错误允许列表内，保留既有通用重试语义；格式判断改走 `ai_serving` 根入口，三项定向测试和整库复测通过 |
| 直接运行 `rustfmt` 使用默认 Rust 2015 版本 | 最终格式检查 | 显式指定项目使用的 Rust 2021 版本后检查通过，代码未因失败命令发生语义变化 |
| Cargo 共享依赖缓存被另一项本地任务长时间占用 | 最终后端复测 | 停止本轮等待命令，使用独立锁文件并以离线方式只读复用已有依赖缓存，未中断或修改另一项任务 |
| 整个旧端点弹窗触发既有 UI 密度检查，目标代码规范检查提示宏顺序 | UI 最终审查 | 将新配置区提取为独立组件并调整 `useI18n` 位置；目标 ESLint 和 UI 静态、运行时检查最终通过 |

## 提供商测试、单账号测试与账号并发（2026-08-01）

### Goal

增加账号级实时并发显示和单账号请求测试；参考 sub2api 将提供商测试改为协议模板优先，支持多种现有请求协议和图片生成，并保留高级 JSON 排错能力。

### Phases

- [x] Test Phase 1：核对 Niffler 现有测试、账号列表和 sub2api 参考实现
- [x] Test Phase 2：记录接口、运行时和前端交互设计
- [x] Test Phase 3：实现账号级并发聚合和单账号后端测试
- [x] Test Phase 4：实现账号列表入口与协议模板优先测试弹窗
- [x] Test Phase 5：补充测试并运行针对性验证

### Safety Boundary

- 不回退工作区已有的其他未提交改动。
- 不改变正常业务请求的调度与失败切换行为。
- Redis 并发读取失败时仅显示为 0，不阻断账号列表和请求测试。
- 单账号测试必须在后端再次校验账号属于当前提供商且支持所选端点。

### Current Phase

已完成：账号级并发显示、单账号测试和协议模板优先测试均已实现并通过针对性验证。

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 前端目标测试首次无法解析 `vue-i18n` | 本地 `node_modules` 缺少已声明依赖 | 按 `package-lock.json` 执行 `npm ci --ignore-scripts` 后重新运行，59 项测试全部通过 |

## GitHub 测试环境与晋级流水线配置（2026-07-28）

### Goal

完成 PR #14 对应的 GitHub 管理员配置、测试服务器受限部署账号、`test` 分支保护和首次测试部署验证，并在条件满足后补全 `main` 晋级规则。

### Current Phase

Environment Phase 5：等待首次测试部署验证

### Phases

- [x] Environment Phase 1：核对 PR、GitHub 权限、现有 Environment 和 Ruleset
- [x] Environment Phase 2：创建测试服务器专用账号、部署目录和专用密钥
- [x] Environment Phase 3：创建 `test` Environment，配置 Secrets 和 Variables
- [x] Environment Phase 4：修复并合并测试部署流水线，配置 `test` 分支保护规则
- [ ] Environment Phase 5：执行首次测试环境部署和健康验证
- [ ] Environment Phase 6：补全 `main` 晋级检查和 production reviewer，复核全部保护规则

### Safety Boundary

- 不使用 `ops`、个人 SSH 或 root 私钥执行测试部署。
- 不在日志、PR、Issue 或回复中输出专用私钥。
- 不修改或重启生产数据库、Redis 和生产应用。
- 保留当前工作区全部既有未提交改动，不纳入本次 GitHub 配置任务。

### Status

in_progress

### Next Action

确认创建受保护的远程 `test` 分支，触发首次 GitHub Actions 测试部署；部署成功后再为
`main` 增加 `Promotion policy` 与 required deployment `test`。

---

## 生产服务器 Telegram 监控（2026-07-28）

### Goal

监控两台生产服务器的磁盘、核心容器和网站健康接口，并通过 Telegram 发送异常与
恢复通知。

### Current Phase

已完成：两台服务器监控、Telegram 测试、去重和恢复逻辑均已验证。

### Phases

- [x] Monitor Phase 1：核对两台服务器依赖、磁盘和健康基线
- [x] Monitor Phase 2：更新生产监控运维文档
- [x] Monitor Phase 3：实现脚本、service 和 timer
- [x] Monitor Phase 4：部署 rn01 监控
- [x] Monitor Phase 5：部署 hd0526 监控
- [x] Monitor Phase 6：发送测试摘要并验证定时器、去重和生产健康

### Safety Boundary

- 监控只读取状态，不自动重启、删除文件或修改数据库。
- 连续 3 次失败才通知容器和网站异常。
- 相同状态不重复通知，恢复时单独通知。
- Telegram 凭据权限必须为 `0600`。

### Status

completed

## Telegram 数据库备份失败通知（2026-07-28）

### Goal

让 `rn01` 的 PostgreSQL 自动备份失败时，通过私人 Telegram Bot 消息主动通知。

### Current Phase

已完成：Telegram 测试消息和 systemd 失败触发关系均已验证。

### Phases

- [x] Alert Phase 1：验证 Bot Token 并取得私人 Chat ID
- [x] Alert Phase 2：新增通知脚本、systemd 服务和运维记录
- [x] Alert Phase 3：部署并发送测试消息
- [x] Alert Phase 4：验证失败触发关系、凭据权限和原备份任务

### Safety Boundary

- Token 和 Chat ID 只保存在权限为 `0600` 的凭据文件中。
- 测试消息必须明确标注测试，不制造真实故障。
- 不修改数据库数据，不故意让生产备份失败。
- Telegram 失败不能覆盖备份任务原始状态和 systemd 日志。

### Status

completed

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 本机连接 Telegram API 被重置 | 首次验证 Bot Token | 改由 `rn01` 调用 Telegram API，Token 和 Chat ID 验证成功 |
| systemd 首次预检查找不到正式路径中的通知脚本 | 安装前验证 unit 文件 | 当时没有替换现有 unit；先安装脚本，再次验证 unit 后通过 |

## 生产网络与数据库连接加固（2026-07-28）

### Goal

关闭 `hd0526:8084` 公网访问，让 Niffler 改用非超级用户 PostgreSQL 账号，并强制
`hd0526` 到 `rn01` 的数据库连接使用 TLS。

### Current Phase

已完成：公网端口、数据库账号、TLS 和备份兼容性验证均已通过。

### Phases

- [x] Security Phase 1：核对端口、防火墙、数据库账号、TLS 和通知条件
- [x] Security Phase 2：关闭 `hd0526:8084` 公网访问并验证
- [x] Security Phase 3：创建非超级用户数据库账号并转移业务对象所有权
- [x] Security Phase 4：启用 PostgreSQL TLS 并切换应用连接
- [x] Security Phase 5：执行完整健康、权限、加密和备份验证

### Safety Boundary

- 每项生产变更前保存权限为 `0600` 的回退文件。
- 不删除 `postgres` 账号，不修改业务表结构或业务数据。
- 前一项验证失败时立即回退，不继续执行后续变更。
- 没有外部接收地址时，不伪造备份通知已接入。

### Status

completed

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| PostgreSQL 拒绝先单独修改表自带序列的所有者 | 首次所有权转移事务 | 整个事务已回滚；调整为先修改表，再处理仍由 postgres 拥有的独立序列 |
| Docker Compose 没有因 `.env` 内容变化自动重建容器 | 首次切换应用数据库账号 | 使用 `--force-recreate` 明确重建 frontdoor 和 background，随后数据库会话用户正确 |
| TLS 文件预检查发现访问规则未安装且目录不可遍历 | PostgreSQL 重启前检查 | 停止执行且未重启数据库；重新安装文件、修正目录权限并用 PostgreSQL UID 实测可读后继续 |

## rn01 首次数据库备份与恢复验证（2026-07-28）

### Goal

为生产 PostgreSQL 创建首份可恢复的异地备份，将备份保存到私有 Cloudflare R2，
并在隔离的 PostgreSQL 15 环境中完成真实恢复验证。

### Current Phase

已完成：首份备份恢复验证、自动备份部署及真实运行检查均已通过。

### Phases

- [x] Backup Phase 1：检查 rn01 磁盘、数据库体积、活动事务和本机恢复容量
- [x] Backup Phase 2：补充备份与恢复运维文档
- [x] Backup Phase 3：生成完整备份、校验并上传 R2
- [x] Backup Phase 4：从 R2 下载并在隔离数据库中恢复验证
- [x] Backup Phase 5：配置自动备份、保留策略和本机失败状态记录

### Safety Boundary

- 使用 PostgreSQL 在线一致性导出，不停止、不重启生产数据库。
- 不在生产数据库中创建恢复测试库，恢复只在本机隔离容器执行。
- 备份文件上传成功并通过恢复验证前，不删除任何生产数据。
- R2 凭据只允许访问 `niffler-db-backups`，不得写入代码仓库或命令输出。

### Status

completed

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 最大关系查询的制表符转义被远程 shell 破坏 | 首次只读容量检查 | 改用 Base64 传递 SQL；查询成功，生产数据库未发生修改 |
| `pg_dump` 不支持误带的 `-X` 参数 | 首次完整导出 | 命令在读取数据前停止，只生成0字节临时文件；删除临时文件并移除该参数后重试 |
| 本机进度检查误用 zsh 保留变量 `path` | 查看慢速复制进度 | 该条只读检查未执行；改用 `file_path`，正在运行的复制未受影响 |
| 首次删除本机临时片段使用了错误的 `/usr/bin/unlink` | 清理已停止的慢速复制 | 文件当时未删除；改用实际路径 `/bin/unlink` 并确认片段已不存在 |
| macOS `date` 不支持 Linux 的 `-Is` 参数 | 记录 R2 下载时间 | 仅时间字段为空，下载、大小和哈希校验成功；后续使用跨平台时间格式 |
| 本机已有 PostgreSQL 15.17 镜像 | 首次创建隔离恢复容器 | 删除尚未写入数据的临时容器和卷，下载与生产一致的 PostgreSQL 15.18 后重新恢复 |
| rclone 首次上传自动备份时收到一次 R2 HTTP 501 | 自动任务真实运行检查 | rclone 第二次尝试成功；随后从 R2 独立核对对象大小和校验文件，结果一致 |

## 数据库磁盘事故止血与恢复（2026-07-27）

### Goal

先停止完整请求正文继续写入 PostgreSQL，保持 Niffler 登录和核心接口可用；在用户
确认历史正文取舍后释放数据库磁盘，并补充防止再次发生的代码与监控改动。

### Current Phase

Recovery Phase 4：修复无对象存储时的无上限数据库回退、清理漏跑和队列毒消息问题。

### Phases

- [x] Recovery Phase 1：将生产正文记录切换为 `basic` 并验证服务与配置
- [x] Recovery Phase 2：盘点可安全回收空间，明确历史正文保留或删除方案
- [x] Recovery Phase 3：按用户确认执行数据库空间回收并验证 PostgreSQL
- [ ] Recovery Phase 4：修复无对象存储时的无上限回退和清理任务漏跑问题
- [ ] Recovery Phase 5：测试、发布并复查磁盘告警与健康检查

### Safety Boundary

- 第一阶段不删除业务数据，只修改可逆的正文记录级别。
- 删除 `usage_body_blobs` 前必须获得用户对历史请求/响应原文丢失的明确确认。
- 不直接删除 PostgreSQL 数据文件或 Docker 数据卷。

### Status

in_progress

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 切换 `request_record_level=basic` 时 PostgreSQL 再次处于 recovery mode，事务未开始 | 用单事务锁定并更新一条系统配置 | 确认没有部分更新；先检查数据库再次恢复的原因和磁盘状态，再执行止血 |
| PostgreSQL 恢复后首次连接使用了不存在的 `niffler` 数据库名 | 执行单行配置更新 | 查询容器实际 `POSTGRES_DB=aether` 后重新执行；错误发生在建立连接前，没有修改任何数据 |
| 清理审计表旧行版本时，默认并行 `VACUUM ANALYZE` 超出容器 64 MiB 共享内存 | 对 `usage_http_audits` 执行普通清理 | 改用单进程、16 MiB 维护内存重新执行，清理和统计刷新成功 |

## 线上登录卡顿诊断（2026-07-27）

### Goal

定位 `niffler.org` 页面和登录流程卡顿的具体环节，区分前端资源、Cloudflare、
网关、认证接口、数据库及服务器资源问题；本轮只诊断，不修改代码或生产配置。

### Current Phase

已完成：根因、时间线、当前恢复状态和后续处理顺序均已确认。

### Phases

- [x] Performance Phase 1：测量首页、静态资源、健康接口和登录相关接口的各阶段耗时
- [x] Performance Phase 2：检查登录前端调用链、后端认证实现、超时和数据库查询
- [x] Performance Phase 3：检查生产容器资源、近期日志、数据库连接和慢请求记录
- [x] Performance Phase 4：交叉验证根因、影响范围和发生时间，给出处理建议

### Constraints

- 不使用或索取用户密码，不执行真实账户登录。
- 不修改生产代码、配置、数据库或运行状态。
- 结论必须同时有线上测量、日志或代码证据支持。

### Status

complete

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 并行探针命令因包含临时文件 `rm -f` 被安全策略拒绝，命令未执行 | 同时抓取公开接口和登录探针响应体 | 不创建临时文件，改为让 `curl` 直接输出响应体和耗时 |
| 重启后聚合查询的远程 shell 引号解析失败，查询未执行 | 一次性查询正文表大小、行数和生产设置 | 接口与数据库未受影响；改为 Base64 传递只读 SQL，避免多层引号 |

## GitHub 受保护生产发布与双人审核（2026-07-27）

### Goal

为 Niffler 建立最小权限的 GitHub `production` 发布环境，使受保护的 `main`
能够调用生产主机上的固定发布入口；增加第二名维护者后，将 `main` 最低批准数
从 0 调整为 1。

### Current Phase

Phase 3：实现并测试受限 SSH 协议、生产工作流和服务器安装流程。

### Phases

- [x] Phase 1：完成 GitHub 与生产主机只读安全审计，确认缺失项和权限边界
- [x] Phase 2：先更新发布设计与运维文档，明确凭证轮换、审批和失败处理
- [ ] Phase 3：实现 GitHub `production` 环境、专用发布凭证和固定发布入口
- [ ] Phase 4：增加第二名维护者，将 `main` 最低批准数调整为 1
- [ ] Phase 5：执行工作流、安全、生产只读和最终权限审计

### Initial Constraints

- 不上传或复用个人 SSH 私钥。
- 不让拉取请求代码直接接触生产凭证。
- 不允许待发布分支替换生产主机固定部署器。
- 不创建无法运行或无法验证的发布工作流。
- 第二名维护者必须由用户提供准确 GitHub 用户名，不能代为猜测。

### Status

in_progress

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 审查 `origin` 时错误使用标签 refspec 与 `--prune`，本地 172 个上游版本标签被清理 | 同步全部 `origin` 引用 | 远端和代码未受影响；立即从 `fawney19/Aether` 重新获取标签，核对上游 172 个、本地匹配 172 个 |
| 首次写入新计划时使用了过期的 `findings.md` 顶部标题作为补丁上下文 | 同时更新三份计划文件 | 读取三个文件当前开头后，改用稳定的一级标题作为插入位置 |
| 新增生产访问测试首次运行提示三个实现脚本不存在 | 先验证安全边界测试确实能阻止缺失实现 | 这是测试先行的预期失败；开始实现受限 SSH、root 包装器和安装脚本 |
| 受限包装器首次成功路径测试将合法上传文件误判为越界 | 使用 `realpath` 直接比较临时目录的逻辑路径 | macOS 会将 `/var` 解析为 `/private/var`；改为分别解析上传目录和文件后比较固定文件名 |
| 隔离容器首次执行安装脚本时提示解释器无权限 | 直接执行尚未设置可执行位的新脚本 | 容器和生产均未修改；为新增运维脚本与测试设置可执行位后重新验证 |
| ShellCheck 官方容器首次调用将脚本路径误当成容器可执行文件 | 沿用普通镜像的参数方式调用带入口点的镜像 | 镜像已安全下载，工作区未修改；显式覆盖入口点后重新运行 |
| ShellCheck 报告测试变量声明警告，并提示已校验提交在 SSH 客户端展开 | 静态检查全部新增和修改脚本 | 分离测试变量声明；提交号展开是固定协议所需且已严格校验，增加局部说明；字面量测试改用命名变量 |
| 生产工作流会在一个扫描指纹匹配后保存同次扫描返回的全部主机密钥 | 逐项复核主机密钥验证与 `known_hosts` 写入关系 | 先新增双密钥回归测试；改为只保存指纹精确匹配的主机密钥 |
| 提交前同步 `origin/main` 遇到 GitHub HTTP/2 framing 错误 | 执行普通 `git fetch origin main` | 本地引用未改变；改用 GitHub API 核对远端准确提交，必要时再以 HTTP/1.1 重试 |
| 首次以 HTTP/1.1 推送安全发布分支收到 GitHub 空响应 | 推送本地安全发布提交 | 先用 GitHub API 检查远端是否实际收到分支；未收到再安全重试 |
| 重试推送时 HTTPS 因 OAuth 令牌缺少 `workflow` 权限被拒绝 | 临时增加 SSH URL 并推送现有分支 | 同一命令的 SSH 目标成功创建远端分支；API 已核对远端与本地提交 SHA 完全一致 |
| PR 首轮 `Release tooling` 在 Linux 上将 `stat -f` 文件系统信息当成文件大小 | 运行受限上传跨平台测试 | GNU 与 macOS 的 `stat` 参数语义不同；所有属性读取改为先尝试 GNU `-c`，再回退 macOS `-f` |
| 最小 Ubuntu 容器缺少测试中写死的 `shasum` | 在 Linux 容器复验跨平台修复 | 测试与生产实现保持一致，先使用 `sha256sum`，仅在不可用时回退 `shasum` |
| Ubuntu 容器以 root 运行单元测试时触发生产模式并查找 `niffler-deploy` | 直接使用容器默认用户复验 GitHub Runner 行为 | 保留 root 无法启用测试覆盖的安全边界；改用容器内普通用户运行测试 |
| 生产专用密钥首次登录被 OpenSSH 拒绝 | 安装器将 `.ssh` 设为 root `0700`、授权文件设为 root `0600` | 临时私钥已删除且 Secrets 未写入；先记录 OpenSSH 读取要求，再改为 root 所有、用户只读不可写并执行真实 sshd 验证 |

## Codex 上游配额窗口与真实展示（2026-07-23）

### Goal

让 Codex 账号配额显示完全以当前上游返回为准，覆盖 5H、7D、1M 以及未来出现的其他窗口；不再把窗口位置强行解释成“周”和“5H”，也不因上游暂时不返回某个窗口而沿用旧数据。

### Current Phase

已完成：通用窗口解析、快照展示、调度判断、用量统计和测试验证

### Phases

- [x] Phase 1：确认上游 `/wham/usage` 与响应头的窗口字段、现有本地持久化和历史兼容边界
- [x] Phase 2：先更新设计记录，再实现后端通用窗口解析和快照汇总
- [x] Phase 3：移除管理端号池和前端对 `weekly/5h` 的展示硬编码
- [x] Phase 4：更新耗尽判断、窗口用量统计和重置处理，支持新增/消失窗口
- [x] Phase 5：补充 5H、7D、1M、单窗口、窗口消失和未知窗口测试
- [x] Phase 6：运行 Rust/前端相关验证，复核与当前未提交图片改动的边界
- [x] Phase 7：修复最终审查发现的失败状态判断、CRLF 事件流和 SQL 数值溢出问题

### Implementation Status

complete

### Decisions

| Decision | Rationale |
|----------|-----------|
| 窗口事实以 `limit_window_seconds`、`window_minutes`、`reset_at` 和 `used_percent` 为准 | 窗口位置 `primary/secondary` 不是业务名称，不能据此推断周/月 |
| 标准窗口代码使用 `5h`、`weekly`、`1m`；其它窗口使用基于真实时长的稳定代码 | 7D 沿用已有标识，避免同义代码和历史用量迁移；界面仍显示 `7D` |
| 额度窗口消失就从当前快照移除，不从历史快照补回 | 防止过期额度被误显示为当前额度 |

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| planning-with-files 恢复脚本路径不存在 | 调用旧 Claude 插件路径 | 以项目现有 `task_plan.md`、`findings.md`、`progress.md` 和工作区为准继续 |

## 本地未提交改动审查与上线（2026-07-15）

### Goal

完整审查当前工作区 24 个未提交文件，修复审查发现的问题，验证 Codex 模型目录版本管理、生图验收脚本和相关文档后，提交、推送并按现有生产流程部署。

### Current Phase

Review Phase 6

### Phases

- [x] Review Phase 1：核对项目规则、变更范围和已上线内容，识别重复或过期改动
- [x] Review Phase 2：逐项审查模型版本管理、测试脚本、文档和任务记录
- [x] Review Phase 3：先更新设计记录，再修复审查问题并补充测试
- [x] Review Phase 4：运行格式、静态检查和单元测试；真实上游模型目录验证安排在生产部署后执行
- [x] Review Phase 5：按文件清单暂存并创建约定式提交，推送远端
- [x] Review Phase 6：等待生产镜像构建，部署 hd0526 并完成两次健康检查和模型目录验证

### Status

complete

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| `planning-with-files` 恢复脚本读到旧会话中与当前工作区不一致的描述 | 恢复未同步上下文 | 以当前 `git status`、完整差异和现有计划文件为准，旧描述不作为本次审查依据 |
| 新增空模型目录回归测试后按预期失败：成功数为 1，预期为 0 | 验证 HTTP 200 空目录不会清空权限 | 将空目录改为失败结果并保留原有权限和缓存，随后重新运行测试 |
| 将提交接到 `eb44f662` 后，生图架构文档出现一处内容冲突 | 保留已上线的同步图片保活说明，同时合并 Codex App 请求头结论 | 手工合并两组验证要求并完成 rebase，代码文件无冲突 |
| 首次触发镜像工作流时，GitHub CLI 使用了错误的默认仓库 | 直接运行 `gh workflow run app-image.yml` | 显式指定 `-R ryfineZ/Niffler` 后成功触发生产镜像构建 |
| 首次生产模型验证脚本存在 shell 引号错误，随后又使用了错误的管理鉴权头 | 调用本机管理接口验证三个号池 | 改用编码后的 Python 脚本，并按代码定义使用 `x-aether-admin-*` 请求头；Plus、Pro、Team 均验证成功 |
| hd0526 没有安装 `rg` | 筛选最近 15 分钟容器错误日志 | 改用 `grep -Eai`，两个容器均未发现 `panic`、`fatal` 或 `error` |

---

## sub2api JSON 导入兼容性（2026-07-14）

### Goal

核对用户提供的 sub2api 导出文件与 Niffler 导入协议，定位“无法解析输入内容”的确切原因并给出修复范围；本轮先诊断，不修改代码。

### Current Phase

Implement Phase 5

### Phases

- [x] Import Phase 1：验证 JSON 语法、编码和顶层结构
- [x] Import Phase 2：检查 Niffler 前端解析与后端导入协议
- [x] Import Phase 3：对照 sub2api 导出结构，确认缺失兼容分支和安全边界
- [x] Import Phase 4：汇总根因、影响范围和修复方案

### Status

complete

### Implementation

- [x] Implement Phase 1：确认显示名称规则与兼容边界
- [x] Implement Phase 2：新增设计记录
- [x] Implement Phase 3：补充前后端失败测试
- [x] Implement Phase 4：实现前端识别、后端展开与名称生成
- [x] Implement Phase 5：运行回归测试和 UI review（功能测试通过；UI 静态审查仅命中弹窗既有样式问题）

**Implementation Status:** complete

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| 读取 `token_import.rs` 时少写了 `dispatch/` 路径 | 检查 Access Token 单独导入行为 | 改用 `apps/aether-gateway/src/handlers/admin/provider/oauth/dispatch/token_import.rs` |
| 一条 `jq` 表达式因逗号优先级错误而索引数组 | 汇总平台、套餐和授权模式 | 拆成独立表达式重新查询 |
| UI 静态审查命中弹窗既有交互密度和配色问题 | 按紧凑工具界面规则复核本次前端改动 | 本次没有改可见界面；记录既有问题，不扩大本次兼容性修复范围 |

---

## 线上生图超时事故（2026-07-14）

### Goal

确认线上 Niffler 生图请求超时的具体环节和影响范围，修复后提交并部署生产环境。

### Current Phase

Incident Phase 5

### Phases

- [x] Incident Phase 1：核对客户端现象和线上网关日志
- [x] Incident Phase 2：核对请求记录、提供商端点与出站链路
- [x] Incident Phase 3：检查超时、取消和账号切换的实现
- [x] Incident Phase 4：得出结论并给出修复范围
- [x] Incident Phase 5：开启生产保活、提交代码、构建部署并完成真实生图验证

### Known Facts

- Codex App 已加载生图工具，请求已到达 Niffler 的 `/v1/images/generations`。
- 线上请求被路由到 Pro 号池的 `gpt-image-2`，多个不同账号都出现 60 秒、120 秒无上游响应。
- 同时段文本 Responses 请求大多仍能完成，问题集中在图片生成链路。
- 更正：进一步核对 `extra_data.image_progress` 后，大多数失败请求已在 0.3–2.8 秒内收到上游响应头，且收到 8–9 个 `keepalive` 事件；并非“上游完全无响应”。
- 连接统一在约 124.6 秒被取消，与 Cloudflare 默认 120 秒 Proxy Read Timeout 一致。
- 代码已有 15 秒 JSON 空白心跳实现，但 `should_enable_openai_image_sync_json_heartbeat` 固定返回 `false`，生产请求的心跳数为 0。
- 生产配置已开启同步生图保活；代码默认值改为开启，同时保留管理员显式关闭能力。
- 提交 `fcc5a165` 已部署至 hd0526，前台和后台容器均健康。
- 部署后真实生图请求 HTTP 200，0.87 秒收到首字节，21.56 秒完成，最终 JSON 中的图片可解码为有效 PNG。

### Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| hd0526 未安装 `rg` | 用 `rg` 过滤容器日志 | 改用 `grep -E` |
| Postgres `json` 不支持 `?` 操作符 | 直接检查 `request_headers` 键 | 转为 `jsonb` 后查询；该取消记录未保存请求头 |

---

# 暂停的原任务：后台钱包检索、用户时间展示与前端表格体验优化

## Goal

先复核并上线本次后台钱包检索和用户创建时间展示改动；随后系统检查前端后台页面在手机端、表格内容展示和列宽调整上的问题，并完成可验证的优化。

## Current Phase

Phase 1

## Phases

### Phase 1: 上线前复核
- [x] 复核钱包管理按用户名、邮箱、用户 ID 检索的前后端链路
- [x] 复核用户管理创建时间显示到具体时间
- [x] 运行前端类型检查、后端编译检查和 diff 空白检查
- [x] 确认本地未提交改动中哪些会进入本次上线
- [x] 修复 Vitest 在 Node 25 下的 localStorage 测试环境问题
- [x] 补充钱包用户搜索回归测试
- **Status:** completed

### Phase 2: 上线
- [ ] 提交并推送本地改动
- [ ] 等待 CI 构建生产镜像
- [ ] 按现有生产发布流程上线
- [ ] 验证后台服务健康和关键页面/API
- [ ] 记录本次上线包含的变更、影响范围和风险
- **Status:** in_progress

### Phase 3: 前端体验盘点
- [ ] 盘点后台主要页面的手机端布局问题
- [ ] 盘点表格内容看不全的问题
- [ ] 盘点表格列宽不能拖动调整的问题
- [ ] 明确优先级，避免一次性无边界改动影响生产
- **Status:** pending

### Phase 4: 前端体验优化
- [ ] 抽象或复用表格横向滚动、列宽调整和移动端展示能力
- [ ] 优先修后台高频页面
- [ ] 补齐空态、加载、错误和移动端布局验证
- **Status:** pending

### Phase 5: 最终验证与上线
- [ ] 运行前端类型检查和必要后端检查
- [ ] 执行 UI review，达到 `PASS ui-review gate`
- [ ] 上线并验证生产页面
- **Status:** pending

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| 先上线前复核当前改动，再做全局前端优化 | 当前钱包检索和时间展示是明确需求，先避免拖延上线 |
| 前端优化按后台高频页面分批推进 | 全量后台页面一次性大改风险高，容易影响生产 |
| 表格优化优先做通用能力 | 避免每个页面分别打补丁，降低后续维护成本 |
| 本次上线包含当前工作区所有已完成改动 | 工作区已有多项前序需求处于未提交状态，生产发布流程要求以提交对应的 CI 镜像发布 |

## Known Facts

- 本地工作区存在大量前序任务的未提交改动，本次上线前必须确认发布范围。
- 本次已新增 `user_search` 后台查询参数，覆盖钱包、流水、退款和充值订单读取链路。
- 用户管理已有 `formatDateTime`，本次只把创建时间显示从日期切到日期时间。
- UI 方向按 Niffler 后台既有设计系统处理，不重做视觉风格。
- 生产发布文档要求通过 GitHub Actions 的 `Build App Image` 产物部署，不能直接在生产服务器编译。

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| `ui_workflow.py` 第一次运行缺少 `--platform`、`--surface`、`--goal` | 调用脚本时只传了 mode/path | 已按脚本要求补齐平台、界面类型、目标、代码路径和审查档位重新运行 |
| `npm --prefix frontend run test:run` 首次失败，`localStorage.clear/getItem is not a function` | Node 25 提供了不完整的实验性 `globalThis.localStorage`，jsdom 没覆盖 | 新增 Vitest setup，在测试环境缺少标准 Storage 方法时安装内存版 Storage |
| 新增 SQLite 钱包搜索回归测试首次编译失败，缺少查询结构体导入 | 测试模块 import 没包含新增查询类型 | 补充 `AdminWalletLedgerQuery` 和 `AdminWalletRefundRequestListQuery` 导入后测试通过 |
# Telegram 监控通知与 Bot 设置（2026-07-28）

## Goal

将生产监控和数据库备份通知改成普通用户能理解的中文，并为授权管理员提供安全的
Telegram 查询和阈值设置命令。

## Phases

- [x] 核对当前监控配置、Bot 更新状态和跨服务器安全条件
- [x] 更新通知与 Bot 命令设计文档
- [x] 重构备份和生产监控通知文案
- [x] 实现 Telegram 命令控制器与参数校验
- [x] 建立 `rn01` 到 `hd0526` 的受限设置同步
- [x] 部署并验证命令、文案、权限和监控兼容性

## Verification

- 两台服务器监控服务执行成功，应用、数据库、Redis 和公开健康接口均正常。
- `rn01` 负责唯一 Bot 轮询，命令菜单已注册，旧更新已通过偏移量跳过。
- 受限 SSH 可读取 `hd0526` 监控状态和设置，任意额外命令被拒绝。
- 监控和设置隔离测试全部通过。

## Decisions

- 命令接收器只部署在 `rn01`，避免同一 Bot 更新被两台服务器竞争读取。
- 默认同时修改两台服务器，命令末尾可指定 `rn01` 或 `hd0526`。
- `rn01` 到 `hd0526` 使用只能操作监控整数设置的受限 SSH 用户，不使用 root 密钥。
- 日常消息只显示业务含义；技术详情继续保留在日志和状态文件中。

---

# 2026-08-03 Codex Responses 历史推理项 ID 兼容修复

## 目标

- 修正 Codex/OpenAI Responses 请求中 `reasoning` 历史项被错误写成 `item_*` 后触发的上游 HTTP 400。
- 对确定性的 `invalid_request_error` 停止账号切换，避免同一无效请求重复发送给多个账号。

## 非目标

- 不修改正常的 `rs_*` 推理项 ID。
- 不更改其他 Responses 项类型的 ID，也不放宽任意无效 ID。
- 本次只完成代码和本地验证，不执行生产部署。

## 阶段

- [x] 核对线上错误、现有转换链路和官方协议行为
- [x] 记录行为变化、影响范围和验证方式
- [x] 先补失败测试，再实现 `item_*` 到 `rs_*` 的限定转换
- [x] 补直接 HTTP 400 的账号切换回归测试并修正策略
- [x] 运行相关测试、格式检查和差异复核

## 验证方式

- 格式转换单元测试覆盖错误前缀、正常前缀和非推理项。
- 流式执行测试覆盖直接 HTTP 400 `invalid_request_error` 不切换账号。
- 运行相关 Rust 测试、`cargo fmt --check` 和 `git diff --check`。

## 错误记录

| 错误 | 处理 |
|------|------|
| 检索官方 Codex 源码时 shell 引号未闭合 | 拆成无嵌套引号的 `rg` 命令后完成检索 |
| 首次追加计划时补丁上下文选错 | 读取文件尾部后使用唯一上下文追加，不覆盖既有任务记录 |
| 首次使用 `--exact` 过滤测试时没有匹配完整模块路径 | 去掉 `--exact` 后确认测试先失败，再实现修复 |
| 网关测试首次编译超过工具单次 30 秒输出窗口 | 等待编译进程完成后重跑同一测试，完整记录失败与通过结果 |

---

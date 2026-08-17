# Findings

## 2026-08-16 支付小数金额完整入账：初始要求

- 用户确认问题不是单纯显示格式，而是支付回调金额存在小数时，实际钱包入账被取整，小数部分丢失。
- 正确结果是订单实付、回调确认金额、钱包入账流水、余额变化和页面显示保持同一精度，不能通过四舍五入或截断掩盖差额。
- 当前尚未确认具体取整位置和受影响渠道，必须先查代码和测试。
- 支付入口集中在 `handlers/public/support/payment`，充值订单创建在 `handlers/public/support/wallet/recharge.rs`，最终原子入账由三种数据库的钱包仓库完成。
- 初步搜索没有发现回调链路直接转换成整数；充值订单的法币应付金额目前统一保留两位小数，DodoPay 回调还存在单独的金额换算，需要继续核对字段单位和入账金额来源。
- 易支付回调会读取渠道返回的 `money`，再除以配置汇率得到 `amount_usd`；这一步仍是小数，没有转成整数。
- 通用回调也会原样将 `f64` 金额交给钱包仓库。因此真正的丢失点更可能在钱包仓库使用订单金额而不是回调实付金额，或在 DoDoPay 回调字段解析与订单金额更新之间。
- PostgreSQL 已确认直接根因：回调先校验渠道实付金额 `input.pay_amount`，校验通过后，钱包余额、累计充值和资金流水仍统一增加下单时的 `order_amount_usd`，完全没有使用回调已经换算出的 `input.amount_usd`。因此渠道实际付款与下单金额出现允许范围内的小数差额时，差额不会进入钱包。
- 支付订单回调成功后只更新 `pay_amount`、`pay_currency`、`exchange_rate`，没有将实际入账美元金额写回订单 `amount_usd`；订单、钱包和资金流水因此可能显示两套金额。
- 当前回调实付金额允许与订单渠道金额相差最多 `0.01`。还需确认 MySQL、SQLite、数据库金额字段精度和现有测试，再确定三种数据库统一的金额规则。
- MySQL 和 SQLite 与 PostgreSQL 使用同一错误规则：充值回调成功后都按 `order_amount_usd` 增加钱包余额和资金流水，而不是按 `input.amount_usd` 入账，问题影响全部数据库实现。
- PostgreSQL 生产结构已经支持所需精度：订单美元金额、钱包余额、累计充值和流水金额均为 8 位小数；渠道实付金额为 2 位小数，汇率为 8 位小数。根治不需要扩大数据库字段，只需统一实际金额的计算、持久化和展示。
- 现有 SQLite 回调测试只覆盖“回调金额与订单金额完全相等”，所以没有发现差额丢失。应增加回调实付金额与下单应付金额存在 0.01 渠道货币差额的用例，并断言订单美元金额、钱包、累计充值、流水及可退款金额完全一致。
- 充值下单时，服务端根据“用户填写的美元充值额 × 支付配置汇率”计算渠道应付金额，并四舍五入到渠道支持的 2 位小数；易支付和 DoDoPay 成功回调再用“渠道实际实付金额 ÷ 回调时使用的汇率”还原实际美元金额。回调已经得到精确到账金额，丢失发生在数据层改回使用原订单美元金额。
- 易支付发送给渠道的金额本身就是两位小数，DoDoPay 也按两位小数创建订单；因此渠道金额仍保持两位小数，美元到账金额按现有数据库能力保留到 8 位小数即可。
- 前端钱包专用金额函数默认固定显示 2 位小数；用户钱包中心和管理端支付订单都使用该函数。即使后端正确入账，超过 2 位的小数仍会被界面隐藏。
- 管理端资金流水及钱包操作抽屉还有多处固定显示 4 位小数，同样不足以显示数据库支持的 8 位精度。修复需要提供统一的钱包金额显示函数：最多显示 8 位、删除无意义的末尾 0、至少保留 2 位；金额值本身不做取整。
- 钱包专用金额函数目前只有用户钱包中心和管理端钱包页复用；用户列表、独立密钥钱包和钱包操作抽屉仍各自固定显示 2 位或 4 位。要保证“实际余额处处一致”，这些钱包入口也要改用同一个 8 位精度显示函数。
- 回调金额匹配用浮点数直接比较 `<= 0.01`；真实的一分钱差额可能因浮点误差变成略大于 `0.01` 而被错误拒绝。应按渠道金额的最小单位（分）比较，保留原有“最多相差 1 分”的业务规则。
- 新增真实边界测试后，旧实现确实在回调实付金额从 72.00 变为 72.01 时拒绝处理，先暴露了浮点金额比较问题；修复比较后，该测试还会继续验证实际美元小数是否完整入账。
- 回调处理原先使用“回调发生时的支付配置汇率”计算美元金额；如果管理员在用户下单后修改汇率，新配置会改变旧订单的到账金额。实际结算现改为始终使用订单保存的下单汇率，并保留订单原币种和原汇率。
- 最终审查发现回调币种没有与订单币种比较。现已在三种数据库实现统一校验：DoDoPay 使用渠道回调中的真实币种；易支付因回调没有币种字段，不再使用最新配置冒充，而是直接按订单币种和订单汇率解释实付金额。
- 旧订单缺少单独汇率但仍保存渠道应付金额和美元金额时，结算可由这两个订单字段还原下单汇率，不再依赖回调时的最新配置。
- 用户设置页原先仍使用普通费用格式，最多只显示部分小数；现已与钱包中心统一使用钱包金额格式。
- PostgreSQL 精度测试原先只有设置环境变量时才真正执行，但持续集成没有调用它；现已加入 PostgreSQL 数据库检查任务，缺少数据库或 SQL 错误会直接使检查失败。

---

## 2026-08-15 管理端套餐可见性与刷新时间：初始结论

- 管理端用户列表接口当前批量读取认证、钱包、用量和用户组，没有读取套餐；前端 `User` 类型和“钱包”列也没有套餐摘要，因此“只能看到钱包”是功能缺失，不是缓存问题。
- 单用户钱包详情会调用套餐可用性查询，但只返回总额、已用和剩余，不返回额度窗口起止时间。
- 套餐额度代码按套餐生效时间计算滚动 24 小时窗口；生产用户 `cwt1312132649` 当前每日窗口为 13:35:29 至次日 13:35:29。
- 正确接口应由服务端返回窗口开始和结束时间；用户列表使用当前页用户 ID 一次批量查询，避免逐用户查询。
- UI 方向：Web 管理后台、紧凑数据表格、延续现有组件和颜色；用户列表增加独立“套餐”列，套餐中心在现有额度信息旁显示下次刷新时间。
- 审查确认展示和扣费原先可能在管理员修改套餐开始时间后使用不同窗口；现已统一使用尚未结束的数据库窗口，窗口结束后再按当前套餐时间计算新周期。
- 新增摘要查询原先存在扫描旧套餐全部扣费明细的风险；现已改为只读当前窗口，缺少当前窗口记录时直接按已用 0 处理，不再查询历史扣费明细。
- 套餐摘要属于附加显示数据；读取失败时继续返回套餐和用户列表，并明确标记读取失败，不能将故障伪装成“无套餐”。
- 管理员套餐操作完成后必须重新读取用户列表，否则弹窗内容已更新而列表套餐列仍是旧数据。

---

## 2026-08-13 老套餐实时跟随套餐号池：最终结论

- `billing_plan_providers` 是所有有效套餐当前号池的唯一来源；`user_entitlement_providers` 只保留购买或发放时的历史记录，不再参与新请求是否可使用套餐的判断。
- 模型范围由当前启用的套餐号池、启用且可用的供应商模型关系、启用的全局模型共同决定。套餐号池为空时，新请求不能使用套餐。
- 请求开始前保存本次实际使用的号池和资金来源；此后修改套餐不会改变已经开始的请求。为安全上线，旧版本已经保存且号池为空的请求仍按旧规则完成结算。
- 请求路径没有增加数据库访问次数；套餐可用性仍是一次查询。后台用户套餐列表从逐条查询号池改为同一查询读取，数据库访问次数减少。
- PostgreSQL 关键用例在隔离本地数据库真实通过；数据层全量测试唯一失败来自远端已有迁移测试的无效插入数据，不属于本次修改。

## 老套餐实时跟随套餐号池（2026-08-13）

- 生产中的四个有效套餐此前没有配置号池；用户已经在后台完成号池配置。
- 当前代码将购买或发放时的号池写入用户套餐记录，套餐模板之后变更不会影响老用户。
- 请求可用性查询和用户模型列表都读取用户套餐记录中的旧号池；两处必须一起改，避免“界面可见但请求不可用”或相反。
- 套餐模板号池应成为唯一实时来源；套餐模型由这些号池当前启用并可用的模型关系计算。
- 请求开始时现有计费准入记录会保存本次实际使用的套餐和号池范围，可以保证套餐变更不破坏执行中的请求结算。
- 用户套餐编辑接口和界面当前允许单独覆盖号池，这与新规则冲突，需要删除该能力，只保留时间和额度调整。
- UI 工作范围是现有 Web 管理控制台的紧凑用户套餐编辑弹窗；保留现有结构和状态，只删除单用户号池选择。

---

## 2026-08-15：rn-hybrid PostgreSQL 无感切换实施

- 用户已同意复审迁移方案，确认无问题后执行；本轮仅迁移 PostgreSQL，Redis另行处理。
- 目标不是绝对 0 秒等待，而是切换窗口内查询在 PgBouncer 等待、不返回 5xx，计划内 PostgreSQL 数据丢失为 0。
- 正式提升前必须完成同版本物理复制、稳定代理、两台 Frontdoor 代理入口切换、新主机 R2 备份恢复验证和双主防护。
- rn01 使用 `postgres:15`，当前实际版本 15.18，`wal_level=replica`、10 个 WAL sender 和10 个复制槽均已可用，不需要重启旧主库；当前没有复制连接或复制槽。
- rn01 数据校验和未启用，时间线为 1；不妨碍建立物理副本。`max_slot_wal_keep_size=-1` 表示复制槽可无限保留 WAL，在仅剩约 20 GiB 磁盘的主机上危险，建立复制前必须设置明确上限并持续监控。
- 当前 HBA 允许所有远程普通数据库用户通过 TLS/SCRAM，但 `all` 不匹配物理复制伪数据库；需要单独增加仅允许 rn-hybrid WireGuard 地址的 `hostssl replication` 规则。
- hd0526 与 OVH 的 Frontdoor 都通过 Compose 运行，连接池最多 10、获取连接超时 5 秒。hd0526 使用较新的镜像 `9f2959a...`，OVH 仍使用 `d5ecd7a...`，旁路实例必须沿用各节点当前镜像，不能在迁库时夹带版本升级。
- hd0526 的 Compose 将 Frontdoor `AUTO_PREPARE_DATABASE` 硬编码为 true，覆盖 `.env` 中的 false；迁移旁路实例必须显式关闭自动迁移，避免通过事务池执行 DDL。
- PgBouncer 的 `PAUSE` 在会话池模式必须等待客户端断开，无法暂停 SQLx 长连接；正式方案必须使用事务池模式。Ubuntu 24.04 提供 PgBouncer 1.22.0。
- PgBouncer 1.22 支持在事务池中跟踪协议级命名预处理语句，需显式设置非零 `max_prepared_statements`。Niffler SQLx 缓存上限为 100，代理设置 256 可覆盖现有使用。
- 代码检查没有发现运行时依赖 `LISTEN`、会话级 `SET`、会话级 advisory lock 或跨事务临时表；现有 `SET LOCAL`、事务级 advisory lock 和 `ON COMMIT DROP` 临时表均位于事务内，与事务池兼容。迁移和回填不通过代理执行。
- 正式代理应集中运行在 rn-hybrid：Frontdoor 先无感迁到 `rn-hybrid PgBouncer → rn01`，切换时 PgBouncer 暂停事务、旧主库干净停止、提升本机副本、更新代理后端并恢复。这样应用连接地址不在提升时变化。
- `wg-db` 使用新的 `10.72.0.0/24`，与 rn01/OVH 的 `10.71.0.0/30`、hd0526/DMIT 的 `10.89.0.0/30` 和现有 Docker 网段均不冲突。
- rn01 当前 PostgreSQL 镜像摘要为 `postgres@sha256:67dc02...`，实际 15.18；新主机将固定使用同一摘要，避免浮动 `postgres:15` 在迁移时获得不同补丁版本。
- Niffler 生产数据库登录用户为 `niffler_app`，当前 15 条应用连接；PgBouncer 只需要为该用户复制 SCRAM verifier，并为本机控制单独创建管理员认证。
- rn-hybrid 维护前基准通过：session2sub2api、Nginx、PostgreSQL 16 均 active，本机反向代理返回 301，公开 HTTPS 返回 200，现有 PostgreSQL 16 可查询。
- rn-hybrid 系统更新与基础加固已成功执行，配置备份位于 `/root/niffler-rn-hybrid-prepare-20260814T171324Z`。重启前现有服务全部正常，Docker 已启用 live-restore，PgBouncer 保持 disabled/inactive，数据库端口没有对公网放行。
- rn-hybrid 已确认运行 6.8.0-137；重启后两轮检查未发现失败服务或 error 级启动日志。现有 `session2sub2api` 数据库大小为 704,838,679 字节，与维护前约 672 MiB 一致。
- 四节点 `wg-db` 已验证可用，客户端仅路由中心节点 `/32`，rn-hybrid 持有三个独立 peer `/32`；这阻止客户端借中心节点互相通信。rn01 的 5432 私网入口又以 WireGuard peer 来源和 iptables 两层限制为仅允许 `10.72.0.1`。
- rn01 私网 TCP 转发不终止 TLS、不持有数据库凭据，也不重建原容器；它只将 `10.72.0.2:5432` 字节流转到本机原 PostgreSQL。实测 rn-hybrid 通过该入口识别到 PostgreSQL 15.18 并接受连接。
- systemd socket proxy 不保留原 TCP 来源地址，PostgreSQL 看到的复制连接来源是 rn01 本机公开接口地址。HBA 因此不能按 `10.72.0.1` 匹配；正确边界是外层只允许 rn-hybrid 进入 `wg-db` 转发口，内层 HBA 只允许专用 `niffler_replica` 从 rn01 本机 `/32` 以 TLS/SCRAM 发起物理复制。
- rn01 将 HBA 作为单文件绑定挂载；对宿主机文件执行原子 `mv` 后，运行中的容器继续持有旧 inode，容器 mountinfo 明确显示来源后缀 `//deleted`。这类配置不能只替换路径后 reload；需要保留 inode 原地写入，或同步当前挂载 inode，或重建容器。本次为避免数据库中断，采用同步当前单文件挂载并立即恢复只读。
- 新副本数据目录约 56 GiB，宿主机仍有约 388 GiB 可用；旧主库基础备份期间始终保有约 21 GiB 可用，复制槽保留 WAL 仅为 MiB 级，没有接近 8 GiB 上限。
- 物理副本运行 PostgreSQL `server_version_num=150018`，`pg_is_in_recovery=true`、事务只读、WAL receiver 为 streaming，连续两轮落后 0 字节；新旧系统标识完全相同。八组关键表计数也一致，基础副本硬门槛通过。
- PgBouncer 运行时验证证明事务池兼容当前生产账号：命名预处理语句没有出现 `prepared statement does not exist/already exists`，事务临时表和 advisory xact lock 正常；暂停期间 `SHOW POOLS` 明确出现 1 个等待客户端，恢复后查询继续完成。
- 新逻辑备份达到约 32.52 GiB，远大于前一天约 2.49 GiB。主要原因是 `usage_body_blobs` 总占用约 27 GiB，gzip 正文在逻辑导出时几乎不能再次压缩；该发现不改变本次迁库数据范围，历史正文不在迁移窗口中删除。
- R2 新备份 `20260814T192002Z` 为 34,917,062,653 字节，远端大小和 SHA-256 均与本地一致。第二次隔离恢复完整通过：127 张公开表、62 个迁移、317 个用户、303 个 API Key、29 个 Provider，用量表存在且可读；第一次失败没有再次出现。
- rn-hybrid 根分区在隔离恢复自动清理后仍有约 388 GiB 可用。新备份定时器已启用，rn01 因剩余空间不足而失败的旧定时器已停用；旧备份实现和文件没有删除。
- 最终阶段复核中，副本仍为 PostgreSQL 15.18、`pg_is_in_recovery=true`，接收和重放位置一致、差值 0；旧主库复制槽 active，发送状态 streaming。PgBouncer active 且只监听 `10.72.0.1:6432`，网站和健康接口均返回 200。
- hd0526 旁路首次启动暴露 SQLx 会发送 `extra_float_digits=2`；PgBouncer 1.22 随包官方文档说明默认仅接受其可跟踪参数，其他参数必须显式列入 `ignore_startup_parameters`。加入该单项并 reload 后，旁路健康、四个数据库接口和两轮公网检查全部通过。
- OVH 旧镜像的嵌入迁移清单止于 `20260804120000`，数据库却已成功应用 `20260809130000`；因此旧镜像任何新容器都会返回 `VersionMissing(20260809130000)`。关闭自动迁移不能绕过兼容检查，也不应删除数据库迁移历史。OVH 旁路需使用 hd0526 已运行和验证的精确镜像，旧 OVH 容器在验证前不动。
- SQLx 0.8.6 的 PostgreSQL migrator 使用会话级 `pg_advisory_lock`/`unlock`；应用关闭自动迁移时仍执行这段兼容检查。事务池不保证两个语句落到同一后端，hd0526 首次启动因此留下空闲迁移锁。通过只在新进程首次启动时临时使用独立会话池别名、健康后将别名改回事务池并在无用户流量时 KILL/RESUME，可以让进程不重启地重连事务池；最终运行的三个别名均为 transaction。
- 两台 Caddy 的单文件绑定仍引用旧的 deleted inode，宿主机 Caddyfile 与运行容器内容不完全相同。热切换脚本分别保存和原地更新宿主机文件及容器当前挂载 inode，校验后调用 Caddy 管理接口 reload；失败时两份都恢复，避免重建 Caddy 中断现有连接。
- hd0526、OVH Caddy 均已指向旁路 Frontdoor；新实例和原实例都 healthy。按容器 IP 检查，两台旧 Frontdoor 当前均无 Caddy 上游 TCP 连接，说明热重载前的存量流已经自然结束。
- 旧主库在三个代理别名暂停、活动事务为 0 时干净停止；其控制文件最终 checkpoint 为 `118/CF8B8A0`。副本接收并重放到 `118/CF8B918` 后才提升，计划内数据丢失为 0。新主库 WAL 文件前缀为时间线 2，旧主库保持 `shut down`、时间线 1。
- 切换后三个 PgBouncer 别名都指向 `127.0.0.1:55432`，新主库直连和每个别名的临时写事务均成功；关键计数不低于切换前，最新用量时间继续前进。两轮公网验证均为 200，应用日志 5xx 为 0，代理和数据库没有切换后的新增错误。
- 迁移后的部署收尾已完成：正式 Compose、环境文件和 Caddy 均描述稳定名称 Frontdoor、唯一 Background 与 rn-hybrid PgBouncer，完整 `docker compose up -d` 已实际验证。
- 不修改应用代码的简化方案可行：三个应用入口固定为会话连接后，当前 SQLx 启动迁移锁可以在同一后端连接上正常释放。平时最多约 25 条应用连接，发布时两个 Frontdoor 新旧并行理论上限 45 条，低于 PostgreSQL 100 条总上限并保留余量。
- PgBouncer 从事务连接改为会话连接时，旧应用连接不能原样继续复用；实测旧 Frontdoor 短暂出现 `prepared statement does not exist`，新建连接后立即停止且公开接口无 5xx。以后任何连接方式变更都必须配套重建应用连接。
- Compose 管理的两台 Frontdoor 和唯一 Background 均 healthy，名字带 `*-next` 的容器已停止并设置 `restart=no`。新服务日志没有数据库连接或迁移错误，PgBouncer 无等待，PostgreSQL 无残留 advisory lock。

---

## 2026-08-15：Niffler 前端冷启动优化与 rn-hybrid 数据库评估

- 数据库事故恢复后的最新真实 Chrome 冷启动基准为：入口 TCP 约 0.80 秒、首包约 0.57 秒、首次绘制约 3.19 秒、首次内容绘制约 6.81 秒；5 个首页接口均约 0.44–0.53 秒。当前问题已从数据库等待收敛为独立的前端静态资源与启动链问题。
- 冷启动共请求约 52 个资源，传输约 767 KB、解压后约 1.84 MB；入口脚本约 269 KB 传输、864 KB 解压，另有多个 UI、Vue、工具库分块和多种字体。需要继续确认关键路径、缓存命中和源码初始化顺序后再定优化项。
- rn01 当前约 59 GB PostgreSQL 数据、96 GB 系统盘和 5.8 GiB 内存，已经发生过磁盘写满；PostgreSQL 容器共享内存仅 64 MiB。这些是评估 rn-hybrid 是否具备安全余量的最低对照标准。
- 生产 HTML 在解析入口脚本前同步预加载 4 个字体文件：Tiempos Text Regular/Medium 与 Styrene A Regular/Medium。四个字体会与入口脚本、CSS 同时竞争冷启动连接和带宽，是否全部属于首屏必要资源需要按实际文字字重核对。
- 生产构建已经对 Vue、工具库和 UI 库设置 `modulepreload`，首页布局和首页组件仍通过动态导入加载，因此至少存在“入口与三组公共包 → 公共布局 → 首页组件”的模块发现链。
- 当前带内容哈希的静态脚本只返回 `Cache-Control: max-age=14400`（4 小时），第一次检查为 Cloudflare `MISS`；这类不可变文件本可安全缓存一年。入口 HTML 保持 `DYNAMIC` 可以接受，但哈希资源缓存策略明显偏保守。
- 本轮真实 Chrome 冷会话复测：HTML 连接和回源在 1.84 秒结束，入口脚本与三个公共包在 3.10 秒结束，随后才发现公共布局、首页和约 24 个布局依赖，约 3.46 秒完成，首次内容绘制为 3.64 秒。模块二次发现额外占约 0.54 秒，慢网下会进一步放大。
- 同一会话热刷新首次内容绘制仅 0.524 秒，所有静态资源均由浏览器缓存直接提供；这证明页面渲染和接口本身不是固定需要 6–7 秒，首访网络、资源缓存和模块发现顺序是主因。
- `PublicLayout` 在首页尚未点击登录时就同步导入并挂载完整 `LoginDialog`；该组件挂载后立即并发请求注册设置、认证设置和 OAuth Provider，还带入表单、校验、OAuth 图标等一批模块。首页首次加载的 5 个接口中有 3 个完全由隐藏登录框触发。
- 首页本身还同步导入三个可视化组件，并在挂载后立即请求最多 1000 条公开模型；这些内容不应阻塞首屏英雄区。`PublicHeader` 和 `PublicFooter` 各自调用同一个站点信息组合函数，需确认其请求是否已做共享去重。
- 字体已经使用 `font-display: swap`，不会强制隐藏文字；真正的问题是 HTML 无条件预加载 4 个字重，首屏随后还请求 Bold 与 Semibold，共计 6 个西文字体、约 260 KB。中文文字主要仍由系统中文字体显示，预加载收益有限。
- `rn-hybrid` 是 RackNerd KVM 虚拟机，不是独立物理机：6 vCPU（Xeon Gold 6152）、15 GiB 内存、16 GiB swap、单块 500 GB 虚拟磁盘，根分区可用约 445 GB。当前负载接近空闲，内存可用约 14 GiB，磁盘仅使用 2%。
- `rn-hybrid` 的磁盘由虚拟机呈现为旋转介质（`ROTA=1`），系统内看不到 NVMe、硬盘 SMART 或 RAID 阵列；因此容量明显够用，但目前没有证据证明随机写、同步提交延迟和宿主机共享 I/O 足以长期承载主数据库。
- 该机并非空机：已运行 Nginx、Uvicorn 和宿主机 PostgreSQL，PostgreSQL 仅监听 `127.0.0.1:5432`。没有安装 Docker。将 Niffler 数据库放上去会与现有业务共享 15 GiB 内存和同一块虚拟磁盘，必须先确认现有服务用途与隔离方案。
- 相比 rn01 的 5.8 GiB 内存/96 GB 磁盘，rn-hybrid 的容量和当前空闲度大幅改善；相较此前规划的 64 GB 内存、冗余 NVMe 主数据库物理机，它只能作为过渡方案，不能因“空间够”直接视为长期主库。
- `rn-hybrid` 现有 PostgreSQL 16.14 只存放 session2sub2api，数据库约 672 MB、8 条空闲连接；默认 `shared_buffers=128MB`、`max_connections=100`。现有业务常驻内存约 250 MB，机器当前确实有足够内存容纳 Niffler 的现状规模。
- `rn-hybrid` 空闲采样时虚拟盘延迟很低，但采样期几乎没有数据库负载，不能代表高并发同步写性能。公开 SSH 正持续受到自动扫描；迁移时数据库端口不能开放到公网，必须建立 WireGuard 私网并限制来源。
- 网络实测显示 `rn-hybrid ↔ hd0526` 平均约 1.02 ms，二者适合放在同一主业务链；`rn-hybrid ↔ OVH` 平均约 24.98 ms，而 OVH 到现有 rn01 WireGuard 也是约 24.87 ms。因此迁移不会改善 OVH 入口延迟，但不会明显恶化；主站当前位于 hd0526，主要流量可保持低延迟。
- rn01 最新只读检查为 PostgreSQL 总库约 55 GB、根分区仍为 79%且仅余 20 GB；PostgreSQL 15.18 常驻约 550 MB。rn-hybrid 的约 445 GB 可用空间相当于当前数据库约 8 倍，作为止住 rn01 容量风险的过渡主库足够。
- 静态资源第二次单独检查中，仅入口脚本已成为 Cloudflare `HIT`；Vue、公共布局、首页分块和字体仍为 `MISS`，SVG 为 `REVALIDATED`。边缘节点首次遇到每个分块时仍需回源，4 小时后又会重复，直接放大“很多小分块”的代价。
- 主入口同步包含中英文两份完整词典，源文件合计约 644 KB；用户一次只使用一种语言。它是入口脚本解压后约 864 KB 的明确大头，应按当前语言只装一份，另一份在切换语言时再加载。
- 禁用所有 Web 字体后，另一次冷会话仍要先完成入口包，再等待约 1.7 秒的公共布局/首页分块，首次内容绘制没有变快。由于这次源站首包也更慢，不能拿绝对秒数直接对比；可以确定字体不是唯一根因，分块二次发现和回源等待仍是主链。
- 源码中的静态文件服务直接使用 Tower `ServeDir/ServeFile`，没有设置静态缓存响应头；两台 Caddy 也没有为 `/assets/*` 添加缓存规则。生产上的 4 小时缓存是 Cloudflare 默认结果，不是有意设计。
- 首页 HTML 的 `<div id="app"></div>` 完全为空，用户必须等入口脚本、公共包、公共布局和首页分块全部下载执行后才看到真正文字。这解释了“首次绘制较早、首次内容绘制明显更晚”；增加极小的静态首页外壳或预渲染能直接缩短可见等待。
- hd0526 到 rn-hybrid 平均 0.869 ms，到 rn01 平均 0.640 ms，差异只有约 0.23 ms，可忽略。迁移到 rn-hybrid 不会导致当前主入口数据库网络变慢。
- rn-hybrid 当前没有启用 UFW，nftables 规则为空；SSH 允许 root 密钥登录、禁用密码登录。系统时钟同步正常，但积压了大量系统更新，并明确要求重启到新内核。它不能在当前状态下直接承担主库，必须先维护重启和加固。
- rn-hybrid 没有业务备份定时器；现有 PostgreSQL 也没有归档 WAL。Niffler 切换前必须先迁移 R2 备份、执行恢复验证，并建立连续复制或可接受的停机窗口。
- rn01 的 PostgreSQL 已具备 `wal_level=replica`、10 个 WAL sender 和复制槽默认能力，适合建立 PostgreSQL 15 同版本流复制以缩短切换停机时间；不应直接把 55 GB 生产库导入 rn-hybrid 现有 PostgreSQL 16 实例并与 session2sub2api 混用。
- 当前 R2 自动备份最近一次在 2026-08-14 04:47 CST 成功，压缩包约 2.49 GB；下一次计划在 2026-08-15 04:31 CST。备份脚本要求本地可用空间至少为数据库原始大小再加 10 GiB，当前约 55 GB 数据库需要约 65 GB，而 rn01 仅余 20 GB，因此下一次备份按现有规则必然会在导出前失败。这使迁出 rn01 或先重构流式备份成为紧急项。
- 每份语言词典源码压缩后约 96–104 KB。默认只同步加载中文、英文按需加载，保守可从首访入口传输中减少约 96 KB，并减少约 329 KB 的解析输入；进一步按公开页/后台拆词典还能继续缩小入口。

---

## 2026-08-14：Niffler 首页卡顿诊断

- 目标是定位用户打开 `niffler.org` 时的当前卡顿，不把模型接口首字问题混入首页加载问题。
- 诊断将分别检查网络握手、Cloudflare/源站首包、静态资源瀑布、前端执行和首页接口请求；当前尚未形成根因结论。
- `https://niffler.org/` 连续 5 次为 HTTP/2 200，TTFB 0.643–0.905 秒，总耗时 0.643–0.906 秒，HTML 压缩后 574 B；当前采样没有复现“入口 HTML 首包几十秒”。
- `us1.niffler.org` 首页 TTFB 0.698–1.029 秒，`us2.niffler.org` 为 0.632–1.012 秒；三条 Cloudflare 入口的首页连接速度同一量级。
- 三个 Cloudflare 域名均解析到 `104.21.2.230` / `172.67.129.200`，本次由 LAX 边缘节点响应；`api.niffler.org` 直接解析到 `23.19.228.223`。
- 三个首页响应都有 `cf-cache-status: DYNAMIC`，入口 HTML 没有被 Cloudflare 缓存，每次打开都需要回源。
- 主站和 `us2` 的 `Last-Modified` 是 2026-08-14 05:16 UTC；`us1` 是 2026-08-05 19:07 UTC，双入口部署版本已不一致。
- `us1/us2 /__niffler_latency` 正常返回 204；主站同一路径返回 200 和首页 HTML，主站没有该探针路由。这不直接造成首页卡顿，但会影响任何误用主站探针的测速口径。
- `agent-browser` 首次启动失败，原因是本地 Playwright 对应的 Chromium 文件缺失；属于检测环境问题，不是 Niffler 页面失败。需要改用已安装 Chrome 或安装对应浏览器后继续采集真实页面瀑布。
- 本机已安装 `/Applications/Google Chrome.app` 和 Microsoft Edge；`agent-browser` 支持通过 `--executable-path` 使用现有浏览器，因此无需下载新的浏览器即可继续只读诊断。
- 使用本机 Chrome 冷启动访问主站后成功复现卡顿：导航总耗时 8.70 秒，首次绘制 7.44 秒，首次内容绘制 10.13 秒。
- 导航时序中 DNS 已缓存为 0，但 TCP/HTTPS 建连阶段记录为 5.77 秒；服务端首包等待约 0.71 秒。入口慢的第一部分发生在浏览器建连，而不是 Vue 渲染。
- 入口脚本约 269 KB 压缩 / 864 KB 解压，加载 1.40 秒；全部 52 个子资源约传输 762 KB、解压 1.84 MB。资源规模不算极端，且静态资源等待时间受前面的慢连接支配。
- 页面完成初始资源加载后同时请求 5 个首页接口。`registration-settings`、`oauth/providers`、`auth/settings`、`global-models` 均约 5.38–5.43 秒，`site-info` 约 10.29 秒；这些等待远大于脚本执行时间，是页面继续变慢的主要证据。
- 5 个接口出现整齐的约 5 秒和约 10 秒阶梯，像超时、重试或受限并发，不像普通数据库查询随机变慢；仍需用直连 API、Cloudflare/源站对照和生产日志确认具体层级。
- Chrome 首次连接比强制 HTTP/2 的 curl 慢约 5 秒，同时服务端声明了 HTTP/3 `Alt-Svc`。需要验证本地/部分用户网络的 QUIC（HTTP/3）尝试是否先失败再回退 HTTP/2，不能仅凭这一条时序直接定论。
- 本地 Caddy 配置显示 `niffler.org`、`us2.niffler.org` 和灰云 `api.niffler.org` 最终都直接反向代理同一个 `niffler-frontdoor:8084`；主站 API 没有额外的跨域代理层。
- 并发复测首页 5 个公开接口时，Cloudflare 主站和直连 `api.niffler.org` 同样慢，排除了 Cloudflare 是接口 5–12 秒延迟的主要原因。
- `oauth/providers` 明确返回 HTTP 500：`postgres error: pool timed out while waiting for an open connection`，TTFB 约 5.8 秒。生产 Frontdoor 的 PostgreSQL 连接池正在等待超时。
- `registration-settings`、`auth/settings`、`global-models` 等接口等待约 5.7–6.3 秒后返回 HTTP 501，正文为对应公开路由未在 Rust Frontdoor 实现；结合相同的 5 秒阶梯，需要检查这些请求是否先因数据库池超时、随后又走到未实现结果。
- `site-info` 在 Cloudflare 主站约 10.83 秒、直连 API 域名约 12.11 秒后才返回默认站点信息；大致等于两次连接池等待超时，说明该接口可能连续尝试了两个数据库读取或一次读取加回退。
- 当前主要故障已收敛到源站 Frontdoor ↔ PostgreSQL/连接池；仍需生产只读检查区分数据库不可达、数据库连接数耗尽、应用池设置过小或连接泄漏。
- 生产应用机资源没有打满：负载约 0.05–0.15，可用内存约 4.3 GiB；`niffler-frontdoor` 约 110 MiB、健康检查显示 healthy。该健康检查只耗时约 60 ms，显然没有覆盖数据库可用性，因此容器“健康”不能代表业务正常。
- Frontdoor 连接的 PostgreSQL 和 Redis 都位于 `192.129.155.207`；敏感凭据没有输出。
- Frontdoor 日志显示大量真实用户 `/v1/responses`、`/v1/models`、`/v1/messages`、`/v1/chat/completions` 请求全部在约 5000–5003 ms 后返回 500。故障不只影响首页，当前模型 API 也在大面积失败。
- 数据库主机 `192.129.155.207` 的 SSH 仍可达，但系统负载为 5.08/4.53/4.45；需要继续检查 PostgreSQL、连接数、锁和资源占用。
- 根因已经确认：数据库服务器根分区 `/dev/vda2` 为 96 GB，已使用 95 GB，剩余显示 0，使用率 100%；不是 inode 耗尽（inode 仅使用 6%）。
- `niffler-postgres` 处于重启循环，已累计失败重启 85 次，明确日志为 `FATAL: could not write lock file "postmaster.pid": No space left on device`。PostgreSQL 从至少 22:14 CST 起无法启动。
- `niffler-redis` 运行但健康状态为 unhealthy。Redis 每十几秒触发一次 RDB 后台保存，均因 `No space left on device` 失败；反复 fork/保存使其一度占用约 100% 单核和 66% 内存。
- 数据库机 5 个 CPU 核的负载约 5.2，内存仅剩约 232 MiB 空闲，3 GiB swap 已使用约 2.8 GiB；CPU 时间约 31–38% 用户态、41–46% 内核态。Redis 失败重试正在扩大故障影响。
- 完整因果链：磁盘写满 → PostgreSQL 无法创建锁文件并退出 → Frontdoor 的 SQLx 连接池没有可用连接 → 每个请求等待 5 秒池超时 → 首页多个接口阶梯式等待 5–12 秒且报错，模型 API 大面积 500。
- 空间构成显示 PostgreSQL 数据卷约 49.5 GB，Redis 数据卷异常达到约 34.9 GB。Redis 卷内除了正式 `dump.rdb`（约 4.36 GB）和 AOF（基础约 4.26 GB、增量约 4.06 GB），还堆积了多个 `temp-*.rdb` 失败快照，单个约 0.35–4.14 GB。
- 应急恢复先清理了约 168 MB 的未使用 Docker 镜像和约 592 MB 的历史 systemd 日志，但空间随 Redis 失败保存继续增长，仍不足以启动 PostgreSQL。
- 随后短暂停止 `niffler-redis`，只删除 Redis 数据卷根目录下的 14 个 `temp-*.rdb` 失败临时快照；正式 `dump.rdb`、AOF 和 PostgreSQL 数据目录均未删除。该操作释放约 22 GB，根分区从 100% 降到 81%，可用约 19 GB。
- PostgreSQL 重启后完成崩溃恢复并记录 `database system is ready to accept connections`；容器已恢复 healthy。
- Redis 从约 5.3 GB 的 AOF 基础 RDB和约 4.06 GB 的增量 AOF 完整加载，日志记录加载 831 个键并 `Ready to accept connections tcp`；容器已恢复 healthy。
- 恢复后两轮线上复测：`site-info`、`oauth/providers`、`global-models`、`auth/settings` 均返回 200，多数 TTFB 约 0.70–0.91 秒；`/v1/models` 未认证请求约 0.75–0.81 秒返回预期 401，不再 5 秒后报 500。第二轮 `oauth/providers` 有一次 2.70 秒，仍需继续观察。
- 数据库服务器内存仍很紧张：Redis 约 4.18 GiB，3 GiB swap 几乎用满。当前已恢复，但 96 GB 磁盘和 5.8 GiB 内存都缺少安全余量，不能将应急清理视为长期解决。
- 恢复后全新 Chrome 会话：首页 TCP 建连约 0.79 秒、入口首包等待约 0.52 秒、首次绘制约 2.58 秒；5 个首页接口均在约 0.30–0.44 秒完成。数据库故障造成的 5–12 秒接口等待已消失。
- 同一全新 Chrome 会话的首次内容绘制仍约 7.95 秒，虽然比故障时约 10.13 秒改善，但页面静态资源/前端启动仍有独立慢点；这不影响“数据库故障已恢复”的结论，后续应作为第二问题继续优化。
- Redis 第一次恢复后约一分钟内又被内核 OOM killer（内存不足保护机制）杀死；证据为内核日志 `Out of memory: Killed process ... redis-server`，当时 Redis 常驻匿名内存约 4.8–5.0 GiB。退出不是磁盘再次写满。
- 为应急恢复新增 `/swapfile-niffler-emergency` 6 GiB 交换文件，原 3 GiB swap 分区仍保留；该文件当前已启用但尚未写入 `/etc/fstab`，重启后不会自动启用。
- Redis 第二次启动加载完成后，通过运行时 `CONFIG SET save ""` 临时关闭定时 RDB 快照；`appendonly=yes`，AOF 仍启用。该设置没有写回配置，容器重建/重启后会恢复原 RDB 计划。
- 扩容 swap 后 Redis 仍在加载/启动阶段发生过一次 OOM 并自动重启，随后恢复 healthy；当前 Redis 约 3.7 GiB，系统总 swap 9 GiB 中约使用 4.5 GiB。说明机器处于严重内存压力，不能只靠 swap 长期运行。
- 最新线上两轮验证仍正常：首页 200，公开接口 200，未认证 `/v1/models` 预期 401；多数 TTFB 约 0.61–1.08 秒，`global-models` 有一次 1.98 秒，未再出现固定 5 秒连接池超时。
- 临时关闭 RDB 后仍发现一个约 137 MB 的 `temp-97.rdb`，应是关闭计划前已启动的后台保存子进程；需要继续观察其完成/消失，确认不会再次累积。
- PostgreSQL 容器默认共享内存只有 64 MiB，日志出现动态共享内存扩容失败。这是独立配置风险，会使并行查询失败，不是本次根分区磁盘满，但需要后续调整容器 `shm_size`。
- `usage:events` 的配置仅按条数执行 `XADD MAXLEN ~ 2000`，没有按字节限制；事件结构直接内嵌客户端请求体、上游请求体、响应体和客户端响应体。对于长 Codex 会话，一条事件可达到 MB 级，2000 条上限仍可能占用数 GB。
- 线上 Redis 消费组在一次检查中约 1980 条事件、1979 条 pending、lag 约 6；同一当前消费者持有几乎全部 pending。队列 worker 按 200 条批次顺序写数据库，只有处理成功后才 XACK + XDEL；因此大正文事件会长期占用 Redis。
- 队列 live 数据量剧烈波动：曾从约 8.4 GB 降至 172 MB，随后一分钟又升至约 3.35 GB；当前 `used_memory` 逻辑数据约 8 GB，但 RSS 约 3.6 GB，其余大量换入 swap。`MEMORY PURGE` 无法解决，因为不是单纯内存碎片。
- Redis AOF 在数分钟内增长到约 9.5 GB，基础 AOF 约 6.9 GB；每个 XADD、XACK、XDEL 都会继续追加。在当前事件速率下，即使队列条数稳定，AOF 也会持续快速增大，直到下次重写。
- 第二次磁盘止血在确认 `rdb_bgsave_in_progress=0`、`aof_last_write_status=ok` 后，删除已被更新 AOF 取代的旧 `dump.rdb`（约 4.36 GB）和不再活动的 `temp-97.rdb`（约 137 MB），磁盘从 96% 回到约 92%。这两个文件已删除，不能从本机恢复；当前 Redis 状态继续由已验证的 AOF 保存。
- 最新事件抽样不读取正文内容，只统计序列化体积和脱敏标识：最近 8 条 completed 事件约 0.19–1.96 MB，集中在 3 个 API Key，模型主要为 `gpt-5.6-sol/luna`。说明快速增长来自正常请求的完整正文重复进入计费队列，不像单个异常文件。
- Background 日志显示数据库恢复后连接池健康；另有大量历史 pending settlement 因缺少 billing admission 反复失败，但没有出现 `usage_worker_*_failed`。队列瓶颈更像处理吞吐低于前台产生速度，而非 worker 已退出。
- 最终触发配置已确认：2026-08-14 20:20 CST，`request_record_level` 被设为 `full`，请求和响应正文上限均为 16 MiB，描述为“供图片链路诊断”。这使每个 UsageEvent 重复携带最多四类完整正文，是三小时内耗尽 Redis、PostgreSQL 和磁盘的直接业务配置原因。
- 2026-08-15 00:05 CST 已将 `request_record_level` 持久化改回 `basic`。新请求继续记录用户/API Key 归属、Provider、模型、令牌、费用、状态、错误和耗时，不再保存完整请求/响应正文。
- 为保留停机期间的计费事件，先暂停 Background，并用 Redis 原子脚本逐条处理截至固定流 ID 的旧事件：清除四类正文及正文引用、设为 disabled，保留其余事件字段，XACK/XDEL 原条目后以新 ID 重新加入精简事件。共处理 1676 条，旧 JSON 约 7.075 GB，精简后约 21.7 MB；没有直接丢弃整条计费事件。
- 精简期间 Frontdoor 保持在线；其他活跃 consumer 已同步处理精简后的事件。恢复 Background 后，新事件体积稳定在约 10–15 KB，而故障时单条最高抽样约 33 MB，证明 `basic` 已生效。
- Redis live Stream 最终降至 `xlen=0`、`pending=0`、内存约 52 KB；历史队列已全部处理完成。
- 在 live 数据缩小后手动执行 `BGREWRITEAOF` 成功，AOF 从约 9.72 GB 压缩到约 419 KB；恢复自动 AOF 重写比例 100，AOF 持续启用。RDB 定时快照本次运行中仍临时关闭。
- AOF 重写后磁盘降到 85%；移除本次创建的 6 GiB 应急 swap 文件后，磁盘进一步降到 79%，可用约 20 GB。原有 3 GiB swap 分区保留，当前仅约使用 201 MB。
- 最终两轮间隔验证：磁盘均为 79%，Redis/PostgreSQL 均 healthy 且无 OOM；Frontdoor/Background 均 healthy；没有新的数据库连接池超时。
- 最终线上验证：首页和 4 个公开接口均返回 200，TTFB 约 0.60–1.04 秒；未认证 `/v1/models` 约 0.64–0.70 秒返回预期 401。数据库故障链已经恢复。
- 最终全新 Chrome 冷启动复测：首页 TCP 约 0.80 秒、入口首包约 0.57 秒、首次绘制约 3.19 秒、首次内容绘制约 6.81 秒；5 个首页接口均约 0.44–0.53 秒。数据库超时已消失，但前端静态资源/启动链仍占约 6–7 秒，是独立的后续性能问题，本轮应急恢复没有改前端。


## 2026-08-14 Codex OAuth Compact 与图片身份收敛

- 官方 Codex 的 Compact 客户端使用普通 HTTP 执行并解析一次性 JSON，不使用 SSE；同时会携带当前 Responses 相同的 `session-id` 和 `thread-id`。
- Niffler 当前把 Compact 客户端的 `stream:true` 直接传成上游流式要求，并向同格式上游发送 `Accept: text/event-stream`；官方 Codex OAuth Compact 必须单独固定为普通 JSON，客户端需要流式时由 Niffler转换返回格式。
- Compact 和 Responses 内置 `image_generation` 都属于当前 Responses 任务，必须按入站 `thread-id` 映射为同一个收敛后 `thread_id`。
- 没有 `thread-id`、会话标识或已解析任务标识的独立请求不能使用 Niffler API Key ID 代替任务；否则同一 API Key 的所有独立请求会被永久合并。正确回退是每个入站请求生成内部任务编号，同一请求的 Provider 重试继续复用。
- 当前 `turn_id` 每个 HTTP 请求重新生成，忽略客户端已有回合编号；工具后续调用会被拆成多个回合。应优先稳定映射客户端回合编号，没有时再生成请求级编号。
- 当前 `prompt_cache_key` 使用任务级 `thread_id`；官方 Codex 默认使用会话级 `session_id`。身份收敛目标是一个账号一个稳定会话、多条独立任务，缓存身份应使用收敛后的 `session_id`。
- 现有收敛核心只接受最终格式 `openai:responses`，因此 Compact 会在核心内直接跳过。
- Codex 独立图片入口的最终格式仍标为 `openai:image`，但请求正文已经转换为 Responses 的 `image_generation` 工具格式，上游地址也是 `/v1/responses`；该入口当前没有调用最终身份收敛函数。
- 管理端“测试模型”的图片请求使用另一条专用执行链，也需要显式调用同一收敛核心。
- 官方 Compact 请求正文包含 `prompt_cache_key`，但不包含 `client_metadata`；安装、会话、任务和窗口身份通过请求头传递。
- Compact 的 `x-client-request-id` 标识本次压缩操作，应保留现有值，不改为稳定的 `thread_id`。
- 用户已决定普通 Responses、Compact 和图片请求完成身份改写后都删除客户端 `x-oai-attestation`，Niffler 不生成替代证明。
- 同格式请求的完整请求头转发会保留客户端 `x-stainless-*`；普通转发会过滤该前缀，但两条链行为不一致。`sec-ch-*` 和 `sec-fetch-*` 在两种转发方式中都可能保留。
- 当前统一 `User-Agent` 明确声明 Ubuntu 22.04 x86_64，因此客户端自报的 SDK、浏览器、操作系统和架构请求头可能产生矛盾。删除这些 HTTP 环境标识不会删除请求正文、工具定义或回合元数据中的工作区和沙箱信息。
- Niffler 执行计划中的 `stream` 表示是否以流式方式向客户端交付，不等同于上游必须返回 SSE。Compact 可以继续走客户端流式执行，同时通过 `Accept: application/json` 和报告上下文明确定义上游为完整 JSON。
- 上游 Compact JSON 可能使用分块传输且没有 `Content-Length`。只按响应长度判断会误把 JSON 分块当成 SSE，因此必须根据已确定的上游模式完整缓冲，再执行 Compact 专用 JSON 转流。
- 通用 Responses 转流会忽略它不认识的 `compaction` 输出；Compact 必须逐项发送 `response.output_item.done`，否则客户端收不到压缩后的历史内容。
- 复核后的任务关系是：同一 OAuth 账号共享安装和会话身份；同一 Responses 任务的普通请求、Compact 和图片请求共享任务身份；不同独立任务仍使用不同任务身份。

---

## 2026-08-13 Niffler 服务器采购计划

- rn01 当前 5 核、5.8 GiB 内存、96 GB 系统盘，数据库约 25 GB；2026-07-28 记录约为 16 GB，容量增长仍需持续监控和保留策略。
- 当前最大关系为 `usage` 约 9.95 GB、结算快照约 5.37 GB、HTTP 审计约 5.03 GB；数据库容量主要被业务记录和审计数据占用，不是用户主表。
- rn01 Redis 当前约使用 2.07 GB，没有 `maxmemory` 上限，策略为 `noeviction`；迁移时必须一起处理，不能只迁 PostgreSQL 后继续让 OVH 应用跨州访问 Redis。
- hd0526 当前磁盘使用 82%，Frontdoor、Background 和 Caddy 合计内存不到 200 MB；OVH VPS 有 8 vCPU、约 23 GiB 内存和 186 GB 可用磁盘，应用层达到当前规模的 3 倍也不需要升级。
- OVH VPS 到 rn01 的当前平均往返延迟约 24.94 ms；主应用和主数据库必须同地区，才能避免已经确认的动态接口延迟。
- 当前注册用户 317，近 30 天有请求用户 96、请求 1,254,294 次；按当前规模的 3 倍规划为 951 名注册用户、288 名月活请求用户和 3,762,882 次请求/月。
- 近 7 天每分钟峰值为 992 次、第 95 百分位为 176 次；按 3 倍分别为约 49.6 次/秒和 8.8 次/秒，现有 OVH VPS 应用资源仍足够。
- 推荐新购 OVH Hillsboro Advance-1 作为 PostgreSQL/Redis 主节点，现有 OVH VPS 作为主 Frontdoor 和唯一 Background；hd0526/rn01 组成迁移期洛杉矶灾备组合。
- 按 3 倍负载和图片/视频工作台规划，数据库物理机必须 64 GB ECC 起步；32 GB 不再作为正式目标配置。
- 数据库 16 天内从约 16 GB 增至 25 GB，当前平均约 0.56 GB/天；若按 3 倍线性增长约为 616 GB/年，因此 2×960 GB RAID1 只能作为实施数据保留策略后的最低配置，首选 2×1.92 TB RAID1 或 4×960 GB RAID10。
- 2026-08-13 GorillaServers 洛杉矶 EPYC 4245P、64 GB、2×960 GB NVMe、1 Gbit/s 页面显示 1 台，价格 119 美元/月，适合作为后续洛杉矶数据库从库。
- 2026-08-13 GorillaServers 洛杉矶 Ryzen 7950X3D、128 GB、3.84 TB NVMe 页面显示 1 台，价格 179 美元/月；单盘不适合主数据库，但适合 FFmpeg 临时处理节点。
- 图片和视频使用独立私有 R2 存储桶及签名地址，媒体不写 PostgreSQL、不经过 Frontdoor；工作台公开上线前需要采购专用 FFmpeg 节点。
- 视频业务没有正式生产数据，首期以 30,000 张图片/月、3,000 个 1080p 视频任务/月和 4 个 FFmpeg 并发作为采购场景，必须在上线后按真实任务时长与文件大小复算。
- 首期媒体场景约新增 570 GB/月；保留 90 天约存储 1.71 TB，按 R2 Standard 当前单价估算约 25.50 美元/月，不含超额操作费用。
- rn01 的约 100 GB 磁盘只能作为迁移期从库；数据库达到 55 GB、rn01 磁盘达到 65%、工作台全面开放或日请求达到当前 2 倍时，应提前采购长期洛杉矶从库。
- PostgreSQL 异步从库只能提供灾难恢复，整个 Hillsboro 突然失联时可能丢失尚未复制的最新事务；当前不应描述为零数据损失自动高可用。
- 大陆用户不能依赖普通 Cloudflare 全球网络作为唯一低延迟路径；建议使用搬瓦工洛杉矶 E-Commerce+SLA 作为独立三网优化入口，线路包含电信 CN2 GIA/CTGNet、联通 AS10099 和移动 CMIN2。
- 2026-08-13 搬瓦工 160G E-Commerce+SLA 页面配置为 6 个 AMD 独享 vCPU、8 GB ECC、160 GB NVMe RAID10、5 TB/月、5 Gbit/s，109.99 美元/月；节点只转发网站和 API，不运行 Niffler、数据库或 FFmpeg，也不转发视频大文件。
- 香港、东京或新加坡只增加 Frontdoor、却继续访问 Hillsboro PostgreSQL/Redis，会让动态请求反复跨太平洋；当前亚太普通用户先使用 Cloudflare，只有亚太流量达到 40% 且网络时间成为主要耗时后才建设完整亚太应用和数据库区域。
- Cloudflare China Network 要求 Enterprise、额外订阅、有效 ICP 备案/许可证和内容审核，当前项目不满足前置条件。
- 媒体处理机改为 OVH 新加坡 Advance-3 2024：12 核 24 线程、64 GB ECC、2×960 GB NVMe、1 Gbit/s/25 TB，官方页面约 S$254.99 至 S$275.99/月；异步 Worker 不直接连接美西数据库或 Redis。
- 媒体 R2 桶应在首次创建时使用 `apac` 位置提示并启用 Local Uploads；大陆到 R2 的真实大文件测试不合格时，再增加阿里云香港 OSS 和传输加速。
- 视频不经过普通 Cloudflare 网站代理或三网入口：Free/Pro 单请求体上限为 100 MB；上传使用对象存储分片直传，预览使用 HLS/DASH 小分段和带鉴权的媒体分发域名。

---

## 2026-08-12 Niffler Codex OAuth 设备指纹收敛方案

### 2026-08-13 审查修复范围

- 保留四项审查问题：统一出站 Codex 客户端版本身份；将父任务和 fork 来源任务标识映射到同一账号命名空间；管理端“测试模型”请求执行同样的收敛；Codex 配置异常不影响其他 Provider。
- 撤回两项：不增加非官方 `thread_id` HTTP 请求头兼容；完整请求记录继续保存实际出站正文，不对正文身份字段做隐藏。
- 实施必须先更新架构设计记录，然后修改代码并运行相关验证。

### Requirements

- 参照 sub2api v0.1.175 的 Codex OAuth 设备指纹收敛行为分析 Niffler。
- 先分析并给出方案，本轮不实现。
- 结论必须区分代码事实、版本差异、合理推断和待确认项。
- 用户进一步明确实际目标：同一 Codex OAuth 账号被多个 Niffler 用户共用时，减少上游可见的稳定身份数量，使请求尽量呈现为一个账号由一个人使用；现有 Codex OAuth 账号必须在功能启用后自动生效，不能要求逐个补配置。
- 因此前次“旧账号缺少模式按 off”和“保留每个 Niffler 用户各自的 conversation_id”两项裁决不符合目标，现已重新打开评审，不能继续作为最终方案。

### Initial Constraints

- 当前工作区已有大量用户改动和既有调查记录；不得覆盖、恢复、暂存或提交。
- 认证与设备标识属于安全敏感链路，不能用未经源码证明的字段猜测方案。
- 需要同时核对首次授权、令牌刷新、正常请求、账号导入导出和多节点运行，避免只修一个调用点。

### Research Findings

- 本机已有 `../sub2api`，远端为 `Wei-Shaw/sub2api`；本地工作区有其他未提交内容，因此后续全部用 `git show v0.1.175:<path>`、提交差异或只读临时工作树取证，不依赖当前文件内容。
- `v0.1.175` 是注释标签，对应合并提交 `93c32fa1a2450351561abc46156d2e28cb5f74ca`，提交时间为 2026-08-12 18:52:01 +08:00，提交主题为合并 PR #5553 `feat/codex-fingerprint-convergence`。
- 标签说明明确声明四档策略 `off/device/session/full`，收敛 `installation_id`、`session_id`、`thread_id` 等标识，目标是减少上游可见设备数和会话数。
- 当前宽泛全文检索混入大量历史设计资料，不能据此判断实现；下一步只比较该合并提交的两个父提交并读取标签对象中的改动文件。
- PR #5553 相对第一父提交只改 8 个文件，共增加 914 行：301 行后端实现、460 行后端单元测试、27 行网关接入，其余是账号新建/编辑/批量编辑界面和中英文文案；没有数据库迁移，也没有改 OAuth 授权或刷新令牌代码。
- 配置存入 OpenAI OAuth 账号的 `extra.codex_fingerprint_mode`。合法值为 `off/device/session/full`；缺失、空值或非法值均按 `session`，非 OpenAI OAuth 账号强制 `off`。界面不保存默认值 `session`，显式 `off` 才关闭，因此升级后所有既有 OpenAI OAuth 账号会自动启用 `session`。
- `installation_id` 优先使用账号既有 `device_id`，否则以数值 `account.ID` 加固定命名空间字符串做 SHA-256，再截成 UUIDv4 格式；`session_id` 同样按账号 ID 确定性派生。这里是“稳定派生”，不是新增持久化指纹列。
- `session` 模式的 `thread_id` 由“账号 ID + 客户端原始 session-id”确定性派生；客户端没有 session-id 时退回账号级 `session_id`。`full` 模式直接令 `thread_id == session_id`。两种模式均每个请求生成新的 UUIDv7 `turn_id`，`window_id = thread_id + ':0'`。
- 改写同时覆盖 HTTP 头和请求体 `client_metadata`，并复用同一个请求级 ID 对象保证 `turn_id` 一致。涉及头：`x-codex-installation-id`、`x-codex-window-id`、`x-client-request-id`、`session-id`、`session_id`、`thread-id`、`x-codex-turn-metadata`；涉及请求体：`x-codex-installation-id`、`session_id`、`thread_id`、`turn_id`、`x-codex-window-id` 及内嵌 `x-codex-turn-metadata`。
- 接入点只在非 compact 的 OpenAI 网关转发路径：解析完成后修改请求体，并将同一指纹对象暂存在当前 Gin 请求上下文；构造上游请求、完成白名单头复制后再改写头，之后才执行 OAuth 出站身份终态校验。
- 单元测试覆盖稳定 UUID、模式默认值、device_id 优先、不同账号隔离、客户端会话到线程的稳定映射、缺失 session-id 回退、四档头与体改写，以及头/体随机 `turn_id` 一致性。当前改动列表没有显示网关级集成测试或前端组件测试。
- Niffler 是 Rust 工作区，Codex OAuth 不是单体服务：OAuth Provider 在 `crates/aether-oauth`，账号/密钥数据在 `aether-data`，请求格式在 `aether-ai-formats`，计划与请求发送在 `apps/aether-gateway`，管理界面在 `frontend`。不能照搬 sub2api 的单文件 + Gin context 方案。
- 精确标识检索显示，Codex 的 `client_metadata/session_id` 逻辑集中出现在 `crates/aether-ai-formats/src/formats/openai/responses/codex.rs` 和 `apps/aether-gateway/src/ai_serving/planner/standard/codex/tests.rs`；没有发现现成 `x-codex-installation-id`、`x-codex-window-id`、`x-codex-turn-metadata` 的生产实现文件。需要读取代码确认是缺失、动态复制还是测试夹具覆盖。
- Niffler 另有站内用户登录的 `client_device_id/session_id`，与上游 Codex 指纹不是同一概念；后续分析必须排除 `user_sessions`、管理端会话和 LinuxDo OAuth 等无关结果。
- Niffler 已有一套“部分稳定化”：Codex 请求缺少 `prompt_cache_key` 时，会按平台用户 API Key ID 生成稳定 UUIDv5；随后从该值做 SHA-256 截断，生成 16 位 `session_id` 和 `conversation_id`。这不是上游 OAuth 账号级，而是用户 API Key 级，因此多人共享一个 OAuth 账号时仍会产生多个上游会话。
- 现有头逻辑采用“缺失才补”：如果客户端原始头或 Provider 规则已经提供 `x-client-request-id/session_id/conversation_id`，Niffler保留它；缺失 `x-client-request-id` 时使用请求 `trace_id`，所以每次请求不同。与 sub2api 的“终态强制覆盖”语义不同，这正是指纹没有真正收敛的主要缺口。
- Niffler 的 Codex 请求体特殊处理位于 `aether-ai-formats`，已经接收 Provider 类型、API 格式、原始头和平台用户 API Key ID；管理/计划层负责把结果写入 `provider_request_body/provider_request_headers`。实现时可在这个现有终态处理附近扩展，不需要把请求级对象塞入 Web 框架上下文。
- 当前 Codex 特殊处理也覆盖 `chatgpt_web` 和 `openai:image` 相关桥接逻辑，compact 请求有独立字段限制。指纹功能的适用边界不能简单写成“所有 Codex 文件”，必须限定最终上游确为 Codex OAuth 的 Responses 请求，并单独决定 compact 和图片桥接是否携带这些标识。
- Niffler 的请求计划已经携带 Provider 配置、Endpoint 配置、Provider Key ID、解密后的 OAuth `auth_config`、原始客户端头和最终 `provider_request_headers/body`。这意味着指纹模式可以从现有配置层进入计划，不需要在执行阶段重新查数据库。
- Provider Key 管理界面当前将 OAuth 凭证视为 `auth_config`，并明确不把内容返回前端。若把指纹模式塞进加密凭证 JSON，管理端无法安全地只编辑模式；因此配置与凭证应分离，不能直接照搬 sub2api 的 `extra`，也不应要求前端回传整份 OAuth Token。
- 数据库已有 Provider Key 级 `fingerprint` JSON 字段，PostgreSQL/MySQL/SQLite 基线、仓库类型、增删改、系统导入导出和管理接口都已贯通；现有用途包括 Grok OAuth 浏览器传输配置和通用透传诊断。新增 Codex 指纹配置优先复用该字段，无需新增数据库列或迁移。
- `fingerprint` 是普通配置，不是 Token；后端新建/更新载荷接受 JSON 对象并会通过管理接口返回。Niffler 可在其中增加命名空间对象，例如 `fingerprint.codex`，避免与现有 `transport_profile` 冲突；前端常用类型仍需补字段声明。
- 现有 Codex 请求体函数只接收平台用户 API Key ID，头函数接收请求 ID和解密凭证，却没有接收 Provider Key ID 或 `fingerprint`。实现的关键改造是把选中 Provider Key 的稳定 ID 与指纹配置显式传进 Codex 最终处理，而非新增查库或全局缓存。
- 现有 `prompt_cache_key/session_id/conversation_id` 逻辑明确保留客户端和 Provider 规则值。启用收敛模式后必须在所有规则与格式转换结束后强制覆盖目标字段，否则规则或原始头仍会造成上游指纹分裂；`off` 继续保持当前兼容行为。
- `StoredProviderCatalogKey` 已把 `fingerprint` 作为账号传输字段，并随候选物化进入 `GatewayProviderTransportSnapshot.key`。同格式透传构造头时已经把该配置传给通用 Provider Header 构造器，因此无需扩展数据库查询或候选缓存结构；标准 OpenAI/Codex 计划还需要显式消费它。
- `fingerprint` 也参与系统导入导出，适合保存账号稳定配置。只用 Provider Key ID 作派生种子会让“导出到另一实例后”指纹随本地新 ID 改变；若目标是跨实例迁移仍保持同一设备，应该在 `fingerprint.codex` 中持久化独立随机 installation/session 种子或 ID，而不是完全依赖数据库主键。
- Niffler 的计划在选中账号后才解析 OAuth 认证，并在每个重试账号上重新生成该账号对应的计划。这正好满足账号级指纹：同一个用户请求换到另一 OAuth 账号时，应跟随新账号的指纹，不能把首个账号的指纹保存在请求全局状态里。
- 标准 OpenAI Responses、OpenAI Chat 转 Responses、通用格式转 Responses、Responses 图片桥接和独立图片路径分别调用 Codex 体/头处理；生产调用点分散在 `standard/openai/responses`、`standard/openai/chat`、`standard/family`、`specialized/image`。方案必须列出并测试所有最终到达 Codex Responses OAuth 的入口，不能只改一条主路径。
- 最合适的结构不是扩大所有通用格式转换函数签名，而是增加一个“选中账号后的 Codex 指纹终态处理器”：接受 Provider 类型、最终 API 格式、Provider Key ID、`fingerprint` 配置、原始客户端头、最终头和最终体，在 Body Rules、格式转换、模型指令和普通头规则之后执行。这样才具备账号信息，也能保证终态覆盖。
- `prompt_cache_key` 应继续按平台用户 API Key 隔离，不纳入设备指纹收敛。它目前用于提示缓存稳定性；sub2api v0.1.175 也没有改写该字段。收敛处理只停止用它派生上游 `session_id`，避免把多个用户变成多个上游会话。
- Header Rules 当前先于 Codex 特殊头执行。启用指纹模式时由终态处理覆盖规则对受控字段的值；未受控头和 `off` 模式保持原语义。文档必须明确这一优先级，避免管理员误以为 Header Rules 可以覆盖收敛标识。
- sub2api v0.1.175 的实际接入只发生在 OpenAI OAuth `Forward` 主路径，且明确跳过 compact；指纹对象存入请求上下文后由普通 `buildUpstreamRequest` 取用。单独的 OpenAI passthrough、WebSocket HTTP bridge 和其他直接调用构造器的路径没有在此次 PR 中新增解析步骤。参考实现本身不是“所有 Codex 出站路径全覆盖”。
- 因此 Niffler 的第一版应以明确契约为准：覆盖最终发往 Codex OAuth 的标准 `openai:responses` HTTP 请求，排除 `openai:responses:compact`、独立 `openai:image`、`chatgpt_web` 内部图片、WebSocket 和非 OAuth 账号；以后若要扩展必须先验证上游协议，而不是把 sub2api 的名字解释成全链路覆盖。
- Niffler 已支持 sub2api OAuth 账号导入，但当前设计明确只映射 `credentials` 中的 Token、邮箱、账号/用户/空间和套餐，并忽略未知字段；不会导入 `extra.openai_device_id` 或 `extra.codex_fingerprint_mode`。若希望 v0.1.175 迁移保持指纹，需要同步扩展导入契约与测试，否则导入后会使用 Niffler 默认策略和新生成的账号指纹。
- sub2api 的 `openai_device_id` 是账号 `extra` 中已有的可选真实安装标识，v0.1.175 在 device/session/full 模式优先沿用它；Niffler 当前 OAuth 凭证解析和导入没有等价的公开配置映射。
- Niffler 的 Key 新建/更新接口已经接受 `fingerprint` JSON，但只校验它是对象，尚未校验其中 Codex 模式或 ID。实现时需要增加命名空间级校验，保留已有 `transport_profile` 等字段；不能让编辑 Codex 模式时覆盖整个 `fingerprint`。
- OAuth 账号由专用 `create_provider_oauth_catalog_key` 建立，当前只为 Grok 自动生成浏览器指纹；刷新/重复导入更新账号时会保留已有 `fingerprint`。Codex 默认配置和可选安装标识应在这个专用创建/更新链路内合并，避免普通 OAuth 刷新清空模式。
- OAuth 批量导入的中间结构当前没有指纹字段。若兼容 sub2api v0.1.175 的 `extra`，需要只提取白名单字段 `codex_fingerprint_mode/openai_device_id`，传到 Provider Key `fingerprint.codex`；不得合并任意 `extra`，以维持现有安全边界。
- 已确认 sub2api v0.1.175 的账户导出结构会输出完整 `Extra` 字段，因此 `codex_fingerprint_mode` 与 `openai_device_id` 能从源导出文件中实际取得，不需要修改 sub2api。
- 账号编辑界面 `OAuthKeyEditDialog.vue` 是最合适的配置入口；它当前只编辑名称、优先级、限速、并发、缓存、备注和模型获取。增加四档选择时必须读取现有 `fingerprint`，只更新 `fingerprint.codex.mode`，并保持其他子配置原样。
- 通用请求头复制不会过滤 Codex 标识；除认证、转发、压缩等少数头外，`x-codex-*`、`session-id/session_id`、`thread-id`、`x-client-request-id` 都会原样进入 Provider 请求头。当前上游确实能看到不同客户端的设备与会话信息，不只是“缺少默认值”。
- 标准头构造顺序是：复制客户端头 → 应用解密凭证中的安全头覆盖 → 应用 Endpoint Header Rules → 重新保证认证头 → Codex 特殊头。指纹终态处理应放在最后一个 Codex 步骤内或其后，才能对受控标识有最终决定权，同时不影响认证头。
- 同格式请求使用更完整的头透传策略，跨格式请求使用普通透传策略，但两者都允许 Codex 标识通过；测试必须同时覆盖同格式 Responses 与 Chat 转 Responses，避免只在一种透传模式下收敛。
- sub2api 的请求级 `turn_id` 使用 UUIDv7，而头和体分别调用当前时间写 `turn_started_at_unix_ms`，极短时间内可能不同。Niffler 方案应在同一个请求级结构中同时固定 `turn_id` 和开始时间，保证所有载体逐值一致。
- 本机 OpenAI 官方 Codex 源码确认这些并非 sub2api 自造字段：当前官方客户端在 Responses 请求中使用 `session-id`、`thread-id`、`x-client-request-id`，并在头或 `client_metadata` 中使用 `x-codex-installation-id`、`x-codex-window-id`、`x-codex-turn-metadata`。
- 官方测试确认 `x-codex-turn-metadata` 包含 `installation_id/session_id/thread_id/turn_id/window_id/request_kind/sandbox/turn_started_at_unix_ms` 等字段，且 `client_metadata` 也承载安装 ID、窗口 ID和内嵌 turn metadata；因此头、体和内嵌 JSON 必须同步改写。
- 官方当前代码还使用 `x-codex-parent-thread-id` 表达子线程关系。sub2api v0.1.175 没有收敛这个字段；Niffler 第一版应明确保留它，而不是错误地把父子线程信息一并固定。若后续上游将其计入设备风险，需要另行设计父子关系映射。
- 官方当前 compact 请求也可能带上述标识，但 sub2api v0.1.175 明确跳过 compact。为忠实参考并降低协议风险，Niffler 首版仍跳过 compact，同时把它列为已知未覆盖项，而不是声称整个 Codex 客户端所有请求都已收敛。
- 官方源码将 `client_metadata['x-codex-turn-metadata']` 视为权威数据，扁平 `client_metadata` 和直接 HTTP 头只是兼容投影。Niffler 处理时必须优先保证内嵌 JSON正确，并同步生成扁平字段和头；只改头不算完整实现。
- 官方元数据还包含 `request_kind/compaction/forked_from_thread_id/parent_thread_id/subagent_kind/thread_source/sandbox/workspaces`。收敛处理只能改五个身份字段和时间，必须原样保留这些业务与拓扑字段，避免破坏子代理、审查、压缩和工作区行为。
- Niffler 的 OAuth 编辑对话框当前不知道 Provider 类型，同一组件被 Provider 详情页和号池页复用。若只对 Codex OAuth 显示四档选择，需要新增 `providerType` 属性并在两个入口传入，不能仅凭 `auth_type=oauth` 显示给 Kiro、Antigravity、Grok 等账号。
- 推荐范围固定为 `provider_type=codex && key.auth_type=oauth && final_api_format=openai:responses`；`chatgpt_web` 虽也可导入 OpenAI OAuth，但有独立内部图片/网页传输语义，本轮不启用指纹收敛。
- Niffler 的稳定 `prompt_cache_key` 源自 2026-04-09 的上游实现，命名空间明确写成 `user:<user_api_key_id>`，说明它的本意是用户级缓存隔离，不应改成 OAuth 账号级。
- Rust 工作区当前 `uuid` 只启用 `v4/v5`，sub2api 使用的请求级 UUIDv7 尚不可用。忠实实现需要在工作区依赖启用 `v7`，或者明确接受 UUIDv4；建议启用 `v7`，因为官方 Codex 当前窗口/请求标识也使用时间有序 UUIDv7。
- 账号级稳定 ID 建议继续采用 sub2api 的版本化确定性派生：以 Provider Key ID 为稳定种子，分别使用独立命名空间派生 installation/session/thread UUID。Provider Key ID 已进入每个候选且多节点一致，不需要新表、缓存或运行时写数据库。
- 可选 `fingerprint.codex.installation_id` 只用于沿用导入的真实/外部安装标识；没有时确定性派生。`session_id` 不开放手填，避免管理员造成头体不一致；如未来要求跨实例原样迁移，可通过现有系统导出保留 Provider Key ID，或再引入版本化持久种子。
- Niffler 当前没有 Responses WebSocket v2 出站实现，检索到的 `ws_request_header_*` 只是客户端元数据兼容字段；因此第一版没有 WebSocket 接入点要修改。HTTP `/v1/responses/compact` 是真实独立路径，必须通过显式格式判断跳过。
- Niffler 目前没有 Provider OAuth 账号批量编辑接口或界面，不能照搬 sub2api 的批量模式选择器。第一版只做单账号编辑；批量切换不是核心运行功能，也不应为本需求额外新增一整套批量 API。
- 前端 `EndpointAPIKey` 当前未在常用类型中显式暴露 `fingerprint`，尽管后端列表载荷会返回该字段。实现界面前需要补类型并核对序列化，不能用类型断言读取未知字段。
- Niffler 的 `codex` Provider 固定指向 `https://chatgpt.com/backend-api/codex`，并与 `chatgpt_web` 分离；按 Provider 类型限制功能不会误伤站内网页登录通道。

### Proposed Solution

- 目标角色：从 Niffler 后端架构、认证安全和运维稳定性视角设计；不把客户端设备标识、平台用户 API Key 和上游 OAuth 账号混为一层。
- 第一版配置使用现有 `provider_api_keys.fingerprint`：`fingerprint.codex = { mode, seed, installation_id? }`。`mode` 为 `off/device/session/full`；`seed` 是后端生成并持久化的随机 UUID，只用于稳定派生；`installation_id` 只保存 sub2api 导入或其他外部已有安装 ID。
- 与 sub2api 按数据库账号 ID 派生不同，Niffler 使用持久化 `seed`。理由是 Niffler 已有可导出的 `fingerprint` 配置，这样多节点、备份恢复和跨实例导入后仍保持同一指纹，不依赖本地数据库主键。
- 修正默认策略：若目标是参照 sub2api v0.1.175 保持相同语义，Codex OAuth 账号缺少模式时必须按 `session`，显式写入 `off` 才关闭。因此功能上线后，现有账号也会生效；若不希望某个账号生效，应在上线前或编辑时明确设为 `off`。不能一边声称参考该版本，一边让现有账号默认关闭。
- 新建 Codex OAuth 账号使用 `session + seed`。现有账号若采用持久 seed 方案，应在启用前一次性补 seed；这是数据补全，不是数据库结构迁移，不能等请求到来后临时写数据库。
- 管理接口仅接受四个模式；新增或启用非 `off` 模式时由后端补 `seed`，前端不能手填。非法模式在保存/导入时显式报错；运行时遇到损坏配置则该账号计划失败并给出不含指纹值的诊断，不能静默降级成另一种模式。
- 适用条件固定为 `provider_type=codex && auth_type=oauth && final_api_format=openai:responses`。首版明确排除 compact、独立图片、`chatgpt_web`、非 OAuth 账号和不存在的 Responses WebSocket 出站路径。
- 稳定 ID 使用版本化 SHA-256 命名空间并格式化为 UUIDv4：installation/session 按账号 `seed`；`session` 模式 thread 按 `seed + 原始客户端 session-id`，缺失时使用账号 session；`full` 模式 thread 等于账号 session。每个请求只生成一次 UUIDv7 turn、一次开始时间，window 为 `thread + ':0'`。
- 原始客户端会话只从未改写的请求头读取，顺序为 `session-id`、`session_id`。这一步必须在普通头复制前保存，不能读取已经收敛后的头。
- 终态处理器在 Body Rules、格式转换、模型指令、认证头和 Header Rules 全部结束后执行。启用模式时，其管理字段优先于客户端头及 Header Rules；`off` 完全保持现有行为。
- `device` 只覆盖 installation；`session/full` 还统一覆盖 HTTP 头的 window、client request、session、thread，以及 `client_metadata` 的 session/thread/turn/window。
- 修正 `conversation_id` 处理：Niffler 当前把它作为出站兼容请求头，并与 `session_id` 一样从用户级 `prompt_cache_key` 派生；sub2api v0.1.175 没有改写该字段，OpenAI 官方 Codex Responses 出站代码使用的是 `thread-id`，没有发送 `conversation_id` 请求头。因此不能未经验证就规定 `conversation_id = thread_id`。首选方案是在 `session/full` 模式删除该兼容头，只发送官方 `thread-id`；`off/device` 保持现状。只有真实上游兼容测试证明仍需要该头时，才把它明确作为 `thread-id` 的兼容副本。
- 现有 `x-codex-turn-metadata` 和体内嵌 JSON只改 installation/session/thread/turn/window/开始时间，保留 request kind、父子线程、子代理、沙箱、工作区等字段。遇到已存在但格式损坏的元数据时返回明确请求错误，避免头体身份不一致。
- 现有 `prompt_cache_key` 继续按平台用户 API Key 隔离；它服务于缓存，不属于 OAuth 账号设备身份。启用收敛后不再用它派生上游 session/conversation。
- OAuth Token 授权与刷新流程无需修改；账号切换重试会针对每个候选重新生成账号级指纹，同一候选内的重试复用同一请求计划和 turn 信息。
- 前端在 `OAuthKeyEditDialog` 增加四档选择，由两个调用入口传入 Provider 类型，只对 Codex OAuth 显示。保存时深度合并 `fingerprint.codex.mode`，保留 seed、installation 和其他 Provider 指纹配置；第一版不新增批量编辑。
- 实施前先新增架构文档并更新 sub2api 导入兼容文档，写明目标、非目标、模式语义、字段优先级、导入行为、影响范围和验证方法。
- 实现顺序：配置契约与校验 → 纯函数式指纹生成/元数据改写 → 选中账号后的终态接入 → OAuth 新建/编辑/导入导出 → 前端配置 → 集成验证与小范围启用。
- 核心验证矩阵包括：四档模式、缺少模式默认 session、显式 off 关闭、多节点稳定、跨导出导入稳定、不同账号隔离、session/full 线程映射、头体及内嵌元数据逐值一致、`conversation_id` 删除兼容性、规则覆盖顺序、非法配置/损坏 JSON、账号切换、同格式与 Chat 转 Responses，以及 compact/image/chatgpt_web/API Key 不受影响。
- 上线先在少量 Codex OAuth 账号启用 `device`，确认上游请求成功率、401/403/429、首字延迟和账号风险状态无异常，再切换 `session`；`full` 仅在确认上游线程标识不影响并发语义后单独启用。回退只需把账号模式改为 `off`，不涉及数据库回滚。

### Remaining Risks

- 收敛会让同一 OAuth 账号下多个平台用户共享 installation/session 标识；这正是目标，同时可能改变上游风控聚合方式，必须小范围观察。
- `full` 将不同客户端会话合并到同一 thread，可能影响上游并发或链路观测，默认不建议使用。
- sub2api 导入只能原样继承其 `openai_device_id`；其 session/thread 是按 sub2api 本地账号 ID 派生，导出文件没有原值，因此迁入 Niffler 后只能从新 seed 生成稳定的新会话身份。
- compact 在官方客户端中也可能携带这些标识，但参考版本明确跳过；首版保持该边界，并在文档中列为已知未覆盖项。

### Full Review Findings

- 上一版存在内部矛盾：一方面要求旧账号“缺少模式即 session”，另一方面要求非 off 模式必须有持久 seed，却没有可原子完成的旧账号 seed 补全过程。不能依赖请求到来时写库，也不能让缺 seed 的账号直接失败，否则发布后会同时影响全部旧账号。
- 重新核对后必须纠正：`provider_api_keys.id` 是创建时 UUID，但 Niffler 的系统配置导出结构 `AdminSystemConfigProviderKey` 不包含 ID；导入新建 Key 时会生成新 ID。此前“系统导入导出保留 Provider Key ID”的判断错误，不能用它承诺跨实例身份不变。
- 最终方案仍建议删除额外 `seed`，理由改为忠实参照和减少状态：sub2api 本身按本地账号 ID 派生，账号重建/跨系统导入也不会保留 session/thread。Niffler 使用版本化命名空间 + Provider Key ID，可保证同一数据库、多节点和普通重启稳定；配置导入、账号删除重建后 installation/session/thread 可能变化。源 `openai_device_id` 存在时只有 installation 能原样保留。
- “跨实例完整保持所有派生 ID”明确列为第一版非目标。如果产品以后要求这项能力，应新增显式 `derivation_id` 并设计旧账号补全与导出语义，不能在本功能中隐含承诺。
- 请求接入不能继续拆成“旧 body helper 改体 + 旧 header helper 改头”。Niffler 有 Responses、Chat 转 Responses、通用格式转 Responses等多条构造路径；最可靠的接口是一个同时拿到最终可变 body 和 headers 的 `apply_codex_oauth_fingerprint_convergence`，在每条候选计划返回前调用一次。
- 该终态函数必须在模型指令、Body Rules、格式转换和 Header Rules 后运行，并且在计划对象冻结前完成。所有请求级随机字段由同一次调用生成，不能在 body/header 两边各生成。
- 现有账号切换会为每个候选重新建计划，因此每个候选会使用自己的 Provider Key ID。请求因 401 刷新 Token 后若重放同一个已建计划，应保持同一个 turn_id；换到新候选则生成该候选自己的 turn_id，这是合理边界。
- 官方 Codex 当前将 `client_metadata['x-codex-turn-metadata']` 定义为权威值，但只在请求本来带有 turn metadata 时才表示一个有类型的 turn。Niffler 不应凭空伪造 `request_kind`、sandbox 或工作区信息；应只在现有 JSON 中替换身份字段，或只写扁平 client_metadata，不创建一个语义不完整的 turn metadata blob。
- `conversation_id` 的最终裁决应比上一轮更保守：sub2api v0.1.175 在指纹改写前会先按自己的原有隔离逻辑生成 `conversation_id`，新功能不删除也不覆盖它。因此“删除 conversation_id”也不是忠实参考行为。第一版应把它排除在受控字段之外，保持 Niffler 当前逻辑；只在后续独立兼容性任务中研究是否废弃。
- 这意味着 session/full 收敛后仍会保留一个按平台用户 API Key 变化的非官方兼容头。是否被上游计入设备/会话风险没有源码证据；方案必须把它列为待观测风险，不能擅自改写或删除。
- `openai_device_id` 在 sub2api 是账号已有的真实设备 ID，并非 v0.1.175 新生成。Niffler 从 sub2api 导入时可以继承它；Niffler 自身 OAuth 登录当前没有等价采集来源，所以未导入的账号应按 Key ID 派生 installation ID，不能声称是真实设备 ID。
- `session` 模式的客户端线程种子不能只读取 `session-id/session_id`：这只覆盖 Codex 原生请求。Niffler 允许 Chat、Claude、Gemini等请求转换到 Codex Responses；它们可能只有 body 会话字段或 `x-aether-session-id`。若直接回退账号 session，会把所有无 Codex 头的请求合成一个 thread。
- Niffler 已在路由前统一解析 `ClientSessionAffinity`，但当前 Codex 适配器漏读官方标准的 `session-id`，只读 `session_id/conversation_id`。最终方案不应再维护第二套手工优先级：先让 Codex 适配器按 `session-id → session_id → conversation_id → body` 识别，再统一使用路由后解析出的 affinity；现有显式 `x-aether-session-id` 仍按通用规则拥有最高优先级。
- `session` 模式 thread 的最终种子为规范化后的 `ClientSessionAffinity.session_key`；若没有任何会话信号，则回退平台用户 API Key ID，防止不同用户全部落入一个 thread。同一信号可能包含 account hint 和 agent，全部只作为哈希输入，不记录、不发送。`full` 模式忽略这些信号并令 thread 等于账号 session。
- 发现此前“所有生产调用点已覆盖”的判断过早：Niffler 对同格式请求优先走 `LocalSameFormatProvider` 路径，该路径目前不调用 Codex 特殊头 helper。只修改标准 OpenAI Responses/Chat/family 路径会漏掉最常见的原生 Responses 同格式请求。
- 最终接入清单至少包括四个候选构造出口：OpenAI Responses 专用、OpenAI Chat 专用、通用跨格式 family、同格式 Provider。每个出口都在最终 body+headers 已完成后调用同一终态函数；compact/image 由函数内部严格返回，不依赖调用方记得跳过。
- 管理端“模型测试”也会直接构造 Codex 请求，但它是诊断路径而非用户流量。若测试目标是验证账号真实可用性，必须使用同一终态函数，否则测试结果与生产不一致；不能只调用旧头 helper。
- 管理 Key 响应实际已经返回 `fingerprint`，此前“前端无法读取”不准确；缺口仅是 `EndpointAPIKey` 与更新类型没有声明它。OAuth 编辑框可以用当前返回值构造完整合并对象后一次 PUT。
- 后端当前 `fingerprint` 更新是整对象替换，不支持子字段 PATCH。前端必须基于打开对话框时的完整对象深拷贝并只替换 `codex.mode/installation_id`；后端还应提供 Codex 专用规范化函数，拒绝非法模式和非法 installation ID，同时保留其他命名空间。
- 当前号池已有通用 `regenerate_fingerprint` 动作，会把整个指纹替换成 Claude Code 随机结构。若该动作能作用于 Codex Key，会破坏 Codex 配置及其他命名空间；实现本功能前必须限制该动作的 Provider 范围或改为命名空间级重生成。这是现存数据破坏风险，不是可选优化。
- 因最终方案删除 seed，正常“轮换 Codex 指纹”不能依赖重生成 Key ID。第一版不提供轮换按钮；如未来需要，应持久化显式 installation/session override 或新建账号，单独设计，不能复用现有通用 regenerate 动作。
- 最终请求策略还会在候选请求构造完成后修改上游头和正文：路由策略可同时修改两者，受管理提示词还会改正文。因此“在候选计划返回前调用”仍不够精确，若调用早于 `apply_final_provider_request_policies_to_decision`，收敛结果可能被后续策略覆盖。
- 正确顺序必须是：先构造完整 `AiExecutionDecision`，再执行最终路由策略和受管理提示词，最后对 decision 中的最终 JSON 正文和头执行 Codex 收敛，然后立即返回。四条生产决策路径都要遵守同一顺序；管理端模型测试也要使用同一逻辑。
- 运行时不需要重新查询数据库判断是否为 OAuth：`GatewayProviderTransportSnapshot` 已同时携带 `provider.provider_type`、`endpoint.api_format`、`key.auth_type`、Key ID 和 `fingerprint`。对 Codex 来说，现有 OAuth 管理语义就是 `key.auth_type == oauth`；终态函数可以据此严格限定 `codex + oauth + openai:responses`。
- 该判断不能只看最终 URL 包含 `/codex`，也不能只看 API 格式；否则自定义 Provider、`chatgpt_web` 或非 OAuth Key 可能被误改写。

### Review Working Decisions（后续 Final Review Adjudication 为最终结论）

- **默认值与发布方式不能混写。** Niffler 历史账号缺少模式时按 `off`，只表示不执行新收敛逻辑；新建账号与 sub2api 导入条目显式保存 `session`。另设系统设置 `codex_oauth_fingerprint_convergence_enabled`，默认 `false`，作为统一启停和紧急回退开关。配置读取失败时保持旧行为并告警，不能阻断用户请求。
- **全局发布开关不是账号模式。** 关闭时即使账号明确配置为 session 也不改写请求；开启后只有明确配置非 off 的账号生效。旧账号无需上线前批量补 off，少量试用账号只需显式设置 device 或 session。
- **批量模式设置应纳入首版。** Niffler 现有号池批量动作已经有通用 `payload`，扩展一个只修改 `fingerprint.codex.mode` 的动作成本有限。它用于分批启用、批量停用和运营回退，不承担旧账号数据迁移。
- **运行时非法模式与管理写入要区分。** 管理 API、单账号编辑和 Niffler 系统配置导入遇到显式非法值时拒绝该条并给出清楚错误；运行时读取历史缺失配置时按 `off`，手工损坏配置也按 `off` 并告警。只有 sub2api 导入解析源文件时，缺失、空值或未知值才按源系统语义写成 `session`。
- **sub2api 导入只对白名单字段负责。** 仅当目标 Provider 是 `codex` 时读取账号顶层 `extra.codex_fingerprint_mode` 和 `extra.openai_device_id`；目标为 `chatgpt_web` 时继续忽略。源模式缺失、空或未知按 sub2api 自身语义解析为 `session`；有效 `openai_device_id` 写入 `fingerprint.codex.installation_id`。
- **重复账号导入按“源配置覆盖 Codex 子配置”处理。** sub2api 条目是账号替换输入，模式应覆盖现有 Codex 模式；源 `openai_device_id` 存在时覆盖 installation ID，不存在时保留 Niffler 现有 installation ID，避免因源文件未携带可选字段而破坏已经绑定的安装身份。其他 fingerprint 命名空间原样保留。
- **installation ID 不应强制 UUID。** sub2api 没有格式校验，真实来源通常是 UUID，但迁移兼容不能假定全部历史值都符合 UUID。建议只接受去除首尾空白后 1–128 字节、可作为 HTTP HeaderValue、且不含控制字符的字符串；超限或非法值拒绝导入/保存。
- **损坏的 turn metadata 不应让整次用户请求失败。** sub2api 会保留无法解析的值，但这会留下未收敛身份。Niffler 更稳妥的硬化方式是：已有值为合法 JSON 对象时保留非身份字段并覆盖受控字段；缺失时不新建；空值、非法 JSON 或非对象时删除该 metadata 字段，同时继续写入扁平身份字段并记录不含原值的告警。这样既不把客户端错误升级成服务不可用，也不会把无法确认的旧身份继续发往上游。
- **`client_metadata` 类型异常可安全重建。** 该字段缺失时建立对象；存在但不是对象时，用新的对象替换并记录告警，因为上游协议本来要求对象。不能尝试合并数组、字符串或数字。
- **父线程和 fork 关系首版保留。** `x-codex-parent-thread-id`、`parent_thread_id`、`forked_from_thread_id` 不在 sub2api v0.1.175 的管理字段内。首版只收敛五个已证明的身份字段，不自行发明父子 ID 映射；文档明确这可能暴露额外线程关系，是兼容优先的已知边界。
- **UUIDv7 是明确依赖变化。** 当前工作区只启用 `uuid` 的 v4/v5；要复现 sub2api 的 turn ID 语义必须增加 v7 feature，并测试同一 decision 内头、体和内嵌 metadata 共用同一个 turn ID/时间。
- **线程来源应复用 Niffler 的统一会话识别。** 前一版列出的独立头字段优先级会与已有 `x-aether-session-id` 优先语义冲突，也会重复维护 Claude Code、OpenCode 和 body 会话字段。正确做法是补 Codex 适配器对 `session-id` 的读取，然后使用已解析的 `ClientSessionAffinity`；只有没有 affinity 时才用平台用户 API Key ID。
- **终态接入宜收口为一个 decision 级函数。** 四条生产路径已经都调用 `apply_final_provider_request_policies_to_decision`。与其让每条路径各自再写一段 Codex 判断，建议把流程封装成 `finalize_provider_request_decision(state, input, transport, decision)`：内部先应用现有最终策略，再读取全局开关并执行 Codex 收敛。这样架构测试可强制四条路径调用同一个终结函数，避免将来新增路径漏掉。
- **执行计划阶段不能重新生成。** `AiExecutionDecision` 之后会转换成 `ExecutionPlan`，401/403 的 OAuth 刷新重试只替换计划中的认证头并原计划重发。因此 turn ID、开始时间和所有收敛字段会保持一致；换账号时重新构造新的 decision，才产生新账号 ID 和新 turn。这一行为符合请求重放边界。
- **管理端模型测试不经过 decision 终结函数。** 它直接构造 body、headers 和 `ExecutionPlan`，所以必须在 Header Rules 和认证头完成后显式调用同一个纯收敛核心；普通生产 wrapper 不能成为唯一入口。compact/image 仍由纯核心的适用范围检查跳过。
- **系统配置导入也要执行相同 fingerprint 校验。** 当前系统导入会在 overwrite/create 分支直接把导入的 `fingerprint` 整体写入 Key，仅验证它是对象。新增 Codex 命名空间校验若只放普通 Key API，会被系统导入绕过；应抽成共享规范化函数，普通新建、更新、系统导入、OAuth 导入和批量模式修改全部复用。
- **现有通用“重新生成指纹”动作必须限域。** 当前动作对任何 Provider 都把整个 fingerprint 换成 Claude Code 结构。首版至少限制为 `claude_code/claude_code_api` 且只替换 `transport_profile` 命名空间；对 Codex 返回明确“不支持该动作”，不能删除 `fingerprint.codex`。
- **批量模式设置可复用现有号池选择与 payload。** 前端已能选当前页或全部筛选结果，后端批量请求已有 `payload`。新增动作 `set_codex_fingerprint_mode`，payload 只允许 `{ "mode": "off|device|session|full" }`；后端还要校验 Provider 必须为 codex、所选 Key 必须为 OAuth，并对每个 Key 只合并 `fingerprint.codex.mode`。不适用账号应按失败项或明确跳过计数返回，不能静默写入。
- **全局开关应放在系统设置的安全/高级区域。** 默认关闭，文案必须说明“关闭时所有账号保持旧行为；开启后，仅明确配置为 device/session/full 的 Codex OAuth 账号执行收敛”。保存后使用现有系统配置缓存失效机制；同一入站请求只解析一次缓存值，不按候选重复查数据库。
- 该系统配置必须在后端共享配置解析处显式限定为布尔值并提供默认 `false`；当前通用配置更新对未知 key 不做类型限制，不能只依赖前端 Checkbox 保证类型。系统配置导出/导入应自然携带它，缺失仍为 false。
- **配置结构最终定为最小对象。** `fingerprint.codex = { "mode": "off|device|session|full", "installation_id"?: "..." }`；不包含 seed、session override、thread override 或轮换计数。旧账号没有 `codex` 对象时，界面展示“保持旧行为”，不要求数据迁移。
- **新建 Codex OAuth 账号显式写默认 session。** 没有外部 installation ID 时只保存 mode，运行时按 Key ID 派生；sub2api 导入也显式保存解析后的模式。OAuth Token 刷新永远不改 fingerprint。
- **`conversation_id`、`prompt_cache_key` 和五个收敛字段是三类对象。** `prompt_cache_key` 保持用户级缓存隔离；现有 `conversation_id` 兼容头首版保持原样，不映射、不删除；installation/session/thread/turn/window 才由模式强制覆盖。文档和测试不能再把 conversation 与 thread 写成同步关系。
- **模式的精确定义如下：** `off` 完全不修改任何指纹字段；`device` 只覆盖 installation（直接头、扁平 client_metadata、已有有效 turn metadata 中的 installation）；`session` 还覆盖账号级 session、按客户端 affinity 派生的 thread、每个 decision 新 turn 和 thread:0 window；`full` 与 session 相同，但 thread 固定等于账号 session。
- **session/full 的 HTTP 头必须同时写两种 session 名。** 固定写 `session-id` 和 `session_id`、`thread-id`、`x-client-request-id=thread_id`、`x-codex-window-id`；直接 installation 头为 `x-codex-installation-id`。请求体扁平字段使用 sub2api/官方兼容的 `session_id/thread_id/turn_id/x-codex-window-id/x-codex-installation-id`。
- **turn metadata 只改已有对象。** 头和 body 中已有合法对象时覆盖 `installation_id/session_id/thread_id/turn_id/window_id/turn_started_at_unix_ms`，其余字段原样保留；两处使用同一次生成的毫秒时间。缺失时不创建，损坏时按前述规则删除。
- **现有诊断脱敏不足，属于上线阻断项。** 请求 trace、runtime miss 和管理端模型测试当前只把认证/Token 类头当敏感数据，`session-id/thread-id/x-client-request-id/x-codex-*` 会明文进入诊断结果。实现必须将受控指纹头加入统一脱敏名单，并保证请求体中的 `client_metadata`/turn metadata 在正文日志级别下也不会明文暴露长期身份；至少诊断预览只显示 `<redacted>`。
- 可观测信息只记录：全局开关、解析后的模式、是否应用、Key ID、是否使用导入 installation、是否删除损坏 metadata、跳过原因和错误类别。不得记录派生种子、installation/session/thread/turn/window 原值，也不得把这些值加入指标标签。

### Final Review Adjudication

- **旧账号缺少配置的最终语义改为 `off`，但只关闭新功能，不影响账号认证、刷新或正常请求。** sub2api 的“缺失即 session”会在总开关打开时同时改变全部历史账号，迫使上线前先批量写 `off`，操作风险没有必要。Niffler 采用更安全的本地语义：既有账号没有 `fingerprint.codex.mode` 时保持旧请求行为；新建 Codex OAuth 账号显式保存 `session`；sub2api 导入按源语义显式保存 `session`。运行时未知或损坏模式同样按 `off` 处理并告警，管理写入则拒绝非法值。
- **全局开关继续保留。** 它不是账号模式的默认值，而是所有账号共同的紧急停止开关；默认 `false`。开关值在一次入站请求内只解析一次并由所有候选共用，避免请求进行中切换开关导致不同候选行为不一致。读取失败或类型错误按关闭处理，新请求最多受其他 Frontdoor 现有 3 秒缓存影响。
- **重复导入不能无条件覆盖。** 批量导入调用现有不允许覆盖活动账号的重复检测；活动重复账号会报错，失效、停用或过期账号才可替换。单条导入只有显式 `replace_existing=true` 才允许覆盖活动账号。仅当现有流程决定写入时，才合并 Codex 指纹：源模式缺失、空或未知写为 `session`；有效 `openai_device_id` 覆盖 installation；源中没有该字段则保留现有 installation；其他 fingerprint 命名空间不变。
- **`conversation_id = thread_id` 明确否决。** `conversation_id` 是 Niffler 现有兼容头，当前按平台用户 API Key 派生；`thread-id` 是官方 Codex Responses 线程标识。sub2api v0.1.175 的新功能没有同步、删除或覆盖 conversation_id。第一版保持 conversation_id 原逻辑，并把它仍可能暴露用户差异列为观测风险；后续若要删除，必须单独做上游兼容验证。
- **所谓“设备指纹收敛”只指 Codex Responses 的身份元数据，不是完整客户端伪装。** 它不改 OAuth 授权/刷新、Token、IP、TLS、User-Agent、Originator、prompt_cache_key、conversation_id、父线程字段或加密上下文，也不能承诺规避上游风控或账号限制。
- **派生输入采用版本化命名空间和长度前缀编码。** installation/session 按 Provider Key ID 派生，session 模式的 thread 再加入规范化后的 `ClientSessionAffinity.session_key`；无 affinity 时加入平台用户 API Key ID。SHA-256 截取 16 字节后设置 RFC 4122 v4/variant 位，避免简单分隔符拼接的歧义。全量模式 thread 等于 session；turn 每个候选 decision 生成一个 UUIDv7，所有载体共用同一 turn 和同一毫秒时间。
- **稳定范围被严格收窄。** 同一数据库、多 Frontdoor、普通重启和 OAuth Token 刷新保持稳定；系统配置导入会新建 Provider Key ID，因此未导入真实 installation ID 的账号会改变 installation/session/thread。第一版不承诺账号删除重建或跨实例导入后身份不变，也不增加 seed 或数据库迁移。
- **运行时异常按可用性优先且必须原子。** 管理接口阻止坏配置进入；若数据库被手工损坏，开关、模式或 installation 无法解析时，本次候选保持原请求不做部分改写并记录无敏感值的错误。客户端 `client_metadata` 类型错误可重建为空对象；已有 turn metadata 是合法 JSON 对象时覆盖身份字段，缺失时不伪造，非法或非对象时删除该载体并继续写扁平字段。整体请求大小继续使用现有限制，不为此功能再设未经验证的独立阈值。
- **改写必须是最终且大小写无关。** 先完成格式转换、Body Rules、Header Rules、路由修改和受管理提示词，再一次性处理最终 body/headers；受控头先大小写无关地删除全部重复形式，再写规范的小写名。Header turn metadata 使用 ASCII JSON，避免合法 Unicode 元数据重写后变成非法 HTTP 头。
- **接入采用两层结构。** 协议层提供不查库的纯派生/改写函数；网关层解析全局开关、Provider/Key/API 格式、账号配置和客户端会话，再调用纯函数。四条生产决策路径统一通过新的 decision 终结函数；管理端模型测试在自己的最终构造点调用同一纯核心。
- **精确适用范围固定为：** 全局开关开启、Provider 类型为 `codex`、Key 的认证类型为 `oauth`、最终上游格式为 `openai:responses` 或 `openai:responses:compact`。普通 Responses 同时改头和 `client_metadata`；compact 只改身份头与已有合法的 header turn metadata，不向 compact 正文增加其协议未声明的 `client_metadata`。`openai:image`、`chatgpt_web`、非 OAuth Key、模型列表和当前不存在的 Responses WebSocket 均不处理。
- **compact 不能照抄 sub2api 的遗漏。** v0.1.175 没有在 compact 路径调用收敛函数，但当前官方 Codex 会给 `/responses/compact` 发送 installation、session、thread、window 和 compaction turn metadata。Niffler 也支持该格式；若跳过，普通请求已收敛的同一会话会在压缩请求重新暴露原标识。compact 应沿用同一个账号/线程派生结果，但保留该端点自身的 `x-client-request-id` 请求语义，不强制改成 thread ID。
- **window 也不能固定为 `thread:0`。** sub2api v0.1.175 固定写 0，但当前官方 Codex 会在每次压缩后把窗口编号递增，并发送 `{thread_id}:{window_number}`。最终处理先从权威 body turn metadata、扁平 body、header turn metadata、直接 header 中依次读取合法的非负窗口编号，再用收敛后的 thread 重新组成 window；确实没有合法值时才用 0。这样既隐藏原 thread，又不会破坏压缩后的窗口状态。
- **四档字段契约保持参考版本的主体语义并修正窗口。** `off` 不改；`device` 只覆盖 installation；`session` 覆盖 installation、账号 session、按客户端会话派生的 thread、请求 turn 和保持原窗口编号的新 window；`full` 与 session 相同但 thread 固定等于账号 session。普通 Responses 的 session/full 写 `session-id`、`session_id`、`thread-id`、`x-client-request-id=thread_id`、`x-codex-window-id`，并在 `client_metadata` 和已有合法 turn metadata 中写对应字段；compact 不覆盖 `x-client-request-id`。`conversation_id` 和 `prompt_cache_key` 不属于受控字段。
- **配置只保存** `fingerprint.codex = { mode, installation_id? }`。无 seed、thread override、session override 或轮换计数。普通新建、更新、系统导入、OAuth 导入和批量操作必须复用同一后端规范化与命名空间合并函数；OAuth Token 刷新不得改 fingerprint。
- **Codex 子配置必须由服务端原子更新。** 现有普通 Key 更新会替换整个 fingerprint；运行时 OAuth 刷新还有读整行后写整行的路径。只靠前端深合并仍可能在并发时丢掉另一个命名空间或刚保存的 Codex 配置。数据仓库应新增按 Key ID 原子替换 `fingerprint.codex` 的方法，并将 OAuth 凭证刷新统一改成现有的字段级凭证更新或等价事务，保证它永远不写 fingerprint。
- **现有通用 `regenerate_fingerprint` 必须先修正。** 它目前会把任意 Provider 的完整 fingerprint 替换为 Claude Code 结构。实现 Codex 功能前必须限制 Provider 并只改对应命名空间；Codex 账号明确拒绝该动作。否则单次号池操作就可能删除 Codex 配置。
- **脱敏是上线阻断条件。** Codex 受控头必须在 orchestration trace、runtime miss 和模型测试摘要中始终显示为 `<redacted>`；请求体只递归遮盖 `client_metadata` 内的身份字段和 turn metadata，不删除其他诊断内容。硬编码的必脱敏名单优先于管理员自定义敏感头配置，防止旧配置漏掉新字段。
- **管理界面同时提供单账号和批量设置。** OAuth 编辑框仅对 Codex OAuth 展示四档模式，历史未配置账号明确显示“保持旧行为”；存在 installation override 时只显示“已配置固定安装标识”并允许确认后清除，不默认展示原值。号池批量动作只允许对 Codex OAuth Key 合并 `codex.mode`，返回成功、跳过和失败数量。
- **上线顺序固定为：** 先更新架构与导入文档；再完成脱敏和通用指纹动作限域；实现纯核心、最终接入、配置与导入、管理界面；部署时全局开关关闭；在测试环境用捕获上游核对所有载体；生产先对少量账号启用 device，再启用 session；确认错误率、OAuth 风险错误和首字延迟后分批扩大。full 仅用于明确实验，不作为默认推荐。
- **回退分三层：** 单账号改 `off`；全局开关关闭使新请求在最多 3 秒内恢复旧行为；代码回退时旧版本忽略新增 JSON 字段，无需数据库回滚。已发送给上游的历史标识不能撤销；正在执行的计划保持构造时的值。
- **验收必须覆盖：** 四档模式、旧账号未配置、新账号显式 session、源导入缺失模式、确定性与隔离、窗口 0 与压缩后非零窗口、同一请求头体一致、Header/Body Rules 最终覆盖顺序、四条生产路径、compact 头部收敛且正文结构不变、image/chatgpt_web 排除、401/403 同计划重放、换账号新 ID、重复导入策略、并发配置/Token 刷新不丢字段、损坏元数据、大小写重复头、诊断脱敏、无每候选数据库查询和管理界面状态。

### Review Reopen Findings（按用户目标重新裁决）

- **sub2api v0.1.175 确实有 `conversation_id`，但不是这次指纹功能新增的字段。** 它原本就是 sub2api 的兼容请求头：先接收客户端值，再按平台 API Key 隔离后转发。v0.1.175 新增的指纹收敛随后覆盖 installation/session/thread/turn/window，并没有覆盖或删除 `conversation_id`，所以同一 OAuth 账号仍可能因不同平台 API Key 出现多个 `conversation_id`。
- **官方 Codex 代码中的 `conversation_id` 主要是应用内部对任务/线程 ID 的旧命名，不等于上游 HTTP 的 `conversation_id` 头。** 当前官方上游请求明确发送的是 `session-id`、`thread-id`、`x-client-request-id` 以及 `x-codex-*` 元数据；没有把 `conversation_id` 作为 Codex Responses 必需请求头。因此 Niffler 删除该兼容头不会缺少官方 Codex 必需字段。
- **Niffler 当前会为 Codex Responses 自动补 `conversation_id`。** 它来自平台用户 API Key 对应的 `prompt_cache_key`，所以共享同一个 OAuth 账号的不同 Niffler 用户仍会带不同 `conversation_id`。这与用户要求的身份收敛目标冲突。
- **最终裁决改为：启用任一非 off 收敛模式时，不向 Codex Responses 上游发送 `conversation_id`。** 生成 thread 前仍可把客户端原始 conversation/session 信号作为内部线程区分输入；完成账号级映射后，删除出站头和不属于官方协议的同名兼容字段。`off` 保留当前行为，确保紧急回退时没有额外兼容变化。
- **旧账号“缺少配置”只表示创建时还没有这项新设置，不表示 OAuth Token、账号或登录失效。** 为满足用户目标，最终规则改为与 sub2api 一致：全局功能开启后，Codex OAuth 账号缺少、空白或未知模式均按 `session`；只有明确保存 `off` 的账号才关闭。由此所有现有账号自动生效，不需要逐个编辑。
- **不能只处理 `conversation_id`，还要重审 Niffler 自动生成的 `prompt_cache_key`。** 当前值同样按平台用户 API Key 生成；官方 Codex 会发送该字段，且其源码显示它有明确的线程/任务缓存用途。最终方案应继续保留缓存隔离，但不能继续使用能稳定区分 Niffler 用户的原值，而应按已经收敛的线程生成，使上游看到的是“同一账号的一组任务缓存”，不是“多个 Niffler 用户的固定标签”。
- **父线程和 fork 字段是官方任务拓扑，不应简单删除或全部固定。** 若客户端携带这些原始线程标识，应使用与普通 thread 相同的账号级确定性映射，保留父子关系同时去掉客户端原始值；否则会出现当前 thread 已收敛、parent/fork 仍暴露原始 ID 的不一致。
- **官方 Codex 的默认 `prompt_cache_key` 就是当前 thread ID。** 这给出了明确的兼容做法：Niffler 在 session/full 模式下应把 `prompt_cache_key` 覆盖为收敛后的 thread ID，而不是继续保留平台用户 API Key 派生值；既符合官方客户端行为，又消除一个稳定的站内用户标签。
- **当前 Niffler 对 `user-agent` 和 `originator` 只是“缺失时补默认值”，并非强制统一。** 不同客户端自带值仍会到达上游。要实现用户提出的实际目标，非 off 模式必须统一 Codex 客户端身份头，并强制用 OAuth 凭证中的 `chatgpt-account-id`，不能信任客户端自报账号。
- **官方现版本还可能发送 `x-oai-attestation`（设备证明），而 sub2api v0.1.175 的普通 Responses 指纹功能没有为其生成或收敛证明。** Niffler 不能伪造密码学设备证明；是否透传、删除以及上游是否要求它必须通过捕获上游测试确认。方案必须把它列为明确剩余边界，不能承诺仅靠 UUID 改写就一定呈现为单设备。

### Review Reopen Final Scheme

- **稳定状态：** 生产总开关开启；全部 Codex OAuth 账号在未明确选择其他模式时按 `session`；`off` 只作为单账号例外和紧急回退。旧账号、新账号和从其他系统导入的账号使用同一规则。
- **默认 session 的上游形态：** 同一个 OAuth 账号只有一个 installation 和一个 session；每个真实客户端任务映射成该账号下的独立 thread；每次上游请求有新的 turn；窗口编号随压缩过程保留。这样表现为“一个人、一套安装与会话、同时处理多个任务”，不会把所有并发任务硬塞进一个线程。
- **账号稳定来源：** 优先使用 OAuth 凭证中稳定的账号用户标识生成账号级 installation/session；有 `openai_device_id` 时 installation 直接沿用；稳定 OAuth 标识缺失时才回退 Provider Key ID。所有输入只参加单向派生，不直接发往上游或写日志。
- **线程来源：** 在改写前读取客户端已有 thread/session/conversation、Niffler 已解析的会话亲和信号和缓存键，按固定优先级选出任务标识，再与 OAuth 账号稳定来源一起确定性映射。没有任何任务信号时，最后使用平台 API Key ID作为内部输入；上游只能看到映射后的账号线程 ID，看不到原始 Niffler 用户或 API Key。
- **session/full 必须统一的字段：** `x-codex-installation-id`、`session-id`、`thread-id`、普通 Responses 的 `x-client-request-id`、`x-codex-window-id`、`turn_id`，以及请求体 `client_metadata` 和已有合法 turn metadata 中的对应值。头和体必须复用同一组结果。出站 HTTP 使用官方连字符头 `session-id`；删除 Niffler 旧兼容头 `session_id`，请求体 `client_metadata.session_id` 继续保留。
- **额外消除的用户标记：** session/full 删除出站 `conversation_id`；将 `prompt_cache_key` 覆盖为收敛后的 thread ID；父线程和 fork ID 使用同一账号级线程映射；`chatgpt-account-id` 强制使用当前 OAuth 凭证的真实值；统一 `user-agent/originator/version` 为同源、受控的 Codex 客户端身份。
- **保留的正常任务差异：** 请求正文、工具调用、`previous_response_id`、服务器返回的 turn state、工作区和子代理业务元数据继续按任务变化。它们表达一个人的不同工作，不应全部固定，否则会破坏功能或制造明显矛盾。
- **模式边界：** `off` 完全保留旧行为；`device` 只做安装标识和客户端身份头统一，是较弱模式，不能满足用户主目标；`session` 是默认和推荐模式；`full` 再把所有 thread 合成一个，只用于实验，因为并发任务可能互相污染缓存和线程关系。
- **旧账号规则：** “缺少配置”仅指旧记录没有后来新增的 `fingerprint.codex.mode`。它与 OAuth Token 是否有效无关。总开关开启后，缺少、空白或未知模式均按 session；界面显示“会话收敛（默认）”。有效旧账号无需编辑即可生效，原本已失效的 OAuth 凭证仍需按原流程重新授权。
- **sub2api 导入规则：** 有效的 `off/device/session/full` 原样迁移；源模式缺失、空白或未知时按 sub2api 自身语义写为 session；合法 `openai_device_id` 迁移为固定 installation；源中没有 device ID 时按 Niffler 的账号级规则生成。重复账号是否替换仍遵守现有导入规则，不能静默覆盖活动账号。
- **请求处理顺序：** 先读取原始任务标识；再选择实际 OAuth 账号并完成格式转换、Body/Header Rules、受管理提示词和认证头；最后一次性做指纹收敛并冻结请求。任何后续规则都不能再次覆盖受控字段。普通 Responses、compact、同格式、Chat/Claude/Gemini 转 Responses 和管理端模型测试均必须经过同一终态处理；image、chatgpt_web 和非 OAuth Key 不处理。
- **compact 差异：** 使用同一 installation/session/thread/window；保留合法窗口编号；按官方行为不强行添加普通 Responses 的 `x-client-request-id`；不向 compact 正文增加未声明字段。
- **失败规则：** 启用收敛时不得在解析失败后静默发送原始身份。损坏的客户端 turn metadata 删除该载体并使用重新生成的扁平字段；无法形成完整账号级身份时跳过该 Provider Key 或显式失败。关闭总开关或单账号 off 才允许恢复旧行为。
- **持久化与并发：** 配置继续放现有 Provider Key `fingerprint.codex`；模式和可选 installation 更新必须由后端只修改 Codex 子对象，OAuth Token 刷新只修改凭证字段，防止并发覆盖。请求执行阶段不写数据库、不新增数据库查询。
- **设备证明边界：** 捕获上游测试必须验证正常 Responses 与 compact 在去除客户端 `x-oai-attestation` 后是否仍成功。若可选，session/full 删除该客户端设备证明；若上游强制要求，不能伪造，相关路径不能承诺单设备形态，必须停止该范围上线并单独处理。
- **网络出口也是上线条件：** 同一个 OAuth 账号必须在所有 Frontdoor 使用同一个固定代理或固定出口；否则上游仍会看到同一 installation/session 从多个公网地址同时使用。该功能只解决请求层标识，不能靠改头掩盖多出口、异常并发或请求内容本身。
- **客户端版本必须同源：** `user-agent`、`originator`、`version` 和相关 Beta 头由一份受控 Codex 客户端配置生成并定期随版本更新，不能继续使用客户端各自值，也不能长期硬编码已经明显过时的版本。
- **上线顺序：** 先完成架构文档和脱敏，再实现纯映射与统一终态处理，接着完成账号设置和导入；测试环境捕获上游逐字段验收后，生产部署时总开关先关闭，验证完成后开启。开启总开关并确认旧账号按 session 生效才算功能上线完成。
- **验收重点：** 用多个 Niffler 用户同时调用同一个 OAuth 账号，上游只能看到一个 installation、一个 session、同源客户端身份和真实 OAuth account ID；线程只随任务变化；没有 `conversation_id`；`prompt_cache_key` 等于映射后的 thread；父子线程关系一致；原始用户/API Key/客户端身份不出现在头、正文元数据或日志。不同 OAuth 账号必须得到不同身份；多节点、重启和 Token 刷新保持稳定；off 必须完整恢复旧行为。

### Issues Encountered

| Issue | Resolution |
|-------|------------|
| 两次补丁分别因任务标题空格和 findings 模板标题假设错误而未应用 | 已读取真实首行并按现有标题追加，业务文件未受影响 |
| 初次全文检索范围过宽，结果被无关 Codex 文档截断 | 改为按 v0.1.175 合并提交的父提交差异定位精确文件 |
| 检索重复导入实现时使用了不存在的旧目录和未匹配通配符 | 全仓按符号重新定位真实文件，确认活动重复账号默认拒绝、可替换账号才更新 |
| 复审记录补丁假定 progress 中存在一条精确句子，导致三文件补丁整体未应用 | 先检索真实锚点后重新应用，未修改业务文件 |

### Review Reopen 4：已交付方案复审结论

- **总体判断：目标和默认 `session` 模式正确，但方案不能直接进入实现。** 关键字段有一处明确错误，第一版范围有过度扩张，稳定 ID 来源、窗口映射、配置保存和失败行为还需要写成可执行规则。
- **明确错误：`x-client-request-id` 不是每次请求新生成。** sub2api v0.1.175 在 session/full 模式下把它写成收敛后的 `thread_id`；官方 Codex Responses 也将当前 thread ID 写入该头。同一任务应保持稳定。每次上游请求新生成的是 `turn_id`（UUIDv7）。
- **`conversation_id` 的处理方向保留。** sub2api v0.1.175 确实会在既有兼容逻辑中继续生成/隔离该头，但新指纹功能不依赖它；官方 Codex Responses HTTP 契约使用 `session-id`、`thread-id`、`x-client-request-id`。Niffler 非 off 模式可读取客户端原值作为任务输入，随后删除出站 `conversation_id`，避免继续暴露按 Niffler 用户 API Key 派生的稳定值。
- **第一版必须准确对标覆盖范围。** sub2api v0.1.175 的指纹 ID 在普通 Responses HTTP 路径计算，并明确跳过 compact；因此“对标同等级功能”的第一版应覆盖所有最终落到普通 Codex Responses HTTP 的同格式和跨格式入口，不应把 compact、WebSocket 和图片接口混在首版承诺中。后续扩 compact 要按官方独立契约另做设计和上游兼容验证。
- **账号稳定来源要拆成两个层次。** imported `openai_device_id` 优先成为 installation。没有该值时，应优先使用持久的 Provider Key UUID（直接对标 sub2api 的 account.ID 语义）确定性派生，保证老账号无需写库即可生效、跨节点一致、重启不变。OAuth `account_id/account_user_id/user_id` 只用于判重或可选跨记录归并，不应直接替代 Provider Key 主键，否则字段缺失或账号空间变化会造成已有设备身份变化。
- **不应要求“多实例共享服务器密钥”才能稳定。** sub2api 使用普通 SHA-256 从本地账号主键派生，没有这项运维前提。Niffler 如果使用 HMAC，则必须证明现有部署已经有不会轮换的专用密钥；否则首版应使用带版本命名空间的确定性 UUID 派生，原始主键不直接出站即可。
- **窗口规则需要精确定义。** 官方当前 `x-codex-window-id` 是 `thread-id:窗口序号`，窗口序号随上下文压缩从 0 增长。首版应只接受合法的非负整数后缀，然后用收敛后的 thread 重建；缺失或非法时使用 0。不能透传原始前缀，也不能声称维护服务端窗口状态。
- **`prompt_cache_key` 必须跟随收敛 thread。** Niffler 当前默认值按平台用户 API Key 生成；官方 Codex 默认使用当前 thread ID。session/full 模式应强制覆盖为收敛 thread，device/off 保持原行为。
- **配置位置可以复用 `fingerprint` JSON，但现有再生操作是阻断项。** Provider Key 的 `fingerprint` 已被 Claude Code 传输指纹使用，管理端 `regenerate_fingerprint` 可能整体替换对象。实现前必须规定 `fingerprint.codex` 与其他命名空间深合并，并保证再生其他指纹不会删除 Codex 配置。
- **模式默认规则保持：** 仅 Codex OAuth 账号参与；缺失、空白、未知值均解析为 session；明确 off 才关闭。旧账号 OAuth 凭证不会失效，也无需重新授权。若增加发布级总开关，它应只控制本功能的紧急停用，不能复用 sub2api 的 `enable_fingerprint_unification` 名称，因为后者注释和调用语义是另一套通用 `X-Stainless-*` 指纹功能。
- **失败行为首先保证不会透传原始身份。** 未知模式按 session；非法 installation override 忽略并按 Provider Key ID 派生；损坏的 turn metadata 删除后用同一组扁平身份继续；缺少任务信号使用稳定回退。只有 Provider Key ID 等必需账号信息本身缺失、确实无法生成完整身份时才返回明确的本地配置错误，不能静默发送原始身份，也不能以此为由换另一个 OAuth 账号。
- **客户端身份头不是这个功能的核心验收字段。** sub2api 在同一路径还有独立的 Codex 身份头统一逻辑。Niffler 可在相同终态处理器中强制使用受控且相互匹配的 User-Agent/originator/version，但必须作为独立兼容规则写清版本来源；不应把“定期更新”当作未定义的实现步骤，也不能承诺它代表官方原版客户端。
- **设备证明必须明确排除伪造。** 客户端 `x-oai-attestation` 不能与收敛后的 installation 混用。首版应默认删除客户端提供的该头，并以普通 Responses 的实际上游兼容测试作为发布门禁；若上游强制要求真实证明，停止该路径上线，不能自行生成假证明。
- **网络出口不是请求改写功能本身。** 固定出口可作为生产部署建议和风险说明，但不能作为首版代码验收条件，否则会把应用功能和网络拓扑绑定在一起。功能验收只证明请求层标识收敛，不承诺上游只能观察这些字段。
- **任务来源必须有唯一、固定的优先级，但不应照搬 sub2api 只读取 session。** 官方 Codex 的 `thread-id` 才是当前任务，`session-id` 可能被同一根任务下的多个子任务共用；只按 session 派生会把这些子任务合成一个 thread。Niffler 应在改写前依次读取官方 `thread-id`、正文 `client_metadata.thread_id`、官方 `session-id/session_id`、现有会话亲和信号（其中可包括 `conversation_id/prompt_cache_key`），最后才以平台 API Key ID 作为无任务信号时的稳定输入。所有原值只参加账号命名空间下的确定性派生，不直接出站。
- **`turn_id` 不能无条件重生。** 官方 Codex 的 turn ID 表示一次用户回合，同一回合可能因传输重试再次发送。正确规则是：进入 Niffler 的一次逻辑请求生成一个 UUIDv7，并在该逻辑请求的所有上游重试中复用；下一次用户请求再生成新值。头、正文和内嵌 turn metadata 必须共享同一个值。

### Product Decision Reopen 5：最终配置入口与生效范围

- 用户最终确认只保留系统级统一开关，入口为“系统设置 → Provider 高级设置 → Codex OAuth 身份收敛”；不按单个账号或单个 Provider 分别配置。
- 开关关闭时，所有 Codex OAuth 账号保持 Niffler 当前行为；开关开启时，所有现有及以后新增的 Codex OAuth 账号统一使用 `session`（会话收敛）。
- 账号编辑页不显示该配置，不增加账号批量设置，也不提供 `device/full/off` 等账号级覆盖；任何账号都不能绕过或单独修改全局决定。
- 每个 Codex OAuth 账号仍使用各自稳定的 installation/session 身份；“全局统一”只统一启停和模式，不会让不同 OAuth 账号共用同一个设备 ID。
- 旧账号无需补字段、重新授权或逐个保存。全局开关开启后立即按账号稳定 ID 派生并生效。
- sub2api 导入不再把源 `codex_fingerprint_mode` 作为 Niffler 的账号级生效配置；可继续迁移合法的 `openai_device_id`，仅用于保持该账号自己的安装身份稳定。
- 本节是配置产品形态的最终裁决，取代上文所有单账号编辑、批量设置、Provider 级设置及“账号显式 off”相关建议；请求字段和身份派生规则仍以 Review Reopen 4 为准。

### 实施结论

- 已按最终裁决实现为一个系统级布尔开关，默认关闭；没有账号级或 Provider 级覆盖项。开关开启后，所有有效 Codex OAuth 账号立即生效，OAuth 登录、令牌刷新和账号可用性不受影响。
- 身份收敛位于最终请求规则之后，覆盖原生 Responses、OpenAI Chat 转 Responses、其他标准格式转换以及同格式透传这四类真实出口；最终格式不是普通 `openai:responses` 时不生效。
- 每账号稳定安装和会话、每账号加任务信号稳定线程、每入站逻辑请求一个回合 ID 的规则已经由测试固定；账号间不会共用身份，任务间不会被强制合成一个线程。
- sub2api 导入仅白名单迁移合法 `openai_device_id`；源 `codex_fingerprint_mode` 不影响 Niffler 全局开关，任意 extra 字段不会进入 fingerprint。
- 身份相关请求头无论管理员是否保存过旧的敏感头配置，都会由运行时固定清单脱敏。出站正文仍遵循现有请求正文记录级别，不新增单独展示入口。
- 第一版仍明确排除 Compact、图片、ChatGPT Web 和 WebSocket；固定网络出口不属于本功能。该功能减少请求层稳定身份数量，不能消除网络地址、并发量、内容和使用习惯等其他观察信号。

### 发布结果（2026-08-13）

- 功能经合并请求 #37 进入 `test`，测试提交 `6ca4a7fbc4d1e1e980c287a3d33b08285345b8b9` 的镜像构建和测试环境部署成功。
- 功能经合并请求 #38 进入 `main`，准确生产提交为 `e1edf18d9481823422ee8608faf4b54f1402af07`。
- 生产镜像构建 31672661254 和生产部署 31673279709 均成功；前台、后台容器以及两轮公开健康检查均正常。
- 全局开关仍保持默认关闭；上线代码和启用生产行为是两个独立动作，本次没有变更运行中的系统配置。

---

## 2026-08-09 有限钱包持续透支严重计费缺陷审查

### 最终边界

- 不做金额预占，不增加文本、图片、视频或工具调用限制，不增加 Redis 用户许可或新的并发限制，不修改用户请求正文和模型能力。
- 允许余额大于 0 时已经进入上游的请求按真实费用完成结算；这些同时执行的请求可能共同形成最后一批欠费。余额等于 0 时不能再靠钱包开始请求。
- 钱包变为负数后不能再作为新请求的支付来源；当前请求能由有效套餐供应商提供且套餐窗口仍有余额时仍可使用套餐。无关、过期或已耗尽的套餐不能覆盖钱包拒绝。
- 严格保证“只透支一笔”必须串行请求或预占金额，会影响 Codex、多标签页和并行任务。本方案选择保留正常并发：钱包负数后不能再授权钱包请求，但仍可由有效套餐独立授权。
- 用户同一时刻只能有一个生效套餐。同套餐续费允许排在当前到期时间之后；不同套餐不能创建重叠权益，创建支付订单和支付发放都必须在事务内复核。

### 实施复审补充

- 请求级准入不能只保存合并后的权益 ID 和供应商 ID。准入现在保存每个权益对应的供应商范围；重试可以在当前套餐供应商范围内切换，结算只核销这次实际供应商可以使用的权益。
- 单套餐约束不进入模型请求链路，只作用于套餐下单与支付发放，不影响模型请求响应时间。
- 正常模型请求在调度阶段并行读取钱包和套餐窗口，准入与已有同步请求记录同事务保存；已确认供应商套餐不会再追加第二次顺序资金查询。旧静态套餐兼容路径仍会做一次模型级复核，迁移确认供应商后消失。

### 已确认事实与根因

- 截图用户的充值余额确实由结算逐笔写成负数，不是前端格式问题；该用户在对应模型套餐耗尽后，钱包仍被持续扣款。
- 2026-08-09 13:02（北京时间）的生产只读快照中有 44 个 `finite + active` 负钱包，合计欠费 2454.090266 USD；其中 41 个存在“已结算请求继续扣款且结算后余额为负”的直接证据，合计欠费 2454.031614 USD，另外 3 个迁移钱包没有同类结算证据，单独列为待核对。生产余额仍会变化，实际补偿前必须重新冻结快照。
- 初始鉴权使用所有套餐的聚合余额。任意套餐还有余额时，即使该套餐不支持本次模型，也会覆盖钱包余额不足的拒绝结果。
- 请求正文阶段先做费用估算；无法估算时直接放行，后面的模型级套餐校验没有执行。现有测试还明确要求这种请求通过。
- 结算把“钱包记录存在”误当成“允许透支”，并将所有未覆盖费用继续写入充值余额，因此余额负数会一直扩大。
- 管理端和用户端派生字段使用 `max(0)` 隐藏负数，用户列表因此显示 0；启用状态又被显示为“正常”，延误了事故发现。
- 用户列表后端已经批量读取当前页钱包，前端却再次拉取全部钱包，钱包列表又逐个查询套餐，存在重复查询和逐用户查询放大。
- 这次事故不是预占造成的，缺少预占也不是直接根因。直接根因是准入错误、结算无条件透支、套餐范围混用和界面隐藏负数。

### 套餐范围新增审查

- 当前正式架构将套餐范围保存为固定的 `allowed_global_model_ids`；文档还明确规定供应商新增模型不会自动进入旧套餐，管理员必须重新编辑套餐。
- 当前套餐页面虽然提供“按供应商快速选择”，实际操作只是把该供应商当时关联的模型 ID 批量复制进套餐；套餐最终没有保存供应商关系，也没有后续同步机制。
- 因此供应商后来增加 `gpt-5.6-sol` 时，旧套餐不会自动包含它。此次用户看到套餐仍有余额，但该套餐对 `gpt-5.6-sol` 不生效，确实属于事故触发条件之一。
- 最终选择供应商 ID 作为套餐范围的唯一配置，不再保留需要人工同步的模型白名单；供应商与全局模型的有效关系按照管理配置状态动态解释。
- 前端实现已经直接证明“按供应商快速选择”只是一次性操作：它读取供应商当前的 `global_model_ids`，然后把这些 ID 加入或移出 `form.allowed_global_model_ids`；保存载荷仍然只有模型 ID。
- 后端套餐校验强制要求 `allowed_global_model_ids` 非空，没有供应商范围字段。数据层已经具备按供应商批量读取当前启用全局模型的查询，可以作为动态解析的基础能力。
- 用户购买或管理员发放套餐后，系统会把套餐权益复制到 `user_plan_entitlements.entitlements_snapshot`；准入和结算都读取这份快照中的固定模型 ID。因此即使管理员后来手工修改套餐模型，已购套餐也不会自然同步，问题不只发生在套餐模板层。
- 现有“供应商当前启用模型”查询只检查模型映射自身启用且存在全局模型 ID，没有在该查询中同时要求供应商启用、模型可用和全局模型启用；若用于套餐动态范围，必须统一使用完整的可路由关系，不能直接照搬当前简化查询。
- 使用记录与结算输入已经包含 `provider_id` 和 `global_model_id`，因此可以同时验证“请求模型是否来自套餐选中的供应商”和“最终实际供应商是否属于套餐范围”，无需根据供应商名称猜测。
- 动态供应商套餐不能只在准入阶段把供应商临时展开为模型集合；候选路由和最终结算也必须使用同一供应商范围，否则可能用供应商 A 的模型关系取得套餐资格，最后却由未包含在套餐中的供应商 B 提供服务。
- 最终数据语义应改为：套餐和已购权益保存 `allowed_provider_ids`，可用模型在请求时由这些供应商当前有效的供应商模型映射动态派生；不再保存一份需要人工同步的模型 ID 清单。
- “当前有效映射”应取管理配置状态：供应商启用、供应商模型启用且可用、全局模型启用。临时账号冷却、并发占满或单个端点故障只影响本次是否有可用服务，不应永久改变套餐宣称的模型范围。
- 选中供应商形成模型资格后，套餐请求的实际路由也只能使用该套餐选中的供应商；多个供应商取并集，实际候选从能够提供本次全局模型的选中供应商中选择。
- 请求开始时必须保存本次匹配到的套餐、供应商范围、实际全局模型和最终供应商；结算使用这份请求级决定，避免供应商配置在长请求中途变化导致准入与扣费不一致。
- 套餐所选供应商 ID 可以继续保存在已购权益快照中：供应商后续新增或移除模型会动态影响旧权益，套餐模板后来改选其他供应商则不应静默改变已经售出的合同。若产品明确要求模板修改也追随，应另设显式“同步现有用户”操作并留下审计记录。
- 供应商新增模型会自动进入所有选择该供应商的套餐，这是该设计的核心行为，也意味着新增高成本模型会扩大旧套餐权益。后台保存供应商模型变更前必须展示受影响套餐；需要不同档位时应拆分供应商服务池，而不是恢复静态模型复制。
- 现有静态模型清单无法可靠反推出最初选择的供应商，同一个全局模型可能来自多个供应商。迁移时只能生成建议并由管理员确认，不能无审计地自动猜测。

### 统一业务规则

- `actual_wallet_balance` 表示“充值余额 + 赠款余额”的真实有符号总额；`spendable_wallet_balance = max(actual_wallet_balance, 0)`；`debt_usd = max(-actual_wallet_balance, 0)`。
- `billing_state` 单独表示 `active`、`in_debt` 或 `unlimited`，不能再用钱包的启用状态代替计费状态；`in_debt` 不等于整个账户停用，仍可能有可用套餐。
- 钱包小于 0：钱包支付不可用；当前请求有适用且未耗尽的套餐时允许使用套餐，否则拒绝。部分充值后仍小于 0，钱包支付仍不可用。
- 钱包小于 0：禁止购买、续购或自动续费套餐，避免用户通过新增套餐绕过欠费清偿；已经生效的套餐不受影响。充值使真实钱包余额恢复到 0 或正数后，才恢复套餐购买资格。
- 钱包等于 0：钱包支付不可用；只有适用且未耗尽的套餐可以授权请求。
- 钱包大于 0：正常请求；套餐先扣，套餐不足部分继续扣钱包。
- 旧数据中的 `allow_wallet_overage` 不再控制业务行为。新保存的套餐固定写为 `true`；套餐不足部分必须扣钱包，不能免费，也不能因旧配置为 `false` 而拒绝余额大于 0 的请求。
- 费用预估只保留为提示和监控，不能再决定放行，也不能因为估算失败绕过余额与套餐校验。

### 请求准入

1. 完成身份、账户启用状态和 API Key 校验。
2. 读取真实钱包余额；有限钱包小于 0 时标记钱包支付不可用，但不能在解析套餐前直接拒绝。
3. 解析本次实际使用的全局模型，再查询仍有效、窗口有余额且所选供应商当前支持该模型的套餐。
4. 套餐命中时得到本次允许使用的供应商集合，候选路由只从该集合选择；套餐未命中时再按钱包和普通供应商权限判断。钱包为负且没有命中套餐时返回 402。
5. 在调用上游前，将 `billing_admitted=true`、准入时间、实际模型、命中的套餐权益、钱包资格和允许供应商集合写入不可变的请求级计费准入记录；与现有同步候选写入使用同一次事务和网络往返。该记录不是金额锁定。

早期阶段只能提前拒绝，最终允许必须集中在同一个计费准入函数中产生。协议转换、Codex 特殊路径和不同 API 协议都必须调用同一函数，不能各自复制判断。

### 结算规则

- 结算继续使用真实用量和真实价格，在现有 PostgreSQL 事务内锁定钱包与命中的套餐窗口，保持请求级幂等，并校验实际供应商属于准入时保存的套餐供应商集合。
- 只有持久化为 `billing_admitted=true` 的请求可以扣除用户套餐或钱包。这样，欠费前已经开始的并发请求仍可正常完成和结算；欠费后误创建、重放或绕过准入的请求不能继续扩大负余额。
- 删除 `wallet_can_overdraft = wallet exists` 这一无条件透支语义。有限钱包只有“本请求已由当时可用的钱包或套餐合法准入”时才允许按真实费用结算到负数。
- 如果发现没有有效准入记录却产生上游成本，保存平台成本和 `billing_violation` 异常，不静默扣负钱包，并立即告警。
- 充值先冲抵负余额；余额恢复到大于 0 后重新允许钱包付费请求。已通过准入但延迟完成的旧请求仍按真实费用结算，并在流水中明确展示。
- 负钱包下由套餐授权的请求先扣套餐，钱包不能单独成为准入依据；该请求使套餐耗尽时，它和此前已开始的同批请求仍按真实费用结算，之后没有其他适用套餐的新请求才被拒绝。
- 套餐最后一批请求超过剩余额度时，套餐只扣到 0，超出部分继续扣钱包；即使钱包在请求开始时已经为负，也允许这批由套餐合法授权的请求继续扩大钱包欠款。套餐额度本身不能变成负数。
- 套餐允许钱包补差时，准入记录需要证明本请求是在套餐仍有余额时合法开始；结算按真实费用执行“套餐扣到 0 → 剩余金额扣钱包”。这批请求完成后，钱包仍负且没有其他适用套餐的新请求才被拒绝。

### 接口与用户体验

- 管理端用户列表、钱包详情、用户钱包中心和首页都返回并展示同一个真实余额，不再使用 `max(0)` 隐藏负数。
- 用户列表的钱包列直接显示 `-$626.71`，使用红色“欠费”状态，支持欠费筛选和金额排序；套餐余额单独展示，不能与钱包欠款相抵后显示“总可用”。
- “账户启用”和“计费状态”分开显示，杜绝“状态正常、钱包欠费”的同屏矛盾。
- 402 文案直接说明：“钱包欠费 $626.71，请充值后继续使用。”充值页面显示恢复使用至少还需充值多少；部分充值后仍欠费时明确说明仍不可使用。
- 钱包欠费但仍有可用套餐时，页面显示“钱包欠费，套餐仍可使用”；套餐最后一批超出额度的部分会继续计入钱包欠款。只有当前请求没有可用套餐时才提示充值后继续。
- 钱包欠费时，套餐购买和续费入口显示不可用，并明确提示“请先充值结清钱包欠费”；不能只在前端隐藏按钮，服务端购买接口和自动续费任务必须执行同一校验。
- 请求记录展示套餐扣款、钱包扣款、结算后真实余额、准入状态和计费异常，便于客服和财务核对。
- 套餐管理改为直接选择供应商，界面实时预览这些供应商当前提供的模型；模型列表只读，不再让管理员维护第二份静态清单。供应商新增或移除模型时展示会自动变化。
- 供应商模型配置页面在保存前展示“将影响哪些套餐和多少有效用户”，让自动扩展权益成为可见的业务操作。

### 性能设计

- 请求链路不增加金额预占、Redis 往返、租约续期或资源限制计算；模型请求正文和上游能力完全不变。
- 将现在的“聚合套餐检查 + 费用估算后模型检查”合并为一次最终查询：同时判断钱包、套餐窗口以及套餐供应商是否支持本次全局模型。供应商模型关系按目录版本缓存并在配置变化时失效，数据库操作次数可以持平或减少。
- 准入证据与上游前已有的同步候选记录同事务保存，不新增跨机房数据库往返；结算校验放在已有事务中。查询复杂度变化仍需通过首个响应时间实测确认，不能预先承诺绝对为零。
- 管理端用户列表直接返回当前页批量钱包与套餐摘要，删除前端全量钱包拉取和后端逐钱包查询，列表性能会优于现状。
- 验收比较修改前后的准入耗时、首个响应时间、结算锁等待和用户列表查询数；首个响应时间不得出现有统计意义的回退。

### 历史数据处理

- 不重放历史套餐窗口，不追缴本次缺陷造成的额外使用，也不修改原始请求、上游成本和结算记录。
- 只读列出受本次缺陷影响且当前钱包为负的用户，包含用户 ID、用户名、邮箱、钱包 ID和真实负余额，供管理员核对。
- 处理方式统一为通过审计调整将负钱包补到 0，调整原因固定为本次计费缺陷补偿；被用户使用的额外金额视为平台赠送。
- 本轮评审不直接修改生产钱包。实际批量补齐前必须冻结名单和金额快照，并由用户确认最终执行范围。

### 分阶段实施

- 第一阶段：先更新需求和架构文档，明确负钱包仍可使用套餐、供应商动态模型、最后一批超用、钱包补差和充值恢复规则；同时先修复负余额展示和告警。
- 第二阶段：新增套餐供应商、用户权益供应商和请求级计费准入结构；管理员确认旧套餐供应商映射并完成数据校验，此时不切换线上行为。
- 第三阶段：部署兼容新旧数据的准入、候选路由和结算代码，但保持新规则关闭；影子计算新旧决定差异，验证 `gpt-5.6-sol`、多个套餐和双 Frontdoor 场景。
- 第四阶段：两台 Frontdoor 都运行新代码后，通过共享开关同时启用“PostgreSQL 最终准入 + 同步准入记录 + 套餐供应商路由 + 新结算”。不能先启用其中一半，避免准入和结算使用不同规则。
- 第五阶段：更新用户模型目录、钱包中心、套餐管理和用户列表，继续观察首个响应时间、结算锁等待、最后一批超用和计费异常。
- 第六阶段：修复上线并确认不再产生新的违规扣款后，重新冻结受影响负钱包名单和金额，通过审计调整补到 0；不重放账单，不修改历史结算证据。

### 必须通过的验证

- 负钱包加无关、过期或已耗尽套餐返回 402；负钱包的请求模型由套餐供应商支持且窗口有额度时允许，并优先扣套餐。
- 给套餐供应商新增 `gpt-5.6-sol` 后，新请求无需修改套餐或已购权益即可使用；移除映射后新请求停止使用，已开始请求仍按准入快照完成结算。
- 同一全局模型由多个供应商提供时，套餐请求只能路由到套餐所选供应商；结算发现实际供应商越界必须记为计费异常。
- 正常文本、Codex、Claude、Gemini、图片、视频和工具请求的正文与能力不因计费修复发生变化。
- 多个请求在余额大于 0 时已经开始，可以正常完成并结算；余额变成 0 或负数后钱包不能授权新请求，但适用套餐仍能授权。实际费用未超过套餐剩余额度时钱包扣款为 0；超过部分继续扣钱包。
- 当前余额为负且请求没有有效准入证据时，结算不能继续扣钱包；重复结算不能重复扣款。
- 钱包准入时已经为负但套餐仍有余额的请求可以完成；套餐扣到 0 后，剩余费用继续扣钱包。结算明细必须同时显示套餐扣款和钱包扣款，套餐余额不得为负。
- 部分充值后余额仍小于等于 0 时钱包支付继续拒绝、适用套餐继续可用；余额大于 0 后按钱包与套餐供应商规则恢复钱包支付。
- 钱包为负时，套餐购买、续购和自动续费均被服务端拒绝，已有套餐仍可使用；余额清偿到 0 后恢复购买资格。
- 数据库、管理接口、用户列表、钱包详情和用户中心显示的真实余额完全一致，负数不得截断为 0。
- 管理端用户列表查询次数固定于当前页规模，不再全量拉取钱包或逐用户查询套餐。
- 生产指标中“负钱包授权钱包请求”“无准入证据继续扣钱包”“套餐供应商越界路由”和“套餐耗尽后缓存放行”必须持续为 0。

### 最终方案一致性评审

- 评审结论为“方向通过、当前代码不得直接照旧逻辑修补”。实施必须同时解决套餐最后一批免费使用、3 秒陈旧缓存、异步准入失败后放行、供应商关系数据结构和实际供应商越界五项问题；历史数据不重算，只做名单核对和补偿。

- 当前方案总体方向正确，但“负钱包仍可使用套餐”引入了一个必须明确的结算边界：套餐请求完成后实际费用可能超过请求开始时的套餐剩余额度。
- 当前结算在 `allow_wallet_overage=false` 且实际费用超过套餐剩余额度时，直接把状态记为 `insufficient_quota`，套餐和钱包都不扣；请求已经成功却可能形成免费使用。`allow_wallet_overage=true` 时又因为钱包存在即允许透支而继续扣负钱包。两条现有行为都不能保留。
- 最终规则必须保存请求准入时的资金来源：套餐权益、套餐供应商、钱包当时是否可用、是否允许钱包补差。结算不能根据结束时的余额重新猜测资金来源。
- 对“钱包在准入时已经为负、仅靠套餐获准”的请求，仍按“套餐扣到 0 → 剩余金额扣钱包”结算。该请求可以继续扩大钱包欠款，因为它是在套餐耗尽前合法开始的最后一笔或最后一批请求。
- 套餐和钱包的金额都必须写入同一结算明细；套餐不能为负，钱包可以为负。结算后没有其他适用套餐时，负钱包不能再授权下一批请求。
- 当前模型级套餐可用性使用 Redis 缓存 3 秒，结算后没有对应的准确失效。套餐已经耗尽后的 3 秒内，新请求仍可能读取旧余额并被放行；这些请求不是耗尽前已经准入的最后一批，不能继续把该缓存用于最终资金授权。
- 钱包余额和套餐窗口的最终准入必须读取 PostgreSQL 已提交状态，并尽量合并为一次查询。供应商与模型目录属于低频配置，可以使用按版本失效的缓存；资金余额不能依赖定时过期缓存。
- 通用 `request_metadata` 会经过过滤、合并和大小限制，不适合直接承担不可变的资金授权凭证。计费准入应使用专用、服务端只写的结构化字段或请求级计费准入表，并与请求开始记录在同一次数据库提交中完成。
- 计费准入必须在调用上游前持久化成功；如果现有使用记录是异步或上游后才落库，就必须调整顺序。持久化失败时不能调用上游，否则结算无法区分合法最后一批和绕过准入的请求。
- 代码复核确认现有 `record_pending` 使用后台任务异步写入，并且写入失败只记录日志、不阻断上游。因此此前“随现有请求记录保存且不增加等待”的表述不成立；资金准入凭证不能复用这条异步链路。
- 实现时应优先复用进入上游前已经同步执行的请求候选或调度记录写入，将计费准入字段放入同一次提交；如果没有可复用的同步提交，就必须增加一次同步准入写入，并如实接受这部分数据库延迟，不能为了零延迟继续采用失败后放行。
- 代码确认所有同步和流式执行在调用上游前都会等待 `ensure_execution_request_candidate_slot` 写请求候选；这条同步往返可以与不可变计费准入记录放在同一事务中，避免新增网络往返。
- 现有请求候选写入失败或写入器不可用时仍只告警并继续执行。有限钱包或套餐请求接入计费准入后必须改为失败即停止上游调用；无限钱包和明确免费请求可按独立规则处理，不能共用资金路径的失败放行。
- 一个客户端请求可能尝试多个候选供应商。计费准入应以 `request_id` 为唯一键只创建一次，保存允许供应商集合和资金来源；每个候选记录保存实际供应商并验证属于该集合，重试不能重新选择另一种资金来源或绕过钱包状态。
- 套餐模板和用户权益目前都只用 JSON 保存范围，供应商删除流程也不知道哪些套餐正在引用它。供应商成为套餐的正式业务关系后，不应只塞进 JSON 数组。
- 推荐增加 `billing_plan_providers(plan_id, provider_id)` 和 `user_entitlement_providers(user_entitlement_id, provider_id)` 两张关系表；前者表示当前销售配置，后者表示购买时的供应商快照。供应商被有效套餐或权益引用时禁止物理删除，只允许停用或先完成迁移，避免权益静默失效。
- 供应商模型新增或移除仍通过现有供应商模型关系动态生效，不需要把模型复制进上述关系表。关系表只固定“用户买了哪些供应商范围”，不固定供应商以后提供哪些模型。
- 套餐按供应商授权后，资金判断和路由存在先后依赖，不能继续先把所有套餐金额汇总再选供应商。应先按“请求模型 + 每个供应商”计算可用套餐，再优先选择有套餐资金的候选供应商，最后保存实际供应商对应的套餐权益顺序。
- 钱包为负时，只能产生有套餐资金的候选供应商；钱包为正时，没有套餐资金的供应商才可以走钱包路径。已经存在可用套餐时，不应因为另一个钱包供应商排序更高而跳过套餐直接扣钱包。
- 套餐供应商临时全部不可用时返回 503，不应静默切到套餐外供应商并扣钱包。`allow_wallet_overage` 只处理套餐额度不足，不等于允许供应商故障时改变计费来源。
- 多个有效套餐同时支持实际供应商和模型时，按最早到期、最早购买、权益 ID 的固定顺序消耗，并把顺序写入准入记录。当前结算虽然先按到期时间查询，随后使用按权益 ID 排序的 `BTreeMap`，实际消耗顺序会被改变，必须修复。
- 用户侧“可用模型”接口当前只根据用户或分组的供应商和模型限制过滤，没有合并用户有效套餐的供应商范围。套餐改为供应商授权后，该接口也必须返回“钱包按量可用模型 ∪ 有效套餐可用模型”，否则套餐实际能用但模型列表看不到。
- 钱包摘要默认仍聚合所有套餐并与钱包相加；供应商范围动态化后，更不能用一个总可用金额表达所有模型。默认接口应分别返回钱包真实余额和套餐列表；只有传入具体全局模型时才能计算该模型当前可用的套餐额度。
- 当前本地拒绝只有通用 `BalanceDenied`，无法区分钱包欠费、钱包余额不足、套餐耗尽、套餐不支持当前供应商模型。需要结构化错误原因和稳定错误码，避免所有场景都显示同一句“请充值”。
- 套餐支持模型但所选供应商临时没有健康候选时，这是服务不可用，应返回可重试的 503；钱包或套餐没有支付能力才返回 402。不能把供应商故障伪装成欠费，也不能因为供应商故障切到未包含在套餐中的供应商后仍扣套餐。
- 不做金额预占、资源限制和新增并发控制后，只能保证钱包继续变负来自有限数量的已准入请求，不能保证透支美元金额存在硬上限。单个请求实际费用和并发中的最后一批成本仍是明确接受的剩余风险，可通过“最后一批钱包欠费金额”监控，不再设计独立的套餐超用金额。
- 性能目标需要修正为“不新增跨机房网络往返”，而不是笼统承诺零延迟：最终资金查询替换现有错误查询；不可变准入与已有同步候选写入同事务；供应商模型目录缓存。上线必须对比首个响应时间和数据库锁等待。

### 本地实施记录

- 最终业务规则复审通过；新增正式架构文档 `docs/architecture/billing-overdraft-root-fix.md`。购买限制必须同时校验创建支付订单和支付回调发放，避免已打开收银台或延迟回调绕过欠费状态。
- 已新增 PostgreSQL、MySQL、SQLite 同版本迁移，包含 `billing_plan_providers`、`user_entitlement_providers` 和 `billing_request_admissions`。供应商关系使用真实外键；请求准入保存资金来源、钱包资格、钱包补差资格、权益顺序、允许供应商和实际供应商。
- 迁移契约测试先因文件不存在按预期失败；新增三份迁移后测试通过。后续仍需运行真实 PostgreSQL、MySQL 和 SQLite 迁移测试，不能只依赖文本断言。
- 套餐供应商关系不能只停留在接口字段：已完成套餐保存、修改和读取的关系表事务接入，旧套餐读取时供应商列表为空，后续按兼容规则继续使用静态模型范围。
- 负钱包购买限制已覆盖订单创建，管理员赠送使用独立支付方式并明确绕过该限制；还必须在支付回调真正发放权益前读取当时的真实钱包余额，避免延迟回调绕过。
- 套餐供应商快照和回调第二次余额校验现已完成：普通购买欠费后不会新增权益，管理员赠送不受影响。外部支付已成功但发放被拒时，订单需要进入可退款或人工处理状态；生产上线前仍需核对各支付网关对该状态的处理和用户提示。
- 请求级准入与上游请求记录已实现同事务写入，正常路径没有增加第二次跨机房往返；下一阶段必须让同步、流式执行在调用上游前使用该方法，并在有限计费写入失败时返回明确错误，不能继续沿用只告警的旧行为。

## Niffler 美西双入口与用户侧延迟显示（2026-08-09）

- 用户最终指定 `us1.niffler.org` 经 Cloudflare 指向 OVH `15.204.120.221`，`us2.niffler.org` 和根域名经 Cloudflare 指向 hd0526 `23.19.228.223`；只有 `api.niffler.org` 灰云直连 hd0526，并删除 `hub.niffler.org`、`cf.niffler.org`。
- 切换前 Cloudflare 只有 `api.niffler.org` 记录，内容为 `15.204.120.221` 且已开启代理；`us1`、`us2` 当时不存在。
- OVH Frontdoor 只监听 `127.0.0.1:18084`，hd0526 Frontdoor 只监听 `127.0.0.1:8084`，两台 Caddy 分别对外提供 HTTPS，应用端口没有直接暴露。
- OVH 当前只允许 Cloudflare 地址访问 80/443，并为 Niffler 域名固定使用 Cloudflare Origin CA 源站证书；灰云直连前必须开放公开 80/443，并为 `api`、`us1` 使用浏览器信任的自动证书。
- hd0526 当前 80/443 已公开，Caddy 使用自动证书；新增 `us2` 站点即可申请公开证书。
- 两台 Frontdoor 共用 rn01 上的 PostgreSQL 和 Redis，适合同时承载请求；Background 必须继续只在 hd0526 运行，避免重复后台任务。
- 跨域测速不应调用健康接口或数据库。两台 Caddy 提供 `204 No Content`、允许跨域且禁止缓存的专用响应，前端即可测量浏览器完成 HTTPS 往返的耗时。
- 用户没有授权 `us1`、`us2` 直连；两条记录应开启 Cloudflare 代理。这样测到的是浏览器经过 Cloudflare 到对应源站的完整请求耗时，可以观察两条业务路径的差异，但不是纯粹的用户到源站网络延迟。
- 从当前本机经过 Cloudflare 请求 `api.niffler.org/_gateway/health` 约 0.69 秒；直接请求 hd0526 约 1.10 秒。该数据只代表当前本机，不能代替所有美西用户结果。
- OVH 到 rn01 WireGuard 地址的 ICMP 往返为 24.5 至 26.6 毫秒；hd0526 到 rn01 公网地址为 0.6 至 1.0 毫秒。
- 同一公开模型列表接口从应用机本地访问时，OVH 为 108 至 168 毫秒；hd0526 首次为 82 毫秒、后续预热请求为 8 至 12 毫秒。迁移后动态页面和鉴权变慢属实，主要原因是 OVH 与数据库跨机房，而不是 OVH CPU 不足。
- OVH 本机健康接口仍只需约 0.6 至 1.0 毫秒，说明 Frontdoor 进程自身没有明显计算压力；涉及共享存储的请求才出现显著差距。
- OVH 线上 Caddy 还承载 `autocar.3jiezhiwai.com`，原仓库配置没有这段内容。生产更新不能直接覆盖旧文件；最终配置已保留该站点并纳入版本管理。
- Caddy 2.11 会优先复用已加载的 `*.niffler.org` Cloudflare Origin CA 证书，并跳过 `api`、`us1` 的公开证书申请。最终方案改为所有 OVH Niffler 域名统一使用 Caddy 自动管理的公开证书，Cloudflare 严格 TLS 仍可正常验证。
- 最终 DNS 已纠正并核对：`api` 为灰云并指向 `23.19.228.223`；根域名和 `us2` 为橙云、源站是 `23.19.228.223`；`us1` 为橙云、源站是 `15.204.120.221`；`hub`、`cf` 已删除，根域名邮件记录未修改。
- 1.1.1.1 与 8.8.8.8 均返回 `us1`、`us2` 的 Cloudflare 地址；实际测速响应包含 `server: cloudflare` 和 `cf-ray`，确认代理已经生效。
- `us1` 和 `us2` 的测速地址均返回 204，并带有允许跨域、禁止缓存和 Timing-Allow-Origin 响应头；`api` 直连 hd0526 的健康接口返回 200。
- OVH 已开放公开 80/443，以支持唯一灰云域名 `api` 和自动续证；应用端口仍只监听 `127.0.0.1:18084`。hd0526 应用端口也继续只监听 `127.0.0.1:8084`。
- OVH 只运行健康的 Frontdoor，hd0526 运行健康的 Frontdoor 和唯一 Background；两台 Caddy 最近日志没有新增错误。
- `api`、`us1` 和 `us2` 的健康接口、公开认证设置与公开模型列表均返回 200，说明两个入口都能完整代理应用请求，不只是测速地址可用。
- 前端类型检查、目标 ESLint、9 项相关测试和生产构建通过；仅有项目原有的 Browserslist 数据过旧和大分块提示。首页测速代码尚未提交或发布到生产。
- OVH Caddyfile 是单文件只读绑定挂载；使用 `install` 替换主机文件会让容器继续读取旧文件节点。生产更新必须原位置写入，或只重建 Caddy 服务并在重建后核对主机与容器内校验值。
- 最终生产状态：OVH 只提供 `us1`、内部源站地址和原有汽车站点；hd0526 提供 `api`、根域名、`us2` 及原有独立站点。OVH Frontdoor/Caddy、hd0526 Frontdoor/Background/Caddy 均健康。

## Niffler 受管理提示词：用户分组语义修正（2026-08-04）

## Niffler 受管理提示词：用户分组语义修正（2026-08-04）

- 用户明确要求的“分组”是用户分组，不是调度分组；当前已上线实现将配置放在 `routing_groups.config_json.managed_instructions`，配置对象选错了。
- 正确的用户操作是复用现有 API Key 创建和编辑页面中的用户分组选择。用户将 API Key 切换到 CTF/渗透分组或成人分组后，后续请求自动使用该用户分组的配置。
- 当前请求路由支持用户、API Key、用户分组与显式请求头绑定，但这套调度分组选择不能作为提示词配置来源；否则客户端请求头可能改变提示词配置，不符合可信服务端状态要求。
- 新实现应从鉴权后的 API Key 记录读取服务端 `group_id`，再读取对应用户分组的受管理提示词配置。客户端不需要也不允许通过 `x-aether-scheduler-group` 选择提示词配置。
- “我的 API Keys”里的分组已经承担用户可见的分组切换入口，不应再增加一套同义选择器；管理端只需在用户分组编辑处增加受管理提示词配置。
- 当前调度策略页没有用户/API Key 绑定管理界面，进一步说明将用户需求实现为调度分组配置会造成管理员只能调用接口、普通用户无法按现有产品流程切换。
- `api_keys.group_id` 已通过外键指向 `user_groups.id`，用户创建和编辑 API Key 时都会提交该字段；这条现有链路可以直接作为可信配置选择依据。
- 用户分组当前没有通用 `config_json` 字段，已有列主要是模型范围、限速、并发、可见性和销售倍率；实现需要明确新增存储字段，并同步 PostgreSQL、MySQL、SQLite、引导结构、导入导出和仓库映射。
- 现有认证查询已经联接 `api_keys` 与 `user_groups` 并读取组名、可见性和价格字段；增加受管理配置时应在这次鉴权查询中一并读取，避免每次最终请求再查数据库。
- 管理端用户组的创建、编辑已经共用 `UpsertUserGroupRecord`，前端也共用 `UserGroupsDialog`；新增字段可以沿现有保存链路接入，无需新增独立配置页。
- 用户组配置适合使用独立的可空 JSON 字段 `managed_instructions`：字段为空表示未配置，对象内继续保留 `enabled`、`profile_id`、`merge_mode`，避免引入含义不明的通用配置容器。
- 三种数据库、逻辑结构和引导结构都需要同一字段；SQLite 鉴权仓库测试已经证明 API Key 的 `group_id` 能联表读回用户组配置。
- 请求级快照现在只核对用户分组 ID 和实际配置值；调度分组 ID、版本和 `x-aether-scheduler-group` 不再参与受管理提示词选择。
- 用户 API Key 写入测试已证明现有编辑接口能从默认分组切换到成人分组；用户不需要新的配置入口。
- 独立请求测试已证明安全分组只包含安全规则，成人分组只包含成人创作规则；同一请求内用户分组或配置发生变化会明确失败。
- 用户分组配置区属于普通后台表单，没有高频交互、每帧 DOM 测量、重排、模糊层或高成本动效风险；最终检查得到 `PASS ui-review gate`。

## Niffler 受管理提示词配置：最终审查修复（2026-08-04）

- 统一最终处理函数重复作用于同一个决策时，会再次执行 Provider Request 路由规则；如果规则覆盖已经注入的目标字段，随后仅凭内部元数据去重会造成请求正文与运行记录不一致。
- 请求快照只比较路由分组 ID 和版本，不能识别同一 ID、同一版本下的 `managed_instructions` 原始配置变化；快照还需要保存并比较实际配置值。
- 控制台在 `managed_instructions` 不存在时回退显示注册表第一项，导致“未配置”和“已关闭某个配置”看起来相同；未配置时应保持空选择且不显示版本摘要，启用时再使用默认配置。
- 最终处理现在先检查服务端记录的已注入状态，重复调用不会再次执行请求正文路由规则；第二次调用只更新去重记录，请求正文保持不变。
- 请求快照现在同时保存并比较实际的 `managed_instructions` JSON；即使分组 ID 和版本未变，配置内容发生变化也会明确拒绝继续。
- 控制台现在将字段不存在显示为“未配置”，不预选配置也不显示摘要；已保存但关闭的配置仍显示其真实选择和摘要，首次启用时才写入默认配置。

## Niffler 受管理提示词配置：分组改造最终结果（2026-08-03）

- 唯一配置来源已固定为当次请求选中的 `routing_groups.config_json.managed_instructions`；全局模型、Provider、Endpoint 和账号都没有覆盖配置。
- 请求共享快照记录分组 ID 与版本，因此同一分组可以正常切换后续服务，换成另一分组或版本会明确拒绝，客户端正文标记不能代替服务端状态。
- 分组创建、修改和发布共用同一校验入口；控制台规范化与按模型草稿编辑都会保留整个分组的受管理提示词字段，不会因编辑排序设置而丢失。
- 控制台入口归入路由分组编辑页，注册表使用 `admin:routing_profiles` 权限；旧的全局模型入口和 `/api/admin/models/global/managed-instruction-profiles` 不再存在。
- 关闭、格式不支持和 `if_missing` 跳过时，运行记录仍会保存明确原因、`applied`、`deduplicated`、分组 ID 和分组版本；分组未配置时不增加该记录。
- 界面属于普通后台设置表单，没有拖拽、指针跟随、每帧 DOM 测量、模糊材质或高成本动画；静态与真实运行时检查已得到 `PASS ui-review gate`。

## Niffler 受管理提示词配置：本轮补充核对（2026-08-03）

- 路由分组已经以 `routing_groups.config_json` 保存完整配置，并在前后端分别由 `RoutingGroupConfig` 表示；可以直接增加可选的 `managed_instructions` 字段，不需要数据库迁移。
- 请求选择分组后，`LocalRoutingRequestContext.group_config_json` 已携带当次请求使用的分组配置，且随 `LocalRequestedModelDecisionInput` 克隆到 Provider、Endpoint 和账号尝试；运行时不再需要按全局模型 ID 查询数据库。
- 分组控制台入口是 `RoutingProfiles.vue`，同一编辑页同时支持统一排序和按模型排序。受管理提示词属于整个分组，应放在分组基本信息之后、排序方式之前，不能放进按模型编辑区。
- 分组创建与更新当前保存 `config_json`，后端需要在写入和发布版本前统一校验其中的受管理提示词；前端类型应复用现有注册表类型，避免再维护一套配置 ID。
- UI 工作流将本次定为 Web 管理页中等规模重构：保留现有分组保存、发布、排序流程和紧凑密度；合并重复状态与说明，不增加新页面或第二套视觉语言。
- 页面主任务是为整个分组选择并启用配置。新区域只需要标题、启用开关、配置选择、加入方式和必要的版本摘要；放在分组基本信息之后可直接说明归属，且不会被按模型排序界面误解为单模型设置。
- 后端分组创建、更新和发布都汇聚到 `validate_config_json`；在这里同时调用受管理提示词校验即可一次覆盖三个写入入口，错误继续按现有 400 返回。
- `RoutingGroupConfig` 当前没有扩展字段透传机制，Rust 和 TypeScript 类型都必须显式增加 `managed_instructions`；前端 `normalizeRoutingGroupConfig` 也必须保留该字段，否则编辑其他路由设置时会把提示词配置静默删除。
- 当前没有 `RoutingProfiles.vue` 组件测试。需要至少新增工具层配置保留测试，并为分组页增加保存载荷或独立配置组件测试，避免只依赖手工页面检查。
- 分组选择优先级已经固定为显式请求、API Key 默认、用户默认、用户组默认、系统默认；只有启用的分组会进入 `LocalRoutingRequestContext`。提示词配置自然继承同一选择结果，不需要再设计第二套分组选择规则。
- `LocalRequestedModelDecisionInput` 的克隆会复制路由上下文并共享 `OnceCell`。快照应记录分组 ID 和版本，发现共享快照被不同分组上下文复用时显式失败；同一分组内切换全局模型应允许继续。
- 七个最终请求入口当前都额外传入 `state` 和 `candidate.global_model_id`，只用于读取模型配置。迁移后统一函数只需要请求输入和最终决策，调用点可以同时删除这两个参数，减少无关数据库依赖。
- 注册表响应构造当前位于全局模型读取模块，迁移时应移动到路由管理处理器，并将路由权限从 `admin:models` 改为 `admin:routing_profiles`；旧固定路径和分类测试应删除。
- 全局模型创建与更新目前仅多调用了一次受管理配置校验，删除该调用即可恢复原有模型保存行为；提示词字段即使残留在模型 `config` 中也不会被运行时读取或形成覆盖。
- 审查确认三个问题需要修复：关闭状态的未知配置被隐藏且不能直接替换；后端按去除首尾空格后的 ID 校验却保存原始值；七条生产入口目前只有源码字符串检查，没有覆盖三种最终格式的真实最终处理测试。
- 全局模型更新即使没有修改 `config`，也会再次校验现有配置；因此旧 ID 或其他无效配置会阻断显示名、价格等无关修改。控制台必须显示问题并允许在不启用功能的情况下选择有效配置。
- `normalize_json_object` 只检查 JSON 对象形状，不会把解析器规范化后的 `profile_id` 写回配置；修复应统一拒绝带首尾空格的 ID，避免运行时与控制台看到不同值。
- 运行链路测试应直接调用共享最终策略函数，使用真实全局模型仓库配置，分别断言 Responses、Chat、Claude 的最终请求体和运行记录；源码入口检查继续保留，用于防止生产调用点遗漏。
- 用户最终将公开配置收敛为两个：`security_research_v1` 合并原安全研究与逆向工程规则，`adult_fiction_v1` 保留成人创作；`direct_v1`、`chemistry_v1` 和 `weapons_engineering_v1` 删除，原 `security_ctf_v1`、`reverse_engineering_v1` 被合并配置替代。
- `core_v1` 不是控制台配置，继续作为两个配置共享的内部正文；普通模型不绑定受管理提示词，避免额外 Token 和对客户端规则的影响。
- 完整网关测试使用多种只提供认证、候选和 Provider 目录的最小数据状态，这些环境没有全局模型读取器；受管理提示词必须将“没有读取能力”视为功能不可用并保持默认关闭，不能误报模型记录丢失。读取器存在时仍按生产语义要求记录必须存在且配置必须有效。
- 第一次全量测试的大量 500 和后续等待都由上述同一误判引起；修正后 2964 项库测试和 28 项附加测试全部通过，说明没有剩余的跨协议回归。
- `LocalRequestedModelDecisionInput` 是一次客户端请求在不同 Provider、Endpoint 与账号尝试间共享的可克隆上下文；当前没有受管理提示词状态，适合增加共享的请求级快照，避免每次重试重新读取或出现配置漂移。
- `AiExecutionDecision` 同时携带最终 `provider_api_format`、`provider_request_body` 与 `report_context`；统一后处理可以在正文最终成形后修改请求体，并将执行结果写入现有运行记录上下文，不需要修改数据库结构。
- 设计仍采用“先执行 Provider Request 路由规则，再注入受管理提示词”的顺序，防止路由正文补丁删掉已注入内容；未启用配置时不改变当前请求体。
- 最终 Provider Request 路由处理目前有 7 个生产调用点：OpenAI Responses、OpenAI Chat、标准格式族、同格式透传、文件、图片和视频；新统一入口可以覆盖这些位置，对非目标格式直接跳过。
- 管理端已有统一路由分类和本地读取响应分派；新注册表接口应使用同一 `global_models_manage` 权限族，并在通用“按 ID 查询”之前识别固定路径，避免将 `managed-instruction-profiles` 误当成模型 ID。
- 前端已有 Radix Vue 的 `Select` 组件和统一表单样式；受管理提示词区可以复用现有控件，不增加新的界面依赖。
- 固定注册表路径必须放在 `/api/admin/models/global/{id}` 规则之前；现有路径计数规则会将 `/api/admin/models/global/managed-instruction-profiles` 识别成单模型查询。
- `GlobalModelFormDialog.vue` 当前在基本信息后直接进入价格配置，且提交时保留整个 `config`；新增独立紧凑子组件并绑定 `config.managed_instructions` 可以避免扰动现有模型能力与计费逻辑。
- 前端类型 `ModelConfig` 允许扩展字段但没有受管理提示词的显式类型；需要增加 `enabled`、`profile_id`、`merge_mode` 类型以及服务端注册表响应类型，便于界面校验和预览。
- 各文本决策在构造 `report_context` 时已经写入 `global_model_id`，而调用最终 Provider Request 处理的位置仍持有具体 `candidate.global_model_id`；统一处理函数可以直接接收该 ID，不需要从 JSON 反向解析。
- 全局模型的创建与更新都会在 `write.rs` 中归一化完整 `config`；在记录构造前调用同一校验器即可覆盖管理端新增和修改，不影响数据库契约。
- 运行时可通过 `AppState::get_admin_global_model_by_id` 读取当前全局模型及其 `config`；请求级共享快照仍负责保证后续失败切换不会读到不同配置。
- 外部需求文档含明文验证凭据，只用于本地研究，Aether 的设计、实现、测试和日志均不得复制这些值；对真实上游的付费验证不属于本轮本地实现验证。
- 需求建议的流水顺序与当前代码有一处差异：图片桥接和 Codex 特殊字段在决策构造前已经完成，而 Provider Request 路由规则在决策构造后执行。实现采用最终后处理：先执行路由规则，再临时分离“精确匹配的现有图片桥接后缀”，注入受管理提示词，最后原样接回图片后缀，从而同时保证路由不可删除注入内容和图片规则始终位于末尾。
- 网关包已经依赖工作区 `sha2`，注册表无需新增依赖；共享核心和两份专业源码用 `include_str!` 在编译期嵌入，并在进程静态注册表初始化时统一生成正文和摘要。
- 需求正文最终确认两个配置的完整专业模块和固定注册表字段；实现不把提示词复制进模型 `config`，模型只保存配置 ID、启用状态和合并模式。
- 受管理配置解析采用“字段不存在或显式 `null` 表示未配置；对象存在时三个字段必须完整且类型正确”的规则。关闭状态仍校验配置 ID 和合并模式，避免保存一个以后启用就立即失败的配置。
- 请求级快照已接入 7 条现有最终策略调用路径；增量 `cargo check -p aether-gateway` 通过，说明新增异步数据库读取与现有候选决策签名兼容。
- 图片桥接原实现会裁剪客户端末尾空白并用宽松标记判断重复；现已改为只识别完整固定后缀且不裁剪原字符串，为最终注入完整保留客户端内容提供稳定边界。
- 管理注册表不依赖数据库，可由现有本地管理响应直接返回；接口只暴露配置说明、版本和摘要，不返回完整提示词正文，且沿用 `admin:models` 权限。
- 前端已有可访问性语义完整的 `Switch`、Radix `Select`、`Badge` 和 `Skeleton`，新配置区可以覆盖加载、关闭、可选和错误状态而不增加依赖。
- 全局模型 API 的 `ModelConfig` 是唯一需要扩展的持久类型；表单载荷辅助函数会原样保留 `config`，子组件只需更新 `managed_instructions`，不能重建其他配置键。
- 界面读取注册表后才允许首次启用；已启用配置即使注册表暂时读取失败仍能使用开关关闭，避免错误状态锁死已有模型。
- 两个提示词正文的固定摘要已经通过独立 Shell 管道按同一规范计算，并写入 Rust 预期值；后续源码中除换行格式外的任何字符变化都会触发测试失败。
- 路由契约测试将 7 个最终决策入口固定到同一后处理函数，避免以后新增或重构某条现有路径时退回只执行 Provider Request 路由、漏掉受管理提示词。
- 最终差异复核发现图片桥接可能被 Provider Request 路由规则从末尾挤开；最终处理现会在图片工具仍存在时重新定位最后一份完整固定桥接正文。没有图片工具时，相同客户端文本仍按客户端内容保留，不能据此伪造服务端状态。

## 2026-08-03 Niffler 受管理提示词配置

- 需求文档已经扩展为同时支持最终上游格式 `openai:responses`、`openai:chat` 和 `claude:messages`，并明确了内部状态防重复、摘要规范和 `if_missing` 运行记录。
- 当前仓库提交为 `621c53ef1`，与需求文档记录的研究版本一致。
- 工作区已有三处未提交后端修改和一份新架构文档，属于 Codex Responses 推理项 ID 修复；本任务必须保留，尤其需要谨慎处理可能重叠的执行运行时文件。
- 仓库没有项目内 `AGENTS.md`，继续执行本轮提供的通用规则和文档先行要求。
- 本次不使用需求文档中的在线验证密钥，除非实现和离线验证全部完成且确实需要最小在线对照；任何密钥都不能进入仓库文件或日志。
- UI 已确定为 Web 管理后台的现有全局模型编辑表单重构：保留现有结构、保存入口、设计令牌和表单组件，只增加紧凑配置区；不增加卡片套卡片、长说明或新的视觉风格。
- 管理端主任务是启用配置、选择配置和合并方式，并在保存前确认版本摘要与组合顺序；需要覆盖开关联动、即时预览、加载、不可用、保存失败和完成反馈。
- 前端实际是 Vue/TypeScript，相关页面入口为 `frontend/src/features/models/components/GlobalModelFormDialog.vue`；首次 UI 工作流误填 React，必须重新生成 Vue 版本约束。
- 当前未提交改动集中在 Responses 推理项 ID 规范化和无效请求停止换号；本任务尚未修改这些文件。
- 全局模型的数据契约已经在公开模型、管理模型、创建和更新记录中保存 `config: Option<Value>`，第一版可以按需求复用 JSON 配置，不需要新增数据库列。
- 前端全局模型表单已经读取并提交任意 `config` 字段，`global-model-form-helpers.ts` 会在创建和更新时保留非空配置；新增配置应在该表单内做类型化读写，不能重建或覆盖其他现有键。
- 请求记录已有 `provider_request_body` 和 `request_metadata`，适合分别保存最终请求正文与受管理提示词元数据；仍需确认写入发生在请求转换后的哪个阶段。
- `ExecutionPlan` 保存最终请求体、客户端格式和上游格式，但没有单独的全局模型配置或任意请求内部状态字段。
- 调度候选保存 `global_model_id` 和 `global_model_name`，不保存完整全局模型配置；候选查询目前只从 `global_models.config` 提取模型映射。直接给候选结构增加完整配置会波及三种数据库实现和大量测试，应先确认能否在规划阶段按已有 `global_model_id` 读取一次管理模型配置。
- 网关状态已有按全局模型 ID 读取 `StoredAdminGlobalModel` 的入口，可能无需改变数据库查询结构；最终方案要以请求规划函数是否可访问状态为准。
- 标准请求的候选载荷构建函数是异步的，已经同时拿到 `AppState`、候选 `global_model_id`、最终 `provider_api_format` 和 Endpoint `body_rules`；这里可以在不扩展调度查询结构的情况下读取全局模型配置。
- Responses/Chat 规范化函数当前先应用 Endpoint `body_rules`，再执行 Codex 特殊处理和图片桥接，正好提供需求要求的注入顺序；要保持图片规则位于末尾，受管理提示词应作为可选参数传入这些规范化函数并在两者之间应用。
- 请求决策完成后还存在路由组的 Provider 请求体修改阶段。需求文档只描述 Endpoint `body_rules`，Aether 设计记录必须明确路由组修改与受管理提示词的先后关系，避免后续规则无意覆盖注入结果。
- 已确认路由组的 Provider 请求体修改发生在候选请求体和报告上下文初步生成之后；如果在更早阶段注入，路由组仍可能覆盖 `instructions/messages/system`。统一可靠的顺序应是：格式转换与 Endpoint `body_rules` → 现有特殊处理和图片规则 → 路由组修改 → 受管理提示词最终处理，其中 Responses 需要识别并保持图片规则仍位于末尾。
- 同格式请求存在独立的 passthrough 决策路径，会绕过标准跨格式正文构建函数；只修改 Responses/Chat 规范化函数无法覆盖验收范围。
- 报告上下文允许加入任意 `extra_fields`，可以保存 `managed_instructions` 元数据，不需要改变数据库请求记录表结构。
- 直接给 `StoredMinimalCandidateSelectionRow` 增加字段会影响约 221 处结构体构造，改动面明显不合适；应避免为一个全局模型配置扩展底层候选行契约。
- `aether-ai-formats` 的标准格式矩阵集中完成四种协议转换、Endpoint `body_rules` 和 Codex 特殊处理，但同格式旁路仍可能绕开它，因此最终决策生成后的统一处理更稳妥。
- 候选决策构建完成后同时具备最终请求体、最终上游格式、全局模型 ID 和可扩展报告上下文。可新增一个只处理文本格式的最终处理函数：先读取并验证全局模型配置，再修改最终请求体并写入报告元数据；`openai:responses:compact` 明确跳过。
- 需要避免每个失败账号都重复查询全局模型。下一步先查是否已有按名称读取或请求级缓存入口；若没有，应在一次请求的候选准备阶段解析一次，而不是在执行重试循环里读取。
- 网关既支持按全局模型 ID 也支持按名称读取管理模型；仍应以候选中的 ID 为准，因为客户端名称可能经过别名和模型映射解析。
- 图片桥接规则使用固定的 `<niffler-codex-image-generation>` 完整文本，并总是追加到 Responses `instructions` 末尾。最终注入函数可以只在该完整固定文本确实是后缀时临时分离，完成受管理提示词合并后再原样追加，避免依赖宽松的标签搜索。
- 受管理提示词最终处理必须在路由组修改之后执行；否则路由组可以改写或删除已经注入的字段。实现将把这一当前代码差异写入 Aether 设计记录。
- `LocalRequestedModelDecisionInput` 是一条请求在候选规划期间共享的对象，真实业务入口只有少量构建点。各入口完成候选预选后都能从首个候选取得最终 `global_model_id`，因此可以在这里读取、解析一次配置并保存到请求输入，随后所有账号和 Endpoint 复用同一份不可变配置。
- 使用客户端传入的模型名称提前读取配置不可靠，因为候选预选可能通过别名或模型指令解析到另一个全局模型；必须在得到已解析候选后按 `global_model_id` 读取。
- 内部可调度候选对象只有约 7 处构造，但 Provider 号池会动态产生候选。把已经解析的配置放在请求输入中比复制到每个候选更直接，也能天然保证失败后更换账号不改变配置。

---

## 2026-08-03 全部本地改动提交与生产同步

- 仓库 `ryfineZ/Niffler` 为公开仓库；提交前必须排除真实用户用量报告、凭证和其他敏感本地文件，即使用户要求提交全部改动也不能将其公开。
- 当前没有已暂存文件；本地 `main` 为 `45eed37d2`，远端和生产 `main` 为 `fac7862dc`。本地远端跟踪引用过期且提交图显示异常分叉，不能直接在当前 `main` 上创建发布提交后推送。
- 当前工作区共 50 个已跟踪修改文件和 35 个新文件；其中 22 个文件内容已经位于远端/生产，整理到远端最新基准时应自然消失，不能重复提交。
- `outputs/` 含真实用户名称、请求 ID、钱包 ID、套餐账本和余额变化等计费明细，仓库又是公开仓库，不能提交。已在 `.gitignore` 中明确忽略该本地报告目录，文件本身保留在本机。
- 本机没有安装 `gitleaks`、`trufflehog` 或 `detect-secrets`；后续使用 Git 已跟踪差异和新文件的定向敏感模式检查，并人工检查所有环境示例文件。
- 已跟踪差异和全部未跟踪文件的定向敏感模式检查均未发现密钥、令牌、私钥或带密码的数据库连接；两个生产监控环境示例只包含节点名、阈值、容器名和公开健康地址。
- 其余新文件最大约 15 KiB，没有异常大文件；排除 `outputs/` 后可以进入远端基准整理阶段。
- 通过 GitHub CLI 已从远端 `main` 成功创建深度 1 的隔离发布仓库；该方式绕开当前本地旧 `main` 的异常提交图，不需要重置或覆盖用户工作区。
- 直接复制本地完整文件会回退远端在同一文件中的新增内容，`docs/operations/ci-image-deploy.md` 已出现明显逆向删除。发布整理必须使用本地 `45eed37d2` 的文件差异与远端 `fac7862d` 做三方合并，而不是按文件覆盖。
- 已从第一个完整隔离仓库的远端 `fac7862d` 创建新的 detached 干净工作树，并将本地 `45eed37d2` 对象导入其对象库；三方合并所需的远端版本、本地基础版本和本地修改均已就绪。
- 三方合并后 Rust 格式检查、Git 差异检查和全部新增 Shell 脚本语法检查通过；本机没有 ShellCheck，不能将 ShellCheck 计入本轮验证结果。
- 三方合并后的网关 2942 项测试首次运行有 2940 项通过、2 项失败；失败均因 HTTP 429 被账号池新增的模型容量暂停分支提前接管。流式换号仍需把 429 视为可换账号错误，但账号池暂停必须继续使用现有 `rate_limited_429` 和 Retry-After，因此修复点应放在账号池写入分支而不是移除流式 429 支持。
- 修正账号池分支优先级后，网关 2942 项完整复测、数据层 508 项和计费 38 项测试全部通过；说明本地改动与远端最新版合并后的核心后端行为一致。
- 前端相关测试、类型检查、目标代码规范检查和生产构建均通过；构建只有既有的浏览器数据过旧与大分块提示，没有编译错误。
- 监控脚本以 Linux、root 为运行前提，macOS 直接测试失败不表示脚本缺陷；在 `hd0526` 临时目录使用测试内置的假 Docker、假磁盘和假 Telegram 依赖运行后，两项测试均通过，且没有接触生产监控配置。

## 2026-08-03 远端与生产版本核对

- GitHub 远端当前有 4 个分支；远端 `main` 最新提交为 `fac7862dc76e1baf0f76f165bd67c1068e53839a`，提交说明为合并 Provider 结算统计一致性修复。
- 本地 85 个状态异常文件中有 22 个文件内容与远端 `main` 完全一致；本地仍显示修改是因为本地 `main` 基准过旧，不能据此判断尚未提交。
- 本轮还需核对生产实际镜像、数据库迁移和接口行为，才能判断其余 12 个关注文件是否已经通过其他提交或部署方式上线。
- 仓库现有生产核对入口为 `actions-production-deploy.sh`、`actions-production-ssh-command.sh`、`fixed-production-deployer.sh` 和 `deploy-ci-artifact.sh`；后续只读取这些明确文件和生产状态，不使用不确定的脚本名。
- 生产受限 SSH 的 `status` 命令只读返回 `/opt/niffler-app/.niffler-deployed-commit`；固定部署器还会校验镜像的 `org.opencontainers.image.revision` 与目标提交一致，因此可用状态文件、容器镜像标签和容器健康状态交叉确认生产版本。
- `hd0526` 生产状态文件、`frontdoor` 镜像、`background` 镜像及两个镜像的 revision 标签均为 `fac7862dc76e1baf0f76f165bd67c1068e53839a`；两个容器均为 running/healthy。该提交正是远端 `main` 当前最新提交，因此远端 `main` 的代码已经部署到生产。
- Provider 统计的本地剩余差异重新分类后只有 3 个：一个 SQL 断言测试改为匹配已经上线的生产 SQL，一个数据结构清单补充已上线的新表，一个 `provider_contribution.rs` 等价写法调整；它们不包含尚未上线的 Provider 运行逻辑。此前将迁移列表测试归入 Provider 统计不准确，该测试实际属于 GPT-5 价格迁移。
- CCSwitch 的基础余额兼容已经在生产，但本地 5 个文件是其后续口径调整：生产当前仍将钱包和套餐余额相加写入 `remaining`；本地改为 `remaining` 只表示钱包、套餐单独显示，并将无限额账号的数值余额改为 `null`。这部分当前不在生产提交。
- GPT-5 本地差异实际为 4 个文件：计费规则支持显式空缓存写入价为零、价格数据库迁移、迁移列表测试和运维说明。迁移会禁用两个无价格的 Pro 模型并更新 GPT-5.4/5.5/5.6 系列价格；是否已经通过人工数据库操作生效仍需查询生产数据库。
- 生产 `_sqlx_migrations` 已成功记录 Provider 统计迁移 `20260801190000`，没有 GPT-5 价格迁移 `20260731190000`。Provider 统计迁移确定已上线；GPT-5 价格迁移没有通过正常发布流程执行。
- 生产 `global_models`、Plus 号池、Pro 号池和 `niffler_model_base_prices` 中的 GPT-5.4/5.5/5.6 价格与本地迁移目标一致，`gpt-5.4-pro` 已禁用，影子价格版本也存在；因此价格数据确实已经上线，只是没有对应迁移记录和远端迁移文件。
- GitHub 历史显示 CCSwitch 基础兼容分别在 `505d1e0922`、`2f0359ed2e` 等提交上线；本地这 5 个文件是之后尚未提交的第二次口径调整。GPT-5 价格迁移路径在远端没有任何提交记录，计费规则文件自 5 月后也没有本地显式空值修复，进一步确认生产价格是通过数据库操作上线，而不是代码发布。
- GPT 模型 `updated_at` 会被模型刷新任务持续更新，不能单独作为价格写入时间证据；迁移历史缺失、远端文件缺失和生产价格已存在三项证据共同支持“数据已上线、代码未归档”的结论。

## 2026-08-02 OpenAI Responses 输出前自动换号

- 线上容量错误由 OpenAI Responses SSE 中的 `response.failed` 返回，上游 HTTP 状态仍可能是 200；直接 OAuth 使用不同账号身份和调度通道，因此不经过 Niffler Pro 号池的失败账号。
- 当前生产修复能在有限预读窗口内识别嵌入错误并换号；窗口固定为最多 5 帧、16 KiB，读取下一帧等待 750 毫秒，属于概率性缓解，不是协议提交边界。
- 账号池现有 4 个有效 Pro 账号且周额度均未耗尽；容量错误是账号与模型组合上的临时上游容量问题，不等同于额度耗尽或 OAuth 失效。
- 当前后端只支持提供商级 `failover_rules`（最大重试、状态码和正则规则），没有 `stream_failover` 配置；此前给出的 TOML 只是方案示例，直接添加不会生效。
- `provider_endpoints` 已有 `config` JSON 字段，端点本身已有 `api_format` 和 `max_retries`，最合适的存储位置是 `openai:responses` 端点的 `config.stream_failover`，避免重复声明适用格式和重试次数。
- 管理前端已有端点编辑弹窗、最大重试次数和提供商故障转移规则弹窗，但没有流式提交策略入口。
- 用户确认不做灰度发布；新能力需限定在目标端点、默认关闭、可一键关闭，并通过测试和监控控制上线风险。
- 当前本地 `main` 为 `45eed37d2`，工作区存在其他任务的大量未提交改动；`origin/main` 比本地多出已合并的容量错误有限预读修复。实现必须手工吸收所需逻辑，不能直接拉取、重置或覆盖现有工作区。
- 仓库没有额外的项目内 `AGENTS.md`；继续遵守本轮提供的全局规则和文档先行要求。
- 远端已合并的《Codex 顶层流式错误处理》将容量错误、上下文超限和账号冷却纳入统一失败分类，但 OpenAI Responses 仍依赖固定帧数、字节数和 750 毫秒读取等待；本任务应在该修复基础上改为端点可控的协议提交边界。
- 现有错误传播文档明确区分“用户请求不可重试”和“临时上游错误可换号”，新逻辑必须复用当前分类结果，不能仅按错误字符串无条件重试。
- `GatewayProviderTransportSnapshot.endpoint.config` 已随每个执行计划进入网关运行时，无需新增数据库列或全局配置；端点更新接口也已支持读写任意 JSON `config`。
- 端点已有独立 `max_retries`，新配置不应再增加 `max_attempts`；实际总尝试次数继续由现有值决定，新配置只控制输出提交、可重试错误和暂停时间。
- OpenAI 官方 Responses 文档将流定义为带类型的 SSE 事件，要求消费者按 `event.type` 处理；`response.created` 只表示响应对象创建，`response.failed` 明确表示响应失败，文本和函数参数由独立 delta 事件产生。这为协议事件驱动的提交边界提供了正式依据。
- 账号池调度游标已经持有 `requested_model`，且账号真正返回前会再次检查运行时暂停状态；新增“提供商 + 账号 + 模型”暂停键后，可在该最后检查点排除失败组合，不需要将模型暂停混入账号全局健康状态。
- 当前 `EndpointFormDialog` 已支持合并保存 `endpoint.config`、保存反馈和禁用状态，但没有展示端点 `max_retries`。新配置区需要在启用时一并编辑端点现有 `max_retries`，不能把次数重复写入 `stream_failover`。
- 端点创建和更新最终都经过网关的 Provider record builder，更新路径可取得端点最终 `api_format` 与合并后的 `config`；在此处做后端二次校验，可以同时覆盖管理界面、直接 API 调用和配置导入后的运行时防御。
- `append_local_failover_policy_to_value` 已被各类本地执行计划统一调用；将端点级流策略一并写入 report context，可避免流执行阶段重复读取数据库，并能随远程执行计划完整传递。
- `stream_failover` 仅允许写入 `openai:responses` 端点，缺省或关闭时保持原请求逻辑。
- 管理接口拒绝不支持的模式、错误类型和越界参数；历史导入数据在运行时会限制到安全范围，避免异常配置拖垮请求。
- 配置层 6 项针对性测试全部通过，可以继续接入流处理。
- SSE 解析现在只在完整空行事件边界上作判断，跨数据帧的半个 `response.failed` 不会被误当成正常输出。
- 连续 8 个 `response.in_progress` 后出现容量失败的测试可以换号，证明核心路径不再依赖固定 5 帧窗口。
- 模型暂停键对模型名进行大小写和空白归一化；调度测试确认同一账号只跳过失败模型，其他模型仍能安排。
- 输出前换号使用内置临时错误允许列表：容量、上游过载和限流可换号；参数错误、上下文超限等确定性错误直接返回。
- 前端将等待时间显示为秒、暂存上限显示为 KB，保存时转换为后端的毫秒和字节；端点其他配置不会被覆盖。
- 整库回归要求保留其他协议原有的通用失败分类；参数错误不换号由新的 Responses 输出前路径单独执行临时错误允许列表，不能扩大成全局分类规则。
- 网关架构测试要求格式判断统一经过 `ai_serving` 根入口，执行层和编排层不能直接依赖底层格式包。

---

## 2026-08-02 Provider 账号计费统计一致性

- 根因已确认：终态 usage 首次写入时仍是 `pending`，Provider 请求数和 Token 已进入统计，
  结算完成后费用和 Codex 窗口贡献没有再次同步，形成长期错误零值。
- 请求级贡献表现在保存每个请求当前应计入 Provider 的请求数、Token、基础成本、窗口用量
  和修订号；重复结算按差额生成稳定事件，不会重复累计。
- 历史数据不在迁移事务中全表扫描。后台按账号和 `(created_at, request_id)` 分批整理，
  单账号完成后再统一重建累计统计和窗口统计。
- 统计重建、增量刷新和窗口重置使用固定锁顺序；并发测试验证最终结果以请求级贡献为准。
- 单账号统计修复失败会保存失败次数和错误原因，并延迟重试；其他账号继续处理。
- 本机 PostgreSQL 集成测试因系统共享内存资源不足被跳过，当前没有真实执行这些数据库场景；
  编译、Schema 检查、静态 SQL 测试、内存测试和精度测试均已通过。

## 2026-08-01 提供商测试、单账号测试与账号并发

- Niffler 现有模型测试后端已经支持 OpenAI Chat、Responses、图片、Embedding、Rerank、Claude、Gemini 以及多个专用适配器；本次应复用该链路，不另写裸 HTTP 测试。
- `TestModelRequest` 已声明 `api_key_id`，但前端 composable 尚未传递；后端候选构造已经有按账号过滤的基础逻辑，补齐前端参数和非 Kiro 通用路径即可。
- 提供商账号列表来自 `/api/admin/endpoints/providers/{provider_id}/keys`，最终每行数据由 `build_admin_pool_key_payload` 组装；运行时状态已经集中在 `AdminProviderPoolRuntimeState`。
- Redis 请求令牌保存于 `ap:provider_pool:in_flight:{provider_id}`，令牌末尾结构为 `...:{key_id}:{uuid}`；使用从右侧按冒号解析可以按账号聚合，不需要新增 Redis 计数器。
- 账号列表当前已有 `concurrent_limit` 和请求统计字段，新增 `current_concurrency` 后可以直接显示“当前 / 上限”。
- `ProviderDetailDrawer` 已有账号分页刷新函数，但没有定时刷新；打开抽屉期间每 3 秒刷新当前页即可覆盖并发观察场景。
- `ModelsTab` 已拥有统一模型测试弹窗和 `useModelTest`，通过暴露 `openAccountTest(keyId)` 给父抽屉可以复用同一套结果和图片预览，不需要复制测试逻辑。
- 当前测试弹窗的请求头/请求体是默认主界面；应新增协议模板区和高级编辑开关，结构化字段最终仍转换为现有请求体。
- 复核确认：`request_candidates.api_key_id` 表示用户 API Key，提供商账号必须继续写入 `key_id`；模型测试追踪已恢复为空用户 API Key 字段。
- 复核确认：号池配置存在时，账号列表直接复用 `read_admin_provider_pool_runtime_state` 的按账号并发结果；无号池配置的提供商才单独读取 Redis 令牌。
- 验证确认：账号并发聚合、单账号 `api_key_id` 传递、账号按钮入口、Responses Compact、Gemini 模板和现有图片模板均有针对性测试覆盖。

## 2026-07-28 GitHub 测试环境与晋级流水线配置

- PR #14 已于 2026-07-28 合并到 `main`（合并提交 `c9bcd31070d721552ef0e84f865934f9444564fc`）；最新 CI 全部通过。
- 仓库实际检查名称为 `check`、`Frontend`、`Release tooling`，与管理员配置说明一致。
- PR #14 新增的主分支晋级检查名称为 `Promotion policy`。
- 测试域名 `niffler-test.123.253.224.101.sslip.io` 当前解析到 `123.253.224.101`。
- 当前工作区包含其他已完成任务的未提交改动，本任务只操作 GitHub 设置、服务器配置和必要的远端分支，不改动这些文件。
- 当前 GitHub CLI 登录账号为 `ryfineZ`，对仓库具有管理员权限。
- 现有 `production` Environment 只要求 `ryfineZ` 审批，已开启禁止自审，并仅允许 `main` 分支部署。
- `g-dxw` 是仓库协作者，权限为 write，可以加入 production reviewer。
- 现有 main Ruleset ID 为 `19769823`，已启用删除保护、强推保护、单人审批、旧审批失效、对话解决、严格状态检查和仅 Merge 合并。
- 已创建 `test` Environment：无审批、仅允许 `test` 分支；三个部署 Secret 和四个非敏感 Variable 均已写入。
- 已创建 `Protect test integration branch` Ruleset（ID `19890918`）：禁止删除和强推，要求 Merge PR、对话解决以及 `check`、`Frontend`、`Release tooling` 三项通过；2026-07-28 起不再要求额外审批，允许提交者自行合并 `test` 分支 PR。
- 已将 `g-dxw` 加入 production Environment 审核人；仍开启禁止发起人自审和只允许受保护分支部署。
- 测试服务器 `123.253.224.101:22` 的 ED25519 主机指纹已核对。2026-08-02 已通过 root 密码取得一次管理员访问，新增 `niffler-test-admin` 管理密钥与既有 `niffler-test-deploy` 部署密钥，并保留服务器原有授权密钥；两把新密钥均完成真实登录验证。
- PR #14 已获批准，所有必需检查均通过，且当前 `main` 已是其祖先；GitHub 仍返回
  `mergeStateStatus=BLOCKED`，不是“分支未同步”导致。原 PR 的测试服务器文档和首次部署
  行为存在缺口，已在隔离分支准备修复，不能直接合并原 PR。
- 原 PR 将 GitHub 环境配置为 `niffler-test-deploy`，文档却要求创建 `ops`，并把源站健康地址
  写死为未说明的 `127.0.0.1:18084`。修复改为显式的
  `MYLINGWEAVE_SOURCE_HEALTH_URL`（当前按既有测试主机约定配置为
  `http://127.0.0.1:18084/_gateway/health`），
  并支持没有现有 `app` 容器的首次测试部署。
- 修复后的首次部署会显式使用待发布镜像执行迁移检查，避免错误使用测试 `.env` 的旧镜像；
  同时强制测试目录为 `/opt/niffler-test` 并要求 root 创建的环境标记文件，防止常见的
  测试配置误指向生产目录。独立审查和全部相关脚本测试均已通过。
- 从公网实测，`niffler-test.123.253.224.101.sslip.io` 已由 Nginx 提供 HTTPS，健康接口返回
  Aether Gateway 的 `status=ok`；80/443 可达、8084 未公开。2026-08-02 已从测试服务器确认
  容器仅绑定 `127.0.0.1:18084`，该健康地址返回 200，因此 GitHub 的源站健康地址配置正确。
- 服务器现已启用全局仅公钥认证：`PasswordAuthentication no`、`KbdInteractiveAuthentication no`、
  `AuthenticationMethods publickey`，root 仅允许公钥登录；`sshd -t`、热加载、两把新密钥登录和
  密码登录拒绝均已验证。原先优先加载的 `99-niffler-hardening.conf` 曾开启密码认证，已先备份再修正。
- `Promotion policy` 与 required deployment `test` 不能在 PR #14 合并前加入 main Ruleset：该工作流尚未存在于 main，会反过来阻塞本 PR。此前短暂加入后已立即恢复 main Ruleset 原有状态。
- 本机 GitHub CLI 的 OAuth 授权缺少 `workflow` scope，无法合并包含 `.github/workflows/app-image.yml` 的 PR；已使用登录为仓库所有者的 GitHub 页面完成普通 Merge，未使用管理员绕过。
- 合并后的 `main` 已完成 Release Tooling CI 与 Frontend CI；本次合并没有触发生产部署。

---

## 2026-07-28 生产服务器 Telegram 监控

- `rn01` 根分区使用率 42%，PostgreSQL 和 Redis 容器健康。
- `hd0526` 根分区使用率 83%，已经超过计划的 80%预警线；frontdoor 和
  background 容器健康。
- Niffler 公开健康接口返回 `status=ok`。
- 两台服务器都有 Docker、curl、jq 和 systemd，可以使用同一套监控脚本。
- 网站健康检查只放在 `hd0526`，避免两台服务器对同一故障重复通知。
- 两台服务器的 timer 已启用并每分钟运行，最近执行结果均为 success。
- `hd0526` 的 83%磁盘预警已发送，之后相同状态没有重复通知。
- 隔离测试确认连续失败阈值、相同异常去重、恢复通知和磁盘状态变化逻辑均正确。

## 2026-07-28 Telegram 数据库备份失败通知

- Telegram Bot `niffler_ops_alert_bot` 的 Token 已通过官方 Bot API 验证。
- 用户已向 Bot 发送开始消息，`getUpdates` 返回一个私人 Chat ID。
- 当前本机网络连接 Telegram API 会被重置，`rn01` 可正常访问，因此通知必须由
  `rn01` 直接发送。
- 失败通知由 systemd 的 `OnFailure` 触发，成功通知由备份服务的 `ExecStartPost`
  触发；人工测试消息单独使用 test 模式。
- 通知脚本和 systemd 服务已部署，Bot Token 与 Chat ID 文件权限为 `0600`。
- 测试消息经 Telegram API 投递成功；备份服务的 `OnFailure` 已指向通知服务。
- 已将成功消息接入 `ExecStartPost`，并补发本次凌晨备份的成功通知，Telegram API
  返回投递成功。
- 原备份定时器保持 enabled 和 active，最近一次备份服务结果仍为 success。

## 2026-07-28 生产网络与数据库连接加固

- 公网 `8084` 位于 `hd0526`，当前 Docker 映射为 `0.0.0.0:8084`，外部请求健康
  接口返回 HTTP 200。
- Caddy 与 frontdoor 位于同一 Docker 网络，通过 `niffler-frontdoor:8084`
  通信；将主机端口改为 `127.0.0.1:8084` 不影响 Caddy。
- `rn01` 的 PostgreSQL 5432 和 Redis 6379 虽绑定公网地址，但 nftables 只允许
  `hd0526` 的固定公网地址，其他来源的外部探测均超时。
- PostgreSQL 当前 `ssl=off`，远程访问规则为 `host all all all scram-sha-256`。
- 生产应用的三个数据库连接变量均使用 `postgres` 超级用户；数据库中没有其他
  可登录角色。
- Aether 已支持 `AETHER_GATEWAY_DATA_POSTGRES_REQUIRE_SSL`，可在不修改业务代码
  的情况下强制 PostgreSQL TLS。
- public schema 有 120 张业务表、2 个序列、1 个视图和 4 个枚举类型由
  `postgres` 拥有；需要将这些业务对象交给新的应用账号，才能保留迁移能力。
- Niffler 没有 SMTP 配置，`rn01` 也没有邮件发送服务或外部通知地址；当前只能
  记录备份失败，不能真实发送通知。
- `hd0526:8084` 已改为只监听 `127.0.0.1`；外部连接被拒绝，Caddy、主页和公开
  健康接口仍正常。
- `niffler_app` 已接管数据库及 public 业务对象，角色的超级用户、建库、建角色和
  复制权限均为 false；迁移所需的建表、改表和建索引权限已在回滚事务中验证。
- PostgreSQL 已启用 TLS 并拒绝远程明文登录；最终 15 条应用会话全部使用
  `niffler_app` 和 TLSv1.3。
- 当前为强制加密但不校验自签名证书身份；证书身份校验需要应用后续支持
  `verify-ca` 或 `verify-full`。
- 认证设置接口、主页和公开健康接口均返回 HTTP 200，全部核心容器健康，备份结构
  检查通过。
- 日志仍能看到此前已记录的历史用量队列外键错误；该错误在本次变更前已存在，
  与数据库账号和 TLS 切换无关。

## 2026-07-28 rn01 首次数据库备份与恢复验证

- 目标角色是负责生产数据恢复的运维人员；本轮首先保证备份可恢复，而不是只生成
  一个无法验证的压缩文件。
- `rn01` 根分区为 96 GB，当前使用 39 GB、可用 53 GB；inode 使用率 5%。
- 生产 PostgreSQL 15.18 容器 `niffler-postgres` 健康，数据库名为 `aether`，
  当前数据库约 16 GB。
- 最大关系为 `usage` 约 5.8 GB、`usage_http_audits` 约 4.0 GB、
  `usage_settlement_snapshots` 约 3.3 GB 和 `request_candidates` 约 1.3 GB。
- 当前只有 1 个检查连接处于 active、12 个连接 idle，没有超过 5 分钟的长事务。
- 本机可用空间约 62 GB，Docker 29.2.1 可用；恢复测试可以放在本机 PostgreSQL 15
  隔离容器，不占用 `rn01` 生产磁盘。
- R2 存储桶 `niffler-db-backups` 已创建为私有，专用凭据只允许读写该存储桶。
- 首次完整导出于 2026-07-28 00:23:33 开始，00:30:20 完成；压缩备份大小为
  1,459,124,818 字节，SHA-256 为
  `059705e60c37061461b12ac955c3f7ecbca28220224d142389e840918609e113`。
- 备份已上传到
  `postgres/aether/daily/2026/07/aether-20260727T162328Z.dump`，R2 返回 HTTP 200；
  对象大小和 SHA-256 元数据与生产文件一致。
- 已从 R2 重新下载备份，本机文件大小和 SHA-256 再次一致；恢复验证不会使用
  `rn01` 上的原始文件。
- 下载文件已在没有公网端口的 PostgreSQL 15.18 隔离容器中使用
  `pg_restore --exit-on-error` 完整恢复成功。恢复库包含 120 张 public 表，
  `users` 294 条、`provider_api_keys` 55 条、`usage` 1,482,291 条，
  无无效索引和未验证约束。
- 恢复完成时生产 `usage` 比备份多 442 条，来源是备份完成后的正常线上请求；
  用户数、表数和关键表结构一致。
- 已安装并启用 `niffler-postgres-backup.timer`，每天北京时间 04:30 执行，
  最多随机延迟 10 分钟。自动任务真实执行成功，生成对象
  `postgres/aether/daily/2026/07/aether-20260727T165106Z.dump`，大小
  1,459,812,843 字节，SHA-256 为
  `7276373c159414f2ca84116c1d195e2fb4fc94cbd1a56ad7fbf43724b8af3a23`。
- 自动任务会保留 7 份每日、4 份每周和 6 份每月备份；执行结果写入
  `/var/lib/niffler-backup/status.env`，失败详情写入 systemd 日志。
- 自动上传第一次尝试收到一次 R2 HTTP 501，rclone 自动重试后成功；从 R2 独立
  复核对象大小和校验文件均一致。
- 自动任务结束后，`rn01` 临时备份目录为空，根分区可用 53 GB，PostgreSQL
  仍正常接受连接。
- 当前没有邮箱、短信或聊天工具通知地址，失败时只会记录本机状态和日志；需要
  后续接入外部监控，才能在无人查看服务器时主动通知。

## 2026-07-27 数据库磁盘事故止血与恢复

- 推荐先把生产 `request_record_level` 从 `full` 改为 `basic`，这是可逆操作，只停止
  保存完整请求/响应正文，不影响用户、登录、余额、计费、用量摘要和路由。
- rn01 磁盘再次达到 100% 后，PostgreSQL 因无法写
  `pg_logical/replorigin_checkpoint.tmp` 持续恢复并重启，配置更新因此无法执行。
- 已精确删除没有任何容器引用、可重新下载的 `mongo:6.0`、旧版 Vaultwarden、
  OpenList 和 Alpine 镜像，未删除容器、数据卷或 Niffler 回滚镜像；根分区由无可用
  空间恢复到约 1.3 GiB 可用，PostgreSQL 随即恢复健康。
- 生产配置已于 2026-07-27 17:55:49（北京时间）切换为 `basic`。两次复核中，
  登录依赖的认证设置和 OAuth Provider 接口均返回 HTTP 200，耗时约 0.08–0.11 秒。
- 切换后 `usage_body_blobs` 新增行数为 0，最后一条正文时间为 17:55:39，说明
  完整正文写入已经停止；8 秒间隔内磁盘只变化约 86 KiB，没有继续快速增长。
- 当前 `usage_body_blobs` 为 345,380 行、总计约 53 GiB，其中约 52 GiB 位于大字段
  存储；`usage_http_audits` 有 657,242 行仍带正文引用或记录状态。
- `usage` 主记录中旧版正文列非空行数为 0；正文表只通过 `request_id` 引用
  `usage`，删除正文不会级联删除用量、计费或用户数据。
- `usage_body_objects` 约 31 MiB，共 55,352 行，全部为 `unavailable`，没有
  `object_key`，说明未配置对象存储时还写入了失败元数据；这些记录不包含正文，
  也不是本次 52 GiB 占用来源。
- 释放 52 GiB PostgreSQL 物理空间需要单独处理历史正文；普通 `DELETE` 和普通
  `VACUUM` 不会立即把表文件空间归还给文件系统。
- 当前仅余约 1.7 GiB，无法在原盘执行需要复制整张 52 GiB 表的 `VACUUM FULL`。
  紧急恢复的可行选择是扩容后保留近期正文，或明确接受删除全部历史正文后以
  `TRUNCATE` 快速释放空间。
- 用户已明确确认删除历史正文。已清空 `usage_body_blobs` 345,380 行和
  `usage_body_objects` 55,352 行，并清除 657,302 条审计记录中的旧正文引用与状态。
- rn01 根分区由 99%（约 1.4 GiB 可用）降至 41%（约 54 GiB 可用）；两轮复核中
  两张正文表和全部正文引用保持为 0。
- 删除前后用户数 293、API 密钥数 265 保持不变；`usage` 与
  `usage_settlement_snapshots` 均从 1,474,329 继续同步增长，说明用量和计费记录
  未被删除且新请求仍正常结算。
- `usage_http_audits` 已使用单进程、16 MiB 维护内存完成 `VACUUM ANALYZE`，旧行
  版本从约 65.7 万降至 19；默认并行清理因容器 `/dev/shm` 仅 64 MiB 曾失败一次，
  不影响正文删除和线上请求。
- Redis `usage:events` 还存在 1,054 条历史事件，其中 860 条处于待确认状态并已重试
  约 14.9 万次；至少有事件引用已删除的 Provider 密钥，导致每约 30 秒一次外键错误。
  这是独立的队列毒消息问题，不能在未核对用量是否已落库前直接删除。

## 2026-07-27 线上登录卡顿诊断

- 用户反馈 `niffler.org` 整体很卡，登录流程也会卡住。
- 本轮只做诊断，先区分浏览器资源加载、Cloudflare、应用认证接口、数据库和服务器资源。
- 从当前网络连续测量：主页首字节约 0.57–0.89 秒，`/health` 约 0.57–0.68 秒；
  Cloudflare、TLS 和基础应用健康接口没有出现数秒级等待。
- 登录页依赖的 `/api/auth/settings` 连续 4 次均在约 5.56–5.68 秒后返回 HTTP 501，
  存在稳定的固定时长阻塞，是当前最明显的异常点。
- 代码路由确认 `/api/auth/settings`、`/api/auth/login` 等认证请求由公开支持路由处理；
  下一步检查设置接口实现及生产日志，确定 5 秒等待来自模块调用、数据库还是外部服务。
- `/api/auth/settings` 与 `/api/auth/registration-settings` 的响应头均显示
  `x-aether-execution-path: public_proxy_passthrough`，响应体明确写明 Rust frontdoor
  未实现该路由；两者分别等待约 5.61 秒、5.68 秒后返回 501。
- 同一路由层的 `/api/auth/me` 在无凭证时约 0.61 秒返回 401，说明慢点不是整个
  认证路由分类，而是需要转交给旧控制服务的公开认证接口。
- 前端 `authApi.getAuthSettings()` 和 `getRegistrationSettings()` 会直接请求这两个
  慢接口；登录页初始化若等待它们，用户会先承受固定 5 秒等待并收到错误配置。
- `LoginDialog.vue` 打开登录框时用 `Promise.all` 同时等待注册设置、认证设置和
  OAuth Provider 列表；任一接口等待或失败都会拖住整组初始化。当前前两个接口
  都会走不可用的转交路径，因此登录框必然受影响。
- 当前 `main` 实际已经实现两个 `auth_public` 设置接口；实现会读取系统配置，
  但使用 `.await.ok()?` 吞掉读取错误并返回 `None`。上层随后将请求当成“本地未实现”
  返回 501，所以线上 501 文案具有误导性，真实原因更可能是配置读取依赖失败。
- `/health` 返回的数据库池指标为 `pool_size: 0`、`max_capacity: 0`；结合稳定约
  5 秒等待，下一步重点验证 Rust frontdoor 是否没有可用数据库连接。
- 交叉探针已直接返回数据库错误：
  - `/api/auth/login` 使用不存在的诊断账号请求，约 5.58 秒后返回 500，
    错误为 `postgres error: pool timed out while waiting for an open connection`。
  - `/api/oauth/providers` 约 5.65 秒后返回相同连接池超时。
  - `/api/public/stats` 约 5.60 秒后以空数据返回；`/api/public/site-info` 串行读取
    两个配置项，约 10.67 秒后返回默认值，时长正好约为两次连接池超时。
- 这说明卡顿根因不是密码校验、浏览器资源或 Cloudflare，而是 Rust frontdoor
  无法从 PostgreSQL 连接池取得任何连接；部分接口显式报 500，部分吞掉错误后返回
  501、空数据或默认值。
- 应用主机 `hd0526` 本身资源正常：负载约 0.08，5.8 GiB 内存中约 4.5 GiB 可用，
  系统盘仍有约 17 GiB；frontdoor/background 容器均显示 healthy，因此不是应用
  主机 CPU、内存或磁盘不足。
- 生产日志给出故障起点：2026-07-27 08:51 UTC 起 PostgreSQL 返回
  `could not extend file ...: No space left on device`；约 08:52 又出现数据库连接被
  对端重置，随后 frontdoor 和 background 持续出现连接池超时。
- 因此更深层根因指向 PostgreSQL 所在主机磁盘写满，数据库随后无法正常服务；
  容器健康检查只检查进程/HTTP 存活，没有发现数据库已不可用。
- 数据库主机 `rn01` 已确认根分区 `/dev/vda2` 为 96 GiB，已用约 91 GiB，
  `Avail=0`、使用率 100%；inode 只用 5%，属于数据容量耗尽，不是文件数量耗尽。
- `niffler-postgres` 容器正在反复重启，日志持续报无法写入 `postmaster.pid`：
  `No space left on device`。数据库目前不是“性能下降”，而是已经停止服务。
- 主机占用集中在 `/var/lib/docker/volumes`，约 75 GiB；Docker 统计所有本地卷约
  80.53 GiB，而镜像约 5.18 GiB、容器日志约 321 MiB，说明主要占用来自持久化卷，
  不是普通容器日志。
- 卷级别已定位：Niffler PostgreSQL 数据卷约 69 GiB，Niffler Redis 卷约 5.4 GiB；
  其它数据库卷均不足 0.5 GiB。
- PostgreSQL 的 69 GiB 中，数据库目录 `base/16384` 约 68 GiB；其中单个普通堆表
  文件节点 `983372` 已分成 52 个 1 GiB 段和一个约 361 MiB 尾段，总量约 51.4 GiB。
- 该表段从 7 月 25 日开始快速增加，7 月 27 日仍持续生成，直到 16:52（北京时间）
  磁盘写满；它带有 FSM/VM 文件，可确认是表数据而非单纯索引或 WAL。需要继续从
  PostgreSQL 系统目录离线映射其表名。
- 已从生产 `pg_class` 文件的只读副本离线映射：文件节点 `983372` 是
  `pg_toast_17751`，其所属业务表 OID `17751` 为 `usage_body_blobs`。
- `pg_class` 统计估算 `usage_body_blobs` 约 37.5 万行；约 51.4 GiB 占用位于其
  TOAST 大字段区，说明磁盘主要被请求/响应正文内容占满，而不是 WAL、索引或日志。
- 日志中最先写失败的文件节点 `16622` 是 `provider_api_keys` 的 TOAST 表；它不是
  最大占用源，只是在磁盘耗尽后某次密钥数据更新首先暴露了写入失败。
- 7 月 25 日的架构改动明确要求：完整正文和 Base64 图片应写入对象存储，PostgreSQL
  只保留元数据；历史 `usage_body_blobs` 当时未迁移。
- 当前生产提交 `f7602638` 已包含该架构对应改动，但 frontdoor 容器环境变量中没有
  `AETHER_USAGE_OBJECT_STORE_URL` 或对应前缀配置，说明生产没有接入对象存储。
- 旧的 6 月保护设计把 `full` 模式单个正文限制为 256 KiB、正文保留 1–2 天；
  7 月 25 日改动为了完整保存图片和流数据移除了固定上限。需要进一步确认对象存储
  未配置时是否仍回退写入 `usage_body_blobs`，以及清理任务是否来得及处理。
- 代码已确认未配置对象存储时的实际行为：`prepare_usage_body_storage` 会把每个正文
  完整 JSON 压缩后放入 `detached_blob_bytes`，随后写入 PostgreSQL
  `usage_body_blobs`；不会标记为不可用，也没有单体大小上限。
- `MAX_INLINE_USAGE_BODY_BYTES` 当前为 0，因此所有非空正文都会进入大字段或对象存储；
  生产没有对象存储时，等价于“所有完整正文继续写数据库，而且取消了 256 KiB 上限”。
- 清理任务虽然会删除超过保留期的正文，但当前 51.4 GiB 增长发生在约两天内；
  对于包含 Base64 图片或完整 SSE 的流量，即使 1–2 天保留期也足以写满 96 GiB 主机。
- 表结构确认 `usage_body_blobs.payload_gzip` 是 PostgreSQL `bytea` 大字段，每个请求
  最多可按四类正文分别写入；系统默认详细正文保留 1 天、压缩正文保留 2 天。
- 已离线读取生产 `system_configs`：`request_record_level` 自 6 月 2 日起明确设置为
  `full`；`detail_log_retention_days=1`、`compressed_log_retention_days=2`、
  `cleanup_batch_size=1000`。
- 因此故障条件完整成立：生产开启完整正文记录，7 月 25 日版本取消正文大小上限，
  同时没有配置对象存储，所有完整正文继续进入 PostgreSQL；两天保留窗口内就积累
  约 51.4 GiB 大字段。
- frontdoor/background 的数据库连接池获取超时均配置为 5000 ms，正好解释外部接口
  固定约 5.6 秒才报错；frontdoor 最大 10 连接，background 最大 5 连接。
- 自动正文清理只在每天 03:00 执行。当前 background 容器在 7 月 27 日约 03:20
  （北京时间）启动，已经错过当天 03:00 的清理时间，下一次要等到 7 月 28 日。
- 当前容器日志中没有成功的 `usage_cleanup_completed`；磁盘在 7 月 27 日约 16:40
  开始报满、16:52 数据库停止，说明新版本启动后还没等到下一次清理，数据库已经先
  被正文写满。每日单次清理与固定部署时间共同扩大了风险。
- 17:23（北京时间）复核时，数据库主机可用空间自行恢复到约 1.3 GiB，PostgreSQL
  完成崩溃恢复并重新健康；`/api/oauth/providers` 已恢复为 HTTP 200、约 0.57 秒。
- 当前根分区仍为 99%，恢复不稳定且没有解除写满条件；在 `full` 和数据库正文回退
  保持不变时，服务很可能很快再次停止。
- 用户确认 17:22 左右手动重启了 `rn01`；复查显示主机 uptime 约 2 分钟，数据库
  恢复确由本次重启触发。
- 重启后五个外部关键接口均恢复 HTTP 200，耗时约 0.60–0.70 秒；当前登录前链路
  已恢复正常速度。
- PostgreSQL 只读聚合确认数据库约 68 GiB，`usage_body_blobs` 总占用约 52 GiB：
  主表约 150 MiB，其余几乎全部是 TOAST 正文大字段。
- `usage_body_blobs` 实际共有 400,503 行：request 131,626、provider request
  61,981、response 104,287、client response 102,609；最早记录为 7 月 24 日，
  已超过配置的两天保留期，说明清理没有及时移除旧正文。
- 数据库恢复后仍在持续写入新正文，最新记录时间已到 17:25；重启没有改变
  `request_record_level=full` 或数据库回退路径，只是暂时让服务重新可用。
- 生产清理历史表明清理逻辑此前能运行：7 月 24–26 日每天 03:00 均成功，7 月 26 日
  清理了 51,201 条正文和 53,724 条头信息。
- 7 月 27 日没有 `usage_cleanup` 记录；部署后的 background 在 03:23 启动，只补跑
  了审计和请求候选清理，没有补跑 03:00 的正文清理。
- 当前 400,503 条正文中，292,733 条已经早于“当前时间减两天”，107,800 条位于
  两天窗口内。大量过期正文未清理的直接原因是每日单次任务被本次重启/部署错过。
- 事故由两个条件叠加造成：无对象存储时取消正文大小上限导致写入速度大幅增加；
  正文清理只按固定时间执行且启动后不补跑，导致过期数据继续占盘。
- 17:28（北京时间）再次复核：PostgreSQL 正常接受连接，容器 healthy，四个登录
  关键接口均为 HTTP 200、约 0.59–0.65 秒；磁盘仍为 99%、可用约 1.7 GiB。
- 当前恢复结论：重启恢复了数据库和登录，但没有消除磁盘占用和持续写入条件；
  必须尽快停止正文继续写入并清理过期正文，否则仍有再次停机风险。

## 2026-07-27 GitHub 受保护生产发布与双人审核

- `main` 当前规则集已经禁止删除、强制推送和非 PR 变更，只允许普通合并，并要求
  `check`、`Frontend`、`Release tooling` 三项检查。
- 仓库当前只有一名具有写权限的维护者，因此最低批准数仍为 0。
- 仓库是公开仓库；GitHub 当前没有 Environment、Actions Secret、Actions
  Variable、Deploy Key 或自托管 Runner。
- Actions 允许使用全部第三方 Action，未强制固定到提交 SHA；工作流默认权限为
 只读，不能代为批准 PR。
- 生产发布由授权管理员从本机调用 `/opt/niffler-release/bin/deploy-production`；
  GitHub 尚未配置专用生产发布凭证或受保护的 `production` 环境。
- `hd0526` 当前 SSH 身份是 `root`，唯一一条 `authorized_keys` 记录没有
  `command=`、`restrict`、`from=` 或 `no-*` 限制；服务器不存在
  `niffler-deploy` 专用用户。
- Docker Socket、固定发布器、发布状态目录和应用目录均由 `root` 管理。直接把
  Actions 新密钥加入现有 root 授权会获得完整服务器控制权，不符合最小权限要求。
- 生产固定部署器与仓库脚本 SHA-256 一致，线上应用仍为 `f7602638`，当前
  `main=89fecfb7` 只多出分支整合文档，不存在应用代码差异。
- 新方案必须确保只有受保护 `main` 的准确提交在环境审批后能够使用生产凭证，
  且凭证不能复用个人 SSH 身份。
- `Build App Image` 已有测试环境 SSH 部署范例，会核对服务器主机密钥指纹，但
  当前生产流程没有 `production` Job；工作流也允许从手动选择的 ref 构建。
- 现有 `deploy-ci-artifact.sh` 会先通过远程脚本读取 Docker 和状态文件，再用
  SCP 上传镜像并执行固定部署器。这个协议默认远端 SSH 用户可以访问 Docker，
  不适合直接套用到无 Docker 权限的专用发布用户。
- GitHub 官方环境保护支持把环境 Secret 限制到指定分支，并在审核通过前不向
  Runner 提供 Secret。公开仓库可使用 required reviewers、禁止自行批准和禁止
  管理员绕过。
- GitHub 官方安全建议指出，只有完整提交 SHA 能让第三方 Action 引用保持不可变。
  当前仓库允许全部 Action，工作流仍使用 `@v5`、`@v7` 等浮动标签；任何接触生产
  Secret 的新 Job 至少必须固定自身使用的 Action，启用全仓库强制前需先机械更新
  全部现有工作流。
- 推荐的服务器边界是：创建无 Docker 组权限的 `niffler-deploy` 用户，只允许
  上传到本人专用目录；通过 root 所有的固定包装器校验提交号、文件所有者、真实
  路径和参数后，再调用固定部署器。不能允许 Actions 任意执行 root shell。
- 生产 SSH 使用非默认端口；Actions 需要将主机、端口、用户和服务端 Ed25519
  指纹分别存入 `production` 环境，不能依赖本机 `~/.ssh/config` 别名。
- `/opt/niffler-app/.env` 和 `docker-compose.yml` 都是 `root:root 0600`，
  专用用户无法读取生产密钥或 Compose 配置；部署状态文件为 0644，可作为只读
  状态查询来源。
- 固定部署器已经验证目标必须等于远端 `main`、镜像 revision 必须等于目标提交、
  PostgreSQL 迁移必须兼容，并在健康失败时自动回退。受限包装器只需固定参数和
  文件边界，不应复制这些发布判断。
- 现有固定部署器测试具备假 Git、Docker 和健康端点，可扩展包装器测试，覆盖非法
  提交号、路径逃逸、错误所有者、符号链接、非专用调用者和成功转交。
- 新增受限协议不接受交互式 shell、SCP 或任意远程参数；自动发布不开放
  `--allow-rollback`，紧急回滚仍保留在管理员本机入口。
- Runner 会在上传前计算镜像 SHA-256，并要求服务器返回同一摘要后才调用部署；
  服务器包装器仍会独立验证文件真实路径、所有者、模式、大小和镜像提交标签。
- `Deploy Production` 只使用固定到完整提交
  `93cb6efe18208431cddfb8368fd83d5badbf9bfd` 的 `actions/checkout`，输入目标必须
  等于工作流从 `main` 检出的 `GITHUB_SHA`。
- 工作流最初的主机密钥验证只确认扫描结果中存在期望指纹，却会将其它未匹配密钥
  一并写入 `known_hosts`；必须只保留逐条计算后与环境指纹精确一致的密钥。
- 修正后会逐条解析扫描密钥、独立计算 SHA-256 指纹，并只安装匹配记录；回归测试
  已确认同次扫描中的其它有效密钥不会进入 `known_hosts`。

## 2026-07-23 Codex 上游配额窗口

- 用户要求覆盖的不只是 5H，还包括 7D（周额度）、1M（月额度）和上游以后可能返回的其它窗口。
- sub2api 当前 OpenAI 配额查询调用 `https://chatgpt.com/backend-api/wham/usage`，窗口结构包含 `used_percent`、`limit_window_seconds`、`reset_after_seconds` 和 `reset_at`；`primary_window`、`secondary_window` 只是位置字段。
- sub2api 当前把窗口标准化成 `codex_5h_*` 和 `codex_7d_*`，并没有可靠证据说明 1M 在所有账号上都会返回，因此 Niffler 不能把“没有 1M”当成“没有月额度”。
- Aether 当前 Codex 解析器把付费账号的窗口位置重排后写成 `primary/weekly` 和 `secondary/5h`，只生成 `weekly`、`5h`、`spark_5h`、`spark_weekly` 四种代码。
- Aether 前端 `providerKeyQuota.ts`、ProviderDetailDrawer、管理号池 payload 和部分窗口统计/重置逻辑均固定筛选 `weekly/5h`，会漏掉 `7d`、`1m` 和未知时长窗口。
- 窗口用量统计按快照中的 `code` 关联数据库汇总；因此如果新快照把 7D 改成 `7d`，需要同时兼容旧汇总中的 `weekly`，否则历史用量会消失。
- 本次实现应保留上游实际窗口时长和重置数据；只能把标准时长转换成可读标签，不能通过固定窗口名伪造数据。
- 已实现通用 `windows[]` 作为当前配额事实来源：5H、7D、1M 和未知时长均保留真实秒数、重置信息和可读标签。
- 7D 窗口继续使用已有的 `weekly` 内部标识，界面显示 `7D`；`primary/secondary` 字段只保留读取兼容，当前展示、耗尽判断和窗口用量统计不再依赖固定两窗口。
- 账号级窗口参与普通 Codex 耗尽判断和本地用量统计；feature/model/workspace 窗口只展示独立上游额度，不串入普通请求用量。
- 上游本次没有返回的窗口会从当前快照移除，避免旧的 5H、周额度或月额度继续显示。
- 最终审查发现同步生图失败状态虽已从正文识别出来，但成功副作用仍读取修改前的 HTTP 状态；现已统一读取归一化后的状态。
- 最终审查发现同步生图收尾解析使用固定 LF 分隔，而同文件实时解析已支持 CRLF；现已复用同一分块规则。
- 最终审查发现 PostgreSQL 增量统计直接转换和相乘窗口数值，异常快照可能触发 `BIGINT` 错误；现已与重建统计保持相同的范围保护。

## 2026-07-15 本地未提交改动审查与上线

- 当前分支 `codex/fix-codex-lite-tools` 停在 `153046f0`，生产后续提交 `fcc5a165`（同步生图保活）和 `eb44f662`（sub2api OAuth 导入）与其构成线性提交链。
- 如果直接从当前分支构建并部署，会回退上述两个已上线功能；本次提交必须接到 `eb44f662` 之后再进入生产构建。
- 当前未提交业务代码主要实现 Codex 模型目录客户端版本自动更新；生图部分只有架构文档、验收文档和一次性验证脚本，没有新的生产生图代码。
- `planning-with-files` 恢复脚本返回的是旧会话描述，与当前 `git status` 不一致；本次审查只采用当前工作区、提交历史和生产状态作为依据。
- 审查发现上游返回 HTTP 200 和空模型数组时，`has_success=true` 会进入成功持久化并将 `allowed_models` 设为空；这与设计中“同步失败不清空权限”不一致，必须将空目录视为失败。
- 审查发现持久化的客户端版本状态无法解析时，版本刷新函数直接返回；损坏记录会使自动发现永久停止，应告警后使用进程内版本或内置已验证版本继续检查，并在成功后覆盖损坏记录。
- 手动刷新和后台同步已经通过 `ModelFetchTransportRuntime::resolve_codex_model_fetch_client_version` 使用同一有效版本，未发现两条路径版本不一致。

---

## 2026-07-14 sub2api JSON 导入兼容性

- 截图显示错误发生在点击“导入”后，提示为“无法解析输入内容，请检查格式”；需要继续确认是前端 `JSON.parse` 失败，还是业务格式适配失败被统一映射成该提示。
- 文件本身是有效 UTF-8 JSON，无 BOM，约 18 KB，不存在 JSON 语法或编码错误。
- 顶层为对象，字段为 `exported_at`、`proxies`、`accounts`；`accounts` 包含 12 条记录。
- 每条账号记录包含 `name`、`type`、`platform`、`priority`、`concurrency`、`credentials`、`extra` 等字段，凭证位于嵌套的 `credentials.access_token`。
- 前端错误文案来自 `OAuthAccountDialog.vue`，由 `parseImportText(inputText)` 返回空结果时统一显示，说明报错不等于 `JSON.parse` 语法失败。
- 前端 `isBatchImport` 只把顶层数组或多行输入识别为批量导入；任何可解析的顶层对象都强制走单条导入。
- 单条导入只读取顶层的 `access_token` / `refresh_token`，不会读取 `accounts[].credentials.access_token`，因此当前文件必然解析失败。
- 后端批量解析同样只支持顶层数组、单账号对象或逐行 Token/JSON；顶层 `{ accounts: [...] }` 会被当成单账号对象，解析为 0 条。
- 文件中的 12 条记录都有 `credentials.access_token`，并携带 `chatgpt_account_id`、`chatgpt_user_id`、`email`、`plan_type=team` 等可映射字段；现有 Niffler 批量导入结构本身可以承载这些数据，只缺少 sub2api 包装结构展开和嵌套字段映射。
- sub2api 将该结构定义为正式的 `AdminDataPayload`：顶层必须包含 `proxies` 与 `accounts`，账号凭证固定放在 `credentials` 对象中；不是临时或异常文件格式。
- sub2api 当前导入界面也以此结构做类型和版本校验，并支持多个此类文件合并，因此 Niffler 应按明确格式适配，而不是让用户手工改成数组。
- 现有 Niffler 后端已经支持仅有 Access Token 的 Codex 临时账号，并有相应测试；本文件没有 Refresh Token 不构成导入阻塞。
- 文件内 12 个 Access Token 都不是三段式 JWT，而是 `personalAccessToken` 模式；适配时不能依赖 JWT 解码补齐账号身份，必须读取 `credentials` 中的账号 ID、用户 ID、邮箱和套餐字段。
- Niffler 只要将嵌套令牌明确映射为 `access_token`，现有执行层就会按 Access Token 临时账号导入，不会误走 Refresh Token 交换。
- 推荐同时修复前后端：前端负责识别 sub2api 包装对象并走批量任务；后端负责展开 `accounts[]`、校验账号平台并做字段映射，避免只有网页入口可用。
- 本功能应限定为“授权凭证导入”：账号名、身份信息和套餐可以保留；sub2api 的代理、并发、优先级、倍率和自动暂停字段与 Niffler 语义不同，不应在这个入口静默套用。
- 无效或平台不匹配的账号应计入失败结果并显示原因，不能像当前 `filter_map` 一样静默消失；日志和错误样本不得包含令牌正文。

### 方案复核补充

- 前一版将 sub2api 外层 `name` 映射为 `account_name` 不够准确：Niffler 新建号池记录时优先使用邮箱命名，无法保留 sub2api 的 12 个自定义名称。应给批量导入条目增加独立的记录名称字段，并仅在创建新记录时使用；替换既有记录时保留原名称。
- 文件中 12 个账号名称、邮箱和用户 ID 均唯一；`chatgpt_account_id` 相同，符合 Team 工作区多个成员的结构。现有重复检测会组合账号 ID 与用户 ID，不会把这 12 条误判为同一个账号。
- 重复导入不会产生重复记录：活动中的同账号会返回“已存在”错误；失效、停用或过期记录才会被替换。该行为应在导入结果中明确显示，不应改成无条件覆盖。
- `access_token_import_temporary=true` 会阻止自动刷新；这些 Personal Access Token 没有过期字段，Niffler 会持续使用到上游拒绝，再由健康状态标记异常。这是凭证自身限制，需要在结果中提示“不可自动刷新”。
- `chatgpt_account_is_fedramp`、`openai_auth_mode`、`model_mapping` 和 WebSocket 模式目前没有对应的 Niffler 账号级语义，不应伪造映射。

---

## 2026-07-14 线上生图超时

- 截图中出现“已加载工具”和“生成服务超时”，说明不是 Codex App 未加载生图工具。
- 生产日志确认请求通过鉴权后进入 `/v1/images/generations`，路由到 `Pro号池` / `gpt-image-2`。
- 多个不同 Pro 账号连续出现超时，排除单个账号失效。
- Niffler 当前图片同步请求总超时为 900000ms（15 分钟）；客户端约 2 分钟后取消并重试，因此整个任务可持续十几分钟。
- 04:59 曾有同一图片链路在 77.2 秒返回 HTTP 200；随后多个图片请求开始长时间无响应。
- 数据库统计：近 6 小时该图片端点 1 次成功、30 次取消；30 次取消都在 124.2–124.7 秒，状态码 499。
- 其中大多数取消请求的上游首响应为 0.3–2.8 秒，已接收 8–9 个 SSE 事件，最后事件是 `keepalive`；上游一直在保持连接。
- Niffler 为了最终转换成 OpenAI Images JSON，把上游 SSE 全部缓存，没有把 `keepalive` 发给 Cloudflare/Codex App。
- Cloudflare 官方当前默认 Proxy Read Timeout 为 120 秒；线上请求经 Cloudflare 代理，实际 499 时间与该限制一致。
- 代码中已存在每 15 秒输出换行符的 JSON 空白心跳实现，但开关函数自 2026-05-10 的合并提交起固定为 `false`；线上记录也确认 `downstream_heartbeat_count=0`。
- 单纯打开旧心跳仍需要验证错误状态和账号切换：该包装层会先固定返回 HTTP 200，且内层返回 `Ok(None)` 时不能直接交还外层调度循环。
- 最终采用的是完整账号候选流程外层的保活包装，不是旧的单账号内层开关；因此保留失败后的账号切换能力。
- 每 15 秒写出的内容是 JSON 合法空白，最终响应仍可被标准 JSON 解析并用于 Codex App 图片预览。
- 生产冒烟请求的响应开头实际包含 2 个空白字节，随后是合法 JSON；图片 Base64 解码后的文件头为标准 PNG。

---

# 原任务：后台钱包检索、用户时间展示与前端表格体验优化

## 上线前复核

- 钱包管理需要按用户名检索的问题，代码层已经扩展为 `user_search` 查询参数。
- 后台接口已经覆盖：
  - 钱包列表 `/api/admin/wallets`
  - 资金流水 `/api/admin/wallets/ledger`
  - 退款审批 `/api/admin/wallets/refund-requests`
  - 充值订单 `/api/admin/payments/orders`
- Postgres 查询匹配用户名、邮箱、用户 ID；涉及独立密钥钱包时也匹配密钥名称和密钥 ID。
- SQLite、MySQL 测试仓库、内存仓库的查询结构已同步，避免不同环境字段不一致。
- 用户管理列表和详情的创建时间已经改为 `formatDateTime(user.created_at)`。
- 已新增设计记录 `docs/architecture/admin-wallet-user-search-and-created-time.md`。
- 新增 SQLite 回归测试会验证钱包列表、资金流水、退款审批、充值订单都能按 Alice / 邮箱搜索到。

## 测试环境问题

- Node 25 会提供实验性的 `globalThis.localStorage`，但未配置 `--localstorage-file` 时对象缺少 `getItem/clear/setItem/removeItem`。
- Vitest 在 jsdom 环境下没有覆盖这个不完整对象，导致依赖 localStorage 的测试失败。
- 修复点放在 `frontend/src/test/setup.ts`，只影响测试环境，不进入生产构建。

## UI Workflow

- 平台：Web / 浏览器页面。
- 界面类型：Niffler 后台管理页面。
- 主任务：管理员高效筛选、查看和操作后台数据。
- 视觉方向：沿用现有后台工具型设计，不引入新风格。
- 重做级别：medium，优先优化结构、表格和响应式，不重做业务功能。

## 待确认

- 本地工作区有大量前序任务改动，发布前需要确认哪些已经在生产，哪些需要随本次上线。
- 全局前端表格优化需要先找到现有表格组件和后台高频页面，避免逐页硬改。
# 2026-07-28 Telegram 监控通知与 Bot 设置

- 截图中的“磁盘 /”表示 Linux 根分区，不是多块不同磁盘；应统一显示为“系统盘”。
- “生产监控测试”是人工触发的完整检查摘要，“磁盘 / 预警”是状态机首次发现 83%
  超过 80% 后发送的真实告警；两者用途不同，但原文案没有解释清楚。
- 磁盘阈值和连续失败次数来自每台服务器的配置文件，当前分别为 80%、90% 和 3 次，
  不是固定写在检查逻辑中。
- Telegram Bot 当前没有命令接收器，Webhook 为空，可以使用 `getUpdates`。
- Bot 更新必须持久化最高 update ID 加一，否则重启后会重复处理旧命令。
- 两台服务器不能同时调用 `getUpdates`；命令接收器应保持单实例。
- `rn01` 当前没有安全访问 `hd0526` 的专用身份，需要新增强制命令 SSH 密钥，不能
  复用已有隧道密钥，也不能分发 root 私钥。

---

# 2026-08-03 Codex Responses 历史推理项 ID 兼容修复

- 线上请求 `c2c7723c-b855-4184-9a6c-1acc3e8597fd` 在 `input[67].id` 使用
  `item_7bda3e00a09d6e4dc9d0abef`，上游要求 `reasoning` 项 ID 以 `rs_` 开头。
- 三个 Pro 账号返回同一 HTTP 400，说明错误由请求内容决定，不是单个账号、网络或额度问题。
- 线上请求正文采集未开启，无法从后台还原 `input[67]` 的完整字段；修复测试使用错误消息中已确认的最小形态。
- 官方 OpenAI 文档规定历史推理项使用 `rs_*` ID，并建议原样回放带 `encrypted_content` 的推理项。
- 官方 Codex 源码的 `ResponseItem::id_prefix()` 将 `Reasoning` 映射为 `rs`，缺失 ID 时也使用该前缀生成 ID；反序列化为兼容旧记录而保持宽松。
- Aether 当前 Codex Responses 请求转换会处理字符串输入和图片历史项，但不会校正推理项 ID。
- 当前执行策略能阻止 SSE `response.failed` 中的 `invalid_request_error` 切换账号，但直接 HTTP 400 仍走通用账号切换逻辑。
- 兼容规则应只处理 `type = reasoning` 且 ID 为非空 `item_*` 的历史项，保留后缀并替换为 `rs_*`；正常 `rs_*` 和其他项不变。
- 直接 HTTP 400 的停止规则限定为 Responses 请求和结构化 `invalid_request_error`；临时容量错误优先按原规则切换账号，普通 Chat Completions 行为不变。

---
# 2026-08-10 提交构建前最终复审

- 已确认阻断问题仍存在：PostgreSQL、MySQL、SQLite 结算在找不到 `billing_request_admissions` 请求扣费决定记录时，仍通过现有钱包行推断允许透支，并继续按旧规则选择钱包或套餐。停机切换方案已经要求停机前清空待结算记录、启动后所有付费请求必须先写请求扣费决定记录，因此新版本不能保留这一永久回退。
- SQLite 测试 `sqlite_repository_overdraws_finite_wallet_and_settles_usage` 没有创建请求扣费决定记录，却明确断言有限钱包从 12 美元扣到 -3 美元；该测试当前保护的是待删除的旧行为。
- 当前工作区同时包含计费根治和美西双入口/首页延迟显示两项任务，共 88 个修改或未跟踪文件。即使最终复审通过，提交时也必须按任务明确选择文件，不能把无关生产 Caddy、首页测速和运维文档混入计费提交。
- 已确认第二项阻断问题：请求调用上游前新增了顺序数据库往返。运行时先单独读取请求扣费决定记录，再解析资金来源，随后开启事务插入决定记录、写请求记录并再次查询决定记录。PostgreSQL 的套餐额度查询还会按每个权益和每个额度窗口逐条顺序查询。代码没有达到设计文档“不新增顺序跨机房数据库往返”的性能要求。
- 当前没有找到生产 PostgreSQL 对新扣费路径的行为测试；现有 PostgreSQL 工作流只覆盖迁移、兼容检查和导入导出。新请求扣费决定、套餐加钱包补差、供应商范围、重复结算和并发主要由 SQLite 测试覆盖，且其中仍包含保护错误旧行为的测试。
- 钱包余额大于 0、等于 0、小于 0，有无适用套餐，以及负钱包禁止购买/续费和单生效套餐等主要业务判断已经接入；本轮在这些入口暂未发现新的阻断问题。
- 发现一项记录准确性问题：同一请求 ID 已有请求扣费决定记录时，一致性校验比较了用户、钱包、资金来源、套餐和允许供应商，却漏掉 `selected_provider_id`。不同实际供应商的并发写入可能被当成同一决定接受，数据库留下的“选中供应商”不一定准确；当前结算另读实际使用供应商，因此更偏向审计记录错误，而不是已确认的重复扣费。
- 套餐与钱包联合结算的核心算术符合已确认规则：结算事务锁定套餐权益和钱包；套餐每个窗口最多扣到 0，剩余基础费用按销售倍率扣钱包；套餐请求允许钱包补差后变负；重复结算先锁使用记录并读取已完成状态。
- “新请求缺少请求扣费决定记录”的生产处理仍未实现完整：当前只有永久旧规则回退。若直接改成返回错误，后台直写路径只记录日志，使用记录仍可能停留在待结算；现有超时清理还可能把它作废。提交前必须明确实现保留费用、重试和可追踪处理，不能只删除扣款代码。
- 请求扣费决定表每个请求新增一行，但当前没有归档或清理任务；现有请求记录清理只处理 `request_candidates`。这不会立即造成错扣，但长期会导致表和索引持续增长，不符合“完整根治”所需的运维闭环。
- 用户钱包中心在 `walletOnlyBalance < 0` 时无条件显示 `wallet.inDebtPlanStillUsable`，没有同时检查是否存在仍有余额的适用套餐。无套餐或套餐耗尽用户也会看到“套餐仍可使用”，属于明确的错误提示。
- PostgreSQL 迁移只创建套餐供应商和已购权益供应商关系表，不会给现有套餐或现有权益自动写供应商。设计文档明确要求管理员确认后批量生成关系；当前提交范围尚未看到对应的生产迁移/补录脚本。若只部署代码，受事故影响的旧权益仍按旧静态模型范围运行，`gpt-5.6-sol` 不会自动恢复。
- 套餐管理中英文说明仍写着“不同套餐可以并存，按各自可用模型范围生效”，与用户已经确认的“同一时间只能有一个套餐”直接冲突。后端虽然增加了重叠检查，但管理端会向管理员展示错误规则。
- 套餐定义页面已经改成直接选择供应商，并只展示这些供应商动态提供的模型；这一部分符合本次需求，不再按静态模型列表保存新套餐。
- 现有用户套餐的后台编辑接口虽然底层数据结构支持 `allowed_provider_ids`，但 HTTP 请求体没有这个字段，更新时还固定传入 `None`；管理页面也没有批量修改现有用户套餐供应商范围的入口。因此“由管理员确认后补录旧套餐供应商关系”目前只有文档要求，没有可执行的生产操作。
- PostgreSQL 套餐到账时会先锁定该用户的钱包行，再检查其他套餐时间是否重叠，因此同一用户的并发到账会按顺序处理；这一实现能保证“同一时间只能有一个套餐”，暂未发现需要额外数据库限制才能成立的问题。
- 已实际运行错误旧行为的 SQLite 测试；测试通过，证明当前代码确实会在没有请求扣费决定记录时把有限钱包从正数扣成负数，而不是仅凭静态阅读推测。
- `cargo fmt --all -- --check` 通过，代码格式没有阻断问题。
- 已取得性能问题的直接证据：PostgreSQL 先查套餐，再对每个套餐查一次 5 小时窗口，并对每个额度窗口逐条查使用量；请求写入前还单独读取请求扣费决定记录，写事务内部插入后又查询一次。设计文档明确规定不能增加这种顺序跨机房查询。
- PostgreSQL CI 当前只运行迁移、兼容性、导入导出检查，没有执行新扣费行为测试；生产数据库上的金额拆分、重复结算和并发结算仍无自动验证。
- 最终工作区检查仍为 88 个修改或未跟踪文件，且包含两个不同任务；`git diff --check` 通过。由于存在上述生产阻断问题，本轮未创建提交、未启动构建。

## 2026-08-10 最终复审问题修复

- 正常请求的报告数据已经包含规划阶段生成的请求扣费决定；请求写入前再次读取数据库属于重复查询，可以删除。
- 结算返回错误时事务回滚，终态使用记录仍保存实际费用和 `pending` 结算状态；旧的超时请求清理只选择请求状态为 pending/streaming 的记录，因此不会作废已经 completed/failed/cancelled 但仍待结算的费用。
- 三种数据库的结算旧回退已删除：只允许使用请求开始时保存的钱包 ID、资金来源、套餐权益和供应商范围，不能再按当前用户或 API Key 查找钱包后自行决定扣款。
- 请求写入前的额外读取已删除；PostgreSQL 决定记录的插入与冲突返回合并为一条 SQL，减少两次顺序数据库查询。
- PostgreSQL 结算测试已扩展为同时覆盖：缺少请求扣费决定时不扣款、并发重复结算只扣一次、套餐额度用尽后钱包承担差额。
- 旧架构文档仍有“不同套餐可以并存”的历史说明，已统一为“同一用户同一时间只能有一个有效套餐；同套餐续费顺延”。
- 首次运行缺失决定记录的 SQLite 测试时，失败来自测试把充值余额 10 美元误写为总余额 12 美元；代码确实没有扣款。测试已分别断言充值余额 10、赠款 2，并为正常扣款用例补上请求开始时保存的扣费决定。
- PostgreSQL 静态回归测试原先会在读取自身源码时命中断言字符串，属于测试写法错误；已改为正向确认结算只使用请求输入的独立密钥属性和已保存的透支许可。
- 结算模块全量测试发现 6 个旧套餐测试没有建立请求扣费决定。修复方式是让测试明确保存实际资金来源、套餐权益和实际供应商；不恢复任何生产回退逻辑。套餐外模型测试同时改为正余额钱包获准后扣钱包，符合已确认规则。
- 补齐真实请求条件后，结算模块 29 项测试全部通过；缺失决定记录、钱包最后一批透支、套餐额度与钱包补差、供应商不一致和重复结算均有覆盖。
- 直接写使用记录的路径在结算失败后只记一次日志，数据库虽会保留实际费用和待结算状态，但没有后续自动重试。现有五分钟维护任务适合增加一批“已结束且待结算”的轻量查询，并复用同一结算事务重试；多实例同时执行也由现有行锁和幂等判断保护。
- 已为使用记录读取接口增加专用的“已结束且待结算”批量读取；PostgreSQL 直接按最终结算状态筛选，SQLite 和内存实现遵循相同条件，MySQL 复用其现有内存读取层。
- 五分钟维护任务现在会在清理超时请求前重试已结束但待结算的费用；单条失败只记录对应请求并继续处理其他记录，金额和待结算状态保持不变。
- PostgreSQL、MySQL、SQLite 都只清理已经结算并且没有服务尝试记录的扣费决定；SQLite 实际数据库测试确认待结算费用对应的记录不会删除。
- 真实 PostgreSQL 并发测试发现：第二个结算事务在等待使用记录行锁时，SQL 语句已经取得旧的数据视图；锁释放后虽然能看到“已结算”状态，却看不到同一事务刚写入的结算明细，因此返回的钱包余额为空。钱包只扣了一次，金额正确，但返回结果不完整。修复为检测到已结算后再执行一次无锁读取，取得刚提交的完整结算明细。
- PostgreSQL 测试进入套餐拆分场景后，测试生成的订单 ID 超过表中 36 字符限制；已缩短测试 ID，这不是生产逻辑错误。
- 修复并发读取后，真实 PostgreSQL 完整测试通过：缺扣费决定的钱包保持不变；同一请求并发结算钱包只扣一次；2 美元套餐额度用完后钱包承担 6 美元；优化后的套餐额度批量查询能正确返回结算前 2 美元和结算后 0 美元。
- 旧套餐供应商范围不是只写了文档：后台修改接口会验证供应商存在且启用，测试确认更新后返回保存的供应商 ID。
- 路由复查发现两个关联问题：套餐额度查询没有传当前全局模型，正余额用户在套餐供应商失败后仍可切到套餐外供应商；更严重的是，旧套餐没有供应商关系时会返回空扣费决定，导致调用上游后无法结算。正确做法是按当前模型查询套餐；新套餐覆盖当前模型时只走套餐供应商，套餐外模型由正余额钱包支付；旧套餐按旧模型范围判定后，将本次实际供应商写入扣费决定。
- 当前套餐是否适用于请求，不能根据这一页服务列表里是否碰巧出现套餐供应商判断。服务列表可能分批读取；只要按当前全局模型查询出的有效套餐带有供应商范围，该请求就必须限制在这些供应商内，当前批次没有匹配项时应继续读取下一批，不能转为钱包支付。
- 分批请求执行器会在当前批次过滤为空后自动继续读取下一批，因此按模型锁定套餐供应商不会提前结束正常请求，也不会增加新的数据库查询；每一批仍会重复读取一次钱包和套餐状态，这是原有分页调用结构，后续性能检查需确认是否要缓存为单次请求快照。
- 分批读取的资金查询已改成按全局模型保存在当前请求内；同一模型后续批次直接复用。不同全局模型才重新读取，避免错误复用其他模型的套餐范围。
- 旧扣费回退、旧透支测试名、旧“多套餐并存”文案和旧分页套餐判断的全仓搜索均无残留；`git diff --check` 通过。
- 数据层全量 525 项测试通过后继续检查发现：请求付款决定仍保存单个 `selected_provider_id` 并参与不可变记录冲突比较，一次请求切换供应商重试时会与首次记录冲突。付款决定应只保存稳定的资金来源和允许供应商范围，实际供应商由每次服务尝试和最终使用记录保存。
- 套餐结算的供应商校验仍允许在实际供应商超出套餐范围时，只要请求开始时钱包为正就改扣钱包。这会掩盖路由错误，并违反“当前模型有适用套餐时只在套餐供应商内重试”；应直接拒绝结算、保留待处理费用并报警。
- 请求付款决定已调整为跨重试稳定：新记录不再固定首个供应商；套餐只保存权益对应的完整供应商范围。旧套餐使用空供应商范围明确表示“模型已经校验，可在该模型的实际供应商间重试”。
- 套餐结算和请求记录写入现在都按权益供应商范围验证实际供应商；钱包在请求开始时为正也不能让套餐请求越界后改扣钱包。明确请求套餐外模型时会从一开始保存为钱包支付，不受此限制。
- 新增迁移尚未上线，因此已直接删除请求付款决定中的“选中供应商”列及索引，避免保存第一个供应商造成误导。最终实际供应商继续使用现有使用记录字段，允许供应商集合继续保存在请求付款决定中，两者职责分开。
- 一次批量删除字段时误删了现有使用记录测试里的同名“最终供应商”字段；该字段属于另一张表且必须保留，已立即恢复。首次复跑失败来自这次测试编辑错误，不是生产逻辑失败。
- 内存结算实现仍按 API Key 或用户临时查找钱包，并有测试保护该旧行为。虽然该实现只用于测试，继续保留会让测试环境掩盖生产同类问题。现已要求内存结算也必须取得请求开始时保存的钱包 ID；缺记录或钱包不一致直接报错，不再猜测。
- 内存结算没有套餐额度账本，不能假装正确执行套餐拆分；遇到套餐付款决定时明确返回不支持。套餐拆分行为继续由 SQLite 和真实 PostgreSQL 测试覆盖。
- 删除单个供应商列后的全新 PostgreSQL 数据库测试通过，证明新迁移无需依赖旧表残留；测试继续覆盖缺付款决定不扣款、并发重复结算、套餐与钱包补差和额度批量查询。独立测试数据库随后已删除。
- 管理端用户列表现在直接使用用户列表接口返回的钱包数据，展示充值余额与赠款余额之和的真实值；负数不再取零，也不再误显示“套餐加钱包”的合计值。
- 钱包详情接口同时返回真实钱包余额、可消费钱包余额和欠费金额：真实余额保留负数，可消费钱包余额最低为零；总可用额度只将非负钱包余额与套餐余额相加，避免负债抵消仍可用的套餐额度。
- 用户钱包中心只有在套餐仍有效且套餐余额大于零时才提示“套餐仍可使用”；无套餐或套餐已用完时明确提示新付费请求不可用。
- 套餐购买限制由服务端数据库事务执行，不依赖前端按钮：欠费钱包不能创建用户支付订单；已有其他生效套餐或其他套餐待支付订单时不能再买；同套餐续费仍可顺延；管理员赠送不受欠费限制。
- 支付发放时会再次检查欠费和套餐重叠，符合设计记录中“订单创建后状态变化必须进入人工处理”的规则，避免只靠下单时的一次检查。
- 三种数据库在结算套餐请求时都先校验实际供应商；越界会回滚，不会执行后面的钱包扣款。复查发现校验后的不可达代码仍保留“越界时改成钱包资金来源”的旧表达，虽不构成当前运行时错误，仍已删除以免以后重新引入旧行为。
- 历史 Niffler 金额预占表和兼容接口仍保留，但运行时会强制将预占开关设为关闭；即使旧配置或按 API Key 灰度配置写着开启，也不会创建新预占。现有测试覆盖旧配置无法重新开启预占。
- 删除不可达旧结算分支后，`aether-data` 全量 527 项测试全部通过，覆盖迁移、钱包、套餐购买、供应商范围、请求记录、结算和待结算重试。
- 网关全量 2,983 项测试全部通过；计费路由、鉴权、Codex/Claude/Gemini、图片、视频、流式与非流式请求及钱包结算均未出现回归。
- 前端类型检查和全量 572 项测试全部通过；负余额界面、套餐供应商选择和现有套餐编辑测试通过。测试输出只有原有浏览器数据过期提醒及测试预期日志，不影响结果。
- 前端生产构建通过；只出现已有的大文件分块提醒，不影响构建产物。
- 全新本地 PostgreSQL 数据库再次通过真实迁移、缺付款决定不扣款、并发重复结算和套餐钱包补差测试；命令结束后已自动删除临时数据库。
- 严格静态检查首次只发现三个测试使用了不必要的动态数组，已改为固定数组后等待复跑；没有发现生产代码告警。
- 修正三个测试数组后，计费相关四个 Rust 包的全部目标通过严格 Clippy，警告按错误处理；本地 PostgreSQL 临时数据库计数确认是 0，清理完成。
- 新增 PostgreSQL、MySQL、SQLite 迁移的主键和外键长度与现有套餐、权益、供应商、钱包、用户及 API Key 表一致；全新 PostgreSQL 迁移已实际执行通过。
- CI 已增加真实 PostgreSQL 计费行为测试，不再只验证迁移是否能运行；测试使用现有隔离测试库，并由测试自身清理本次数据。
- 中英文基础语言文件同时包含另一项首页线路测速改动，不能整文件纳入计费提交。计费新增文案已集中补入现有语言合并层，因此计费提交可以排除两个混合语言文件，仍保留完整中英文显示。
- 计费修复已提交为 `ae5b8d6e5`，提交包含 93 个计费相关文件；部署配置、线路测速、任务记录和混合语言文件均未包含。
- 提交内容的独立前端生产构建通过；后端 `aether-gateway` 发布构建通过，首次完整发布编译耗时约 4 分钟。
- 2026-08-10 新需求替换“欠费禁止购买套餐”：欠费用户可以下单，订单实付金额必须为套餐价格加下单时欠款，并明确展示金额组成。
- 本轮界面属于现有 Web 控制台购买流程的小范围改造；保留现有视觉和页面结构，重点是金额可见性、支付确认和错误状态，不做视觉重构。
- `planning-with-files` 会话恢复脚本没有报告未同步内容；工作区仍有线路测速和计划文件等既有改动，后续只修改本任务文件，不混入其他任务。
- 界面改造定位为现有 Web 控制台付款流程调整：保留套餐卡片和付款渠道，只重排付款确认区的信息优先级。
- 付款确认区优先显示实付总额，再用一行金额明细解释“套餐价格 + 欠款金额”；同一信息不在标题、说明和按钮中重复堆叠。
- 购买操作必须覆盖加载、重复点击、不可用和失败状态，沿用现有设计令牌，不新增独立视觉风格或多层卡片。
- 付款弹窗最多保留一层核心容器；金额数字是视觉重点，币种和说明降级，不能用多层摘要卡解释三项金额。
- 只有欠费用户需要显示欠款行和“付款后结清下单时欠款”的短说明；无欠费用户继续看到正常套餐价格，避免所有用户承担额外信息负担。
- Web 页面改造继续使用真实购买流程的信息顺序，不平均分割成三张金额卡；付款弹窗只保留金额结果、必要说明和唯一确认动作。
- 文案直接写“套餐价格”“钱包欠款”“本次实付”和付款后的结果，不解释系统内部处理；确认按钮在创建订单期间必须禁用并显示处理中，失败后保留金额供用户重试。
- 现有架构文档明确写了“欠费不能创建订单、到账时再次拒绝、购买按钮不可用”，本轮必须先整体替换这些规则，不能只放开前端按钮。
- 数据层已有 `CreatePlanPurchaseOrderOutcome::WalletInDebt`，PostgreSQL、MySQL、SQLite 和网关都存在相应分支；订单当前广泛使用单个 `amount_usd`，需要继续确认它代表套餐价还是支付总额及退款基数。
- 套餐订单和充值订单共用订单记录，已有 `order_kind` 与 `product_snapshot`；用户套餐下单有 DodoPay 和另一条付款渠道两条入口，必须统一计算总额，避免只修一条渠道。
- 数据库迁移实际位于 `crates/aether-data/migrations/{postgres,mysql,sqlite}`；本轮需要为三种数据库保持相同订单金额语义。
- 现有套餐订单创建参数只有 `amount_usd`、支付币种金额、汇率和套餐信息；表中没有单独的套餐价格或欠款金额字段。当前 `amount_usd` 由套餐价格换算得出，并作为订单美元金额。
- 两条付款渠道当前都先按套餐价算支付金额，再创建本地订单；DodoPay 先创建本地订单后调用支付平台，ePay 先生成签名链接后写本地订单。合并付款必须在生成任一支付请求前由同一个数据层步骤确定欠款，防止页面金额、订单金额和实际收款不一致。
- 原始迁移只给订单增加套餐类型和套餐内容副本，没有为付款组成预留字段；后续需确认新增明确列是否比塞进套餐内容副本更适合退款、后台查询和审计。
- 订单接口当前只返回总美元金额和支付币种金额，没有金额组成；要做到前后端和后台都可核对，套餐价格与欠款金额应成为订单明确字段，`amount_usd` 继续表示本次实际收款总额。
- 付款链接当前使用创建订单前计算的局部 `pay_amount`。欠款必须在数据库事务中锁定钱包后确定，因此两条渠道都应改为先创建本地订单，再使用订单返回的实付金额生成付款请求，避免并发结算时收款金额与订单欠款不一致。
- 支付回调会把支付币种金额换算成美元金额，并与订单 `amount_usd` 校验；因此将 `amount_usd` 定义为套餐价加欠款后的实付总额，现有回调校验仍可直接保护少付或金额不一致。
- 套餐订单到账后当前明确把可退款金额设为 0；本轮不改变套餐退款政策，但欠款偿还仍需写钱包金额变化记录，便于管理员解释余额从负数恢复的过程。
- 数据层同时存在支付平台回调到账和管理员手动到账两条套餐发放路径，两条路径都必须使用订单保存的欠款金额并保持重复处理只生效一次。
- SQLite 代表实现确认：创建订单会在同一事务读取钱包余额并执行单套餐限制；当前欠费分支直接返回拒绝。删除拒绝后，可在这里记录 `max(0, -真实钱包余额)`，保证订单金额按创建时状态确定。
- 自动到账在订单已经到账时直接返回，不重复发放；手动到账也先检查订单状态和已有套餐权益。因此将欠款偿还放在“尚未发放权益”的同一分支内，可与套餐发放共用现有幂等保护。
- 当前自动到账和手动到账都会在发放前重新拒绝负钱包。新规则应删除这一拒绝，改为在同一事务按订单保存的欠款金额增加充值余额并写钱包记录，然后发放套餐；付款期间新增欠款不属于本订单，仍保留为负数。
- 钱包扣款在可用余额耗尽后会继续减少充值余额，因此实际欠款落在充值余额；套餐订单偿还欠款时应增加充值余额，并增加累计充值，钱包总额会按订单欠款上升。
- PostgreSQL 创建套餐订单已经使用 `FOR UPDATE` 锁定钱包行，可原子记录下单时欠款；MySQL 和 SQLite 需要保持同一业务结果。管理员赠送不能附带欠款付款，欠款金额应固定为 0。
- 最小且可审计的数据结构是新增 `debt_repayment_usd`：订单 `amount_usd` 改为实际收款总额，套餐价格可由两者相减；历史套餐订单欠款默认为 0，原金额语义自然兼容。
- 购买页当前明确禁用欠费用户，点击普通套餐后会创建订单并立刻打开支付页面，没有确认金额的停留步骤。新规则不能只改按钮；欠费订单创建成功后应先显示准确金额组成，用户再次点击“前往付款”才打开支付页面。
- 页面已经同时读取钱包欠款和套餐价格，可在下单前提示“购买时会一并结清欠款”；准确实付金额必须使用服务端订单返回值，不能只用页面缓存余额自行相加。
- 现有“最新订单”区域可以承载一层紧凑金额明细，无需新增弹窗或多层卡片：套餐价格、钱包欠款、本次实付，下面保留唯一付款按钮和取消按钮。
- 前端公共 `PaymentOrder` 目前没有欠款字段，需要新增 `debt_repayment_usd`，并由套餐接口返回派生的 `plan_amount_usd` 方便直接显示。
- 用户套餐页没有现成测试；本轮应新增针对欠费购买的组件测试，至少证明按钮可点击、下单后不自动跳转、金额三项明显显示、用户点击后才提交付款。
- 基础中英文语言文件仍有另一个任务的未提交改动；本轮继续只在已经用于计费覆盖的 `frontend/src/i18n/index.ts` 修改新增文案，避免混入无关文件。
- 设计已确定：订单总额保存套餐价格加下单时欠款，新增欠款金额字段；到账只偿还订单记录金额。付款过程中产生的新欠款不改变已生成的支付订单，仍会在钱包继续显示。
- 欠费订单需要多一次用户确认，普通无欠费订单保留自动打开支付页；这只改变购买体验，不影响任何模型请求响应时间。
- 内存测试现有用例明确断言欠费用户下单被拒绝，可直接改为先验证新规则失败：欠费 3 美元、套餐 1 美元、汇率 7.2 时，订单总额应为 4 美元、支付金额应为 28.80 元；管理员赠送仍为 0 欠款还款。
- SQLite 套餐到账测试已覆盖不可退款和权益幂等，适合扩展为“下单记录欠款、到账钱包归零、重复到账不重复增加余额”的真实数据库回归测试。
- 现有 SQLite “到账时再次检查负钱包”测试正好保护了待删除的旧行为。它应改为两种新边界：下单后新增欠款仍按已付款订单发放且不偿还新增欠款；下单时已有欠款则合并收款、到账归还并保证重复到账不重复还款。
- 支付回调的测试替代存储只覆盖普通充值订单，不适合证明套餐钱包还款；金额与到账正确性应由 SQLite 和真实 PostgreSQL 数据层测试承担，网关只测试订单实付金额被传给支付平台。
- 两条新行为测试已在旧代码上按预期失败：内存实现返回 `WalletInDebt`，SQLite 也在创建套餐订单时返回 `WalletInDebt`。失败点与旧规则完全一致，不是测试环境或数据问题。
- 前端测试项目主要使用 Vue `createApp` 挂载真实组件并替换 API 模块，没有统一的套餐页测试工具；新增测试将沿用该项目现有挂载方式，避免引入新测试依赖。
- 现有视图测试会替换 API、提示消息和图标后挂载组件，再等待异步加载完成；套餐页测试可以按同一方式触发真实按钮点击并检查是否调用支付表单。
- 测试用 `createApp` 已自动安装项目国际化；套餐测试只需用轻量组件替换布局、卡片、选择框和按钮，保留按钮禁用与点击事件即可验证真实业务逻辑。
- 前端测试首次运行暴露组件替换工厂的变量提升问题，改成工厂内定义后测试已正常挂载；随后旧页面显示“请先结清钱包欠费”而不是购买按钮，按预期证明旧界面阻止欠费购买。
- 前端红灯已稳定复现：旧页面按钮文案为“请先结清钱包欠费”且不可购买，测试失败原因正是待修改的产品行为。
- 订单数据结构只有两个公共记录类型，三种数据库各有统一行映射函数；新增欠款字段会集中修改这些映射和订单查询，不需要改动钱包余额、退款请求或套餐权益的数据结构。
- PostgreSQL 的订单查询使用统一返回列常量，MySQL 与 SQLite 使用重复列清单；实现时必须全仓检查每个 `payment_orders` 查询都包含欠款字段，否则行映射会在运行时缺列。
- 三种数据库的套餐下单结构相同：先读取并锁定钱包、检查套餐冲突，再插入订单。公共金额函数可以直接接入这三个位置，保持汇率取整和欠款计算一致。
- 首次批量补订单映射时，通用上下文误命中了退款申请映射；已立即删除错误字段并改用函数级上下文补到正确的订单映射。后续同名金额字段修改必须带结构体或函数上下文。
- 第一次编译发现同类上下文问题也误改了内存充值订单的局部变量，套餐订单反而仍是单变量；已按完整函数名修正，并为普通充值和兑换订单明确写入 0 欠款。
- PostgreSQL 和 MySQL 自动到账、手动到账都在发放权益前锁定同一用户的钱包并完成套餐冲突检查；欠款偿还应放在所有业务检查通过之后、插入权益之前，任何后续失败都会随事务一起回滚。
- 网关 DodoPay 已经是“先建本地订单、再向支付平台下单”，只需改用本地订单返回的总额。ePay 的付款内容是本地生成的签名参数，不会提前调用外部平台，可安全改为先建订单、再使用订单总额生成参数，无需增加数据库访问。
- 三种数据库到账已改为先按订单欠款写充值余额和资金记录，再插入权益；非测试代码编译通过。测试编译只剩内存测试样本缺少新字段，已补 0，不涉及生产逻辑。
- SQLite 行为测试第一次运行发现批量补查询列时，普通充值和兑换订单的插入列也新增了欠款字段，但值列表未补 0；已为 MySQL、SQLite 两类非套餐订单明确写入 0，修正列值数量。
- SQLite 真实数据库测试现已通过：欠费 3 美元创建 4 美元套餐订单；下单后新增 1 美元欠款；到账偿还订单记录的 3 美元、发放套餐，余额保留 -1；重复到账不再次还款。
- 内存业务语义测试通过：欠费 3 美元购买 1 美元套餐生成 4 美元订单和 28.80 元支付金额，管理员赠送不附加欠款。
- 更正前面的 ePay 判断：生成付款参数不需要调用外部平台，但仍应将签名后的准确付款内容写回订单，便于审计和后续读取；因此套餐购买会多一次短数据库更新，模型请求链路不受影响。
- 网关已统一使用数据库订单返回的实付金额：DodoPay 用该金额向支付平台下单，ePay 用该金额签名并将付款内容写回订单；数据层拒绝无效或溢出的最终金额。
- 真实 PostgreSQL 事务测试通过：下单时欠费 3 美元、套餐 1 美元生成 4 美元订单；下单后新增 1 美元欠款，到账只偿还原 3 美元并发放一次套餐，重复到账不重复增加余额。独立测试数据库已删除。
- 欠费购买页面组件测试两项通过：欠费订单先展示三项金额并等待“前往付款”；无欠费订单仍自动打开支付页面。
- 前端全量类型检查当前被仓库其他文件的 14 处既有错误阻断，本次修改文件没有出现在错误列表中。
- 订单金额由锁定钱包的下单事务计算并保存，支付平台、回调验款和页面展示全部读取同一订单金额，避免余额变化造成展示金额与签名金额不一致。
- `payment_orders.amount_usd` 现在表示套餐订单实付美元总额，新增 `debt_repayment_usd` 单独保存其中用于还款的金额；历史订单和非套餐订单默认欠款金额为 0。
- 支付到账先按订单记录增加充值余额并写资金记录，再发放套餐；两步位于同一事务，任何一步失败都会整体回滚。订单状态和权益记录共同保证重复回调、重复手动到账不会再次还款。
- 下单后新增的欠款不会修改已经签名的订单金额。订单只偿还下单时记录的欠款，新增欠款继续显示；用户已经足额支付的套餐仍正常开通。
- DodoPay 使用数据库订单返回的实付金额创建支付订单；ePay 使用同一金额生成签名付款内容，并短暂更新本地订单保存付款内容。新增数据库更新只发生在购买套餐时，不进入模型请求路径。
- 欠费用户下单后，页面先展示“套餐价格、钱包欠款、本次实付”和结清说明，用户点击“前往付款”后才打开付款页；无欠费订单保持原有自动付款流程。
- 最终复核通过：数据层全量 529 项、网关全量 2,983 项、前端全量 574 项、前端生产构建、目标 ESLint、Rust 格式和严格 Clippy 均通过；真实 PostgreSQL 事务测试也已在独立临时库执行并清理。
- UI 运行时复核得到 `PASS ui-review gate`。界面只增加下单后的金额确认，不增加动画、循环测量或模型请求路径工作。
- 前端全量类型检查仍被 14 处本次范围外的既有错误阻断；本次修改文件未出现在错误列表，不能将这一结果写成全量类型检查通过。
- 当前计费分支 `99615afa1` 已推送并完成独立 Linux amd64 镜像构建，但尚未进入 `main`。
- 远端 `main` 为 `ed5e9016e`，包含计费分支没有的两笔提示词提交；远端 `test` 为 `378180f1d`，也有独立更新，二者都不是计费分支的祖先，不能直接快进。
- `main` 规则要求合并请求、1 个批准以及 `Frontend`、`Release tooling`、`check` 三项状态；仓库晋级检查要求正式来源为 `test`。`test` 同样要求合并请求和三项状态，但无需人工批准。
- 当前账号可绕过 `main` 规则，但直接绕过会破坏仓库已建立的 test 验收与生产晋级链，不采用该方式。
- test 合并请求 #34 的首次前端失败来自新依赖安全公告，不是计费或合并冲突：DOMPurify 3.4.12、nanoid 3.3.16 和 5.1.5 被判定有风险。
- 现有依赖范围允许只更新锁文件到 DOMPurify 3.4.13、nanoid 3.3.18 和 5.1.16；更新后 `npm audit --omit=dev` 报告 0 漏洞，`package.json` 无需修改。
- 2026-08-12 全量分叉审计确认：Niffler `origin/main` 与 Aether `upstream/main` 的共同祖先是 `ed75ae6d56ab03eb5e6e3cd87f2137880c99694d`（2026-05-20 01:06:58 +0800）。
- 共同祖先之后，Aether 有 776 个独有提交，Niffler 已发布主线有 357 个独有提交；两者不是简单版本落后关系。
- Aether 一侧从共同祖先到固定主线按禁用重命名推断的完整口径修改 2,676 条路径，新增约 676,034 行、删除约 323,095 行；较小的默认 Git 统计会受目录迁移和重命名配对影响，不用于覆盖结论。
- Niffler 当前工作区位于未进入主线的计费分支且存在其他未提交改动；主比较对象固定为 `origin/main`，当前分支和工作区另做附录，避免混淆已发布状态。
- 上游 `ab0a90de9` 将 Redis 运行时改为固定长连接通道；Niffler 固定主线在 4 个旧运行时文件中仍有 31 处 `get_multiplexed_async_connection()` 调用，且缺少当时引入的 `crates/aether-runtime-state/src/redis/runtime.rs`。Aether 最终实现经分层后位于 `crates/aether-runtime/state/src/redis/`，说明该能力必须连同连接路由和调用方做语义移植，不能只复制单文件。
- 上游 `a04673a90` 新增 `end_to_end_first_byte_time_ms` 和 `end_to_end_time_ms`，明确把选号、等待、失败尝试和换号计入端到端指标；Niffler 当前没有这两个字段，只保留成功尝试的 `first_byte_time_ms`。
- 路径清单交叉校验结果：Niffler 改动 1333 条路径，Aether 改动 2676 条，交集 699，并集 3310，最终树不同 3309；唯一一致项是双方删除同一 Provider/Codex 窗口统计 SQL 文件。
- 双方有 698 条同路径并行演进且最终分叉，主要集中于网关 277、数据层 119、前端 features 78、前端 views 54；核心层不能按目录整体替换。
- `git cherry` 双向检查没有补丁等价提交。相同产品能力必须按实际行为比较，不能仅凭提交标题或功能名判断已吸收。
- Aether 的大规模 crate 分层会让 Git 把相似但不同语义的文件误识别为重命名；审计完整性固定使用禁用重命名的路径清单。
## 2026-08-12：Niffler Core 最终态审计待证假设

- 不按提交标题直接判断保留或删除；必须追到 `origin/main` 的最终运行时调用点。
- 重点区分四类对象：当前业务权威数据、迁移期双写/影子数据、只读观测与回滚证据、已经失效但仍需保留历史数据的兼容结构。
- 已确认管理端仍暴露一组 `/api/admin/niffler-core/*` 路由，前端也仍包含迁移控制台、就绪状态、产品方案和上游接入页面；存在代码不等于仍参与用户请求，需要继续核对鉴权、路由、计费结算和钱包更新调用链。
- 数据库迁移若已在线上执行，后续结论应优先采用“停止写入/隐藏入口/保留只读历史/另行迁移归档”，不能把删除迁移文件或直接删表当成普通代码还原。
- `Niffler Core` 提交簇只作为追踪索引，不是一个可以整体保留或整体剔除的功能单元；其中很可能同时包含现行权威模型、迁移脚手架和已经被后续钱包逻辑取代的旧预留机制。

### 第一轮源码证据

- 设计文档明确写明第 1 至第 3 批的上游服务、产品策略和错误返回设置均为影子结构，初始约束是“不参与现有调度、计费和错误返回”。因此，管理端路由和写接口仍然存在，只能证明配置可达，不能证明它们已经成为线上请求权威来源。
- 文档定义的长期目标远大于已落地范围，包含统一产品策略、额度预占、结算快照、邀请返利和路由尝试。需要按每个实体单独核对最终调用者，不能依据设计目标推断实现完成。
- `apps/aether-gateway/src/data/state/niffler_core.rs` 暴露了完整的读写封装，但当前粗略引用数大量来自管理端、测试和数据层本身。下一步必须排除这些定义与管理调用，再找业务请求链路里的真实读写。
- Niffler Core 的数据库对象已进入三种数据库迁移、逻辑 schema、生成基线和 Postgres bootstrap；即使最终判定某个对象不再使用，也不能通过还原提交或删除历史迁移来处理。

### 第二轮源码证据：已存在的业务调用

- `niffler_runtime.rs` 会读取用户 Key 的影子产品策略绑定和运行时灰度设置；设计文档后段也明确记录第 5 批第三片已经把开启 `enable_new_routing` 的对象切到新产品策略与新上游服务/账号作为业务读源。因此“Niffler Core 全部仍是影子配置”已经不符合最终代码。
- `usage/reporting/mod.rs` 会按灰度写路由尝试、结算快照和预占干跑记录，并在请求结束时处理真实预占；这些对象分别可能是旁路证据、对账副本或请求前约束，必须继续拆分。
- `niffler_error_return.rs` 会在风险规则命中后写账号风险事件；是否改变用户错误文案和调度状态还要继续核对调用点与开关。
- `state/runtime/referrals.rs` 会创建 Niffler 返利账本；该部分已超出纯影子观测，需要检查新旧返利发放互斥和幂等约束。
- `maintenance/runtime/niffler_billing_reservation_expiry.rs` 会扫描并终结过期预占。即使后来禁止新建预占，历史 active 记录仍需要这类清理任务，不能先删维护代码。

### 真实预占已经退出准入链路

- `NifflerRuntimeRolloutDecision::from_setting` 无条件把 `enable_billing_reservation` 解析为 `false`，旁边注释明确说明“金额预占已退出请求准入，保留旧配置字段只为兼容历史数据”。这意味着数据库或管理端即使把开关设为真，也不能重新启用。
- 最终测试同时覆盖 Key 级和产品策略级开关，断言开启后不会创建预占；另一个测试断言历史 active 预占不会减少当前钱包可用余额。
- 初步判定：请求前真实预占属于 `REMOVE/RESTORE_UPSTREAM` 的行为，当前代码已经做了安全停用；表、事件、查询、过期清理和只读对账暂时属于 `KEEP_REBASE`，用于历史记录收尾。管理端仍允许编辑一个永远不生效的开关会造成误导，属于待移除或改成只读历史状态的入口。
- 产品策略、账号能力和新路由仍在 `enable_new_routing` 灰度下参与模型权限、销售倍率和可调度账号过滤，属于真实业务权威读源，不能随预占逻辑一起删除。

### 新路由当前的权威范围

- 鉴权上下文在新路由命中时，用 Niffler 产品策略覆盖旧分组的允许模型、分组 ID/名称、默认销售倍率和模型级销售倍率；同时跳过旧 Provider/API 格式限制。这是用户权限与售价的直接行为变化，判定为必须保留的产品能力。
- 候选预选会按 Niffler 上游服务 ID、账号 ID 和账号模型能力过滤旧传输候选行。新模型负责决定“谁可以被选”，旧 Provider catalog 仍承载端点、协议、凭证和传输快照。当前属于双模型拼接，不是完整替换。
- `ae5b8d6e` 的 8 月透支修复没有删除 Niffler 新路由，而是停用真实预占并重写钱包/套餐准入与最终结算。说明上游合并时应保留新路由的产品能力，同时以当前 Niffler 计费根修复为结算基线，不能恢复旧预占估算。
- 风险点：新路由解析产品策略和账号能力使用 5–30 秒本地缓存，并在候选预选前访问 Niffler Core 仓储；Aether 后续新增的请求热路径缓存、数据库保护和准入机制必须与这里合并，不能直接覆盖 `candidate_source.rs`。

### 错误规则、影子报告与返利账本

- 错误返回规则是灰度业务行为：命中 `enable_error_return_rules` 后，平台错误和上游错误会真实改写用户收到的文案；上游风险关键词还会写账号风险事件。风险事件当前只记录配置的保护动作，没有看到它直接修改旧 Provider Key 状态，因此“文案改写”应保留，“自动封禁/冷却”仍需确认没有其他消费方。
- 路由尝试与结算快照的写入函数和日志均明确标为 shadow（旁路记录），失败只告警、不影响用户请求；它们适合作为迁移对账证据保留，但不应被当作结算权威来源。
- 预占干跑与真实预占终结都位于 `enable_billing_reservation` 分支，而最终灰度决策强制把该值设为 `false`。正常新请求不会再执行这些分支；代码仅可能服务于直接管理操作或历史数据清理。
- 返利存在两条路径：未命中灰度时旧逻辑实际发放并可旁路写账本；命中 `enable_referral_ledger` 时，旧明细先进入账本待发状态，再由 Niffler 账本事务实际发钱。返利账本不是纯影子表，应保留其订单幂等与人工重试/取消能力，并在合并前专项验证旧队列不会再次发放。
- 返利灰度解析要求一个用户只能唯一命中一把 Key 或一个产品策略；多 Key 或多策略冲突时跳过新账本。这种安全拒绝避免随意选择，但也意味着启用策略和用户 Key 结构存在隐含运营约束，需要在方案中列为上线检查项。

### 账号风险动作与管理端可达性

- 全仓搜索未发现账号风险事件的运行时读取或执行器；当前只写 `niffler_account_risk_events`，配置中的 `pause_scheduling` / `disable_account` 没有实际修改账号状态。管理端允许配置这些名称会让管理员误以为保护动作已执行，建议保留风险事件记录和 `record_only`，暂时剔除或禁用另外两个动作，直到实现明确的状态转换、恢复时限和审计闭环。
- Niffler Core 四个管理页面都已注册正式路由；主导航直接展示“核心”入口。产品策略和上游接入属于日常业务配置，可以保留；就绪检查和迁移控制台属于迁移运维工具，不宜长期占用一级导航，应在完成迁移后收进高级运维区域，并隐藏已失效的预占开关。

## 2026-08-12：路径与提交双向追踪

- 新增 `map_paths_to_commits.py`，以每个提交相对第一父提交的 `--no-renames` 差异生成路径到提交/功能簇的反向索引；合并提交只标为 `integration_merge`，用于补全通过合并结果才进入主线的路径来源，不当成功能补丁重放。
- Niffler 路径提交图包含 1,367 个历史触达路径，其中 30 个不在最终并集；Aether 包含 2,857 个历史触达路径，其中 148 个不在最终并集。它们是分叉期间曾改动、随后恢复、删除或经目录迁移消失的历史路径。
- 最终并集的 3,310 条路径全部通过来源校验：Niffler 最终改动路径均有 Niffler 提交来源，Aether 最终改动路径均有 Aether 提交来源；没有 `missing_provenance`。
- 每条路径记录首次/末次提交、全部相关提交、功能簇、最终是否存在和最终差异范围；后续人工功能判断可以回查到机器清单，不依赖提交标题摘要。

### 功能簇归类复核

- 已把 Niffler 316 个非合并提交全部归入明确功能簇；原先宽泛的 `misc_product_fix` 已清空。
- 覆盖复核发现本地仓库原先是浅克隆，浅边界 `38aa0849` 被误当成无父提交。补全历史后确认其真实父提交为 `14f9ca5e`，实际影响为 3 个 Provider 测试文件、增加 64 行并删除 1 行；双向清单已全量重建。
- 完整提交账本现已闭合：Niffler 的提交、影响、分类和处置文件均为 357 条；Aether 四份文件均为 776 条。生成器新增浅克隆拒绝检查，防止后续复现不完整结果。
- 已把 Aether 592 个非合并提交全部归入明确功能簇；原先 90 个 `misc_upstream` 经标题和主题规则复核后已清空。
- 合并提交单独归入 `integration_merge` 和 `HISTORY_ONLY`，不重复计算为功能实现；路径来源账本仍记录其第一父差异，以覆盖只有合并结果进入主线的最终变化。
- 两个超大 Niffler 提交 `f7d9cbf1`、`e1012747`、`fc6b56e1` 仍需按文件和最终行为拆解：它们分别混合早期二开、调度/导入修复、网关容错与生产运维，不能给整提交统一处置标签。

## 2026-08-12：Niffler 商业化最终基线

- 2026-08-10 的透支修复把请求级 `billing_request_admissions` 设为最终计费准入凭证；结算缺失准入、供应商越界或重复执行时不能猜测扣款。
- 最终规则明确拒绝金额预占、用户级串行和输出 Token 限制；合法开始的最后一批请求可让钱包变负，套餐只扣到 0，剩余费用补扣钱包。
- 套餐模型范围已从早期静态模型清单演进为“套餐选择供应商，模型随供应商当前模型关系动态决定”；旧静态范围仅用于未确认历史权益兼容。
- 欠费套餐订单保存下单时欠款，支付到账事务先偿还订单欠款再发放套餐；该语义与通用 Aether 钱包/支付实现不同，合并时必须保护。
- `03-niffler-customizations.md` 已建立按最终能力划分的 Niffler 二开目录和初步处置，不以历史标题直接替代最终代码判断。

## 2026-08-12：Aether 上游早期更新线索

- 分叉后首周已包含 API Key IP 白名单、通知/Server 酱、官方支付退款、Provider Key 余额刷新、后台用户服务端分页、Postgres 参数、隧道安全、Windsurf、Gemini CLI、OpenAI 图片与流式终态修复等多条独立能力线。
- 2026-05-21 的 `ab0a90de9` 引入 Redis 长连接通道治理；2026-05-22 的 `8966fd6aa` 在数据库连接池压力下保护后台 worker；2026-05-24 的 `576918daa` 优化 Provider 调度数据库热点。这三项都早于此前错误限定的日期范围，是必须评估的基础设施更新。
- 上游对 OpenAI/Codex 流式终态、断连 drain、SSE 控制块、首字超时和候选 watchdog 的修复从 5 月持续到 7 月，不能只挑 7 月 30 日的端到端计时提交。

### 完整时间线的四条主线

- 流式与失败语义：5 月 21 日起连续处理终态事件、下游断开、HTTP 200 内错误、SSE 控制事件、首字可见性、watchdog、重试、取消和终态单调性；7 月 30 日再补端到端耗时、失败诊断和 payload 处理。
- 运行时性能：Redis 长连接治理、后台 worker 的数据库压力保护、调度查询热点、Provider catalog 缓存、网关数据库压力、候选记录队列、运行时准入、用量 worker 自动扩缩和 20k 流热路径。
- 协议与账号：Windsurf、Antigravity、Gemini CLI、Kiro、OpenAI/Codex/Responses、Claude、Grok 等协议持续更新；OAuth 账号状态、Agent Identity、FedRAMP、凭证隔离清理和 Provider transfer limit 也在演进。
- 架构重构：7 月 15 日起执行 layered crate boundaries（分层 crate 边界），大量模块在 workspace 内拆分和迁移。它影响 1,600 多个历史路径，是多数 7 月后功能的前置基础，不能把后续提交直接套到旧 Niffler 文件布局。

### 7 月至 8 月的关键上游能力

- GPT-5.6/Codex 请求契约、processing tier（处理档位）授权/价格/结算、Search 与执行协议、Responses Compact/V2、动态配额窗口和 Agent Identity。
- 在线价格目录同步、价格来源追踪和 tiered pricing（分档价格）修正。
- API Key 历史身份解耦、批量账号配置/动作、Provider transfer limit、OAuth 账号管理增强。
- Provider request execution 加固、routed pool 调度加固、端到端故障切换与请求诊断、Responses continuation history。
- 2026-08-12 把默认服务端数据库连接池下限提高到 32；该默认值不能脱离 Niffler 实际数据库容量直接照搬。

## 2026-08-12：逐路径覆盖账本

- `generated/path_coverage_ledger.tsv` 已覆盖最终并集中的 3,310 个路径；每条都关联最终差异范围、冲突等级、双方功能簇和对应语义报告章节。
- 覆盖状态全部为 `mapped`。这表示“每一处路径差异”都有机器清单和人工语义归属，不表示每个文件都适合独立实施；实施决策仍按功能/数据边界成组执行。
- Niffler 与 Aether 最终树直接差异为 3,309 个路径、约 +685,290/-481,491 行；另一个路径是双方相对共同基点都删除且最终一致。

## 2026-08-12：生产风险复核中的迁移事实

- Niffler 固定主线包含 134 个迁移 SQL 文件、62 个版本；Aether 固定主线包含 109 个文件、58 个版本。两边同一版本和数据库组合共有 67 组，其中 64 组内容一致，3 组 `20260403000000` 基线分别在 PostgreSQL/MySQL/SQLite 中内容不同。
- Niffler 的迁移校验对已应用版本只把 checksum 漂移记为告警，不阻止启动。因此，直接用 Aether 基线替换 Niffler 基线可能通过版本校验，却让新空库结构和已升级生产库结构不一致；路线图已增加真实 schema 目录对账门禁。
- Niffler 独有迁移组合 67 组，Aether 独有 42 组，禁止按目录覆盖或仅按迁移版本排序执行；每项上游 schema 变化都必须生成新的 Niffler 版本或确认已等价存在。
# 2026-08-13 Review Fix 实施确认

- 身份收敛开关应在确认请求属于 Codex OAuth Responses 后读取；因此请求级上下文只保存任务信息和共享的延迟读取结果，创建上下文本身不访问系统配置。
- 客户端身份使用 Niffler 已校验的 Codex 客户端版本，同时覆盖 `user-agent`、`originator` 和 `version`，避免三个字段互相矛盾。
- 父任务只处理官方语义明确的线程字段：HTTP `x-codex-parent-thread-id`、客户端元数据同名字段、任务元数据 `parent_thread_id` 和 `forked_from_thread_id`；不处理属于轮次语义的 `parent_turn_id`、`root_turn_id`。
- 最终回归确认：普通用户请求和管理端模型测试都在所有请求规则之后执行同一套收敛；开关关闭不改请求；损坏配置只拒绝适用的 Codex OAuth Responses，不影响其他 Provider。
- 发布源是 `ryfineZ/Niffler`；`main` 和 `test` 都由仓库 Ruleset 保护。当前分支提交是远端 `main` 的祖先，不能直接推送；必须在最新 `main` 上重放本功能，再按受保护晋级链发布。

## 2026-08-14：Codex OAuth 客户端环境身份一致性

- Niffler 的普通转发会预先过滤 `x-stainless-*`，完整透传仍可能保留它；`sec-ch-*`、`sec-fetch-*`、`x-aether-tls-*`、浏览器来源、语言偏好和客户端应用字段也可能进入最终请求。因此删除动作必须位于 Codex OAuth 身份收敛的最后阶段，不能只改通用转发白名单。
- 统一出站身份采用 Linux Codex CLI 后，继续透传客户端的 Node SDK、浏览器、macOS、Windows、arm64、来源页面或真实入口网络字段会形成互相矛盾的环境描述。
- 请求正文、工具定义以及 `x-codex-turn-metadata` 内的工作区、沙箱和任务类型属于模型完成任务所需的上下文，不能按设备身份删除。
- 删除规则使用明确名称和明确前缀，不按模糊关键词删除；`x-business-context` 这类不属于环境身份的业务自定义字段保持不变，避免误伤项目上下文。

## 2026-08-14：Codex App 生图未展示诊断

- 用户截图中只有“已读取 Imagegen 技能”，没有 `image_gen.imagegen` 工具调用或图片结果；模型随后输出的“已生成”是普通文本，不证明生图执行成功。
- 当前架构明确区分两条链路：Codex App 本地 `image_gen` 工具与 Niffler 服务端托管 `image_generation` 工具；“读取技能”本身不会执行任何一条生图请求。
- 架构文档记录：Codex App 本地工具的稳定加载依赖 Provider 配置携带非空 `X-OpenAI-Actor-Authorization`；服务端托管工具则应在 Responses 请求中注入 `image_generation`。
- 服务端代码默认为 `codex` 和 `chatgpt_web` 启用托管图片工具，但 Provider 或 Endpoint 中的显式布尔值优先；`false` 会完全阻止注入。
- 现有真实接口验收文档记录，生产“Pro号池”当时明确配置 `openai_responses_image_generation_tool_enabled: false`；文档同时要求正式发布时改为 `true` 或删除该值。这是待用当前生产状态确认的直接线索。
- `rn01` 当前可以读取访问；生产 PostgreSQL 与 Redis 容器健康，同机的隔离生图验证容器仍在运行。这说明可直接核对当前数据库配置，无需根据旧文档猜测。
- 更正：2026-08-14 查询到的 Pro号池当前值确实是 `openai_responses_image_generation_tool_enabled: true`，但 Provider `updated_at = 17:17:21`，晚于 `wenwen` 15:15‑15:16 的失败请求。因此不能用当前值证明目标请求时已开启；旧文档中的 `false` 仍是直接线索，需从 `audit_logs` 还原变更历史。
- `audit_logs` 表在 8 月 14 日 14:30‑17:30 没有任何记录，无法从应用审计还原 17:17 Provider 更新。下一步查 Frontdoor 访问日志是否出现管理端 Provider 更新请求，并检查目标请求前的最近数据库备份。
- 当前最可疑的请求级分支是：只要请求 `tools` 中声明 `image_gen` namespace 或 `image_gen.imagegen`，网关就不再注入服务端 `image_generation`。如果客户端声明了本地工具却没有实际执行，就会形成截图中的“读了技能、没有图片、文本误报成功”。
- 用户补充：问题使用生产用户 `wenwen` 测试，新截图显示当天 16:39 附近发出“帮我生成一张你的自拍照”，等待 1 分 41 秒后只返回文本，没有工具调用或图片。
- 用户确认历史上通过用户级 `~/.codex/config.toml` 为 `model_providers.cpa.http_headers` 加入 `X-OpenAI-Actor-Authorization = "niffler-native-image"`，并设置 `requires_openai_auth = false` 后曾恢复生图；最近多次版本更新后复发。
- OpenAI 官方配置参考确认：`model_providers.<id>.http_headers` 用于每个 Provider 请求的静态 HTTP 请求头；`requires_openai_auth` 表示该 Provider 是否使用 OpenAI 身份验证，官方默认值已是 `false`。Provider 配置必须位于用户级配置，项目级 `.codex/config.toml` 不能覆盖 `model_providers`。
- 生产 `usage` 记录同时保留客户端请求头、原始请求体、最终上游请求头和请求体；因此可以针对 `wenwen` 的准确请求比较 `X-OpenAI-Actor-Authorization` 是否入站、是否被网关保留，以及两个 `tools` 数组的差异。
- 已在生产数据库定位 `wenwen` 近期请求。2026-08-14 15:15:29、15:15:45 和 15:16:07 连续三个请求都使用 Pro号池的 Responses 端点，后两个模型为 `gpt-5.6-sol`；最后一个请求用时 54.725 秒，首字节 0.749 秒，被记为 HTTP 200 `completed`。
- 2026-08-13 16:36:54 还有一个 `wenwen` 请求持续 885.266 秒后以 HTTP 499 取消；16:38:59 和 16:39:15 的后续请求均为数秒内完成的普通文本响应。
- 因此当前异常不是 HTTP 错误或图片流中断，而是请求未进入生图执行却被按普通文本成功结算。
- 这三个请求的 `usage` 行仍在，但入站/出站请求头、请求体、响应体和压缩列已全部为空。数据库的生产隐私清理策略已移除逐字请求证据，无法再从 `usage` 还原请求头或 `tools`。
- 还可用已定位的 request ID 查应用容器日志；如果日志也不记工具类型，需结合代码差异和本机当前 `config.toml` 完成根因判定。
- 当前运维文档记录 `api.niffler.org` 和根域名的真实用户流量主要落到 `hd0526` Frontdoor，OVH 保留 `us1` 备用入口。因此首先在 `hd0526` 查目标 request ID，再根据本机 `cpa.base_url` 判断是否需要补查 OVH。
- 直接解析本机 `/Users/zhangyufan/.codex/config.toml` 确认：`model_providers.cpa` 存在，但 `requires_openai_auth = true`，且完全没有 `http_headers` 表，因此当前客户端不可能发出 `X-OpenAI-Actor-Authorization: niffler-native-image`。这与用户所述的历史修复配置相反，已形成直接根因证据。
- `hd0526` 当前 Frontdoor 运行镜像提交 `9f2959a28ae62d0ac28e48518557fa96218faf5f`，容器于查询前约 5 分钟重建；目标请求发生在重建之前，当前容器日志可能已不再包含该请求。
- 更正：当前 Frontdoor 容器日志仍包含目标 request ID。三次请求都进入 `/v1/responses` 的流式执行路径，以 HTTP 200 `completed` 结束；访问日志没有记录图片工具执行或失败事件。
- 本机当前全局 `model_provider` 实际为 `custom`，不是用户历史修复所指的 `cpa`。`custom` 与 `cpa` 当前均为 `requires_openai_auth = true`，两者都缺失 `X-OpenAI-Actor-Authorization`。因此只向非活动的 `cpa` 加请求头也不会影响这次会话。
- `/Users/zhangyufan/.codex/config.toml` 最后修改时间是 2026-08-14 14:06:27 +0800，早于 `wenwen` 15:15–15:16 的生图测试。时间顺序确认错误配置已在测试前生效。
- 本机配置备份中可找到含 `niffler-native-image` 的旧版，说明该请求头确实曾存在；当前文件已不包含。
- 旧备份 `/Users/zhangyufan/.codex/config.toml.bak-before-custom-provider-20260725` 给出完整对照：当时活动 `model_provider = "cpa"`，`requires_openai_auth = false`，且 `http_headers` 含 `X-OpenAI-Actor-Authorization = "niffler-native-image"`。当前配置同时改了活动 Provider、身份验证标志和图片授权请求头，不是单一字段失效。
- 最近的 Codex OAuth 身份收敛代码会删除客户端环境请求头，但明确名单和前缀中都没有 `x-openai-actor-authorization`。当前代码不会因身份收敛删除该请求头，因此新提交直接从网关丢头的猜测不成立。
- 当前公共首页的 Codex 配置生成器和用户指南都输出 `requires_openai_auth = true`，且都没有 `X-OpenAI-Actor-Authorization`。另一条安装脚本已使用 `requires_openai_auth = false`，但也没有添加图片授权请求头。三个用户配置出口当前不一致，且都没有生成已验证的完整生图配置。
- 这些公共配置文案并非最近身份收敛提交新增：首页的 `true` 可追溯到初始提交，指南中的 `true` 来自 2026-07-23。近期版本更新更可能是触发了配置重新生成/切换，从而把早先手工修好的值覆盖。
- 本机安装并正在运行 CC Switch，其数据目录为 `/Users/zhangyufan/Library/Application Support/com.ccswitch.desktop`。当前 `custom` Provider 形态和“切换到 custom Provider 前”的旧备份名称一致，CC Switch 或其 Provider 切换流程是配置重写的主要疑点。
- 2026-08-14 的其他 Codex 任务记录中没有 `config.toml`、`requires_openai_auth` 或 `niffler-native-image` 的修改痕迹；macOS 统一日志在 13:55–14:15 也没有记录 Codex 进程改配置。暂时没有证据表明是 Codex 任务直接改写。
- 远端 `main` 当前为生产镜像同一提交 `9f2959a28ae62d0ac28e48518557fa96218faf5f`；当前本地尚未有该提交对象，需只读获取后对照最终上线代码。
- 已只读获取生产提交 `9f2959a28ae62d0ac28e48518557fa96218faf5f`。它是 2026-08-14 13:06:05 的 test 合并，相比已审查的身份收敛修复 `d13c9f6bc`，只改了 PostgreSQL 钱包、管理表格宽度和相关文档/测试；没有改图片链路、Codex 请求头或配置生成器。最近这次生产发布不是直接代码回归来源。
- CC Switch 当前进程打开的主数据库是 `/Users/zhangyufan/.cc-switch/cc-switch.db`，日志是 `/Users/zhangyufan/.cc-switch/logs/cc-switch.log`。可从这两处核对 14:06 前后的 Provider 切换与生成配置，不需要操作 CC Switch 界面。
- CC Switch 3.16.5 当前数据库有 11 份 Niffler Codex Provider。其中 `Niffler - Plus` 和 `Niffler - ryfine` 同时保存了 `requires_openai_auth = false` 和 `X-OpenAI-Actor-Authorization = "niffler-native-image"`；Friday、team 和两份 Plus copy 使用 `custom + true` 且无请求头；Will、dudu、yiyou、zan-max 为 `cpa + false` 但同样无请求头；名为 `niffler` 的旧配置为 `cpa + true` 且无请求头。
- 8 月 1 日至 8 月 14 日的 CC Switch 备份中，这些 Provider 的关键字段持续保持上述差异。这证明 CC Switch 按 Provider 保存完整 Codex 配置，切换/复制/重新导入不会自动从其他已修好的 Provider 继承生图字段。这是“修好后又复发”的直接机制。
- CC Switch 当前选中的是 OpenAI Official，而截图中会话标记为 `Niffler - wenwen`；当前数据库没有同名 Provider。因此本机当前的全局 `config.toml` 可以证明字段会被切换覆盖，但不能单独作为 `wenwen` 该会话入站请求头的逐字证明；该结论仍以服务端请求类型和客户端无工具调用的组合证据为主。
- 生产记录确认两个相关 `gpt-5.6-sol` 请求都来自 `Codex Desktop/0.146.0-alpha.9.2`，桌面版本为 `26.727.51351`，macOS arm64。两次请求的计费维度都明确为 `image_count = 0`、`image_output_pricing_mode = "none"`，只记录了文本 Token；没有图片产出或图片定价。
- 目标请求只有一个成功的 Pro号池 `openai:responses` 执行尝试，`required_capabilities` 为空，候选记录也没有图片能力或图片工具标记。这进一步确认服务端最终只执行了普通文本 Responses。
- 更正：首次图片历史明细查询在工具的 30 秒窗口内没有返回输出，空输出不能解读为零行。改用聚合查询确认：`wenwen` 自 2026-07-08 起有 3,866 条保留请求，其中 119 条命中图片路径、135 条使用 `gpt-image-*` 模型，存在可对照的历史图片记录。
- `wenwen` 最近的真实图片请求发生在 2026-08-03 22:19:22（北京时间），请求 ID 为 `f1bd5a58-56f0-42e5-b413-eee8f5dfb902`，使用 Pro号池的 `gpt-image-2` 和 `/v1/images/edits`，状态 `completed`，用时 54.989 秒。同日晚间存在多条同类成功 Images API 记录；8 月 3 日之后没有新的 `gpt-image-*` 记录。
- 8 月 3 日 22:14:29 的 `gpt-5.6-sol` Responses 在 18 秒后紧接一条成功 `/v1/images/edits`；22:18:48 的 Responses 在 34 秒后也紧接成功图片请求。这与 Codex Desktop 本地工具先读取模型回应、再单独调用 Images API 的链路一致。
- 成功生图与 8 月 14 日失败测试的客户端完全相同：都是 `Codex Desktop/0.146.0-alpha.9.2`、桌面版本 `26.727.51351`、macOS arm64。因此不能将复发归因于 Codex Desktop 客户端版本升级。两次差异在于：成功时实际发出 Images API，失败时始终只有 Responses。
- 生产隐私清理已同时移除 8 月 3 日成功样本与 8 月 14 日失败样本的入站/上游请求头和请求体，无法从生产库逐字比较 `X-OpenAI-Actor-Authorization` 或 `tools`。结论必须依据“同一客户端版本、服务端图片配置已开启、成功时有 Images 调用、失败时无 Images 调用、Provider 模板关键字段不一致”的组合证据，不冒充有逐字请求头证据。
- 从 8 月 3 日成功样本到当前生产版本，相关路径只有 8 月 4 日的 Responses 兼容提交和 8 月 13‑14 日的 OAuth 身份收敛提交。已直接审查 8 月 4 日 `f4017f361`：图片部分只调整桥接指令后缀的保留/去重和函数可见性，没有改变图片工具启用、跳过或 Images 路由条件。身份收敛提交也不删除 Actor Authorization 请求头。目前没有服务端代码更新直接造成该复发的证据。
- 当天 Codex Desktop 启动日志明确显示 `image_generation` 功能标志已开启；8 月 13‑14 日日志没有记录 `image_gen`、Actor Authorization 或图片工具加载失败。这只能排除全局功能标志被关闭，不能证明当时 Provider 请求头正确。
- 更正：上述本机 `~/.codex/config.toml` 属于排查者当前 Mac，不是 `wenwen` 电脑的配置，不得用于解释 `wenwen` 请求。用户已亲自确认 `wenwen` 的实际配置含 `X-OpenAI-Actor-Authorization = "niffler-native-image"`，且 `requires_openai_auth = false`。“客户端 Provider 配置被覆盖”的根因结论作废。
- 新的已知前提：`wenwen` 与 8 月 3 日成功生图使用同一 Codex Desktop 版本，当前客户端配置也正确；问题必须重新在服务端工具注入、请求转换、上游能力判定或返回事件链路中定位。
- 当前服务端存在明确互斥分支：只要 Responses `tools` 中声明 `namespace:image_gen` 或 `image_gen.imagegen`，`apply_openai_responses_image_generation_bridge_body_edits` 就不注入托管 `image_generation`。这意味着客户端工具“被声明但模型未调用”时，服务端不会补上托管生图。
- 8 月 4 日新增的受管理提示词逻辑使用 `codex_openai_responses_has_image_generation_tool` 判断是否保留图片桥接指令；该判断只识别托管 `image_generation`，不识别客户端 `image_gen`。因此如果 `wenwen` 所在用户组启用了新受管理提示词，需检查它是否改变了原客户端指令的位置或有效性。
- 客户端 `image_gen` 与托管 `image_generation` 的互斥分支于 2026-07-11 由提交 `8a3003ca58` 引入，早于 `wenwen` 8 月 3 日的成功生图；它是可能放大问题的设计缺口，但不是 8 月 3 日之后新出现的回归。
- 8 月 3 日成功 Responses 与 8 月 14 日失败 Responses 使用同一 API Key 用户组 ID `aeec3aa3-9f88-48f0-b745-c1680a9c9434`；组名从“GPT Pro 0.33”改为“GPT Pro 0.29”，但不是切换到另一用户组。需直接检查该组的受管理提示词配置和更新时间。
- 生产 `user_groups` 直接查询确认：该组 `managed_instructions` 为空，最后更新于 8 月 12 日 16:21。因此 8 月 4 日引入的受管理提示词没有应用到 `wenwen` 的请求，可排除。
- 以 8 月 3 日 22:19 前的最后一个 `main` 提交 `621c53ef1` 为成功基线，对比当前生产 `9f2959a28`：与图片/Responses 相关的后续变更包括 8 月 5 日 Images 直通改造、8 月 9 日运行记录调整、8 月 13‑14 日 Codex OAuth 身份收敛。需用全站图片请求时间分布先确定回归发生日期。
- `usage` 已有 `(api_family, endpoint_kind)` 索引；已知成功图片行为 `api_family = openai, endpoint_kind = image`，失败目标行为 `openai, responses`。后续全站统计使用索引字段，避免再扫描 JSON 元数据。
- 全站图片记录排除服务端全局回归：8 月 13 日有 26 次图片请求、17 次完成；8 月 14 日有 58 次且全部完成，最晚到 16:02:43。身份收敛上线后图片端点仍正常工作。
- 8 月 14 日 15:52:33 和 15:59:11，另一用户使用 Codex Desktop `26.803.81509` Windows 客户端，通过与 `wenwen` 完全相同的 API Key 用户组 ID 和 Pro号池，成功调用 `/v1/images/edits`。因此 Provider、用户组、号池、Images 路由和 8 月 13 日后的服务端都可成功生图；差异收缩到客户端请求内容或客户端版本/平台。
- Codex Desktop `26.727.51351` 全站最晚成功生图于 8 月 13 日 11:58:47，共有 170 条成功图片记录、9 个用户；该客户端版本本身没有被服务端整体拒绝。
- 更正：8 月 4 日 13:55‑14:01 的同版本 Mac 成功样本属于另一用户，不是 `wenwen`；定向查询 `wenwen` 在该时段为 0 行。`wenwen` 自己的最后成功图片记录仍是 8 月 3 日 22:19:22。
- 对比 `wenwen` 自己的 8 月 3 日成功 Responses 和 8 月 14 日失败 Responses：用户 API Key、用户组、Pro号池 Provider ID、Endpoint ID、模型、API 格式、流式设置、推理强度和出站 TLS 配置均一致。
- 已确认的有效差异是实际选中的上游 Provider API Key（具体 Codex OAuth 账号）不同：8 月 3 日成功样本中可见账号引用哈希为 `3e6e980a5d`，8 月 14 日三次失败都是 `6c19080f3d`。另一个差异是 Codex Desktop User-Agent 的环境位从 `dumb` 变为 `unknown`，其余服务端元数据相同。
- 8 月 14 日另一用户的成功 Codex Desktop 生图链中，图片前的 Responses 使用了第三个 OAuth 账号引用 `43b6345a26`，随后 Images 请求使用图片端点的账号 `0a3078d3ef`。这证明 Responses 阶段的上游账号会变化，需核对 `6c19080f3d` 账号本身的配置和候选能力。
- 直接查询三个 Responses OAuth 账号配置：失败账号 `6c19080f3d` 处于 `active`，`is_active = true`，认证类型为 OAuth，没有模型、API 格式、能力、包含/排除模型限制，没有 OAuth 失效或最近错误。另一成功账号 `43b6345a26` 的这些字段与它相同。
- 8 月 3 日成功账号 `3e6e980a5d` 后来于 8 月 12 日被标记 OAuth Token 失效，但这发生在它成功生图之后，不能解释 8 月 3 日当时的差异。
- 失败账号与同日成功 Responses 账号的 `upstream_metadata.codex` 字段集合完全一致，均含 plan type、credits 和两个限额窗口状态；没有一方缺少账号元数据的异常。
- 失败账号与同日成功 Responses 账号都是 Pro plan，credits 标志一致，次级窗口使用率分别为 72% 和 68%，都没有超限或账号异常。
- 成功/失败 Responses 的 `request_candidates` 均只有一个成功候选，同一 Provider、Endpoint、`gpt-5.6-sol`、`local_same_format`、无格式转换、HTTP 200，`required_capabilities` 均为空。除 OAuth 账号外没有候选规则差异。
- 下一个服务端重点不是“身份收敛显式删除了 Actor Authorization”，而是“最终请求头重建/白名单是否从未带入该头”。用户已确认客户端配置会发送该头，因此必须沿入站到 ChatGPT Codex 上游的完整头部构造链检查。
- 全仓搜索没有任何 `X-OpenAI-Actor-Authorization` 的显式保留、注入或测试；它只可能依赖通用透传逻辑。标准 Responses 路径的最终请求头由 `aether-provider-transport` 中的 `build_standard_provider_request_headers` / `build_openai_passthrough_headers` 构建，需直接审查这两个函数的入参和过滤规则。
- 直接审查通用头部构造：Actor Authorization 不在 `should_skip_upstream_passthrough_header` 或完整透传过滤名单中；`openai:responses -> openai:responses` 是 same-format 路径，会从传入的 `effective_headers` 做完整透传，再替换上游 OAuth Authorization。只要该头仍存在于 `effective_headers`，这一层会保留它。
- 因此剩余的头部风险点只有三个：`effective_headers` 在更早阶段已经移除该头、Endpoint `header_rules` 删除/改写该头，或最后的 Codex 身份收敛重建后未保留。下一步逐个排除。
- `effective_headers` 的语义已确认：没有路由组上下文时它就是原始入站 HeaderMap；有路由组时使用 `LocalRoutingRequestContext.effective_headers`。因此只需检查路由变换创建该 HeaderMap 的代码，不存在其他隐式中间层。
- 路由变换的创建点也是先完整 `parts.headers.clone()`，再只应用显式 routing mutation plan；未选中 routing group 时直接使用原始入站头。代码不存在默认重建白名单。剩余需核对目标请求是否选中路由组，以及 Pro 号池 Endpoint 的 `header_rules`。
- Pro号池 Responses Endpoint `ddb219df-...` 的 `header_rules` 为空，且该 Endpoint 自 2026-06-11 后未更新；它不会删除 Actor Authorization。
- 已逐行审查 8 月 13‑14 日 Codex OAuth 身份收敛的最终 `rewrite_outbound_headers`：它只删除/改写安装、会话、任务、回合、账号、设备证明、User-Agent 和明确的客户端环境头，没有重建整个 HeaderMap，也没有删除 Actor Authorization。因此现有代码在该头真正入站时会一直保留到 ChatGPT Codex 上游。
- 结论边界：由于目标行的原始头已清理，无法证明那次实际请求确实带入该头；但可以排除“网关收到后在当前代码中丢弃”。
- 第一次查询 17:17 Provider 更新时间附近的 Frontdoor 日志时，过滤条件命中了大量普通业务请求，没有提供配置修改来源证据；该查询已停止。下一步只查 09:17:21Z 附近的管理接口写请求。
- 代码确认 Provider 普通修改接口是 `PATCH /api/admin/providers/{id}`。精确查询 17:17:00–17:17:40（09:17:00–09:17:40Z）的 Frontdoor HTTP 完成日志，没有找到 Provider 管理写请求；`updated_at` 可能来自其他入口、数据库直接操作，或日志没有记录该请求。
- 扩大到 17:15–17:20 后仍未找到 `PATCH /api/admin/providers/{id}` 或 `update_provider` 日志。
- rn01 的定时备份在当天 04:35 开始、04:47 成功上传，文件为 `postgres/aether/daily/2026/08/aether-20260813T203501Z.dump`。该备份早于 wenwen 15:15 测试，可直接还原当时 Provider 图片工具开关。
- 备份脚本上传并校验后会清理本地文件；目标备份当前只在 R2。rn01 主机没有独立 `pg_restore`，但现有 PostgreSQL 容器内可用，只需流式读取归档目录或目标表，不必恢复到生产数据库。
- 已通过 R2 流和 PostgreSQL 容器成功读取归档目录，确认目标表为 `public.providers`，表数据归档项存在。下一步只输出该表的 COPY 列定义和目标 Provider ID 一行。
- 从标准输入按表筛选归档没有返回内容，尚不能解释为空表；这更可能是自定义格式归档在管道输入下无法按目录随机访问。后续先确认字节数，再决定是否下载到临时目录。
- 更正上一条的原因判断：归档流本身可按表提取，表匹配必须写 `providers`，不能写 `public.providers`。
- 04:35 测试前备份中的 Pro Provider 记录明确包含 `openai_responses_image_generation_tool_enabled: true`，记录更新时间为 04:34:45。因此 15:15 失败时服务端托管图片工具开关已经开启；17:17 的再次保存不是修复动作，也不能解释失败。
- 直接搜索 Frontdoor 容器的 149 MB 原始日志文件，找到 wenwen 三个目标请求；每次只记录鉴权通过、候选账号选定和 HTTP 200 完成，没有图片工具注入或图片调用事件。日志本身不足以判断最终请求体中的工具列表。
- 目标请求编号对应的网关耗时分别约 7.0 秒、2.4 秒和 16.7 秒；均走 `execution_runtime_stream`，不是 Images API，也没有网关错误。
- 当前服务端有一个关键互斥条件：若请求工具列表中出现 namespace `image_gen`，或 function/custom `image_gen.imagegen`，网关就不注入托管 `image_generation`，也不追加“必须实际调用图片工具”的桥接指令。此时成图完全依赖客户端本地工具被模型正确调用。
- 该互斥分支由提交 `8a3003ca5` 引入，时间早于 wenwen 8 月 3 日成功记录；它能解释当前失败机制，但不能单独解释为何最近才复发。还需比较成功与失败请求是否都声明了本地图片工具，或上游模型行为/工具定义是否变化。
- 已核对开关传递链：Provider 配置值会直接传入 Responses 请求规范化；Codex Provider 且 Responses 格式时允许注入托管工具。没有发现开关在规划阶段被覆盖或丢失。
- 由于请求体已经清理，现存 `request_candidates` 和 HTTP 审计结构没有独立保存工具名或工具数量；仅靠这三条历史记录无法直接恢复 `tools` 数组。
- 失败使用的上游 OAuth 账号对应 Provider Key 自 8 月 6 日起承载大量请求；对 wenwen 本人共有 56 次 Responses，其中目标三次是最后三次。wenwen 自 8 月 3 日后没有 Images 请求，因此同用户历史无法验证该账号是否能触发图片工具。
- 反向关联 8 月 14 日全部成功 Images 请求后确认：失败所用的同一个上游 Responses OAuth 账号多次成功产生后续 Images 请求，最近样本就在 wenwen 失败前约 2 分钟（15:13）和失败后约 2 分钟（15:17）。可排除该 OAuth 账号本身不支持图片或已失效。
- 15:13 的紧邻成功样本使用 Windows Codex Desktop 26.803.81509、另一个用户组和 `gpt-5.6-terra`；wenwen 使用 Mac Codex Desktop 26.727.51351，先后为 `gpt-5.5` 与 `gpt-5.6-sol`。两者共同证明服务端和上游账号可用，但不能排除旧版 Mac 客户端请求形状或特定工具声明差异。
- 四条对比记录的 `request_metadata` 字段集合一致，均未保存工具清单；可比较的稳定字段只有客户端、模型、用户组、流式标志等。
- 成功基线到当前生产的图片桥接逻辑没有改变“本地 `image_gen` 存在就跳过托管工具”这一条件；后续只加强了托管工具提示词并修正重放的 reasoning ID，不能解释本地工具被声明却未调用。
- 唯一在 wenwen 最后一次成功后新增、且会改写所有 Codex OAuth Responses 请求的公共逻辑，是 8 月 13 日上线的身份收敛：重写安装、会话、任务、账号、User-Agent 等身份字段，但明确保留 Actor Authorization 和工具数组。需先确认该功能在 15:15 是否开启，再判断它与旧版 Mac 客户端是否相关。
- 当前库和 04:35 测试前备份均确认：`codex_oauth_identity_convergence_enabled=true`，开启时间为 8 月 13 日 15:12。它在 wenwen 8 月 3 日成功时不存在，在 8 月 14 日失败时已生效，是目前唯一与回归时间一致的服务端行为变化。
- 身份收敛会删除客户端 `x-oai-attestation`（设备证明）并重写安装、会话、任务、账号、User-Agent、Originator 等身份字段；Actor Authorization 虽未被删除，但其上游授权是否依赖这些配套身份字段仍需核对。
- OpenAI 官方 Codex 仓库已有与截图高度一致的问题 #32251：GPT‑5.6 读取 imagegen skill 后不调用 `image_gen`，却直接声称完成；同一环境中的 GPT‑5.5 能正常调用。另有 #32435 明确记录 `gpt-5.6-sol` 的命名空间工具调用路由异常。两者说明 GPT‑5.6 工具调用本身存在已知回归。
- 官方 app-server 文档确认 `x-oai-attestation` 是桌面客户端按请求生成并转发到 ChatGPT Codex 的设备证明。Aether 身份收敛主动删除它是事实，但尚无官方证据证明 image_gen 的 Actor Authorization 必须依赖该证明。
- 三次目标请求的调用顺序与截图吻合：先有一条 `gpt-5.5` 请求，随后两条 `gpt-5.6-sol`；后两条输入上下文从约 20.8K 增至 27.0K，符合“模型先读取 skill，客户端回传 skill 内容，模型再给最终答复”的两阶段过程。失败答复发生在界面选中的 5.6 Sol 阶段，而不是服务端 Images 请求。
- 仓库已有明确单元测试固定当前行为：请求声明 namespace `image_gen` 或 function `image_gen.imagegen` 时，必须保留客户端工具，同时不得注入托管 `image_generation`，也不得追加托管工具强制调用指令。该设计让 GPT‑5.6 的本地工具调用回归没有任何服务端备用路径。
- 重要更正：wenwen 8 月 3 日两次成功前置 Responses 也使用 `gpt-5.6-sol`，且客户端版本、Mac 系统和桌面构建号与 8 月 14 日失败完全相同。不能把根因简单归为“选了 GPT‑5.6”。
- 8 月 3 日成功链随后真实调用 `/v1/images`，说明当时本地 `image_gen` 路径确实执行；8 月 14 日相同模型和客户端只读取 skill 后虚假完成，没有任何 Images 请求。
- 全站同一桌面构建 26.727.51351 的最后成功图片记录是 8 月 13 日 11:58，早于身份收敛 15:12 开启；开启后没有该旧构建的成功图片样本。该时间关系支持“身份收敛与旧客户端本地图片路径不兼容”，但原始头被清理后仍缺少逐请求直接证明。
- `gpt-5.6-sol` 是当前唯一会保留 Responses Lite 请求头的模型。服务端只有在最终请求含托管 `image_generation` 时才移除 Lite；一旦客户端声明本地 `image_gen`，托管工具被跳过，Lite 头也会保留。
- OpenAI 仓库 #32435 记录的正是 `gpt-5.6-sol` 在 Responses Lite 下的命名空间工具路由异常；#32251 则记录“读取 imagegen skill、未调用工具、虚假报完成”。wenwen 的表现与这两条已知问题组成同一故障链。
- 这套 Lite/本地工具互斥逻辑在 8 月 3 日成功时已存在，因此它是放大器和缺失的备用路径，不是单独的回归起点；回归起点仍指向身份收敛后的出站身份变化或上游 5.6 工具行为更新。

### 根因结论

- 已确认的失败机制：wenwen 的 5.6 Sol 请求读取了 imagegen skill，却没有产生本地 `image_gen` 调用；Aether 因客户端声明本地图片工具而主动关闭托管 `image_generation` 备用路径，最终模型在没有图片结果时虚假报完成。
- 最可能的回归触发：8 月 13 日开启的 Codex OAuth 身份收敛重写了旧版 Mac 客户端的出站身份，同时 Sol 继续走 Responses Lite。本地图片工具路径由此暴露或重新触发 GPT‑5.6 已知的工具命名空间/调用异常。
- 已排除：wenwen 本机两个配置项、Provider 图片开关、Pro号池、Images 端点、所选上游 OAuth 账号能力、服务端显式删除 Actor Authorization。
- 无法由历史数据直接确认：目标请求入站时 Actor Authorization、设备证明和 `tools` 数组的原始值，以及上游最终收到的逐字请求；这些字段已经被生产隐私清理删除。

## 2026-08-14 GPT-5.6 Sol 本地图片工具替换修复

- OpenAI Docs 明确列出 `gpt-5.6-sol` 的 Responses API 托管 `image_generation` 工具为支持状态。
- OpenAI Docs 的托管图片协议是在请求 `tools` 中声明 `{"type":"image_generation"}`，结果以 `image_generation_call` 返回；协议不要求 `X-OpenAI-Actor-Authorization`。
- Aether 当前检测到 `namespace:image_gen` 或 `function/custom:image_gen.imagegen` 后会跳过托管工具注入；这是本次需要改变的互斥行为。
- Aether 已有托管工具注入、Responses Lite 清理和 `image_generation_call` 响应重写能力，本次应复用现有实现，不新增第二套图片协议。
- `X-OpenAI-Actor-Authorization` 在灰度期间继续保留于客户端配置，便于验证替换规则和快速回退；托管工具链路本身不依赖该请求头。
- 已完成请求改写：`gpt-5.6-sol` 请求包含本地 `image_gen` 时，只移除图片工具，保留其他工具，并注入托管 `image_generation`；显式选择已移除图片工具时改为 `auto`。
- 仅在托管图片功能开启且使用标准 Responses 格式时启用；其他模型及关闭托管图片功能的请求维持原行为。
- 最终请求包含托管图片工具时，现有逻辑会移除 `X-OpenAI-Internal-Codex-Responses-Lite`。
- 原始记录链路初查：运行时上下文包含原始请求头/体与上游请求头/体，但用量事件入库前会遮盖敏感请求头；请求体只有 `request_record_level=full` 才完整保存，并受存储和保留策略影响。
- 生产当前 `request_record_level` 为 `basic`，且缺少 `enable_auto_cleanup` 配置，因此按默认值执行自动清理；这会继续丢失请求/响应正文。
- 新增独立配置 `enable_usage_detail_cleanup`：关闭后只停止用量明细自动清理，不影响审计日志、请求候选和节点指标清理；手工清理仍可执行。
- 故障诊断配置采用 `request_record_level=full` 与 `enable_usage_detail_cleanup=false`。Actor Authorization 不在默认敏感头清单中，会按原值保存；认证令牌仍遮盖。
- 生产配置已改为完整记录，请求体和响应体上限均为 16 MiB；兼容当前旧版线上代码，正文、压缩正文和请求头保留期同步延长为 3650 天，请求候选记录仍保持 30 天清理。
- 新的独立清理开关需要随本次代码发布后才生效；发布前由 3650 天保留期保证新记录不会被现有自动清理删除。
- 本地代码和相关测试已全部通过；未在本轮提交或发布业务代码，因此生产请求改写仍是旧行为，Codex Desktop 实机验收必须在发布后执行。

## 2026-08-15 GPT-5.6 图片桥接审查修复

- 官方模型文档确认 `gpt-5.6` 是指向 GPT-5.6 Sol 的别名，因此图片工具替换和 Responses Lite 判定必须同时识别两个名称。
- Responses `tool_choice` 支持 `allowed_tools` 对象，其中 `tools` 是嵌套工具列表；只检查顶层对象会遗留本地 `image_gen` 引用。
- 流式完整记录当前优先写对象存储；对象存储未配置或写入失败时状态为 `Unavailable`，已有内存缓冲却不会写入 `body_base64`。
- `enable_usage_detail_cleanup=false` 当前直接停止整个用量清理任务，连过期 Key 清理也不再执行；正文和请求头依赖父用量记录，因此保留明细时必须同时保留父记录，但过期 Key 清理应继续。
- 流式对象上传失败描述仍会把逻辑正文引用写入顶层上下文；用量正文处理器看到该引用后会删除本地正文并改记为引用。回退修复必须只为 `storage_status=available` 的对象暴露顶层正文引用，失败描述本身仍保留用于诊断。
- 已实现：对象上传成功时继续保存引用；未配置或上传失败时保存受上限约束的本地缓冲，并按实际情况标记完整或截断，同时保留失败描述。
- 已实现：`allowed_tools` 内的本地图片项改成托管 `image_generation`，保留 `mode` 和其他工具并去重；顶层显式本地图片选择继续改为 `auto`。
- 已实现：`gpt-5.6` 与 `gpt-5.6-sol` 共用 Sol 模型判定，图片替换和无托管工具时的 Lite 兼容行为一致。
- 已实现：关闭用量明细清理后，正文、请求头和父用量记录不自动删除，过期 Key 清理继续执行；全局自动清理关闭和管理端手工清理语义不变。

## 2026-08-15 当前生产版本生图问题复核

- OpenAI 官方模型页确认 `gpt-5.6` 是 `gpt-5.6-sol` 的别名，Sol 在 Responses API 中支持托管 `image_generation`。
- OpenAI 官方图片工具指南直接使用 `model: "gpt-5.6"` 与 `tools: [{"type":"image_generation"}]`；成功结果位于 `image_generation_call.result`。
- 当前本地分支停在较早的审计提交，远端跟踪分支显示今天 19:24 的 `origin/main` 为 `f342367ad`；当前工作区仍有上一轮未提交的生图审查修复，不能直接把本地文件当作已上线代码。
- 今天 13:48 的主线历史已包含第一版 Sol 托管图片工具修复和用量明细清理开关；需要继续确认 19:24 最新主线是否包含上一轮审查发现的四项补充修复。
- 已逐文件对比 `origin/main@f342367ad`：最新主线仍只有第一版修复，不包含当前工作区的四项补充改动。
- 主线仍只精确识别 `gpt-5.6-sol`，未识别官方 `gpt-5.6` 别名；`allowed_tools.tools` 内的本地图片项不会同步替换。
- 主线在流式完整记录写对象存储失败时仍生成无效正文引用，内存中已捕获的响应不会回退入库；这会继续削弱故障定位能力。
- 主线关闭 `enable_usage_detail_cleanup` 时会提前结束整个用量清理任务，过期 Key 清理也随之停止。
- 当前正式部署使用同一提交镜像更新 OVH Frontdoor、hd0526 Frontdoor 和 hd0526 单活 Background；容器镜像标签和镜像 revision 可用于核对实际线上提交，不能只看本地 `origin/main`。
- 正式入口目前包含 OVH 与 hd0526 两个 Frontdoor，最新请求核对必须覆盖实际落点，不能只查单台服务器日志。
- 本机可直接只读访问 `hd0526`，OVH 生产主机对应 SSH 别名为 `ovh-US-WEST-OR-VPS-4`；后续版本核对覆盖两台。
- 实际生产版本核对：hd0526 Frontdoor 与唯一 Background 均运行 `f342367ad`，健康且在 19:35 左右重建，和当前 `origin/main` 一致。
- OVH Frontdoor 仍运行旧镜像 `9f2959a28`，健康但未同步今天的新主线；其状态文件显示 `d5ecd7aa`，与实际容器 revision 不一致。`us1` 流量会继续走旧图片行为，发布修复时必须同时修正 OVH 版本和状态文件一致性。
- 数据库已完成今天的主从切换：`rn-hybrid` 上 `niffler-postgres15` 当前为主库，业务库名为 `aether`；`rn01` 的旧 PostgreSQL 容器已停止。后续请求核对只读查询 rn-hybrid。
- 当前 `usage` 表已具备原始请求、最终上游请求、上游响应、客户端响应及各压缩字段，能够直接判断新请求是否完成本地工具替换和图片结果返回。
- `usage.username='wenwen'` 没有直接命中，说明当前记录没有冗余用户名或账号名称不同；需先从 `users` 精确解析用户 ID，再按 `usage.user_id` 查询，不能把空结果当成没有请求。
- 已从 `users` 精确找到 `wenwen`，后续按其用户 ID 做索引查询；不再对整个 `usage` 做无界关联聚合。
- `wenwen` 最近一条使用记录仍停留在 2026-08-14 15:16:07；今天 19:35 左右部署后没有新的测试请求，因此当前线上版本尚未经过 `wenwen` 的真实客户端验证。
- `wenwen` 最近三条相关记录的请求体、上游请求体、响应体、客户端响应体及其压缩字段均为空，旧记录无法还原工具列表和图片返回链路。
- 数据库切换后的当前主库配置不是此前确认过的完整记录状态：`request_record_level` 已回到 `basic`；请求和响应正文上限仍为 16 MiB，`enable_usage_detail_cleanup=false` 以及请求头保留 3650 天仍在。
- 当前主库没有显式 `enable_auto_cleanup`、正文明细保留期和压缩正文保留期记录，这些项目将使用程序默认值；在主线清理实现未补齐前，`enable_usage_detail_cleanup=false` 还会连带停止过期 Key 清理。
- hd0526 与 OVH 的 Frontdoor 都没有配置 `AETHER_USAGE_OBJECT_STORE_URL`。最新主线在对象存储未配置时不会将已缓冲的流式响应回退写入数据库，因此仅把记录级别改为 `full` 仍不能保证保存图片响应原文。
- 图片桥接开关保存在 Provider 或 Provider Endpoint 的 `config` 中；当前库使用 `providers` 与 `provider_endpoints` 两张表，需按 `wenwen` 实际命中的服务核对，不能只看任意一个 Provider。
- `wenwen` 最近请求实际命中 `Pro号池`（`provider_type=codex`）与 `openai:responses` Endpoint；Provider 的 `openai_responses_image_generation_tool_enabled=true`，Endpoint 没有覆盖该值，因此图片桥接开关当前有效，问题不在这个配置项。
- 最后一次线上复查时，hd0526 的 Frontdoor 与 Background 仍为 `f342367ad` 且健康；OVH Frontdoor 仍为 `9f2959a28`，状态文件仍错误记录为 `d5ecd7aa`，版本不一致没有自行恢复。
- 本地 `origin/main` 当前仍指向 `f342367ad`（19:24 的计费修复）；直接访问 GitHub 读取远端引用连续 30 秒无响应，需用 GitHub 页面再确认远端是否有更晚提交。
- 已通过当前登录的 GitHub CLI 直接查询仓库 API：远端 `main` 最新提交仍是 `f342367ad`，与本地远端引用及 hd0526 线上 revision 一致。
- 当前修复应基于 `f342367ad` 新建干净分支，只移植已审查的图片桥接和记录链路改动；不能整体合并当前落后且有未提交内容的工作分支。

## 2026-08-16 号池分配模式与策略组合调度修复

- 管理端明确采用两层配置：分配模式五选一；额度平均、健康优先、延迟优先等策略可组合并排序。修复必须保留这套产品语义。
- 当前核心调度把分配模式放在排序向量首位，并给账号分配唯一名次；向量按字典序比较，因此后续策略事实上无法改变账号顺序。
- 缓存亲和的无绑定排序使用最近使用优先，成功后又刷新所选账号的最近使用时间，形成持续选择同一账号的正反馈。
- 生产近期请求没有可提取的会话标识，却仍集中到同一账号；这不是显式会话绑定带来的合理缓存亲和。
- 正确边界是：有效的显式缓存绑定属于硬命中；没有绑定时，先按用户启用的策略顺序比较，再用分配模式决定仍相同的账号。
- 当前分页默认每页 64、最多扫描 512；Pro 号池现有 55 个活动账号，因此分页不是这次线上集中的直接原因，但现有懒加载会让超过首批账号的更优策略值无法参与首轮选择。
- 需要分别验证五种分配模式：缓存亲和保留显式绑定；LRU 选择最久未用；单号优先维持稳定复用；负载均衡维持请求级分散；优先级优先维持人工优先级。它们都只能在策略无法区分账号时决定最终顺序。
- 网关游标在普通分页前还会读取评分系统的前 `score_top_n` 个账号；达到窗口数量后立即调度，同样会让后续页面的额度、健康和延迟策略数据无法参与首轮选择。
- 分页修复不能只扩大普通查询页数；需要合并评分阶段和普通页面、按账号编号去重，并在最大扫描范围内完成统一策略排序。
- 成功反馈当前只要启用任意策略就刷新 LRU。LRU 时间确实被缓存亲和、LRU 和单号优先使用；负载均衡、优先级优先或纯策略本身不需要该运行时写入，后续需按分配模式收窄，避免无关状态持续影响切换后的行为。
- 五种分配模式的独立排序实现与数据库基础查询方向一致：缓存亲和按最近使用、LRU 按最久未用、单号优先按内部优先级再最近使用、负载均衡按请求种子散列、优先级优先按内部优先级。共同缺陷是它们都曾排在叠加策略之前，不是五套各自不同的算法错误。
- 运行时 LRU 状态现已确认只被缓存亲和、LRU 和单号优先读取；负载均衡与优先级优先可以停止读写该状态，不影响各自分配结果。
- 网关原先会在号池统一调度之前直接返回缓存绑定账号，因此旧绑定只校验运行时冷却，没有检查目录中的封禁、额度耗尽、临时不可用和成本上限。缓存绑定必须先经过与普通账号相同的完整调度过滤，才能称为有效绑定。
- 最终行为是：有效缓存绑定先复用；没有有效绑定时按策略调度的保存顺序逐项比较；指标相同继续比较下一项；最后才由五选一分配模式决定顺序。显式策略会在配置的最大扫描范围内跨页统一比较。
- 后续评审发现 `pool_policy_overrides` 原先只改变账号读取顺序和扫描范围，最终选择仍重新读取号池基础设置。当前管理页面没有这个编辑入口，但接口或导入配置可以写入该字段，因此仍需保证覆盖设置真实生效。
- 全量读取判断必须同时满足“用户明确开启、当前服务商支持、核心调度能够执行”。服务商自动补充的默认策略不能算作用户明确开启，否则只配置 LRU 的大号池也会从单页读取退化为读取最大扫描数量。
- 最后一处不一致发生在请求成功之后：账号选择已经使用路由覆盖设置，成功记录却重新读取号池页面设置，导致缓存绑定和 LRU 更新时间可能按另一种分配模式执行。
- 修复方式是让本次请求实际采用的路由覆盖设置随执行计划进入成功记录；号池页面设为缓存亲和、路由改为负载均衡时不再保存缓存绑定，反向覆盖时会按缓存亲和保存。

## 2026-08-16 WorkBuddy 调用 Niffler GPT 报错诊断

- 报错发生于 2026-08-16 11:44:54（UTC+8），用户 ID 为 `49e6581a-6979-4cf5-b622-ed6870ccdf10`。
- 外层 Request ID 为 `08a8b8e928184de69ba768fcfedeb90e/1a6e825d-b004-4906-b46e-e841722582df`；Trace ID 与冲突的 `request_id` 均为 `7eb0664447224e01ac5a5ec5f9d00644`。
- 服务端明确报告：同一 `request_id` 已存在不同的 `billing admission`（计费准入）；待从代码和生产记录确认哪两个准入值冲突、为何请求记录仍显示成功。
- 本轮只读排查，不修改线上状态。
- 精确错误由 `validate_stored_admission_matches_input` 产生。数据库已存在同一 `request_id` 时，代码会逐字段比较用户、API Key、钱包、全局模型、资金来源、准入时钱包余额、钱包支付/补差许可、套餐权益、供应商范围和版本；任一字段变化都会拒绝第二次写入。
- 当前比较包含 `wallet_balance_at_admission` 的精确数值相等。即使用户、模型、套餐和供应商完全相同，只要第二次解析出来的钱包余额与首次保存值有微小差异，也会得到这条错误。
- `resolve_request_billing_admission` 在没有复用现成准入上下文时，会重新读取钱包与套餐额度，并把当时 `balance + gift_balance` 写入准入。若同一请求标识再次走到这里而未携带首次 `billing_admission`，重算值可能与数据库首次记录不同。
- 代码允许重试复用首次准入，前提是重试上下文携带已保存的 `billing_admission`；需要继续追踪 WorkBuddy 请求为何进入重新计算分支，以及生产记录中究竟是哪一字段不同。
- 该逐字段精确比较自 2026-08-10 引入后没有修改；2026-08-13 的 PostgreSQL numeric 热修只修正数据库数值读取，不是本次冲突检查的新变化。
- 当前生产主线 `f342367ad` 仍包含这套比较逻辑。工作区没有对准入比较、PostgreSQL 准入写入或准入解析的未提交修改，因此可以用本地代码解释线上行为。
- PostgreSQL 写入使用 `ON CONFLICT (request_id) DO UPDATE SET request_id = EXCLUDED.request_id`：发生重复时不会覆盖第一次准入，只返回原记录再逐字段校验。由此可确认数据库保留的是首次成功准入，错误来自后续同标识的冲突尝试。
- 请求执行前会优先从 `report_context.billing_admission` 读取准入；Planner 正常会把同一轮解析出的准入放入每个执行计划的上下文，因此普通的同请求换账号重试应复用相同准入，不应自行重算。
- 另一条代码路径会在 Planner 没有附带 `billing_admission` 时重新读取钱包与套餐并生成准入。后续要结合该请求的候选记录和日志，判断是否走了这条旧路径，或请求标识被跨独立调用复用。
- 网关的 `extract_or_generate_trace_id` 会无条件采用请求头 `x-trace-id`；仅在该头缺失时调用 `Uuid::new_v4().to_string()` 生成带连字符的 UUID。该 Trace ID 随后直接成为 `billing_request_admissions.request_id`。
- 本次冲突值 `7eb0664447224e01ac5a5ec5f9d00644` 是 32 位无连字符十六进制，不符合 Niffler 自生成 UUID 的格式，说明它来自 WorkBuddy、WorkBuddy 前置服务或外层代理传入的 `x-trace-id`。
- 已确认可只读连接当前数据库主机 `rn-hybrid` 和主 Frontdoor `hd0526`；OVH 的实际 SSH 别名为 `ovh-US-WEST-OR-VPS-4`，下一步关联三处生产证据。
- 生产日志形成完整时间线：03:44:46.856 UTC，hd0526 收到 Trace ID `7eb066...0644`，Niffler 用户 `855ba931-...`、API Key `workbuddy`，鉴权余额为 `492.8047`；03:44:48.967 该请求以 HTTP 200 完成，耗时 2147 ms。
- 仅约 5 秒后，03:44:53.984 UTC，hd0526 再次收到完全相同 Trace ID、同一用户和 API Key，但鉴权余额已为 `492.6940`；03:44:55.270 写候选和计费准入时触发冲突，03:44:55.272 以 HTTP 500 结束。
- 数据库 `billing_request_admissions` 只保留首次准入：创建于 11:44:47.998（UTC+8），资金来源为钱包，`wallet_balance_at_admission=492.80474626`，状态 admitted。第二次请求重算的余额约为 492.6940，与首次保存值不同，足以触发精确比较失败。
- 两次请求均落在 hd0526；同期 OVH 没有该 Trace ID。外部负载均衡跨节点不是本次原因。
- 用户错误报告中的 App User ID `49e6581a-...` 不是 Niffler 数据库用户 ID；Niffler 日志对应的实际用户是 `855ba931-...`。前者应属于 WorkBuddy 自身用户体系，不能直接拿它查询 Niffler 用量表。
- `usage.request_id` 有唯一索引；同一个 Trace ID 最多只有一条管理端请求记录。第二次请求又在 `ensure_execution_request_candidate_slot` 阶段失败，发生在模型调用和用量记录生成之前，因此管理端只显示第一次成功记录，符合当前实现。
- `request_candidates` 按 `(request_id, candidate_index, retry_index)` 唯一；精确查询该 Trace ID 可进一步证明数据库只留下第一次调用的执行候选。
- `usage` 精确查询只有一条：`gpt-5.6-sol` 流式请求，状态 completed、HTTP 200，用户计费 `0.11072075` 美元，创建于 11:44:48。该金额与两次鉴权余额差约 `0.1107` 完全吻合，证明第一次请求结算导致第二次看到的余额变化。
- `request_candidates` 也只有第一次调用的一条成功候选（candidate_index=0、retry_index=0、status=success、HTTP 200）。第二次请求没有插入候选，和日志中的“候选槽写入失败”位置一致。
- 因此“请求记录里都是成功”不是记录状态错误：成功记录代表第一次实际调用；第二次失败在正式请求记录建立之前，管理端没有相应失败行可显示。
- 当前主库确认 Niffler 用户 `855ba931-...` 的用户名正是 `kaige`。
- 两台生产 Caddy 配置都没有生成或覆盖 `x-trace-id`，可以排除 Niffler 自有反向代理制造重复 Trace ID。
- 首次成功请求的 HTTP 审计明确保存：`x-trace-id=7eb066...0644`、`traceparent=00-7eb066...0644-60e7c3f9eb2f3aac-01`、`x-request-id=2c19984c...`，User-Agent 为 `WorkBuddy/5.3.13 ... CLI/2.115.0`。因此该 Trace ID 和 W3C traceparent 确实由 WorkBuddy 请求带入。
- 用户错误报告对应第二次调用的 Request ID 前缀为 `08a8b8e9...`，与第一次成功请求保存的 `x-request-id=2c19984c...` 不同，但 Trace ID 相同。说明这不是同一个 HTTP 请求的内部账号重试，而是 WorkBuddy 在同一分布式追踪下发起了第二个独立 HTTP 请求。
- 根本契约冲突已经明确：WorkBuddy 按追踪规范让多个相关请求共用一个 Trace ID、各自使用不同 Request ID；Niffler 却把可复用的 Trace ID 当成每次计费唯一的幂等键。第一次结算后余额变化，使第二次的准入内容不同并被拒绝。
- 当天 hd0526 还出现过其他相同错误，其中部分请求 ID 是 Niffler 自生成的带连字符 UUID，说明“准入逐字段精确比较可变余额”在内部重试场景也有更广的隐患；这不改变 `kaige` 本次事故的直接证据链，但修复时不能只针对 WorkBuddy User-Agent。
- 建议修复边界：计费幂等键必须改用 Niffler 为每个入站 HTTP 请求生成的服务端唯一 ID；外部 `x-trace-id` 和 `traceparent` 只用于链路追踪。已保存准入的同请求内部重试应直接复用首次准入，不能重新读取可变钱包余额后做精确相等比较。

### 系统性复审新增结论

- 当前入口只有一个 `trace_id` 概念：访问日志中间件从外部 `x-trace-id` 读取或生成后，又把它写回请求头；下游代理、控制决策、执行计划、候选、用量和计费都继续使用这个值。代码没有独立的服务端请求身份对象。
- `x-aether-control-request-id` 只是执行结果回传到访问日志的内部响应头，不是入口生成的独立请求 ID；当前它最终仍来自执行计划的 `request_id`，没有解决 Trace ID 与业务请求 ID 混用。
- 入口中间件只在外部没有 `x-trace-id` 时生成 UUID；外部值无需可信代理标记、格式校验、长度归一或唯一性隔离，客户端可以稳定制造计费主键碰撞。
- 该混用不只影响计费：`usage.request_id`、候选唯一约束、本地运行时诊断清理、报告关联和响应控制头都共享同一个外部 Trace ID。仅放宽准入比较会把碰撞继续扩散到用量覆盖、候选冲突和诊断串线。
- 系统方案必须引入服务端 `request_id`，不能只修改 `validate_stored_admission_matches_input` 或忽略 `wallet_balance_at_admission`。
- `GatewayPublicRequestContext` 当前只有 `trace_id`，没有 `request_id`、客户端请求 ID 或幂等键；大量公共、管理和内部路由共享该结构，身份拆分会影响面广，必须通过统一类型一次性收口，不能在各模型格式中零散生成。
- AI Planner 的 Chat、Responses、图片、视频、文件和通用格式都显式执行 `request_id: trace_id`；用量报告又以同一值作为事件主键并把它写回 `request_metadata.trace_id`。因此修复需覆盖全部模型格式和内部 finalize 报告，不是只改 `/v1/chat/completions`。
- 已有候选 UUID、`candidate_index`、`retry_index` 可以继续表示上游执行尝试；无需再用 Trace ID 兼任尝试 ID。
- 已复核一个 Niffler 自生成 UUID 的同类错误 `029bd71e-...`：同一个 Claude 请求依次执行 3 个候选，分别返回 503、503、500；候选耗尽后系统继续尝试扩展/重建执行计划，重新写同一请求的计费准入时发生冲突。
- 该请求的 3 个候选都失败且用户费用为 0，数据库准入余额没有被本请求结算改变。因此这类内部冲突不能用“第一次扣费后余额变化”解释，证明还存在第二个独立根因：同一服务端请求在后续候选批次重新计算准入，而不是从数据库或首次上下文复用不可变准入。
- 候选持久化的 `extra_data` 会剔除 `billing_admission`，生产库无法事后直接比较两次完整输入；现有错误日志也不记录差异字段。当前可确认流程缺陷，尚不能仅凭日志断言具体变化的是浮点余额、套餐范围还是其他快照字段。
- 系统修复必须同时处理两类问题：外部 Trace ID 跨请求复用，以及内部候选分页/重规划重新计算准入。只拆分 request_id 仍会留下第二类故障。
- 每个执行候选进入同步或流式执行时都会调用 `ensure_execution_request_candidate_slot`，该函数同时解析并写计费准入。动态候选源跨页继续产出新尝试时，也会再次经过该入口；计费准入并没有在“请求开始”阶段只创建一次。
- 用量 pending 记录反而在候选槽与准入写成功之后才创建，所以准入冲突天然不会留下完整失败用量。这再次说明请求生命周期、计费准入和候选尝试的建立顺序需要重构。
- 动态分页游标会按全局模型缓存一次 `BillingProviderRoutingScope`，同一游标内不同页面不会主动重读钱包；每个候选的 report context 也会携带由该 scope 生成的准入。因此刚才的内部 UUID 冲突更可能发生在候选循环耗尽后的另一条 fallback/重新规划链，而不是同一分页游标自身刷新。
- 无论具体 fallback 在哪里，架构缺陷已经成立：准入的创建/复用依赖每条计划是否恰好携带 report context，而不是请求级统一所有者。系统方案应先建立请求级准入，再让所有 Planner 和 fallback 只引用它。
- 代理主流程在本地候选耗尽后，会继续进入 control-execute 应急 fallback；fallback 会重新构建一份计划并再次执行候选槽/准入写入。刚才 UUID 样本的错误紧跟在 `candidate_loop_exhausted` 后，和这条路径完全吻合。
- 本地候选耗尽时本可记录一条失败用量，但 control fallback 的准入冲突以 `GatewayError` 提前返回，绕过了 `record_failed_usage_for_exhausted_request`，最终旧 pending 用量后来被维护任务标成“超过 10 分钟未完成”。这就是另一个可观测性问题：执行阶段已明确失败，管理端却显示延迟生成的 504 超时。
- 更精确地说，单次 `run_ai_sync_execution_path` 在已有 Exhausted 结果时不会进入 plan fallback；重复来自代理层随后调用 `maybe_execute_via_control`。该函数克隆 control decision、刷新鉴权上下文后，再次调用同一个 `maybe_execute_sync_request`/`maybe_execute_stream_request`，等于重新跑完整本地执行链。
- 因此内部 UUID 冲突的直接流程根因是：第一次本地候选已耗尽，代理层的 control-execute 应急路径把同一 HTTP 请求当成新一轮规划，重新读取计费 scope，再使用同一 request_id 写准入。
- 这条应急路径本身也需复审：若没有新的执行节点、不同路由或明确恢复条件，直接重复同一候选链只会增加上游调用和延迟。系统方案应规定 fallback 只能消费同一个请求上下文与准入，并避免重试已失败的相同候选身份。
- control-execute 应急路径仅在客户端显式发送 `x-aether-control-execute-fallback: true` 时启用，不是所有请求都会重复执行。受影响客户端大概率是使用 Niffler/Aether 扩展协议的工具，系统修复仍不能按 User-Agent 特判。
- 当前测试重点验证应急执行不再调用旧的远程 `/execute-*` HTTP 接口，却没有覆盖“本地候选已经执行并耗尽后，不应再次运行相同候选”以及“第二轮必须复用首次准入”的契约。
- 纠正上一轮过早判断：现有证据不能把 UUID 样本唯一归因于代理层 control-execute。其 HTTP 审计没有保存该 opt-in 头；同时 `run_ai_sync_execution_path` 在某个步骤返回 Exhausted 后仍会继续后续本地步骤（Standard Family、Same Format Provider 等），这些步骤也会各自重新构建候选和准入。错误也可能在同一次 execution path 的下一个重叠步骤发生。
- 可以确定的更深层根因是“一个 HTTP 请求包含多段可重入规划链，但计费准入没有请求级统一所有者”。control-execute 只是可能的第三层重复入口，不能作为唯一修复点。
- 已确认规划步骤存在实质重叠：`LocalStandardFamily` 的候选源会扫描标准候选，构建 payload 时若上下游格式相同又直接委托 same-format 逻辑；execution path 随后仍会单独运行 `LocalSameFormatProvider`，它会重新扫描同一 API 格式候选。首轮耗尽后，同一请求可能再次遇到同一服务和账号。
- 因而内部 UUID 样本最合理且有代码支撑的直接路径是 StandardFamily 耗尽后进入独立 SameFormatProvider 步骤，第二套 Planner 重新读取计费 scope 并触发冲突。修复需要让执行步骤互斥或共享统一的已尝试集合，不能仅让准入冲突“通过”。
- 计费数据契约把 `wallet_balance_at_admission` 定义为 `f64`，PostgreSQL 钱包和准入列则是定点 `numeric`，读取时多处强制转换为 double precision；校验又执行精确 `==`。这会把审计快照误当身份字段，并引入数据库舍入/浮点表示风险。
- 现有正式计费文档已经明确两条当前代码违反的规则：同一请求分批读取服务时必须复用首次资金结果；相同请求重试必须返回已保存准入，不能重新计算。此次修复属于恢复既定契约，不是新增业务规则。
- 现有 AI 请求可见性文档也要求所有进入公共 AI 入口并产生响应的请求都写入 `usage`。准入冲突直接经 `GatewayError` 返回而没有终态记录，属于已实现流程与设计文档不一致。
- 因此无需另建一套管理页面或重复的请求日志表。更合适的做法是让现有 `usage` 真正承担请求生命周期：每个服务端 request_id 先建立一条最小 pending 记录，所有正常、拒绝和内部错误路径都在统一终态守卫中完成或作废。
- 预估金额预留也使用当前 `request_id` 生成稳定 UUID 和幂等键。身份拆分必须覆盖 `billing_reservations`，否则修复准入主键后仍会让同 Trace ID 的独立请求共用同一笔预留。
- 最终身份模型应明确分为四层：服务端 `request_id` 唯一表示一次入站 HTTP 调用；`trace_id` 只关联分布式链路；`client_request_id` 只用于排查客户端调用；显式 `idempotency_key` 才表示客户端希望重放同一业务请求。默认不能根据 Trace ID 或客户端 Request ID 去重或复用扣费结果。
- 公共入口必须始终生成服务端 `request_id`，并剥离客户端伪造的内部请求 ID。跨 Frontdoor、隧道或内部执行时只能通过受信任内部头传播该 ID；响应可返回它供管理端检索。
- 请求级执行上下文必须持有首次生成或数据库返回的 `billing_admission` 和 `attempted_candidates`。首次候选仍在同一数据库事务中写准入与候选，避免增加 OVH 到数据库的顺序往返；同一进程的后续步骤直接复用上下文，跨节点或上下文丢失时读取已有准入，而不是重新查询钱包和套餐。
- 并发重复写入采用“插入失败后读取已保存准入”的语义。只对用户、API Key、钱包、全局模型和结构版本等真正所有权字段检查冲突；资金来源、准入余额、套餐权益和供应商范围属于首次决定的快照，后续直接以已保存值为准，不能拿当前状态重新计算后比较。
- 金额字段应使用定点 Decimal 或统一的最小货币单位，不能继续在 PostgreSQL `numeric` 与 Rust `f64` 之间做精确相等。该调整用于金额正确性和审计一致性，但不能替代请求级准入复用。
- `LocalStandardFamily` 与 `LocalSameFormatProvider` 需要改为互斥覆盖，或共享请求级已尝试集合。候选身份至少由供应商、Endpoint、Key、映射后模型和执行策略组成；后续规划、分页和 control fallback 不得再次调用已失败的同一路径。
- `usage` 应在 AI 请求确认进入网关后建立最小 pending 记录，并由请求级终态守卫统一收尾为 completed、failed 或 cancelled；未调用上游、无需计费的失败写 `billing_status=void`。现有候选级守卫继续负责单次尝试，不能替代请求级收尾。
- 管理端继续使用现有使用记录页面，但查询应同时支持服务端 Request ID、Trace ID 和客户端 Request ID。第二次准入失败这类错误必须立即显示为 failed/void，不能依赖十分钟后的 stale pending 任务改写成 504。
- 数据库采用加法迁移：保留现有 `request_id` 主键，新增可索引 `trace_id` 和 `client_request_id`，必要时在元数据中记录 `identity_version`。历史数据不改写；旧记录按原规则查询，新记录由服务端 Request ID 关联准入、候选、用量和预留。
- 两台 Frontdoor 的上线顺序必须是先执行兼容迁移，再部署同时支持新旧身份版本的代码，最后通过共享开关统一切换。Background 和内部隧道必须先能识别新的服务端 Request ID；不得单节点先改变主键语义。
- 可独立支持客户端幂等，但不应阻塞根因修复：显式 `Idempotency-Key` 按用户、API Key、路由和请求体摘要限定范围；同键同正文返回已有结果或处理中状态，同键不同正文返回 409。流式响应重放需要单独设计。
- 验收必须覆盖：同 Trace ID 的两个 WorkBuddy 请求各自成功并生成两条记录；同请求多规划步骤只生成一份准入；多节点并发写返回同一准入；钱包或套餐在重试间变化仍复用首次决定；相同候选不重复执行；准入前失败立即出现 failed/void；PostgreSQL、MySQL、SQLite 和内存实现语义一致；请求首包耗时不新增顺序数据库往返。
- 实施复核确认公共代理入口只有一份 `GatewayPublicRequestContext`，适合在这里一次性保存内部调用编号、客户端跟踪号和客户端请求号。访问日志继续使用客户端跟踪号关联链路，模型执行、使用记录、收费准入和金额预留改用内部调用编号。
- 入口并发限制发生在请求上下文创建之前，也必须生成独立内部调用编号，否则相同客户端跟踪号下的两次入口拒绝仍会覆盖使用记录。内部调用编号应在进入并发限制前创建，并贯穿整个代理处理函数。
- 现有响应完成逻辑已经从执行响应头读取内部 Request ID 写访问日志，因此可以让成功、失败和入口拒绝都显式返回内部调用编号，同时继续保留原 Trace ID 响应头。
- 首次收费决定中，用户、密钥、钱包和全局模型属于不可变化的归属信息；余额、付费来源和可用服务范围属于当时快照，内部换服务时必须使用已经保存的第一次结果。
- 客户端跟踪号继续写入使用记录的 `trace_id`，新的内部调用编号用于使用记录主键、收费预留和服务执行。
- 实现时发现旧的远程规划或执行服务可能在结果中返回自己的请求号；如果直接相信该值，外部编号仍可能重新进入使用记录。网关现在统一以入口生成的内部调用编号为准，远程服务返回值只作为执行结果内容处理。
- 使用记录必须在建立服务执行记录和保存收费决定之前写入处理中状态，否则收费决定保存失败时依然没有记录。同步与流式执行都已调整为先登记，再保存收费决定和服务信息，失败时立即完成记录。
- 服务去重不需要新增数据库表：同一 HTTP 调用的所有本地规划步骤和应急执行共用一份内存清单；跨节点时只有实际负责执行的节点持有该清单。服务执行记录继续由数据库保存最终事实。
- 为了让管理员仍能用 WorkBuddy 跟踪号串起使用记录和服务执行记录，客户端跟踪号与客户端请求号同时保存到两类记录的现有扩展信息中。本次无需修改表结构；后续只有查询量足够大时才需增加专用索引。
- 现有 Codex OAuth 指纹收敛测试通过，说明入口调用编号变化没有改变上游账号、设备、会话和任务身份。

## 2026-08-16 WorkBuddy 请求身份与收费修复代码审查

- 多节点隧道转发不会直接产生两条使用记录：入口节点虽然先生成一个内部编号，但不执行模型也不写使用记录；实际执行节点重新生成编号并通过响应返回，入口最终保留执行节点编号。该行为有额外编号浪费，但暂未构成数据错误。
- 收费复用仍有一个需要继续核实的缺口：数据库写入会返回第一次保存的收费决定，但后续规划步骤在筛选可用服务前仍会重新读取当前钱包和套餐。若同一次调用的余额或套餐在自动换服务期间变化，后续规划可能提前排除第一次收费决定原本允许的服务，尚未真正做到整次调用复用第一次决定。
- 收费决定或服务记录保存失败时，同步和流式执行会先写失败使用记录，然后把错误直接返回给 Axum。该错误没有经过代理层的统一响应完成逻辑；`GatewayError::Internal` 也不写内部调用编号或跟踪号响应头。因此这类失败虽然已经有数据库记录，客户端和访问日志仍拿不到用于精确查询的内部调用编号，多个调用共用同一跟踪号时仍难以确定是哪一条失败。
- 同步图片请求在默认开启的空白心跳模式下，不经过统一的服务执行循环，而是直接逐个调用执行服务；本轮新增的重复服务清单只放在统一循环中。因此这条默认路径没有重复服务保护。当前测试只覆盖两个不同地址，尚未证明同一地址重复出现时会被阻止。
- “先登记再执行”目前仍发生在已经生成执行计划之后，而且 `record_pending` 只把写入任务交给后台线程，不等待数据库确认。服务筛选、钱包/套餐读取或规划阶段在生成第一条执行计划前报错时，代理仍会直接返回错误而没有使用记录；即使已经进入执行阶段，上游调用也可能早于处理中记录实际入库。实现尚未达到行为文档所写的“确认是模型调用后登记、第一次调用上游前必须已经存在记录”。
- 代理主流程多处使用 `await?` 直接上抛规划或执行错误；只有正常返回结果、明确无服务和已耗尽分支会进入统一响应完成逻辑。因此前述内部编号缺失不是日志展示问题，而是错误响应确实绕过了写响应头和访问日志内部编号的函数。
- 正常模型响应也可能暴露错误编号：上游响应过滤没有删除 `x-aether-control-request-id`，执行层和代理完成层都采用“已有就保留”的写法。只要上游服务返回这个同名头，客户端和访问日志就会使用上游提供的编号，而使用记录与收费仍使用网关内部编号。隧道转发确实需要保留执行节点编号，但普通上游响应不应拥有覆盖内部编号的权限。

### 最终结论

- P1：后续换服务仍会重新读取当前余额和套餐范围；数据库只在保存时返回第一次收费决定，没有让后续服务筛选复用它。
- P1：规划、收费决定或服务记录失败时会直接返回通用错误，绕过统一响应和失败收尾；客户端拿不到内部调用编号，部分失败也没有最终使用记录。
- P1：“处理中”记录在生成执行计划后才提交，而且只交给后台任务，不等待数据库写入成功；收费预留和上游调用可能早于使用记录。
- P2：普通上游响应可以保留并覆盖网关的内部调用编号，造成客户端、访问日志与收费记录的编号不一致。
- 重复服务拦截在普通同步和流式执行中正常共享，明确增加重试次数也能继续执行；默认同步生图心跳路径没有经过这份清单，需要另补一条覆盖用例。

### 审查修复实现核对

- 收费范围目前只缓存在单个分页游标里；新的规划步骤仍会重新读钱包和套餐。修复需要一份随整个 HTTP 请求共享的收费范围，不能只依赖某个执行计划携带的数据。
- 代理入口已经在最开始生成内部调用编号，但并发门关闭、请求信息解析、限流检查、收费准备和本地执行等多处错误仍直接向上返回，确实会绕过统一响应收尾。
- 现有 `record_pending` 只启动后台任务且不返回写入结果；要保证先有“处理中”记录，必须新增可等待的写入方法，并在首次收费判断和上游调用前调用。
- 上游响应头目前会被原样复制，内部调用编号仅在响应缺失时补充；因此同名上游头可以覆盖网关编号。普通上游响应需要直接丢弃该头，最终响应必须无条件写入本次内部编号。
- 修复采用请求级共享收费范围：第一次读取钱包和套餐后立即保存，后续所有规划步骤和应急路径读取同一份结果；若同一次调用的用户、密钥或模型发生变化，直接停止处理。
- 公共模型请求在进入执行前同步写入最小使用记录；数据库确认失败时不调用模型。平台直接处理、拒绝和统一错误记录也改为等待最终写入，不再只启动后台任务。
- 代理入口现在统一接住内部错误，使用已生成的内部调用编号完成响应和失败记录；已有完整请求信息时保留用户与密钥信息，不再退化成匿名失败记录。
- 普通上游的 `x-aether-control-request-id` 会在响应复制阶段删除；隧道所有者返回的编号通过仅限内部代码路径的标记保留。
- 同步生图心跳此前绕过重复服务清单；现已在真正调用前登记并跳过重复路径，相关测试证明相同服务只调用一次。

## 2026-08-16 迁移观察页现状复核

- 该页最初用于 Niffler Core 分阶段迁移：按 Key 或产品策略小范围启用新路径，并对照新旧路由、结算和返利结果，因此放在“迁移观察”而不是日常业务设置。
- 新调度、错误提示和返利流水开关仍会改变真实业务；结算快照与路由记录主要是旁路对账证据，不改变实际扣费结果。
- 钱包预占已于 2026-08-10 退出请求流程，运行时无条件关闭；当前前端仍显示可保存的开关，与真实行为不一致。
- 页面的“保存后不影响线上”提示也已过时：除钱包预占外，其他多个开关已接入运行流程。
- 管理接口返回的“仅预览”和保存记录里的“只做旁路观察”标记也与当前运行逻辑不一致，不能继续作为真实影响说明。
- “新调度”会改变模型权限、服务账号范围和销售倍率；“错误提示”会改写用户实际收到的错误；“返利流水”会决定是否生成返利账本。返利表中的重试、取消按钮还会直接修改业务状态，因此它们都不应藏在迁移看板里。
- 当前一致性检查无条件要求每条结算快照都有钱包预占；预占功能停用后，新记录会被标成“缺少预占”，已经不能准确反映现行业务是否正常。
- 结论：不长期保留“迁移观察”这个页面，也不能直接整页删除。先删除无效的钱包预占开关并修正一致性检查；将新调度、错误提示、返利开关和返利处理操作迁到对应业务页面；路由记录、结算快照和历史预占只保留为只读运维记录。等旧链路完全停止且历史留存要求满足后，再移除迁移入口和只服务于旧迁移的对照逻辑。

## 2026-08-16 main 提交与生产发布核对

- 当前工作区位于 `codex/fix-nested-chatgpt-auth-import`，不是 `main`；本地 `main` 落后 `origin/main` 53 个提交。
- 当前分支包含一组与 `origin/main` 内容相近但提交编号不同的历史提交，不能直接强制移动 `main` 或覆盖远端。安全做法是先把当前未提交改动保存成普通提交，再将该提交放到最新 `origin/main` 之上，解决可能出现的真实冲突。
- 未提交范围共 63 个文件，包含请求身份与收费、号池分配、生图桥接、正文保存、测试和架构记录；用户明确要求提交全部本地改动。
- 未跟踪文件只有两份架构文档和一张 48 KiB 的 Niffler 微信头像；`.env` 与 `.DS_Store` 没有出现在待提交清单中。
- 项目正式生产路径是：推送 `main` 触发应用镜像工作流，取得指定提交的 CI 镜像产物，再通过受限生产发布脚本部署；旧的 `deploy.sh` 明确要求镜像先由 GitHub Actions 构建，不能用本机临时镜像替代。
- 进一步核对发现应用镜像工作流当前只在推送 `test` 时自动运行；`main` 需要手工启动 `Build App Image`，随后再手工启动 `Deploy Production` 并传入当前 `main` 的完整提交编号。
- 当前分支与 `origin/main` 从较早共同提交分开：远端一侧 42 个提交，当前分支一侧 9 个提交，且双方有多处同文件改动。提交当前工作区后必须在最新 `origin/main` 上逐项解决冲突，不能把当前分支整体合并进 `main`，否则会重复带入已被远端替代的历史提交。
- GitHub CLI 在该工作区会默认识别上游 `fawney19/Aether`；目标仓库是 `ryfineZ/Niffler`，后续工作流查询和启动必须显式指定目标仓库。

## 2026-08-17 三线路测速生产发布

- 本地 `main` 工作区在发布计划记录前保持干净，相对 `origin/main` 领先两个已验证提交：账号表空列修复与三线路测速入口。
- 本次应用变更不包含数据库迁移；新增生产前置条件是 DMIT Caddy 必须先提供允许跨域、禁止缓存的 `/__niffler_latency` 204 响应。
- 发布继续使用目标仓库 `ryfineZ/Niffler` 的正式 GitHub Actions；准确工作流参数和当前生产镜像仍需在任何写操作前重新核对。
- `origin` 的读取与写入地址均为 `https://github.com/ryfineZ/Niffler.git`，GitHub CLI 当前以仓库所有者账号登录并具备 `repo` 权限。
- 实际镜像工作流文件是 `.github/workflows/app-image.yml`，生产发布文件是 `.github/workflows/deploy-production.yml`；生产发布只接受当前 `main` 的完整 40 位提交号。
- 当前工作区新增的未提交内容只有本轮 `task_plan.md`、`findings.md`、`progress.md` 发布记录，业务提交仍保持不变。
- GitHub API 显示远端 `main` 已前进到 `53c7eda766c56159f88c0927fe80cc4032bd40a3`（支付回调实付金额修复）；本地已完成 fetch，目前相对 `origin/main` 领先两个业务提交、落后该一个生产提交，推送前必须先整合。
- 远端 `53c7eda76` 的 `Build App Image` 运行 `31952627950` 和 `Deploy Production` 运行 `31953155532` 均已成功，说明它是当前生产基线，三线路版本必须包含它。
- `Build App Image` 在 `main` 上使用无输入的 `workflow_dispatch`，产物名为 `niffler-app-linux-amd64`；`Deploy Production` 要求输入当前 `main` 的完整 40 位提交号，并由固定部署器执行迁移兼容、容器健康和自动回退检查。
- 发布前生产状态存在版本漂移：hd0526 运行 `53c7eda76`，OVH 仍运行 `00716100e`；两边 Frontdoor 均健康且没有重启，正式发布必须把两边统一到同一准确镜像。
- DMIT 活动配置尚未包含测速处理器，`cn.niffler.org/__niffler_latency` 当前返回网页且缺少跨域与禁缓存响应头；仓库候选配置正确，但必须先备份、验证并热加载，才能发布三线路首页。
- 远端生产基线已通过普通 merge 纳入本地 `main`，合并提交为 `42fda987d`；支付修复、账号表修复和三线路测速三个提交均为其祖先，自动合并没有产生冲突。
- 只读生产审计的脱敏命令发生失误，工具输出意外包含连接凭据。凭据不写入仓库或后续报告，本次上线后需要单独轮换 PostgreSQL 与 Redis 凭据。
- 合并后验证通过：前端三组 27 项、数据层支付回调 7 项、网关支付 53 项，共 87 项测试通过；前端类型检查、生产构建和 Rust 全工作区全目标编译也通过。
- GitHub 生产工作流只能更新 hd0526 的 Frontdoor 与 Background；OVH 必须复用同一 CI 产物并调用现有固定部署器执行 Frontdoor-only 发布，显式使用本机 `18084` 和 `us1` 公网健康地址。

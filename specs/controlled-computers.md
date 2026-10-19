# Qunica 受控电脑 Spec

Status: proposed
Version: 0.7
Since: 2026-10-17
Implementation: M2a/M2b development implemented locally; final regression recorded separately; external device/VPS/model/performance acceptance pending

跨阶段范围、已确认取舍与里程碑见 [总索引](server-sync-computer-automation.md)。

实现进度（2026-10-19）：M2a/M2b 开发主路径已接入。独立 Supervisor、显式电脑绑定、受控文件/Shell、Chromium + Playwright MCP、noVNC 接管、持久 home 及停机备份/恢复均有生产路径；出口代理阻断控制面、Docker 子网与元数据，接管和超时保留不确定执行边界。OpenCode 是当前支持的电脑 ACP preset，其进程、自带工具与 native session 位于电脑内，模型和群笔记使用不同的逐次派发授权；后端同时校验网关来源证明、job/turn、generation 和预算，实际用量统一记账。其他 preset 不回退宿主。部署合同见 [实现指南](../backend-rs/crates/backend/src/docs/guide/computers.md)，证据和外部边界见 [本地验收](computers-local-acceptance.md)。真实设备、VPS TLS、真实模型与性能验收没有以 fixture 替代，因此整份 Spec 保持 proposed。

## 5. P1：浏览器、沙箱与 computer use

### 5.1 资源和能力

第一版实现 Linux Docker provider，定义 provider-neutral `ComputerProvider` 边界：创建/启动/停止/销毁、文件、命令、浏览器、观察/输入、终端、持久工作目录和能力查询。E2B、Daytona 等仅保留扩展位置，不同时接入多家。

电脑具有 owner、group 或 agent 绑定、provider、持久 home 引用、状态、generation 和能力集合。执行绑定用显式 `execution_environment_id`，不能仅凭 `cloud_sandbox` 字段判定可用；原占位资源在迁移后仍显示未配置，直到完成有效绑定。

- Team 电脑属于同一信任边界的群组，允许成员共享工作文件与明确共享的浏览器身份。
- Private 电脑隔离整个 home 和浏览器身份，不与 Team 隐式复制登录态。
- 第一版一台电脑仅允许一个执行驱动和一个活动桌面；不实现多屏并发，也不提高现有群成员并发。
- 文件、Shell、浏览器与 computer use 指向同一电脑；运行详情明确显示电脑和执行位置。

### 5.2 Supervisor 与执行边界

Docker 管理由独立 Supervisor 执行，控制接口只供已认证后端调用；Agent 容器不能访问 Docker socket、Supervisor 凭据、后端数据库、签名密钥或模型服务商的全量配置。服务器模式的受控 Agent 不得回退到宿主 Shell。

当前 Compose 的 socket 挂载需要在交付受控电脑前移出应用/Agent 可达环境，改成权限受限的 Supervisor 部署。这一步排在 M2a 第一位：`docker-compose.yml` 已用 DANGER 注释承认该挂载等于把宿主 root 交给任何登录用户，移除之后沙箱与宿主的权限差异才成立。容器采用非特权用户、资源限额、受控挂载和独立网络；禁止 privileged/host network/任意宿主路径挂载。容器共享宿主内核，第一版不宣称虚拟机级或敌对多租户隔离。[Docker 安全边界](https://docs.docker.com/engine/security/)

容器内只挂载服务端解析后的工作区/home；原 Git worktree 绑定需要映射到对应路径。网络默认允许完成任务所需的公网访问，但隔离控制面、数据库网络和云元数据地址；内部业务站点通过实例配置显式放行。工具中的路径检查和 Shell 审批不能替代执行环境隔离。

### 5.3 浏览器、桌面与接管

第一条浏览器路径复用现成组件：容器内 Chromium + Playwright MCP 提供 navigate、snapshot、act、tabs、download/upload，图形桌面与人工接管先用现成 noVNC 镜像。目标是先跑通“网页操作 → 下载 → 人工接管 → 交还”的完整流程，再根据实测缺口决定是否自研页面工具。无论用哪种实现，旧元素引用操作必须失败，不能悄悄定位新元素；现成组件不满足时在适配层补校验。浏览器登录态保存在绑定电脑的 home。

computer use 提供观察截图与有序输入，操作携带 screen/generation，并校验模型图像能力。不能支持的动作返回明确能力错误，不伪造成功。页面工具不能完成时可转桌面操作或请求人工接管。

人机控制状态为 `agent_controlled → takeover_requested → user_controlled → released`。取得用户控制前，停止接收 Agent 新动作并等待在途动作结果明确；无法确认时保持等待，不授予第二个驱动。独占控制凭据使用租约和 generation，旧页面、旧运行和过期凭据不能继续输入。用户释放后由明确继续动作恢复 Agent。

主 WS 只同步电脑状态、接管请求、控制权和操作摘要。屏幕画面使用独立、鉴权且可撤销的通道；文件通过 HTTPS 传输，不把画面和大文件塞入账号事件日志。预览/控制凭据不暴露 provider 密钥或原始内部地址。

### 5.4 持久化与 Runtime 适配

停止、后端重启、重建电脑后保留 home、项目文件和浏览器身份。第一版 Docker 本地卷是持久边界；基础镜像中的系统修改不等于可迁移工作数据。需提供一致性备份/恢复和明确的销毁确认；MVP 不承诺任意供应商间无损迁移。

内置 Agent 的文件/Shell/浏览器工具通过电脑适配层执行。ACP CLI 若绑定电脑，其进程及自带文件/Shell 工具也必须位于该电脑内；只把某个浏览器 MCP 放进容器不足以隔离 CLI。

P1a 先交付内置 Runtime。P1b 提供至少一个 ACP preset 的容器内 stdio 启动、模型代理配置、取消与会话恢复；所选 preset 必须支持指向后端代理并使用对应协议。无法代理的 preset 禁止选择该受控电脑并说明原因，不自动降级到宿主或注入厂商凭据。群笔记 MCP 需适配容器可达的逐次派发授权桥，不能假设宿主可执行文件存在于沙箱中，也不能泄漏长期账号令牌。

用户在电脑内完成第三方登录，不复制桌面浏览器 profile。日志与操作记录屏蔽密码、Cookie 和控制凭据；交付物关联 task/turn、路径、大小和下载入口，不能只留在聊天声明中。

### 5.5 后端模型代理（用户已确认）

模型服务商 API key、可续期登录凭据和完整 provider 配置仅留在后端。容器发送模型请求给后端代理，由代理校验权限、选定已配置上游并注入厂商凭据；不采用把厂商短期令牌逐次下发容器的替代方案。内置 Runtime 已在后端发起推理，不为其重复增加网络代理跳转，但与容器调用共用授权、用量与预算规则。

- **代理授权**：容器只取得 Qunica 代理访问凭据，绑定 owner、computer generation、agent、job/turn、允许的 provider/model 和调用预算。该凭据不是厂商令牌、账号登录令牌或 Supervisor 凭据；后端可撤销且有有限有效期。它不能授权文件、群笔记或电脑控制等其他接口。任务结束、取消、授权撤销或电脑 generation 变化后拒绝新调用，并终止可取消的在途流。
- **上游约束**：后端根据绑定解析上游地址与认证头，不接受容器指定任意 URL、厂商密钥或跨账号 provider。代理入口只暴露所需模型能力，不是通用 HTTP 转发器；仅允许经授权的执行网络访问。
- **协议与流式**：为已声明支持的 preset 提供匹配的协议适配，保留流式文本、工具参数、图像输入、错误与 usage 语义。上游仍可使用 SSE；代理不把模型流混入客户端账号事件。取消、超时、流中断与背压有界处理；已开始的推理不因流中断自动重新提交，也不声称取消能撤回上游已产生的费用。
- **预算与对账**：每次代理调用有独立 model_call_id，绑定原 job/turn。代理执行调用数、并发、超时与 Token 预算限制；以厂商实际返回用量结算，缺失用量保留为未知。代理与 ACP 回传同一调用时只记账一次，不因 CLI 重启或新连接重置预算。
- **凭据隔离**：长期模型凭据不得写入容器环境、home、镜像、可读挂载、工具结果或错误日志。返回错误屏蔽上游凭据；默认审计记录调用元数据和用量，不记录认证头及代理访问凭据。第三方网站/应用登录与模型认证分别处理。
- **兼容性边界**：后端代理不能自动把 CLI 订阅/OAuth 身份转换成已配置 API provider。受控电脑只声明支持已验证的 preset、协议与模型组合；不支持的组合明确报错，不能通过导入宿主 CLI 登录态绕过代理。现有本地信任执行方式与受控电脑的能力声明区分。

代理的具体内部路由名和 provider 适配清单在实现时确定；以上授权、流式、取消、计量与隔离要求是 M2b 的交付门槛，不留作可选增强。M2a 只运行内置 Runtime，推理仍在后端发起，不需要代理；代理与容器内 ACP 一起在 M2b 交付。

### P1 必须通过

1. 用生产执行路径在真实 Docker Chromium 完成 fixture 网站操作和下载，独立核实文件内容。
2. 页面引用或 screen generation 过期时拒绝输入；相同动作无效果不能无限重复。
3. 人工接管与 Agent 输入互斥；旧控制凭据失效，释放后正确恢复。
4. 停止、重启与重建保留工作文件和对应浏览器身份；Private 与 Team 数据不混用。
5. Agent 无法访问控制面 socket、数据库、其他电脑卷或后端密钥；不能绕回宿主 Shell。
6. 不支持电脑执行的 ACP preset 被明确拒绝；支持的 preset 进程和自带 Shell 确实在目标容器。
7. 电脑启动或操作中失败时有持久状态、有限重试和可解释恢复，不伪造成功。
以下 8–10 为 M2b 验收：

8. 容器内用代理完成一次真实协议的流式推理 fixture，核实文本/工具参数/usage；检查环境、home、挂载、错误和日志均不含厂商凭据。
9. 过期、撤销、跨 job/账号及旧电脑 generation 的代理凭据被拒绝；不能指定任意上游，取消后不发起新调用。
10. 代理流中断不暗中重发推理；用量能归属原 job/turn，代理与 ACP 回报不重复计量，不支持代理的 preset 明确拒绝。

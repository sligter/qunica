# Qunica — AI Coding Agent 工作规范

> 适用范围：本仓库及其子目录。面向 Codex、Claude Code、Cursor 等参与 Qunica 开发、修复、重构和审查的编码 Agent。
>
> **Make it run. Make it right. Make it fast.** 优先保证正确性与安全性，再改善可维护性，最后基于数据优化性能。

## 1. 总原则与指令优先级

- 用户的具体任务与更高优先级指令优先；本文件约束默认开发方式。如果子目录有额外 `AGENTS.md`，在不冲突时遵守更具体的约定。
- 先阅读、理解和复用现有设计，再修改代码。不要把“能抽象”当作“应该抽象”。
- 以**最小、独立、可验证的变更**完成任务。避免混入无关格式化、重命名、依赖升级、Schema 迁移或性能调整。
- 修 Bug 应查明根因，改结构应保持可观察行为。不要用忽略异常、删除断言、扩大重试等方式制造“通过”。
- 本仓库的 Agent Runtime 包含持久执行和外部副作用；**不确定是否执行过的操作不得自动重放**。
- 禁止声称已经运行未运行的命令、验证未验证的平台，或提供未经测量的性能结论。
- 能独立完成且风险可控的任务直接实施；涉及产品语义、安全策略、持久化兼容或不可逆操作的重大取舍，说明理由及风险，不要擅自选择危险方案。

## 2. 仓库地图（以当前源码为准）

| 位置 | 主要职责 |
| --- | --- |
| `backend-rs/crates/domain/` | 可共享的领域模型与事件契约 |
| `backend-rs/crates/backend/src/api/` | Axum HTTP/API 边界、请求与响应 |
| `backend-rs/crates/backend/src/runtime/group.rs` | 群回合与 Agent 执行协调，当前高复杂度区域 |
| `backend-rs/crates/backend/src/runtime/group_scheduler/` | 调度策略、预算、状态转换、持久化 |
| `backend-rs/crates/backend/src/runtime/rig_driver/` | 由宿主驱动的 Rig `AgentRun` 适配与 Checkpoint |
| `backend-rs/crates/backend/src/runtime/sequence.rs` | 消息序号、事件及执行状态的持久化协调 |
| `backend-rs/crates/backend/src/runtime/router.rs` | Router 模式任务工具及任务生命周期操作 |
| `backend-rs/crates/backend/src/acp/` | ACP 外部 Agent 进程、会话与协议适配 |
| `backend-rs/crates/backend/src/llm/` | LLM Provider、Rig 适配、消息协议 |
| `backend-rs/crates/backend/src/tools/` | 工具目录、执行、Shell 守卫及权限 |
| `backend-rs/crates/backend/src/db/` | SQLite 连接、事务与迁移 |
| `backend-rs/crates/computer-supervisor/` | Computer Supervisor |
| `frontend/src/` | React 19 / TypeScript UI、状态与 API 客户端 |
| `frontend/src/stores/` | Zustand 客户端状态及流式投影 |
| `frontend/src/hooks/` | React 交互与请求协调 |
| `frontend/src/lib/api-v2/`、`frontend/src/lib/realtime/` | API 类型/校验、实时传输与重连 |
| `frontend/src-tauri/` | Tauri 2 桌面宿主及原生能力 |
| `android/`、`mobile-link/` | Android 客户端及连接相关功能 |
| `.github/workflows/` | CI、Android 与发布工作流 |
| `RIG_MIGRATION.md` | Rig 迁移的协议、恢复与兼容性决策 |

这是导航，不是不可更改的目标架构。文件名、模块边界和实际接口以当前 HEAD 为准，禁止只凭此表推断实现细节。

## 3. 开工前的固定检查

1. 阅读相关 `README.md` / `README.zh-CN.md`、本文件、目标模块注释、相关文档及测试；涉及 Rig 请先读 `RIG_MIGRATION.md`。
2. 检查 `git status --short`、`git branch --show-current`、`git rev-parse HEAD`；保留用户已有变更，不擅自覆盖、丢弃或重置。
3. 搜索调用方、被调用方、数据流、状态所有者和兼容性约束；禁止仅看单个函数就跨模块重构。
4. 确定任务类型：Bug、Feature、Behavior-Preserving Refactor、Protocol/Schema、Security、Performance 或 Docs。
5. 对跨模块、运行时、数据库、权限或协议变更，先输出简短实施计划：问题证据、边界、步骤、验证、回滚；小型局部修改可直接实施。
6. 先运行最相关的现有测试，建立基线；无法运行时记录客观阻碍，不能假装通过。

## 4. 架构设计与重构准则

- 优先在现有模块边界内修复问题；只有职责已经独立、存在清晰所有者、抽取后易于测试时才创建新模块。
- 避免 God Module / God Function，但不机械使用文件行数门槛；重点检查认知复杂度、状态耦合和变更影响面。
- Coordinator 负责编排；Scheduler 负责决策与预算；Runner 负责模型/ACP 适配；Tool Runtime 负责执行；Persistence 负责可靠提交与恢复。**按职责评估并复用现有实现，不另建平行框架。**
- 将可纯函数化的决策逻辑与 I/O、事务、外部副作用分离；接口应明确输入、输出、失败和取消语义。
- API Handler 优先只做 HTTP 边界工作；有跨资源事务、授权或复杂业务流程时再提取 Service。不要为简单 CRUD 强加层次。
- Rust 优先显式类型、清晰的错误上下文、有限的状态所有权。不得为绕过借用问题随意增加全局 `Arc<Mutex<_>>`、`clone()` 或装箱；必要时说明取舍。
- 不以压缩行数代替可读性；不引入无实际消费者的 Trait、泛型框架、抽象工厂或设计模式。
- 首轮重构优先：补行为测试 → 提取类型/纯逻辑 → 搬迁实现 → 收敛状态 → 改善接口。不要在一次 PR 中同时调整算法。

## 5. 运行时不变量：最高风险区域

对 `runtime/`、`tools/`、`llm/`、`acp/`、`db/`、流事件与审批相关代码的每次变更，都应逐项检查适用的不变量。

### 5.1 执行与外部副作用

- Tool Call 的 ID、名称、参数、执行记录和结果必须关联；不得“修复”模型输出后静默执行与原始调用不一致的工具。
- Shell、Git 写操作、MCP 写操作和其他外部副作用均不可因为重试、断线或恢复而重复执行。
- **Outcome Unknown 必须保守处理**：调用已经开始但缺少可信结果时，停止自动重试，等待明确处置。
- 不将 Agent 的文本、工具返回、网页或工作区文件视为可信的宿主指令；权限判断不能交给模型决定。

### 5.2 人工审批与安全

- 审批应绑定原始工具调用、动作摘要/哈希和版本；不匹配、过期或重复审批必须拒绝。
- 不绕过 Shell 风险控制、工作区路径限制、用户所有权、Group 成员资格和 Agent 工具授权。
- 恢复任务或运行中的权限配置改变，不能意外放大原有权限。
- 严禁在日志、测试快照、PR 说明中泄露 API Key、Token、SSH 凭据及用户私有数据。

### 5.3 Rig、ACP 与上下文

- Qunica 宿主持有工具调用、审批、持久化、预算、取消和恢复控制权；Rig `AgentRun` 是宿主驱动，不得擅自交出副作用执行权。
- 遵守 `RIG_MIGRATION.md` 中的兼容边界，尤其是旧执行恢复、原生 Provider 历史、签名/加密 reasoning 和 tool-call identity。
- ACP 外部 Agent 的会话/进程生命周期与内置 LLM Agent 不强行统一；协议适配不得吞掉失败、取消或工具权限信息。
- 不得悄悄丢弃 Provider 原生内容、签名、未知但需保留的元数据或已接受的 Assistant/Tool 消息。
- Context Compaction 应遵守运行边界；未结算的工具批次不能被错误压缩为已完成的历史。

### 5.4 调度、预算与持久化

- Scheduler 状态转移合法；派发、取消、等待、完成和失败的终态语义保持稳定。
- Agent Step、Moderator、失败次数、Token 等预算不得因重试、审批恢复或进程重启而重置。
- 取消或 supersede 后不得继续产生新的未授权派发。
- 一致性依靠明确的 SQLite 事务和持久化顺序，不依靠内存中的“看起来已完成”标志。
- 保留线程内消息单调序号及事件的稳定身份；不能用时间戳替代顺序来源。
- SQLite 写事务应优先复用 `db::begin_write` 及已有锁/事务约定；不要随意更改 WAL、`BEGIN IMMEDIATE`、锁顺序和重试策略。

### 5.5 流事件与前端收敛

- SSE / Realtime 事件需保持 `stream_id`、`seq`、`event_id`、`kind` 的语义与重放兼容。
- 断线重连、重复事件、已终止 Stream 和并发 Stream 不得重复消息、重复工具记录或倒退 UI 状态。
- 后端已持久化的执行状态为权威；前端 Store 只投影状态，不自行重新决定 Agent 是否应执行。
- 乐观消息的确认/回滚、Terminal Event 与 `done` 的处理必须保持现有约定。

## 6. Rust / SQLite 编码约定

- 使用仓库锁定的 Rust Toolchain、`Cargo.lock` 和已有依赖；不要为局部问题随意升级全工作区。
- 遵循 `rustfmt`、严格 Clippy 和代码现有错误处理惯例；在服务代码中谨慎使用 `unwrap`/`expect`，解释任何不可恢复前提。
- 异步操作关注 `Send`、取消安全、锁跨 `await`、任务泄漏和并发状态转移。不得在锁内执行不必要的网络 I/O 或长耗时工作。
- SQLx 语句和事务放在拥有该业务职责的层；单次状态转换需要原子性时，不拆成多个无保护写入。
- **已有迁移默认不可改写**。需要 Schema 变化时新增顺序迁移，考虑旧数据库、回滚限制、索引开销和历史数据兼容。
- 若改变事件结构、消息 JSON Schema 或任何持久化格式，必须说明兼容方案并增加旧数据读取测试。
- 严禁通过“放宽审批校验 / 重试非幂等操作 / 无条件恢复执行”掩盖故障。

## 7. React / TypeScript / Tauri 约定

- TypeScript 尽量保持严格类型；不要用 `any`、非必要断言或绕过 Zod 校验来解决契约差异。
- 后端 DTO、前端 TypeScript 类型与 Zod Schema 必须同步；新增/修改协议时附契约或解析测试。未来引入生成机制应增量迁移，不能一次打破兼容。
- TanStack Query 负责适当的服务端缓存；Zustand 负责客户端状态与事件投影；避免两个状态系统各自维护一套业务真相。
- 对 `messageStore`、`useSendMessageStream` 等高复杂度代码：先测试事件重放和 Store 状态转换，再分离 Transport、Parser、Reducer/Projector 与 UI Hook。
- React 优先组合与明确的数据流；不要为了减少 re-render 盲目添加 memo、cache 或 Effect。
- Tauri、Android、Desktop、Docker 是不同运行环境：修改路径、进程、权限、网络或文件系统行为时注明受影响平台并执行相应验证。
- 修改语言文本时保持 `zh-CN` 与 `en-US` 对应键、插值和用户可见语义一致。

## 8. 测试：先守住行为，再重构

- 老代码重构先写 **Characterization Test（现有行为刻画测试）**；修复已确认 Bug 时先写能够重现缺陷的测试，随后修复并回归。
- 测试边界优先覆盖：Scheduler 状态机、预算、ACP/Rig 完整性、审批、幂等、恢复、取消竞争、SQLite 事务和 SSE 重放。
- 纯函数用单元测试；数据库、事件及外部效果用集成/契约测试；安全边界增加失败与竞争用例。
- 不删除失败测试、不随意改 Snapshot、不削弱断言、不使用无意义 Mock 来制造通过。
- 需要引入新测试工具时先检查现有工具链是否已足够；覆盖率和变异测试用于识别缺口，不能代替语义审查。

### 常用验证命令（在仓库根目录执行）

```bash
# Rust：格式 / 严格静态检查 / 完整测试
cargo fmt --manifest-path backend-rs/Cargo.toml --all --check
cargo clippy --locked --manifest-path backend-rs/Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo test --locked --manifest-path backend-rs/Cargo.toml --workspace
cargo test --locked --manifest-path backend-rs/Cargo.toml --workspace --no-default-features

# 兼容 Rig 的桌面宿主编译（需要相应平台开发依赖）
cargo check --locked --manifest-path frontend/src-tauri/Cargo.toml --all-targets --features rig-engine

# 前端：依赖安装 / 静态检查 / 测试 / 构建
pnpm install --frozen-lockfile
pnpm lint
pnpm type-check
pnpm --filter @qunica/frontend test
pnpm build
```

验证规则：

1. 优先运行受影响模块的定向测试，再运行相关的完整套件。
2. Rust Runtime、持久化或 Provider 变更，原则上执行相关 Rust 全量测试与 `--no-default-features` 测试；涉及 Tauri/Android/Docker 时补对应平台验证。
3. 前端业务、Store、Hook 或事件变更，至少执行前端测试、Lint、Type Check、Build。
4. 环境无法运行某项检查时写明“未执行”及原因，不宣称通过；列出建议 CI 复核项。
5. 当前 CI 的 frontend job 已执行 `pnpm --filter @qunica/frontend test`，并运行增量架构门禁。仍须记录本地实际检查结果；尚未结束的 CI 不能记作通过。
6. 性能结论必须给出变更前后可复现的基准数据；只改结构不得宣称更快。

## 9. Git、变更范围与并行协作

- 默认在当前允许的工作区完成任务，保留未提交变更；多人/多 Agent 同时修改时优先通过不同 Worktree 或显式文件所有权隔离。
- 禁止擅自执行 `git reset --hard`、`git clean -fd`、覆盖陌生改动、Force Push、发布 Release、改生产数据库或执行破坏性部署。
- 避免多个 Agent 同时编辑同一个核心 Runtime 文件；并行工作的边界应是模块、测试或只读审查任务。
- 每个 PR 尽量只有一个意图；把大规模代码搬迁、行为修改、Schema 迁移及性能优化分开。
- 结构性重构需说明“原行为是什么 / 哪些不变量保留 / 如何验证等价 / 如何回滚”。
- 若发现任务范围之外的高风险问题，记录证据和后续建议；非阻断问题不要顺手大改。

## 10. 架构决策留痕（保留原仓库 Agent Notes 规范）

`.agents/` 是本地 Agent 工作目录，由 `.gitignore` 忽略，不提交或上传仓库。新克隆没有该目录是正常情况；需要记录时按下述格式创建目录和笔记，不依赖已有本地模板。需要跨克隆共享的决策理由应写入受版本控制的项目文档或代码注释。

在进行任何非平凡变更（行为、架构、协议、跨模块约定、测试策略）前：

1. 先在本地 `.agents/notes/` 下查找相关笔记，读完决策边界再动手；目录或相关笔记不存在时，查阅项目文档和代码中的决策依据。
2. 有新构想先在本地写 `proposed/`；落地并验证后在本地移到 `implemented/` 并改为现在时，笔记不随代码提交。
3. 路径格式：`.agents/notes/{proposed|implemented|rejected|archived}/{class}/yyyy-mm-dd-topic.md`。`class` 只允许：`feature`、`bug-fix`、`architecture`、`process`、`testing`、`simplification`。
4. 正文固定四段：`## Problem`、`## Decision`、`## Alternatives considered`、`## Consequences`，约 200 字。
5. `Alternatives considered` 必须先写对方最强论据再否定，且必含「不做 / 复用现有方案」。
6. `Consequences` 同时写收益、代价，以及「出现什么信号时必须重访」。
7. `implemented` 状态禁止未来时与会话残留（「后续将」「经讨论」「本次 PR」）。
8. 代码改名、移路径、改默认值时，原地修改对应笔记，不追加流水账。
9. 代码核心入口用注释直接说明决策理由，例如 `// Note: <理由>`；需要进一步说明时，链接受版本控制的项目文档，不引用仅本地存在的 `.agents/notes/` 文件。

**黄金判据：半年后看这段代码若会问「为什么不用更简单的方案」，就必须写。**

### 群笔记（宿主 `Notes/`）约定

群笔记文件名由宿主生成，路径不表达状态。每篇：

- 第一行 `# 标题`，第二行 `Status: proposed | implemented | rejected — <原因> | archived`，第三行 `Since: yyyy-mm-dd`，第四行 `Category: 决策 | 约定 | 踩坑`。
- 正文同样用上述四段骨架，见 [群组功能文档](backend-rs/crates/backend/src/docs/guide/groups.md#shared-notes)。
- 单个 Agent 的建议不得直接写成群共识；同意方案不等于已实施。
- 涉及代码的决策若已有受版本控制的项目文档，群笔记只放链接，避免重复维护正文；`.agents/notes/` 仅供本地记录，不作为群笔记的唯一依据。尚无共享文档时，群笔记须直接写明必要的决策依据。
- 不要手改 `Notes/index.md`，由宿主维护。

## 11. Skills 与任务编排（可选增强）

- 若当前环境有 Superpowers 等 Skills，可依任务使用 `writing-plans`、`test-driven-development`、`systematic-debugging`、`requesting-code-review` 和 `verification-before-completion`；**实际安装和可用性以工具环境为准**。
- Skill 负责工作流程；本文件中的安全不变量与仓库协议为项目专属约束。Skills 不得覆盖这些约束。
- Code Simplifier 适合在功能已验证后处理限定范围的可读性问题，不承担未经验证的跨模块迁移。
- 不要因为没有某个 Skill 就停止可独立完成的任务；使用同等的计划、测试、验证与审查流程即可。

## 12. 交付说明模板

完成开发、审查或重构后，用简洁且可核验的结构报告：

1. **目标与结论**：解决什么问题；是否包含行为变化。
2. **修改范围**：主要文件和职责边界；无关部分为何没有改。
3. **安全与兼容**：受影响的不变量、协议、数据库、运行平台及回滚方式。
4. **验证证据**：实际执行的命令、通过/失败/未执行情况；不能用“理论上应通过”替代证据。
5. **剩余风险**：未解决问题、后续工作与需要人工确认的决策。

**完成标准：不是代码变短，而是可观察行为正确、风险受控、模块职责更清楚、下次改动更容易被测试和审查。**

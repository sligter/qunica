# Router 任务复用与通知收敛 Spec

Status: implemented
Version: 0.2
Since: 2026-11-02

在 [Router 调度模式](router-worker.md) 之上解决三个问题：一个问题会造出十几个任务线程；新派发丢掉之前处理同一问题的上下文；每个任务结束和 Router 每次回复各弹一条通知。

## 1. 目标与非目标

目标：

- 同一问题、同一执行者的后续工作落在原任务线程里，Worker 带着该线程的历史与 ACP 会话继续。
- Router 每个回合都能看到本会话已有的任务，不靠它自己记住 `task_id`。
- 一次请求引出的 N 个任务，默认只产生 1 条「Router 已汇总」通知，外加需要用户动作的「任务等待你回复」。
- 不新建表，不改执行核心：复用任务线程、`execution_jobs`、回报合并、取消与重启恢复。

非目标：

- 不引入「工作流 / workstream」层（见 §9）。
- 不自动把其他任务的结果注入 Worker 的交办指令（见 §9，作为后续阶段候选）。
- 不改对话助手面板，不改 Watchdog 行为。
- 不做跨设备通知去重：每台机器各自弹通知，这是现状。

## 2. 存储与配置

任务复用：不需要迁移。线程已有 `assigned_agent_id`、`origin_thread_id`、`origin_turn_id`（0052）；续投就是往已有线程追加一条 `router_dispatch` 消息和一个回合。

通知：新增迁移 `0057_task_job_event.sql`，仿照 `0055_watchdog_job_event.sql` 重建 `account_execution_jobs_update` 触发器，在 `job.changed` 载荷里加一个字段：

- `task_reported`：该 job 所在线程是派发任务，且其结果会回报给 Router。条件与 `router::report_in_tx` 一致：`origin_thread_id` 非空、群 `status='active'` 且 `router_agent_id` 非空、来源线程处于 active/running/paused。

账户快照 `api/account.rs` 的线程行同步加 `dispatched`（`origin_thread_id IS NOT NULL`），供本地流（`useResumeStream`）判断线程类型。

旧数据兼容：老任务线程本来就有 `origin_thread_id`，触发器重建后立即生效；老客户端忽略未知字段，行为与现在相同。

配置：

- `max_router_tasks`、`MAX_TASKS_PER_REQUEST` 数值不变，计数口径变化见 §3。
- 通知级别存 localStorage，新键 `qunica:notifications:task-updates`，取值 `summary`（默认）| `all`。理由同 `replyNotifications.ts` 头注释：通知配合按机器授予的 OS 权限，不应随账户同步到每台设备。

## 3. 工具

### 3.1 `DispatchTask` 增加可选 `task_id`

| 参数 | 变化 |
| --- | --- |
| `task_id`（新增，可选） | 给出时续投该任务：不建线程，在原线程追加交办消息和回合 |
| `assistant` | 续投时可省略；若给出必须等于该任务的执行者，否则报错（换人请新建任务） |
| `title` | 续投时忽略，保留原标题，避免任务列表标题来回变 |
| `topics` | 续投时省略表示不改订阅；给出则替换为新列表 |

续投规则：

- 只能续投**本来源线程**派发的任务（`origin_thread_id = 当前线程`）。否则报告会回到别的主对话（`report_in_tx` 按 `origin_thread_id` 投递）。
- 线程 `archived`/`cleared`：拒绝，提示新建任务。线程 `paused`：沿用 `reopen_paused_thread_in_tx`。
- 任务 `waiting_for_user`：拒绝，与 `SendToTask` 相同。
- 任务正在运行：消息排在当前工作之后，与 `SendToTask` 相同；不占新并发额度（它已经算在进行中里）。
- 任务空闲（completed/failed/cancelled/interrupted/idle）：占一个并发额度，达到 `max_router_tasks` 时失败。
- 返回 `{task_id, title, assistant, status:"continued", run, topics}`，`run` 是该线程第几次交办。

### 3.2 计数口径

- 每次请求上限 `MAX_TASKS_PER_REQUEST` 改为数「自最近一条用户消息以来，Router 写进本来源线程各任务的交办消息」，而不是数新建线程。否则续投和 `SendToTask` 可以绕过上限无限循环。
- `SendToTask` 对空闲任务同样检查并发上限（现在不检查，属于漏洞）。对运行中任务的追加不变。

### 3.3 Router 上下文注入

Router 回合的系统提示追加「本会话任务」段，由运行时每回合生成，至多 8 条，按 `updated_at` 倒序，只含本来源线程、未 archived/cleared 的任务：

```
Tasks in this conversation (newest first):
- <task_id> · <title> · <assistant> · <status> · run <n> · updated <time>
```

标题由 Router 自己写，仍按普通文本转义。多于 8 条时末行提示用 `ListTasks` 查全。

`ROUTER_PROMPT` 增量（替换第二条中的 follow-up 一句）：

> Before dispatching, check the tasks in this conversation. If one already works on the same subject with the same assistant, pass its task_id to DispatchTask to continue it: the assistant keeps that task's history. Start a new task only for a different subject or assistant; then put what it needs from earlier tasks into the instructions, using ReadTask to get their results.

`SendToTask` 保留，用于给运行中的任务补一句话；描述里注明「要开始新一段工作请用 DispatchTask 加 task_id」。

### 3.4 回报

`report_in_tx` 不变，报告条目加 `run` 字段。同一任务多次交办会产生多份报告，合并逻辑不变。

### 3.5 取消

`CancelTask` 语义不变：取消进行中回合并退订话题。被取消的任务之后仍可用 `task_id` 续投；话题不会自动恢复，需在续投时传 `topics`。

### 3.6 重启恢复

没有新状态。续投写入的消息、回合、job 和首次派发走同一条 `insert_message_in_tx` 路径，恢复由 job 队列负责。

## 4. 通知

分级规则（纯函数 `replyNotificationPolicy(kind, outcome, taskReported, level)`，放在 `lib/replyNotifications.ts`）：

| 运行类型 | completed | failed | waiting |
| --- | --- | --- | --- |
| 主对话（含 Router 回复、Router 汇报回合） | 通知 | 通知 | 通知 |
| 派发任务，`task_reported=true`，级别 `summary` | 静默 | 静默 | 通知 |
| 派发任务，`task_reported=false` | 通知 | 通知 | 通知 |
| 派发任务，级别 `all` | 通知 | 通知 | 通知 |
| Watchdog 检查 | 静默（现状） | 静默 | 静默 |

理由：结果已经通过 `task_report` 回到 Router，Router 的汇报回复本身就是那 1 条通知；任务失败也在报告里。`waiting` 需要用户去任务线程回答，Router 不能代答，所以保留。`task_reported=false`（群已切出 Router 模式、来源线程已归档）时没有汇总可等，退回逐条通知，避免用户什么都收不到。

合并窗口：同一会话（`conversation_id`）的通知，第一条立即发；之后 20 秒内的通知先攒着，窗口结束时合成一条「N 条更新」。用于覆盖 Router 正忙时报告分成多个回合、以及任务 `waiting` 与随后 Router 回复连发的情况。窗口内用户切到该会话则丢弃攒下的通知。

与 Watchdog 豁免的关系：同一机制——触发器把线程类型写进 `job.changed` 载荷，快照再带一份兜底。`accountSync.ts` 现在的 `watchdog` 判断并入 `replyNotificationPolicy`，不再单独写条件。

前端最小改动点：

- `lib/replyNotifications.ts`：新增策略函数、合并窗口、级别读写；`notifyReplyOutcome` 增加 `kind`/`taskReported` 入参。
- `lib/realtime/accountSync.ts`：从载荷读 `task_reported`，传给 `notifyReplyOutcome`。
- `hooks/useResumeStream.ts`、`hooks/useSendMessageStream.ts`：从快照线程的 `dispatched` 推出 `kind`；本地流拿不到 `task_reported` 时按 `true` 处理（本地流只在用户打开任务线程时存在，那时看得到）。
- 设置页通知开关下加一个二选一：「任务完成：只通知汇总 / 每个都通知」。

## 5. 前端

- 任务选择器、`task_report` 卡片不变；卡片标题后显示「第 n 次」（`run > 1` 时）。
- `DispatchTask` 工具卡片在 `status:"continued"` 时显示「继续任务」而不是「派发任务」。
- 任务线程内多条 `router_dispatch` 消息按现有方式渲染。

## 6. 安全

- 续投限制在本来源线程，Router 不能借 `task_id` 操作其他主对话的任务。
- 「本会话任务」段只含 Router 写的标题和状态，不含 Worker 输出，不引入新的不可信文本。
- 审批门、权限、并发上限不变；计数口径收紧后续投不能绕过上限。

## 7. 测试

已落地的后端用例在 `tests/group_stream/router_mode.rs`（14 项，连跑三次稳定；完整 `tests/group_stream` 为 192 项通过、1 项跳过，跳过项需要 Docker；`group_scheduler` 12 项通过）。用例内容：

1. 续投：派发 → 报告 → `DispatchTask{task_id}` 不新建线程，Worker 的回合能看到第一次交办和自己的回复；报告 `run=2`。
2. 续投拒绝：别的来源线程的任务、`waiting_for_user`、archived、`assistant` 不一致，各返回原因。
3. 并发：`max_router_tasks=1`，一个任务运行中时续投另一个空闲任务失败；续投运行中的任务成功排队。`SendToTask` 对空闲任务同样受限。
4. 每次请求上限：连续续投同一任务达到 16 次后失败。
5. Router 提示包含「本会话任务」段，只列本来源线程任务，至多 8 条。
6. 迁移：`job.changed` 载荷的 `task_reported` 在 Router 模式任务为 true，主对话为 false，群切回 bounded 后为 false。
7. 取消后续投：可续投，话题未恢复；传 `topics` 后恢复。

前端 vitest（实际覆盖）：

- `lib/replyNotifications.test.ts`：策略表逐格；合并窗口（第一条立即发，窗口内攒成一条，切到会话后丢弃）；级别读写与 localStorage 不可用时默认 `summary`。
- `lib/realtime/accountSync.test.ts`：3 个任务完成 + 1 个 Router 回复 → 1 条通知；`task_reported=false` 的任务完成 → 逐条通知；任务 `waiting` → 通知。

## 8. 落地拆分

| 阶段 | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| P1 通知收敛 | 迁移 0057、快照 `dispatched`、策略函数、合并窗口、设置项 | 已实现 | 前端测试全过；手工：派 3 个任务，只收到 1 条汇总通知 |
| P2 Router 看见任务 | 「本会话任务」段、`ROUTER_PROMPT` 增量、计数口径、`SendToTask` 并发与额度检查 | 已实现 | 后端测试 3、4、5；手工：同一问题追问 3 次，任务数不再线性增长 |
| P3 续投 | `DispatchTask.task_id`、来源线程限制、报告 `run`、前端「第 n 次」与「继续任务」 | 已实现 | 后端测试 1、2、6、7 |
| P4 候选 | 新任务自动附相关任务引用（§9 c） | 未启动 | 只在 §10 的信号出现时启动 |

与草案的两处偏差，实现时按代码事实调整：

- P2 阶段的 `ROUTER_PROMPT` 先不提 `task_id`（该参数要到 P3 才存在，提前写会教模型调用不存在的参数），改用「同一主题请用 `SendToTask` 在已有任务里跟进」；P3 落地后提示词改为「pass its task_id to DispatchTask」。
- 前端识别「继续任务」不再匹配工具结果文本里的 `"status":"continued"`：那依赖 serde_json 的键顺序，且摘要按固定长度截断会丢掉标记。改为读工具调用参数里是否有 `task_id`，其次才看结果里的 `status`，集中在 `toolDisplayName.ts` 一处，实时时间线与历史回合共用。
- 实现时补上了草案未列的两个漏洞：`SendToTask` 原本不检查每次请求上限，也不检查空闲任务的并发额度；两者现在与 `DispatchTask` 共用同一段检查。

P1 与 P2/P3 无依赖，可并行。P2 单独上线就能缓解线程膨胀；P3 让续投成为一个参数，而不靠 Router 选对工具。

## 9. 备选方案

- **不做，完全复用 `SendToTask`**：最强论据是零新代码——它已经能往空闲任务追加指令，Worker 带着线程历史和 ACP 会话继续，只改提示词就行。仍不够：Router 必须自己记住 `task_id`，历史窗口截断后就记不住；`SendToTask` 不受并发和每次请求上限约束，鼓励它会放大成本漏洞；它可以操作同群其他主对话的任务，报告却回到原来源线程；模型在「开新任务」这个时刻调用的是 `DispatchTask`，复用入口放在别的工具里命中率低。P2 吸收了它的长处（只改提示加上下文），P3 补上缺口。
- **(b) 工作流 workstream 层**：一条工作线代表「Worker × 子问题」，派发只往里加一次运行，概念最清楚，也方便 UI 按工作线分组。否决：要新表或新列、新的生命周期（何时关闭工作线）、新 UI 和迁移；而一个任务线程加 `run` 计数已经表达了同样的东西。若用户需要「一条工作线跨多个 Worker」再重访。
- **(c) 每次新建线程，自动注入相关历史摘要**：不改线程模型，Router 不用判断续投，跨 Worker 也能带上下文。否决作为首选：选「相关」要么靠关键词要么再调一次模型，选错就把无关内容塞进指令；注入的是别的 Worker 的输出，属于不可信文本，会进入新 Worker 的指令，形成注入链；每次派发都加 token；线程数照样增长，问题 3 没解决。保留为 P4：只附任务 id 和标题的引用，由 Worker 按需读取。
- **通知：后端汇总后只发一个事件**：后端最清楚批次边界。否决：通知是否弹出取决于本机窗口焦点和正在看的会话，只有前端知道；后端汇总还要新事件类型。

## 10. 代价与重访信号

收益：任务数随「子问题数」增长而不是随「追问次数」增长；Worker 续投时保留上下文；通知从 N+1 条降到 1 条（外加需要动作的 waiting）。

代价：

- 新迁移重建一个触发器；前端多一个策略函数和计时器。
- Router 每回合多约 8 行提示（约 200–300 token）。
- 续投的任务线程越来越长，Worker 每次运行读的历史变多，单次成本上升。
- Router 要判断「同一主题」，判断错会把无关工作塞进旧线程。
- 静默后用户只能从 Router 回复里知道某个任务完成；Router 汇报回合失败时，靠主对话 failed 通知兜底。

风险：

- 同群多个主对话并发：续投限制在本来源线程；`ListTasks` 仍是全群视图，只读，不改。
- 多设备：每台机器各自弹通知（现状），合并窗口只在单机内生效。
- 取消与订阅：取消后续投不恢复话题，Router 需显式传 `topics`；漏传时话题事件回到 Router，属于可见、可纠正的行为。
- 旧数据：老任务 `run` 从现有交办消息数推算，不回填。

重访信号：

- 续投任务的 Worker 回合频繁超出上下文或 Token 预算 → 需要线程内压缩或「新任务 + 摘要」（P4）。
- 用户反馈错过了任务结果 → 检查 `task_reported` 判断或改回默认 `all`。
- 同一问题下任务数仍明显多于执行者数 → Router 不按提示续投，考虑服务端提示相似任务或强制续投。
- 需要一条工作线跨多个 Worker 或在 UI 中按工作线分组 → 重新评估 (b)。

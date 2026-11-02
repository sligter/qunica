# Router 调度模式 Spec

Status: implemented
Version: 0.2
Since: 2026-11-01

Router 是群组调度方式的第三种（API 中 `scheduler_mode = "router"`）。主对话只由一位 Router 成员应答；重活由它派发到独立任务线程，由指定 Worker 在后台执行，完成后回报给 Router，再由 Router 回复用户。

## 1. 目标与非目标

目标：

- 主对话不被长任务占住：Router 派发后立即结束本回合，用户可以继续说话。
- 复用现有持久化执行核心：任务线程、`execution_jobs`、`group_turns`、审批、取消、重启恢复、线程级占用都不新造。
- Worker 结果可靠回到 Router：回报与 job 终态在同一写事务提交，崩溃不丢、不重复。

非目标（后续阶段）：事件过滤、投机执行、多渠道、计算/状态/环境分离。Watchdog 与链式召回见 [watchdog-recall.md](watchdog-recall.md)。

## 2. 配置与存储

迁移 `0052_router_scheduler.sql`：

- `groups.router_agent_id`（`ON DELETE SET NULL`）、`groups.max_router_tasks`（1–16，默认 4）。
- `threads.assigned_agent_id`、`threads.origin_thread_id`、`threads.origin_turn_id`。

`groups.scheduler_mode` 的 CHECK 约束（0018）只允许 `bounded`/`automatic`，SQLite 改约束需重建整表。因此 Router 模式在库中存为 `scheduler_mode='bounded'` 加非空 `router_agent_id`，API 读写时映射为 `"router"`（`GroupRow::effective_scheduler_mode`、`SchedulerConfigFields::stored_mode`）。

校验（`api/groups.rs`）：

- Router 模式必须给出 `router_agent_id`；切到其他模式时清空。
- Router 必须是该群活跃成员，且运行时为 `llm_chat`（ACP Agent 无法使用 Router 工具）。创建群时须在 `initial_agents` 中。
- 从群中移除当前 Router 返回 409，需先换 Router 或换模式；删除该 Agent 本身时自动清空 `router_agent_id`。
- 模板复制有效模式、`router_agent_id` 与 `max_router_tasks`。

## 3. 应答者

`runtime/group.rs::thread_responder` 决定线程的唯一应答者：

- 线程有 `assigned_agent_id`：只运行该 Worker（派发任务线程）。
- 否则群为 Router 模式且是群会话：只运行 Router。用户 `@` 其他成员也由 Router 应答。
- 固定应答者时跳过 `select_agents` 与 `automatic` 主持循环。

Router 回合：有可派发成员（活跃、未静音、非自己）时获得 4 个 Router 工具和 Router 提示段；不提供 `AgentAsTool`，因为同步 `call` 会占住主对话。Worker 回合获得 Worker 提示段，不获得 Router 工具。

## 4. Router 工具（`runtime/router.rs`）

| 工具 | 参数 | 行为 |
| --- | --- | --- |
| `DispatchTask` | `assistant`、`instructions`、可选 `title`、`topics` | 一个写事务内创建任务线程、首条消息、回合、job、回执，并订阅话题（默认继承来源线程的话题，见 [Thread 订阅](thread-subscriptions.md)）；返回 `{task_id, title, assistant, status:"started", topics}` |
| `ListTasks` | 可选 `task_id` | 无参数：列出最近 20 个派发任务及状态；带 `task_id`：返回该任务最新回复（至多 20000 字符） |
| `SendToTask` | `task_id`、`message` | 向任务追加指令；线程忙则排队。任务在等用户回复时拒绝 |
| `CancelTask` | `task_id` | 取消该线程所有进行中回合并退订其话题；无进行中回合时返回 `idle` |

约束：

- `assistant` 按成员 id 或显示名（不区分大小写，可带 `@`）匹配可派发成员。
- 进行中派发任务数达到 `max_router_tasks` 时 `DispatchTask` 失败并返回原因。
- 同一来源线程自最近一条用户消息以来最多派发 16 个任务（`MAX_TASKS_PER_REQUEST`），防止 Router 处理回报时无限再派发。
- 派发消息以 `sender_type='agent'`、`sender_id=<router>` 写入，`content_json` 带 `router_dispatch: true`；Worker 只看到交办指令，看不到用户原话。
- 不新增唤醒句柄：派发的 job 由 Router 回合结束时 `finish_job` 末尾的 `launch` 认领。

## 5. 回报

`commands::finish_job` 在写入 job 终态的同一事务中调用 `router::report_in_tx`：

- 只回报 `succeeded`（completed）、`failed`、`waiting_for_user`；`cancelled` 不回报（取消方已知情，`ListTasks` 可查）。
- 条件：群仍为 Router 模式，来源线程处于 active/running/paused。
- 在来源线程写入 `sender_type='system'`、`message_type='task_report'` 的消息，`content_json.task_reports` 存结构化条目（任务、标题、执行者、状态、摘录至多 4000 字符、是否截断），并为 Router 入队一个回合。
- 若来源线程已有未开始的回报回合，新报告并入该消息与 job 载荷，不再新建回合。
- `waiting_for_user` 的报告提示 Router 告知用户去任务里回答，不得代答。

Router 看到的报告正文把 Worker 输出包在 `<task_result untrusted="true">` 中；Worker 文本里的闭合标签会被改写，避免提前结束围栏。会话转写层还会整体转义标记。

前端经 `message.changed` 实时事件重新拉取消息；消息 API 返回 `task_reports` 字段。不新增流事件。

## 6. 前端

- 群设置“回合方式”增加“路由分派”，选中后显示 Router 成员选择与“并发任务数”。
- `task_report` 系统消息渲染为任务卡片：标题、执行者、状态、摘录、“打开任务”（`/groups/:id?thread=<task_id>`）。
- 任务选择器为派发任务标注“派发任务”；默认选中线程时跳过派发任务，落在主对话。
- `DispatchTask` 调用沿用通用工具卡片展示。

## 7. 安全

- 审批门不变：Worker 在任务线程里触发的审批照常等待用户，Router 不能代批。
- 交办不提升权限：Worker 按群当前成员、工具、工作区与电脑配置运行。
- 任务并发上限、每次请求派发上限限制成本；回报合并限制 Router 回合数。

## 8. 测试

后端 `tests/group_stream/router_mode.rs`：

1. 配置：API 返回 `router`、库存 `bounded`；移除 Router 返回 409；非成员 Router、越界上限、缺 Router 返回 400；换 Router；切回 bounded 清空 Router。
2. 完整链路：`@Coder` 仍由 Router 应答 → 派发 → Worker 只见交办指令 → `task_report` → Router 总结；Router/Worker 工具与提示段正确。
3. 并发上限：`max_router_tasks=1` 时同回合第二次派发失败。
4. `SendToTask` 追加并产生第二份报告；`CancelTask` 对空闲任务返回 `idle`。

单元测试：摘录按字符截断、报告围栏与伪造闭合标签。

尚未覆盖：报告合并、重启恢复、`waiting_for_user` 报告。

前端：设置表单（Router 必填、保存字段）、`task_report` 卡片与链接。

## 9. 备选方案

- **不做，用 `AgentAsTool` 的 `call`/`fan_out`**：已存在，结果同步回到调用者，实现最简单。否决：调用期间主对话被占住，用户无法插话；全部工作挤在一个回合预算里；Worker 没有独立线程、历史与审批上下文。
- **复用 `automatic` 模式的主持人**：已有“选下一个发言者”的循环。否决：主持人没有工具、在单回合内同步推进，做不到派发后释放主对话。
- **重建 `groups` 表以放宽 CHECK 约束**：存储语义最直接。否决：整表重建要迁移全部外键与触发器，风险远大于一列标记；代价是 API 与库存值不一致，需经映射函数读写。

## 10. 代价与重访信号

收益：主对话常驻可用；长任务并行；结果可靠回传。代价：新增 3 个线程列与 2 个群列；Router 多一次模型调用；回报合并让 Router 看到批次而非逐条；模式存储需映射。

重访信号：Router 回报回合频繁超出预算；用户需要 Worker 之间直接协作；需要事件 Trigger 驱动派发（进入 P2）；再次需要改 `scheduler_mode` 约束时一并重建表并去掉映射。

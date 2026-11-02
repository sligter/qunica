# Thread 订阅 Spec

Status: implemented
Version: 0.1
Since: 2026-11-02

路线图 P2。外部话题（topic，文章中的 Thread）指同一个 GitHub issue/PR 或调用方指定的键。同一话题的触发事件归入同一线程；Router 派出的任务订阅话题后，后续事件直通该任务，不再经过 Router。

## 1. 目标与非目标

目标：

- 同一 PR 的评论、review、CI 结果不再各开一个线程。
- 已派发的 Worker 直接收到其话题的后续事件（快路径）；未被订阅的事件交给话题主线程（Router 模式下由 Router 处理，即慢路径）。
- 线程尚未开始的事件批量合并，一次模型调用处理多条积压事件。
- 复用发生项、job、回执的原子事务和幂等语义，不新建事件队列。

非目标：事件过滤（Fast/Slow Path 过滤）、IM 等其他信道、Worker 自行订阅/退订工具、Router 回报与事件合并为同一批次。

## 2. 话题来源

- GitHub（`api/automations/topics.rs::github`）：`repository.full_name` 小写，加编号。`pull_request.number`、`issue.number`、`check_suite/check_run/workflow_run.pull_requests[0].number` → `github:owner/repo#N`；`discussion*` → `github:owner/repo/discussions/N`。issue 与 PR 共用编号空间，所以 PR 上的 `issue_comment` 与其 review、CI 落到同一话题。`push` 等无编号事件没有话题，行为不变。
- 通用 Webhook：签名正文可选 `topic`（1–200 字符，无控制字符）。有 `topic` 时它进入签名身份摘要；没有时摘要与旧版一致。`github:` 前缀保留给已验签的 GitHub 适配器。
- 定时触发没有话题。

## 3. 存储

迁移 `0053_thread_topics.sql`：`thread_topics(thread_id, group_id, topic, created_at)`，主键 `(thread_id, topic)`，线程删除时级联。线程与话题多对多：一个任务可跟随多个话题，一个话题可被多个任务跟随。有 `origin_thread_id` 的线程（派发任务）是订阅者，其他线程是话题主线程。

## 4. 路由（`automations/service.rs::route_topic`）

在 `enqueue` 的同一写事务内，去重和容量检查之后：

1. 群内跟随该话题、状态为 active/running/paused 的派发任务，按订阅时间取前 16 个，逐个投递。消息开头说明“这是本任务跟随话题的新事件”。这些 job 不写 `automation_id`：它们属于该任务，不受“同一自动任务依次执行”约束，暂停自动任务也不取消；仍写 `occurrence_id` 与预算快照，预算和超时照常生效。
2. 没有订阅者时投到话题主线程（最近更新、未归档的那个）；没有则新建线程，标题为 `<自动任务名> · <话题>`，并登记话题。主线程消息沿用自动任务提示词，job 写 `automation_id`。

投递前若线程处于 paused 则恢复为 active，与人工发消息一致。

回执带 `route`（`subscribers`/`home`）；订阅者路由另带 `deliveries` 列表。发生项表仍每事件一行，`job_id` 指向首个投递的 job。

## 5. 批量合并

目标线程存在未被领取（`queued` 且 `worker_active=0`）、来自同一自动任务和同一话题的事件 job 时，新事件追加到该消息与 job 载荷，回执带 `batched: true`。上限每批 20 个事件、消息 256 KiB，超出则新开一个 job。消息 `content_json` 记录 `automation_id`、`topic` 和 `automation_events`（每项含发生项、事件 ID、来源标签）。

## 6. Router 集成（`runtime/router.rs`）

- `DispatchTask` 新增可选 `topics`（最多 8 个）。省略时继承来源线程的话题，`[]` 表示不跟随。返回值列出实际跟随的话题。
- `ListTasks` 返回每个任务的 `topics`。
- `CancelTask` 同时删除该任务的订阅，返回 `unsubscribed`，话题后续事件回到 Router。
- 订阅任务每次运行结束仍按 [Router 回报](router-worker.md#5-回报) 汇报给来源线程。
- Router 与 Worker 提示段各增加一句说明话题与事件来源。

## 7. 前端

线程 API 返回 `topics`。任务选择器为派发任务标注“订阅 <话题>”。

## 8. 安全

事件正文仍包在 `[Untrusted ... data]` 标签中，作为数据而非指令；话题只决定投递位置，不提升权限，审批照常。投递范围限于该自动任务所属的群。

## 9. 测试

- 单元：GitHub 话题推导（PR、issue 评论、review、check suite、workflow run、discussion、无编号）；话题规范化；`topic` 参与签名身份、空值与旧摘要一致、非法与保留前缀被拒。
- `tests/automations.rs`：同 PR 的三类事件进同一线程，积压事件合并为同一 job 且载荷含两者，重放返回原回执，其他 PR 另开线程，线程 API 返回 `topics`；通用话题投给订阅任务（job 不带 `automation_id`），任务归档后回到主线程，`github:` 前缀返回 400。
- `tests/group_stream/router_mode.rs`：Router 从话题线程派发，任务继承话题；第二个事件直通 Worker，Router 只收到两份报告；取消任务后返回 `unsubscribed`，第三个事件回到主线程。

未覆盖：多个订阅者同时投递、批次上限边界、重启时合并中的 job。

## 10. 备选方案

- **不做，Router 用 `SendToTask` 手动转发**：零改动。否决：每个事件都要一次 Router 调用，延迟与成本随事件数线性增长，且 Router 上下文被事件填满。
- **独立的订阅服务/事件总线**：语义最完整。否决：需要第二套持久队列，难以与 job 接收保持同一事务。
- **话题作为 `threads` 的一列**：更简单。否决：无法表达一个任务跟随多个话题、一个话题被多个任务跟随。

## 11. 代价与重访信号

收益：同一外部对象的事件集中；Worker 直接响应其话题；积压事件只花一次调用。代价：新增一张表；订阅任务的 job 脱离自动任务的暂停与串行约束；合并让单次运行看到批次。

重访信号：常见 GitHub 事件推导不出话题；订阅任务积压或互相冲突；需要 Worker 自行退订或转交 Router；需要 IM 信道话题。

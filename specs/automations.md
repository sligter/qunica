# Qunica 自动触发 Spec

Status: proposed
Version: 0.7
Since: 2026-10-17
Implementation: persistent queue, generic and GitHub Webhooks, background timer, client panel and manual stop confirmation implemented and locally verified; real-device and real-model acceptance pending

跨阶段范围、已确认取舍与里程碑见 [总索引](server-sync-computer-automation.md)。

当前队列合同见 [6.4](#64-队列竞争与恢复)：独立线程并行，线程级占用取代下列 2026-10-18 实现记录中的群级占用。

实现进度（2026-10-18）：发生项、独立线程、消息、回合、job 与命令回执在同一写事务接收；CRUD、启停、secret 轮换/撤销、历史与 Webhook 路由已接入；后台每 5 秒 tick 与启动 recovering tick 使用服务端注入的 UTC 时间，无客户端在线也执行。五字段 cron/IANA 时区、夏令时缺失跳过与重复只取第一次、24 小时补跑、HMAC 原始字节与规范化摘要保留离线回归。群级活动 job 唯一约束覆盖人工和自动入口，人工未开始项优先；暂停取消未领取自动项，执行中的任务不受影响；重启后 queued 继续、running 标为 interrupted，需 `turn.acknowledge_stopped` 人工确认后释放群。自动化预算与群当前上限取交集，超时在安全检查点发出取消。客户端 `/settings/automations` 已提供列表、按群组筛选、创建/编辑、未来五次预览、启停、Webhook secret 轮换/撤销、运行历史与中英双语；编辑前的预览走只读 `POST /api/v2/automations/preview`，不产生发生项。GitHub 原生适配器使用 `X-Hub-Signature-256` 校验原始正文，`X-GitHub-Event` 做订阅过滤，`X-GitHub-Delivery` 作为发生项键；`ping` 默认忽略，空事件表接收除 `ping` 外全部事件。投递 ID 头不在 GitHub 签名范围内，因此额外按规范化 payload 摘要去重，改头重放不会创建第二个任务。停止确认界面已接入 Turn 详情，通过 `turn.acknowledge_stopped` 解除重启或不确定电脑操作造成的群阻塞。自动触发 API+SQLite 回归 7 项、领取/取消单元回归 3 项、日程 9 项、验签 8 项；前端新增 9 项页面回归，前端全量 927 项通过；后端全量 983 项通过、2 项 opt-in Docker 用例单独运行通过，Clippy（拒绝 warning）与格式检查通过，结果见 [本地验收记录](automations-local-acceptance.md)。真实三端与真实模型评测未运行。M3 本地实现不依赖 Docker，但不能据此宣称 P1 或整份 spec 已通过。

## 6. P2：时间与事件唤醒群组

### 6.1 自动任务与触发

`automations` 保存 owner、group、名称、提示词、触发配置、启停状态、时区、执行预算、配置版本和最近运行结果。第一版支持一次性时间、五字段 cron 和签名通用 Webhook；GitHub webhook 作为后续首个专用适配器，复用同一持久入口。

自动触发创建独立任务线程，记录来源、事件 ID、计划时间和 automation 版本，复用群成员、拓扑、模型与工具配置。运行时重新校验群组、成员、工作区、电脑和凭据是否仍有效；无效则失败并报告，不能借历史配置获得已撤销权限。

执行快照记录入队时 automation 版本及开始时有效的群配置，支持追溯。Git 项目可复用现有 task worktree；非 Git 任务仍受绑定环境的互斥约束。

### 6.2 时间语义

- 使用 IANA 时区，数据库保存 UTC 的计划发生时间和 next_run_at；调度不依赖客户端时区和在线状态。
- 每次 cron occurrence 的唯一键包含 automation、配置版本和规范化发生时间。夏令时缺失的本地时刻跳过，重复本地时刻只执行一次，并在预览中显示该规则。
- 服务停机后的默认 missed-run 策略为 `coalesce_latest`：仅补最近 24 小时内最新一次，其他记为 skipped；配置可选择全部跳过。MVP 不做无限历史补跑。
- 创建/修改时显示接下来五次执行时间；修改配置与更新 next_run_at 原子提交，不能让旧版本继续生成新发生项。

### 6.3 Webhook 语义

提议入口 `POST /api/v2/automation-hooks/:hook_id`，独立于客户端登录，按自动任务专用 secret 验证。通用适配器使用事件 ID、时间戳和 HMAC-SHA256，签名内容为 `timestamp + "." + raw_body`；允许时差 5 分钟，使用恒定时间比较。请求体上限建议 64 KiB，并设置来源/账号速率与队列容量限制。

通用 JSON 正文采用 `{"event_id":"delivery-id","payload":...}`，事件 ID 必须包含在被签名的原始正文中。不能仅信任可替换的事件 ID 头，否则捕获一次有效请求后可通过改 ID 绕过去重。签名使用原始字节；相同事件内容的去重摘要使用规范化 JSON，以免空白或对象键顺序制造冲突。这些校验函数已测试，HTTP 接收已与发生项、线程、回合、job 和回执的原子事务接通。

`(automation_id, source, external_event_id)` 唯一。同事件同内容返回原 occurrence/job；同 ID 不同内容拒绝。专用 GitHub 适配器已接入同一持久入口，使用其原生签名和 delivery ID，不强迫 GitHub 产生通用格式；delivery 头不在签名内，额外以规范化 payload 摘要防止改头重放。secret 支持轮换与撤销，不能出现在 URL、事件文本或客户端常规读取响应中。

成功 202 必须在发生项与作业原子提交后返回。队列满返回 429 和重试信息，不先确认后丢弃。外部 payload 是有来源标识的非可信数据，不能拼成 system prompt；第一版不执行用户提供的任意过滤脚本。

### 6.4 队列、竞争与恢复

`execution_jobs` 作为统一执行接收记录，状态包括 queued、running、waiting_for_user、succeeded、failed、cancelled、interrupted。原有 turn/dispatch 状态保持事实来源，job 通过关联 ID 反映结果，不能用另一套调度器重新选人。

新任务入口按 owner + thread 占用，人工和自动请求共享线程级租约；同群不同任务线程可以同时执行，同线程仍依次运行。迁移 0046 将 `occupies_group` 改为 `occupies_thread`，用 `ux_execution_jobs_thread_owner` 取代群级唯一索引；`ux_group_turns_one_active_thread` 保留回合级约束。占用独立于状态，取消后的 worker 收尾期间仍阻塞同线程后继。审批等待、重启中断和停止确认仅阻塞或释放对应线程；同一自动任务的发生项仍依次执行，共享电脑仍保留资源级独占。人工任务优先于尚未开始的自动任务，不能隐式抢占正在执行的任务。所有入口共同参与入队和租约裁决，不能只限制定时器而让手动入口绕过。任务历史、ACP 会话与取消按线程隔离；共享目录的文件修改不自动隔离，需要文件隔离时使用现有分支任务/worktree。

审批、停止、接管和继续是针对已有 job 的控制命令，不排在该 job 之后。回答等待中的问题时，消息显式关联等待的 job/问题，由统一入口续接该工作；不能创建一个被原 waiting_for_user job 阻塞的新任务。普通新任务不会自动顶替等待中的工作，用户可明确停止原工作后继续队列。

同一自动任务最多一个 running 和一个排队中的定时发生项；新的定时发生按 missed-run 策略合并。不同 Webhook 事件不静默合并，达到容量后明确拒绝。共享电脑还持有自身执行租约，避免不同群绑定同一环境后发生双重控制。

领取作业使用数据库原子状态迁移和带 generation 的执行租约；重启后 queued 项仍可领取，已开始项失联则标 interrupted。确认旧进程与在途副作用已停止前，不得把租约交给新 worker。已有启动收尾逻辑必须与作业恢复协调，避免 turn 被判失败而 job 仍显示 running。

自动重试默认仅覆盖执行前或已确认无副作用的失败，建议最多 3 次、指数退避。已发生或无法确认的外部副作用、进行中的工具调用和执行中崩溃转人工恢复；不能承诺通过重跑解决所有中断。

需要审批或补充信息时进入 waiting_for_user，持久保存请求并通过 WS 同步，客户端离线也不能自动放行。无人值守触发不改变现有权限；不得自动开启 bypass_approvals。已存在的显式无人值守配置在创建自动任务时显示其实际权限。

“暂停自动任务”禁止新的发生项并取消未开始项；不会暗中停止正在运行的工作。“停止当前运行”只取消指定 job/turn，不关闭未来计划。界面分别提供这两个动作，并明确当前状态。结果写入任务历史，在线客户端通知；系统 Push 另行设计。

### P2 必须通过

1. 可控时钟验证 cron、IANA 时区、夏令时、next_run 更新和停机补跑，不依靠测试实际等待。
2. 没有客户端在线时，定时任务仍创建线程、执行并保存可查询结果。
3. 相同 Webhook 重放、并发交付和响应丢失只产生一个发生项；错签名、旧时间戳与超限请求被拒绝。
4. 接收后崩溃的 queued 工作不丢失；执行中崩溃被标为 interrupted，未确认的外部操作不自动重放。
5. 群忙时人工与自动任务按规定入队；停止、暂停、审批和取消在所有客户端一致。
6. 触发后权限/配置已变化时重新验证；不会隐式提高自动任务权限。

验证分为协议/状态机的离线测试、真实 API+SQLite 的集成测试、三端交互验收，以及显式启用的真实模型任务评测。模型声称完成不能代替文件、数据库或目标服务效果；缺少设备、Docker 或模型凭据的项目记录为“未运行”。文档交付仅检查规范、链接与现状依据，不表示上述实现测试已经通过。

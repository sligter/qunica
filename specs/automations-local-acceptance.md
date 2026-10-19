# 持久自动触发本地验收记录

日期：2026-10-18。范围：M3 的定时、签名 Webhook、原子接收、群级互斥、重启恢复与客户端自动任务面板。
目标合同见 [自动触发 Spec](automations.md)、[多端同步 Spec](realtime-sync.md) 与
[实时同步协议](../backend-rs/crates/backend/src/docs/guide/realtime.md)。

## 环境

Windows 后端，内存 SQLite 或临时文件 SQLite；时间由测试或调用方以 UTC 注入。
执行期间没有 WebSocket/SSE 客户端在线，也没有配置真实模型服务。空群自动任务按
现有调度器返回 `silence`，不能代表模型能力或真实产出。

M3 回归本身不依赖 Docker；引擎恢复后另行运行的受控电脑 opt-in 容器 fixture、
Linux Supervisor HTTP 冒烟和 Docker Runtime 绑定回归通过，结果记录在
[受控电脑实现边界](../backend-rs/crates/backend/src/docs/guide/computers.md#verification)，
不计入本页 M3 通过数量。

## 已观察结果

| 操作 | 独立观察 |
| --- | --- |
| 到期任务 tick，无客户端 | 同一事务写入 occurrence、线程、用户消息、turn、job 与 command receipt；返回 202 后记录均可查询。 |
| 同一到期时刻再次 tick | `next_run_at` 已在上一事务推进，不产生第二个 occurrence。 |
| Webhook 原样重放 | 返回首次的 occurrence/job，不新增行。 |
| 两个相同 Webhook 并发交付 | 两项均 202，指向同一 job，occurrence 计数只增加一次。 |
| 同 event_id 改正文/错签名/旧时间戳/超 64 KiB | 分别返回 409、401、401、413。 |
| 停机两天后 recovering tick | 只补最近 24 小时内最新一次，并写入 skipped 记录；不展开历史。 |
| 群被其他 running job 占用 | 自动任务 job 保持 queued、不占群；暂停后未领取项与 turn 一并取消，占用群的 worker 不受影响。 |
| 重启时 queued 任务 | 重启后继续领取并完成，不丢失已接收工作。 |
| 重启时 running 任务 | 标为 interrupted，同群 queued 任务被阻塞；`turn.acknowledge_stopped` 成功后 queued 任务才继续。 |
| 自动化预算 32k token，群当前 64k | 运行时快照取交集 32k；旧任务不能借历史配置扩大预算。 |
| 未保存的日程预览 | 只读预览接口返回最近五次；非法 cron/时区返回 400，不创建发生项。 |
| 设置页创建、暂停、secret、历史 | 列表显示群组/触发/下次运行/最近状态；暂停携带当前 version；secret 只显示一次；历史区分来源与终态。 |
| GitHub 原生投递 | 校验 `X-Hub-Signature-256`，按 `X-GitHub-Event` 过滤，写 occurrence/job；提示词带 GitHub 事件与 delivery 身份。 |
| GitHub 改头重放/改正文 | 同 payload 换 delivery 返回原 occurrence；同 delivery 换正文返回 409；未订阅事件返回 202 ignored 且不建 occurrence。 |
| GitHub 错签名/缺头/超大 | 401；缺 event/delivery 头被拒绝；超过 1 MiB 返回 413。 |
| 停止确认界面 | Turn 详情在重启或电脑执行不确定时显示确认，确认后调用 `turn.acknowledge_stopped` 并释放群队列。 |

## 自动检查

- `cargo test --test automations -- --test-threads=1`：7 项通过，覆盖原子接收、
  只读预览、通用 Webhook、GitHub 原生投递、补跑合并、暂停取消和重启确认释放。
- `cargo test --lib claim_tests`：3 项通过，覆盖人工优先、群独占、未确认中断和
  取消后等待电脑操作结果。
- 纯函数：日程 9 项、验签 8 项通过。
- 产品回归：既有等待审批的普通 Continue 路径仍复用原 job，不因群级互斥排队死锁；
  暂停线程收到新消息会取代旧的中断检查点并重新激活线程。
- 后端全量 `cargo test -j 2 --workspace`：983 项通过、0 失败、2 ignored；两项
  ignored 均为需真实 Docker 的用案，已分别以 `--ignored` 运行通过。
- 前端新增 9 项页面回归：列表、群组筛选、暂停携带 version、创建 payload、保存前
  预览、GitHub 事件订阅、Webhook secret 只显示一次、运行历史、事件驱动预览；
  前端全量 927 项通过，TypeScript、ESLint 与生产构建通过。
- `cargo clippy --workspace --all-targets -- -D warnings` 与
  `cargo fmt --all -- --check` 通过。
- 桌面与 Android 未在本轮改动，也没有运行三端验收。

## 尚未运行

真实模型执行评测、真实三端联测、VPS 的 HTTPS/域名与弱网/负载基线。GitHub 适配器
与停止确认界面已由本地回归覆盖，但仍需在真实 GitHub 组织和三端上验收。以上仍是
P2 与整体 P0/P1 的门槛，不能由本地协议、SQLite 与页面回归替代。

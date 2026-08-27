<p align="center">
  <img src="assets/qunica-logo.png" alt="Qunica 标志" width="160">
</p>

<h1 align="center">Qunica</h1>

<p align="center">
  <strong>以群组为核心的工作台：人和 AI Agent 在同一间房间里一起做事。</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.1_alpha-informational?style=flat" alt="版本">
  <img src="https://img.shields.io/badge/desktop-Windows-0078D4?style=flat" alt="平台">
  <img src="https://img.shields.io/badge/stack-Tauri%20%2B%20Rust%20%2B%20React-informational?style=flat" alt="技术栈">
</p>

<p align="center">
  <a href="#这是什么">概览</a>
  ·
  <a href="#开始使用">开始使用</a>
  ·
  <a href="#建好这个房间">功能</a>
  ·
  <a href="#外部-runtime">Runtime</a>
</p>

<p align="center">
  <a href="README.md">English</a> | <b>简体中文</b>
</p>

<p align="center">
  <sub><em>一个项目群 · 多个 Agent · 共享历史、文件与工作区</em></sub>
</p>

---

## 为什么叫 Qunica？

**群**（qún，the group）+ **quorum**——够人数到场，事才议得起来。Qunica 里的项目群正是如此：人和 Agent 在同一间房里到齐，工作才算开始。读作 /ˈkwiːnɪkə/（“KWI-ni-ka”），中文可叫「群卡」。

Logo 讲的是同一个故事：五个对话气泡向同一个火花收敛——那就是 quorum 本身。

---

## 这是什么？

多数 AI 产品只给你「一个聊天窗口 + 一个 Agent」。可真实工作是个团队：产品、研究、研发、审查、文档——不同角色，同一个项目上下文。Agent 越多，你花在几个互不认识的标签页之间搬运上下文的时间就越多。

Qunica 把项目当成一个 **群（Group）**。Agent 是你可以邀请、配置、观察的成员；人和 Agent 共享同一份消息历史、文件、工作区与执行轨迹。每个 Agent 有自己的模型、工具、Skills 与工作区绑定，也可以通过 Agent Client Protocol 驱动外部 CLI Agent（Codex CLI、Claude Code、Pi、OpenCode、DeepSeek Harness），在你选定的 workspace 里干真实活。

我们赌的是这一条：多 Agent 协作想变好，得让**房间本身成为产品**。一个群、一份对话日志、一条工作区边界。每个 Agent 各开一个聊天孤岛的路子，走不通。

---

## 建好这个房间。

*建群、拉人，项目的上下文从此都留在这一个房间里。*

- **[群组](backend-rs/crates/backend/src/docs/guide/groups.md) →** 群是协作的容器。像拉同事进群一样邀请专业 Agent；所有成员共享一份历史和一个工作区。
- **[群模板](backend-rs/crates/backend/src/docs/guide/groups.md) →** 把成员名单和设置存成可复用模板，下一个项目直接套用；名称、头像与工作区仍然每个群单独选。
- **[Agent](backend-rs/crates/backend/src/docs/guide/agents.md) →** 每个成员有自己的名称、提示词、模型、工具、Skills 与工作区；同一个 Agent 可以加入多个群、开多段对话。
- **[私聊](backend-rs/crates/backend/src/docs/guide/direct-chats.md) →** 同样跑在统一调度器上的一对一会话，适合不需要协调的活。
- **任务线程 →** 一段对话里可以钉住多条工作线，各自归档、恢复、删除或单独清空。
- **[共享笔记](backend-rs/crates/backend/src/docs/guide/groups.md#shared-notes) →** 群的 Markdown 记事本，每个成员都能读能改，就摆在聊天旁边，不塞进聊天里。

## 定好对话的规则。

*谁先说、能说几句、烧多少 `token`，你来定；轮到谁接话，调度器自己走。*

- **[通信拓扑](backend-rs/crates/backend/src/docs/guide/groups.md#communication-modes) →** `mesh`、`star`、`hierarchical`、`ring` 决定合法的发言路径；`speaking_order` 让它确定化。
- **[调度模式](GROUP_SCHEDULER.md) →** `bounded` 在预算内运行；`automatic` 让主持 Agent 反复派发或结束本轮。群聊与私聊共用同一个持久化调度器。
- **主持 Agent →** 一个带着自己供应商与模型的成员，替你挑选下一个合法发言人，不用死守固定顺序。
- **@ 提及 →** 提及模式下由 @ 指定应答者；群发模式下 @ 只决定开场发言人。Agent 写出来的 @ 只是展示文字，永远不会触发派发。
- **[预算与失败熔断](GROUP_SCHEDULER.md#budget-profiles) →** 限制总步数、单人步数、交接跳数、主持调用次数与 token；连续失败会停掉本轮而不是烧穿它。
- **[AgentAsTool](GROUP_SCHEDULER.md) →** 结构化委托：`call` 私下调用帮手并拿回结果，`handoff` 把公开回复交出去——同一个 Agent 不会被唤醒两次。

## 把真活交给它们。

*干哪个目录、能用哪些工具，先过你这一关；跑过的每一步，都留下记录。*

- **[工作区](backend-rs/crates/backend/src/docs/guide/workspaces.md) →** 文件和 shell 工具的所有路径都对着根目录解析，越界的请求直接拒绝。
- **[内置工具](backend-rs/crates/backend/src/docs/guide/agents.md#built-in-tools) →** 读、写、精确编辑、Glob、Grep、受守卫的 Bash、WebSearch、Fetch、图片与视频生成、AskUser、TodoWrite、计划审批。
- **审批门禁 →** 破坏性 Bash 会让本轮暂停等你确认——可以记住规则，也可以给信得过的 Agent 开无人值守模式；高危一类命令（格式化磁盘、关停主机）永不执行。
- **[外部 CLI Agent](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md) →** 通过 ACP 驱动 Codex CLI、Claude Code、Pi、OpenCode、DeepSeek Harness，以及任何自定义 ACP 服务。每次运行都记录命令、工作目录、状态、退出码与 stdout/stderr 尾部。
- **[MCP 服务](backend-rs/crates/backend/src/docs/guide/mcp-servers.md) →** 接入 `stdio`、Streamable HTTP、SSE 三种传输；工具以 `mcp__<服务>__<工具>` 命名，服务级白名单 + Agent 级再筛选。
- **[Skills](backend-rs/crates/backend/src/docs/guide/skills.md) →** 可复用的指令块，Agent 需要时通过 `SkillManager` 加载；支持原文粘贴、包导入、GitHub 导入。
- **[供应商](backend-rs/crates/backend/src/docs/guide/providers.md) →** OpenAI 兼容、Anthropic、Gemini 三种接口方言；模型发现由后端发起，API 密钥不出本机。
- **单条消息覆盖 →** 只对某一条消息切换模型或思考程度。
- **工作区 Git →** 状态、分支、差异、历史、暂存、提交与同步，都在应用内完成。

## 全程看得见。

*每轮对话都留底，能回放、能查——活干到哪，一眼就有。*

- **实时流式 →** token、消息、错误与 turn trace 实时出现在房间里。
- **[Turn trace](GROUP_SCHEDULER.md) →** 哪个 Agent 跑了、为什么选它、花了多少——按次派发、按轮持久化。
- **[终端](backend-rs/crates/backend/src/docs/guide/terminal.md) →** 仅桌面版的标签页终端，停靠在对话底部（`Ctrl`/`Cmd` + `` ` ``）。它从工作区启动但**故意不做**沙箱，用之前先读文档。
- **[内置助手](backend-rs/crates/backend/src/docs/guide/assistant.md) →** 配置答疑，代改配置先暂存、你批准才生效；不碰文件，永远读不到明文密钥。
- **日志 →** 应用内可见，落盘位置 `%APPDATA%\qunica.desktop\logs`。

## 机器是你的。

*本地优先。数据在你自己的盘上，跑着的服务也只有你自己开的那一个。*

- **Windows 桌面 →** Tauri 2 外壳 + 进程内 Rust 后端：托盘常驻、原生目录选择器、免安装的 portable 可执行文件。
- **浏览器版 →** 同一个 React 前端可以跑在浏览器里（`pnpm dev`），连本地后端即可，不必套桌面壳。
- **SQLite 存储 →** 群、Agent、轮次与历史都在 `%APPDATA%\qunica.desktop\qunica.sqlite3`。登录令牌由本机生成的密钥签名。
- **还不是托管 SaaS →** 注册与登录都对着你自己的后端，没有别人服务器上的账号。

---

## 开始使用

当前首要打包目标是 Windows。从源码构建：

```powershell
pnpm install
pnpm desktop:build
```

构建产物：

```text
frontend/src-tauri/target/release/bundle/nsis/Qunica_<version>_x64-setup.exe
frontend/src-tauri/target/release/bundle/portable/Qunica_<version>_x64-portable.exe
```

portable 版直接双击运行，无需安装。开发模式：

```powershell
pnpm dev          # 浏览器里跑 Web UI
pnpm desktop:dev  # 桌面应用开发模式
```

### 五分钟跑起你的第一个 Agent

1. **添加供应商。** Qunica 调模型用的 API 密钥；没有它，任何 Agent 都无法回复。
2. **添加工作区。** 一个允许 Agent 读写的本地目录。
3. **创建 Agent。** 绑定供应商、工作区、系统提示词与工具集。
4. **和它说话。** 开一个一对一的私聊，或者建个群多拉几个 Agent。

完整教程：[快速上手](backend-rs/crates/backend/src/docs/guide/getting-started.md)。

---

## 外部 Runtime

Qunica 不带模型。它驱动你已经安装并登录好的 Agent CLI，换供应商只是下拉框里选一下，用不着迁移。

| Runtime | CLI | 说明 |
| --- | --- | --- |
| Claude Code | `claude` | 带工具调用的流式输出，处理权限问答 |
| OpenAI Codex | `codex` | 沙箱执行剖面 |
| Pi Agent | `pi` | ACP 适配器 |
| OpenCode | `opencode` | ACP 服务 |
| DeepSeek Harness | `dsh` | 仅提示词的 ACP 界面；按模式的沙箱隔离，不可用时闭合失败 |
| 自定义 ACP 服务 | 任意 | 任何通过 stdio 说 Agent Client Protocol 的程序 |

举例：`codex` 以 `codex exec --sandbox danger-full-access <prompt>` 运行，`claude` 以 `claude -p --output-format stream-json --permission-mode bypassPermissions --max-turns <n> <prompt>` 运行。Qunica 只负责检测与启动这些 CLI，**不保存**它们的账号凭据——请在应用外安装并登录。Full-auto 模式能力很强：只绑定你舍得让它改的工作区。细节见 [外部 CLI Agent](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md)。

---

## 文档

| 我想… | 从这里开始 |
| --- | --- |
| 搞懂概念、今天就跑起来 | [快速上手](backend-rs/crates/backend/src/docs/guide/getting-started.md) |
| 建群、定路由与对话规则 | [群组](backend-rs/crates/backend/src/docs/guide/groups.md) · [调度器设计](GROUP_SCHEDULER.md) |
| 配置 Agent 与工具 | [Agent](backend-rs/crates/backend/src/docs/guide/agents.md) · [Skills](backend-rs/crates/backend/src/docs/guide/skills.md) |
| 用 ACP 驱动外部 CLI Agent | [外部 CLI Agent](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md) |
| 接 MCP 工具服务 | [MCP 服务](backend-rs/crates/backend/src/docs/guide/mcp-servers.md) |
| 配置供应商与全局设置 | [供应商](backend-rs/crates/backend/src/docs/guide/providers.md) · [设置](backend-rs/crates/backend/src/docs/guide/settings.md) |
| 使用文件与工作区 | [工作区](backend-rs/crates/backend/src/docs/guide/workspaces.md) · [工作区文件](backend-rs/crates/backend/src/docs/guide/workspace-files.md) |
| 使用终端或内置助手 | [终端](backend-rs/crates/backend/src/docs/guide/terminal.md) · [助手](backend-rs/crates/backend/src/docs/guide/assistant.md) |

完整指南在 [`backend-rs/crates/backend/src/docs/guide/`](backend-rs/crates/backend/src/docs/guide/)。

---

## 开发

环境要求：[Node.js](https://nodejs.org/) ≥ 20、[pnpm](https://pnpm.io/) 9、稳定的 [Rust](https://rust-lang.org/) 工具链；桌面打包需要 Windows。

```powershell
pnpm install
pnpm dev          # Web UI，带热更新
pnpm desktop:dev  # 桌面应用开发模式
```

质量检查：

```powershell
pnpm type-check
pnpm lint
cargo fmt --manifest-path backend-rs/crates/backend/Cargo.toml --all --check
cargo clippy --manifest-path backend-rs/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```

---

<p align="center">
  <sub>Qunica</sub><br>
  <sub>本地优先的多 Agent 协作 · v0.1.1-alpha</sub>
</p>
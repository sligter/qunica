<p align="center">
  <img src="assets/ag-swarmer-logo.png" alt="AG Swarmer Logo" width="160">
</p>

<h1 align="center">AG Swarmer</h1>

<p align="center">
  <strong>以群组为核心的工作台：人和多个 Agent 在同一间房间里一起做事。</strong>
</p>

<p align="center">
  <a href="README.md">English</a>
  ·
  <a href="#这到底是什么">概览</a>
  ·
  <a href="#开始使用">开始使用</a>
  ·
  <a href="#技术架构">架构</a>
  ·
  <a href="#当前状态">状态</a>
</p>

<p align="center">
  <sub><em>一个项目群 · 多个 Agent · 共享历史、文件与工作区</em></sub>
</p>

---

## 这到底是什么？

AG Swarmer 是一个以 **群组（Group）** 为主要交互容器的多 Agent 协作工作台。

多数 AI 产品是「一个聊天窗口 + 一个 Agent」。真实工作更像团队：产品、研究、研发、审查、文档——不同角色，同一项目上下文。AG Swarmer 把项目当成一个 **群**：Agent 是你可以邀请、配置、观察的成员。人和 Agent 共享同一份消息历史、文件、工作区与执行轨迹。

Agent 是群里的正式成员：有自己的角色、模型、工具、Skills、MCP 服务，也可以挂外部 CLI runtime（Codex CLI、Claude Code），在绑定的 workspace 里直接执行任务。

---

## 你能用它做什么

- **拉起一个项目群**，按角色邀请多个专业 Agent，像拉同事进群一样。
- **保留长期项目上下文**：消息、文件、工作区状态、执行记录与 Agent 回复都在同一空间。
- **给每个 Agent 分派职责**：不同模型、工具权限、Skills 与工作区绑定。
- **实时看执行过程**：流式 token、最终消息、错误与 turn trace 直接出现在群里。
- **让外部 CLI Agent 动真实文件**（在用户确认过的 workspace 内），并保留命令、工作目录、退出码与 stdout/stderr tail 审计。
- **在应用内查看工作区 Git**：分支、差异、历史、暂存、提交与同步操作都留在应用里。
- **接入 MCP 工具**（stdio / Streamable HTTP / SSE），让外部能力像内置工具一样可调。
- **使用内置助手** 解答配置与用法；代为改配置时先暂存，你批准后才生效。
- **在本机轻量运行** Windows 桌面版：托盘常驻、原生目录选择、免安装可执行文件。

---

## 为什么是这种形态

一个群、一份对话日志、一条工作区边界，项目上下文都收在这三样里。

人、LLM Agent、外部 CLI Agent、MCP 工具与 Skills 汇合在同一个项目房间，不用在七个互不认识的标签页之间来回切。边界来自成员关系、工作区绑定与工具白名单，权限跟着成员走，全局开关那一套用不上。

这个项目赌的是 **房间** 本身就是产品：多 Agent 协作的上下文、执行与记录都在同一处，Agent 不必各开一个聊天孤岛。

---

## 三个小故事

**功能群蜂。** 你把 PRD 丢进有产品 / 后端 / 前端 / 测试 Agent 的群。它们讨论、起草接口、提出测试，并留下可回看的「为什么这样设计」痕迹。

**绑定工作区落地。** 你把本地仓库绑成群 workspace，拉进 Codex 或 Claude Code，改动发生在你看得见的地方——讨论与实现同室，每条外部命令都有 runtime 审计。

**安全的配置帮手。** 卡在供应商或 MCP 上时，内置助手用「暂存操作」提议更改；你点批准前什么都不会真改。密钥始终掩码，危险路径它够不着。

---

## 当前状态

| ✅ 已经可用 | 🚧 持续打磨 | 💭 方向，不是承诺 |
|---|---|---|
| 注册 / 登录 / JWT 鉴权 | Windows 以外的桌面打包 | 更丰富的多 Agent 编排策略 |
| 群组、Agent、入群、群聊 | 更深入的工作区 Git 审查体验 | Agent 市场 / 能力包分发 |
| 流式回复、清空消息、turn trace | 移动端 / 轻量远程查看 | 更强的企业知识库接入 |
| 工作区文件浏览、引用、UTF-8 编辑、安全预览 | 从旧版 Docker/Postgres 环境迁移数据 | — |
| 工作区 Git：状态、分支、差异、历史、暂存/提交/同步 | — | — |
| LLM 供应商配置；单条消息覆盖模型与思考程度 | — | — |
| Skills 管理与注入 | — | — |
| MCP：stdio · Streamable HTTP · SSE | — | — |
| 外部 CLI runtime：Codex CLI、Claude Code | — | — |
| 内置助手 + 批准后才生效的应用内操作 | — | — |
| Windows 桌面：Tauri、进程内 Rust 后端、SQLite、托盘、免安装可执行文件 | — | — |

<sub>请围绕 ✅ 列规划使用；💭 列是产品方向，不是发版清单。</sub>

---

## 开始使用

### 我想要 Windows 桌面版

从源码构建（当前首要打包目标是 Windows）：

```powershell
pnpm install
pnpm desktop:build
```

构建产物：

```text
frontend/src-tauri/target/release/bundle/nsis/AG Swarmer_<version>_x64-setup.exe
frontend/src-tauri/target/release/bundle/portable/AG Swarmer_<version>_x64-portable.exe
```

免安装版：直接运行独立的 `AG Swarmer_<version>_x64-portable.exe`。

### 我想开发 Web UI

```powershell
pnpm install
pnpm dev
```

### 我想跑桌面开发模式

```powershell
pnpm install
pnpm desktop:dev
```

### 质量检查

```powershell
pnpm type-check
pnpm lint
```

Rust 后端：

```powershell
cargo fmt --manifest-path backend-rs/crates/backend/Cargo.toml --all --check
cargo clippy --manifest-path backend-rs/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```

---

## 核心概念

| 概念 | 含义 |
|---|---|
| **Group** | 群组 / 项目空间，协作主容器 |
| **Agent** | 可复用的 AI 成员，可加入不同群组 |
| **Workspace** | 群组绑定的本地工作目录；外部 Agent 与工具在此读写 |
| **Runtime** | Agent 执行方式：LLM chat runtime 或 external CLI runtime |
| **Streaming** | 前端实时展示 token / 最终消息 / 错误 |
| **Audit** | 外部运行记录命令、工作目录、状态、退出码、stdout/stderr tail |
| **MCP** | 以 `mcp__<服务名>__<工具名>` 暴露的 Model Context Protocol 工具 |
| **Assistant** | 内置助手：解答用法 + 暂存配置更改（批准后生效） |

---

## 能力亮点

### MCP 工具

在资源库的 **MCP 服务** 中登记后，Agent 可像调用内置工具一样使用它们。

| 协议 | 说明 |
|---|---|
| `stdio` | 启动本地进程，stdin/stdout 收发按行分隔的 JSON-RPC 2.0 |
| `Streamable HTTP` | 单个 HTTP 端点；返回 JSON 或 SSE；会话靠 `Mcp-Session-Id` |
| `SSE`（旧版） | GET 打开事件流，再 POST 到 `endpoint` 事件给出的消息地址 |

- 工具命名：`mcp__<服务名 slug>__<工具名>`，避免跨服务重名冲突。
- 服务级白名单 + Agent 级再筛选。
- 保存前可「测试连接」，查看真实暴露的工具列表。
- 连不上的服务只让本回合缺席其工具，并在系统提示中注明，不会拖垮整回合。
- stdio 继承当前进程环境并叠加配置覆盖；HTTP 请求头值在接口中掩码返回。

### 工作区文件引用与预览

- 从工作区面板拖入文件时，消息保存的是工作区内 **相对路径引用**，不复制内容；发送前服务端确认路径属于当前会话绑定工作区。
- 拖入目录不会递归塞文件，只在光标处插入目录相对路径。
- 应用内编辑仅支持 UTF-8 文本；保存带打开时的内容摘要，外部已改则保留本地草稿并要求确认，避免静默覆盖。
- HTML 预览在无脚本、无同源、无导航权限的沙箱 iframe 中；图片与 PDF 有大小限制的安全预览。
- Office / 未知 / 无法安全预览的格式：仅元数据 + 原文件下载。

### 内置助手

悬浮面板，帮助配置、解答用法，并代为执行应用内操作。它是带 `is_system` 标记的系统 Agent，走普通私聊，因此流式、恢复、中断与 turn trace 全部复用现有机制。它不出现在 Agent 库与私聊列表中，也无法被通用 Agent 接口改删。

**边界：**

- 没有工作区、没有文件工具、没有 shell。需要读写文件请用绑定了 workspace 的普通 Agent。
- 所有配置更改都是 **暂存** 的：`AppPropose` 只写待批准记录；**批准接口** 才是唯一改数据的路径，且走与 UI 相同的 handler。
- 以下内容完全不能暂存，只能 `AppPrefill` 预填表单由你完成：供应商 API 密钥、stdio MCP、CLI runtime 安装、任何删除。
- 读配置永远拿不到明文密钥。

记录在 **设置 → 助手操作记录**。标题栏齿轮可改助手使用的供应商与模型；提示词、工具与「无工作区」固定不变，它才能安全持有配置工具。助手需 **单独绑定** 供应商，添加供应商不会自动绑定。

### 单条消息的模型与思考程度

输入框可为单条消息覆盖模型与思考程度。

- 模型选择器仅在「会话只有一个 Agent 且供应商提供多模型」时出现。
- 思考程度仅在模型声明支持时出现（OpenAI `reasoning_effort`、Anthropic `thinking.budget_tokens`、Gemini `thinkingConfig`）。

### 外部 CLI Agent

独立于普通 LLM chat 的 runtime，在解析后的 workspace 中运行，流式回传到会话。

| Runtime | 调用形态（示意） |
|---|---|
| Codex CLI | `codex exec --sandbox danger-full-access <prompt>` |
| Claude Code | `claude -p --output-format stream-json --permission-mode bypassPermissions --max-turns <n> <prompt>` |

应用只负责检测与启动 CLI，**不保存** 其账号凭据。请在应用外自行安装并登录。Full-auto CLI 能力很强：只绑定你确认过的 workspace。

---

## 技术架构

```text
frontend/
  React + Vite + TypeScript + TanStack Query + Zustand
  Tauri desktop shell：src-tauri/；进程内链接 Rust 后端

backend-rs/
  Rust backend workspace
  Axum HTTP API 与 API v2 runtime
  SQLite 桌面数据存储
  外部 CLI runtime 适配

shared/
  跨包 TypeScript 事件 / 契约
```

桌面运行结构：

```text
AG Swarmer.exe
  ├─ Tauri WebView shell
  ├─ 在进程内启动 Rust / Axum 后端
  ├─ 监听 http://127.0.0.1:8765
  └─ 前端通过 runtime API base URL 访问后端
```

```text
┌──────────────────────────────────────────────────────────────┐
│ 客户端                                                       │
│  Web (Vite)          Desktop (Tauri WebView)                 │
└──────────────┬───────────────────────────┬───────────────────┘
               │ HTTP / 流式事件           │
               ▼                           ▼
┌──────────────────────────────────────────────────────────────┐
│ ag-swarmer-backend（Rust / Axum）                            │
│  鉴权 · 群组 · Agent · 聊天 · 工作区 · MCP · runtime         │
└──────────────┬───────────────────────────┬───────────────────┘
               │                           │
        ┌──────▼──────┐             ┌──────▼──────┐
        │   SQLite    │             │  Workspace  │
        │  （桌面）   │             │  + CLI/MCP  │
        └─────────────┘             └─────────────┘
```

---

## 它不是什么

- **不是单聊聊天机器人。** 工作单元是「有成员的群」。
- **还不是托管多租户 SaaS。** 桌面默认本机 SQLite。
- **还没做完。** 首个打包目标是 Windows；外部 CLI 的 full-auto 请谨慎使用。

**它是什么：** 群形态的多 Agent 工作台——共享上下文、执行过程可见，Agent 能在你选定的工作区里干活。

---

<p align="center">
  <sub>AG Swarmer</sub><br>
  <sub>本地优先的多 Agent 协作 · v0.1.1-alpha</sub>
</p>

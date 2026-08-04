<p align="center">
  <img src="assets/ag-swarmer-logo.png" alt="AG Swarmer Logo" width="160">
</p>

# AG Swarmer

AG Swarmer 是一个以“群组”为核心交互容器的多 Agent 协作工作台。它把多个 AI Agent 放进同一个项目空间里，让用户像管理一个团队一样，创建角色、分配任务、查看上下文、观察执行过程，并让 Agent 围绕同一份群聊历史、文件和工作区协同完成复杂任务。

项目当前同时支持 Web 开发形态和 Windows 桌面形态。桌面版使用 Tauri 打包前端，并把 Rust 后端作为 sidecar 一起启动，打包后的应用可在本机用 SQLite 运行，不依赖 Docker、Postgres、Redis 或 MinIO。

## 项目定位

传统 AI 对话产品通常是“一个聊天窗口 + 一个 Agent”。AG Swarmer 面向更接近真实工作的场景：一个项目往往需要产品、研发、测试、研究、文档、代码审查等多个角色协作。这里的“群”就是项目容器，Agent 是可以被邀请进群的成员。

适合的使用场景：

- 让多个专业 Agent 围绕同一个项目需求并行分析、讨论和输出。
- 在群空间内保存长期上下文，包括消息、文件、工作区、执行记录和 Agent 回复。
- 给每个 Agent 配置不同角色、模型、工具权限和工作区。
- 使用外部 CLI Agent，例如 Codex CLI、Claude Code，让它们在指定 workspace 内直接执行任务。
- 在本机桌面环境中运行一个轻量的多 Agent 协作工具。

## 核心概念

- **Group**：群组/项目空间，是协作的主容器。
- **Agent**：可复用的 AI 成员，可以加入不同群组。
- **Workspace**：群组绑定的本地工作目录，外部 Agent 和工具在这里读取、修改文件。
- **Runtime**：Agent 的执行方式。当前包含 LLM chat runtime 和 external CLI runtime。
- **Streaming**：Agent 输出通过流式事件返回，前端可实时展示 token、最终消息和错误。
- **Audit**：外部 Agent 运行会记录命令、工作目录、状态、退出码和 stdout/stderr tail，便于排查问题。

## 当前能力

- 用户注册、登录和 JWT 鉴权。
- 创建和管理群组。
- 创建和管理 Agent。
- Agent 加入群组并参与群聊。
- 群消息、清空消息、流式回复。
- 群组 workspace 文件浏览。
- LLM provider 配置。
- Skills 管理和注入。
- 内置助手：悬浮面板，可查看配置、解答用法，并在用户批准后代为修改配置。
- MCP 工具接入：stdio、Streamable HTTP、SSE 三种传输协议。
- 外部 CLI Agent runtime：
  - Codex CLI：`codex exec --sandbox danger-full-access <prompt>`
  - Claude Code：`claude -p --output-format stream-json --permission-mode bypassPermissions --max-turns <n> <prompt>`
- Windows 桌面版：
  - Tauri shell。
  - Rust backend sidecar。
  - SQLite 本地数据存储。
  - 系统托盘常驻。
  - 原生目录选择。
  - 免安装 zip。

### MCP 工具

在资源库的 **MCP 服务** 中登记 Model Context Protocol 服务后，Agent 就可以像调用内置工具一样调用它提供的工具。支持三种标准传输协议：


| 协议                | 说明                                                                                                |
| ----------------- | ------------------------------------------------------------------------------------------------- |
| `stdio`           | 启动本地进程，通过其 stdin/stdout 收发按行分隔的 JSON-RPC 2.0 消息。                                                  |
| `Streamable HTTP` | 单个 HTTP 端点，POST JSON-RPC，服务端返回 `application/json` 或 `text/event-stream`；会话通过 `Mcp-Session-Id` 保持。 |
| `SSE`（旧版）         | GET 打开事件流，服务端用 `endpoint` 事件告知消息端点，之后 POST 请求、响应从事件流返回。                                           |


工具命名与生效方式：

- 服务的工具会以 `mcp__<服务名 slug>__<工具名>` 的形式暴露给模型，因此不同服务的同名工具不会互相冲突。
- 每个服务可以配置工具白名单；每个 Agent 还可以在自己的工具设置里进一步只选其中一部分。
- 保存前可以点击“测试连接”，直接看到该服务实际暴露了哪些工具。
- 连接不上的服务只会让它自己的工具在本回合缺席，并在系统提示里注明原因，不会让整个回合失败。
- stdio 服务会继承当前进程的环境变量，并叠加配置中的环境变量覆盖项；HTTP 服务的请求头值在接口返回时以掩码显示。

### 工作区文件引用与预览

- 从工作区文件面板拖入文件时，消息保存的是工作区内的文件引用，不会复制文件内容或创建副本；发送前仍会由服务端确认该相对路径属于当前群聊或私聊的工作区。
- 拖入目录时不会递归添加其中的文件，而是在消息编辑器光标位置插入目录的工作区相对路径。
- 应用内编辑仅支持服务端确认的 UTF-8 文本。保存请求会携带打开文件时的内容摘要；如果文件已被外部程序修改，应用会保留本地草稿并要求确认后刷新，避免静默覆盖。
- HTML 预览运行在不授予脚本、同源或导航权限的沙箱 iframe 中；图片和 PDF 使用受大小限制的安全预览。
- Office 文档、未知格式和无法安全预览的文件不做应用内转换或编辑，仅显示元数据并提供原文件下载。

### 内置助手

应用内置一个悬浮助手面板，帮助用户完成配置、解答用法，并代为执行应用内的操作。

它是一个 `is_system` 标记的系统 Agent，通过普通私聊运行，因此流式输出、恢复、中断和 turn trace 全部复用现有机制。它不出现在 Agent 库和私聊列表中，也无法通过通用的 Agent 接口修改或删除。

**它不能做什么**：

- 没有工作区，没有文件工具，没有 shell。需要读写文件的任务应交给绑定了工作区的普通 Agent。
- 所有配置更改都是**暂存**的。助手调用 `AppPropose` 只会写入一条待批准记录，界面上出现批准卡片，用户批准后才真正生效。批准接口是唯一会改动数据的路径，并且它调用的正是 UI 走的同一套 handler，因此暂存的更改无法绕过任何校验。
- 以下内容助手完全无法暂存，只能通过 `AppPrefill` 返回一个预填好的表单链接，由用户自行完成：
  - 供应商 API 密钥
  - 启动本地进程的 MCP 服务（`stdio` 传输）
  - CLI runtime 安装
  - 任何删除操作
- 读取配置时永远拿不到明文密钥：供应商只返回"是否已配置"，MCP 只返回请求头名称。

助手提议过的所有更改及其结果，记录在 **设置 → 助手操作记录** 中。

面板标题栏的齿轮图标可随时打开助手设置，修改它使用的供应商和模型；提示词、工具和"无工作区"是固定的，因为这些正是它能安全持有配置工具的前提。

助手本身也是 LLM Agent，需要为它单独绑定一个供应商才能对话——这与其他 Agent 使用的供应商是独立的，添加供应商不会自动绑定。绑定前面板会列出已有的供应商供选择；如果一个都没有，则提供创建入口。

### 单条消息的模型与思考程度

消息输入框可以为单条消息选择模型和思考程度，覆盖 Agent 自身的配置。

- 模型选择器仅在会话只有一个 Agent 且其供应商提供多个模型时出现——多 Agent 会话中无法确定该选择应用于谁。
- 思考程度仅在所选模型声明支持时出现。三档（low/medium/high）会按供应商各自的语义映射：OpenAI 的 `reasoning_effort` 枚举、Anthropic 的 `thinking.budget_tokens`、Gemini 的 `thinkingConfig`。

## 技术架构

```text
frontend/
  React + Vite + TypeScript + TanStack Query + Zustand
  Tauri desktop shell under src-tauri/

backend-rs/
  Rust backend workspace
  Axum HTTP API and API v2 runtime
  SQLite desktop data storage
  External CLI runtime adapters

shared/
  Cross-package TypeScript event/contracts
```

桌面版运行结构：

```text
AG Swarmer.exe
  ├─ Tauri WebView shell
  ├─ starts ag-swarmer-backend.exe sidecar
  ├─ waits for http://127.0.0.1:8765/api/v2/health
  └─ frontend calls backend through runtime API base URL
```

## Windows 桌面版

构建安装包和免安装 zip：

```powershell
pnpm install
pnpm desktop:build
```

构建产物：

```text
frontend/src-tauri/target/release/bundle/nsis/AG Swarmer_0.1.0_x64-setup.exe
frontend/src-tauri/target/release/bundle/portable/AG Swarmer_0.1.0_x64-portable.zip
```

### 本地终端

仅 Tauri 桌面应用可在已绑定的本地工作区中打开多标签交互终端。使用 Ctrl/Cmd +  展开或折叠终端面板。

终端运行完整的宿主 Shell，并非工作区沙箱；它可访问当前账户权限允许的工作区外文件和进程。切换聊天或将应用隐藏到系统托盘不会停止终端；真正退出应用会结束 PTY 及其后代进程。重启应用只恢复标签元数据和启动目录，不恢复旧进程、命令输入或输出。用户终端与 Agent 工具执行彼此独立。

免安装版解压后直接运行 `AG Swarmer.exe`。不要把 `ag-swarmer-backend.exe` 从同一目录移走。

## 桌面运行行为

- 点击窗口关闭按钮时，应用会隐藏到系统托盘，不会退出。
- 左键点击或双击托盘图标会打开主页面。
- 右键托盘图标可打开主页面、系统设置、日志目录，或退出应用。
- 启动后端前，launcher 会清理正在监听 TCP `127.0.0.1:8765` 的旧进程，避免旧 sidecar 占用端口导致启动失败。
- 桌面端 workspace 选择使用 Tauri 原生目录选择器，保存真实文件系统路径。

## 桌面数据与日志

桌面版数据目录：

```text
%APPDATA%\dev.ag-swarmer.desktop
```

重要文件：

```text
%APPDATA%\dev.ag-swarmer.desktop\ag-swarmer.sqlite3
%APPDATA%\dev.ag-swarmer.desktop\desktop-secret.key
```

日志目录：

```text
%APPDATA%\dev.ag-swarmer.desktop\logs\launcher.log
%APPDATA%\dev.ag-swarmer.desktop\logs\backend.log
```

也可以从托盘右键菜单打开日志目录，或运行：

```powershell
explorer "$env:APPDATA\dev.ag-swarmer.desktop\logs"
```

`desktop-secret.key` 会在桌面版首次启动时生成并复用，用于 JWT 签名。如果删除该文件，已有登录 token 可能失效，重新登录即可。

## 外部 CLI Agent

外部 CLI Agent 是独立于普通 LLM chat 的 runtime。它们会在解析后的 workspace 目录中运行，并以流式输出回传到群聊。

当前支持：

- Codex CLI
- Claude Code

桌面应用只负责检测和启动这些 CLI，不保存它们的账号凭据。用户需要在应用外自行安装并完成登录。

## 开发

前端 Web 开发：

```powershell
pnpm dev
```

桌面开发：

```powershell
pnpm desktop:dev
```

质量检查：

```powershell
pnpm type-check
pnpm lint
```

Rust 后端验证：

```powershell
cargo fmt --manifest-path backend-rs/crates/backend/Cargo.toml --all --check
cargo clippy --manifest-path backend-rs/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```

## 当前限制

- 首个打包目标是 Windows。
- 桌面版使用 SQLite，本地数据不会自动从 legacy 的 Docker/Postgres 环境迁移。
- 外部 CLI Agent 默认具备 full-auto 执行能力，应只绑定到用户确认过的 workspace。


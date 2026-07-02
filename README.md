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
- Pi Agent 暂未实现，外部 runtime adapter 已预留扩展位置。
- 外部 CLI Agent 默认具备 full-auto 执行能力，应只绑定到用户确认过的 workspace。

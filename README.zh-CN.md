<p align="center">
  <img src="assets/qunica-logo.png" alt="Qunica 标志" width="132">
</p>

<h1 align="center">Qunica</h1>

<p align="center">
  <strong>让人和 AI Agent 在同一个房间里规划、分工，把事情交付出去。</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.1-c65d3b?style=flat-square" alt="0.1.1 版本">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-3f6f91?style=flat-square" alt="Windows | macOS | Linux">
  <img src="https://img.shields.io/badge/data-本地优先-4f7651?style=flat-square" alt="本地优先">
</p>

<p align="center">
  <a href="#qunica-是什么">概览</a> ·
  <a href="#首次使用">首次使用</a> ·
  <a href="#外部-runtime">Runtime</a> ·
  <a href="#运行项目">运行</a> ·
  <a href="#用-docker-运行">Docker</a> ·
  <a href="README.md">English</a>
</p>

---

## 为什么叫 Qunica？

**群**（qún，the group）加上 **quorum**：人到齐了，事情才议得起来。Qunica 的产品模型也是这样——一个群把人和 Agent 放进同一段对话，围绕同一组项目文件工作。

## Qunica 是什么

多数 Agent 工具给每个 Agent 单开一个聊天窗口。Qunica 给整个项目一个房间。

群组里放着成员、对话历史、共享笔记、工作区、文件和执行记录。每个 Agent 仍然保留自己的模型、提示词、工具、Skills 与工作区权限。Qunica 的调度器负责决定下一位发言者、限制本轮预算，并把过程记下来。

内置 Agent 支持 OpenAI 兼容接口、Anthropic 与 Gemini。Codex、Claude Code 等外部 CLI Agent 则通过 Agent Client Protocol（ACP）加入。

## 现在能做什么

- **把项目留在一个群里。** 人和 Agent 共用对话、笔记、文件与工作目录。
- **定好发言规则。** 可以选择 `mesh`、`star`、`hierarchical`、`ring` 路由，也可以跑一轮有界讨论，或让主持模型继续调度。
- **把真活交出去。** Agent 能读写文件、使用受守卫的 shell、调用 MCP、加载 Skills、搜索网页，也能把工作交给其他群成员。
- **看清每一步。** 流式输出、审批、错误、Token 用量与派发轨迹都留在原对话里。
- **直接处理仓库。** 在应用内浏览和编辑工作区文件，查看 Git 状态与差异，暂存、提交、同步，或打开集成终端。
- **机器仍归你管。** SQLite 数据、密钥与工作区都留在本机后端。破坏性 shell 操作需要审批，高危命令会直接拒绝。

## 首次使用

创建本地账户后，Qunica 会带你完成三项设置：

1. 选择群组工作区的根目录。
2. 填写模型服务的接口地址、模型与 API 密钥。
3. 指定内置助手默认使用的模型。

接着创建一个 Agent，绑定工作区和工具，再把它邀请进群。完整流程见 [快速上手](backend-rs/crates/backend/src/docs/guide/getting-started.md)。

## 外部 Runtime

Qunica 会检测并启动已经安装、已经登录的 Agent CLI，不保存这些 CLI 的账户凭据。

| Runtime | 命令 | 接入方式 |
| --- | --- | --- |
| OpenAI Codex | `codex` | ACP + 沙箱配置 |
| Claude Code | `claude` | 流式工具调用与权限处理 |
| Pi Agent | `pi` | ACP 适配器 |
| OpenCode | `opencode` | ACP 服务 |
| DeepSeek Harness | `dsh` | 仅提示词的 ACP 界面，沙箱不可用时闭合失败 |
| 自定义 ACP 服务 | 任意 | 兼容 ACP 的 stdio 命令 |

Full-auto CLI Agent 能修改其工作区与运行权限覆盖的所有内容。请只绑定你愿意让它改动的目录。细节见 [外部 CLI Agent](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md)。

## 运行项目

环境要求：Node.js 20+、pnpm 9、稳定版 Rust 工具链；桌面版支持 Windows、macOS 和 Linux。

```powershell
pnpm install
pnpm desktop:dev
```

构建安装包与 portable 可执行文件：

```powershell
pnpm desktop:build
```

产物位于 `frontend/src-tauri/target/release/bundle/`。若只在浏览器中连接本地后端，运行 `pnpm dev`。

## 用 Docker 运行

一个容器在同一个端口上同时提供 API 和网页界面。

```bash
docker compose up -d --build
```

打开 <http://127.0.0.1:18765>；容器会自动把持久化的 `/workspaces` 卷设为首次运行的工作区根目录。宿主机端口避开 8765，那是桌面版自带后端占用的端口。数据保存在 `/data` 与 `/workspaces` 两个卷里。Agent 会在容器内执行 shell 命令，所以不要把端口直接暴露到公网。公网 VPS 部署时，请按 [DOCKER.md](DOCKER.md#public-vps-first-boot) 关闭注册并创建初始账户。

## 文档

| 主题 | 指南 |
| --- | --- |
| 群组、路由、预算与共享笔记 | [群组](backend-rs/crates/backend/src/docs/guide/groups.md) |
| Agent、内置工具与委托 | [Agent](backend-rs/crates/backend/src/docs/guide/agents.md) |
| 工作区与文件 | [工作区](backend-rs/crates/backend/src/docs/guide/workspaces.md) · [工作区文件](backend-rs/crates/backend/src/docs/guide/workspace-files.md) |
| 提供商、MCP 与 Skills | [提供商](backend-rs/crates/backend/src/docs/guide/providers.md) · [MCP](backend-rs/crates/backend/src/docs/guide/mcp-servers.md) · [Skills](backend-rs/crates/backend/src/docs/guide/skills.md) |
| 内置助手与终端 | [助手](backend-rs/crates/backend/src/docs/guide/assistant.md) · [终端](backend-rs/crates/backend/src/docs/guide/terminal.md) |

## 开发

```powershell
pnpm type-check
pnpm lint
pnpm --filter @qunica/frontend test
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```
- [Linux Do](https://linux.do/) — A community for developers, by developers.

<p align="center"><sub>Qunica · 本地优先的多 Agent 协作</sub></p>

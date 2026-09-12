<p align="center">
  <img src="assets/qunica-logo.png" alt="Qunica 标志" width="132">
</p>

<h1 align="center">Qunica</h1>

<p align="center">
  <strong>让人和 AI Agent 在同一个房间里规划、分工，把事情交付出去。</strong>
</p>

<p align="center">
  <a href="https://github.com/sligter/qunica/releases/latest"><img src="https://img.shields.io/badge/version-0.1.2-c65d3b?style=flat-square" alt="0.1.2 版本"></a>
  <img src="https://img.shields.io/badge/desktop-Windows%20%7C%20macOS%20%7C%20Linux-3f6f91?style=flat-square" alt="Windows | macOS | Linux">
  <img src="https://img.shields.io/badge/mobile-Android-3f6f91?style=flat-square" alt="Android">
  <img src="https://img.shields.io/badge/data-本地优先-4f7651?style=flat-square" alt="本地优先">
  <img src="https://img.shields.io/badge/stack-Tauri%202%20%C2%B7%20Rust%20%C2%B7%20React%2019%20%C2%B7%20SQLite-6b6259?style=flat-square" alt="Tauri 2 · Rust · React 19 · SQLite">
</p>

<p align="center">
  <a href="https://qunica.cc">官网</a> ·
  <a href="#下载">下载</a> ·
  <a href="#qunica-是什么">概览</a> ·
  <a href="#四种群拓扑路由">拓扑</a> ·
  <a href="#有界调度与主持调度">调度</a> ·
  <a href="#runtime-与模型">Runtime</a> ·
  <a href="#从源码运行">运行</a> ·
  <a href="#用-docker-运行">Docker</a> ·
  <a href="README.md">English</a>
</p>

<p align="center">
  <img src="assets/qunica-app-preview.png" alt="Qunica 官网工作台演示：示例会话、Agent 与项目文件" width="920">
</p>
<p align="center"><sub>取自<a href="https://qunica.cc/#preview">官网</a>的工作台演示，使用示例内容。</sub></p>

---

## 为什么叫 Qunica？

**群**（qún，the group）加上 **quorum**：人到齐了，事情才议得起来。Qunica 的产品模型也是这样。一个群把人和 Agent 放进同一段对话，围绕同一组项目文件工作。

## Qunica 是什么

多数 Agent 工具给每个 Agent 单开一个聊天窗口，你自己变成它们之间的消息总线，在十几个标签页之间来回复制上下文。Qunica 换了一种做法：给整个项目一个房间。

群组里放着成员、对话历史、共享笔记、工作区、文件和执行记录。每个 Agent 仍然保留自己的模型、提示词、工具、Skills 与工作区权限。Qunica 的调度器负责决定下一位发言者、限制本轮预算，并把过程记下来。五个 Agent 的一轮讨论会按规则结束，而不是等到你没了耐心。

内置 Agent 可直连 OpenAI 兼容接口、OpenAI Responses、Anthropic 与 Gemini，也包括本地网关。Claude Code、Codex 等外部 CLI Agent 通过 Agent Client Protocol（ACP）进入同一个房间，沿用你本机已登录的账号。

<p align="center">
  <img src="assets/qunica-one-room.svg" alt="架构图：人、内置 Agent 与外部 CLI Agent 进入同一个群组房间，房间里有共享对话、共享笔记、工作区与 Git、执行记录，下方是带拓扑、模式与预算的调度器。Agent 可以触达文件与受守卫的 Shell、MCP 与 Skills、网页搜索以及其他成员。一切运行在本机，只有模型请求离开机器。" width="1000">
</p>

## 核心能力

- **同室协作，同一份上下文。** 人和 Agent 共用对话、Markdown 笔记、文件与工作目录，不用在窗口之间粘贴。
- **发言规则可推理。** 用 `mesh`、`star`、`hierarchical`、`ring` 决定谁先说、谁能接手。`call` 与 `handoff` 的结构化委派也在这些规则之内。
- **回合必然终止。** 有界调度按顺序消费候选发言者；主持调度让主持人逐步选人。两者都受步数、跳数、Token 与失败预算约束。
- **交付真活，不止聊天。** Agent 能读写文件、运行受守卫的 Shell、调用 MCP、加载 Skills、搜索网页，也能把工作交给其他成员。
- **接入你已有的 CLI Agent。** Claude Code、Codex、Pi、OpenCode、DeepSeek Harness，或任意 stdio 方式的 ACP 服务。Qunica 只检测并启动已安装的程序，不保存它们的凭证。
- **每一步都有记录。** 流式输出、审批、错误、Token 用量、工具调用树与派发轨迹都留在原对话里。
- **仓库就地处理。** 在应用内浏览和编辑工作区文件，查看 Git 状态与差异，暂存、提交、同步，或打开集成终端。
- **机器仍归你管。** SQLite 数据、API 密钥与工作区都在本机后端。破坏性 Shell 操作需要审批，高危命令直接拒绝。

## 四种群拓扑路由

<p align="center">
  <img src="assets/qunica-topologies.svg" alt="四张示意图：mesh 中每个 Agent 与其他所有 Agent 相连；star 由一个中心节点向四个 Agent 派发；hierarchical 由一个负责人带两个组长及其组员；ring 中四个 Agent 按固定顺序轮转" width="1000">
</p>

| 模式 | 谁来发言 | 适合场景 |
| --- | --- | --- |
| `mesh` | 无固定顺序，所有合法成员对等。无主持的回合里，Agent 的 `@提及` 会请求该成员接着回复。 | 头脑风暴、方案评审、互相交叉检查 |
| `star` | 中心 Agent 先发言，再派发给各专精成员。 | 需要一位协调者把控节奏 |
| `hierarchical` | 负责人先发言，然后是各自的团队。 | 拆解需求、分工执行、汇总验收 |
| `ring` | 按 `speaking_order` 固定轮转。 | 需求、开发、审查、测试这类流水线 |

## 有界调度与主持调度

两者跑在同一套持久化调度器上，区别只在「谁决定下一位发言者」。

- **`bounded`** 按拓扑顺序消费候选列表，确定性收敛。流程事先确定时用它。
- **`automatic`** 引入一位**主持人**（Moderator），可单独配置服务商与模型。每一步由主持人挑选下一位合法发言者，或直接结束回合。需求开放、分工需要临场判断时用它。

两者受同一组预算约束：Agent 总步数、单 Agent 步数、调度跳数、主持人调用次数、总 Token、连续与累计失败次数。任一耗尽即结束回合。详见 [群组](backend-rs/crates/backend/src/docs/guide/groups.md)。

## Runtime 与模型

Qunica 会检测并启动本机已安装且已登录的 Agent CLI，不保存它们的账号凭证。

| Runtime | 命令 | 集成方式 |
| --- | --- | --- |
| Claude Code | `claude` | 流式工具调用、权限处理、多文件编辑 |
| OpenAI Codex | `codex` | 带沙箱配置的 ACP |
| Pi Agent | `pi` | ACP 适配器 |
| OpenCode | `opencode` | ACP 服务 |
| DeepSeek Harness | `dsh` | 仅提示词的 ACP 界面，沙箱失败即拒绝 |
| 自定义 ACP 服务 | 任意 | 任何兼容的 stdio 命令 |
| 直连 API | — | OpenAI 兼容接口（`/v1/chat/completions`，含 Ollama 等本地网关）、OpenAI Responses、Anthropic、Gemini |

全自动的 CLI Agent 可以修改其工作区和运行时权限允许的一切内容。请使用你愿意让它改动的工作区。详见 [外部 CLI Agent](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md) 与 [模型服务](backend-rs/crates/backend/src/docs/guide/providers.md)。

## 本地优先，从设计开始

| | Qunica | 常见云端多 Agent SaaS |
| --- | --- | --- |
| 源码与数据 | 本机 SQLite 与本地磁盘 | 上传至第三方数据库 |
| API 密钥 | 仅保存在本地后端，只发往你配置的厂商 | 托管在服务商的服务器上 |
| Shell 执行 | 受守卫：高危命令硬性拦截，破坏性操作需你审批 | 云端容器黑盒执行 |
| 复用本地 CLI | ACP 直接接入你已登录的 `claude`、`codex` 等 CLI | 无法复用本机环境 |
| 离线与局域网 | 可配合本地模型网关，在局域网内使用 | 依赖服务商在线 |

Qunica 没有托管服务。除了你配置的模型请求，没有任何数据离开本机。

## 下载

在 [最新发布](https://github.com/sligter/qunica/releases/latest) 中选择对应平台的安装包，每个版本都附带 SHA256 校验文件。

| 平台 | 安装包 |
| --- | --- |
| Windows 10/11 | 安装版与便携版 `.exe`，x64 与 ARM64 |
| macOS 11+ | `.dmg` 与 `.app` 压缩包，Apple Silicon 与 Intel |
| Linux | AppImage、`.deb`、`.rpm`，x64 与 ARM64 |
| Android 8+ | 手机选 `android-arm64-release.apk`，模拟器选 `android-x86_64-release.apk` |
| 无界面服务器 | `qunica-server-*` 压缩包，支持 Windows、macOS、Linux |

Android 端通过局域网扫码与桌面配对，外出时可走自建 TCP 中继。消息、审批与终端数据均经 Noise 端到端加密。手机不运行后端，也不运行任何 Agent CLI。详见 [android/README.md](android/README.md)。

## 首次使用

创建本地账户后，Qunica 会带你完成三项设置：

1. 选择群组工作区的根目录。
2. 填写模型服务的接口地址、模型与 API 密钥，或让 Qunica 检测已安装的 CLI Agent。
3. 指定内置助手默认使用的模型。

接着创建一个 Agent，绑定工作区和工具，再把它邀请进群。完整流程见 [快速上手](backend-rs/crates/backend/src/docs/guide/getting-started.md)。

## 从源码运行

前置条件：Node.js 20+、pnpm 9 与稳定版 Rust 工具链。桌面构建支持 Windows、macOS 与 Linux。

```powershell
pnpm install
pnpm desktop:dev
```

构建安装包与便携版：

```powershell
pnpm desktop:build
```

产物位于 `frontend/src-tauri/target/release/bundle/`。如需用浏览器访问本地后端，运行 `pnpm dev`。

## 用 Docker 运行

一个容器同时提供 API 与 Web 界面，共用一个端口。

```bash
docker compose up -d --build
```

打开 <http://127.0.0.1:18765>。容器会自动把持久化的 `/workspaces` 卷作为首次运行的工作区根目录。宿主端口避开了桌面版后端使用的 8765。数据保存在 `/data` 与 `/workspaces` 两个卷中。Agent 会在容器内执行 Shell 命令，请不要把端口暴露到公网。若部署在公网 VPS，请按 [DOCKER.md](DOCKER.md#public-vps-first-boot) 关闭注册并初始化首个账户。

## 文档

| 主题 | 指南 |
| --- | --- |
| 群组、路由、预算与共享笔记 | [群组](backend-rs/crates/backend/src/docs/guide/groups.md) |
| Agent、内置工具与委派 | [Agent](backend-rs/crates/backend/src/docs/guide/agents.md) |
| 私聊 | [私聊](backend-rs/crates/backend/src/docs/guide/direct-chats.md) |
| 工作区与文件 | [工作区](backend-rs/crates/backend/src/docs/guide/workspaces.md) · [工作区文件](backend-rs/crates/backend/src/docs/guide/workspace-files.md) |
| 模型服务、MCP 与 Skills | [模型服务](backend-rs/crates/backend/src/docs/guide/providers.md) · [MCP](backend-rs/crates/backend/src/docs/guide/mcp-servers.md) · [Skills](backend-rs/crates/backend/src/docs/guide/skills.md) |
| 外部 CLI Agent | [外部 CLI Agent](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md) |
| 内置助手、终端与设置 | [助手](backend-rs/crates/backend/src/docs/guide/assistant.md) · [终端](backend-rs/crates/backend/src/docs/guide/terminal.md) · [设置](backend-rs/crates/backend/src/docs/guide/settings.md) |
| Docker 与 Android | [DOCKER.md](DOCKER.md) · [android/README.md](android/README.md) |

## 常见问题

**Qunica 会把我的代码上传到别处吗？**
不会。数据保存在本机 SQLite 数据库与本地磁盘。唯一的对外流量是发往你所配置模型服务商的请求。

**什么是 ACP？CLI Agent 怎么加入？**
Agent Client Protocol 是通过 stdio 驱动 Agent 的标准协议。Qunica 找到你已安装并登录的 `claude`、`codex`、`pi`、`dsh` 或 `opencode`，启动它并通过该通道通信。你的账号凭证不会被复制进 Qunica。

**有界调度和主持调度该选哪个？**
流程固定、想要可预测顺序时选有界。需求开放、需要一位主持人临场决定下一位发言者时选主持。两者受同一组预算约束，都不会无限运行。

**受守卫的 Shell 怎么保护我的电脑？**
内置过滤器会硬性拦截格式化磁盘、删除系统目录等毁灭性命令。可能修改文件的命令会在对话中弹出审批卡片，你确认后才执行。

## 开发

```powershell
pnpm type-check
pnpm lint
pnpm --filter @qunica/frontend test
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```

## 社区

- [官网](https://qunica.cc) · [版本发布](https://github.com/sligter/qunica/releases) · [问题反馈](https://github.com/sligter/qunica/issues)
- [Linux Do](https://linux.do/) — 由开发者组成、面向开发者的社区。

<p align="center"><sub>Qunica · 本地优先的多 Agent 协作</sub></p>

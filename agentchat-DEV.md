# AgentChat V1.0 开发文档与技术栈选型

## 0. 文档说明

本文档配套 [agentchat-PRD.md](agentchat-PRD.md)，确定 AgentChat（ag-swarmer）从零到 MVP 的技术栈、系统架构、模块划分与开发节奏。

**用户已确认的核心决策**：
- 后端：Python + TypeScript
- 部署：Web 优先（V1 不打包桌面端）
- LLM：多供应商对等支持

**关键产品约束**（来自 PRD）：
- 群是上下文容器，Agent 是可调度成员（PRD §5.1, §5.2）
- 多 Agent 在同一上下文协同工作，状态可见、可中断、可恢复（PRD §5.5, §9.6, §14）
- 长任务需要 checkpoint 和上下文恢复（PRD §9.5.4, §9.6.4）
- 写操作（文件、记忆）需要用户审批（PRD §9.12.3）
- 实时消息流式输出（PRD §17.1, §17.2）
- 项目级长期记忆 + 向量检索（PRD §9.7.4, §10.11）

---

## 1. 技术栈总览

### 1.1 后端（Python）

| 层 | 选型 | 版本 | 理由 |
|---|---|---|---|
| Web 框架 | **FastAPI** | 0.115+ | 原生 async、Pydantic v2、自动生成 OpenAPI、SSE/WS 支持完善 |
| ASGI 服务器 | **Uvicorn** + Gunicorn workers | 0.32+ | 标准组合，生产环境稳定 |
| Agent 编排 | **LangGraph** | 0.2+ | StateGraph 完美匹配「群 + 多 Agent + checkpoint + 中断」模型；PostgreSQL checkpointer 开箱即用 |
| LLM 抽象 | **LangChain Core** | 0.3+ | 统一 ChatModel 接口，原生支持 Anthropic/OpenAI/Google/DeepSeek/Qwen 等，匹配多供应商策略 |
| ORM | **SQLAlchemy 2.0** (async) + **Alembic** | 2.0+ | 与 LangGraph checkpointer 共存良好；async 全栈 |
| 数据校验 | **Pydantic v2** | 2.9+ | FastAPI 原生集成；用作 API schema 与领域模型 |
| 任务队列 | **Celery** + Redis broker | 5.4+ | 长任务（Agent 执行）异步化；支持取消、重试、超时 |
| 实时通信 | **sse-starlette** + FastAPI WebSocket | - | SSE 用于 Agent 流式输出；WS 用于状态/审批/在线推送 |
| 缓存 / Pub-Sub | **Redis 7** | 7.x | 跨实例广播 Agent 状态、消息事件 |
| 向量库 | **pgvector** | 0.7+ | 与 PostgreSQL 同库，匹配 PRD §10.11 的 `VECTOR` 列 |
| 对象存储 | **MinIO** (S3 兼容) | - | 群文件存储；生产可平替为 AWS S3/阿里云 OSS |
| 测试 | **pytest** + **pytest-asyncio** + **httpx** | - | 标准组合 |
| 类型检查 | **mypy** + **ruff** | - | 严格模式；CI 强制 |
| 包管理 | **uv** | 0.5+ | 速度快、lockfile 稳定 |

### 1.2 前端（TypeScript）

| 层 | 选型 | 版本 | 理由 |
|---|---|---|---|
| 框架 | **React 19** | 19.x | 生态最完整；Server Component 暂不启用以保持简单 |
| 构建工具 | **Vite 6** | 6.x | 启动快、HMR 稳定 |
| 路由 | **React Router 7**（library mode） | 7.x | 不引入 SSR 复杂度 |
| UI 组件 | **shadcn/ui** + **Radix UI** | latest | 可定制（PRD 要求微信 PC 风格的紧凑布局，Ant Design 偏后台风过重） |
| CSS | **Tailwind CSS 4** | 4.x | 与 shadcn/ui 标配 |
| 客户端状态 | **Zustand** | 5.x | 轻量；按 group/agent/ui 切片 |
| 服务器状态 | **TanStack Query 5** | 5.x | 缓存、乐观更新、SSE/WS 集成简单 |
| 表单 | **react-hook-form** + **zod** | - | Agent 配置表单复杂，需要严格校验 |
| 流式渲染 | **@microsoft/fetch-event-source** | - | 比原生 EventSource 稳定，支持 POST + 自定义 header |
| Markdown | **react-markdown** + **remark-gfm** + **shiki** | - | 群笔记、Agent 回复、代码高亮 |
| 虚拟滚动 | **@tanstack/react-virtual** | - | 长消息流必备 |
| 图标 | **lucide-react** | - | 与 shadcn/ui 一致 |
| 测试 | **Vitest** + **@testing-library/react** + **Playwright** | - | 单测 + E2E |
| 包管理 | **pnpm** | 9.x | monorepo 友好 |

### 1.3 跨端契约

| 用途 | 选型 |
|---|---|
| API 契约 | FastAPI 自动生成 OpenAPI 3.1 → `openapi-typescript` 生成 TS 类型 |
| WS/SSE 事件 | 在 `shared/events.ts` 手写联合类型；后端用同名 Pydantic Model |
| 包结构 | **pnpm workspace**：`backend/` 独立，`frontend/` + `shared/` 在 pnpm workspace 中 |

### 1.4 基础设施

| 用途 | 选型 |
|---|---|
| 主数据库 | **PostgreSQL 16** + pgvector |
| 缓存 / 队列 | **Redis 7** |
| 对象存储 | **MinIO**（dev）→ S3 兼容（prod） |
| 容器化 | **Docker** + **Docker Compose**（dev） |
| 反向代理 | **Caddy 2** 或 nginx |
| 进程管理 | **systemd** 或 docker-compose（生产） |
| 日志 | **structlog** (Python) + **Loki** + Grafana（可选） |
| 追踪 | **OpenTelemetry** + Jaeger（可选） |
| CI/CD | GitHub Actions：lint → test → build → deploy |

---

## 2. 系统架构

### 2.1 部署拓扑（V1.0 单实例）

```text
┌──────────────────────────────────────────────────────────────┐
│                         浏览器                                │
│  React SPA (Vite build) + TanStack Query + Zustand            │
└──────────────┬───────────────────────────────────────────────┘
               │ HTTPS / SSE / WSS
               ▼
┌──────────────────────────────────────────────────────────────┐
│                   Caddy / nginx 反向代理                      │
└──────────┬─────────────────────────┬─────────────────────────┘
           │ /api /sse /ws            │ 静态资源
           ▼                          ▼
┌────────────────────────────┐   ┌──────────────────────────┐
│   FastAPI (Uvicorn workers)│   │ 静态文件托管             │
│  ┌──────────────────────┐  │   └──────────────────────────┘
│  │  REST API Layer      │  │
│  │  SSE / WebSocket     │  │
│  ├──────────────────────┤  │
│  │  Service Layer       │  │
│  │  (Group/Agent/Msg)   │  │
│  ├──────────────────────┤  │
│  │  Agent Runtime       │  │   ┌──────────────────────────┐
│  │  (LangGraph)         │◄─┼──►│  Celery Workers          │
│  └──────────┬───────────┘  │   │  长任务 / 工具调用       │
└─────────────┼──────────────┘   └──────────┬───────────────┘
              │                              │
   ┌──────────┼──────────────────────────────┼────────────┐
   ▼          ▼                              ▼            ▼
┌────────┐ ┌──────────┐ ┌──────────────┐ ┌──────────────────┐
│PostgreSQL│ │ Redis    │ │ MinIO (S3)   │ │ LLM Providers    │
│+pgvector │ │ Pub/Sub  │ │ 群文件       │ │ Anthropic/OpenAI │
│          │ │ Queue    │ │              │ │ Google/DeepSeek..│
└──────────┘ └──────────┘ └──────────────┘ └──────────────────┘
```

### 2.2 后端模块划分

```text
backend/app/
├── main.py                      # FastAPI 入口、生命周期、中间件
├── core/
│   ├── config.py                # Pydantic Settings
│   ├── security.py              # JWT、密码哈希
│   ├── deps.py                  # FastAPI 依赖注入
│   ├── exceptions.py            # 业务异常
│   └── logging.py               # structlog 配置
│
├── api/                         # HTTP 路由层（薄）
│   ├── v1/
│   │   ├── auth.py
│   │   ├── groups.py            # 群 CRUD
│   │   ├── members.py           # 群成员管理
│   │   ├── agents.py            # Agent 资产库
│   │   ├── group_agents.py      # Agent 入群配置
│   │   ├── messages.py          # 消息发送/查询
│   │   ├── threads.py           # 工作线程
│   │   ├── notes.py             # 群笔记
│   │   ├── files.py             # 群文件
│   │   ├── approvals.py         # 审批
│   │   └── memory.py            # 记忆查询
│   ├── sse.py                   # SSE 流式输出
│   └── ws.py                    # WebSocket：状态/通知
│
├── models/                      # SQLAlchemy ORM 模型
│   ├── user.py
│   ├── group.py
│   ├── agent.py
│   ├── group_agent.py
│   ├── message.py
│   ├── thread.py
│   ├── checkpoint.py            # 与 LangGraph checkpointer 解耦：业务级 checkpoint
│   ├── note.py
│   ├── file.py
│   ├── approval.py
│   └── memory.py
│
├── schemas/                     # Pydantic（API in/out）
│   └── ...
│
├── services/                    # 业务逻辑（无 HTTP/ORM 细节泄漏）
│   ├── group_service.py
│   ├── agent_service.py
│   ├── message_service.py
│   ├── thread_service.py
│   ├── note_service.py
│   ├── file_service.py
│   ├── approval_service.py
│   └── memory_service.py
│
├── agents/                      # 多 Agent 编排核心（LangGraph）
│   ├── runtime.py               # GroupRuntime：单群的 StateGraph 容器
│   ├── state.py                 # GroupState（共享状态结构）
│   ├── nodes/
│   │   ├── router.py            # @ 解析、路由消息到目标 Agent
│   │   ├── agent_node.py        # 单 Agent 推理节点
│   │   ├── tool_node.py         # 工具调用节点
│   │   ├── approval_node.py     # 审批等待节点（human-in-the-loop）
│   │   └── checkpoint_node.py   # 业务 checkpoint 写入
│   ├── interrupt.py             # 中断检测与恢复（PRD §9.6）
│   ├── context_builder.py       # 上下文优先级与压缩（PRD §15）
│   └── scheduler.py             # 群级 Agent 调度（响应模式、频率限制）
│
├── llm/                         # LLM 抽象层
│   ├── registry.py              # 注册各 Provider
│   ├── factory.py               # 按 Agent 配置创建 ChatModel
│   ├── providers/
│   │   ├── anthropic.py
│   │   ├── openai.py
│   │   ├── google.py
│   │   ├── deepseek.py
│   │   └── qwen.py
│   └── tracing.py               # token、耗时记录
│
├── tools/                       # 工具系统
│   ├── registry.py              # 工具注册表（Provider 无关）
│   ├── builtins/
│   │   ├── file_read.py
│   │   ├── file_write.py        # 触发审批
│   │   ├── note_read.py
│   │   ├── note_write.py
│   │   ├── search.py
│   │   └── group_query.py       # 查询群消息/任务
│   ├── mcp_adapter.py           # MCP 协议 → LangChain Tool（适配多 Provider）
│   └── permission.py            # 权限校验装饰器
│
├── skills/                      # Skills 系统（自研，多 Provider 通用）
│   ├── loader.py                # SKILL.md → SkillSpec
│   ├── registry.py
│   └── injector.py              # 注入 system prompt
│
├── memory/                      # 记忆子系统（PRD §10.11）
│   ├── embeddings.py            # 向量化（多 Provider 兼容）
│   ├── retriever.py             # 语义 + 关键词混合检索
│   ├── writer.py                # 记忆写入策略（含审批）
│   └── decay.py                 # 过期/降权
│
├── workspace/                   # 群级工作空间（PRD MVP §13：每个群即 workspace）
│   ├── manager.py               # 创建/销毁 workspace 目录
│   ├── filesystem.py            # 受控文件操作
│   └── diff.py                  # diff 生成（用于审批 UI）
│
├── events/                      # 事件总线
│   ├── bus.py                   # Redis Pub/Sub 封装
│   ├── types.py                 # 事件类型联合
│   └── publisher.py             # 业务侧发布点
│
└── tasks/                       # Celery 异步任务
    ├── agent_run.py             # Agent 长任务执行入口
    ├── file_indexing.py         # 文件向量化
    └── memory_consolidation.py  # 定时摘要

backend/alembic/                 # 迁移
backend/tests/                   # pytest
```

### 2.3 前端模块划分

```text
frontend/src/
├── main.tsx                     # 入口
├── App.tsx                      # 路由根
├── routes/                      # 路由配置（React Router 7）
│
├── pages/
│   ├── auth/                    # 登录/注册
│   ├── home/                    # 默认主页（最近会话）
│   ├── group/                   # 群聊主界面（核心）
│   │   └── [groupId].tsx
│   ├── agents/                  # Agent 资产库
│   │   ├── list.tsx
│   │   └── editor.tsx
│   └── settings/
│
├── components/
│   ├── layout/
│   │   ├── ThreeColumnLayout.tsx     # 左中右三栏（PRD §11.1）
│   │   ├── Sidebar.tsx               # 群列表（左）
│   │   └── RightPanel.tsx            # 右侧 Tab 容器
│   ├── chat/
│   │   ├── MessageList.tsx           # 虚拟滚动消息流
│   │   ├── MessageItem/              # 按 message_type 多态渲染
│   │   │   ├── UserMessage.tsx
│   │   │   ├── AgentMessage.tsx      # 流式渲染、引用折叠
│   │   │   ├── ToolCallMessage.tsx   # 工具调用可视化
│   │   │   ├── ApprovalCard.tsx      # 审批卡片
│   │   │   ├── CheckpointCard.tsx    # checkpoint 卡片
│   │   │   ├── FileChangeCard.tsx
│   │   │   └── SystemMessage.tsx
│   │   ├── ComposerInput.tsx         # 输入框：@、文件、引用
│   │   └── MentionPicker.tsx         # @Agent 选择器
│   ├── agent/
│   │   ├── AgentBoard.tsx            # 右侧看板（PRD §9.10）
│   │   ├── AgentCard.tsx             # 单 Agent 状态卡
│   │   ├── AgentEditor.tsx           # 配置表单（react-hook-form + zod）
│   │   └── AgentInviter.tsx
│   ├── group/
│   │   ├── GroupCreator.tsx
│   │   ├── GroupSettings.tsx
│   │   └── AnnouncementEditor.tsx
│   ├── note/
│   │   ├── NoteList.tsx
│   │   └── NoteEditor.tsx            # 与右侧面板共用
│   ├── file/
│   │   ├── FileList.tsx
│   │   └── FileDiffViewer.tsx        # 审批用 diff
│   └── approval/
│       └── ApprovalCenter.tsx
│
├── stores/                      # Zustand
│   ├── authStore.ts
│   ├── groupStore.ts            # 当前群、群列表
│   ├── messageStore.ts          # 流式增量合并
│   ├── agentStore.ts            # Agent 状态实时更新
│   └── uiStore.ts               # 右侧 Tab、抽屉
│
├── hooks/
│   ├── useSSE.ts                # 群消息 SSE 订阅
│   ├── useWebSocket.ts          # 状态/通知 WS
│   ├── useStreamingMessage.ts   # Agent 流式 token 合并
│   └── useApproval.ts
│
├── lib/
│   ├── api/                     # 由 openapi-typescript 生成
│   ├── client.ts                # axios/fetch 封装
│   ├── ws.ts                    # WebSocket 客户端
│   └── markdown.tsx
│
└── types/                       # 共享类型（由 shared/ 同步）
```

---

## 3. 关键技术决策与对应模块

### 3.1 LangGraph 作为多 Agent 编排核心

**为什么**：PRD 中「群 + 多 Agent + checkpoint + 中断 + 审批等待」与 LangGraph 的 StateGraph + Checkpointer + Interrupts + Human-in-the-loop 几乎一一对应。自研同等能力 ≥ 4 周。

**映射关系**：

| PRD 概念 | LangGraph 概念 |
|---|---|
| 群（Group） | 一个 StateGraph 实例（GroupRuntime） |
| GroupState | StateGraph 的 State 类型（消息列表、活跃 thread、待审批） |
| Agent 节点 | StateGraph node（包装单 Agent 推理） |
| Thread | LangGraph thread_id（与业务 thread 1:1） |
| Checkpoint | LangGraph checkpointer（PostgresSaver） |
| 中断 | LangGraph interrupt_before/after + 业务中断检测 |
| 审批等待 | interrupt + waiting_approval state |

**关键文件**：[backend/app/agents/runtime.py](backend/app/agents/runtime.py)、[backend/app/agents/state.py](backend/app/agents/state.py)、[backend/app/agents/interrupt.py](backend/app/agents/interrupt.py)

### 3.2 多 LLM Provider 抽象

**策略**：基于 LangChain Core 的 `BaseChatModel` 接口，所有 Agent 配置中的 `model_config` 字段经 `llm/factory.py` 实例化为对应 Provider 的 ChatModel。

**支持清单**（V1.0）：
- Anthropic（claude-opus-4-7、claude-sonnet-4-6、claude-haiku-4-5）
- OpenAI（gpt-4o、gpt-4-turbo）
- Google（gemini-2.0-flash、gemini-2.0-pro）
- DeepSeek（deepseek-chat、deepseek-reasoner）
- 阿里通义（qwen-max、qwen-plus）

**统一抽象**：工具调用、流式输出、token 用量统计在 LangChain 层已抹平差异。

**关键文件**：[backend/app/llm/factory.py](backend/app/llm/factory.py)、[backend/app/llm/providers/](backend/app/llm/providers/)

### 3.3 MCP 与 Skills 的 Provider 中立化

PRD §19 MVP 要求支持 skills 和 mcp，但这些是 Anthropic 概念。多 Provider 策略下的处理：

- **MCP**：把 MCP Server 当成工具来源，通过 `tools/mcp_adapter.py` 把 MCP tool 转成 LangChain `BaseTool`。任何 Provider 都能调用（只要支持 function calling）。
- **Skills**：自研 SKILL.md 格式（参考 Claude Code 的 SKILL.md），由 `skills/injector.py` 把 Skill 描述拼到 system prompt，把 Skill 自带的工具加到该 Agent 的工具集。Provider 中立。

### 3.4 群即 Workspace（PRD MVP §13）

每个群对应一个独立目录（`workspaces/{group_id}/`）：
- 群文件实际落地路径
- Agent 文件读写沙箱（`workspace/filesystem.py` 强制路径校验）
- 后续阶段三的 git worktree 直接在此目录下创建

**关键文件**：[backend/app/workspace/manager.py](backend/app/workspace/manager.py)、[backend/app/workspace/filesystem.py](backend/app/workspace/filesystem.py)

### 3.5 实时通信策略

| 用途 | 协议 | 通道 |
|---|---|---|
| Agent 流式回复 | SSE | `GET /api/v1/messages/stream?thread_id=...` |
| 群级事件（新消息、Agent 状态、审批） | WebSocket | `WSS /ws/group/{group_id}` |
| 跨实例广播 | Redis Pub/Sub | 内部 |

WebSocket 仅推送事件 ID 与轻量元数据，详细数据由前端按需 GET（避免 WS 通道过载）。

### 3.6 审批与中断的统一抽象

审批 = 一种特殊的中断。两者在 LangGraph 层都是 `interrupt`，区别是：
- 中断：由用户消息触发，恢复时可能修改任务目标
- 审批：由 Agent 主动请求，恢复时只有 approve/reject 两个分支

统一在 `agents/interrupt.py` 处理，避免两套并行机制。

---

## 4. 数据库设计要点

### 4.1 与 PRD §10 的关系

PRD §10 的 SQL 已基本完整。本项目直接采用，仅做以下补充：

1. **users 表**：PRD 未列出但必需。`id, email, password_hash, name, avatar_url, created_at, updated_at`。
2. **agent_runs 表**：每次 Agent 推理的细粒度记录（用于看板、token 统计、审计）。
3. **tool_calls 表**：工具调用单独表，支持 PRD §9.10.2 看板的「最近工具调用」展示。
4. **memory.embedding**：列类型 `vector(1536)`（OpenAI ada-002 兼容；其他维度用动态 schema 切分多张表或用 `halfvec`）。
5. **LangGraph checkpointer 表**：由 `langgraph-checkpoint-postgres` 自动建，与业务表共存于同一 DB。

### 4.2 关键索引

- `messages(group_id, created_at DESC)` — 消息流分页
- `messages(thread_id, created_at)` — thread 内查询
- `memories USING ivfflat (embedding vector_cosine_ops)` — 向量检索
- `notes(group_id, pinned DESC, updated_at DESC)` — 置顶笔记优先

### 4.3 迁移策略

- Alembic 自动生成 + 人工 review
- 每个 PR 一个 migration
- 生产 zero-downtime：仅添加列、不删列、不重命名（V1.0 阶段）

---

## 5. 开发阶段（与 PRD §20 Roadmap 对齐）

### 5.1 Phase 0：脚手架（Week 1）

- [ ] monorepo 结构（pnpm workspace + backend python 子项目）
- [ ] docker-compose.dev.yml（pg、redis、minio）
- [ ] FastAPI Hello + Uvicorn
- [ ] React + Vite + Tailwind + shadcn 初始化
- [ ] OpenAPI 自动生成 → TS 类型同步脚本
- [ ] CI：lint + test
- [ ] Alembic 初始迁移：users、groups、agents、group_members、group_agents

### 5.2 Phase 1：MVP 核心闭环（Week 2-6）

对齐 PRD §19 MVP：

| Week | 功能 | 关键模块 |
|---|---|---|
| 2 | 用户/群/Agent CRUD、JWT 认证 | api/, services/, models/ |
| 3 | 群消息存取、SSE 流式输出、@ 解析 | api/messages.py, agents/router.py |
| 3-4 | 单 Agent 推理（LangGraph 单节点 StateGraph） | agents/runtime.py, llm/factory.py |
| 4 | Agent 看板（WebSocket 推状态） | api/ws.py, frontend/agent/AgentBoard |
| 4-5 | 群笔记 CRUD + 笔记引用入上下文 | services/note_service.py, agents/context_builder.py |
| 5 | 文件上传（MinIO）+ 文件读取工具 | services/file_service.py, tools/builtins/file_read.py |
| 5-6 | 文件写入审批（diff 生成 + UI 卡片 + interrupt） | workspace/diff.py, agents/nodes/approval_node.py |
| 6 | 基础 checkpoint（LangGraph PostgresSaver） + 中断恢复 | agents/interrupt.py |
| 6 | 群级 workspace 隔离 | workspace/manager.py |

### 5.3 Phase 2-4：按 PRD §20 Roadmap 推进

不在本文档详细展开，但以下是设计上需要前瞻的接口：

- `agents/runtime.py` 必须支持多节点协作（Phase 2 多 Agent 协作）
- `workspace/manager.py` 必须预留 git worktree 钩子（Phase 3）
- `core/config.py` 必须支持组织维度（Phase 4 企业化）

---

## 6. 关键文件路径速查

后续编码主要修改这些文件，按重要度排序：

| 优先级 | 路径 | 用途 |
|---|---|---|
| P0 | [backend/app/agents/runtime.py](backend/app/agents/runtime.py) | LangGraph StateGraph 装配 |
| P0 | [backend/app/agents/state.py](backend/app/agents/state.py) | GroupState 定义 |
| P0 | [backend/app/agents/interrupt.py](backend/app/agents/interrupt.py) | 中断/恢复/审批等待 |
| P0 | [backend/app/llm/factory.py](backend/app/llm/factory.py) | 多 Provider 工厂 |
| P0 | [backend/app/api/sse.py](backend/app/api/sse.py) | 流式输出端点 |
| P0 | [backend/app/api/ws.py](backend/app/api/ws.py) | 实时事件 |
| P1 | [backend/app/workspace/filesystem.py](backend/app/workspace/filesystem.py) | 受控文件 IO |
| P1 | [backend/app/workspace/diff.py](backend/app/workspace/diff.py) | 审批 diff |
| P1 | [backend/app/agents/context_builder.py](backend/app/agents/context_builder.py) | 上下文优先级与压缩 |
| P1 | [backend/app/tools/registry.py](backend/app/tools/registry.py) | 工具注册 |
| P1 | [backend/app/tools/mcp_adapter.py](backend/app/tools/mcp_adapter.py) | MCP→LangChain |
| P1 | [backend/app/skills/injector.py](backend/app/skills/injector.py) | Skills 注入 |
| P0 | [frontend/src/components/chat/MessageList.tsx](frontend/src/components/chat/MessageList.tsx) | 消息流虚拟滚动 |
| P0 | [frontend/src/components/chat/MessageItem/AgentMessage.tsx](frontend/src/components/chat/MessageItem/AgentMessage.tsx) | 流式渲染 |
| P0 | [frontend/src/components/agent/AgentBoard.tsx](frontend/src/components/agent/AgentBoard.tsx) | 右侧看板 |
| P0 | [frontend/src/hooks/useSSE.ts](frontend/src/hooks/useSSE.ts) | SSE 订阅 |
| P0 | [frontend/src/hooks/useWebSocket.ts](frontend/src/hooks/useWebSocket.ts) | WS 订阅 |

---

## 7. 验证与测试

### 7.1 端到端验证（MVP 完成时）

按 PRD §22.1 成功标准设计 E2E 用例：

1. **创建群闭环**（Playwright）：注册 → 登录 → 创建群 → 选模板 Agent → 进入群聊 → 看到群公告
2. **@Agent 流式回复**：在群中 @ Agent → SSE token 逐个到达 → 完成后右侧看板状态变 `idle`
3. **多 Agent 协作**：同时 @ 两个 Agent → 两个 Agent 顺序响应 → 看板同时显示两者状态
4. **文件审批**：让 Agent 修改文件 → diff 卡片出现 → approve → 文件实际改动
5. **中断恢复**：发起长任务（mock 慢响应）→ 中途插入新消息 → checkpoint 写入 → 点击「继续」→ 任务从断点恢复
6. **多 Provider**：分别配置 Claude/GPT/DeepSeek 三个 Agent → 同一群中混合工作

### 7.2 后端单测重点

- `agents/interrupt.py`：中断判定矩阵（PRD §9.6.2 六维度逐一覆盖）
- `agents/context_builder.py`：上下文优先级排序
- `workspace/filesystem.py`：路径越权防御（`..`、绝对路径、symlink）
- `tools/permission.py`：权限装饰器
- `llm/factory.py`：每个 Provider mock 一个最小推理用例

### 7.3 前端单测重点

- `useStreamingMessage.ts`：增量 token 合并
- `MessageItem` 多态渲染：每种 `message_type` 一个 snapshot
- `AgentBoard`：状态机驱动的图标/颜色变化

### 7.4 本地一键启动

```bash
# 后端
cd backend && uv sync && uv run alembic upgrade head && uv run uvicorn app.main:app --reload

# 前端
cd frontend && pnpm install && pnpm dev

# 基础设施
docker compose -f docker-compose.dev.yml up -d
```

应在 < 5 分钟内可访问 [http://localhost:5173](http://localhost:5173) 看到登录页。

---

## 8. 风险与缓解

| 风险 | 缓解 |
|---|---|
| LangGraph API 变动较快 | 锁定 minor 版本；封装成 `agents/runtime.py` 内部细节，业务层不直接 import |
| Python GIL 影响并发 | Agent 推理为 IO bound（等 LLM），FastAPI async 已足够；CPU 密集任务（向量化、diff）走 Celery |
| 多 Provider 工具调用差异 | 强制走 LangChain 工具接口；为不支持 function calling 的模型走 ReAct 提示词回退 |
| pgvector 索引随数据增长退化 | V1 用 ivfflat；记忆量超 100w 后切 hnsw |
| SSE 在某些代理后断流 | Caddy 配置 `flush_interval -1`；前端用 fetch-event-source 自动重连 |
| MCP server 进程管理 | MCP server 作为外部进程独立部署；adapter 通过 stdio/sse 与之通信，崩溃不影响主进程 |

---

## 9. 不在 V1.0 范围

明确剔除（避免 scope creep）：

- 桌面端打包（Tauri/Electron）
- Git worktree 与代码任务执行（Phase 3）
- 企业 SSO、组织空间（Phase 4）
- 移动端
- 完整插件市场
- 跨群 Agent 私下通信（PRD §9.11.2 已禁止）

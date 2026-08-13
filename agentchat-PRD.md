# 产品需求文档 PRD  

> **历史文档（已废弃）**：本文描述早期产品设想，不代表当前技术实现。当前 Rust/SQLite 单引擎群聊行为见 [群组指南](backend-rs/crates/backend/src/docs/guide/groups.md) 与 [调度器设计](GROUP_SCHEDULER.md)。

## 产品名称：AgentChat 群协作工作台

版本：V1.0  
文档状态：初稿  
目标平台：PC Web / 桌面端优先，后续支持移动端轻量查看  
目标用户：AI Agent 使用者、开发者、团队负责人、企业协作团队  
核心定位：一个以“群”为主容器的多 Agent 协作产品，让用户可以像在微信群里聊天一样，创建、邀请、管理多个 Agent，并让 Agent 在同一个上下文中协同完成任务。

---

# 1. 背景与问题

## 1.1 背景

当前 AI Agent 产品大多以「单 Agent + 单会话」为核心交互模型。用户通常在一个对话框中与一个 Agent 交互，Agent 拥有自己的上下文、工具能力和执行环境。

但在真实工作流中，很多任务并不是单一 Agent 可以高质量完成的。例如：

- 产品经理需要一个 PRD Agent、竞品分析 Agent、用户研究 Agent 协同工作。
- 开发者需要架构 Agent、前端 Agent、后端 Agent、测试 Agent 并行处理任务。
- 企业内部希望构建一个类似“项目群”的 AI 工作空间，让多个 Agent 共同参与项目。
- 用户希望在一个群中保留长期上下文、群内笔记、任务进度、执行记录和 Agent 之间的协作痕迹。

因此，需要一种新的 Agent 协作形态：

> 群是主容器，Agent 是群成员，用户在群中组织任务、分配角色、监督执行、沉淀知识。

---

# 2. 产品目标

## 2.1 核心目标

设计一款「群式多 Agent 协作工作台」，支持用户创建群空间，邀请多个 Agent 进入群，围绕同一项目上下文协作。

## 2.2 业务目标

1. 降低多 Agent 协作的使用门槛。
2. 提供类 IM 的自然交互体验，让用户快速理解和上手。
3. 支持 Agent 长任务执行、消息中断、上下文恢复和任务看板。
4. 形成项目级长期记忆，包括全局记忆、群内笔记、任务记录、文件资产。
5. 支持 Agent 之间的协同、拉人、分工、审批和权限控制。
6. 为后续插件市场、Agent 市场、企业知识库接入打基础。

---

# 3. 用户画像

## 3.1 个人高级用户

### 典型用户

- 独立开发者
- 自媒体创作者
- 产品经理
- 研究员
- 咨询顾问

### 需求

- 用多个 Agent 分别承担不同角色。
- 希望保留长期上下文。
- 希望 Agent 可以持续处理复杂任务。
- 希望能够看到每个 Agent 的执行过程和结果。
- 希望像微信群一样自然地组织 AI 协作。

---

## 3.2 团队用户

### 典型用户

- 创业团队
- 产品研发团队
- 设计团队
- 增长团队
- 咨询团队

### 需求

- 团队成员和 Agent 共处一个项目群。
- 群主可以管理 Agent 权限。
- Agent 修改文件、代码、文档时需要用户审批。
- 群内需要公告、文件、任务、笔记、上下文沉淀。
- 多个 Agent 需要独立执行任务，互不干扰。

---

## 3.3 企业用户

### 典型用户

- 企业 AI 中台负责人
- IT 管理员
- 业务部门负责人
- 内部知识库负责人

### 需求

- Agent 需要接入企业知识库和系统工具。
- 所有操作需要审计。
- 敏感权限需要审批。
- 数据需要分级隔离。
- 需要私有化部署或企业级权限体系。

---

# 4. 产品定位

## 4.1 一句话定位

AgentChat 是一款以群聊为核心交互的多 Agent 协作工作台，帮助用户像管理团队一样管理 AI Agent，共同完成复杂任务。

## 4.2 产品形态

整体类似微信 PC / 飞书群聊 / Slack，但群成员中不仅有人，也有 Agent。

核心结构：

```text
用户
 └── 群空间
      ├── 群成员：人类用户
      ├── 群成员：Agent
      ├── 群消息
      ├── 群笔记
      ├── 群文件
      ├── 群任务
      ├── Agent 运行状态
      ├── Agent 独立上下文
      └── 群级长期记忆
```

---

# 5. 核心设计原则

## 5.1 群是主容器

所有协作围绕“群”发生。

一个群代表一个项目、一个主题、一个长期任务或一个临时工作空间。

群内包含：

- 人类成员
- Agent 成员
- 消息流
- 文件
- 任务
- 公告
- 笔记
- 上下文
- 权限
- 运行记录

---

## 5.2 Agent 是全局资产

Agent 不隶属于某一个群，而是用户或组织拥有的全局资产。

用户可以将同一个 Agent 邀请进多个群。

例如：

- “产品经理 Agent”可以进入多个产品项目群。
- “代码审查 Agent”可以进入多个研发群。
- “法务审核 Agent”可以进入合同群和商务群。

---

## 5.3 Agent 进入群后拥有群内身份

Agent 作为群成员存在，拥有：

- 群昵称
- 群角色
- 群权限
- 群内记忆
- 群内任务
- 群内上下文
- 群内可访问文件
- 群内执行记录

---

## 5.4 私聊是轻量模式

私聊不作为复杂协作的主场景。

私聊主要用于：

- 单 Agent 快速问答
- 临时测试 Agent 能力
- 私人草稿
- 非群内任务

默认入口仍然是群。

---

## 5.5 上下文要可见、可控、可恢复

Agent 的上下文不能完全黑盒。

用户应该能够看到：

- Agent 当前正在参考哪些消息。
- Agent 读取了哪些文件。
- Agent 使用了哪些群笔记。
- Agent 的任务状态是什么。
- Agent 是否因为中断而重建上下文。
- Agent 对哪些信息形成了长期记忆。

---

# 6. 需求范围

## 6.1 V1.0 需要覆盖

1. 群空间管理
2. Agent 管理
3. Agent 入群
4. 群聊消息
5. Agent 唤醒和响应
6. Agent 任务执行
7. 消息中断与恢复
8. 群笔记
9. 群公告
10. 群文件
11. Agent 看板
12. 权限审批
13. 基础记忆系统
14. Agent 协作机制
15. 基础数据模型

---

## 6.2 V1.0 暂不覆盖或弱覆盖

1. 完整插件市场
2. 移动端完整执行能力
3. 企业级私有化部署
4. 跨组织 Agent 共享市场
5. 复杂工作流编排器
6. 高级自动化触发器
7. 多模态实时语音协作

---

# 7. 产品信息架构

## 7.1 一级模块

```text
AgentChat
├── 左侧导航
│   ├── 最近会话
│   ├── 群空间
│   ├── 私聊
│   ├── Agent 资产库
│   ├── 文件库
│   └── 设置
│
├── 中间主区域
│   ├── 群聊消息流
│   ├── 任务执行流
│   ├── Agent 回复
│   ├── 用户输入框
│   └── 上下文引用卡片
│
└── 右侧面板
    ├── 群信息
    ├── 群成员
    ├── Agent 看板
    ├── 群笔记
    ├── 群文件
    ├── 任务列表
    ├── 权限审批
    └── 运行日志
```

---

# 8. 核心场景

## 8.1 场景一：用户创建一个产品设计群

### 用户目标

用户希望围绕一个新产品想法，让多个 Agent 协作完成需求分析、PRD、原型建议和开发任务拆解。

### 流程

1. 用户点击「新建群」。
2. 输入群名称：AI 协作产品设计。
3. 选择群类型：产品设计。
4. 系统推荐 Agent：
   - 产品经理 Agent
   - 用户研究 Agent
   - 竞品分析 Agent
   - 技术架构 Agent
   - UI 设计 Agent
5. 用户确认创建。
6. 群创建成功。
7. 系统生成群公告和初始群笔记。
8. 用户在群中发送需求。
9. 多个 Agent 根据角色分别响应。

---

## 8.2 场景二：用户邀请 Agent 入群

### 用户目标

用户发现当前群需要一个新的 Agent，例如「数据分析 Agent」。

### 流程

1. 用户点击群成员区域的「邀请 Agent」。
2. 打开 Agent 资产库。
3. 搜索或选择「数据分析 Agent」。
4. 设置该 Agent 的群内权限：
   - 可读群消息
   - 可读群文件
   - 可写群笔记
   - 可创建任务
   - 修改文件需审批
5. 点击确认。
6. Agent 进入群。
7. 群消息提示：数据分析 Agent 已加入群聊。
8. Agent 自动读取群公告、群笔记和最近上下文。
9. Agent 发送自我介绍和可承担职责。

---

## 8.3 场景三：Agent 主动拉另一个 Agent 入群

### 用户目标

当前 Agent 发现任务需要其他专业能力，希望邀请另一个 Agent 参与。

### 流程

1. 用户要求产品经理 Agent 输出一版商业化方案。
2. 产品经理 Agent 判断需要财务测算能力。
3. Agent 在群中发起建议：

```text
产品经理 Agent：
这个任务涉及收入模型和成本结构，建议邀请「财务分析 Agent」协助。
是否邀请？
```

4. 用户点击「确认邀请」。
5. 系统将财务分析 Agent 加入群。
6. 财务分析 Agent 读取必要上下文。
7. 两个 Agent 协作完成商业化方案。

### 规则

- Agent 不能直接拉其他 Agent 入群。
- Agent 只能提出邀请建议。
- 用户或管理员确认后才能执行。
- 被邀请 Agent 的权限使用默认模板，也可由用户调整。

---

## 8.4 场景四：Agent 执行长任务时被中断

### 用户目标

Agent 正在执行长任务时，用户临时提出新问题，希望不会丢失原任务状态。

### 流程

1. 用户让开发 Agent 实现一个功能。
2. Agent 开始执行：
   - 分析需求
   - 读取文件
   - 修改代码
   - 运行测试
3. 用户突然发送新消息：

```text
先停一下，帮我解释一下你刚才为什么这么设计？
```

4. 系统检测到当前 Agent 正在执行长任务。
5. 消息进入「中断处理」。
6. Agent 暂停当前任务，并保存 checkpoint：
   - 当前任务目标
   - 已读取文件
   - 已完成步骤
   - 当前推理摘要
   - 待继续步骤
7. Agent 回复用户的插入问题。
8. 用户点击「继续原任务」。
9. Agent 从 checkpoint 恢复执行。

---

## 8.5 场景五：Agent 修改文件需要用户审批

### 用户目标

用户希望 Agent 可以生成或修改内容，但关键写入操作需要确认。

### 流程

1. 用户要求 Agent 修改 PRD 文档。
2. Agent 读取文件并生成修改建议。
3. 系统展示 diff：
   - 删除内容
   - 新增内容
   - 修改原因
4. 用户选择：
   - 批准全部
   - 拒绝全部
   - 逐条批准
   - 要求修改
5. 用户批准后，系统写入文件。
6. 群内生成审计记录。

---

# 9. 功能需求

---

## 9.1 群空间管理

### 9.1.1 创建群

用户可以创建一个新的群空间。

#### 输入字段

| 字段 | 类型 | 必填 | 说明 |
|---|---|---:|---|
| 群名称 | string | 是 | 最多 50 字 |
| 群描述 | text | 否 | 群的用途 |
| 群类型 | enum | 否 | 通用、产品、研发、运营、研究、自定义 |
| 初始 Agent | list | 否 | 可选择多个 |
| 权限模板 | enum | 否 | 个人、团队、企业 |
| 是否开启群记忆 | boolean | 是 | 默认开启 |
| 是否允许 Agent 建议拉人 | boolean | 是 | 默认开启 |
| 是否允许 Agent 自动创建任务 | boolean | 是 | 默认开启 |

#### 创建后默认生成

- 群 ID
- 群公告
- 群默认笔记
- 群成员列表
- 群权限配置
- 群消息根线程
- 群级记忆空间

---

### 9.1.2 群列表

左侧展示用户参与的所有群。

#### 展示信息

- 群头像
- 群名称
- 最后一条消息
- 未读数
- 正在运行的 Agent 数量
- 是否有待审批事项
- 是否有异常任务

#### 排序规则

优先级从高到低：

1. 置顶群
2. 有待用户处理事项的群
3. 最近活跃群
4. 创建时间倒序

---

### 9.1.3 群设置

群主或管理员可以修改：

- 群名称
- 群头像
- 群描述
- 群公告
- 群权限
- Agent 默认权限
- 群记忆开关
- 群消息保留策略
- 文件访问策略
- 审批策略

---

## 9.2 Agent 资产库

### 9.2.1 Agent 是全局资产

Agent 不直接属于某一个群，而是属于用户或组织。

一个 Agent 可以被加入多个群。

### 9.2.2 Agent 基础属性

| 字段 | 类型 | 说明 |
|---|---|---|
| agent_id | string | Agent 唯一 ID |
| owner_id | string | 所属用户或组织 |
| name | string | Agent 名称 |
| avatar | string | 头像 |
| description | text | 描述 |
| system_prompt | text | 系统提示词 |
| model_config | object | 模型配置 |
| tool_config | object | 工具配置 |
| memory_policy | object | 记忆策略 |
| visibility | enum | 私有、组织可见、公开 |
| status | enum | 启用、停用、归档 |
| created_at | datetime | 创建时间 |
| updated_at | datetime | 更新时间 |

---

### 9.2.3 创建 Agent

用户可以创建自定义 Agent。

#### 创建方式

1. 从模板创建
2. 从空白创建
3. 从现有 Agent 复制
4. 从群内对话沉淀生成

#### Agent 模板示例

- 产品经理 Agent
- 研发工程师 Agent
- 测试 Agent
- UI 设计 Agent
- 用户研究 Agent
- 法务 Agent
- 财务 Agent
- 数据分析 Agent
- 项目经理 Agent

---

### 9.2.4 Agent 配置项

#### 基础配置

- 名称
- 头像
- 描述
- 角色定位
- 语言风格
- 默认响应方式

#### 模型配置

- 模型供应商
- 模型名称
- temperature
- max tokens
- 上下文窗口
- 是否允许模型降级
- 是否允许多模型路由

#### 工具配置

- 搜索工具
- 文件读写工具
- 代码执行工具
- Git 工具
- 数据库工具
- API 调用工具
- MCP 工具
- 自定义工具

#### 记忆配置

- 是否开启全局记忆
- 是否开启群内记忆
- 记忆写入策略
- 记忆读取策略
- 记忆过期策略
- 用户是否需要审批记忆写入

---

## 9.3 Agent 入群

### 9.3.1 用户邀请 Agent 入群

入口：

- 群成员面板
- 输入框 `@添加Agent`
- Agent 资产库
- 任务推荐
- 系统推荐

### 9.3.2 入群配置

用户邀请 Agent 入群时，需要确认：

| 配置项 | 说明 |
|---|---|
| 群内昵称 | 默认使用 Agent 名称 |
| 群内角色 | 如产品、研发、评审、观察者 |
| 可访问上下文范围 | 全部群消息 / 最近消息 / 指定线程 |
| 可访问文件范围 | 全部文件 / 指定目录 / 不可访问 |
| 可写权限 | 是否可写笔记、任务、文件 |
| 工具权限 | 是否可调用外部工具 |
| 审批策略 | 高风险操作是否需要审批 |
| 自动响应策略 | 被 @ 才响应 / 关键词响应 / 主动响应 |

---

### 9.3.3 Agent 入群初始化

Agent 入群后需要执行初始化流程。

#### 初始化读取内容

优先级：

1. 群公告
2. 群目标
3. 群内置顶笔记
4. 最近 N 条消息
5. 当前活跃任务
6. 与自身角色相关的文件
7. 其他 Agent 的角色说明

#### 初始化输出

Agent 自动发送一条入群消息：

```text
大家好，我是「技术架构 Agent」。
我已读取群公告、当前目标和最近 30 条上下文。
我可以负责：
1. 技术方案设计
2. 系统模块拆解
3. 数据模型建议
4. 风险评估
如需我参与，请 @我。
```

---

## 9.4 群消息系统

### 9.4.1 消息类型

| 消息类型 | 说明 |
|---|---|
| 用户文本消息 | 用户输入 |
| Agent 文本消息 | Agent 回复 |
| 系统消息 | 入群、退群、权限变更等 |
| 任务消息 | Agent 执行任务进展 |
| 文件消息 | 上传、创建、修改文件 |
| 审批消息 | 权限申请、diff 审批 |
| 引用消息 | 引用上下文 |
| checkpoint 消息 | 任务断点 |
| 工具调用消息 | Agent 调用工具的可视化记录 |

---

### 9.4.2 @ 机制

用户可以通过 @ 指定 Agent 响应。

#### 支持形式

```text
@产品经理Agent 帮我整理一下需求
@技术Agent 看看这个方案能不能实现
@所有Agent 各自从自己的角度评审一下
```

### 9.4.3 Agent 响应规则

Agent 的响应由群配置和 Agent 配置共同决定。

#### 响应模式

| 模式 | 说明 |
|---|---|
| 仅被 @ 响应 | 默认安全模式 |
| 关键词响应 | 检测到相关任务主动响应 |
| 主动建议 | Agent 可以主动提出建议 |
| 静默观察 | 只读取，不发言 |
| 禁言 | 不读取也不发言 |

V1.0 默认：

- 新入群 Agent 默认为“仅被 @ 响应”。
- 用户可以手动开启主动建议。
- Agent 主动建议需限制频率，避免刷屏。

---

### 9.4.4 消息引用

Agent 回复必须标明其参考的关键上下文。

例如：

```text
根据你在 14:32 提到的“群是主容器”，以及群笔记《Agent 权限模型》，我建议……
```

可折叠展示：

- 参考消息
- 参考文件
- 参考笔记
- 参考任务
- 参考记忆

---

## 9.5 Agent 工作线程

### 9.5.1 Thread 设计

每个 Agent 在群中拥有独立工作线程。

一个 Agent 可以同时拥有多个任务线程，但默认同一 Agent 同一时间只执行一个主任务。

### 9.5.2 Thread 类型

| 类型 | 说明 |
|---|---|
| chat_thread | 普通对话 |
| task_thread | 长任务执行 |
| review_thread | 审核任务 |
| tool_thread | 工具调用任务 |
| recovery_thread | 中断恢复任务 |

---

### 9.5.3 Thread 状态

```text
created
running
waiting_user
waiting_approval
interrupted
paused
completed
failed
cancelled
```

---

### 9.5.4 Checkpoint 机制

Agent 在执行任务过程中需要定期创建 checkpoint。

#### 触发时机

1. 长任务开始时
2. 每完成一个关键步骤
3. 调用高风险工具前
4. 用户插入新消息时
5. 系统即将重启上下文时
6. Agent 即将超出上下文窗口时
7. 任务完成时

#### Checkpoint 内容

| 字段 | 说明 |
|---|---|
| checkpoint_id | 唯一 ID |
| thread_id | 所属线程 |
| agent_id | 所属 Agent |
| group_id | 所属群 |
| task_goal | 当前任务目标 |
| progress_summary | 当前进度摘要 |
| completed_steps | 已完成步骤 |
| pending_steps | 待完成步骤 |
| referenced_messages | 已引用消息 |
| referenced_files | 已读取文件 |
| tool_calls | 已调用工具 |
| intermediate_outputs | 中间产物 |
| next_action | 下一步动作 |
| created_at | 创建时间 |

---

## 9.6 消息中断与上下文恢复

### 9.6.1 中断场景

当 Agent 正在执行长任务时，用户可能会：

- 追问当前设计原因
- 临时要求暂停
- 插入新需求
- 修改任务目标
- 要求转交另一个 Agent
- 要求回滚
- 要求查看中间状态

### 9.6.2 中断处理策略

系统需要判断新消息是否为中断。

#### 判断维度

| 维度 | 示例 |
|---|---|
| 是否 @ 当前运行 Agent | @开发Agent 先停一下 |
| 是否包含暂停语义 | 等等、先别做、暂停 |
| 是否包含任务修改语义 | 改成、不要、重新 |
| 是否要求解释 | 为什么这样做 |
| 是否开启新任务 | 另外帮我做 |
| 是否与当前任务强相关 | 对刚才那个方案补充一下 |

---

### 9.6.3 中断后的行为

系统提供用户选择：

1. 暂停当前任务并处理新消息
2. 将新消息加入当前任务需求
3. 创建新任务线程
4. 忽略该消息对当前任务的影响
5. 取消当前任务

---

### 9.6.4 上下文恢复

当用户点击「继续任务」后，Agent 根据 checkpoint 重建上下文。

恢复过程：

1. 读取最新 checkpoint。
2. 拉取相关消息。
3. 拉取相关文件。
4. 拉取群笔记。
5. 生成恢复摘要。
6. 向用户确认或直接继续。
7. 继续执行下一步。

---

## 9.7 群笔记

### 9.7.1 群笔记定位

群笔记是群级长期上下文的一部分，用于沉淀结构化信息。

适合保存：

- 项目目标
- 决策记录
- 需求列表
- 术语定义
- 产品方案
- 技术方案
- 待办清单
- 会议纪要
- Agent 分工

---

### 9.7.2 群笔记类型

| 类型 | 说明 |
|---|---|
| 普通笔记 | Markdown 文档 |
| 决策记录 | 记录关键决策 |
| 需求文档 | PRD、需求池 |
| 技术文档 | 架构、接口、数据模型 |
| 任务笔记 | 任务相关上下文 |
| Agent 记忆摘要 | Agent 生成的长期摘要 |

---

### 9.7.3 笔记权限

- 用户可创建、编辑、删除。
- Agent 可申请创建或编辑。
- Agent 默认不能直接删除。
- Agent 修改重要笔记需要审批。
- 所有修改保留版本历史。

---

### 9.7.4 笔记与上下文

Agent 在回复时可以引用群笔记。

系统需要支持：

- 笔记向量化检索
- 关键词检索
- 手动引用
- 置顶笔记优先读取
- 与任务线程绑定

---

## 9.8 群公告

### 9.8.1 群公告作用

群公告是 Agent 入群时必须读取的最高优先级上下文。

建议包含：

- 群目标
- 协作规则
- 当前阶段
- 输出标准
- 权限要求
- 禁止事项

### 9.8.2 群公告示例

```markdown
# 群目标
本群用于设计 AgentChat 多 Agent 群协作产品。

# 当前阶段
正在进行 V1.0 PRD 设计。

# 协作规则
1. Agent 默认被 @ 后才回复。
2. 涉及产品决策时，先给方案再给建议。
3. 修改文档需要用户确认。
4. 重要结论需要写入群笔记。

# 输出要求
所有方案需包含目标、流程、边界、风险和数据结构。
```

---

## 9.9 群文件

### 9.9.1 文件能力

支持用户上传、创建、管理文件。

文件类型包括：

- Markdown
- 文本文档
- PDF
- 图片
- CSV
- Excel
- Word
- 代码文件
- 压缩包
- 其他附件

---

### 9.9.2 文件权限

文件可配置访问范围：

| 权限 | 说明 |
|---|---|
| 群成员可读 | 人和 Agent 都可读 |
| 仅用户可读 | Agent 不可读 |
| 指定 Agent 可读 | 只有特定 Agent 可读 |
| 可写需审批 | Agent 修改前需审批 |
| 禁止写入 | Agent 只能读取 |

---

### 9.9.3 Agent 文件操作

Agent 可执行：

- 读取文件
- 总结文件
- 对比文件
- 生成新文件
- 提出修改建议
- 创建 diff
- 申请写入
- 引用文件片段

默认规则：

- 读操作根据权限直接执行。
- 写操作默认需要审批。
- 删除操作 V1.0 不允许 Agent 执行。

---

## 9.10 Agent 看板

### 9.10.1 看板定位

Agent 看板用于实时展示群内所有 Agent 的状态、上下文、任务和运行过程。

### 9.10.2 看板展示内容

每个 Agent 卡片展示：

- Agent 名称
- 当前状态
- 当前任务
- 执行进度
- 当前使用工具
- 最近 checkpoint
- 消耗 token
- 已运行时间
- 是否等待用户
- 是否有异常
- 快捷操作

---

### 9.10.3 Agent 状态

```text
idle 空闲
reading_context 读取上下文
thinking 思考中
responding 回复中
using_tool 使用工具
waiting_user 等待用户输入
waiting_approval 等待审批
paused 已暂停
interrupted 已中断
completed 已完成
failed 失败
offline 离线
```

---

### 9.10.4 快捷操作

用户可以对 Agent 执行：

- @它
- 暂停
- 继续
- 取消任务
- 查看上下文
- 查看工具调用
- 查看 checkpoint
- 调整权限
- 移出群聊
- 创建新任务
- 转交任务

---

## 9.11 Agent 协作

### 9.11.1 协作模式

#### 模式一：用户分配任务

用户直接 @ 多个 Agent。

```text
@产品Agent @技术Agent 请分别评估这个方案的产品价值和技术风险。
```

#### 模式二：Agent 建议协作

Agent 发现自己无法独立完成任务，建议邀请或唤醒其他 Agent。

#### 模式三：系统推荐协作

系统根据任务内容推荐相关 Agent。

#### 模式四：任务转交

一个 Agent 将任务转交给另一个 Agent，但需要用户确认。

---

### 9.11.2 Agent 之间的通信

V1.0 推荐采用“群内可见通信”，不做完全隐式的 Agent 私下通信。

也就是说，Agent 之间的协作应当在群消息或任务线程中可见。

原因：

- 用户可监督
- 避免黑盒协作
- 便于审计
- 方便上下文沉淀

---

### 9.11.3 Agent 互评机制

用户可以要求多个 Agent 互相评审。

示例：

```text
@技术Agent 请评审产品Agent刚才的方案。
@产品Agent 请根据技术Agent的意见修订方案。
```

系统也可提供快捷按钮：

- 让其他 Agent 评审
- 让专家 Agent 复核
- 发起多 Agent 投票
- 汇总多 Agent 意见

---

## 9.12 权限模型

### 9.12.1 角色体系

群内人类用户角色：

| 角色 | 权限 |
|---|---|
| 群主 | 全部权限 |
| 管理员 | 管理成员、Agent、文件、审批 |
| 普通成员 | 发消息、上传文件、调用 Agent |
| 访客 | 只读或有限发言 |

Agent 群内角色：

| 角色 | 权限 |
|---|---|
| 核心 Agent | 可参与主要任务 |
| 协作 Agent | 被调用时参与 |
| 观察 Agent | 只读上下文，不主动发言 |
| 工具 Agent | 负责工具执行 |
| 审核 Agent | 负责审核和评估 |
| 禁用 Agent | 暂停读取和响应 |

---

### 9.12.2 权限维度

| 权限项 | 说明 |
|---|---|
| read_messages | 读取群消息 |
| read_notes | 读取群笔记 |
| write_notes | 写群笔记 |
| read_files | 读取群文件 |
| write_files | 写群文件 |
| create_tasks | 创建任务 |
| manage_tasks | 管理任务 |
| invite_agents | 建议邀请 Agent |
| use_tools | 使用工具 |
| call_external_api | 调外部 API |
| modify_memory | 写入记忆 |
| access_private_context | 访问私有上下文 |

---

### 9.12.3 高风险操作审批

以下操作默认需要审批：

1. 修改文件
2. 删除文件
3. 写入长期记忆
4. 调用外部 API
5. 发送外部消息
6. 执行代码
7. 访问敏感文件
8. 拉 Agent 入群
9. 修改群公告
10. 修改权限配置

---

## 9.13 激活源与通知机制

### 9.13.1 Agent 激活源

Agent 可以被以下事件激活：

| 激活源 | 说明 |
|---|---|
| 用户 @ | 用户主动点名 |
| 群消息 | 满足关键词或规则 |
| 定时任务 | 定时触发 |
| checkpoint 恢复 | 继续历史任务 |
| 审批通过 | 继续执行等待中的操作 |
| 文件变更 | 文件更新后触发 |
| 任务状态变化 | 任务被转交或更新 |
| Webhook | 外部系统触发 |

---

### 9.13.2 通知类型

通知包括：

- Agent 完成任务
- Agent 等待审批
- Agent 执行失败
- Agent 请求上下文
- Agent 建议邀请其他 Agent
- 有新的 checkpoint
- 文件发生变更
- 群内有新的重要决策

---

# 10. 数据模型

## 10.1 Group 群表

```sql
CREATE TABLE groups (
  id UUID PRIMARY KEY,
  owner_id UUID NOT NULL,
  org_id UUID,
  name VARCHAR(100) NOT NULL,
  avatar_url TEXT,
  description TEXT,
  group_type VARCHAR(50),
  announcement TEXT,
  memory_enabled BOOLEAN DEFAULT TRUE,
  allow_agent_suggest_invite BOOLEAN DEFAULT TRUE,
  allow_agent_create_task BOOLEAN DEFAULT TRUE,
  default_agent_response_mode VARCHAR(50) DEFAULT 'mentioned_only',
  status VARCHAR(30) DEFAULT 'active',
  created_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL
);
```

---

## 10.2 Agent 表

```sql
CREATE TABLE agents (
  id UUID PRIMARY KEY,
  owner_id UUID NOT NULL,
  org_id UUID,
  name VARCHAR(100) NOT NULL,
  avatar_url TEXT,
  description TEXT,
  system_prompt TEXT NOT NULL,
  model_config JSONB,
  tool_config JSONB,
  memory_policy JSONB,
  visibility VARCHAR(30) DEFAULT 'private',
  status VARCHAR(30) DEFAULT 'active',
  created_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL
);
```

---

## 10.3 Group Agent 关联表

```sql
CREATE TABLE group_agents (
  id UUID PRIMARY KEY,
  group_id UUID NOT NULL,
  agent_id UUID NOT NULL,
  display_name VARCHAR(100),
  role VARCHAR(50),
  response_mode VARCHAR(50) DEFAULT 'mentioned_only',
  permissions JSONB,
  context_scope JSONB,
  file_scope JSONB,
  approval_policy JSONB,
  status VARCHAR(30) DEFAULT 'active',
  joined_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL,
  UNIQUE(group_id, agent_id)
);
```

---

## 10.4 Group Members 群成员表

```sql
CREATE TABLE group_members (
  id UUID PRIMARY KEY,
  group_id UUID NOT NULL,
  user_id UUID NOT NULL,
  role VARCHAR(30) DEFAULT 'member',
  status VARCHAR(30) DEFAULT 'active',
  joined_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL,
  UNIQUE(group_id, user_id)
);
```

---

## 10.5 Messages 消息表

```sql
CREATE TABLE messages (
  id UUID PRIMARY KEY,
  group_id UUID NOT NULL,
  thread_id UUID,
  sender_type VARCHAR(20) NOT NULL, -- user, agent, system
  sender_id UUID,
  message_type VARCHAR(50) NOT NULL,
  content TEXT,
  content_json JSONB,
  references JSONB,
  reply_to_message_id UUID,
  status VARCHAR(30) DEFAULT 'visible',
  created_at TIMESTAMP NOT NULL
);
```

---

## 10.6 Threads 工作线程表

```sql
CREATE TABLE threads (
  id UUID PRIMARY KEY,
  group_id UUID NOT NULL,
  agent_id UUID,
  created_by UUID,
  thread_type VARCHAR(50),
  title VARCHAR(200),
  goal TEXT,
  status VARCHAR(50),
  priority INT DEFAULT 0,
  current_checkpoint_id UUID,
  started_at TIMESTAMP,
  completed_at TIMESTAMP,
  created_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL
);
```

---

## 10.7 Checkpoints 表

```sql
CREATE TABLE checkpoints (
  id UUID PRIMARY KEY,
  group_id UUID NOT NULL,
  thread_id UUID NOT NULL,
  agent_id UUID NOT NULL,
  task_goal TEXT,
  progress_summary TEXT,
  completed_steps JSONB,
  pending_steps JSONB,
  referenced_messages JSONB,
  referenced_files JSONB,
  tool_calls JSONB,
  intermediate_outputs JSONB,
  next_action TEXT,
  context_snapshot JSONB,
  created_at TIMESTAMP NOT NULL
);
```

---

## 10.8 Notes 群笔记表

```sql
CREATE TABLE notes (
  id UUID PRIMARY KEY,
  group_id UUID NOT NULL,
  title VARCHAR(200) NOT NULL,
  note_type VARCHAR(50),
  content TEXT,
  created_by_type VARCHAR(20), -- user, agent
  created_by_id UUID,
  visibility VARCHAR(30) DEFAULT 'group',
  pinned BOOLEAN DEFAULT FALSE,
  version INT DEFAULT 1,
  status VARCHAR(30) DEFAULT 'active',
  created_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL
);
```

---

## 10.9 Files 文件表

```sql
CREATE TABLE files (
  id UUID PRIMARY KEY,
  group_id UUID NOT NULL,
  uploader_type VARCHAR(20),
  uploader_id UUID,
  name VARCHAR(255) NOT NULL,
  path TEXT NOT NULL,
  mime_type VARCHAR(100),
  size BIGINT,
  permissions JSONB,
  embedding_status VARCHAR(30),
  status VARCHAR(30) DEFAULT 'active',
  created_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL
);
```

---

## 10.10 Approvals 审批表

```sql
CREATE TABLE approvals (
  id UUID PRIMARY KEY,
  group_id UUID NOT NULL,
  thread_id UUID,
  agent_id UUID,
  requester_type VARCHAR(20),
  requester_id UUID,
  approval_type VARCHAR(50),
  title VARCHAR(200),
  description TEXT,
  payload JSONB,
  status VARCHAR(30) DEFAULT 'pending',
  approved_by UUID,
  approved_at TIMESTAMP,
  rejected_reason TEXT,
  created_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL
);
```

---

## 10.11 Memories 记忆表

```sql
CREATE TABLE memories (
  id UUID PRIMARY KEY,
  scope_type VARCHAR(30) NOT NULL, -- user, group, agent, group_agent
  scope_id UUID NOT NULL,
  memory_type VARCHAR(50),
  content TEXT NOT NULL,
  embedding VECTOR,
  source_type VARCHAR(50),
  source_id UUID,
  confidence FLOAT,
  status VARCHAR(30) DEFAULT 'active',
  created_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL
);
```

---

# 11. 核心交互设计

## 11.1 PC 端布局

采用微信 PC 风格：

```text
┌────────────────────────────────────────────┐
│ 左侧列表         中间聊天区          右侧面板 │
│                                            │
│ 群 A             群消息流            群信息   │
│ 群 B             Agent 回复          Agent看板│
│ 私聊             输入框              文件/笔记│
│ Agent库                              审批    │
└────────────────────────────────────────────┘
```

---

## 11.2 左侧区域

### 包含

- 搜索框
- 最近会话
- 群列表
- 私聊列表
- Agent 资产库入口
- 设置入口

### 群列表特殊状态

- 运行中标识
- 待审批标识
- 异常标识
- 未读消息数
- Agent 正在输入标识

---

## 11.3 中间聊天区

### 顶部

- 群名称
- 在线成员数
- 运行中 Agent 数
- 快捷入口：群设置、邀请 Agent、任务看板

### 消息流

展示：

- 用户消息
- Agent 消息
- 工具调用
- 任务进度
- 审批卡片
- checkpoint 卡片
- 文件变更卡片

### 输入框

支持：

- 文本输入
- @Agent
- 上传文件
- 创建任务
- 引用消息
- 插入群笔记
- 选择执行模式

---

## 11.4 右侧面板

支持 Tab 切换：

1. 群信息
2. Agent 看板
3. 群成员
4. 群笔记
5. 群文件
6. 任务
7. 审批
8. 日志

---

# 12. 关键页面需求

## 12.1 群首页

### 主要元素

- 群消息流
- 输入框
- 当前活跃 Agent 状态
- 右侧群信息

### 用户操作

- 发消息
- @Agent
- 上传文件
- 查看 Agent 状态
- 发起任务
- 暂停任务
- 审批操作

---

## 12.2 Agent 资产库页面

### 功能

- 查看全部 Agent
- 创建 Agent
- 编辑 Agent
- 复制 Agent
- 删除/归档 Agent
- 查看 Agent 被加入哪些群
- 查看 Agent 使用统计

---

## 12.3 Agent 配置页

### 配置项

- 基本信息
- 系统提示词
- 模型配置
- 工具配置
- 权限模板
- 记忆策略
- 默认响应模式
- 测试对话

---

## 12.4 群设置页

### 配置项

- 群名称
- 群公告
- 群权限
- 成员管理
- Agent 管理
- 文件策略
- 记忆策略
- 审批策略
- 删除群

---

## 12.5 审批中心

### 审批类型

- Agent 修改文件
- Agent 写入记忆
- Agent 调用外部 API
- Agent 邀请其他 Agent
- Agent 访问敏感文件
- Agent 执行代码

### 审批动作

- 同意
- 拒绝
- 修改后同意
- 仅本次同意
- 永久允许
- 调整权限

---

# 13. 核心流程

## 13.1 创建群流程

```text
点击新建群
 → 输入群名称
 → 选择群类型
 → 选择初始 Agent
 → 选择权限模板
 → 确认创建
 → 生成群空间
 → Agent 初始化
 → 进入群聊
```

---

## 13.2 Agent 被 @ 响应流程

```text
用户发送 @Agent 消息
 → 消息入库
 → 调度器识别目标 Agent
 → 检查 Agent 状态
 → 检查权限
 → 构建上下文
 → 创建 thread
 → Agent 推理
 → 流式输出
 → 创建 checkpoint
 → 任务完成
```

---

## 13.3 Agent 长任务流程

```text
用户发起任务
 → Agent 确认任务目标
 → 创建 task_thread
 → 生成执行计划
 → 创建 checkpoint
 → 分步骤执行
 → 每步更新状态
 → 必要时请求审批
 → 生成最终结果
 → 写入任务记录
 → 完成
```

---

## 13.4 中断恢复流程

```text
Agent 执行中
 → 用户插入消息
 → 判断是否中断
 → 保存 checkpoint
 → 暂停当前线程
 → 处理插入消息
 → 用户选择继续
 → 加载 checkpoint
 → 重建上下文
 → 继续执行
```

---

## 13.5 Agent 建议拉人流程

```text
Agent 判断需要协作
 → 生成邀请建议
 → 展示推荐 Agent
 → 用户确认
 → 设置权限
 → Agent 入群
 → 初始化上下文
 → 开始协作
```

---

# 14. 调度与运行机制

## 14.1 Agent 调度器

调度器负责：

- 识别消息目标 Agent
- 判断是否需要响应
- 分配任务线程
- 管理 Agent 状态
- 管理中断
- 管理 checkpoint
- 调用模型与工具
- 处理错误重试

---

## 14.2 同进程与独立线程

根据当前方案：

- Agent 运行在同一服务进程中。
- 每个 Agent 任务拥有独立线程或异步任务。
- 每个 Agent 使用独立 tokio runtime 或隔离执行单元。
- panic 隔离，避免单个 Agent 崩溃影响整个群。
- 工具调用需要独立超时和取消机制。

---

## 14.3 Agent 协作中的 Git Worktree

针对代码类任务：

- 每个 Agent 每个任务使用独立 branch。
- 每个 Agent 可使用独立 git worktree。
- 避免多个 Agent 同时修改同一工作区。
- 合并前展示 diff。
- 用户确认后合并到主分支。

---

# 15. 上下文系统

## 15.1 上下文来源

Agent 构建上下文时可读取：

1. 当前用户消息
2. 当前 thread 历史
3. 群公告
4. 置顶群笔记
5. 相关群笔记
6. 最近群消息
7. 被引用消息
8. 相关文件
9. Agent 个人记忆
10. 群内 Agent 记忆
11. checkpoint
12. 任务状态

---

## 15.2 上下文优先级

优先级从高到低：

1. 当前用户明确指令
2. 当前审批/权限约束
3. 群公告
4. 当前 thread 上下文
5. 被用户引用的消息或文件
6. 置顶群笔记
7. 相关群笔记
8. 最近消息
9. Agent 记忆
10. 全局记忆

---

## 15.3 上下文压缩

当上下文超过模型窗口时：

1. 保留当前任务目标。
2. 保留用户最新指令。
3. 保留关键决策。
4. 对历史消息进行摘要。
5. 对工具调用结果进行摘要。
6. 对中间过程写入 checkpoint。
7. 明确标注摘要来源。

---

# 16. 搜索机制

## 16.1 搜索范围

用户和 Agent 可搜索：

- 群消息
- 私聊消息
- 群笔记
- 群文件
- Agent 记录
- 任务记录
- checkpoint
- 审批记录

---

## 16.2 搜索方式

- 关键词搜索
- 语义搜索
- 条件筛选
- 时间筛选
- 按 Agent 筛选
- 按文件类型筛选
- 按任务状态筛选

---

## 16.3 Agent 使用搜索

Agent 调用搜索时必须返回引用来源。

例如：

```text
我检索了以下上下文：
1. 群笔记《V1.0设计原则》
2. 4月28日 15:20 的产品讨论
3. 文件 /docs/schema.md
```

---

# 17. 非功能需求

## 17.1 性能

| 指标 | 要求 |
|---|---|
| 群列表加载 | < 1s |
| 消息发送入库 | < 300ms |
| Agent 首 token 返回 | < 3s，复杂任务可展示排队状态 |
| 普通消息检索 | < 1s |
| 语义搜索 | < 3s |
| 文件上传 50MB | < 10s，视网络 |
| Agent 状态刷新 | 实时或 < 1s |

---

## 17.2 稳定性

- Agent 任务失败不影响群消息系统。
- 单个 Agent 崩溃不影响其他 Agent。
- 工具调用失败需要可重试。
- 长任务必须可恢复。
- 所有关键操作需要日志。
- 消息流使用 SSE 或 WebSocket 保证实时性。

---

## 17.3 安全

- Agent 权限最小化。
- 高风险操作审批。
- 文件访问控制。
- 外部 API 调用审计。
- 敏感信息脱敏。
- 支持组织级数据隔离。
- 用户可查看 Agent 访问记录。

---

## 17.4 可观测性

需要记录：

- Agent 响应耗时
- 模型调用耗时
- 工具调用耗时
- token 消耗
- 错误日志
- checkpoint 数量
- 中断次数
- 审批通过率
- 用户反馈

---

# 18. 埋点与指标

## 18.1 核心指标

| 指标 | 含义 |
|---|---|
| DAU | 日活用户 |
| WAU | 周活用户 |
| 创建群数 | 用户创建的群空间数量 |
| 活跃群数 | 有消息或任务的群 |
| Agent 入群数 | Agent 被加入群的次数 |
| Agent 调用次数 | 用户 @ 或系统激活 Agent 的次数 |
| 多 Agent 协作次数 | 同一任务中多个 Agent 参与次数 |
| 长任务完成率 | task_thread completed / created |
| 中断恢复成功率 | interrupted 后 completed 比例 |
| 审批通过率 | approvals approved / total |
| 文件修改成功率 | Agent 提交 diff 并被采纳比例 |
| 用户留存 | 次日、7日、30日留存 |

---

## 18.2 行为埋点

### 群相关

- create_group
- update_group
- delete_group
- open_group
- pin_group

### Agent 相关

- create_agent
- invite_agent_to_group
- remove_agent_from_group
- mention_agent
- agent_response_start
- agent_response_complete
- agent_task_failed

### 任务相关

- create_thread
- start_task
- pause_task
- interrupt_task
- resume_task
- complete_task
- cancel_task

### 审批相关

- create_approval
- approve_action
- reject_action
- modify_permission

---

# 19. MVP 范围

## 19.1 MVP 必须做

1. 群创建与群列表
2. Agent 创建与资产库、skills、mcp
3. Agent 入群
4. 群聊消息
5. @Agent 响应
6. SSE 流式输出
7. Agent 状态展示
8. 基础 checkpoint
9. 群笔记
10. 文件上传和读取
11. Agent 修改文件审批
12. 简单权限模型
13. 右侧 Agent 看板
14. 每个群即是一个workspace
---

# 20. Roadmap

## 阶段一：MVP，0-2 个月

目标：验证“群 + Agent”交互是否成立。

功能：

- 群空间
- Agent 资产库
- Agent 入群
- @Agent 回复
- 群笔记
- 群文件
- 基础任务线程
- Agent 看板
- 简单审批
- SSE 流式消息

---

## 阶段二：协作增强，2-4 个月

目标：让多 Agent 协作真正可用。

功能：

- 多 Agent 任务协作
- Agent 建议拉人
- 中断恢复增强
- checkpoint 可视化
- 语义搜索
- 任务看板
- 文件 diff 审批
- Agent 互评
- 群内决策沉淀

---

## 阶段三：工程化执行，4-6 个月

目标：支持代码、文档、数据等复杂执行任务。

功能：

- Git worktree
- 独立 branch
- 代码修改审批
- 测试运行
- 工具调用审计
- 外部 API 权限
- 长任务后台运行
- 错误恢复
- 企业知识库接入

---

## 阶段四：企业化，6-12 个月

目标：支持组织级使用和规模化部署。

功能：

- 组织空间
- 企业权限
- 审计日志
- 私有化部署
- SSO
- 组织 Agent 市场
- 数据隔离
- 成本统计
- 管理后台

---

# 21. 风险与应对

## 21.1 多 Agent 容易刷屏

### 风险

多个 Agent 同时响应会造成消息噪音。

### 应对

- 默认仅被 @ 响应。
- 主动响应需要用户开启。
- 同一轮最多 N 个 Agent 主动响应。
- 支持 Agent 静默观察。
- 支持一键暂停所有 Agent。

---

## 21.2 上下文混乱

### 风险

群消息太多，Agent 容易引用错误上下文。

### 应对

- 线程化任务。
- 引用来源可见。
- 群笔记沉淀关键结论。
- checkpoint 保存任务状态。
- 上下文优先级明确。

---

## 21.3 权限风险

### 风险

Agent 可能误修改文件、误调用工具或泄露信息。

### 应对

- 最小权限。
- 高风险审批。
- 操作审计。
- 文件级权限。
- Agent 行为日志。
- 默认不允许删除。

---

## 21.4 长任务失败

### 风险

Agent 执行时间长，可能中断、超时、上下文丢失。

### 应对

- checkpoint。
- 任务线程。
- 状态机。
- 可恢复执行。
- 错误重试。
- 用户可手动继续。

---

## 21.5 用户理解成本高

### 风险

用户不理解群、Agent、线程、checkpoint 的关系。

### 应对

- UI 类微信群，降低心智成本。
- 默认配置足够简单。
- 高级能力渐进暴露。
- Agent 入群自动介绍。
- 使用模板群快速开始。

---

# 22. 成功标准

## 22.1 MVP 成功标准

上线后 4 周内：

1. 50% 新用户成功创建至少一个群。
2. 40% 新用户邀请至少一个 Agent 入群。
3. 30% 活跃用户在一个群中使用两个及以上 Agent。
4. Agent @ 响应成功率 > 95%。
5. 长任务 checkpoint 恢复成功率 > 80%。
6. 用户对“群式 Agent 协作”的理解度调研 > 70%。
7. 单群平均消息数 > 20。
8. 单群平均 Agent 数 > 2。

---

# 23. 推荐 V1.0 产品命名

可选名称：

1. AgentChat
2. Agent群
3. WorkAgent
4. AgentRoom
5. ChatOps AI
6. TeamAgent
7. AI 工作群
8. SwarmChat

推荐使用：

> AgentChat

原因：

- 易理解。
- 突出聊天式交互。
- 兼容个人和团队场景。
- 后续可扩展为 AgentChat Cloud / AgentChat Enterprise。

---

# 24. 总结

这款产品的核心不是再做一个普通聊天机器人，而是重新定义 Agent 的协作容器。

核心判断：

> 群是上下文容器，Agent 是可调度成员，任务是协作单元，checkpoint 是恢复机制，权限审批是安全边界，群笔记是长期记忆。

V1.0 最关键的产品闭环是：

```text
创建群
 → 邀请 Agent
 → 在群里 @Agent
 → Agent 基于群上下文工作
 → 任务过程可见
 → 中断可恢复
 → 结果可沉淀
```

只要这个闭环成立，后续就可以自然扩展到多 Agent 项目协作、代码执行、企业知识库、组织级 AI 工作台和 Agent 市场。

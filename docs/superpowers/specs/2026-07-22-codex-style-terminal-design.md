# Codex 风格本地终端设计

日期：2026-07-22
状态：已确认，等待实施计划

## 1. 背景与目标

AG Swarmer 的聊天页已经包含工作区文件与 Git 侧栏，但用户不能在绑定工作区中直接操作一个持续存在的交互式终端。本功能在 Tauri 桌面端增加 Codex 风格的底部终端：终端从聊天内容区底部展开，支持真实 PTY、多标签、实时输入输出、ANSI 控制、窗口尺寸同步以及跨页面切换的会话保持。

首版目标：

- 在本地工作区启动完整的系统 Shell，而不是逐条执行命令的输出面板。
- 每个群聊或私聊拥有独立的终端集合。
- 支持多个标签；每个标签对应一个独立 PTY 和 Shell 进程。
- 折叠终端、切换聊天或将窗口隐藏到托盘时，终端继续运行。
- 真正退出应用时结束所有终端；下次启动只恢复标签元数据并创建新 Shell。
- 桌面端通过 Tauri 原生 PTY 实现，同时用 Transport 接口为未来浏览器端保留扩展边界。
- Windows 作为首版发布验收平台；内部接口保持跨平台。

## 2. 已确认的产品决策

| 主题 | 决策 |
| --- | --- |
| 交互能力 | 完整交互式 PTY 终端 |
| 首版环境 | Tauri 桌面端；浏览器端只保留接口 |
| 布局 | 聊天内容区底部可拖拽面板 |
| 会话归属 | 每个聊天独立 |
| 标签能力 | 多标签；支持新建、重命名、切换和关闭 |
| 页面切换 | 不终止会话 |
| 应用重启 | 恢复标签名称、顺序和启动目录；启动新 Shell |
| Agent 关系 | 用户终端与 Agent 工具调用完全独立 |
| 文件系统权限 | 完整本机 Shell；初始目录为工作区，但允许访问其他路径 |
| 发布平台 | Windows 首发，保持跨平台结构 |

## 3. 范围外事项

首版不包含：

- 浏览器或远程 WebSocket 终端。
- 云沙箱工作区终端。
- Agent 命令输出镜像到终端，或 Agent 与用户共享输入会话。
- 横向或纵向分屏。
- SSH、容器终端和自定义远程配置。
- 应用退出后继续运行的守护进程或旧 PTY 恢复。
- 对 Shell 的工作区沙箱限制。
- 命令、输出、环境变量或 Shell 历史的持久化。
- 完整 Shell Profile 管理界面。

## 4. 总体架构

数据流如下：

```text
React TerminalDock / xterm.js
            |
            v
TerminalTransport（环境无关契约）
            |
            v
TauriTerminalTransport（invoke + ordered Channel）
            |
            v
Rust TerminalManager（Tauri managed state）
            |
            v
跨平台 PTY -> 系统 Shell
```

Tauri 官方将 Channel 定位为快速、有序的流式通信机制，适用于子进程输出等高频数据，因此终端输出使用每会话 Channel，而不是全局事件广播。命令型操作继续使用 Tauri `invoke`。终端渲染使用 xterm.js；尺寸同步使用 FitAddon 与 `ResizeObserver`。

### 4.1 前端组件

`TerminalRuntimeProvider` 挂载在长期存在的应用主布局层，而不是具体聊天页内部。它负责：

- 保存所有存活会话的运行时对象。
- 保持 Channel 订阅，使路由切换不会丢失输出。
- 隔离不同 `conversationId` 的标签集合和活动标签。
- 在注销、聊天删除或应用退出时触发清理。
- 管理版本化的标签元数据持久化。

`TerminalDock` 负责底部面板布局和当前聊天的标签展示。主布局持续挂载终端运行时；离开聊天路由时仅隐藏面板，不销毁 PTY。每个存活会话保留其 xterm.js 实例，非活动实例通过隐藏容器保持状态，直到标签关闭或应用退出。

建议的前端单元边界：

- `TerminalDock`：面板、拖拽条、折叠和最大化。
- `TerminalTabBar`：标签切换、新建、重命名、关闭和退出状态。
- `TerminalPane`：xterm.js 生命周期、输入、粘贴、聚焦和尺寸观察。
- `TerminalRuntimeProvider`：跨路由运行时与清理。
- `TerminalTransport`：运行环境无关协议。
- `TauriTerminalTransport`：Tauri IPC 实现。
- `terminalMetadataStore`：仅保存可恢复的非敏感元数据。

### 4.2 Transport 契约

Transport 暴露以下能力：

```ts
interface TerminalTransport {
  create(request: CreateTerminalRequest, onEvent: (event: TerminalEvent) => void): Promise<TerminalDescriptor>
  write(conversationId: string, sessionId: string, data: string): Promise<void>
  resize(conversationId: string, sessionId: string, cols: number, rows: number): Promise<void>
  close(conversationId: string, sessionId: string): Promise<void>
}
```

`CreateTerminalRequest` 包含 `conversationId`、绝对启动目录、初始 `cols` 和 `rows`。`TerminalDescriptor` 至少包含随机 `sessionId`、解析后的 Shell 名称和启动目录。

终端事件使用可判别联合：

- `output`：PTY 字节的 Base64 表示；前端还原为 `Uint8Array` 后写入 xterm.js，避免 UTF-8 字符被分块破坏。
- `exit`：可选退出码和信号信息。
- `error`：稳定错误码及可显示消息。

未来浏览器端通过实现 `WebSocketTerminalTransport` 复用同一 UI、状态模型与契约测试；首版不实现服务端终端接口。

### 4.3 Rust PTY 管理器

桌面 Rust 层新增 `TerminalManager` 作为 Tauri managed state。它维护：

- `sessionId -> TerminalSession` 映射。
- 会话所属 `conversationId`。
- PTY master、writer、child/process-group 句柄。
- 输出读取任务和有序 Channel。
- 会话退出与关闭状态。

每次 `write`、`resize` 和 `close` 同时提交 `conversationId` 与 `sessionId`。管理器在操作前校验归属，拒绝跨聊天控制。`sessionId` 使用不可预测的随机 UUID。

首版采用跨平台 PTY 抽象；Windows 走 ConPTY，macOS/Linux 保留相同接口。具体 crate 版本在实施计划中锁定并通过最小探针验证输入、缩放、退出和进程树清理。

Tauri 命令集合：

- `terminal_create(request, channel)`
- `terminal_write(conversation_id, session_id, data)`
- `terminal_resize(conversation_id, session_id, cols, rows)`
- `terminal_close(conversation_id, session_id)`
- `terminal_close_all()`，仅用于注销和退出流程

重启标签由前端关闭旧运行时后重新调用 `terminal_create`，无需单独的 Rust 重启命令。

## 5. Shell 与工作目录解析

终端只对桌面端的本地工作区启用。创建前满足：

- 聊天绑定了本地工作区。
- 工作区路径是存在的绝对目录。
- 当前运行环境是 Tauri 桌面端。

Windows Shell 依次尝试：

1. PATH 中的 `pwsh`。
2. Windows PowerShell。
3. `cmd.exe`。

macOS/Linux 优先使用有效且可执行的 `$SHELL`，再回退到平台常见 Shell，最后回退 `/bin/sh`。Shell 解析逻辑位于独立、可单测的模块中。

用户新建标签时，启动目录默认为当前聊天最新绑定的工作区根目录。持久化的“目录”指标签的启动目录，不追踪用户执行 `cd` 后的实时目录；应用重启后从保存的启动目录创建新 Shell。若保存目录已失效，则回退到当前工作区根目录，并在标签中提示。

终端是完整本机 Shell。它可以离开工作区并访问当前操作系统用户有权访问的文件和进程。首次使用终端时显示一次明确提示；这不是安全沙箱。

## 6. 交互与视觉行为

### 6.1 入口与快捷键

- 聊天标题栏提供终端图标按钮。
- `Ctrl/Cmd + \`` 切换展开与折叠。
- 快捷键仅在聊天路由生效，并避免覆盖输入法组合态或已有文本编辑快捷键。
- 当前聊天还没有标签时，首次打开自动创建默认 Shell。

### 6.2 底部面板

- 面板默认高度为聊天内容区的 35%。
- 最小高度 180px，最大高度为聊天内容区的 70%。
- 顶部拖拽条调整高度；双击恢复默认高度。
- 拖拽期间前端即时布局，但 PTY resize 合并发送，只提交最新行列数。
- 折叠只隐藏面板，不结束任何进程。
- 最大化让终端临时占满聊天内容区，再次点击恢复之前高度。
- 高度作为用户级布局偏好保存，聊天的展开状态和活动标签按聊天保存。

### 6.3 标签

- `+` 创建新标签。
- 单击切换；双击名称进入内联重命名。
- 关闭标签会终止对应 Shell 及其进程树。
- 正在运行的标签显示轻量活动状态。
- 进程退出后保留 xterm.js 滚动内容，标签显示退出码并提供“重新启动”。
- 关闭最后一个标签后保留空面板，提供“新建终端”；用户主动折叠后才隐藏。

### 6.4 终端行为

- 支持 ANSI 颜色和控制序列、中文、emoji、命令历史、Tab 补全及 Ctrl+C。
- 使用 xterm.js 原生选择和复制；粘贴遵循桌面端标准快捷键。
- 字体采用现有产品主题中的等宽终端字体栈，颜色从主题 token 派生，暗色和亮色主题都满足可读性要求。
- xterm.js scrollback 使用有限配置，首版默认 5,000 行，避免无限内存增长。

## 7. 生命周期与持久化

### 7.1 运行期

- 切换聊天：原聊天终端继续运行；目标聊天显示自己的标签集合。
- 折叠或最大化切换：PTY 不受影响。
- 窗口关闭到系统托盘：终端继续运行。
- 标签关闭：终止单个会话。
- 聊天删除：终止并清除该聊天全部会话和元数据。
- 用户注销：终止全部会话，防止终端跨账号暴露。
- 从托盘真正退出：终止全部终端后再关闭内置后端和应用。

### 7.2 应用重启

本地存储使用带版本号的键保存：

- `conversationId`
- 标签稳定元数据 ID、名称和顺序
- 启动目录
- 活动标签
- 面板展开状态和高度偏好

不保存命令、输出、环境变量、PTY 状态或 Shell 历史。启动后仅恢复标签描述；当用户打开对应聊天时按需创建新的 Shell。创建失败不会删除元数据，用户可修正工作区后重试或关闭标签。

## 8. 性能与背压

- Rust 按有限大小读取 PTY 输出并发送分块事件，目标块大小不超过 16 KiB。
- 前端将同一动画帧内的输出合并后写入 xterm.js，降低 React 与 WebView 调度压力。
- React 状态不保存完整终端输出；输出直接进入 xterm.js buffer。
- Channel 生命周期由全局 Terminal Runtime 持有，不随聊天组件卸载。
- resize 使用短时间合并策略，只保留最后一次尺寸。
- scrollback 限制为默认 5,000 行，可在未来设置中开放，但不属于首版。

如果前端 Channel 已断开或应用正在退出，Rust 停止发送并进入对应会话的关闭流程，不建立无界输出队列。

## 9. 错误处理与资源回收

错误仅影响对应标签，不阻塞聊天：

- 不支持的运行环境或工作区：显示稳定的不可用状态，不调用 IPC。
- 启动目录无效：显示原因，并允许使用当前工作区根目录重试。
- Shell 不可用或 PTY 创建失败：显示错误和“重试”。
- 写入失败：将标签转为异常状态，并等待退出事件或允许关闭。
- 正常或异常退出：保留输出、显示退出码并允许重启。
- resize 失败：记录诊断信息；终端继续工作，下一次尺寸变化可重试。

资源回收遵循幂等原则，重复关闭同一会话不会报用户可见错误。关闭流程：

1. 标记会话正在关闭并拒绝新输入。
2. 尝试正常终止 Shell。
3. 短暂等待退出。
4. 强制终止完整进程组或进程树。
5. 关闭 PTY reader/writer 和 Channel，移除管理器记录。

Windows 使用能覆盖后代进程的 Job/进程树策略；Unix 使用进程组。实现必须通过“子进程再启动子进程”的真实清理测试，不能只验证直接 Shell 进程退出。

## 10. 测试策略

### 10.1 Rust 自动化测试

- 默认 Shell 解析与回退。
- 绝对目录、不存在目录和非目录输入。
- 会话 ID 随机性及聊天归属校验。
- 输入、resize、退出、重启和幂等关闭。
- 正常终止超时后的强制清理。
- TerminalManager 的并发会话隔离。
- 使用假 PTY 验证命令契约，不要求单元测试启动真实 Shell。

### 10.2 IPC 契约测试

- `create/output/exit/error` 的序列化。
- 输出事件顺序和 Base64 字节往返。
- Channel 断开与重复关闭。
- Tauri Transport 使用模拟 IPC；未来 WebSocket Transport 必须通过相同契约测试。

### 10.3 React 测试

- 标题栏按钮和快捷键开关面板。
- 首次打开自动创建标签。
- 新建、切换、重命名、关闭和重启标签。
- 拖拽高度、默认高度恢复、折叠和最大化。
- 退出状态、启动错误和不支持状态。
- 标签元数据恢复及无效目录回退。
- 不同聊天之间的会话隔离。
- 路由切换后 Transport 会话仍存活。
- resize 合并和组件清理。
- 与聊天输入框及现有快捷键不冲突。

### 10.4 Windows 桌面验收

- 验证 `pwsh -> Windows PowerShell -> cmd` 回退。
- 验证 ANSI、中文、emoji、交互命令、历史、Tab 补全和 Ctrl+C。
- 验证拖拽与最大化后的 PTY 行列同步。
- 验证长输出时聊天界面仍可操作。
- 验证切换聊天、折叠和隐藏到托盘后会话继续。
- 验证关闭标签、注销和退出应用后不遗留 Shell 或后代进程。
- 验证应用重启后恢复标签元数据，但不恢复旧输出和旧进程。
- 验证浏览器和云工作区只显示不可用提示，不产生终端 IPC。

### 10.5 回归范围

- 群聊与私聊消息发送、流式输出和 Composer。
- 工作区文件、Git 和笔记侧栏。
- 当前聊天页宽度与高度调整。
- Tauri 隐藏到托盘及真正退出流程。
- 中英文界面文案和明暗主题。

## 11. 可观测性

桌面日志记录结构化终端生命周期事件：创建、解析后的 Shell 类型、启动失败、退出码、关闭原因和强制清理结果。日志不记录用户输入、终端输出、环境变量或完整命令行内容。前端只显示适合用户理解的错误，诊断细节写入现有桌面日志目录。

## 12. 参考资料

- [Tauri：Calling the Frontend from Rust](https://v2.tauri.app/develop/calling-frontend/)
- [Tauri：Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/)
- [xterm.js：Using addons](https://xtermjs.org/docs/guides/using-addons/)

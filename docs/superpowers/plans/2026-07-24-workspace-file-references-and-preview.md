# 工作区文件引用、编辑与预览实施计划

> **For agentic workers:** 按任务顺序执行；每项完成后进行独立审查。用户明确要求不采用 TDD：先实现任务，再补齐该任务的自动化测试与验证。

**目标：** 在群聊和私聊中支持把右侧工作区文件拖入聊天输入框引用，并提供安全的文本编辑、图片/HTML/PDF 预览和通用下载能力。

**架构：** 后端抽取会话绑定工作区的授权和路径安全逻辑，使群聊与私聊共享列表、读取流、文本读取与条件保存端点；消息附件验证也改为按会话查询工作区。前端把现有群组专用文件面板改为会话通用组件，Composer 通过结构化拖放载荷区分文件附件和目录文本，详情面板根据后端元数据选择渲染器。

**技术栈：** Rust/Axum/SQLx、React 19、TanStack Query、TypeScript、Vite、Vitest、Tailwind、现有 Tauri 下载能力。

## 全局约束

- 不采用 TDD；实现完成后再集中添加并运行自动化测试。
- 仅支持活动的本地工作区；禁止云端、未绑定、跨用户、跨会话、绝对路径、`..`、UNC 和符号链接逃逸。
- 文件拖入消息只能引用现有普通文件；目录拖入只插入规范化相对路径，绝不作为附件。
- 文本保存必须携带内容摘要版本；发生外部修改时返回冲突，不覆盖磁盘内容。
- HTML 预览必须使用无 `allow-scripts`、`allow-same-origin`、`allow-forms`、`allow-popups` 的 sandbox iframe。
- 复用既有本地化、`fetchJson`、认证头、错误样式及 Tauri 下载路径。

---

## 文件结构与职责

- `backend-rs/crates/backend/src/api/workspace_files.rs`：新增共享的会话工作区授权、路径解析、元数据、文本版本和原子保存服务。
- `backend-rs/crates/backend/src/api/groups.rs`：将群聊工作区文件处理迁移到共享服务，保留群组特有上传/Git 路由。
- `backend-rs/crates/backend/src/api/direct_chats.rs`：暴露私聊会话的工作区文件 handlers。
- `backend-rs/crates/backend/src/api/messages.rs`：统一群聊/私聊的附件工作区验证调用。
- `backend-rs/crates/backend/src/api/mod.rs`：注册共享与私聊文件路由。
- `backend-rs/crates/backend/tests/groups.rs`、`backend-rs/crates/backend/tests/direct_chats.rs`：端到端权限、预览、保存冲突和附件测试。
- `frontend/src/lib/workspaceDrag.ts`：结构化拖放项目编码、解码与不可信载荷过滤。
- `frontend/src/hooks/useConversationWorkspaceFiles.ts`：按会话 scope 和 ID 请求文件列表、读取、流、保存、上传及失效缓存。
- `frontend/src/components/chat/Composer.tsx`：文件附件与目录路径插入、拖放视觉状态、去重和附件行元数据。
- `frontend/src/components/chat/ConversationWorkspacePanel.tsx`：替代群组专用面板，面向群聊/私聊传递 `scope`、`conversationId` 与 Composer 回调。
- `frontend/src/components/chat/WorkspaceFilesTab.tsx`：使用通用 hooks、创建结构化拖放源、承载类型化详情预览。
- `frontend/src/components/chat/workspace-preview/*`：文本编辑、图像、HTML、PDF、通用文件预览的独立组件。
- `frontend/src/components/chat/ConversationChatView.tsx`：将统一文件面板与 Composer 的文件/目录接收回调连接。
- `frontend/src/types/api.ts`、`frontend/src/i18n/resources/{en-US,zh-CN}.ts`：新增会话工作区 API 类型与本地化文案。
- 对应 `*.test.tsx`、`*.test.ts`：验证纯函数、Composer、预览组件、面板接入和 API hooks。

## Task 1：共享会话工作区文件安全服务与 API

**Files:**
- Create: `backend-rs/crates/backend/src/api/workspace_files.rs`
- Modify: `backend-rs/crates/backend/src/api/groups.rs`
- Modify: `backend-rs/crates/backend/src/api/direct_chats.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Modify: `backend-rs/crates/backend/src/api/messages.rs`

**Consumes:** `groups` 表中的 `owner_id`、`workspace_id`、`conversation_kind`；既有 `resolve_workspace_path` 和文件下载逻辑。

**Produces:** 统一的 `ConversationScope`、`load_owned_local_workspace`、`read_workspace_file_text`、`save_workspace_file_text`、`stream_workspace_file` 和 `validate_conversation_attachments` 接口；群聊与私聊的等价文件 API。

- [ ] 实现会话归属和活动本地工作区解析：按 scope (`groups` / `direct-chats`)、conversation ID 和认证用户加载会话，验证会话类型、所有者、活动状态、工作区状态、`backend_type = local`，并 canonicalize 根路径。
- [ ] 将路径解析封装为普通文件/目录两个入口：拒绝空根、绝对/UNC、父目录片段、非 UTF-8 路径、目录误作文件、失效路径和 canonicalize 后根目录外的符号链接。
- [ ] 实现文件列表、文件信息和原有受鉴权下载流的通用 handlers；用实际路径推断 `Content-Type`，对下载端点保留安全的 `Content-Disposition` 文件名。
- [ ] 实现文本读取返回：`path`、`name`、`mime_type`、`size`、`content`、`is_text`、`truncated`、`version`、`message`。版本使用 SHA-256（或同等强度摘要）覆盖完整磁盘字节；最大读取与编辑字节数为常量，并在截断时禁止保存。
- [ ] 实现条件保存 `PATCH`：接收 `{ content, version }`，验证 UTF-8 与最大完整文本大小，重新读取并比较摘要，冲突返回 `409`，一致时以同目录临时文件 + rename 原子替换，并返回新元数据与版本。
- [ ] 将原有群聊预览、下载和附件验证迁移到共享服务；新增私聊的 `/workspace-files`、`/preview`、`/download`、`/text`、`/text/save` 路由。保留群组上传、重命名、删除、Git 端点。
- [ ] 将群聊和私聊消息路径都改为同一个附件验证函数，保证大小、MIME、文件类型、去重和持久化元数据规则相同。
- [ ] 格式化并编译后端：`cargo fmt --manifest-path backend-rs/Cargo.toml`，`cargo check --manifest-path backend-rs/Cargo.toml --workspace`。
- [ ] 提交：`feat(workspace-files): add shared conversation file API`

## Task 2：后端安全与并发回归测试

**Files:**
- Modify: `backend-rs/crates/backend/tests/groups.rs`
- Modify: `backend-rs/crates/backend/tests/direct_chats.rs`
- Modify: `backend-rs/crates/backend/tests/groups.rs`（若测试辅助函数应抽取，则在同文件公共 test helpers 区域抽取）

**Consumes:** Task 1 的共享路由和版本化文本 response/request。

**Produces:** 群聊和私聊的安全覆盖与消息附件持久化回归测试。

- [ ] 为群聊编写端到端测试：读取 UTF-8 文本返回内容/版本，带正确版本保存更新磁盘，保存后版本变化。
- [ ] 在读取之后从测试直接写入磁盘，再用旧版本保存，断言 `409`、磁盘保持外部内容、响应不泄露路径。
- [ ] 覆盖截断文本、二进制、目录、删除后的文件、过大内容和保存时符号链接逃逸均不能写入。
- [ ] 为私聊创建绑定本地工作区的对话，覆盖列表、图片/PDF/HTML 下载流、文本读取保存和附件消息持久化。
- [ ] 为群聊与私聊分别覆盖跨 owner、错误会话类型、无工作区、云端工作区、`../`、绝对路径、UNC 和根外符号链接访问失败。
- [ ] 运行：`cargo test --manifest-path backend-rs/Cargo.toml --test groups --test direct_chats`；再运行 `cargo clippy --manifest-path backend-rs/Cargo.toml --all-targets -- -D warnings`。
- [ ] 提交：`test(workspace-files): cover conversation file access and saves`

## Task 3：前端通用数据模型、认证流和拖放协议

**Files:**
- Modify: `frontend/src/types/api.ts`
- Modify: `frontend/src/lib/workspaceDrag.ts`
- Create: `frontend/src/lib/workspaceDrag.test.ts`
- Create: `frontend/src/hooks/useConversationWorkspaceFiles.ts`
- Create: `frontend/src/hooks/useConversationWorkspaceFiles.test.tsx`

**Consumes:** Task 1 定义的群聊/私聊 URL 形状及文本 response/save contract。

**Produces:** `ConversationScope`、文件项目与文本版本类型；`encodeWorkspaceDragItem`/`decodeWorkspaceDragItem`；统一的 React Query hooks。

- [ ] 在 API 类型中新增 `ConversationScope`、会话工作区文件、预览、文本读取和条件保存 request/response；保留旧群组类型的兼容别名直到 Task 5 完成迁移。
- [ ] 用版本化自定义 MIME（例如 `application/x-ag-swarmer-workspace-item+json`）替换仅有路径数组的拖放编码；严格验证 JSON 对象字段、相对路径和 `file | directory` 类型，拒绝 `text/plain` 伪造为工作区项目。
- [ ] 保留 `workspacePathsFromDataTransfer` 给旧的“插入所选路径”操作，但新增 `workspaceItemsFromDataTransfer` 给 Composer 拖放；纯函数中做去重、空值过滤和目录/文件分类。
- [ ] 实现 scope 到路由前缀的单一映射，所有 hooks 都通过该映射构造 URL、query key 与缓存失效前缀。
- [ ] 实现列表、预览、二进制 Blob、文本读取、条件保存、上传、下载 hooks；所有 fetch 走现有 token 并在 mutation 成功后失效当前会话文件树和预览缓存。
- [ ] 为 Blob 辅助函数提供 `URL.revokeObjectURL` 生命周期 API；调用者不得把 token 放进 URL。
- [ ] 实现后补测试：拖放 JSON 的有效/无效/去重分支，群聊与私聊 URL 映射，保存成功后的缓存失效和冲突错误保留。
- [ ] 运行：`pnpm --filter @ag-swarmer/frontend test -- workspaceDrag useConversationWorkspaceFiles`、`pnpm --filter @ag-swarmer/frontend type-check`。
- [ ] 提交：`feat(workspace-files): add conversation file client contracts`

## Task 4：Composer 文件引用和目录路径拖放

**Files:**
- Modify: `frontend/src/components/chat/Composer.tsx`
- Modify: `frontend/src/components/chat/Composer.test.tsx`
- Modify: `frontend/src/components/chat/ConversationChatView.tsx`
- Modify: `frontend/src/components/chat/ConversationChatView.test.tsx`
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`

**Consumes:** Task 3 的结构化工作区拖放项目和通用文件查询 hooks。

**Produces:** 文件拖放的待发送附件、目录路径的光标插入、可访问的拖放状态和跨群聊/私聊适配。

- [ ] 将 Composer 的 pending attachment 模型分为上传中的 `File` 附件和已存在的 `WorkspaceAttachment`，后者保留服务端确认的 path/name/mime/size，不伪造空 `File`。
- [ ] 拖入结构化工作区文件时先请求文件元数据，确认当前会话可访问且是普通文件后加入附件；按路径去重并不超过后端规定数量。外部系统文件仍走现有上传路径。
- [ ] 拖入目录时使用 textarea 的 `selectionStart`/`selectionEnd` 插入相对路径；无焦点时追加，用单个空格处理前后边界，并恢复插入后光标与焦点。
- [ ] 将 drop target 从 textarea 扩展到整个 Composer 卡片，新增 `dragenter`/`dragleave` 深度计数和视觉高亮，避免子元素切换时闪烁；禁用状态不接受拖放。
- [ ] 发送时将两类“已上传/已存在”附件统一为 `{ path }`，并仅在消息发送成功后移除；删除任一待发送条目绝不删除磁盘文件。
- [ ] 为路径插入和拖放错误添加中英文文案（文件已添加、目录路径已插入、不支持、无工作区、无法读取），同时补充 `aria-live` 状态说明。
- [ ] 实现后补测试：群聊与私聊 scope 的文件拖入发送、重复文件去重、目录在选区替换/末尾追加、外部文件继续上传、禁用 Composer 拒绝 drop、附件移除和发送 payload。
- [ ] 运行：`pnpm --filter @ag-swarmer/frontend test -- Composer ConversationChatView`。
- [ ] 提交：`feat(chat): support workspace file and folder drops`

## Task 5：统一文件面板和类型化安全预览

**Files:**
- Create: `frontend/src/components/chat/ConversationWorkspacePanel.tsx`
- Modify: `frontend/src/components/chat/GroupWorkspacePanel.tsx`
- Modify: `frontend/src/components/chat/WorkspaceFilesTab.tsx`
- Create: `frontend/src/components/chat/workspace-preview/WorkspaceTextEditor.tsx`
- Create: `frontend/src/components/chat/workspace-preview/WorkspaceImagePreview.tsx`
- Create: `frontend/src/components/chat/workspace-preview/WorkspaceHtmlPreview.tsx`
- Create: `frontend/src/components/chat/workspace-preview/WorkspacePdfPreview.tsx`
- Create: `frontend/src/components/chat/workspace-preview/WorkspaceFileFallback.tsx`
- Create: `frontend/src/components/chat/workspace-preview/WorkspacePreviewRouter.tsx`
- Modify: `frontend/src/components/chat/ConversationChatView.tsx`

**Consumes:** Task 3 hooks 和 Task 4 Composer 路径/附件接收回调。

**Produces:** 可供群聊与私聊复用的右侧面板，安全类型化预览和文本编辑 UI。

- [ ] 将 `GroupWorkspacePanel` 的 tabs、Git 功能和 group-specific props 保留为群聊包装器；引入 `ConversationWorkspacePanel` 负责所有会话共享的文件列表和 preview。私聊显示文件 tab，不显示群组 Git tab。
- [ ] 让 `ConversationChatView` 基于 `scope` 渲染正确面板，并向 Composer 与面板传递相同的会话工作区 context；确保已有面板宽度、隐藏状态和文件导航 store 行为不回归。
- [ ] 将文件行拖放源改为 Task 3 的结构化 item；目录和文件都有明确 `aria-grabbed`/拖拽说明，点击、键盘选择、双击导航和菜单行为保持。
- [ ] 实现 `WorkspacePreviewRouter`：以后端 `is_text` 与 MIME/后缀选择组件，不能依赖前端文件名作权限判断。
- [ ] 文本编辑器展示原始内容、脏状态、保存/刷新按钮、截断禁用状态和服务端错误。保存调用条件保存 mutation；`409` 时保留编辑并要求确认才能刷新覆盖本地缓冲。
- [ ] 图片预览仅使用认证下载的 Blob Object URL，遵循最大尺寸、加载失败 fallback 和完整大小 lightbox。
- [ ] HTML 预览把认证读取的 Blob URL 载入 `<iframe sandbox="">`，不添加任何放宽 sandbox 的 token；加载失败则显示下载 fallback。
- [ ] PDF 预览把认证 Blob URL 作为 `<object>` 或 `<iframe>` source，包含可访问标题和不支持浏览器的下载 fallback。
- [ ] 未知、二进制和 Office 文件显示 MIME、大小、路径与下载按钮；所有 Object URL 在组件切换/卸载时释放。
- [ ] 实现后补测试：群聊显示 Files/Git，私聊只显示 Files；每种 MIME 路由到正确预览；HTML iframe sandbox 为空且无额外权限；图片/PDF/未知 fallback；文本保存、截断禁用、冲突保留编辑和确认刷新。
- [ ] 运行：`pnpm --filter @ag-swarmer/frontend test -- WorkspaceFilesTab ConversationWorkspacePanel WorkspacePreview Composer`。
- [ ] 提交：`feat(workspace-files): add editor and secure file previews`

## Task 6：整合、可访问性、回归与交付验证

**Files:**
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`
- Modify: `README.md`
- Modify: 已有测试文件中受 scope/API 类型迁移影响的 mocks

**Consumes:** Tasks 1–5 的完整功能。

**Produces:** 全语言覆盖、端到端回归证据和用户可见说明。

- [ ] 审计所有新增状态、按钮、错误、冲突确认和预览 fallback 的英文/简体中文键，避免把英文错误或内部 API 名称直接展示给用户。
- [ ] 审计键盘与读屏行为：文件行可聚焦、拖放不替代选择/菜单、Composer 有 drop status、保存/刷新/下载可操作，预览 iframe/object 有标题。
- [ ] 在 README 的工作区文件功能说明中写明：文件拖入是引用不是复制，目录拖入插入路径，编辑只支持 UTF-8 文本且有并发冲突保护，HTML 受沙箱限制，Office 文档下载。
- [ ] 运行前端完整门槛：`pnpm --filter @ag-swarmer/frontend test`、`pnpm type-check`、`pnpm lint`、`pnpm build`。
- [ ] 运行后端完整门槛：`cargo fmt --manifest-path backend-rs/Cargo.toml -- --check`、`cargo test --manifest-path backend-rs/Cargo.toml --workspace`、`cargo clippy --manifest-path backend-rs/Cargo.toml --all-targets -- -D warnings`。
- [ ] 运行 `git diff --check`，确认仅包含本功能文件；人工桌面验收群聊和私聊各一次：文件拖入、目录拖入、文本保存、外部修改冲突、图片、PDF、HTML、未知文件下载。
- [ ] 提交：`docs: document workspace file references and previews`

## 计划自检

- 规格中的拖放、群聊/私聊、类型预览、版本冲突、安全边界和验证标准均对应至少一个任务。
- 没有使用 TDD 步骤；每个任务的测试都在实现之后。
- API 类型、拖放协议、会话 scope 和保存版本在任务间以明确接口传递。
- Office 转换、二进制编辑、递归目录附件、协同编辑和桌面任意路径访问均不在任务范围内。

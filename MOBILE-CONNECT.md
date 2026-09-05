# mobile-connect 开发方案

状态：PR 1–6 的主要代码已在工作区实现，自动化检查与生产构建已执行；尚未提交、部署或完成手机真机验收。用户已明确要求 Android 应用，PR 7 的 Android 首版现已实现，安装包与验证记录见 [android/README.md](android/README.md)；通知、配对、设备撤销和 iOS 仍未实施。

## 当前交付记录

- PWA：元数据、品牌图标、静态资源精确白名单 SW、更新提示、断网提示、安全区与 `100dvh`；认证失败清理与连接中止；HTTPS/CSP 代理示例见 `docker/Caddyfile.mobile` 和 `DOCKER.md`。
- 移动外壳：1024px 断点、导航 Drawer、工作区与 Git 全屏 Sheet、Assistant 底部 Sheet、触屏原生长按菜单、输入区上方的审批和人工输入。
- SSE：重试改用已有服务端新增的 GET 回放路由，不重复 POST 消息或审批；保存已应用游标、去重、合并前台/联网恢复、屏蔽旧连接回调。恢复查询接收取消信号，忽略取消后的迟到快照；首个 Turn 事件之前断线也会刷新消息。
- 终端：同一个 runtime/xterm 在桌面和手机之间切换；隐藏全屏视图保留 PTY 与画面；增加触控辅助键、异常 EOF 续接、重复帧去重和缺帧报错。页面重载后恢复旧 PTY 不在本次保证内。
- 部署：新增精确匹配的 `QUNICA_ALLOWED_ORIGINS` 配置及 HTTPS/SSE 代理说明；设备会话撤销与 Web Push 尚未实现。独立 Tauri Android 壳已按用户后续要求落地，采用 Keystore 凭据存储。

已执行的检查：

| 检查 | 结果 |
| --- | --- |
| 前端全量 Vitest（本轮修复前基线） | 113 个文件、781 项通过 |
| 本轮修复后的发送/审批恢复、SSE、HTTP 终端专项 | 4 个文件、49 项通过，含新增的 11 项边界测试 |
| TypeScript、生产构建 | 通过；构建仍提示部分 chunk 超过 500 kB |
| ESLint | 0 错误、14 条警告 |
| Rust `group_stream` | 124 项通过、1 项长工具循环失败；失败项 `group_stream_continues_past_24_tool_rounds` 单独重跑通过，尚未定位全套运行时的偶发原因 |
| Rust `mobile_cors_tests` | 1 项通过 |
| Rust `web_assets` | 7 项通过，包括 SW、主题脚本与 manifest 的内容和 MIME 类型 |
| Chrome 152 移动视口冒烟 | 登录页 360×800、390×844 无横向溢出；本地生产预览的 SW 激活，manifest 可读取 |

发布前仍需执行本方案的 iPhone Safari/PWA、Android Chrome/PWA、桌面/Tauri 与真实 HTTPS 代理验收，特别是已登录聊天、后台挂起、网络切换、中文软键盘、旋转与安全区、升级以及终端回进。Chrome 模拟视口不等同于这些验收；当前不将 M1/M2 标记为已发布。

## 1. 目标与范围

手机作为 Qunica 的远程控制客户端：查看会话、发送消息、跟踪任务、处理审批与人工输入、访问服务端工作区，并在后续阶段使用全屏终端。

```text
手机浏览器 / 同源 PWA
        │ HTTPS：普通请求 + SSE
        ▼
HTTPS 反向代理（可限制在可信 VPN 内访问）
        │ 内部连接
        ▼
Qunica Server
ACP · Agent CLI · PTY · 工作区 · 持久化消息 · Turn 状态
```

- 复用现有 React 前端与服务端 API，不新增后端服务；必要时小范围修改现有后端。
- 手机端按单任务、单栏、触控优先重新编排，共享业务组件、查询和状态处理；桌面布局保持原有行为。
- 手机不运行 ACP、Agent CLI、PTY，不访问服务器宿主机之外的本地工作目录。
- MVP 不做离线执行、离线发送队列、后台常驻或原生壳；网络恢复后由服务端状态校准界面。
- 默认按独立域名根路径部署；子路径部署有明确需求时，再统一调整资源地址、路由和 manifest scope。

## 2. 已核实的复用基础

| 位置 | 当前能力与限制 | 开发处理 |
| --- | --- | --- |
| `frontend/index.html` | 已有 viewport，缺少 `viewport-fit=cover` | 补 PWA 元数据 |
| `frontend/src/components/layout/AppLayout.tsx` | 主窗口挂载侧栏和 TerminalDock；有根级自定义右键菜单 | 按断点切换外壳，保留触屏原生菜单 |
| `frontend/src/components/layout/AppSidebar.tsx` | 现有桌面侧栏 | 在移动 Sheet 中复用内容 |
| `frontend/src/components/ui/sheet.tsx` | 已有 Sheet 与 Radix Dialog 基础 | 复用，不新增抽屉库 |
| `frontend/src/lib/api-v2/sse.ts` | 支持 `lastEventId`、`openWhenHidden` 和有限重试 | 补显式生命周期恢复 |
| `frontend/src/hooks/useSendMessageStream.ts` | 发送时生成 `client_request_id` | 恢复保持原请求身份 |
| `frontend/src/hooks/useResumeStream.ts` | 处理恢复执行与审批回答 | 区分恢复订阅和恢复执行 |
| `backend-rs/crates/backend/src/api/sse_replay.rs` | 按事件游标读取持久化回放 | 复用，确认失效与无游标分支 |
| `frontend/src/terminal/httpTransport.ts` | 服务端 PTY，SSE 输出，HTTP 输入和 resize | 复用传输；新建连接的附着能力仍需评估 |
| `frontend/src/terminal/TerminalRuntimeProvider.tsx` | 管理终端会话与输出订阅；输出缓冲不是完整历史快照 | 保持 runtime 稳定，不把隐藏等同于关闭 |
| `frontend/src/terminal/TerminalPane.tsx` | 创建 xterm 实例 | 避免页面切换导致画面丢失 |
| `frontend/src/stores/authStore.ts` | localStorage Token，已有 logout 和跨标签同步 | 补齐移动恢复时的失效与清理链路 |
| `backend-rs/crates/backend/src/api/mod.rs` | CORS predicate 内仍是写死的默认规则 | 按需增加精确 origin 配置 |
| `DOCKER.md`、`backend-rs/crates/backend/tests/web_assets.rs` | 已有同源 Web 服务和部署基础 | 扩展现有部署文档和测试 |

具体实现前仍需沿相关调用链检查，避免只修发送流而遗漏审批恢复流、群聊、私聊或线程。

## 3. PR 拆分与交付顺序

### PR 1：PWA 基础与首次远程部署的认证基础

实现：

- 在 `frontend/index.html` 增加 `viewport-fit=cover`、theme-color、manifest 和 apple-touch-icon。
- 新增 `frontend/public/manifest.webmanifest` 与图标，复用现有品牌素材；包含 name、short_name、start_url、scope、display: standalone，以及 192×192、512×512 图标。Apple 触屏图标单独提供合适尺寸。
- 在 `frontend/src/index.css` 和根布局建立 `100dvh` 高度链，保留合适的回退值、`min-height: 0` 和唯一聊天主滚动区。安全区覆盖上下左右，避免父子重复加 padding。
- 生产 Web 构建注册原生 Service Worker；开发环境及桌面壳不注册。先用浏览器原生 API，不新增 PWA 依赖。
- SW 仅处理同源、无认证头的 GET 静态资源白名单：构建产物、字体和图标。API、SSE、认证响应、工作区文件、附件和上传内容直接走网络；不得按扩展名宽泛缓存。
- HTML 导航先走网络；如提供离线入口，只回退到不含用户数据的固定应用外壳。离线明确显示断连，不将未发送消息伪装成成功。
- 缓存按构建版本管理，验证旧页面与旧 chunk 的兼容；提供更新提示，不在聊天输入或审批时自动刷新。
- 在生产 HTTPS 入口配置 CSP，核对现有内联样式、脚本及资源来源后收紧，禁止为图省事开放任意脚本来源。认证/API 响应禁用代理缓存。
- 复用登录和退出流程；普通 API、聊天 SSE 与终端 SSE 遇到认证失效时停止重试、清理会话内存并进入重新登录。主动退出还应中止活动连接并清理已有用户缓存。
- 按 `DOCKER.md` 配置首次账户、关闭公开注册及 HTTPS 入口；可信 VPN 不替代手机 PWA 的 HTTPS 部署要求。

验收：可安装并以 standalone 打开；安全区无遮挡；深链接刷新正常；缓存内无业务响应和 Token；退出后旧流不再更新界面；新版本更新不丢草稿。此 PR 不承诺离线聊天。

### PR 2：响应式 Shell

实现：

- 以 `< 1024px` 为紧凑布局断点，CSS 优先；确需控制挂载时使用一个共享 `useMediaQuery`，监听断点变化。
- 顶栏提供会话导航与当前会话名称；侧栏通过现有 Sheet 实现 Drawer，复用会话查询，选择后关闭。
- 聊天占满内容区；工作区文件和 Git 使用全屏 Sheet，Assistant 使用底部 Sheet。内容仍复用现有组件，避免复制页面。
- 紧凑布局不挂载桌面 TerminalDock；PR 6 完成后显示全屏终端入口。
- 布局变化不卸载共享会话状态或 TerminalRuntimeProvider，不触发任务取消或 PTY 删除。
- 根级右键菜单通过 `(pointer: fine)` 门控，触屏长按保留系统选择菜单；混合输入设备需额外验证，不能用屏幕宽度推断输入方式。
- Sheet 保留标题、焦点约束、关闭后焦点返回和可访问名称。浏览器返回先关闭当前覆盖层，再离开会话。

验收：手机单栏可用；1023/1024px 边界和旋转不重发任务；桌面主窗口和辅助窗口行为不回退；长按文字可复制。

### PR 3：SSE 生命周期与状态恢复

实现位置以现有 `sse.ts`、发送/恢复 hooks 及共享状态为主，不另建通用连接框架。

恢复步骤：

1. 为每个活动流记录会话、线程、stream ID、原请求身份、恢复所需请求参数，以及最后成功应用的 event ID。跨 stream 隔离，退出时清除。
2. 监听回到前台的 `visibilitychange` 与 `online`，合并并发触发；事件仅作为恢复信号，实际请求结果才代表连通性。
3. 作废旧连接代次并 abort 旧订阅，阻止迟到事件污染新状态；同一流同一时刻只允许一个恢复流程。
4. 重新查询消息、当前 Turn、待审批与待人工输入状态。订阅中断不等于服务端任务失败。
5. 仍在执行且有有效游标：以原请求参数及 `lastEventId` 恢复订阅；不得生成新的 `client_request_id`，不得自动再次提交审批答案。
6. 已完成或已取消：以服务端消息校准界面，停止该流恢复。等待用户操作时重建对应卡片，保留正确的操作对象 ID。
7. 游标无效：刷新服务端快照；若任务仍运行，采用已验证的只读跟踪/回放能力继续观察。现有接口不足时，小范围补现有后端，不能重复执行发送或 resume 来代替订阅。
8. 页面被系统回收后，从服务端重建状态；不只持久化游标而丢掉游标之前的局部消息。MVP 不持久化请求正文和完整流缓存。

关键边界：

- `openWhenHidden: true` 不保证手机后台存活。当前重试是连续失败后耗尽，有正常事件会重置计数。
- “连接内自动重试”和“新建连接显式恢复”都要验证游标与去重。底层库维护 SSE `id`，不能假定它等同于业务处理完成位置。
- 按 stream/序号确认增量事件去重；消息 ID 去重不能代替 token、工具和推理增量去重。
- 拉取快照与回放之间要建立清晰的状态替换边界，避免把快照已有文本再追加一次。
- 覆盖首事件前断线和无游标分支；普通发送可复用原请求 ID，但必须验证服务端幂等行为。没有相同幂等保证的 resume 路径不得盲重发。
- 认证失败停止恢复；取消、切换账户和卸载都需清理监听与旧连接。自动恢复失败后保留明确的手动重试入口。

验收：网络切换、重试耗尽、后台完成、首事件前断线、旧回调迟到、无效游标和页面重载均不重复创建任务，不重复追加文本，不丢待审批状态。

### PR 4：移动审批、人工输入与软键盘

实现：

- 工具调用和推理过程默认折叠为状态卡片，用户可展开查看详情。
- 待审批和人工输入固定在输入区上方；显示具体工具、操作内容及允许/拒绝，长内容独立展开，不挤掉输入框。
- 复用现有权限和回答接口；提交中防重复点击，失败可重试，服务端已处理时刷新状态。
- 触控目标至少 44×44 CSS px；支持屏幕阅读器、焦点可见性和文本缩放。
- 消息区保持唯一聊天主滚动容器，用户阅读历史时不强制滚到底部。
- 真机核对软键盘、中文输入法候选、横屏与 Sheet。`100dvh` 不作为键盘避让保证；出现遮挡时再加入最小 Visual Viewport 适配，不能通过禁止缩放解决。

验收：键盘打开时仍能看见输入与审批；长命令可完整查看；中文组合输入不误发送；任务在后台进入等待状态后回前台可继续处理。

### PR 5：部署完善与可配置 CORS

PR 1 已承担首次部署必要的 HTTPS 和认证基础，本 PR 完善配置与运维验证。

- 同源部署无需新增允许来源。出现跨源客户端时再增加 `QUNICA_ALLOWED_ORIGINS`，启动时解析，精确匹配协议、主机和有效端口。
- 不接受 `*`、路径、用户信息或模糊后缀匹配；非法配置启动时报错。明确保留哪些桌面/开发默认来源，不能声称新增配置自动收紧原规则。
- 跨源预检覆盖 Authorization、Content-Type、last-event-id 和实际 HTTP 方法；不为 Bearer Token 模式额外打开 cookie credentials。
- HTTPS 代理对 SSE 禁用响应缓冲与缓存，确认心跳、空闲超时和及时转发；验证业务流和终端流。
- 更新 `DOCKER.md`，给出同源生产入口、原端口仅内部可达、SW/CSP 与 SSE 检查步骤。
- 公网设备会话撤销按部署需求独立排期，不与 Tauri 绑定。当前 localStorage 清除不等于已签发 JWT 被服务端撤销；若要求逐设备撤销，需要现有后端增加会话检查。

验收：合法 origin 通过、相似恶意域名被拒绝、预检成功；代理下事件逐条到达；认证失败没有重试风暴。未实现设备撤销时，部署文档明确该限制。

### PR 6：全屏终端

- 复用 `TerminalPane`、`TerminalRuntimeProvider`、HTTP transport 及服务端 terminal API；只在用户首次打开时加载终端资源并按现有流程创建会话。
- MVP 全屏页面采用同一实例的呈现方式切换，第一次打开后保持 xterm 与 runtime 存活，关闭页面只隐藏视图；显式“关闭终端”才删除 PTY。
- 验证隐藏期间仍消费输出，重新打开不丢画面；禁止桌面 Dock 与移动全屏各建一个终端实例竞争同一输出。
- 增加 Esc、Tab、Ctrl 和方向键辅助栏。Ctrl 使用一次性组合状态并清晰显示，按键走现有串行 write，保留输入焦点。
- 在容器、方向和键盘可视区变化后 fit，再复用 resize；核对中文输入、粘贴、滚动与复制。
- 单独检查异常 EOF、后台挂起和续传游标窗口；当前 transport 的自动重连不等于页面重载后附着已有 PTY。
- MVP 保证同一页面生命周期内退出全屏再进入的连续性。页面重载后的旧 PTY 附着和完整画面恢复不承诺；提供明确状态及新开入口，验证遗留会话清理。若该能力成为发布要求，再补最小附着接口与恢复协议。

验收：返回聊天不杀 PTY；再次进入画面连续；旋转正确 resize；Ctrl+C 只发送一次；断线不丢失或重复输入；Token 失效退出终端访问。

### PR 7：明确需求触发后追加

- Web Push：有后台完成提醒需求时实施；手机通知能力先做平台验证，不靠 SSE 常驻实现推送，也不因需要推送就直接决定使用原生壳。
- Tauri 2 Mobile：应用商店或已验证无法满足的原生集成需求出现后再做。移动构建排除 `qunica-backend`、`portable-pty`、桌面启动器及本地 shell 权限。
- 移动壳仍作为远程客户端，Token 使用经验证的 Keychain/Keystore 集成，且运行时选择不能把“处于 Tauri”直接等同于“桌面原生终端”。
- 扫码配对采用短时、一次性授权；设备撤销由服务端控制。具体协议在启动该阶段时设计。

## 4. 测试与发布门槛

优先扩展现有 Vitest/组件测试和 Rust 集成测试，不新增测试框架。文档编写不代表以下检查已执行。

| 阶段 | 最小自动化检查 | 真机/部署检查 |
| --- | --- | --- |
| PR 1 | manifest/缓存白名单、认证失败停止重试；必要时扩展 `web_assets.rs` | 安装、深链接、升级、退出、缓存检查 |
| PR 2 | AppLayout、侧栏与 Sheet 的断点和焦点回归 | 长按、旋转、返回、桌面回归 |
| PR 3 | 在 SSE、发送/恢复 hooks 现有测试中覆盖恢复竞态与幂等 | Wi-Fi/蜂窝切换、后台、页面回收 |
| PR 4 | 审批防重复、恢复卡片、组合输入 | iOS/Android 键盘及读屏 |
| PR 5 | CORS 精确匹配、预检、非法配置 | HTTPS、SSE 实时转发、认证过期 |
| PR 6 | transport、runtime、pane 的隐藏恢复与输入顺序 | 终端回进、Ctrl、旋转、断线 |

前端变更按影响范围运行定向测试，并执行现有检查：

```powershell
pnpm --filter @qunica/frontend test src/lib/api-v2/sse.test.ts src/hooks/useSendMessageStream.test.tsx src/hooks/useResumeStream.test.tsx
pnpm type-check
pnpm lint
pnpm build
```

上述测试列表用于 PR 3；其他 PR 换成对应受影响测试。涉及后端静态服务时运行：

```powershell
cargo test --manifest-path backend-rs/Cargo.toml -p qunica-backend --test web_assets
```

后端 CORS/回放改动执行对应现有及新增定向测试；不因纯前端布局改动重跑无关后端套件。

发布至少覆盖 iPhone Safari 与已安装 PWA、Android Chrome 与已安装 PWA，以及桌面浏览器/Tauri 主窗口和辅助窗口。记录实际系统和浏览器版本，覆盖约 360/390 CSS px、横屏、1023/1024px 边界与桌面宽屏。

## 5. 里程碑与完成定义

- **M1：手机核心闭环。** PR 1–4 完成，PR 5 中实际部署所需项完成。手机可登录、切换会话、发送、恢复状态、审批与回答；工作区与 Git 可访问；桌面无回退。
- **M2：手机全屏终端。** PR 6 完成，明确区分视图隐藏、连接中断、页面重载与 PTY 关闭。
- **M3：原生与通知。** 仅按 PR 7 的明确需求推进，不阻塞 M1/M2。

每个 PR 交付小范围代码、必要检查结果及仍未验证的边界；不把“代码已合并”视为真机验收完成。不得为了坚持零后端改动而静默重复执行任务或丢失状态。

## 6. 浏览器约束参考

- 安全区与 `viewport-fit=cover` 的实现参考 [WebKit：Designing Websites for iPhone X](https://webkit.org/blog/7929/designing-websites-for-iphone-x/)，具体值以目标设备实测为准。
- 软键盘可能只改变 Visual Viewport，参考 [Chrome：Viewport resize behavior](https://developer.chrome.com/blog/viewport-resize-behavior)。
- SSE 库内部的事件 ID 与可见性处理参考 [fetch-event-source 源码](https://github.com/Azure/fetch-event-source/blob/main/src/fetch.ts)；落地时以仓库锁定版本为准。

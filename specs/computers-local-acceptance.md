# 受控电脑本地验收记录

日期：2026-10-19。范围：M2a/M2b 的代码、协议与真实 Linux Docker 执行路径。
目标合同见 [受控电脑 Spec](controlled-computers.md)，部署及限制见
[实现指南](../backend-rs/crates/backend/src/docs/guide/computers.md)。

## 环境与范围

Windows 宿主、Docker Desktop Linux Engine 29.1.3，独立临时 SQLite、Supervisor
日志目录与电脑卷。执行镜像为 Node 24 Bookworm、Chromium、Playwright MCP
0.0.83、OpenCode 1.19.3；客户端 noVNC 固定 1.5.0。模型请求使用本地 OpenAI
流式协议 fixture，没有真实厂商密钥或付费推理。测试创建的电脑、卷、网络及备份
目录由 fixture 清理，不操作其他项目的容器。

## 已观察结果

- 生产文件/Shell 路径：Write → Bash → Read 全部指向电脑 home；宿主工作区没有
  该产物。跨 owner、绑定、generation、活动回合以及另一线程驱动均受校验。
- Supervisor：鉴权、请求边界、持久 journal、重启 fencing、取消后保留动作锁、
  超时后的明确停止、接管等待及旧凭据失效均有状态机回归。
- 真实 Chromium：访问 fixture 站点，读取页面引用，点击下载；独立读取文件字节
  确认为 `verified browser artifact`。过期页面引用被拒绝，桌面 JPEG 与 RFB
  通道成功建立，人工控制释放后必须明确交还。
- 一致性备份：经 Supervisor 确认停止后导出 home，验证卷标签和归属，再恢复到
  同一绑定；重新启动后工作文件与持久 Cookie `identity=fixture` 均保留。
- 真实 OpenCode ACP：通过每电脑出口和后端代理完成协议推理；其自带 Bash 工具
  在目标容器写出 `acp-produced.txt`，独立读取确认字节，宿主没有该文件。第二次
  派发恢复相同 native session ID；取消后进程终止且授权被撤销。
- 分支任务：新建时不依赖或修改宿主工作区；派发在电脑内创建或核实 worktree，
  内置文件/Shell 与 ACP 使用同一任务 cwd。缺仓库或分支不匹配时拒绝执行。
- 模型代理：文本、工具参数、usage 和 DONE 保留；同一请求不再次提交，实际用量
  每调用只记账一次。错误模型、任意 URL 字段、预算超支、过期/撤销、错误网关与
  旧 generation 拒绝。半开流在取消后结束，无隐式重发。
- 群笔记桥：模型与笔记使用分开的授权，调用复用现有群笔记实现；移除成员或
  取消 job 后失效。长期账号令牌、Supervisor 令牌和厂商密钥不进入电脑。
  内置 Runtime 的专用笔记工具仍由后端执行，不新增电脑挂载或普通文件访问。
- 网络边界：公网地址判断、混合 DNS、保留/私有地址、云元数据和显式拒绝网段
  有离线测试；Docker 子网即使在内部站点 allow 范围内仍优先拒绝。桥接路由
  只接受可信网关添加的电脑/generation 证明，不是任意后端 HTTP 转发。
- 客户端：只读桌面、独占接管、释放后显式恢复、换账号不携带旧控制凭据有
  页面回归。屏幕独立传输，不写入账号事件；未确认输入不会跨重连重放。

## 执行记录

- 后端 `cargo test -j 2 --workspace` 全量通过；Docker 用例在默认运行中为 ignored，
  以下启用命令分别运行，不以 ignored 代表验收成功。
- 末次电脑与任务接口 13 项、运行流 131 项通过，包含容器分支任务、取消和恢复；
  后端群笔记兼容修正后的真实 Docker 生产 Runtime 用例复核通过，文件/Shell
  产物只在容器，专用群笔记仍可读，模型提示不含宿主工作区路径。
- 前端 `pnpm --filter @qunica/frontend test -- --maxWorkers=2`：132 个测试文件、
  929 项测试通过。初次并发运行中的两项超时，单独运行与降低并发后的全量均通过。
- TypeScript、ESLint（0 error，14 条既有 Fast Refresh warning）、生产构建通过。
- 严格 Clippy、Rust 格式检查、桌面 `cargo check --locked` 通过。
- 出口规则与备份归档边界：Python 7 项离线测试通过；Docker Compose 配置校验通过。

```sh
cargo test --manifest-path backend-rs/Cargo.toml -j 2 -p qunica-computer-supervisor --test control_plane --test desktop -- --include-ignored
cargo test --manifest-path backend-rs/Cargo.toml -j 2 -p qunica-backend --test computers -- --include-ignored
cargo test --manifest-path backend-rs/Cargo.toml -j 2 -p qunica-backend --test group_stream docker_bound_runtime -- --ignored
python scripts/test-computer-egress.py
python scripts/test-computer-home.py
```

## 外部验收边界

真实手机/桌面/Web 三端与完整 Noise 联测、VPS HTTPS/TLS/域名、真实模型能力评测、
弱网与负载基线仍是跨阶段验收项目。本地 fixture 不证明这些项目通过。M2 的开发
路径已接入，整体 Spec 仍保留 proposed，直到相应外部验收有独立证据。

# 多端同步本地验收记录

日期：2026-10-18。范围：M1b 本地协议、生产构建和浏览器交互。
目标合同见 [多端同步 Spec](realtime-sync.md) 与 [实时同步协议](../backend-rs/crates/backend/src/docs/guide/realtime.md)。

## 环境

Windows 后端、独立临时 SQLite 数据库和工作目录；生产 Web 构建由后端同源提供。
Chrome 的两个隔离 context 使用同一账号、两个独立设备 session，连接 loopback HTTP/WS。
验证没有配置真实模型服务，空群任务的预期结果为 `silence`，不代表模型能力评测。

## 已观察结果

| 操作 | 独立观察 |
| --- | --- |
| A 新建群，B 停在主页 | B 未打开群便显示新群；收到账号 WS 的 `events` 并 ack。 |
| A 通过界面发送消息 | 主 WS 返回收据并订阅指定 run，用户消息只出现一次，B 打开后得到相同消息和调度终态。 |
| B 连接 A 的服务器终端 | B 先只读观察，同一输出游标的命令结果在两端可见。 |
| A 释放输入，B 请求输入 | B 取得输入，A 保持只读并看到 B 的实际 Shell 输出。 |
| B 重新加载客户端 | 保存的两个 PTY id 保持不变，重连后取得原会话输出；没有隐式创建替代 Shell。 |
| B 删除消息 | 删除后没有消息操作按钮，重开页面不会恢复该行。 |
| B 撤销 A 的设备会话 | A 的现有连接退出并显示撤销提示；B 保持登录和终端连接。 |
| 修复 WS 传输字段后重新发送 | 生产构建正确接收调度事件，控制台没有 malformed scheduler event。 |

浏览器实测暴露并修复了 WS 外层字段透传、终端刷新按钮翻译路径及远程入口的本机措辞。
终端会话列表和标签提示展示短 session id，以区分同目录下的多个 Shell。

## 自动检查

- 后端全量：`cargo test --all-targets`，944 项通过。
- 加入 Supervisor 与自动触发算法后的最终工作区全量：965 项通过；需真实 Docker 的独立用例默认排除，复验限制见 [受控电脑](../backend-rs/crates/backend/src/docs/guide/computers.md#verification)。
- 2026-10-18 后续 M3、电脑 Runtime 与 Docker 复验：工作区全量 983 项通过、0 失败、2 ignored；受控电脑 opt-in 容器 fixture、Docker Runtime 绑定回归与 Linux Supervisor HTTP 冒烟另行运行通过，结果见 [自动触发验收](automations-local-acceptance.md) 与 [受控电脑实现边界](../backend-rs/crates/backend/src/docs/guide/computers.md#verification)。
- 前端最终全量：`npm test -- --maxWorkers=2`，909 项通过。
- 后续边界修正：配对/SSE 15 项、WS 领域事件/调度/终端 87 项、PTY 恢复 33 项、登录/引导文案 5 项通过。
- Web 和 Android 前端生产构建、TypeScript、桌面 `cargo check --locked --offline`、后端 Clippy（拒绝 warning）与格式检查通过。
- ARM64 调试 APK 完成 Tauri/Rust/Gradle 构建；Noise 协议 11 项测试通过。APK 在本地 `android/artifacts/qunica-0.1.5-realtime-arm64-debug.apk`，SHA-256 为 `202ad7247427f676c008f820ea7a41261c7b962d5b2a35c67f494dc8faa54d8a`。
- ESLint 无 error；保留既有 Fast Refresh 提示。生产构建仍提示部分 chunk 超过 500 KiB。

## 尚未运行

真实 Android/桌面/Web 三端联测、VPS 的 HTTPS/WSS 域名与 IP 证书、完整 Noise 真机配对、弱网及规定负载的性能基线未运行。
同机浏览器 context 不等同于三台物理设备；loopback 不验证公网 TLS。以上项目仍是 P0 验收门槛。

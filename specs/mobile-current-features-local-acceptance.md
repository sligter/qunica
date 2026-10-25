# 手机连接与 Android 更新本地验收

日期：2026-10-25。范围：同步近期电脑交付物、会话面板、远程实例与独立任务能力。使用边界见 [Android 连接指南](../android/CONNECT.md)。

## 已验证的行为

- 桌面远程模式展示当前 HTTPS 服务器二维码，不调用内置服务的配对命令；二维码只含服务器地址。
- Android 扫码识别服务器 origin 与桌面配对链接。重复连接同一规范化服务器地址保留登录；带账号密码的服务器地址在切换前被拒绝。
- 电脑交付物通过 Android 系统保存选择器导出。响应流按不超过 64 KiB 的有序块写入原生缓存，不创建整文件 Blob；字节与偏移正确，长度变化、桥接失败及中断不弹出保存选择器，临时缓存得到释放。
- 下载按钮等待系统保存结果，防止重复传输；账号变化会中断下载，取消系统保存属于正常退出。
- 生产代码的独立浏览器 fixture 覆盖 320、393、768、1280px，手机四个标签高度为 48px，完整位于 64px 容器内；连接页与标签页没有横向溢出。截图完成目视复核。fixture 不连接真实模型、服务器或相机。

## 检查与产物

- 针对性前端回归：7 文件、30 项通过。
- 完整前端回归：`pnpm --dir frontend exec vitest run --exclude '**/src-tauri/**' --maxWorkers=2`，137 文件、973 项通过。排除项为原生工程及其生成文件；包含全部 136 个 `src` 测试文件和根目录 PWA 测试。
- TypeScript、Web 生产构建与 Android 生产资源构建通过；ESLint 0 error，保留既有 14 项 Fast Refresh warning。
- ARM64 和 x86_64 调试 APK 的 Rust/Tauri/Gradle 构建通过；Windows 使用项目既有的符号链接失败复制回退。
- 两个 APK 的签名与 `zipalign -c -P 16 4` 检查通过；APK 中 ABI 正确，原生库与构建暂存库哈希一致。Tauri 的页面资源嵌入原生库，不作为 APK 的独立 JavaScript 文件保存。

本地产物位于忽略的 `android/artifacts/`：

| 文件 | SHA-256 |
| --- | --- |
| `qunica-0.1.5-mobile-update-arm64-debug.apk` | `e37fb8a7749e63f52164ea2f1a9ad418fccaaa3a6e6a71d78786a50a2ae804f1` |
| `qunica-0.1.5-mobile-update-x86_64-debug.apk` | `e9388027943fb080d54eaa3f647edbeb7ac8945c750770287dad70c43ec20f3c` |

## 验收边界

真实相机扫码、APK 安装后的系统保存选择器、前后台切换、弱网、公网 HTTPS/WSS、实际 1000 MiB 文件与物理多端联测未运行。1000 MiB 的现有上限通过 UI 边界回归；流式字节完整性以较小的本地 fixture 验证。两个 APK 使用调试签名，不是正式签名发布包。

## 顶部布局复验

2026-10-25 根据手机截图反馈，连接详情和切换入口移入导航侧栏；主聊天提供导航、会话名称、助手及更多操作的单一标题栏，群任务控制独占第二行。助手面板保留现有路由返回关闭及入口焦点恢复。非聊天页面和会话加载/失败时使用公共导航栏。

生产模式 Android 前端的实际 AppLayout、AndroidShell、群聊和助手组合 fixture 在 320、393、768px 下观察到：标题栏为 48px，任务栏为 45px，顶部总高 93px（不含系统状态栏）；无额外品牌或连接栏，无横向溢出。三个可见标题栏入口均为 44px，使用正常文档流，矩形不相交。助手开启、关闭与焦点恢复通过，导航中可查看连接地址并进入切换页。三种宽度的聊天、导航和助手截图完成目视复核。数据与原生会话恢复由 fixture 提供，未运行物理设备联测。

补充回归覆盖聊天标题注册/销毁、主页与聊天切换、唯一导航入口、返回后的焦点，以及安全会话不再添加常驻连接栏；末次针对性回归 4 文件、60 项通过。最终完整前端回归 137 文件、977 项通过；TypeScript 与 ESLint 检查通过（保留既有 14 项 Fast Refresh warning）。Web 与两个架构 APK 重新构建，签名和 16 KiB ZIP 对齐检查通过。新版调试包及校验和在忽略的 `android/artifacts/`：

| 文件 | SHA-256 |
| --- | --- |
| `qunica-0.1.5-mobile-layout-arm64-debug.apk` | `0d632ec3e89c7458cf810acd47cfd9817b8e1968bba4c5acb8ddf90117b5cb41` |
| `qunica-0.1.5-mobile-layout-x86_64-debug.apk` | `524a7084736d4d13aaa3cde01f0042132d533cc4359255a7fcd1d16a56bca578` |

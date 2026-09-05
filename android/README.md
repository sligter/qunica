# Qunica Android

Android 原生安装包，使用独立 Tauri 2 壳加载仓库内打包的 React 界面。手机通过 HTTPS 连接 Qunica Server；不启动本地后端、ACP、Agent CLI 或 PTY。

本轮安装包：

- [ARM64 手机 APK，约 29.3 MB](artifacts/qunica-0.1.1-android-arm64-debug.apk)
- [x86_64 模拟器 APK，约 29.6 MB](artifacts/qunica-0.1.1-android-x86_64-debug.apk)
- [SHA-256 校验文件](artifacts/SHA256SUMS.txt)

二进制产物保存在本地忽略目录，不纳入源码版本控制。

## 使用

1. 安装 ARM64 APK（普通 Android 手机），或 x86_64 APK（对应架构的模拟器）。当前开发构建使用 Android 调试签名，不是应用商店发行包。
2. 首次打开填写服务器根地址，例如 `https://qunica.example.com`。不支持 HTTP、URL 中的用户名/密码或子路径部署。证书必须被 Android 系统信任，不绕过证书检查。
3. 连接检查通过后，使用服务器已有账户登录。聊天、审批、工作区和终端都在服务器执行。
4. 需要切换服务器时，先退出登录，在登录页顶部选择“切换服务器”。切换会清空旧登录身份，不把旧 Token 发给新服务器。

服务端部署要求见根目录 `DOCKER.md`。本地界面的 Origin 为 `https://tauri.localhost`，当前服务端默认允许该来源；如果代理另外限制了 CORS，也需要允许它及 Authorization、Content-Type、last-event-id 请求头。应部署同一仓库版本的后端，以使用 mobile-connect 的 GET SSE 回放入口。

连接错误会留在设置页，可修改地址后重试。应用没有后台常驻服务；回到前台时复用已有消息/终端恢复流程。关闭全屏终端仅隐藏视图；强制关闭或重新加载应用后的旧 PTY 重新附着不在首版保证内。

## 构建与 Android Studio

在仓库根目录运行：

```powershell
pnpm install
rustup target add aarch64-linux-android x86_64-linux-android
pnpm android:build
# x86_64 模拟器版本
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/android.ps1 build -Target x86_64
pnpm android:open
```

Android Studio 工程位于 `android/src-tauri/gen/android`，原生源文件已经纳入仓库。不要再次运行 `tauri android init` 覆盖原生配置。通过脚本构建后，可在 Android Studio 导入工程、查看 Kotlin 源码并附加调试；标准 Tauri 原生开发循环需要 CLI 与 IDE 配合运行。

脚本优先使用 `JAVA_HOME`、`ANDROID_HOME` / `ANDROID_SDK_ROOT`、`NDK_HOME`，未设置时查找已安装的 Android Studio 和 SDK。它只修改本进程环境，不修改系统设置；会沿用已启用的 Windows HTTP 代理。Rust Android targets 需提前安装。首次构建需要访问 crates.io、Google Maven、Maven Central 与 Gradle 分发服务。

在禁止符号链接的 Windows 环境，脚本在 Cargo 编译成功后复制生成的 `.so`，再调用 Gradle 打包；不要求修改 Windows 开发者模式。若 `.qunica/gradle-8.14.3` 存在则使用该本地分发，否则使用 Gradle Wrapper。

复制打包时去掉原生调试符号，完整符号仍在 Cargo `target` 目录。Gradle 的增量 APK 可能保留替换旧大文件后的空洞；需要精简交付包时，删除对应 `build/outputs/apk` 下的旧 APK 后重新打包。本轮交付包已重新生成。

默认生成调试签名 APK：

- 手机：`android/src-tauri/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk`
- 模拟器：`android/src-tauri/gen/android/app/build/outputs/apk/x86_64/debug/app-x86_64-debug.apk`

`-Release` 构建不配置发行签名。应用商店发布前需自行设置正式签名、版本号与分发流程，不应分发调试密钥作为正式密钥。

## 实现边界

- `android/src-tauri/Cargo.toml` 仅依赖 Tauri 和 JSON 支持，与桌面 crate 和 backend crate 隔离。
- Android 构建使用 Vite `android` mode。运行时不走桌面 localhost API、本地文件选择器、桌面升级器或本地终端。
- Android Token 不写入 localStorage。服务器和 Token 作为一个记录，使用 Android Keystore 中的 AES-256 密钥、GCM 随机 IV 加密，密文保存在应用私有 SharedPreferences；禁用备份和设备迁移。
- 登录写入安全存储成功后才建立内存登录状态。退出立即清除内存状态，并按顺序持久化 Token 清除；写入失败显示重试提示，不能当作服务端 JWT 已撤销。
- 仅本地打包界面可以访问原生桥；远程网页导航被拦截。未开放 shell、桌面命令或任意原生文件系统权限。
- Android 的系统栏、刘海和 IME 在原生窗口中处理；返回键由 Tauri 的 WebView 历史处理，沿用 Sheet 的 history 状态。
- 未加入推送、扫码配对、生物认证、原生分享/文件下载、逐设备会话撤销或 iOS 工程。这些需要独立实现和验收。

## 检查

```powershell
pnpm --filter @qunica/frontend test src/lib/androidSession.test.ts src/components/auth/AuthCard.test.tsx src/lib/authFetch.test.ts
pnpm type-check
pnpm lint
```

`SessionVaultTest` 是设备侧测试，覆盖实际 Keystore 加密读写、退出后的 Token 清除和密文篡改拒绝。在已构建 x86_64 原生库、启动模拟器后，可运行 Gradle 的 `:app:connectedX86_64DebugAndroidTest`，并用 `-x :app:rustBuildX86_64Debug` 复用刚构建的原生库。

本轮实际验证：

- ARM64 与 x86_64 调试 APK 构建成功；签名验证、16 KB ZIP 对齐检查通过。
- 前端 114 个测试文件、798 项测试通过（`vitest run --maxWorkers=4`）；类型检查通过；lint 0 错误、14 条警告。首次高并发运行中的两项页面超时，在定向重跑和最终全量检查中均通过。
- Pixel_10 x86_64 模拟器（Android 17 预览系统、16 KB 内存页）安装和启动成功；两项 Keystore 设备侧测试通过。
- 人工检查连接页、安全区、输入框键盘避让、返回键收起键盘、HTTP 地址拒绝提示；ARM64 未在实体手机安装验证。
- 模拟器运行中曾出现 System UI 无响应和 WebView renderer 被系统终止的日志；重新启动应用后页面恢复，原因尚未定位。不能将本轮冒烟视作完整稳定性验收。
- 未提供真实服务器地址，尚未执行 Android 到实际 Qunica Server 的登录、聊天、审批和终端端到端联调。

原生实现参考 [Tauri 移动插件文档](https://v2.tauri.app/develop/plugins/develop-mobile/) 与 [Android Keystore 参数文档](https://developer.android.com/reference/android/security/keystore/KeyGenParameterSpec)。

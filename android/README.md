# Qunica Android

独立 Tauri 2 Android 应用，使用本地打包的 React 界面。通过局域网扫码连接桌面，消息、审批和终端数据均经过 Noise 端到端加密，无需证书或额外 VPN。手机不运行本地后端、ACP、Agent CLI 或 PTY。

## 使用

手机 UI 更新安装包与验收说明见 [MOBILE-UI.md](MOBILE-UI.md)。此次更新覆盖聊天、资源库和设置等页面，可直接覆盖安装。

文件下载修复版：[ARM64 APK](artifacts/qunica-0.1.1-download-arm64-debug.apk)。覆盖安装后，在文件列表或预览中点击下载，等待系统“保存到”窗口，选择 Downloads（下载）或其他目录并确认保存。文件由已认证的连接读取，支持局域网加密连接；不需要额外存储权限。取消保存不会留下临时下载。

1. 更新桌面版，在「设置 → 系统设置 → 手机连接」选择局域网网卡，开启并生成二维码。
2. 手机与电脑连接同一 Wi-Fi，安装 ARM64 APK，在应用中选择「扫描桌面二维码」。也可粘贴配对链接。
3. 配对码两分钟内有效且只能使用一次。配对成功后用已有工作台账户登录。
4. 更换电脑或电脑 IP 变化时，退出登录后选择「重新配对」。桌面设备列表可撤销旧设备。

局域网代理规则需直连，Wi-Fi 必须允许设备互访。连接地址不是普通 HTTP 网页。手机不需要配置服务器域名、证书或 CORS。

完整协议、限制和验收步骤见 [MOBILE-CONNECT.md](../MOBILE-CONNECT.md)。已保存的旧 HTTPS 服务器会话仍可读取，首次设置默认使用扫码。

## 构建

```powershell
pnpm install
rustup target add aarch64-linux-android x86_64-linux-android
pnpm android:build
# 模拟器版本
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/android.ps1 build -Target x86_64
pnpm android:open
```

Android Studio 工程在 `android/src-tauri/gen/android`。不要重新运行 `tauri android init` 覆盖原生配置。脚本查找本机 Android Studio、SDK 和 NDK；支持通过 `JAVA_HOME`、`ANDROID_HOME` / `ANDROID_SDK_ROOT`、`NDK_HOME` 指定路径。首次构建需下载 Rust / Google Maven / Gradle 依赖。

Windows 禁止符号链接时，脚本复制 Cargo 已编译的原生库并去掉调试符号，再调用 Gradle 打包，不要求开启开发者模式。存在 `.qunica/gradle-8.14.3` 时优先使用该分发。脚本不会修改系统环境设置。

默认输出为调试签名 APK：

- ARM64：`src-tauri/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk`
- x86_64：`src-tauri/gen/android/app/build/outputs/apk/x86_64/debug/app-x86_64-debug.apk`

`artifacts/` 是被 Git 忽略的本机交付目录。旧版 0.1.1 APK 不包含扫码功能，必须使用本次重新构建的包。商店发行仍需正式签名和版本管理。

## 原生边界

- 依赖独立 `mobile-link` 客户端及扫码插件，不包含 backend、portable-pty、Agent CLI 或桌面 shell 模块。
- UI 与密钥交换代码随 APK 分发，远程网页不能访问原生桥。Android `usesCleartextTraffic=false` 保留；无证书通道使用原生 Noise TCP，不通过 WebView 的明文 HTTP。
- 公钥、设备凭据和账户 Token 保存在 Android Keystore AES-GCM 加密记录中，禁用备份；不回退到 localStorage。
- 账户退出与解除设备配对是两个操作。退出立即清空账户状态，桌面撤销设备会中断该设备所有加密请求。
- 原生请求支持分块背压和取消，聊天 / 终端恢复复用现有状态同步及 SSE 游标。
- 文件下载通过 64 KiB 分块传给原生层，暂存在应用私有缓存；由 Android Storage Access Framework 创建用户选择的目标文件，完成或取消后清理缓存。下载和保存失败会传回界面。
- 未加入公网中继、后台常驻、系统推送或 iOS。

## 验证

```powershell
cargo test --manifest-path mobile-link/Cargo.toml --features server
pnpm --filter @qunica/frontend exec vitest run --maxWorkers=4
pnpm type-check
pnpm lint
```

Android `SessionVaultTest` 覆盖实际 Keystore 加密读写与密文篡改拒绝。实体手机仍需验证相机扫码、同网连接、登录、聊天、审批、终端和断线恢复；模拟器结果不能代替真机验收。

下载专项：`DownloadStagingTest` 覆盖二进制分块、完整性校验、空文件、清理与路径隔离；`FileExportDeviceTest` 在模拟器中通过真实 JS/Rust/Kotlin 桥打开系统保存窗口，并校验保存内容和取消行为。

设备测试需要已启动的模拟器或真机，`.so` 已在 `src/main/jniLibs` 时跳过 Gradle 的 Rust 任务：

```powershell
cd android/src-tauri/gen/android
$skip = '-x','rustBuildUniversalDebug','-x','rustBuildArm64Debug','-x','rustBuildArmDebug','-x','rustBuildX86_64Debug','-x','rustBuildX86Debug'
./gradlew :app:testUniversalDebugUnitTest @skip
./gradlew :app:connectedUniversalDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=app.qunica.mobile.SessionVaultTest @skip
./gradlew :app:connectedUniversalDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=app.qunica.mobile.FileExportDeviceTest @skip
```

两个设备测试类必须分开调用。`FileExportDeviceTest` 会启动 Tauri Activity，而销毁该 Activity 会带走整个应用进程，同一次运行里的其他测试类会在上报前被杀掉。该测试因此不关闭 `ActivityScenario`，由 instrumentation 在运行结束时回收。Activity 销毁引发的原生崩溃（`FORTIFY: pthread_mutex_lock called on a destroyed mutex`）来自 Tauri/wry 的销毁路径，与下载逻辑无关；进程本就在退出，暂存缓存会在下次启动时清理。

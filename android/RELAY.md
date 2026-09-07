# VPS 远程连接（无需证书）

手机可通过自己的 VPS 连接家中电脑，不要求同一 Wi-Fi，也不要求手机与电脑使用同一个代理节点。继续使用 v2rayNG 即可，只要手机能访问 VPS 的 TCP 端口。

本次本机构建：[Windows 便携版](../frontend/src-tauri/target/release/bundle/portable/Qunica_0.1.1_relay_x64-portable.exe)、[ARM64 调试 APK](artifacts/qunica-0.1.1-relay-arm64-debug.apk)。APK 可覆盖安装；桌面需退出旧版再启动新版。本地交付目录未纳入 Git。

```text
Android APK ── Noise 加密 TCP ── VPS:18766 / frps
                                       │ 电脑主动建立的 frp 隧道
                                 电脑 frpc → 127.0.0.1:8766 → Qunica
```

电脑始终持有 Noise 私钥；二维码固定电脑公钥，一次性配对码有效期两分钟。VPS 仅转发密文，无法读取账号 Token、聊天或文件；它仍能看到连接来源、时序与流量大小。监听地址与手机访问地址分开，中继模式不开放电脑的局域网端口。既有 HTTP/SSE 业务和恢复逻辑不变。

本方案关闭 frp 外层 TLS，完全不使用 TLS 证书；认证加密由既有 Noise 通道提供。生成的配置**只用于 8766 的 Noise 入口，不能改为转发明文 API 8765 或其他未加密服务**。frp 控制面 Token 是独立随机值，不是 Qunica 登录 Token；即使中继被冒充，手机仍须通过原电脑公钥验证。frp 的 TLS 默认行为见[官方说明](https://gofrp.org/en/docs/features/common/network/network-tls/)。

## 1. 生成配置

在仓库根目录运行，替换 VPS 地址；IP 也可，无需域名。默认使用两个独立 TCP 端口：7000 为电脑到 VPS 控制通道，18766 为手机入口。若现有代理已占用，请换空闲端口。

```powershell
./scripts/configure-mobile-relay.ps1 -VpsHost YOUR_VPS_IP -ControlPort 7000 -PhonePort 18766
```

生成 `.qunica/relay/frpc.toml` 和 `frps.toml`，共享随机 Token，不打印 Token，也不覆盖已有配置。目录被 Git 忽略。将 `frps.toml` 通过自己的 SSH/SCP 连接传到 VPS，`frpc.toml` 留在电脑。

两端安装同一版本的 [frp 官方发布包](https://github.com/fatedier/frp/releases)（支持 TOML 的 0.52 或更新版）。下载匹配 VPS 系统/CPU 的服务端和 Windows amd64 客户端，并按发布说明校验下载。配置语法和 verify 命令见[官方文档](https://gofrp.org/en/docs/features/common/configure/)。

## 2. VPS 启动 frps

在存放 frps 与配置的目录执行：

```sh
chmod 600 frps.toml
./frps verify -c frps.toml
./frps -c frps.toml
```

在云安全组和 VPS 系统防火墙允许所选两个 TCP 端口。这里没有 443/HTTPS 监听，也不修改现有 v2ray 服务。默认不启用 frp 管理面板；配置仅允许一个指定手机端口，参见[frp 服务端配置](https://gofrp.org/en/docs/reference/server-configures/)。验证跑通后，可按[frp systemd 文档](https://gofrp.org/en/docs/setup/systemd/)将 frps 设为后台服务。

## 3. 电脑启动转发和 Qunica

在仓库根目录，将 Windows frpc 放在 `.qunica/relay/frpc.exe`：

```powershell
./.qunica/relay/frpc.exe verify -c .qunica/relay/frpc.toml
./.qunica/relay/frpc.exe -c .qunica/relay/frpc.toml
```

启动新版桌面端，在「设置 → 系统设置 → 手机连接」选择 **VPS 中继**，输入 `VPS地址:18766`，点击「开启并生成二维码」。界面同时显示手机访问地址和本机监听 `127.0.0.1:8766`。

二维码生成仅代表本地监听成功，并不代表 VPS 已部署或外网已通。frpc 应显示登录、代理启动成功；再用手机完成配对确认整条路径。电脑、Qunica 和 frpc 必须保持运行，电脑不能睡眠。桌面重启后需再次开启手机连接；VPS 地址在本机保留为下次输入默认值。

## 4. 手机配对与地址变更

- 覆盖安装本次中继版 APK。旧 APK 仅接受局域网地址，不能扫描版本 2 的中继二维码。
- 扫码，或者在手机粘贴两分钟内有效的配对链接。VPS 地址是原生 TCP 地址，不是可打开的网页，不加 `http://` 或 `https://`。
- 配对后用原 Qunica 账号登录。中继模式不会绕过账号认证。
- 已配对同一电脑：退出账号后点「重新配对」，在下方「更换已配对电脑的连接地址」填新地址。应用保留电脑公钥与设备凭据，验证新地址上的电脑身份后保存，不必再扫码。
- 改回 LAN 时，先在桌面关闭中继连接并切换到局域网模式，再在手机换成对应 LAN 地址。切换会中断旧连接；待发送消息状态不确定时先刷新确认，避免重复提交。
- 电脑可撤销设备，撤销后现有加密流会中断；同一个配对身份通过 LAN 或 VPS 访问均受撤销限制。

Android 原生 TCP 在 v2rayNG 的 VPN/TUN 路由范围内时按其分流规则走；如果只启用本地 SOCKS 而没有系统路由，Qunica 不会自动读取该 SOCKS 设置。VPS IP 可以设为直连；它若是你的代理节点，注意避免路由回环。现有 v2ray 节点本身不会自动转发回家中电脑，仍需 frps/frpc。

## 验收与定位

手机关闭 Wi-Fi，使用移动数据测试：配对/登录、流式聊天、审批、文件下载到系统目录、终端输出、锁屏后恢复。用电脑设备撤销按钮确认连接失效。端口连通只证明 TCP 能建立，不能替代 Noise 身份和端到端验收。

连接失败时依次检查：VPS 两个端口与 frps、电脑 frpc 是否注册成功、桌面是否启用了 **VPS 中继**、二维码地址端口是否正确、手机路由规则。返回“无法验证原电脑”时保留旧地址，不更换公钥来跳过验证。

本次验证（2026-09-07）：

- 前端 119 个文件、823 项测试通过；类型检查通过；lint 0 错误、14 条既有警告。
- mobile-link 9 项测试通过；另用真实 frp 0.71.0 在本机隔离运行 frps/frpc，完成配对、一次性码重用拒绝、错误公钥拒绝、100003 字节二进制往返、SSE 和撤销验证。
- 配置生成脚本产物通过 frpc/frps 原生 verify；没有自动安装服务或修改防火墙。
- 桌面发布构建、Android ARM64 构建通过；APK 签名及 16 KB 对齐通过，签名身份沿用原 Android Debug 密钥。
- Chrome 中桌面中继选择/二维码、390px 手机地址表单检查通过，无横向溢出（原生桥使用模拟响应）。

真实 VPS 和实体手机移动数据联调需要实际服务器信息，尚未部署。上述本地代理测试不能替代外网验收。

可复用的隔离测试入口：`cargo run --manifest-path mobile-link/Cargo.toml --features server --example relay_smoke -- RELAY_HOST:PHONE_PORT`。需先配置 TCP 中继指向本机 8766，并关闭正式桌面的手机共享以释放该端口。测试使用临时身份及测试路由，不读真实工作区或账号。

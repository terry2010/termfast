# TermFast Android 实施计划

> 本文基于对当前仓库的调查，给出 Android 版 TermFast 的技术结论、实施方案和开发计划。

---

## 1. 背景与目标

TermFast 目前是一款桌面端 SSH 终端 + 代理 + 触发器工具（macOS / Windows / Linux）。目标是在当前代码基础上扩展 Android 版，分两期交付：

- **Phase 1**：在手机上实现 SSH 代理（VPN 方式）。
- **Phase 2**：在手机上实现 SSH 终端。

Android 版应复用现有 Rust 业务核心，避免重写 SSH、代理、触发器、配置等逻辑。

---

## 2. 调查结果

### 2.1 项目结构

当前代码采用 Rust 工作区 + Tauri 2 前端架构：

```text
termfast/
├── Cargo.toml
├── src/                         # React 19 + TypeScript + Tailwind 前端
├── src-tauri/                   # Tauri 2 应用入口
└── crates/
    ├── core/                    # 平台无关业务核心
    ├── credential/              # 凭据存储 trait
    ├── daemon/                  # IPC daemon + TerminalManager
    ├── desktop/                 # 桌面系统代理、托盘、通知
    ├── cli/                     # 命令行工具
    └── test-utils/
```

### 2.2 代码行数统计

| 模块 | 行数 | 备注 |
|---|---|---|
| `crates/core` | ~7.6k | SSH、代理、触发器、配置、日志 |
| `crates/daemon` | ~5.3k | IPC 协议、请求处理、TerminalManager |
| `crates/credential` | ~357 | 凭据存储 trait |
| `crates/desktop` | ~1.4k | 桌面系统代理、托盘、通知 |
| `src-tauri` | ~1.1k | Tauri 应用入口、IPC 桥 |
| `src` 前端 | ~9.7k | React 组件与业务逻辑 |

### 2.3 可复用分析

#### 2.3.1 `crates/core`（强烈推荐复用）

`crates/core/src/lib.rs` 明确说明：

> “Contains all business logic: config, SSH, proxy, trigger engine. Does NOT depend on tauri or daemon — keeps mobile cross-compilation possible.”

`crates/core` 包含以下可复用能力：

- `ssh::client`：`SshClientHandle` 连接、重连、认证、心跳、direct-tcpip 频道。
- `ssh::pty`：PTY 交互式终端打开和 resize。
- `proxy::socks5` / `proxy::http` / `proxy::mixed`：本地 SOCKS5 + HTTP 混合代理。
- `proxy::manager`：`ChannelManager` 管理 SSH 频道并发和统计。
- `server::manager` / `server::instance`：多服务器生命周期管理。
- `trigger::engine`：触发器执行引擎。
- `config`：配置管理、存储 trait、运行时状态。

这些模块不依赖桌面环境，理论上可交叉编译到 Android。

#### 2.3.2 `crates/daemon`（部分复用）

- `daemon/src/terminal.rs`（~506 行）：`TerminalManager` 管理 PTY 会话、输入、resize、输出 base64 转发，可直接复用。
- `daemon/src/handler.rs`（~2.5k 行）：请求处理逻辑映射，若能让 `DaemonState` 在 Android 编译，可复用。
- `daemon/src/server.rs`：`DaemonState` 可复用，但 `DaemonServer` 的 Unix socket 监听不需要。
- `daemon/src/lock.rs`、`frame.rs`、`proto.rs`：除 IPC 协议外，多数 Android 不需要。

#### 2.3.3 `crates/credential`（trait 可复用）

`CredentialStore` trait 简单，但 `keychain.rs` 依赖 `keyring` 桌面库，Android 需要新的 Keystore 实现。

#### 2.3.4 桌面/前端（基本不重写）

- `crates/desktop`：系统代理、托盘、窗口特效，完全不需要。
- `src-tauri`：桌面窗口事件、托盘、自动更新，需要重写或加 `#[cfg]` 隔离。
- `src` 前端：桌面三栏布局，需要重写为移动端 UI；但 store、hooks、类型、i18n 可部分复用。

### 2.4 现有跨平台问题

在让代码编译到 Android 之前，需要处理以下问题：

1. **`crates/daemon/Cargo.toml` 的 `mach2` 依赖**：
   - `mach2` 是 macOS 专用库，但写在了 `[dependencies]` 中，且只用于 `tests/proxy_benchmark.rs`。
   - 需要改为 `dev-dependencies` 或加 `#[cfg(target_os = "macos")]` 隔离，否则 Android 交叉编译失败。

2. **`crates/credential` 的 `keyring` 依赖与模块导出**：
   - `crates/credential/Cargo.toml` 已用 `cfg(target_os = ...)` 把 `keyring` 依赖隔离到 macos/windows/linux，Android 编译时不会拉入 `keyring`。
   - **但 `crates/credential/src/lib.rs` 的 `pub mod keychain;` 和 `pub use keychain::KeychainCredentialStore;` 未加 `cfg` 隔离**，Android 编译时仍会编译 `keychain.rs`，其内部引用 `keyring` 类型导致编译失败。
   - 修复：给 `lib.rs` 的 `keychain` 模块导出加 `#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]`，并提供 Android `KeystoreCredentialStore` 实现（放在 `crates/android-ffi` 中实现 `CredentialStore` trait）。

3. **`directories` 路径问题**：
   - `crates/core/src/config/storage.rs` 和 `crates/core/src/config/runtime_state.rs` 使用 `directories::ProjectDirs`/`BaseDirs` 获取默认路径。
   - Android 上这些调用可能返回 `None` 或无法写入，需要显式使用 app 私有目录。

4. **`crates/core/src/ssh/auth.rs` 的 key 路径**：
   - `generate_keypair()` 写死 `~/.ssh/termfast_<id>_key`，Android 没有 `~/.ssh`。
   - 需要支持传入 key 目录，或改为返回 key bytes 由 Android Keystore 存储。

5. **`crates/core/src/ssh/client.rs` 的 socket 保护**：
   - 当前 `connect()` 直接 `tokio::net::TcpStream::connect()`。
   - Android 上 VpnService 启动后，如果不 `protect` SSH socket，SSH 流量会被 TUN 回收形成回环。

---

## 3. 结论

### 3.1 架构结论

- **在当前项目内做 Android 版（monorepo）**，而不是另开项目。
- 原因：
  - `crates/core` 已被设计成平台无关，可以直接复用。
  - 单一仓库统一版本管理、CI、构建和发布。
  - `src-tauri` 已经支持 `#[cfg_attr(mobile, tauri::mobile_entry_point)]`，如果走 Tauri Mobile 路线必须同项目。
  - 跨仓库引用 `termfast-core` 会增加版本同步和 FFI 契约维护成本。

### 3.2 实施路线

推荐方案：

> **UI 路线决策必须在 Phase 0 第 1 周内做出**，因为它决定了 `crates/android-ffi` 的接口形态、Android 工程结构、CI 矩阵和排期，无法并行设计。两条路线差异巨大到不能推迟。
>
> **推荐：原生 Kotlin + Jetpack Compose**。理由：
> 1. VpnService / 前台 Service / Keystore / Tile / BootReceiver 全是原生 API，Tauri Mobile 反而要包一层 plugin，得不偿失。
> 2. 终端渲染在 WebView 里用 xterm.js 性能差（移动端软键盘 + VT100 渲染 + 大输出滚动），原生 terminal emulator 更可控。
> 3. 桌面三栏布局本来就要重写为移动端，React "复用"的实际收益主要是 store/hooks/类型/i18n，这部分可抽成独立 npm 包供两端引用，不一定要 Tauri Mobile。
>
> 若团队仍倾向 Tauri Mobile，需在 Phase 0 spike 中验证"Tauri 2 mobile + 前台 Service + Rust runtime 集成"是否可行，作为硬门槛。

1. **在当前仓库新增 `crates/android-ffi`**：
   - 依赖 `crates/core` + `crates/terminal`（见下文 TerminalManager 下沉）。
   - **不依赖 `crates/daemon`**——Android 同进程调用，不需要 IPC daemon；拉入 daemon 会带入 `mach2`/`libc`/`directories` 等无关依赖。
   - 对外暴露 JNI/C API：`add_server`、`connect`、`toggle_proxy`、`start_vpn`、`stop_vpn`、终端相关接口。
   - 内部实现 `tokio` runtime、Android config storage、Android credential store、socket protector、`tun2proxy` 启动。

2. **Android 原生层（Kotlin）**：
   - 位于 `android/`（原生 Gradle 项目）。
   - `SshVpnService`：继承 `VpnService`，建立 TUN、启动前台服务、通知。
   - `KeyStoreHelper`：Android Keystore 凭据存取。
   - `SharedPreferencesConfigStorage`：配置持久化。
   - `RustBridge`：JNI 加载与调用。
   - `VpnTileService`：下拉快捷开关。
   - **自启动走 Always-on VPN（主路径）**，不走 `BootReceiver`：Android 7.0+ 官方支持用户在系统设置中将本应用设为"始终开启的 VPN"（`VpnService.prepare()` 授权后，系统设置 → VPN → 点击应用 → "始终开启"）。开启后系统在开机、用户切换、网络变化时自动启动你的 `VpnService`，无需 `RECEIVE_BOOT_COMPLETED` 权限，不受 Play 自启政策限制。这是 Android 平台官方推荐的自启动方式。
   - `BootReceiver`（可选，降级）：仅在用户未启用 Always-on 但希望开机自启时作为兜底，注意 Google Play `RECEIVE_BOOT_COMPLETED` 政策（见 §6）。不作为主路径。

3. **前端**：
   - **原生 Kotlin + Jetpack Compose**（推荐）：重写全部 UI，但可复用桌面端的 store/hooks/类型/i18n 逻辑（抽成独立 npm 包或移植为 Kotlin）。终端渲染可参考 ConnectBot 的 **termlib**（Compose + libvterm，Canvas 渲染）或 MannanSaood/termi（Rust + Compose）。**⚠️ 授权注意：ConnectBot 是 GPL-3.0，若仅"参考思路"无妨，若借用代码需注意 GPL 传染性（与 Termux `TerminalView` 同理）。**
   - Tauri Mobile（备选，需 spike 验证）：复用 React，重写布局为移动端单页/底部 Tab。已有社区 plugin 可参考：`EasyTier/tauri-plugin-vpnservice`（VpnService 封装）、`tauri-plugin-background-service`（前台 Service 生命周期），但成熟度低，需评估。

4. **VPN 代理实现**：
   - 复用 `crates/core` 的 `Socks5Server`/`MixedProxyServer` 在本地 `127.0.0.1:1080` 监听。
   - **选型 `tun2proxy`（Rust，tun2proxy/tun2proxy v0.8.2+）**，1359 stars、MIT、活跃维护（月更）、60k+ 下载。理由：
     1. **纯 Rust，直接 crate 依赖**进 `crates/android-ffi`，与 tokio runtime 统一，无需引入额外的语言运行时或跨语言 FFI。
     2. 功能完整：IPv6、SOCKS5 UDP、DNS over TCP（原生）、UdpGW、fd 注入（有 Android 集成 Wiki）。
     3. 单一 backtrace、统一错误处理。
   - **关于 tun2proxy 的 "session info embedding"（SOCKS5 username 嵌入来源信息）**：该特性是给"上游 SOCKS5 服务器需要按来源 app 做计费/分流"的商业 VPN 后端用的。**本项目不会用到**——本项目的 per-app 代理由 `VpnService.Builder.addApplication/addDisallowedApplication` 在 TUN 层按 UID 过滤完成（见 §4.1.2），流量进入 TUN 时已被筛选；上游 SOCKS5 是自己的 `crates/core::Socks5Server` 再走 SSH direct-tcpip，后端不需要知道来源 app。选型不依赖该特性。
   - DNS 策略：tun2proxy 原生支持 DNS over TCP；在 TUN 内拦截 UDP 53，转 TCP DNS 上游（DoT/DoH 或 TCP 53），避免 UDP 转发丢包（见 §4.1.2 K5b）。

5. **Phase 2 终端**：
   - 复用 `crates/core/src/ssh/pty.rs` 和下沉后的 `crates/terminal/src/manager.rs`（原 `crates/daemon/src/terminal.rs`）。
   - 新增 FFI 接口：`terminal_open`、`terminal_input`、`terminal_resize`、`terminal_close`、`terminal_subscribe`。
   - UI 渲染：原生用自定义 terminal emulator view 或 Termux view（注意 GPL 授权）；Tauri 用 `xterm.js` + 移动端键盘。

### 3.3 重写 vs 新增

- **需要改动的现有代码**：约 1.0–1.8k 行（`mach2`/`keyring` 模块导出隔离、socket protect（socket2 改造，含 `PrefixedStream`/`connect_stream` 调用链透传，不止 `connect` 一行）、key path 注入、目录路径注入、Tauri 桌面 cfg 隔离、**TerminalManager 从 daemon 下沉到 core/terminal**）。其中：
  - socket2 改造实际工作量偏向上限（`connect_and_peek` 返回 `PrefixedStream`，整条 stream 管线要透传 protected socket 到 russh），按 1.2–1.5k 估算；
  - TerminalManager 下沉需引入 `TerminalEventSink` trait + daemon/android-ffi 双侧适配器解耦 `EventForwarder` 依赖（见 §4.2 第 7 点），约 +100–200 行，已并入上限。
- **需要新增的代码**：
  - Phase 1：约 6.8–10.5k 行（Rust FFI 1.5–2k，Android 原生服务 2–3k，移动端 UI 3–5k，构建/CI 0.3–0.5k）。
  - Phase 2：约 1–2k 行（终端 FFI 0.3–0.5k，终端 UI 0.8–1.5k）。
- **数据通路性能开销（已知风险，Phase 0 建基线）**：完整链路是 `TUN fd → tun2proxy（解析 IP/TCP）→ 127.0.0.1:1080 本地 SOCKS5（crates/core）→ SSH direct-tcpip channel`。每个数据包在 localhost 上多一次 SOCKS5 协议封装/解封装 + 一次 tokio task 切换，移动端 CPU/电量敏感场景预估增加 ~10–20% 吞吐开销和延迟。功能上没问题，但需在 Phase 0 spike 用 iperf3/curl 大文件下载对比"直连 SOCKS5 vs 经 TUN"吞吐，建立性能基线；若后续成为瓶颈，备选方案是写一个 `tun2ssh` 适配层让 tun2proxy 直接对接 russh（跳过本地 SOCKS5 一跳），属 Phase 2+ 优化，不在 Phase 1 范围。

### 3.4 技术选型

| 层级 | 选型 | 说明 |
|---|---|---|
| 核心后端 | `crates/core` + `crates/terminal`（复用 + 下沉） | 平台无关，SSH/代理/触发器/配置/终端管理 |
| 后端桥接 | `crates/android-ffi`（新建） | `jni` + `tokio` runtime + socket2 protect |
| 路由转发 | `tun2proxy`（Rust） | 纯 Rust crate 依赖，与项目工具链统一，支持 `tun_fd` 注入 |
| VpnService | Kotlin native | 必须原生前台服务，`foregroundServiceType=specialUse`（API 34+），需 manifest 声明 justification |
| 凭据 | Android Keystore | 通过 JNI 桥接；提供导出/导入（加密）功能 |
| 配置 | SharedPreferences 或 app 私有文件 | 桥接 `ConfigStorage` trait |
| UI | **原生 Kotlin + Compose（推荐）** / Tauri Mobile（备选） | Phase 0 第 1 周决策 |
| 更新 | Google Play（AAB） | 不走 Tauri updater |

---

## 4. 实施方案

### 4.1 Phase 1：SSH 代理（VPN 方式）

#### 4.1.1 Rust 层：`crates/android-ffi`

```text
crates/android-ffi/
├── Cargo.toml
├── src/
│   ├── lib.rs              # JNI 导出函数
│   ├── runtime.rs          # tokio runtime + Android logger
│   ├── config.rs           # Android ConfigStorage 实现
│   ├── credential.rs       # Android CredentialStore 实现
│   ├── network.rs          # SocketProtector 平台实现 + socket2 改造（trait 定义在 crates/core）
│   ├── server_api.rs       # 服务器生命周期 API
│   ├── proxy_api.rs        # 启动/停止 SOCKS5 代理
│   ├── vpn.rs              # 启动/停止 tun2proxy（直接 crate 依赖）
│   ├── event.rs            # 事件回调到 Kotlin（JNI 线程安全）
│   └── terminal_api.rs     # （Phase 2 使用）
```

关键实现点：

- `init`：创建 `tokio::runtime::Runtime`（multi-thread），初始化 `android_logger` 和 panic hook。缓存 `JavaVM` 全局引用供后续回调使用。
- `add_server` / `update_server` / `remove_server` / `list_servers`：操作 `ConfigManager` 和 `ServerManager`。
- `connect` / `disconnect` / `toggle_proxy`：调用 `ServerInstance`。`connect` 内部走 `socket2` 创建 socket → JNI 回调 `VpnService.protect(fd)` → `TokioTcpStream::from_std` → `connect`（见 §4.2 第 3 条）。
- `start_socks5` / `stop_socks5`：启动 `Socks5Server` 或 `MixedProxyServer`。
- `start_vpn`：接收 TUN fd 和 SOCKS5 端口，调用 `tun2proxy` crate 的 `general_run_async`（同 tokio runtime），传入 fd、SOCKS5 地址、MTU、DNS 模式（`over-tcp` 或 `virtual`）。无需跨语言 FFI。**⚠️ 关键前提（Phase 0 硬验证项）：tun2proxy 必须原生支持"接收 Android `VpnService.establish()` 返回的裸 fd"而非自己打开 `/dev/tun`。这是 tun2proxy 在 Android 可用性的核心前提，若 fd 注入不可行，整个主选方案需切换到备选 tun2socks。** 见 §5.1 Phase 0 第 2–3 周硬门槛。
- `subscribe_events`：把 `server:status_changed`、`proxy:status_changed`、`log:entry` 等事件序列化为 JSON 回调给 Kotlin。
  - **JNI 线程安全（FFI 经典坑）**：`JNIEnv` 不能跨线程持有。Rust 侧维护 `JavaVM` 全局引用 + Kotlin callback 对象用 `NewGlobalRef` 提升为全局引用；每次回调用 `AttachCurrentThread` 获取 env，回调后 `DetachCurrentThread`。否则 tokio worker 线程回调时崩溃。

#### 4.1.2 Android 原生层

```text
android/app/src/main/java/com/termfast/app/
├── SshVpnService.kt        # VpnService 实现（含 onRevoke / Always-on 支持）
├── SshVpnTileService.kt    # Quick Settings Tile
├── BootReceiver.kt         # 可选降级兜底（主路径走 Always-on VPN）
├── RustBridge.kt           # JNI 加载与调用
├── KeyStoreHelper.kt       # Keystore 封装
├── SharedPreferencesConfigStorage.kt
├── NotificationHelper.kt   # 前台通知
├── PermissionHelper.kt     # VPN/通知权限
└── MainActivity.kt         # 主入口
```

关键实现点：

- `SshVpnService` 启动 `Builder`：
  - `addAddress`：`10.0.0.2/24` 或类似。
  - `addRoute`：`0.0.0.0/0` 全隧道（IPv4）。
  - **`addRoute("2000::/3")` 或 `addRoute("::/0")`（IPv6）**——不处理 IPv6 会导致 IPv6 流量绕过 VPN 直接走真实网络，是 VPN 工具的严重泄漏。需验证 SSH 出口是否支持 IPv6。**默认 `2000::/3`**（覆盖 IANA 当前全球单播分配段，避免 `::/0` 把 link-local `fe80::/10` 也路由进 TUN 影响本地链路通信）；**若用户内网用 ULA（`fc00::/7`）且需经 VPN 访问，需额外 `addRoute("fc00::/7")`**，否则 ULA 流量会泄漏到真实网络。Phase 1b 提供"路由 ULA"开关。
  - `addDnsServer`：**可配置上游 DNS**（不硬编码 `8.8.8.8`——硬编码第三方 DNS 在隐私审查时易被质疑，且国内 8.8.8.8 不稳）。默认使用系统 DNS 或用户可配置 resolver；该地址会被系统作为 UDP 53 发到 TUN，由 tun2proxy 的 DNS over TCP 处理。
  - `setMtu`：**默认 1400**（移动网络 MTU 常小于 1500，尤其 IPv6 over IPv4，1500 会导致分片丢包）；提供设置项，**含 1280 选项**——IPv6 链路最小 MTU 是 1280（RFC 8200 强制），某些 IPv6-only 蜂窝/运营商路径 PMTU 可能就是 1280，1400 在此类路径仍会分片。建议默认 1400，IPv6-only 网络下若发现分片丢包可切 1280；Phase 0 spike 验证两种 MTU 在 IPv6-only 网络下的分片/丢包对比。
  - `establish()` 拿到 TUN fd 后传给 Rust `start_vpn`。
- **`foregroundServiceType`（Android 14+ 硬性要求）**：manifest 中声明前台 Service type，`startForeground` 时传入对应 type，不声明会被系统拒绝启动。
  - **VPN 场景必须用 `specialUse`**（不是 `connectedDevice`）。原因：`connectedDevice` 的 runtime prerequisites 要求蓝牙/NFC/USB/UWB/`CHANGE_NETWORK_STATE`/`CHANGE_WIFI_STATE` 等权限或设备关联条件之一，VPN 场景不满足（不应为挂 FGS type 而申请无关权限）；`specialUse` 无 runtime prerequisites，但需在 manifest `<service>` 内用 `<property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE">` 声明 justification，Play 审核时 review。
  - 所需权限：`FOREGROUND_SERVICE` + `FOREGROUND_SERVICE_SPECIAL_USE`（API 34+）。
  - API 34+ 的 `startForeground` 传 `FOREGROUND_SERVICE_TYPE_SPECIAL_USE`（按 SDK 分支见下文）。
- `VpnService.prepare()` 引导用户授权。**标准授权流程（首个合规坑，常被写错）**：
  1. `prepare()` **必须用 Activity context 调用**，不能用 Service context（Service 无 Activity 栈，调 `startActivity` 会崩或无 UI）；
  2. `prepare(activity)` 返回非 null Intent（表示未授权）→ `activity.startActivityForResult(intent, REQ_VPN_CODE)`；
  3. 在 `Activity.onActivityResult(reqCode=REQ_VPN_CODE, resultCode=RESULT_OK)` 中才能继续；
  4. 授权后才 `startForegroundService(Intent(action=ACTION_START_VPN))` 启动 `SshVpnService`，Service 内 `establish()` 才会返回非 null fd；
  5. 已授权用户（`prepare()` 返回 null）可直接启动 Service，跳过 2-3。
  - Tile/Quick Settings 触发时无 Activity：Tile 侧先 `prepare()` 拿 Intent，用 `Intent.FLAG_ACTIVITY_NEW_TASK` 启动一个透明授权 Activity 走上述流程，不能在 TileService 里直接 `establish()`。
- **`NotificationChannel`（API 26+ 必需）**：`startForeground` 前必须先 `getSystemService(NotificationManager).createNotificationChannel(...)`，否则 `startForeground` 抛 `BadNotificationException`。channel id 固定（如 `termfast_vpn`），importance 用 `IMPORTANCE_LOW`（持久通知不打扰）。
- **`startForeground` 按 SDK 分支**：
  - API 26–33：`startForeground(notificationId, notification)`（不带 type）。
  - API 34+：`startForeground(notificationId, notification, FOREGROUND_SERVICE_TYPE_SPECIAL_USE)`。
  - 用 `Build.VERSION.SDK_INT >= 34` 分支，否则低版本 lint 报错或运行时 `SecurityException`。
- **`onRevoke()`（必须实现，常被遗漏）**：用户在系统设置切换到其他 VPN 应用、或关闭"始终开启的 VPN"时，系统回调 `onRevoke()`。此时必须立即：拆除 TUN、停止 tun2proxy、停止 SOCKS5、更新 UI 状态为"已断开"。不处理会导致 tun2proxy 仍跑但 fd 已失效 → 大量错误日志 / 崩溃 / 流量黑洞。这是 VpnService 生命周期不可或缺的一环。
- **自身流量保护（防回环）**：
  - **方案 A（推荐，per-socket）**：SSH 连接前，Rust 侧用 `socket2` 创建 socket，通过 JNI 回调 `VpnService.protect(fd)`，再 `connect`。精确但需改造 `client.rs`（见 §4.2 第 3 条）。
  - **方案 B（简单，per-app）**：`Builder.addDisallowedApplication(ownPackageName)` 排除自身所有流量。简单但部分国产 ROM 支持不一致，且与 per-app 代理场景冲突。
  - 建议主用方案 A，方案 B 作为兜底。
- **DNS 处理（K5b，高风险）**：
  - Android 系统把 DNS 查询作为 UDP 53 包发到 TUN。tun2proxy 原生支持 DNS over TCP，但需正确配置 `--dns over-tcp` 或 `--dns virtual`；UDP 53 被 drop 会导致"VPN 连上了但打不开网页"。
  - 方案：在 TUN 内拦截 UDP 53，转 TCP DNS 上游（DoT/DoH 或 TCP 53 到指定 resolver）；或本地起 DNS forwarder，`addDnsServer` 指向 TUN 内地址。
  - spike 必须验证"开启 VPN 后 DNS 解析正常"，不能只测"Chrome 能访问 Google"（Chrome 可能缓存）。
- **Per-app / split tunneling（差异化功能）**：
  - `Builder.addApplication(pkg)` / `addDisallowedApplication(pkg)` 支持 per-app 代理，是 VPS 代理工具的核心差异化功能。
  - Phase 1b 至少加入 per-app 白名单/黑名单 UI。
  - **Split DNS（per-app 场景配套）**：被代理 app 的 DNS 与直连 app 的 DNS 通常需要分流。`VpnService.Builder` 支持 `addDnsServer` + `addRoute` 组合做 split DNS（被代理 app 走 VPN DNS，直连 app 走系统 DNS）。Phase 1b 一并实现。
- **Kill switch（防泄漏，Phase 1b）**：
  - VPN 断开瞬间、重连过程中流量会走真实网络。提供可选 kill switch：VPN 失败/断开期间，**启动一个"空 TUN"的 VpnService（`establish()` 建立但不跑 tun2proxy），让所有流量落入 TUN 黑洞**，而非依赖"不建立 TUN"——不建立 TUN 时流量会走真实网络，恰恰是泄漏。状态机：connected → 跑 tun2proxy；disconnected/reconnecting → 空 TUN 黑洞；off → 不建立 TUN。
- `ConnectivityManager.NetworkCallback` 监听网络切换，通知 Rust 暂停/恢复重连。
- **重连退避与 Doze**：重连由前台 Service 内的 tokio task 做指数退避（如 1s → 2s → 4s → … → 上限 60s），**不依赖 WorkManager**——前台 Service 不受 Doze 网络限制，可直接在 tokio runtime 内重连。Doze 模式下保持 TUN 但允许 SSH 心跳延迟（见 Phase 0 第 4–5 周通过标准）。
- **`Builder.setUnderlyingNetworks(null)`（建议显式调用）**：不调用时系统按当前活动网络估算 VPN 吞吐能力，影响 `NetworkCapabilities` 上报和部分系统 QoS。建议显式 `setUnderlyingNetworks(null)` 让系统自动选择底层网络；per-app 场景如需精确控制可显式传当前 `Network` 对象。属优化项，Phase 1b 处理。
- **Android 备份规则（`dataExtractionRules` / `fullBackupContent`）**：Keystore 凭据不可备份（系统级 Keystore 本就不备份），但 `SharedPreferences`/app 私有目录里的配置（含服务器地址、用户名等半敏感信息）默认会被 Auto Backup，换机/重装后可能泄露。需在 `res/xml/data_extraction_rules.xml` 和 `res/xml/backup_rules.xml` 中排除配置文件，或将其中的敏感字段加密后再存。Phase 3 处理。

#### 4.1.3 前端

- 移动端导航：底部 Tab（服务器、日志、设置）。
- 服务器列表卡片：名称、状态、出口 IP、连接开关、代理开关。
- 添加服务器：快速模式 3 步 + 高级模式。
- 服务器详情：概览、认证、代理、触发器、日志 Tab。
- VPN 开关：首页显式“连接 VPN”按钮。
- 权限引导：首次启动 VPN → 电池优化 → 添加服务器。

#### 4.1.4 构建与 CI

- **ABI 矩阵**：
  - `arm64-v8a`（必选，Google Play 自 2019 年起强制要求 64-bit 支持）
  - `armeabi-v7a`（可选，为兼容老旧 32 位设备；非 Play 强制，按产品决策保留）
  - `x86_64`（建议，CI 模拟器测试用）
- `cargo ndk` 配置上述三个 target。
- `crates/android-ffi` 的 `Cargo.toml`：`crate-type = ["cdylib"]`（产出 `libtermfast.so` 供 JNI 加载；`staticlib` 对 Android 无用，Kotlin 工程不会静态链接 Rust `.a`，徒增构建时间）。
- **R8 / ProGuard JNI keep 规则（生产必做，常被遗漏）**：Android release 构建默认开启 `minifyEnabled`，R8 会重命名 Kotlin 类，导致 JNI 的 `Java_com_termfast_app_RustBridge_*` 符号查找不到（Kotlin 类名被改成 `a.b.c`），运行时 `NoSuchMethodError` / `UnsatisfiedLinkError`。必须：
  - JNI 桥接类（`RustBridge` 及所有被 native 回调的类）加 `@Keep` 注解；
  - 或在 `proguard-rules.pro` 中保留：`-keep class com.termfast.app.** { native <methods>; }`；
  - native 方法所在类、native 方法签名、回调方法签名三者都必须保留。
- **workspace 更新**：把 `crates/android-ffi` 和 `crates/terminal` 加入 `Cargo.toml` 的 `members`（新增字段，当前无 `default-members`）；用 `default-members` 限定桌面 CI 默认构建范围，避免 Android crate 破坏桌面 CI。
- Android Gradle 引用 `libtermfast.so`（含 tun2proxy，静态链接入同一个 .so）。
- GitHub Actions 新增 Android build job：
  - 安装 NDK（tun2proxy 是纯 Rust crate）。
  - `cargo ndk -p termfast-android-ffi --target <abi>` 编译三个 ABI。
  - Gradle build 输出 **AAB**（Google Play 上架格式）+ APK（内部测试用）。

### 4.2 对现有代码的修改

为了让 Rust 工作区能在 Android 编译，需要以下改动：

1. **`crates/daemon/Cargo.toml`**：
   - 把 `mach2 = "0.6.0"` 移到 `[dev-dependencies]` 或 `cfg(target_os = "macos")`。

2. **`crates/credential/src/lib.rs`**：
   - `pub mod keychain;` 加 `#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]` 隔离。
   - `pub use keychain::KeychainCredentialStore;` 同样加 `cfg`。
   - Android 实现放在 `crates/android-ffi` 中实现 `CredentialStore` trait（通过 JNI 回调 Kotlin Keystore）。

3. **`crates/core/src/ssh/client.rs`（socket protect 改造，关键路径）**：
   - 当前 `connect_and_peek`（`client.rs:486`）直接 `tokio::net::TcpStream::connect(addr)`，**connect 之前拿不到 fd，无法 protect**。
   - 改造方案：引入 `socket2` 依赖，新增 `connect_with_protector(addr, protector: &dyn SocketProtector)`：
     1. `socket2::Socket::new(domain, SOCK_STREAM)` 创建 socket
     2. `socket.set_nonblocking(true)`
     3. 通过 `SocketProtector` trait 回调（Android 侧 JNI → `VpnService.protect(fd)`）
     4. `TokioTcpStream::from_std(socket.into())` → `connect`
   - **改造范围不止 `connect` 一行**：`connect_and_peek` 返回 `(String, PrefixedStream)`，`PrefixedStream` 内部包裹 `tokio::net::TcpStream`。整条 stream 管线都要透传 protected socket：socket2 创建 → protect → `TokioTcpStream::from_std` → 仍要包进 `PrefixedStream` → 经 `connect_stream` 喂给 russh。需同步检查 `connect_stream` 及所有调用 `connect_and_peek` 的上游路径，确保 protected socket 一路透传到 russh，不中途换新连接。
   - `SocketProtector` trait 定义在 `crates/core`（平台无关），Android 实现在 `crates/android-ffi`。
   - 桌面版提供 no-op protector，行为不变。

4. **`crates/core/src/ssh/auth.rs`**：
   - 增加 `generate_keypair_at(dir, server_id)` 或让 `generate_keypair` 接受输出目录参数。
   - 或改为返回 `(private_key_bytes, public_key_string, passphrase)`，由调用方决定存储位置（Android 用 app 私有目录或 Keystore）。

5. **`crates/core/src/config/storage.rs` / `crates/core/src/config/runtime_state.rs`**：
   - 保持 `FileConfigStorage::new(path)` 和 `RuntimeStateManager::new(path)` 可通过显式路径创建（已支持）。
   - Android 层通过 `ConfigManager::with_storage` 传入 app 私有目录路径（`context.filesDir`）。
   - `with_default_path()` 在 Android 上会因 `directories::ProjectDirs` 返回 `None` 而失败，Android 不调用此方法。

6. **`src-tauri/src/lib.rs`**：
   - 用 `#[cfg(desktop)]` 包裹托盘、窗口事件、系统代理、自动更新。
   - 移动端 `#[cfg(mobile)]` 下初始化 Rust runtime 并加载 Android service（仅 Tauri Mobile 路线）。

7. **TerminalManager 下沉（新增重构项）**：
   - 把 `crates/daemon/src/terminal.rs`（`TerminalManager`，~506 行）下沉到新 crate `crates/terminal`（或 `crates/core/src/terminal.rs`）。
   - 理由：`crates/daemon` 依赖 `mach2`/`libc`/`directories` 且与 IPC 协议强耦合，Android 同进程调用不需要 IPC daemon。`crates/android-ffi` 只依赖 `core` + `terminal`，不依赖 `daemon`。
   - 桌面 `crates/daemon` 改为依赖 `crates/terminal`，复用同一实现，结构更干净。
   - **依赖边界已确认（非"待确认"）**：`terminal.rs` 第 6 行 `use crate::server::EventForwarder;`，且 `TerminalManager::new`、`forward_terminal_event`、`forward_log` 等多处直接持有 `Arc<Mutex<Option<EventForwarder>>>`。`EventForwarder` 在 `crates/daemon/src/server.rs:14` 定义为类型别名：
     ```rust
     pub type EventForwarder = Box<dyn Fn(&str, serde_json::Value) + Send + Sync>;
     ```
     下沉时**不能**把这个类型别名原样搬到 `crates/terminal`（会割裂语义、且 daemon 侧仍要复用），而是引入 trait 抽象解耦：
     - 在 `crates/terminal` 定义 `pub trait TerminalEventSink: Send + Sync { fn forward(&self, kind: &str, payload: serde_json::Value); }`；
     - `TerminalManager::new` 改为接收 `Arc<dyn TerminalEventSink>`（或 `Arc<Mutex<Option<Box<dyn TerminalEventSink>>>>` 以保留"可后置注入"语义，对齐现有 `set_event_forwarder` API）；
     - `crates/daemon` 提供一个 `DaemonEventForwarder` 实现 `TerminalEventSink`，内部桥到既有 `EventForwarder` 闭包；
     - `crates/android-ffi` 提供 `JniEventSink` 实现，桥到 JNI 回调。
   - `terminal.rs` 本身不引用 `mach2`（mach2 仅在 `tests/proxy_benchmark.rs`），下沉后 `mach2` 留在 daemon。`crates/daemon/Cargo.toml` 的 `libc` 用 `cfg(unix)`，bionic 是 unix，若 `terminal.rs` 用到 `libc`，Android 编译没问题。
   - **工作量影响**：trait 抽象 + daemon/android-ffi 双侧适配器约增加 100–200 行，已并入 §3.3 改动估算上限。

### 4.3 Phase 2：SSH 终端

#### 4.3.1 Rust 层

- 在 `crates/android-ffi` 增加：
  - `terminal_open(server_id, cols, rows) -> session_id`
  - `terminal_input(session_id, base64_data)`
  - `terminal_resize(session_id, cols, rows)`
  - `terminal_close(session_id)`
  - `terminal_subscribe(callback)`
- 内部复用下沉后的 `crates/terminal` 的 `TerminalManager`（原 `crates/daemon/src/terminal.rs`）。
- 输出数据以 base64 通过 JSON 事件回调给 Android。

#### 4.3.2 Android 层

- Tauri Mobile：使用 `xterm.js` 在 WebView 渲染，移动端需增加软键盘、特殊按键（ESC、Ctrl、Tab）、缩放、选择。
- 原生 Kotlin：
  - 使用 `Canvas` + `TerminalEmulator` 自定义终端视图。
  - 或引入 `Termux` 的 `TerminalView`（注意 GPL 授权）。
  - 输入系统：拦截软键盘事件并发送 VT100 序列。

---

## 5. 开发计划

> 基于 2–3 人核心团队（1 Rust、1 Android 原生、1 前端）估算，为经验范围而非精确排期。

### 5.1 Phase 0：Spike（3–5 周）

> Phase 0 是整个项目的硬门槛，必须验证最不确定的技术点。以下通过标准均为硬性，未通过则不进入 Phase 1。

| 周 | 任务 | 通过标准 |
|---|---|---|
| 1 | **UI 路线决策** + 配置 `cargo-ndk` 和最小 Android 工程，编译 `crates/core` 到 `arm64-v8a` | **`cargo ndk -p termfast-core --target aarch64-linux-android` 编译通过，且 `russh`/`ring`/`ssh-key` 全部链接成功**；UI 路线决策文档输出 |
| 1–2 | 用 `crates/android-ffi` 暴露 `add_server`/`connect`/`list_servers`；验证 JNI 线程安全回调 | 真机能添加服务器并获取连接状态；事件回调跨线程无崩溃 |
| 2–3 | 验证 `tun2proxy` + `VpnService` + `Socks5Server` 链路；验证 socket protect 防回环；**建立吞吐基线** | **tun2proxy 原生支持接收 `VpnService.establish()` 返回的裸 fd（非自开 `/dev/tun`），fd 注入链路打通**；开启 VPN 后 Chrome 能访问 Google；**DNS 解析正常（非缓存）**；SSH 流量不回环；**iperf3/curl 大文件下载对比"直连 SOCKS5 vs 经 TUN"吞吐，记录基线（经 TUN 相对直连下降 ≤ 30% 为可接受，> 50% 需登记为 Phase 2+ 优化项）** |
| 3–4 | 验证 IPv6 路由、DNS over TCP、MTU 1400 vs 1280 | IPv6 网络下流量走 VPN 不泄漏；UDP 53 不丢包；**IPv6-only 网络下 MTU 1400 vs 1280 分片/丢包对比，确认默认值与降级选项** |
| 4–5 | 验证后台保活（Pixel + 1 款国产 ROM）；扫一遍 Android 16（API 36）behavior changes | **72 小时锁屏后 Service 仍存活；网络切换（WiFi↔蜂窝）后 10 秒内重连；Doze 模式下保持 TUN 但允许 SSH 心跳延迟**；确认 API 36 无影响 VPN/前台 Service 的新限制（若有则记录并调整方案） |

### 5.2 Phase 1：核心平台层

> 拆为 1a（功能闭环，单机跑通）和 1b（真机矩阵 + 平台特性），降低风险。

#### Phase 1a：功能闭环（5–6 周）

| 周 | 任务 | 产出 |
|---|---|---|
| 1–2 | `crates/android-ffi` 基础：tokio runtime、配置/凭据桥接、错误码、JNI 线程安全 | 可 add/update/remove/list 服务器 |
| 2–3 | TerminalManager 下沉到 `crates/terminal`；socket2 protect 改造 `client.rs` | 桌面测试通过，Android 编译通过 |
| 3–4 | ServerManager FFI、SOCKS5 启动/停止、事件回调 | 真机能连接/断开、开关代理 |
| 4–5 | VpnService + 前台通知（`foregroundServiceType`）+ 权限 + tun2proxy 集成 | Service 常驻、TUN 建立、全设备流量走 SSH |
| 5–6 | 移动端 UI：列表、详情、添加服务器、日志、设置 | 用户可完成"添加服务器 → 开关 VPN → 查看日志" |

**M1a**：Pixel 单机上能"添加服务器 → 连接 → 开 VPN → 浏览器访问 Google"，DNS 正常。

#### Phase 1b：真机矩阵 + 平台特性（3–4 周）

| 周 | 任务 | 产出 |
|---|---|---|
| 1–2 | IPv6 路由、kill switch、per-app 代理、MTU 设置 | IPv6 不泄漏；断开时流量阻断；per-app 白名单可用 |
| 2–3 | 真机矩阵：三星/小米/OPPO/vivo/华为/Pixel；Android 10/12/14/16 | 6 款真机基本可用，记录 ROM 适配清单 |
| 3–4 | 工程化：cargo-ndk 三 ABI、Gradle、CI、签名、AAB | 打出 release AAB |

**M1b**：6 款真机上"添加服务器 → 开 VPN → 浏览器访问 Google"稳定，AAB 可上传 Play 内部测试。

### 5.3 Phase 2：UI 与终端（4–7 周）

| 周 | 任务 | 产出 |
|---|---|---|
| 1–2 | 终端 FFI：`terminal_open`/`input`/`resize`/`close` | 真机能打开 SSH shell |
| 2–4 | 终端 UI：渲染、软键盘、特殊按键、会话列表 | 能正常登录并执行命令 |
| 4–5 | 终端稳定性：ZMODEM、二进制数据、断线重连 | rz/sz 和大输出稳定 |
| 5–7 | UI 打磨、暗色模式、中文/英文、权限引导 | 体验完整 |

**M2**：用户可在手机上完成“添加 VPS → 开关 VPN → 打开 SSH 终端 → 查看日志”。

### 5.4 Phase 3：稳定性与上架（4–6 周）

| 周 | 任务 |
|---|---|
| 1–2 | Keystore 完善（含导出/导入）、网络监听、保活、通知系统 |
| 2–3 | **接入 Crashlytics NDK**：覆盖 Rust panic（通过 JNI 上报）+ Kotlin 异常。⚠️ 注意：armeabi-v7a 在 64 位设备兼容模式下符号化失败（Firebase issue #8325），若保留 armeabi-v7a 需单独验证崩溃上报 |
| 2–4 | 真机矩阵：三星/小米/OPPO/vivo/华为/Pixel；Android 10/12/14/16 |
| 3–5 | 耗电优化、Doze 模式、崩溃修复 |
| 4–6 | Google Play 合规：数据安全表单、隐私政策、`foregroundServiceType` 声明、避免 `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` 权限（改引导手动白名单） |

**M3**：在 6 款以上真机上稳定运行 72 小时，Google Play 内部测试可安装。

### 5.5 总体周期

| 阶段 | 乐观 | 一般 | 悲观 |
|---|---|---|---|
| Spike | 3 周¹ | 4 周 | 5–6 周 |
| Phase 1a 功能闭环 | 5 周 | 6 周 | 7 周 |
| Phase 1b 真机+特性 | 4 周 | 5 周 | 6 周² |
| Phase 2 终端 | 4 周 | 5 周 | 7 周 |
| Phase 3 稳定上架 | 4 周 | 5 周 | 6 周 |
| **合计** | **20 周** | **25 周** | **32 周** |

一般情况约 **6 个月**，若团队 2 人约 **8–10 个月**。

> ¹ Spike 乐观 3 周仅当 tun2proxy fd 注入一次通过、72 小时保活无异常、Android 16 无新增限制三者同时成立；任一不成立按 4–6 周估。
> ² Phase 1b 悲观 6 周覆盖 6 款真机国产 ROM 适配（小米/OPPO/vivo 自启动白名单引导、华为 HMS 限制通常每款 2–3 天调试）+ per-app + kill switch + AAB + CI。

---

## 6. 关键路径与风险

| 风险 | 影响 | 应对 |
|---|---|---|
| `tun2proxy` 在 Android 上不稳定 | 高 | Phase 0 spike 硬性验证；**核心硬门槛：tun2proxy 必须支持接收 `VpnService.establish()` 返回的裸 fd（fd 注入），不支持则切备选**；主选 `tun2proxy`（Rust，纯 crate 依赖）；备选 `tun2socks`（Go，需引入 Go 工具链） |
| **TUN→tun2proxy→本地 SOCKS5→SSH 双跳吞吐开销** | 中 | Phase 0 建吞吐基线（iperf3 对比直连 vs 经 TUN）；若 > 50% 下降，Phase 2+ 写 `tun2ssh` 适配层跳过本地 SOCKS5 一跳 |
| SSH socket 回环 | 高 | 方案 A：`socket2` + JNI `VpnService.protect(fd)`（推荐）；方案 B：`addDisallowedApplication` 兜底 |
| `russh`/`ring`/`ssh-key` Android 交叉编译失败 | 高 | Phase 0 第 1 周硬性验证；锁定 NDK 版本；`ring` 对 NDK 版本敏感 |
| `mach2`/`keyring` 模块导出导致 Android 编译失败 | 中 | `Cargo.toml` 依赖已隔离，`lib.rs` 模块导出需补 `cfg` 隔离 |
| **IPv6 流量泄漏** | 高 | `addRoute("2000::/3")` 或 `::/0`；spike 验证 IPv6 网络下不泄漏 |
| **ULA（`fc00::/7`）内网流量泄漏** | 中 | 默认 `2000::/3` 不覆盖 ULA；Phase 1b 提供"路由 ULA"开关，用户内网场景启用 |
| **DNS UDP 53 丢包** | 高 | TUN 内拦截 UDP 53 转 TCP DNS；spike 验证 DNS 解析正常（非缓存） |
| **Kill switch 缺失导致断开瞬间泄漏** | 中 | Phase 1b 实现可选 kill switch |
| 国产 ROM 杀后台 | 高 | 真机矩阵尽早测试，前台服务 + 通知 + 引导手动白名单（不用 `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` 权限） |
| **Google Play `foregroundServiceType` 审核（API 34+）** | 高 | VPN 必须用 `specialUse`（非 `connectedDevice`，后者要求蓝牙/NFC/USB/网络状态类权限或设备关联条件）；manifest 声明 justification + `FOREGROUND_SERVICE_SPECIAL_USE` 权限；**Play 审核员可能要求补充说明或质疑 why not `connectedDevice`（中等概率）**，提前准备英文 justification 模板，例如：`This app provides user-owned VPS SSH tunneling. The foreground service maintains a persistent TUN-based VPN session to the user's own SSH server; it is not a connected-device scenario (no Bluetooth/NFC/USB/UWB/Network-state prerequisite applies). The specialUse subtype is declared with property android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE = "user_owned_ssh_vpn_tunnel".` |
| **Google Play `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` 权限被拒** | 高 | 不声明此权限，改引导用户手动到设置加白名单 |
| **Google Play `RECEIVE_BOOT_COMPLETED` 自启动受限** | 中 | 自启动主路径改为 **Always-on VPN**（无需该权限，系统自动启动 VpnService）；`BootReceiver` 仅作未开启 Always-on 时的可选降级兜底，注意 `RECEIVE_BOOT_COMPLETED` 政策 |
| **Crashlytics NDK armeabi-v7a 符号化失败** | 低 | Firebase issue #8325；若保留 armeabi-v7a 需单独验证，或仅 arm64-x86_64 接入 NDK 崩溃上报 |
| JNI 回调跨线程崩溃 | 中 | `JavaVM` 全局引用 + `NewGlobalRef` callback + `AttachCurrentThread` |
| Tauri Mobile vs 原生路线 | 中 | **Phase 0 第 1 周决策**（推荐原生 Kotlin + Compose）；Tauri 需额外验证前台 Service 集成 |
| Google Play VPN 类目审核 | 中 | 提前准备隐私政策，说明是用户自有 VPS 工具，非公共 VPN 服务 |
| FFI 接口变更 | 中 | 接口契约先行，Android/Kotlin/Rust 三方对齐后再写 UI |
| Keystore 凭据重装丢失 | 中 | 提供加密导出/导入功能；产品决策而非纯技术决策 |
| **Auto Backup 泄露服务器配置** | 中 | `dataExtractionRules`/`fullBackupContent` 排除配置文件，或敏感字段加密后存储；Phase 3 处理 |

---

## 7. 下一步建议

1. **先启动 Phase 0 Spike（第 1 周必须做 UI 路线决策）**：
   - 让 `cargo ndk -p termfast-core --target aarch64-linux-android` 编译通过，验证 `russh`/`ring`/`ssh-key` 链接。
   - 写一个最小 `crates/android-ffi` demo，真机连接服务器并返回状态，验证 JNI 跨线程回调。
   - 验证 `VpnService` + `tun2proxy` + `Socks5Server` 链路，含 DNS 解析和 IPv6。**重点验证 tun2proxy 的 fd 注入（接收 `establish()` 返回的裸 fd）是否可行，这是主选方案的硬门槛。**
   - **UI 路线决策**：推荐原生 Kotlin + Compose；若选 Tauri Mobile 需额外验证前台 Service 集成。

2. **修复跨平台编译问题（可并行启动）**：
   - `mach2` 移到 dev-dependencies。
   - `crates/credential/src/lib.rs` 的 `keychain` 模块导出加 `cfg` 隔离。
   - `auth.rs` 支持 key 路径/目录注入。
   - `client.rs` 引入 `socket2` + `SocketProtector` trait，支持 `connect_with_protector`。
   - **TerminalManager 从 `crates/daemon` 下沉到 `crates/terminal`**。

3. **接口契约先行**：
   - 先定义 `crates/android-ffi` 的 C/JNI 接口，再并行实现 Rust、Kotlin、UI。
   - 接口契约需包含错误码表、事件 JSON schema、回调线程安全约定。

4. **Android 更新机制**：
   - 不走 Tauri updater（桌面专用）。
   - Android 版通过 Google Play 内部测试 → 正式版分发；CI 输出 AAB。

5. **Keystore 凭据策略（产品决策）**：
   - 默认 Keystore 存储，重装后不可恢复。
   - **必须提供加密导出/导入功能**，避免用户重装后丢失所有 SSH 密钥/passphrase。
   - 这是产品决策，需在 Phase 1a 确认 UX 方案。

---

## 8. 参考

- 历史 Android 项目计划：`docs/android-project-plan.md`（已废弃，被本文档取代；git 历史可见）
- 核心代码：`crates/core/src/lib.rs`
- 终端管理：`crates/daemon/src/terminal.rs`（计划下沉至 `crates/terminal`，见 §4.2 第 7 点）
- 项目工作区：`Cargo.toml`

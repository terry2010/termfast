# TermFast

> 开箱即用的 SSH 终端 + 代理 + 自动化工具，支持桌面和 Android
> An all-in-one SSH terminal + proxy + automation tool for desktop and Android

<p align="center">
  <img src="docs/screenshot.png" alt="TermFast 界面预览 / TermFast UI Preview" width="720">
</p>

[English](#english) | [中文](#中文)

---

## 中文

**TermFast 把 SSH 终端、代理上网和远程服务器自动化三件事做在一起。**

你只要有一台能 SSH 登录的服务器（VPS、树莓派、家里的软路由等），无需在服务器上安装任何额外软件，就能：

- 像使用本地终端一样快速连上服务器
- 一键把这台服务器变成你的 SOCKS5 / HTTP 代理
- 让服务器在你 IP 变化、服务掉线时自动执行修复命令

支持 **macOS、Windows、Linux 桌面** 和 **Android**，界面自动跟随系统语言切换中文/英文。

### 下载安装

前往 [Releases](https://github.com/terry2010/termfast/releases) 页面下载对应平台的安装包：

| 平台 | 文件 | 说明 |
|------|------|------|
| macOS (Apple Silicon) | `TermFast_x.x.x_aarch64.dmg` | DMG 安装包 |
| Windows | `TermFast_x.x.x_x64-setup.exe` | NSIS 安装程序 |
| Windows | `TermFast_x.x.x_x64_en-US.msi` | MSI 安装包 |
| Android | `TermFast-x.x.x-android-arm64.apk` | APK 安装包 (arm64-v8a) |

桌面版支持应用内自动更新（通过 Tauri Updater + GitHub Pages 分发更新清单）。

### 它解决什么问题？

**1. 买了 VPS 不会配代理**

网上教程教你 `ssh -D 1080 root@xxx`，还要配浏览器插件、改系统代理。TermFast 里点一下「启动代理」就能用，还能一键设为系统代理。

**2. 家里宽带 IP 一变，VPS 防火墙就把你挡在外面**

很多运维用户会把服务端口锁成只有自己家 IP 能访问，但运营商 IP 每天变。TermFast 能在连接 SSH 时自动拿到你的公网 IP，并帮你更新服务器上的防火墙白名单。

**3. 远端服务挂了要人工重启**

Web 面板、数据库、爬虫、下载服务等进程异常退出时，TermFast 可以自动检测并执行 `systemctl restart xxx` 等命令，不用你半夜爬起来登录服务器。

### 功能一览

**SSH 终端**

- 真正的交互式终端（PTY），vim、htop、tmux 都能正常用
- 一个服务器可以同时开多个终端标签
- 支持 `rz` / `sz` 传文件，带进度条
- 点「连接终端」即可进入，关闭后回到服务器详情

**一键代理上网**

- 自动在本地开启 SOCKS5 + HTTP 混合代理
- 代理端口显示在界面上，点击就能复制
- 「设为系统代理」让整台电脑流量走 VPS（macOS/Windows/Linux 都支持）
- 内置「测试代理」按钮，一键看出口 IP 和延迟
- Android 版通过 VpnService 实现全局代理，支持分应用代理

**自动触发器**

- 内置模板库：IP 变化更新防火墙、进程掉了自动重启、定时检查服务状态等
- 你也可以自己写 shell 命令，编辑器带语法高亮和占位符提示
- 触发器执行过程实时显示，日志面板里能看到每条命令的输出
- 支持触发器执行成功/失败通知（桌面通知 + Android 通知）

**多服务器管理**

- 左侧列表一眼看到每台服务器是否在线、代理是否开启
- 异常的服务器自动置顶
- 添加服务器有「快速模式」，3 步就能连上
- 配置和触发器模板可以导入导出，换电脑时方便迁移

**通知系统**

- 连接状态变化通知（连接成功/断开/失败）
- IP 变化通知（公网 IP 变更时推送）
- 触发器执行结果通知（成功/失败可分别配置）
- Android 版使用系统通知渠道，桌面版使用 Tauri 通知插件

**自动更新**

- 桌面版启动后自动检查更新，有新版本时弹窗提示
- 用户确认后下载安装包（显示进度/速度/ETA），验证签名，静默安装
- 更新清单 `latest.json` 托管在 GitHub Pages，安装包存储在 GitHub Releases

### 适合谁用？

| 用户类型 | 典型需求 | 能获得的帮助 |
|---------|---------|-------------|
| **普通用户 / 小白** | 买了 VPS 想代理上网，不想研究命令行 | 点几下就能连上并启用系统代理，出错时给出大白话提示 |
| **运维 / 开发者** | 有多台 VPS，需要防火墙白名单、服务自愈 | 多服务器统一管理、触发器自动化、详细日志和调试信息 |
| **Android 用户** | 手机也想通过 SSH 代理上网 | 安装 APK 即可使用，VpnService 全局代理，支持分应用 |

### 与其他工具的区别

| 需求 | TermFast | Cloudflare Tunnel | Tailscale | 手动 ssh -D |
|-----|----------|-------------------|-----------|-------------|
| 服务器上是否需要装东西 | 不需要 | 需要 cloudflared | 需要 Tailscale | 不需要 |
| 代理上网 | 内置，一键开启 | 不适用 | 不适用 | 需要手动配置 |
| IP 变化自动更新防火墙 | 支持 | 不支持 | 不支持 | 自己写脚本 |
| 服务异常自动修复 | 支持触发器 | 不支持 | 不支持 | 自己写脚本 |
| 图形界面 | 有 | 无 | 有 | 无 |
| 多服务器管理 | 有 | 需要多个隧道 | 需要多个网络 | 不方便 |
| Android 支持 | 有 | 无 | 有 | 无 |
| 应用内自动更新 | 有 | N/A | N/A | N/A |

### 快速开始

**桌面版：**

1. 从 [Releases](https://github.com/terry2010/termfast/releases) 下载对应系统的安装包
2. 安装后打开，点击「添加服务器」
3. 填入主机地址、用户名、密码或 SSH 密钥
4. 点「连接终端」进入 SSH，或点「启动代理」开始上网

**Android 版：**

1. 从 [Releases](https://github.com/terry2010/termfast/releases) 下载 APK 文件
2. 安装后打开，添加服务器
3. 点击服务器进入详情，启动代理
4. 系统会弹出 VPN 连接请求，确认后即可全局代理
5. 可在设置中开启分应用代理，选择哪些 App 走代理

### 从源码构建

**桌面版开发：**

```bash
npm install
npm start
```

**Android 版开发：**

```bash
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME=~/Library/Android/sdk
export ANDROID_SDK_ROOT=$ANDROID_HOME

cargo build --target aarch64-linux-android -p termfast-android-ffi
cp target/aarch64-linux-android/debug/libtermfast_android_ffi.so \
   android/app/src/main/jniLibs/arm64-v8a/libtermfast_android_ffi.so
cd android && ./gradlew :app:assembleDebug
```

完整开发依赖与构建命令见 [AGENTS.md](./AGENTS.md)。

### 主要技术栈

**桌面版：** React 19 + TypeScript + Tailwind CSS + xterm.js + CodeMirror 6 + Tauri 2 + Rust（russh、tokio）+ Vitest + Playwright

**Android 版：** Kotlin + Jetpack Compose + MVVM + JNI FFI + VpnService + kotlinx.serialization

**共享核心 (Rust)：**

- `termfast-core`：平台无关的业务逻辑（SSH、代理、触发器引擎）
- `termfast-credential`：凭证管理（密码、SSH 密钥）
- `termfast-daemon`：桌面端守护进程（IPC、命名管道）
- `termfast-android-ffi`：Android JNI 桥接层
- `termfast-desktop`：桌面端 Tauri 集成（托盘、自启动、平台适配）

### 项目结构

```
termfast/
├── src/                    # 桌面版前端 (React + TypeScript)
├── src-tauri/              # Tauri 应用入口 (Rust)
├── crates/                 # Rust 核心库 (core/credential/daemon/desktop/android-ffi/cli)
├── android/                # Android 应用 (Kotlin + Compose)
├── e2e/                    # 端到端测试 (Playwright)
├── scripts/                # 构建脚本（更新清单生成等）
├── .github/workflows/      # CI/CD（ci.yml + release.yml）
└── docs/                   # 设计文档
```

### CI/CD

- **ci.yml**：每次 push 自动运行前端测试、Rust 测试、跨平台编译检查、E2E 测试
- **release.yml**：推送 semver tag（如 `v0.1.9`）时触发，自动构建全平台安装包，创建 GitHub Release 并部署更新清单到 GitHub Pages

### 许可证

[Apache-2.0](./LICENSE)

---

## English

**TermFast combines an SSH terminal, proxy internet access, and remote server automation into one app.**

All you need is an SSH-accessible server (VPS, Raspberry Pi, home router, etc.) — no extra software to install on the server. You can:

- Connect to your server with a real interactive terminal
- Turn that server into your SOCKS5 / HTTP proxy with one click
- Have the server auto-run repair commands when your IP changes or services go down

Available on **macOS, Windows, Linux desktop** and **Android**, with automatic Chinese/English language switching.

### Download & Install

Go to the [Releases](https://github.com/terry2010/termfast/releases) page and download the package for your platform:

| Platform | File | Description |
|----------|------|-------------|
| macOS (Apple Silicon) | `TermFast_x.x.x_aarch64.dmg` | DMG installer |
| Windows | `TermFast_x.x.x_x64-setup.exe` | NSIS installer |
| Windows | `TermFast_x.x.x_x64_en-US.msi` | MSI installer |
| Android | `TermFast-x.x.x-android-arm64.apk` | APK (arm64-v8a) |

The desktop version supports in-app auto-update (via Tauri Updater + GitHub Pages update manifest).

### What Problems Does It Solve?

**1. Bought a VPS but don't know how to set up a proxy**

Online tutorials tell you to run `ssh -D 1080 root@xxx`, configure browser plugins, and change system proxy settings. With TermFast, just click "Start Proxy" and it works — you can even set it as the system proxy with one click.

**2. Your home broadband IP changes and the VPS firewall blocks you**

Many users lock down service ports to only allow their home IP, but ISPs change IPs frequently. TermFast automatically detects your public IP when connecting via SSH and updates the firewall whitelist on your server.

**3. Remote services crash and need manual restart**

When web panels, databases, crawlers, or download services exit unexpectedly, TermFast can automatically detect the failure and run commands like `systemctl restart xxx` — no need to wake up at 3 AM to log in.

### Features

**SSH Terminal**

- Real interactive terminal (PTY) — vim, htop, tmux all work properly
- Open multiple terminal tabs per server
- Supports `rz` / `sz` file transfers with progress bars
- Click "Connect Terminal" to enter, close to return to server details

**One-Click Proxy**

- Automatically starts a local SOCKS5 + HTTP hybrid proxy
- Proxy ports displayed in the UI, click to copy
- "Set as System Proxy" routes all traffic through your VPS (macOS/Windows/Linux)
- Built-in "Test Proxy" button to check exit IP and latency
- Android version uses VpnService for system-wide proxy with per-app support

**Automation Triggers**

- Built-in template library: firewall update on IP change, auto-restart on process crash, scheduled health checks, etc.
- Write your own shell commands with a syntax-highlighting editor and placeholder hints
- Real-time trigger execution display with per-command output in the log panel
- Trigger success/failure notifications (desktop + Android notifications)

**Multi-Server Management**

- Server list shows online status and proxy state at a glance
- Abnormal servers automatically pinned to top
- "Quick Mode" for adding servers in 3 steps
- Import/export configs and trigger templates for easy migration

**Notification System**

- Connection status change notifications (connected/disconnected/failed)
- IP change notifications (pushed when public IP changes)
- Trigger result notifications (success/failure independently configurable)
- Android uses system notification channels, desktop uses Tauri notification plugin

**Auto-Update**

- Desktop version auto-checks for updates on startup, shows a dialog when a new version is available
- User confirms, then downloads the installer (with progress/speed/ETA), verifies signature, and silently installs
- Update manifest `latest.json` hosted on GitHub Pages, installers stored on GitHub Releases

### Who Is It For?

| User Type | Typical Need | How TermFast Helps |
|-----------|-------------|-------------------|
| **Casual users** | Bought a VPS, want proxy access without command-line hassle | Click to connect and enable system proxy, with plain-language error messages |
| **Ops / Developers** | Multiple VPS instances, need firewall whitelists and service self-healing | Unified multi-server management, trigger automation, detailed logs and debugging |
| **Android users** | Want SSH proxy on their phone | Install the APK, VpnService system-wide proxy with per-app support |

### Comparison

| Feature | TermFast | Cloudflare Tunnel | Tailscale | Manual ssh -D |
|---------|----------|-------------------|-----------|---------------|
| Server-side install required | No | Yes (cloudflared) | Yes (Tailscale) | No |
| Proxy internet access | Built-in, one-click | N/A | N/A | Manual config |
| Auto-update firewall on IP change | Yes | No | No | DIY script |
| Auto-repair crashed services | Yes (triggers) | No | No | DIY script |
| GUI | Yes | No | Yes | No |
| Multi-server management | Yes | Multiple tunnels | Multiple networks | Inconvenient |
| Android support | Yes | No | Yes | No |
| In-app auto-update | Yes | N/A | N/A | N/A |

### Quick Start

**Desktop:**

1. Download the installer for your OS from [Releases](https://github.com/terry2010/termfast/releases)
2. Install and open, click "Add Server"
3. Enter host address, username, password or SSH key
4. Click "Connect Terminal" for SSH, or "Start Proxy" for internet access

**Android:**

1. Download the APK from [Releases](https://github.com/terry2010/termfast/releases)
2. Install and open, add a server
3. Tap a server to view details, then start the proxy
4. The system will prompt a VPN connection request — confirm to enable system-wide proxy
5. Enable per-app proxy in settings to choose which apps use the proxy

### Build from Source

**Desktop development:**

```bash
npm install
npm start
```

**Android development:**

```bash
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME=~/Library/Android/sdk
export ANDROID_SDK_ROOT=$ANDROID_HOME

cargo build --target aarch64-linux-android -p termfast-android-ffi
cp target/aarch64-linux-android/debug/libtermfast_android_ffi.so \
   android/app/src/main/jniLibs/arm64-v8a/libtermfast_android_ffi.so
cd android && ./gradlew :app:assembleDebug
```

See [AGENTS.md](./AGENTS.md) for full development dependencies and build commands.

### Tech Stack

**Desktop:** React 19 + TypeScript + Tailwind CSS + xterm.js + CodeMirror 6 + Tauri 2 + Rust (russh, tokio) + Vitest + Playwright

**Android:** Kotlin + Jetpack Compose + MVVM + JNI FFI + VpnService + kotlinx.serialization

**Shared Core (Rust):**

- `termfast-core`: Platform-agnostic business logic (SSH, proxy, trigger engine)
- `termfast-credential`: Credential management (passwords, SSH keys)
- `termfast-daemon`: Desktop daemon (IPC, named pipes)
- `termfast-android-ffi`: Android JNI bridge layer
- `termfast-desktop`: Desktop Tauri integration (tray, autostart, platform adapters)

### Project Structure

```
termfast/
├── src/                    # Desktop frontend (React + TypeScript)
├── src-tauri/              # Tauri app entry point (Rust)
├── crates/                 # Rust core libraries (core/credential/daemon/desktop/android-ffi/cli)
├── android/                # Android app (Kotlin + Compose)
├── e2e/                    # End-to-end tests (Playwright)
├── scripts/                # Build scripts (update manifest generation, etc.)
├── .github/workflows/      # CI/CD (ci.yml + release.yml)
└── docs/                   # Design documents
```

### CI/CD

- **ci.yml**: Runs frontend tests, Rust tests, cross-platform compile checks, and E2E tests on every push
- **release.yml**: Triggered by pushing a semver tag (e.g. `v0.1.9`), builds all platform installers, creates a GitHub Release, and deploys the update manifest to GitHub Pages

### License

[Apache-2.0](./LICENSE)

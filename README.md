# TermFast

> SSH 终端 + 代理上网 + 服务器自动化，一个 App 搞定

支持 macOS、Windows、Linux 桌面和 Android，界面自动切换中英文。

---

## 下载

前往 [Releases](https://github.com/terry2010/termfast/releases) 下载：

| 平台 | 文件 |
|------|------|
| macOS (Apple Silicon) | `TermFast_x.x.x_aarch64.dmg` |
| Windows | `TermFast_x.x.x_x64-setup.exe` |
| Android | `TermFast-x.x.x-android-arm64.apk` |

桌面版支持应用内自动更新，有新版本会弹窗提示。

---

## 它能干什么

### 1. SSH 终端

- 真正的交互式终端，vim、htop、tmux 都能正常用
- 一个服务器可以同时开多个终端标签
- 支持 `rz` / `sz` 传文件，带进度条

### 2. 一键代理上网

- 点一下「启动代理」，服务器就变成你的 SOCKS5 / HTTP 代理
- 「设为系统代理」让整台电脑流量走 VPS
- 内置测试按钮，一键看出口 IP 和延迟
- Android 版通过 VpnService 全局代理，支持分应用代理

### 3. 自动触发器

服务器出状况时自动执行命令，不用你半夜爬起来：

- **IP 变了** → 自动更新防火墙白名单
- **服务挂了** → 自动 `systemctl restart`
- **定时检查** → 定期探测服务是否正常

内置模板库，也可以自己写 shell 命令，编辑器带语法高亮。

### 4. 云同步

- 配置加密后同步到 Dropbox / 百度网盘
- 多设备间保持一致，换电脑不用重新配
- 用主密码加密，云端只存密文

---

## 快速开始

**桌面版：**

1. 下载安装包，打开 App
2. 点「添加服务器」，填入主机、用户名、密码或 SSH 密钥
3. 点「连接终端」进入 SSH，或点「启动代理」开始上网

**Android 版：**

1. 下载 APK 安装
2. 添加服务器
3. 点击服务器，启动代理
4. 系统弹出 VPN 请求，确认即可全局代理

---

## 适合谁用

- **买了 VPS 想代理上网** — 不用敲命令，点几下就能用
- **有多台服务器** — 统一管理，一眼看到哪台在线、哪台异常
- **服务器需要自动维护** — IP 变化更新防火墙、服务挂了自动重启
- **手机也要代理** — Android 版全局代理，支持分应用

---

## 从源码构建

**桌面版：**

```bash
npm install
npm start
```

**Android 版：**

```bash
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME=~/Library/Android/sdk

cargo build --target aarch64-linux-android -p termfast-android-ffi
cp target/aarch64-linux-android/debug/libtermfast_android_ffi.so \
   android/app/src/main/jniLibs/arm64-v8a/libtermfast_android_ffi.so
cd android && ./gradlew :app:assembleDebug
```

---

## 许可证

[Apache-2.0](./LICENSE)

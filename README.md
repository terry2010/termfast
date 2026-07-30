# TermFast

> 现代 SSH 客户端 + 运维 AI 助手，跨桌面和 Android

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

### 2. AI 助手（开发中）

- 自然语言转 shell 命令：输入 `#` + 描述，AI 生成可执行命令
- 命令解释：选中命令，AI 解释每个参数的含义
- 错误诊断：命令执行失败时，AI 自动分析错误并给出修复建议
- 支持自带 API Key（BYOK）和本地模型（Ollama）

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

### 5. 端口转发

- 本地转发（-L）：把远程服务映射到本地，比如远程 MySQL 映射到 `localhost:13306`
- 远程转发（-R）：把本地服务暴露到远程服务器
- 内置 MySQL / Redis / PostgreSQL / Web 快捷模板
- 支持自动启动：SSH 连接后自动启动规则
- 无需先连接 SSH 或开终端，直接点「启动」即可（自动连接）

**服务器 sshd 配置要求：** 端口转发需要服务器允许 TCP 转发。如果启动时报错 `AdministrativelyProhibited` 或 `rejected by the other party`，需要在服务器上修改 `sshd_config`：

```bash
sudo vi /etc/ssh/sshd_config
```

确保你的用户允许 TCP 转发（在文件末尾按用户覆盖）：

```
Match User your_username
    AllowTcpForwarding yes
```

重启 sshd：

```bash
sudo systemctl restart sshd
```

> 注意：全局 `AllowTcpForwarding no` 会对所有未在 `Match` 块中显式允许的用户生效。如果你登录的用户不在任何 `Match` 块中，会继承全局设置而被拒绝。

---

## 快速开始

**桌面版：**

1. 下载安装包，打开 App
2. 点「添加服务器」，填入主机、用户名、密码或 SSH 密钥
3. 点「连接终端」进入 SSH

**Android 版：**

1. 下载 APK 安装
2. 添加服务器
3. 点击服务器，连接终端

---

## 适合谁用

- **有多台服务器** — 统一管理，一眼看到哪台在线、哪台异常
- **服务器需要自动维护** — IP 变化更新防火墙、服务挂了自动重启
- **记不住命令** — AI 助手帮你把自然语言转成 shell 命令
- **多设备办公** — 云同步让配置在桌面和手机间保持一致

---

## 从源码构建

**桌面版：**

```bash
npm install
npm start
```

**Android 版：**

```bash
export JAVA_HOME="/Applications/Android Studio.app/jbr/Contents/Home"
export ANDROID_HOME=~/Library/Android/sdk

cargo build --target aarch64-linux-android -p termfast-android-ffi
cp target/aarch64-linux-android/debug/libtermfast_android_ffi.so \
   android/app/src/main/jniLibs/arm64-v8a/libtermfast_android_ffi.so
cd android && ./gradlew :app:assembleDebug
```

---

## 许可证

[Apache-2.0](./LICENSE)

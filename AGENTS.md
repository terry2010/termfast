# AI 助手开发规范

## Subagent 使用规则（仅允许 reviewer）

**只允许 spawn `reviewer` 这一个只读 subagent profile，禁止其他所有 subagent。**

### 允许

- 使用 `run_subagent` 且 profile 为 `reviewer`（定义在 `.devin/agents/reviewer/AGENT.md`）
- reviewer 是只读 agent：只能 `read/grep/glob/exec`，禁止 `write/edit`
- 用途：每个功能点开发完成后，spawn reviewer 对改动做独立评审

### 禁止

- 禁止使用内置的 `subagent_explore` 或 `subagent_general` profile
- 禁止 spawn 任何其他自定义 subagent profile
- 禁止用 subagent 做实现工作（写代码、改代码一律由主 agent 直接完成）
- 禁止嵌套 subagent（reviewer 不能再 spawn subagent）

### 为什么要限制

历史经验表明，多 subagent 并发会导致 GUI OOM（内存溢出）、界面卡死。
只保留一个只读、工具受限的 reviewer，既能获得独立评审（防偷工），
又把 OOM 风险压到最低：reviewer 只用 auto-approve 的只读工具，
不触发权限弹窗，不并发，不嵌套。

### 何时 spawn reviewer

每个功能点完成开发 + 单元测试后，进入"对比文档确认"环节时，
主 agent 必须 spawn reviewer，在 task prompt 里包含：
1. 本次改动涉及的文件列表
2. design doc 中该功能点的验收标准（逐条）
3. 要求 reviewer 逐条核对并输出带证据的结论

reviewer 返回报告后，主 agent 根据报告决定是否进入下一功能点。
未通过项必须补完后重新 spawn reviewer 复审。

## 文件写入规范（防止 GUI OOM）

写入文件时必须**一段一段地写入**，不能一次性写入整个文件的全部内容。

一次性写入大文件会导致 GUI OOM（内存溢出），工具会崩溃或卡死。

### 正确做法

1. 先用 `write` 工具写入文件的第一段内容（如文件头、imports、第一个类/函数）
2. 再用 `edit` 工具逐段追加后续内容

### 分段阈值

- 超过 **200 行**的文件，必须分 **3 次以上**写入
- 每次写入建议 ≤ 150 行
- 写入前先估算行数，规划分段点（按类/函数/逻辑块切分）

### 示例

```text
# 错误 ❌
write(file_path="big_file.rs", content="<800 行完整内容>")

# 正确 ✅
write(file_path="big_file.rs", content="<第 1 段：imports + 结构体定义>")
edit(file_path="big_file.rs", old_string="// === SECTION 1 END ===",
     new_string="// === SECTION 1 END ===\n<第 2 段：方法实现>")
edit(file_path="big_file.rs", old_string="// === SECTION 2 END ===",
     new_string="// === SECTION 2 END ===\n<第 3 段：测试>")
```

### 实施要点

- 在 `write` 的内容末尾留一个明确的分隔标记注释（如 `// === SECTION N END ===`），便于后续 `edit` 用唯一锚点追加
- 不要用文件末尾的空行作为锚点，容易因空白处理不一致导致 `old_string` 不唯一
- 若文件已存在且需大改，先 `read` 全文，再分段 `edit`，不要 `write` 覆盖



## 自动更新（Tauri Updater）

### 架构

- 使用 `tauri-plugin-updater` 实现自动更新
- 签名密钥：`~/.tauri/termfast-signing.key`（私钥）+ `tauri.conf.json` 里的 `pubkey`（公钥）
- 更新清单：`latest.php` 部署在 `termfast.xisj.com`（PHP 动态生成，带 5 分钟缓存）
- 备用清单：`latest.json` 仍部署到 GitHub Pages（`gh-pages` 分支）作为 fallback
- 安装包存储：GitHub Releases
- 国内加速：`latest.php` 根据客户端 IP（GeoLite2-City 数据库）智能选源
  - 国内 → `termfast.xisj.com/releases/`（Nginx 反代 GitHub Releases）
  - 海外 → `github.com` 直连

### 服务器端文件（`server/` 目录）

| 文件 | 用途 |
|------|------|
| `latest.php` | Tauri 更新清单 + IP 智能选源（主更新端点） |
| `ip.php` | 只显示客户端 IP |
| `ip2.php` | 显示客户端 IP + 浏览器信息 + IP 归属地 |
| `cloud-sync.php` | 云同步 OAuth 代理 |
| `lib/geoip.php` | GeoIP 查询共用函数（MaxMind GeoLite2-City） |
| `data/GeoLite2-City.mmdb` | IP 地理位置数据库（63MB，每月更新） |
| `composer.json` | PHP 依赖（maxmind-db/reader） |

### 服务器部署

1. 安装 PHP + composer：
   ```bash
   apt install php php-curl php-mbstring composer
   ```
2. 安装 PHP 依赖：
   ```bash
   cd server && composer install
   ```
3. 下载 GeoLite2-City 数据库（需注册 MaxMind 免费账号）：
   - 注册：https://www.maxmind.com/en/geolite2/signup
   - 下载 GeoLite2-City.mmdb 放到 `server/data/`
   - 或用 `geoipupdate` 工具自动更新
4. Nginx 配置：
   - PHP 文件放到 `/var/www/html/tools/`
   - 加 `/releases/` 反代 location（见下方 Nginx 配置）
5. crontab 每月自动更新 GeoIP 数据库：
   ```bash
   0 0 1 * * /usr/bin/geoipupdate
   ```

### Nginx 配置（GitHub Releases 反代）

```nginx
# 反代 GitHub Releases 下载，供国内用户加速
location /releases/ {
    proxy_pass https://github.com/terry2010/termfast/releases/download/;
    proxy_set_header Host github.com;
    proxy_set_header Accept-Encoding "";
    proxy_ssl_server_name on;
    proxy_buffering on;
    proxy_max_temp_file_size 1g;
    # 缓存大文件 1 小时
    proxy_cache_valid 200 1h;
}
```

### 首次配置（每个仓库只需一次）

1. 生成签名密钥对（已执行，私钥已保存到本地）：
   ```bash
   npx tauri signer generate --ci -w ~/.tauri/termfast-signing.key
   ```
2. 将 **私钥内容** 添加到 GitHub 仓库 Secret：
   - 名称：`TAURI_SIGNING_PRIVATE_KEY`
   - 值：`cat ~/.tauri/termfast-signing.key` 的完整内容
3. 确认 `src-tauri/tauri.conf.json` 中 `plugins.updater.endpoints` 指向 `https://termfast.xisj.com/tools/latest.php`。
4. 在仓库 **Settings → Pages** 中启用 GitHub Pages（备用 fallback），Source 选择 `Deploy from a branch`，Branch 选择 `gh-pages`。

### 发布流程

1. 同步版本号（必须保持一致）：
   - `package.json`
   - `src-tauri/tauri.conf.json`
   - `src-tauri/Cargo.toml`
   - `Cargo.toml`（workspace version）
2. 提交并推送版本号改动。
3. 打 tag 并推送，触发 GitHub Actions：
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
4. `.github/workflows/release.yml` 自动完成：
   - 在 Windows + macOS  runner 上构建安装包
   - 用 `TAURI_SIGNING_PRIVATE_KEY` 对更新包签名
   - 创建 GitHub Release（草稿 → 自动发布）
   - 从 Release Asset 中读取 `.sig` 文件
   - 生成 `latest.json`（GitHub 直连 URL）并部署到 `gh-pages` 分支（备用）
   - `latest.php` 自动从 GitHub API 抓取最新 Release（5 分钟缓存），无需手动更新服务器

### 环境变量 / Secrets

- `GITHUB_TOKEN`：Actions 自动提供，无需手动设置
- `TAURI_SIGNING_PRIVATE_KEY`：GitHub Secret，私钥内容
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：当前无密码，可留空
- `ANDROID_KEYSTORE_BASE64`：GitHub Secret，Android APK 签名密钥（base64 编码）
  - 生成方式：`base64 -i android/app/keystores/release.keystore`
  - CI 中解码后用于 release APK 签名
- `DROPBOX_APP_KEY`：服务器环境变量，Dropbox 应用 App Key
  - 从 [Dropbox Developer Console](https://www.dropbox.com/developers/apps) 创建应用后获取
  - 配置在服务器的 PHP-FPM pool 或 Nginx fastcgi_param 中
- `DROPBOX_APP_SECRET`：服务器环境变量，Dropbox 应用 Secret（**不存入 App 二进制**）
- `BAIDU_APP_KEY`：服务器环境变量，百度网盘应用 App Key
  - 从 [百度网盘开放平台](https://pan.baidu.com/union/) 创建应用后获取
- `BAIDU_APP_SECRET`：服务器环境变量，百度网盘应用 Secret（**不存入 App 二进制**）

### Cloud Sync 服务器代理架构

云同步功能（Dropbox + 百度网盘）使用服务器代理架构：

- **App 端**：不持有任何 app_key 或 app_secret，OAuth token 交换通过代理服务器进行
- **服务器端**：`server/cloud-sync.php`，持有 app_key + app_secret，只参与 token 交换
- **数据传输**：配置文件用主密码加密后由 App 直接上传到云 API，服务器不接触用户数据

**服务器地址**硬编码在 `crates/cloud-sync/src/lib.rs`：
```rust
pub const CLOUD_SYNC_SERVER: &str = "https://termfast.xisj.com/tools/cloud-sync.php";
```

**服务器部署**：
1. 将 `server/cloud-sync.php` 部署到服务器
2. 在 Nginx/PHP-FPM 中配置环境变量：
   - `DROPBOX_APP_KEY`, `DROPBOX_APP_SECRET`
   - `BAIDU_APP_KEY`, `BAIDU_APP_SECRET`
3. 确保 HTTPS

**百度网盘**：使用 Authorization Code flow（通过服务器），有 refresh_token（10年有效），可自动续期。
**Dropbox**：使用 PKCE + 服务器代理换 token，有 refresh_token，可自动续期。

### Android APK 发布

`release.yml` 中的 `build-android` job 会在 ubuntu-latest 上：
1. 解码 `ANDROID_KEYSTORE_BASE64` Secret 还原 keystore
2. 安装 Android SDK 36 + NDK 27
3. 编译 Rust `.so`（aarch64-linux-android，release 模式）
4. 用 Gradle 构建 release APK（R8 混淆 + 签名）
5. 上传 `TermFast-{version}-android-arm64.apk` 到 GitHub Release

**首次配置**：在 GitHub 仓库 Settings → Secrets → Actions 中添加 `ANDROID_KEYSTORE_BASE64`，
值为 `base64 -i android/app/keystores/release.keystore` 的输出。

### 客户端行为

- 启动后 5 秒静默检查更新
- 有新版本时弹窗显示版本信息和更新内容
- 用户确认后下载安装包（显示进度/速度/ETA），验证签名，静默安装
- 安装完成后提示重启
- 设置页"关于"分区有"检查更新"按钮可手动触发

### 注意事项

- 私钥丢了就无法发布更新，务必备份 `~/.tauri/termfast-signing.key`
- 当前 CI 不配置 Apple Developer ID / Windows EV 代码签名证书，安装包首次打开会有系统安全提示，属正常行为
- 发布前确保四个版本号文件一致，否则自动更新会失败

## 构建与测试命令

### 启动开发环境

```bash
# 启动前后端（推荐）— 自动启动 vite + tauri，HMR 热更新
npm start

# 等价于 npm start
npm run tauri dev

# 仅启动前端 dev server（浏览器调试用）
npm run dev
```

> **HMR 配置**：`vite.config.ts` 中固定配置了 HMR WebSocket（`localhost:1421`），
> 确保 Tauri webview 能正确接收前端热更新。修改前端代码后会自动刷新，无需重启。

### Rust 后端

```bash
# 编译检查
cd src-tauri && cargo check --lib

# 运行单元测试 
cd src-tauri && cargo test --lib

# Clippy 检查 
cd src-tauri && cargo clippy --lib

# 前端类型检查
npx tsc --noEmit
```

### Android 构建（TermFast Android 版）

**环境变量**（需在 shell 中设置或写入 `android/local.properties`）：

```bash
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME=/Users/terry/Library/Android/sdk
export ANDROID_SDK_ROOT=$ANDROID_HOME
```

**Rust Native 庥编译（arm64-v8a）：**

```bash
# Debug 编译
cargo build --target aarch64-linux-android -p termfast-android-ffi

# Release 编译（用于 release APK）
cargo build --release --target aarch64-linux-android -p termfast-android-ffi

# 拷贝 .so 到 jniLibs 并 strip debug symbols
cp target/aarch64-linux-android/release/libtermfast_android_ffi.so \
   android/app/src/main/jniLibs/arm64-v8a/libtermfast_android_ffi.so
/opt/homebrew/share/android-ndk/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-strip \
   --strip-debug android/app/src/main/jniLibs/arm64-v8a/libtermfast_android_ffi.so
```

**Gradle 构建：**

```bash
cd android

# Debug APK
./gradlew :app:assembleDebug

# Release APK（R8 混淆 + 资源压缩 + 签名）
./gradlew :app:assembleRelease

# Release AAB（用于 Google Play 上架）
./gradlew :app:bundleRelease
```

**构建产物路径：**

- Debug APK: `android/app/build/outputs/apk/debug/app-debug.apk`
- Release APK: `android/app/build/outputs/apk/release/app-release.apk`
- Release AAB: `android/app/build/outputs/bundle/release/app-release.aab`

**Release 签名：**

- Keystore: `android/app/keystores/release.keystore`
- Alias: `termfast`，密码: `termfast`
- 签名配置在 `android/app/build.gradle.kts` 的 `signingConfigs.release`
- ProGuard keep 规则在 `android/app/proguard-rules.pro`

**APK 签名验证：**

```bash
"$ANDROID_HOME/build-tools/36.0.0/apksigner" verify -v \
  android/app/build/outputs/apk/release/app-release.apk
```

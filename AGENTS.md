# AI 助手开发规范

## 严禁使用 subagent（绝对规则）

**绝对禁止使用 `run_subagent` 工具！**

使用 subagent 会导致：
- GUI OOM 崩溃（内存溢出）
- 任务失败
- 用户界面卡死

任何情况下都不要调用 `run_subagent`，无论是 `is_background=true` 还是 `is_background=false`。
所有任务必须由主 agent 直接完成，不得委派给 subagent。

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
- 签名密钥：`~/.tauri/ai-subtrans.key`（私钥）+ `tauri.conf.json` 里的 `pubkey`（公钥）
- 更新清单：`latest.json` 托管在 GitHub Pages（`gh-pages` 分支）
- 安装包存储：GitHub Releases
- 国内加速：`latest.json` 里的 URL 用 `gh-proxy.com` 前缀

### 发布流程

```
node scripts/publish.mjs <版本号> "更新内容"
```

脚本自动完成：改版本号 → 带签名构建 → 创建 GitHub Release → 上传 .exe + .sig → 更新 latest.json

### 环境变量

- `GITHUB_TOKEN`：GitHub Personal Access Token（repo 权限）
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：私钥密码

### 客户端行为

- 启动后 5 秒静默检查更新
- 有新版本时弹窗显示版本信息和更新内容
- 用户确认后下载安装包（显示进度/速度/ETA），验证签名，静默安装
- 安装完成后提示重启
- 设置页"关于"分区有"检查更新"按钮可手动触发

### 注意事项

- 私钥丢了就无法发布更新，务必备份
- `TAURI_SIGNING_PRIVATE_KEY` 需要传私钥内容（不是路径），脚本会自动读取文件
- `--build-only` 参数可只构建不发布（本地测试用）

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
 
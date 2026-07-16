#!/usr/bin/env bash
# 发布脚本 — TermFast
# 用法:
#   ./docs/发布.sh v0.1.0          # 指定版本号发布
#   ./docs/发布.sh v0.1.0 "修复了rz bug"  # 带更新说明
#   ./docs/发布.sh v0.1.0 --dry-run  # 只检查不推送
#
# 脚本会自动:
#   1. 检查工作区干净
#   2. 同步 4 个文件的版本号
#   3. 提交版本号改动
#   4. 打 tag 并推送，触发 GitHub Actions 构建 + 发布
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# === 颜色 ===
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
die()   { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# === 解析参数 ===
VERSION=""
MESSAGE=""
DRY_RUN=false

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    v*) VERSION="$arg" ;;
    *) [ -z "$MESSAGE" ] && MESSAGE="$arg" || MESSAGE="$MESSAGE $arg" ;;
  esac
done

[ -z "$VERSION" ] && die "用法: $0 <版本号> [更新说明] [--dry-run]
示例:
  $0 v0.1.0
  $0 v0.1.0 \"修复 rz/sz 传文件 bug\""

# 去掉前缀 v 得到纯版本号
VER_NUM="${VERSION#v}"
[ "$VER_NUM" = "$VERSION" ] && VERSION="v$VER_NUM"

info "准备发布 ${VERSION}"

# === 1. 检查工作区（只关注已跟踪文件的改动，忽略未跟踪文件）===
TRACKED_CHANGES=$(git diff --name-only; git diff --cached --name-only)
if [ -n "$TRACKED_CHANGES" ]; then
  warn "已跟踪文件有未提交的改动:"
  echo "$TRACKED_CHANGES" | sed 's/^/  /'
  echo ""
  read -rp "是否先提交这些改动? [y/N] " yn
  case "$yn" in
    [Yy]*)
      git add -u
      [ -z "$MESSAGE" ] && MESSAGE="prepare ${VERSION}"
      git commit -m "$MESSAGE"
      ok "已提交改动"
      ;;
    *)
      die "请先处理工作区改动再发布"
      ;;
  esac
else
  ok "工作区干净（未跟踪文件已忽略）"
fi

# === 2. 检查 GitHub Secret 是否已配置 ===
info "提醒: 确保已在 GitHub 仓库 Settings → Secrets 中配置 TAURI_SIGNING_PRIVATE_KEY"
info "      确保已在 GitHub 仓库 Settings → Pages 中启用 gh-pages 分支"

# === 3. 同步版本号 ===
info "同步版本号到 ${VER_NUM} ..."

PKG_JSON="package.json"
TAURI_CONF="src-tauri/tauri.conf.json"
TAURI_CARGO="src-tauri/Cargo.toml"
ROOT_CARGO="Cargo.toml"

# package.json
if grep -q "\"version\": *\"[^\"]*\"" "$PKG_JSON"; then
  sed -i '' "s/\"version\": *\"[^\"]*\"/\"version\": \"${VER_NUM}\"/" "$PKG_JSON"
  ok "$PKG_JSON → ${VER_NUM}"
fi

# tauri.conf.json
if grep -q "\"version\": *\"[^\"]*\"" "$TAURI_CONF"; then
  sed -i '' "s/\"version\": *\"[^\"]*\"/\"version\": \"${VER_NUM}\"/" "$TAURI_CONF"
  ok "$TAURI_CONF → ${VER_NUM}"
fi

# src-tauri/Cargo.toml (只改 [package] 下的 version，不改 [dependencies])
# 用 awk 精确匹配 [package] 段
awk -v ver="$VER_NUM" '
  /^\[package\]/ { in_pkg=1 }
  /^\[/ && !/^\[package\]/ { in_pkg=0 }
  in_pkg && /^version *=/ { print "version = \"" ver "\""; next }
  { print }
' "$TAURI_CARGO" > "$TAURI_CARGO.tmp" && mv "$TAURI_CARGO.tmp" "$TAURI_CARGO"
ok "$TAURI_CARGO → ${VER_NUM}"

# 根 Cargo.toml 的 [workspace.package]
awk -v ver="$VER_NUM" '
  /^\[workspace\.package\]/ { in_wpkg=1 }
  /^\[/ && !/^\[workspace\.package\]/ { in_wpkg=0 }
  in_wpkg && /^version *=/ { print "version = \"" ver "\""; next }
  { print }
' "$ROOT_CARGO" > "$ROOT_CARGO.tmp" && mv "$ROOT_CARGO.tmp" "$ROOT_CARGO"
ok "$ROOT_CARGO → ${VER_NUM}"

# === 4. 验证版本号一致 ===
info "验证版本号一致性 ..."
PKG_VER=$(grep -o '"version": *"[^"]*"' "$PKG_JSON" | head -1 | sed 's/.*"\(.*\)"$/\1/')
TAURI_VER=$(grep -o '"version": *"[^"]*"' "$TAURI_CONF" | head -1 | sed 's/.*"\(.*\)"$/\1/')
CARGO_VER=$(grep '^version' "$TAURI_CARGO" | head -1 | sed 's/.*"\(.*\)".*/\1/')
ROOT_VER=$(awk '/^\[workspace\.package\]/{f=1} f&&/^version/{print; exit}' "$ROOT_CARGO" | sed 's/.*"\(.*\)".*/\1/')

if [ "$PKG_VER" != "$VER_NUM" ] || [ "$TAURI_VER" != "$VER_NUM" ] || \
   [ "$CARGO_VER" != "$VER_NUM" ] || [ "$ROOT_VER" != "$VER_NUM" ]; then
  die "版本号不一致:
  package.json:       $PKG_VER
  tauri.conf.json:    $TAURI_VER
  src-tauri/Cargo.toml: $CARGO_VER
  Cargo.toml (workspace): $ROOT_VER
  期望: $VER_NUM"
fi
ok "四个文件版本号均为 ${VER_NUM}"

# === 5. 编译检查 ===
info "编译检查 ..."
if ! cargo check --workspace 2>&1 | tail -1 | grep -q "Finished"; then
  die "cargo check 失败，请修复后再发布"
fi
ok "cargo check 通过"

if ! npx tsc --noEmit 2>&1 | grep -q "^$"; then
  if npx tsc --noEmit 2>&1 | grep -q "error"; then
    die "tsc 类型检查失败"
  fi
fi
ok "tsc 类型检查通过"

# === 6. 提交版本号改动 ===
info "提交版本号改动 ..."
git add "$PKG_JSON" "$TAURI_CONF" "$TAURI_CARGO" "$ROOT_CARGO"

if [ -z "$(git diff --cached --name-only)" ]; then
  warn "版本号未变化（可能已经是 ${VERSION}），跳过提交"
else
  COMMIT_MSG="release ${VERSION}"
  [ -n "$MESSAGE" ] && COMMIT_MSG="${COMMIT_MSG}

${MESSAGE}"
  git commit -m "$COMMIT_MSG"
  ok "已提交: ${COMMIT_MSG}"
fi

# === 7. 打 tag ===
if git tag -l "$VERSION" | grep -q "$VERSION"; then
  die "tag ${VERSION} 已存在，如需重打请先: git tag -d ${VERSION}"
fi

TAG_MSG="${VERSION}"
[ -n "$MESSAGE" ] && TAG_MSG="${MESSAGE}"

git tag -a "$VERSION" -m "$TAG_MSG"
ok "已打 tag: ${VERSION}"

# === 8. 推送 ===
if [ "$DRY_RUN" = true ]; then
  warn "--dry-run 模式，不推送。以下命令未执行:"
  echo "  git push origin main"
  echo "  git push origin ${VERSION}"
  exit 0
fi

info "推送到远程 ..."
git push origin main 2>/dev/null || git push origin master 2>/dev/null || warn "分支推送失败，请手动 git push"
ok "代码已推送"

git push origin "$VERSION"
ok "tag ${VERSION} 已推送，GitHub Actions 已触发"

echo ""
ok "发布流程已启动！"
echo ""
echo "  查看构建进度: https://github.com/terry2010/termfast/actions"
echo "  Release 页面: https://github.com/terry2010/termfast/releases/tag/${VERSION}"
echo "  更新清单:     https://terry2010.github.io/termfast/latest.json"
echo ""
warn "首次发布后记得去 Settings → Pages 选择 gh-pages 分支"

#!/bin/bash
# 清除 macOS quarantine 属性，解决"已损坏"提示
# 双击此脚本即可运行

set -e

APP_PATH="/Applications/TermFast.app"

if [ ! -d "$APP_PATH" ]; then
    echo "❌ 未找到 $APP_PATH"
    echo "请先将 TermFast 拖到 Applications 文件夹"
    echo ""
    echo "按任意键关闭..."
    read -n 1
    exit 1
fi

xattr -cr "$APP_PATH"
echo "✅ 已清除隔离属性，现在可以打开 TermFast 了"
echo ""
echo "按任意键关闭..."
read -n 1

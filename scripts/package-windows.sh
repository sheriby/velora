#!/bin/bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$REPO_ROOT/Cargo.toml")"
TARGET="x86_64-pc-windows-gnu"

if ! command -v makensis >/dev/null 2>&1; then
    echo "未找到 makensis；请安装 NSIS。" >&2
    exit 1
fi
if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    echo "未找到 MinGW-w64；请安装 Windows GNU 交叉编译工具链。" >&2
    exit 1
fi

cd "$REPO_ROOT"
mkdir -p dist
cargo build --profile releasewin --target "$TARGET"
file "target/$TARGET/releasewin/velora.exe" | rg -q "PE32\+ executable.*x86-64.*Windows"
# 防回归：exe 必须含 Common-Controls v6 manifest（TaskDialogIndirect 入口点
# 依赖它激活 comctl32 v6；缺失会使程序启动即报“无法定位程序输入点”）。
strings -a "target/$TARGET/releasewin/velora.exe" | rg -q "Microsoft.Windows.Common-Controls" || {
    echo "错误：exe 未嵌入 Common-Controls manifest，拒绝打包" >&2
    exit 1
}
makensis -DVELORA_VERSION="$VERSION" -DREPO_ROOT="$REPO_ROOT" scripts/package-windows.nsi
test -s "dist/velora-$VERSION-windows-x64-setup.exe"
echo "已生成：$REPO_ROOT/dist/velora-$VERSION-windows-x64-setup.exe"

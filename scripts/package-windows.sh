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
cargo build --profile fastdev --target "$TARGET"
file "target/$TARGET/fastdev/maksher.exe" | rg -q "PE32\+ executable.*x86-64.*Windows"
makensis -DMAKSHER_VERSION="$VERSION" -DREPO_ROOT="$REPO_ROOT" scripts/package-windows.nsi
test -s "dist/maksher-$VERSION-windows-x64-setup.exe"
echo "已生成：$REPO_ROOT/dist/maksher-$VERSION-windows-x64-setup.exe"

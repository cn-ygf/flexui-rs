#!/usr/bin/env bash
# 在 macOS 上用 zig 交叉编译 Windows 目标，并用 wine 无头验证 GDI+ 渲染与 FFI。
# 依赖：brew 安装的 zig / cargo-zigbuild / wine；rustup target add x86_64-pc-windows-gnu
set -euo pipefail

# 让 brew 装的工具进入 PATH（新装后当前 shell 可能未加载）。
BREW_PREFIX="$(/opt/homebrew/bin/brew --prefix 2>/dev/null || /usr/local/bin/brew --prefix)"
export PATH="$BREW_PREFIX/bin:$BREW_PREFIX/sbin:$PATH"

TARGET="x86_64-pc-windows-gnu"
export WINEPREFIX="${WINEPREFIX:-$HOME/.flexui-wine}"
export WINEDEBUG="${WINEDEBUG:--all}"
export WINEDLLOVERRIDES="mscoree,mshtml="

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== 交叉编译（zigbuild）=="
cargo zigbuild --target "$TARGET" -p flexui-windows --example offscreen
cargo zigbuild --target "$TARGET" -p flexui-ffi --example ffi_smoke
cargo zigbuild --target "$TARGET" -p flexui --example xml_demo

BIN="target/$TARGET/debug/examples"
echo "== wine 无头验证：离屏 GDI+ 像素回读 =="
wine "$BIN/offscreen.exe"
echo "== wine 无头验证：FFI 入口 =="
wine "$BIN/ffi_smoke.exe"

echo "== 全部通过 =="

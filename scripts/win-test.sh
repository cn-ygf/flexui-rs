#!/usr/bin/env bash
# 在 macOS 上用 zig 交叉编译 Windows 目标。
# 依赖：brew 安装的 zig / cargo-zigbuild；rustup target add x86_64-pc-windows-gnu
set -euo pipefail

# 让 brew 装的工具进入 PATH（新装后当前 shell 可能未加载）。
BREW_PREFIX="$(/opt/homebrew/bin/brew --prefix 2>/dev/null || /usr/local/bin/brew --prefix)"
export PATH="$BREW_PREFIX/bin:$BREW_PREFIX/sbin:$PATH"

TARGET="x86_64-pc-windows-gnu"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== 交叉编译（zigbuild）=="
cargo zigbuild --target "$TARGET" -p flexui-windows
cargo zigbuild --target "$TARGET" -p flexui-ffi
cargo zigbuild --target "$TARGET" -p flexui

echo "== 全部通过 =="

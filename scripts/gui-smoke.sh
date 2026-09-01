#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

if [[ "$(uname -s)" == "Linux" && -z "${DISPLAY:-}" ]]; then
  if ! command -v xvfb-run >/dev/null 2>&1; then
    echo "Linux 无 DISPLAY；请先安装 xvfb，再运行本脚本。" >&2
    exit 2
  fi
  exec xvfb-run -a cargo run -p flexui --bin window_lifecycle_smoke
fi

exec cargo run -p flexui --bin window_lifecycle_smoke

#!/usr/bin/env bash
# =============================================================================
# x-hub macOS 构建脚本
#
# 依赖（一次性）：
#   - Node.js 18+          https://nodejs.org
#   - Rust 1.77.2+        https://rustup.rs  （curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh）
#   - Xcode Command Line Tools  （xcode-select --install）
#
# 用法：
#   ./scripts/build-mac.sh            # 构建 .app + .dmg（默认）
#   ./scripts/build-mac.sh --dev      # 以开发窗口模式运行（tauri dev）
#   ./scripts/build-mac.sh --test     # 运行 Rust 单元测试（cargo test，无需 GUI）
# =============================================================================
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-build}"

require() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "[✗] 缺少 $cmd"
    return 1
  fi
  echo "[✓] $cmd 已安装"
}

echo "==> 检查环境依赖 =="
missing=0
require node || missing=1
require npm || missing=1
require rustc || missing=1
require cargo || missing=1
if ! xcode-select -p >/dev/null 2>&1; then
  echo "[✗] 缺少 Xcode Command Line Tools（请运行：xcode-select --install）"
  missing=1
else
  echo "[✓] Xcode Command Line Tools 已安装"
fi
if [ "$missing" -ne 0 ]; then
  echo ""
  echo "请先安装缺失依赖后再运行本脚本。"
  exit 1
fi

echo ""
echo "==> 安装前端依赖 =="
npm install

case "$MODE" in
  dev|--dev)
    echo "==> 启动开发窗口（tauri dev）=="
    npm run tauri:dev
    ;;
  test|--test)
    echo "==> 运行 Rust 单元测试 =="
    (cd src-tauri && cargo test)
    ;;
  *)
    echo "==> 构建 macOS 安装包（.app + .dmg）=="
    npm run tauri:build
    # 版本号从 package.json 动态读取（跟随上游同步，勿在此写死）
    VER=$(node -p "require('./package.json').version")
    echo ""
    echo "==> 完成（v$VER）。产物位于：src-tauri/target/release/bundle/"
    echo "     .app：src-tauri/target/release/bundle/macos/x-hub.app"
    echo "     .dmg：src-tauri/target/release/bundle/dmg/x-hub_${VER}_aarch64.dmg"
    ;;
esac
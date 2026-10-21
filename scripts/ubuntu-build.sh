#!/usr/bin/env bash
# Ubuntu 24.04 本地拉取、编译、安装、运行 x-hub（feat/linux-support 测试机用）
set -euo pipefail

REPO_URL="${REPO_URL:-https://github.com/inkchills/x-hub.git}"
BRANCH="${BRANCH:-feat/linux-support}"
WORKDIR="${WORKDIR:-${HOME}/x-hub-build}"

DO_PULL=true
DO_DEPS=true
DO_DEB=true
DO_DEV=false
DO_INSTALL=false
DO_RUN=false

usage() {
  cat <<'EOF'
用法: scripts/ubuntu-build.sh [选项]

  --deb          打 .deb 包（默认，pnpm run tauri:build:deb）
  --dev          开发模式（pnpm run tauri:dev）
  --install      用 apt 安装刚打好的 .deb（需先 --deb 或已有产物）
  --run          启动 x-hub（安装后一般可直接 x-hub）
  --skip-pull    跳过 git fetch/checkout/pull
  --skip-deps    跳过 apt 系统依赖安装
  --branch NAME  检出分支（默认 feat/linux-support）
  --dir PATH     克隆/工作目录（默认 ~/x-hub-build）
  --help         显示本帮助

环境变量: REPO_URL、BRANCH、WORKDIR（与 --branch/--dir 等效时可覆盖默认值）

示例:
  scripts/ubuntu-build.sh
  scripts/ubuntu-build.sh --install --run
  scripts/ubuntu-build.sh --dev --dir ~/src/x-hub --skip-deps
EOF
}

log() { printf '[ubuntu-build] %s\n' "$*"; }
die() { printf '[ubuntu-build] 错误: %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --deb) DO_DEB=true; DO_DEV=false; shift ;;
    --dev) DO_DEV=true; DO_DEB=false; shift ;;
    --install) DO_INSTALL=true; shift ;;
    --run) DO_RUN=true; shift ;;
    --skip-pull) DO_PULL=false; shift ;;
    --skip-deps) DO_DEPS=false; shift ;;
    --branch)
      [[ $# -ge 2 ]] || die "--branch 需要分支名"
      BRANCH="$2"
      shift 2
      ;;
    --dir)
      [[ $# -ge 2 ]] || die "--dir 需要目录路径"
      WORKDIR="$2"
      shift 2
      ;;
    --help|-h) usage; exit 0 ;;
    *) die "未知参数: $1（用 --help 查看）" ;;
  esac
done

APT_PACKAGES=(
  libwebkit2gtk-4.1-dev
  build-essential
  curl
  wget
  file
  libxdo-dev
  libssl-dev
  libayatana-appindicator3-dev
  librsvg2-dev
  pkg-config
  libgtk-3-dev
  libsecret-1-dev
  patchelf
  git
)

install_apt_deps() {
  log "安装 apt 依赖（与 release.yml 一致 + git）…"
  sudo apt-get update
  sudo apt-get install -y "${APT_PACKAGES[@]}"
}

require_node() {
  if command -v node >/dev/null 2>&1; then
    local major
    major="$(node -p "process.versions.node.split('.')[0]")"
    if [[ "$major" -lt 18 ]]; then
      die "Node 版本过旧 ($(node -v))，请安装 Node.js 20：https://nodejs.org/ 或 nvm install 20"
    fi
    return 0
  fi
  die "未找到 node。请安装 Node.js 20（https://nodejs.org/ 或 nvm），本脚本不会静默安装 Node。"
}

require_pnpm() {
  if command -v pnpm >/dev/null 2>&1; then
    return 0
  fi
  if ! command -v corepack >/dev/null 2>&1; then
    die "未找到 pnpm，且 corepack 不可用。请先安装 Node.js 20 并启用 corepack：corepack enable && corepack prepare pnpm@9.15.9 --activate"
  fi
  log "通过 corepack 启用 pnpm…"
  corepack enable
  corepack prepare pnpm@9.15.9 --activate
  command -v pnpm >/dev/null 2>&1 || die "corepack 启用后仍找不到 pnpm，请手动安装 pnpm 9+"
}

require_rust() {
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi
  die "未找到 Rust 工具链。请安装 stable：https://rustup.rs/ （curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh），本脚本不会静默安装 Rust。"
}

sync_repo() {
  if [[ ! -d "$WORKDIR/.git" ]]; then
    log "克隆 $REPO_URL → $WORKDIR（分支 $BRANCH）…"
    git clone --branch "$BRANCH" --single-branch "$REPO_URL" "$WORKDIR"
    return 0
  fi
  log "更新已有仓库 $WORKDIR …"
  git -C "$WORKDIR" fetch origin "$BRANCH"
  git -C "$WORKDIR" checkout "$BRANCH"
  git -C "$WORKDIR" pull --ff-only origin "$BRANCH"
}

find_deb() {
  local deb
  deb="$(find "$WORKDIR/src-tauri/target/release/bundle/deb" -maxdepth 1 -name 'x-hub_*.deb' -print 2>/dev/null | head -1)"
  [[ -n "$deb" && -f "$deb" ]] || die "未找到 .deb，请先执行 --deb 或确认路径: $WORKDIR/src-tauri/target/release/bundle/deb/"
  printf '%s\n' "$deb"
}

install_deb() {
  local deb
  deb="$(find_deb)"
  log "安装: $deb"
  sudo apt install -y "$deb"
}

run_app() {
  if command -v x-hub >/dev/null 2>&1; then
    log "启动 x-hub…"
    exec x-hub
  fi
  local bin="$WORKDIR/src-tauri/target/release/x-hub"
  if [[ -x "$bin" ]]; then
    log "启动未安装的二进制: $bin"
    exec "$bin"
  fi
  die "找不到 x-hub（请先 --install 或 --deb 编译）"
}

main() {
  if [[ "$DO_DEV" == true && "$DO_DEB" == true ]]; then
    : # --dev 已关闭 DO_DEB
  fi

  if [[ "$DO_INSTALL" == true || "$DO_RUN" == true ]] && [[ "$DO_DEB" == false && "$DO_DEV" == false && "$DO_PULL" == false && "$DO_DEPS" == false ]]; then
    [[ "$DO_INSTALL" == true ]] && install_deb
    [[ "$DO_RUN" == true ]] && run_app
    exit 0
  fi

  [[ "$DO_DEPS" == true ]] && install_apt_deps

  require_node
  require_pnpm
  require_rust

  [[ "$DO_PULL" == true ]] && sync_repo

  cd "$WORKDIR"
  log "pnpm install --frozen-lockfile …"
  pnpm install --frozen-lockfile

  if [[ "$DO_DEB" == true ]]; then
    log "pnpm run tauri:build:deb …"
    pnpm run tauri:build:deb
  fi

  if [[ "$DO_DEV" == true ]]; then
    log "pnpm run tauri:dev …"
    exec pnpm run tauri:dev
  fi

  [[ "$DO_INSTALL" == true ]] && install_deb
  [[ "$DO_RUN" == true ]] && run_app

  if [[ "$DO_DEB" == true ]]; then
    deb="$(find_deb)"
    log "完成。deb: $deb"
  else
    log "完成。"
  fi
}

main "$@"

#!/usr/bin/env bash
# 拉取仓库后在目标机安装 x-hub：判断 deb/rpm → 装运行时依赖 → 下载对应包 → 安装。
# 默认拉 inkchills/x-hub 上带 -linux. 的 GitHub Release（预发布即可，不必是 draft）。
set -euo pipefail

REPO="${REPO:-inkchills/x-hub}"
TAG="${TAG:-}"
PKG="${PKG:-}"
SKIP_DEPS=false
DOWNLOAD_DIR="${DOWNLOAD_DIR:-${TMPDIR:-/tmp}/x-hub-pkg}"

usage() {
  cat <<'EOF'
用法: scripts/install-linux.sh [选项]

  --repo owner/name   GitHub 仓库（默认 inkchills/x-hub）
  --tag TAG           指定 Release tag（默认取最新的 v*-linux.*）
  --pkg deb|rpm       强制包格式（默认读 /etc/os-release）
  --skip-deps         不装系统运行时依赖
  --dir PATH          下载目录（默认 $TMPDIR/x-hub-pkg）
  --help              显示本帮助

环境变量: REPO、TAG、PKG、DOWNLOAD_DIR、GH_TOKEN / GITHUB_TOKEN（读 draft 或提高 API 限额）

示例:
  scripts/install-linux.sh
  scripts/install-linux.sh --tag v0.7.0-linux.1b4a7cb
  GH_TOKEN=… scripts/install-linux.sh
EOF
}

log() { printf '[install-linux] %s\n' "$*"; }
die() { printf '[install-linux] 错误: %s\n' "$*" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "缺少命令: $1"
}

sudo_run() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  else
    need_cmd sudo
    sudo "$@"
  fi
}

detect_pkg() {
  local id="" like=""
  if [[ -r /etc/os-release ]]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    id="${ID:-}"
    like="${ID_LIKE:-}"
  fi
  local blob
  blob="$(printf '%s %s' "$id" "$like" | tr '[:upper:]' '[:lower:]')"
  case "$blob" in
    *debian*|*ubuntu*|*linuxmint*|*pop*|*elementary*|*raspbian*)
      echo deb
      ;;
    *fedora*|*rhel*|*centos*|*rocky*|*alma*|*suse*|*opensuse*|*mandriva*)
      echo rpm
      ;;
    *)
      if command -v apt-get >/dev/null 2>&1 || command -v apt >/dev/null 2>&1; then
        echo deb
      elif command -v dnf >/dev/null 2>&1 || command -v rpm >/dev/null 2>&1 || command -v zypper >/dev/null 2>&1; then
        echo rpm
      else
        die "无法判断包格式（ID=${id:-?} ID_LIKE=${like:-?}），请加 --pkg deb 或 --pkg rpm"
      fi
      ;;
  esac
}

auth_header() {
  local token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
  if [[ -n "$token" ]]; then
    printf 'Authorization: Bearer %s' "$token"
  fi
}

api_get() {
  local url="$1"
  local hdr=()
  hdr+=(-H "Accept: application/vnd.github+json")
  hdr+=(-H "User-Agent: x-hub-install-linux")
  local auth
  auth="$(auth_header)"
  if [[ -n "$auth" ]]; then
    hdr+=(-H "$auth")
  fi
  curl -fsSL "${hdr[@]}" "$url"
}

latest_linux_tag() {
  local json
  json="$(api_get "https://api.github.com/repos/${REPO}/releases?per_page=30")"
  python3 - "$json" <<'PY'
import json,sys
raw=sys.argv[1]
try:
    data=json.loads(raw)
except json.JSONDecodeError:
    sys.exit(1)
tags=[]
for r in data:
    tag=r.get("tag_name") or ""
    if "-linux." not in tag:
        continue
    tags.append(tag)
if not tags:
    sys.exit(2)
print(tags[0])
PY
}

asset_url_for() {
  local tag="$1" suffix="$2"
  local json
  json="$(api_get "https://api.github.com/repos/${REPO}/releases/tags/${tag}")"
  python3 - "$json" "$suffix" <<'PY'
import json,sys
rel=json.loads(sys.argv[1])
suffix=sys.argv[2]
for a in rel.get("assets") or []:
    name=a.get("name") or ""
    if name.endswith(suffix):
        print(a.get("browser_download_url") or "")
        print(name)
        sys.exit(0)
sys.exit(3)
PY
}

install_deb_deps() {
  sudo_run apt-get update
  sudo_run apt-get install -y \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    libayatana-appindicator3-1 \
    librsvg2-2
}

install_rpm_deps() {
  if command -v dnf >/dev/null 2>&1; then
    sudo_run dnf install -y webkit2gtk4.1 gtk3 librsvg2 || true
    sudo_run dnf install -y libayatana-appindicator3-gtk3 \
      || sudo_run dnf install -y libappindicator-gtk3 \
      || log "未装上托盘指示器库，可稍后手动安装"
  elif command -v zypper >/dev/null 2>&1; then
    sudo_run zypper --non-interactive install \
      libwebkit2gtk-4_1-0 libayatana-appindicator3-1 librsvg-2-2
  else
    die "未找到 dnf/zypper，无法自动装 RPM 依赖"
  fi
}

install_deb_pkg() {
  local file="$1"
  if command -v apt-get >/dev/null 2>&1; then
    sudo_run apt-get install -y "$file"
  else
    sudo_run dpkg -i "$file" || sudo_run apt-get install -f -y
  fi
}

install_rpm_pkg() {
  local file="$1"
  if command -v dnf >/dev/null 2>&1; then
    sudo_run dnf install -y "$file"
  elif command -v zypper >/dev/null 2>&1; then
    sudo_run zypper --non-interactive install "$file"
  else
    sudo_run rpm -Uvh "$file"
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      [[ $# -ge 2 ]] || die "--repo 需要 owner/name"
      REPO="$2"; shift 2 ;;
    --tag)
      [[ $# -ge 2 ]] || die "--tag 需要 tag 名"
      TAG="$2"; shift 2 ;;
    --pkg)
      [[ $# -ge 2 ]] || die "--pkg 需要 deb 或 rpm"
      PKG="$2"; shift 2 ;;
    --skip-deps) SKIP_DEPS=true; shift ;;
    --dir)
      [[ $# -ge 2 ]] || die "--dir 需要目录"
      DOWNLOAD_DIR="$2"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) die "未知参数: $1（用 --help 查看）" ;;
  esac
done

need_cmd curl
need_cmd python3

if [[ -z "$PKG" ]]; then
  PKG="$(detect_pkg)"
fi
[[ "$PKG" == "deb" || "$PKG" == "rpm" ]] || die "--pkg 只能是 deb 或 rpm，收到: $PKG"

log "仓库=${REPO}"
log "包格式=${PKG}"

if [[ "$SKIP_DEPS" == false ]]; then
  log "安装运行时依赖…"
  if [[ "$PKG" == "deb" ]]; then
    need_cmd apt-get
    install_deb_deps
  else
    install_rpm_deps
  fi
else
  log "跳过系统依赖"
fi

if [[ -z "$TAG" ]]; then
  log "查询最新 Linux Release…"
  TAG="$(latest_linux_tag)" || die "没有找到 tag 含 -linux. 的 Release（若仍是 draft，请设 GH_TOKEN 或先发布预发布）"
fi
log "使用 tag=${TAG}"

suffix=""
if [[ "$PKG" == "deb" ]]; then
  suffix=".deb"
else
  suffix=".rpm"
fi

pair="$(asset_url_for "$TAG" "$suffix")" || die "Release ${TAG} 上没有 ${suffix} 附件。deb 机用 .deb，rpm 机需要工作流同时上传 .rpm。"
url="$(printf '%s\n' "$pair" | sed -n '1p')"
name="$(printf '%s\n' "$pair" | sed -n '2p')"
[[ -n "$url" && -n "$name" ]] || die "解析附件失败"

mkdir -p "$DOWNLOAD_DIR"
dest="${DOWNLOAD_DIR}/${name}"
log "下载 ${name}…"
curl_hdr=()
auth="$(auth_header)"
if [[ -n "$auth" ]]; then
  curl_hdr+=(-H "$auth")
fi
curl -fL --retry 3 "${curl_hdr[@]}" -o "$dest" "$url"
log "已保存 ${dest}"

log "安装 ${name}…"
if [[ "$PKG" == "deb" ]]; then
  install_deb_pkg "$dest"
else
  install_rpm_pkg "$dest"
fi

if command -v x-hub >/dev/null 2>&1; then
  log "安装完成，可执行: x-hub"
else
  log "包已装上，若 PATH 里还没有 x-hub，重新打开终端后再试"
fi

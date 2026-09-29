#!/usr/bin/env bash
# Detect the distro package format, install runtime dependencies, and install
# the latest Linux prerelease from inkchills/x-hub.
set -euo pipefail

REPO="${REPO:-inkchills/x-hub}"
TAG="${TAG:-}"
PKG="${PKG:-}"
SKIP_DEPS=false
DOWNLOAD_ONLY=false
DOWNLOAD_DIR="${DOWNLOAD_DIR:-${TMPDIR:-/tmp}/x-hub-pkg}"
OS_RELEASE_FILE="${OS_RELEASE_FILE:-/etc/os-release}"
PROXY="${XHUB_PROXY:-${HTTPS_PROXY:-${https_proxy:-}}}"
GITHUB_MIRROR="${GITHUB_MIRROR:-}"

usage() {
  cat <<'EOF'
Usage: scripts/install-linux.sh [options]

  --repo owner/name   GitHub repository (default: inkchills/x-hub)
  --tag TAG           Release tag (default: latest Linux prerelease)
  --pkg deb|rpm       Force package format (default: detect distro)
  --skip-deps         Do not install runtime dependencies
  --download-only     Download and verify the package without installing
  --proxy URL         HTTP/SOCKS proxy for GitHub requests (or set HTTPS_PROXY)
  --mirror URL        GitHub URL prefix, e.g. https://gh.inkchills.cn
  --dir PATH          Download directory (default: $TMPDIR/x-hub-pkg)
  --help              Show this help

Environment: REPO, TAG, PKG, DOWNLOAD_DIR, GH_TOKEN, GITHUB_TOKEN,
              HTTPS_PROXY, XHUB_PROXY, GITHUB_MIRROR

Examples:
  scripts/install-linux.sh
  scripts/install-linux.sh --tag v0.7.0-linux.1fefdc5
  scripts/install-linux.sh --skip-deps --download-only
EOF
}

log() { printf '[install-linux] %s\n' "$*"; }
die() { printf '[install-linux] Error: %s\n' "$*" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "Missing command: $1"
}

sudo_run() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  else
    need_cmd sudo
    if [[ -n "$PROXY" ]]; then
      sudo env \
        "http_proxy=$PROXY" "https_proxy=$PROXY" \
        "HTTP_PROXY=$PROXY" "HTTPS_PROXY=$PROXY" \
        "$@"
    else
      sudo "$@"
    fi
  fi
}

apt_run() {
  local apt_proxy_options=()
  if [[ -n "$PROXY" ]]; then
    apt_proxy_options+=(-o "Acquire::http::Proxy=$PROXY" -o "Acquire::https::Proxy=$PROXY")
  fi
  sudo_run apt-get "${apt_proxy_options[@]}" "$@"
}

dnf_run() {
  local dnf_proxy_options=()
  if [[ -n "$PROXY" ]]; then
    dnf_proxy_options+=(--setopt="proxy=$PROXY")
  fi
  sudo_run dnf "${dnf_proxy_options[@]}" "$@"
}

dnf_proxy_option() {
  if [[ -n "$PROXY" ]]; then
    printf -- '--setopt=proxy=%s\n' "$PROXY"
  fi
}

detect_pkg() {
  local source_file="${1:-$OS_RELEASE_FILE}"
  local id="" like="" blob=""
  if [[ -r "$source_file" ]]; then
    # shellcheck disable=SC1090
    . "$source_file"
    id="${ID:-}"
    like="${ID_LIKE:-}"
  fi
  blob="$(printf '%s %s' "$id" "$like" | tr '[:upper:]' '[:lower:]')"

  case "$blob" in
    *debian*|*ubuntu*|*linuxmint*|*pop*|*elementary*|*raspbian*)
      printf 'deb\n'
      ;;
    *fedora*|*rhel*|*centos*|*rocky*|*alma*|*suse*|*opensuse*|*mandriva*)
      printf 'rpm\n'
      ;;
    *)
      if command -v apt-get >/dev/null 2>&1 || command -v apt >/dev/null 2>&1; then
        printf 'deb\n'
      elif command -v dnf >/dev/null 2>&1 || command -v zypper >/dev/null 2>&1 || command -v rpm >/dev/null 2>&1; then
        printf 'rpm\n'
      else
        die "Cannot detect package format (ID=${id:-?} ID_LIKE=${like:-?}); use --pkg deb or --pkg rpm"
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
  local url
  url="$(github_url "$1")"
  local headers=(-H 'Accept: application/vnd.github+json' -H 'User-Agent: x-hub-install-linux')
  local curl_args=(-fsSL --retry 3)
  local auth
  if [[ -n "$PROXY" ]]; then
    curl_args+=(--proxy "$PROXY")
  fi
  auth="$(auth_header)"
  if [[ -n "$auth" ]]; then
    headers+=(-H "$auth")
  fi
  curl "${curl_args[@]}" "${headers[@]}" "$url"
}

github_url() {
  local url="$1"
  if [[ -z "$GITHUB_MIRROR" ]]; then
    printf '%s\n' "$url"
  else
    printf '%s/%s\n' "${GITHUB_MIRROR%/}" "$url"
  fi
}

select_latest_linux_tag() {
  python3 -c '
import json
import sys

try:
    releases = json.load(sys.stdin)
except json.JSONDecodeError as exc:
    print(f"invalid GitHub releases JSON: {exc}", file=sys.stderr)
    sys.exit(1)

matches = [
    release for release in releases
    if release.get("prerelease") is True
    and release.get("draft") is not True
    and "-linux." in (release.get("tag_name") or "")
]
if not matches:
    print("no Linux prerelease found", file=sys.stderr)
    sys.exit(1)

matches.sort(key=lambda release: release.get("published_at") or release.get("created_at") or "", reverse=True)
print(matches[0]["tag_name"])
'
}

asset_suffix() {
  local package="$1" architecture="$2"
  case "$package:$architecture" in
    deb:x86_64) printf '_amd64.deb\n' ;;
    deb:aarch64) printf '_arm64.deb\n' ;;
    rpm:x86_64) printf '.x86_64.rpm\n' ;;
    rpm:aarch64) printf '.aarch64.rpm\n' ;;
    *) die "Unsupported package/architecture: $package/$architecture" ;;
  esac
}

select_asset() {
  local package="$1" architecture="$2"
  python3 -c '
import json
import re
import sys

package, architecture = sys.argv[1:]
suffixes = {
    ("deb", "x86_64"): "_amd64.deb",
    ("deb", "aarch64"): "_arm64.deb",
    ("rpm", "x86_64"): ".x86_64.rpm",
    ("rpm", "aarch64"): ".aarch64.rpm",
}
suffix = suffixes.get((package, architecture))
if suffix is None:
    print(f"unsupported package/architecture: {package}/{architecture}", file=sys.stderr)
    sys.exit(1)

try:
    release = json.load(sys.stdin)
except json.JSONDecodeError as exc:
    print(f"invalid GitHub release JSON: {exc}", file=sys.stderr)
    sys.exit(1)

matches = [asset for asset in release.get("assets", []) if (asset.get("name") or "").endswith(suffix)]
if len(matches) != 1:
    print(f"expected one {suffix} asset, found {len(matches)}", file=sys.stderr)
    sys.exit(1)

asset = matches[0]
name = asset.get("name") or ""
url = asset.get("browser_download_url") or ""
digest = asset.get("digest") or ""
if not name or name != name.rsplit("/", 1)[-1] or ".." in name:
    print("invalid asset filename", file=sys.stderr)
    sys.exit(1)
if not url.startswith("https://"):
    print("asset URL is not HTTPS", file=sys.stderr)
    sys.exit(1)
if not re.fullmatch(r"sha256:[0-9a-fA-F]{64}", digest):
    print(f"asset {name} has no valid SHA-256 digest", file=sys.stderr)
    sys.exit(1)

print(f"{url}\t{name}\t{digest}")
' "$package" "$architecture"
}

install_deb_deps() {
  apt_run update
  apt_run install -y \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    libayatana-appindicator3-1 \
    librsvg2-2
}

install_rpm_deps() {
  if command -v dnf >/dev/null 2>&1; then
    dnf_run install -y webkit2gtk4.1 gtk3 librsvg2
    if ! dnf_run install -y libayatana-appindicator-gtk3; then
      if ! dnf_run install -y libayatana-appindicator3-gtk3; then
        if ! dnf_run install -y libappindicator-gtk3; then
          log 'Warning: AppIndicator package was unavailable; tray integration may not work.'
        fi
      fi
    fi
  elif command -v zypper >/dev/null 2>&1; then
    sudo_run zypper --non-interactive install \
      libwebkit2gtk-4_1-0 libayatana-appindicator3-1 librsvg-2-2
  else
    die 'Cannot install RPM runtime dependencies: dnf or zypper is required'
  fi
}

install_deb_pkg() {
  local file="$1"
  if command -v apt-get >/dev/null 2>&1; then
    apt_run install -y "$file"
  else
    sudo_run dpkg -i "$file" || sudo_run apt-get install -f -y
  fi
}

install_rpm_pkg() {
  local file="$1"
  if command -v dnf >/dev/null 2>&1; then
    dnf_run install -y "$file"
  elif command -v zypper >/dev/null 2>&1; then
    sudo_run zypper --non-interactive install "$file"
  else
    sudo_run rpm -Uvh "$file"
  fi
}

main() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --repo)
        [[ $# -ge 2 ]] || die '--repo requires owner/name'
        REPO="$2"; shift 2 ;;
      --tag)
        [[ $# -ge 2 ]] || die '--tag requires a tag name'
        TAG="$2"; shift 2 ;;
      --pkg)
        [[ $# -ge 2 ]] || die '--pkg requires deb or rpm'
        PKG="$2"; shift 2 ;;
      --skip-deps) SKIP_DEPS=true; shift ;;
      --download-only) DOWNLOAD_ONLY=true; shift ;;
      --proxy)
        [[ $# -ge 2 ]] || die '--proxy requires a URL'
        PROXY="$2"; shift 2 ;;
      --mirror)
        [[ $# -ge 2 ]] || die '--mirror requires a URL'
        GITHUB_MIRROR="$2"; shift 2 ;;
      --dir)
        [[ $# -ge 2 ]] || die '--dir requires a path'
        DOWNLOAD_DIR="$2"; shift 2 ;;
      --help|-h) usage; return 0 ;;
      *) die "Unknown option: $1 (use --help)" ;;
    esac
  done

  [[ "$REPO" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || die "Invalid repository: $REPO"
  if [[ -n "$TAG" ]]; then
    [[ "$TAG" =~ ^[A-Za-z0-9._-]+$ ]] || die 'Invalid release tag'
  fi
  if [[ -n "$PROXY" && ! "$PROXY" =~ ^(https?|socks5h?)://[^[:space:]]+$ ]]; then
    die 'Proxy URL must use http://, https://, socks5://, or socks5h://'
  fi
  if [[ -n "$GITHUB_MIRROR" && ! "$GITHUB_MIRROR" =~ ^https://[^[:space:]]+$ ]]; then
    die 'GitHub mirror URL must use HTTPS'
  fi
  if [[ -n "$GITHUB_MIRROR" && ( -n "${GH_TOKEN:-}" || -n "${GITHUB_TOKEN:-}" ) ]]; then
    die 'Do not send GitHub tokens through a mirror; unset GITHUB_MIRROR to use a token'
  fi
  if [[ -z "$PKG" ]]; then
    PKG="$(detect_pkg)"
  fi
  [[ "$PKG" == deb || "$PKG" == rpm ]] || die "--pkg must be deb or rpm, got: $PKG"

  need_cmd curl
  need_cmd python3
  need_cmd sha256sum

  local architecture suffix release_json release_json_url asset_data asset_url asset_name expected_digest destination temp_file actual_digest
  architecture="$(uname -m)"
  suffix="$(asset_suffix "$PKG" "$architecture")"
  log "Repository=$REPO package=$PKG architecture=$architecture"

  if [[ -z "$TAG" ]]; then
    log 'Querying the latest Linux prerelease...'
    release_json="$(api_get "https://api.github.com/repos/$REPO/releases?per_page=100")" \
      || die 'Failed to query GitHub releases'
    TAG="$(printf '%s' "$release_json" | select_latest_linux_tag)" \
      || die 'No published Linux prerelease found'
  fi
  log "Using release $TAG"

  release_json_url="https://api.github.com/repos/$REPO/releases/tags/$TAG"
  release_json="$(api_get "$release_json_url")" || die "Failed to query release $TAG"
  asset_data="$(printf '%s' "$release_json" | select_asset "$PKG" "$architecture")" \
    || die "Release $TAG has no valid $PKG asset for $architecture"
  IFS=$'\t' read -r asset_url asset_name expected_digest <<< "$asset_data"
  [[ -n "$asset_url" && -n "$asset_name" && -n "$expected_digest" ]] || die 'Failed to parse release asset metadata'
  asset_url="$(github_url "$asset_url")"

  mkdir -p "$DOWNLOAD_DIR"
  destination="$DOWNLOAD_DIR/$asset_name"
  if [[ -f "$destination" ]]; then
    actual_digest="sha256:$(sha256sum "$destination" | awk '{print $1}')"
    [[ "${actual_digest,,}" == "${expected_digest,,}" ]] \
      || die "Existing file has a different digest: $destination"
    log "Reusing verified download $destination"
  else
    temp_file="$destination.part.$$"
    trap 'rm -f "${temp_file:-}"' EXIT
    log "Downloading $asset_name..."
    curl_args=(-fL --retry 3)
    if [[ -n "$PROXY" ]]; then
      curl_args+=(--proxy "$PROXY")
    fi
    auth="$(auth_header)"
    if [[ -n "$auth" ]]; then
      curl_args+=(-H "$auth")
    fi
    curl "${curl_args[@]}" -o "$temp_file" "$asset_url" || die 'Package download failed'
    actual_digest="sha256:$(sha256sum "$temp_file" | awk '{print $1}')"
    [[ "${actual_digest,,}" == "${expected_digest,,}" ]] || die 'Downloaded package SHA-256 does not match GitHub metadata'
    mv "$temp_file" "$destination"
    trap - EXIT
    log "Verified download: $destination"
  fi

  if [[ "$DOWNLOAD_ONLY" == true ]]; then
    log 'Download-only mode; no system changes were made.'
    return 0
  fi

  if [[ "$SKIP_DEPS" == false ]]; then
    log 'Installing documented runtime dependencies...'
    if [[ "$PKG" == deb ]]; then
      need_cmd apt-get
      install_deb_deps
    else
      install_rpm_deps
    fi
  else
    log 'Skipping runtime dependencies.'
  fi

  log "Installing $asset_name..."
  if [[ "$PKG" == deb ]]; then
    install_deb_pkg "$destination"
  else
    install_rpm_pkg "$destination"
  fi

  if command -v x-hub >/dev/null 2>&1; then
    log 'Installation complete; run x-hub.'
  else
    log 'Package installed. Reopen the terminal if x-hub is not yet on PATH.'
  fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
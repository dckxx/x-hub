#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="${INSTALLER_PATH:-$ROOT/scripts/install-linux.sh}"

if [[ ! -f "$INSTALLER" ]]; then
  printf 'FAIL: installer not found: %s\n' "$INSTALLER" >&2
  exit 1
fi

source "$INSTALLER"

assert_eq() {
  local expected="$1" actual="$2" description="$3"
  if [[ "$actual" != "$expected" ]]; then
    printf 'FAIL: %s\n  expected: %s\n  actual:   %s\n' "$description" "$expected" "$actual" >&2
    exit 1
  fi
  printf 'PASS: %s\n' "$description"
}

assert_fails() {
  local description="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    printf 'FAIL: %s should fail\n' "$description" >&2
    exit 1
  fi
  printf 'PASS: %s\n' "$description"
}

assert_eq deb "$(detect_pkg <(printf 'ID=ubuntu\nID_LIKE=debian\n'))" 'detects DEB family'
assert_eq rpm "$(detect_pkg <(printf 'ID=fedora\n'))" 'detects Fedora RPM'
assert_eq rpm "$(detect_pkg <(printf 'ID=rocky\nID_LIKE="rhel centos fedora"\n'))" 'detects RPM via ID_LIKE'

release_json='[{"tag_name":"v0.7.2","prerelease":false,"published_at":"2026-09-29T00:00:00Z"},{"tag_name":"v0.7.0-linux.old","prerelease":true,"published_at":"2026-09-28T00:00:00Z"},{"tag_name":"v0.7.0-linux.new","prerelease":true,"published_at":"2026-09-29T00:00:00Z"}]'
assert_eq 'v0.7.0-linux.new' "$(printf '%s' "$release_json" | select_latest_linux_tag)" 'selects newest Linux prerelease, not stable release'
assert_eq 'https://api.github.com/repos/inkchills/x-hub/releases' "$(github_url 'https://api.github.com/repos/inkchills/x-hub/releases')" 'keeps direct GitHub URL when mirror is unset'
GITHUB_MIRROR='https://gh.inkchills.cn'
assert_eq 'https://gh.inkchills.cn/https://api.github.com/repos/inkchills/x-hub/releases' \
  "$(github_url 'https://api.github.com/repos/inkchills/x-hub/releases')" 'prefixes GitHub URL with configured mirror'
GITHUB_MIRROR=''
PROXY='http://186.2.10.204:3128'
assert_eq '--setopt=proxy=http://186.2.10.204:3128' "$(dnf_proxy_option)" 'configures DNF to use the selected proxy'
PROXY=''

asset_json='{"assets":[{"name":"v0.7.0-linux.x86_64.rpm","browser_download_url":"https://example.test/app.rpm","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},{"name":"v0.7.0-linux_amd64.deb","browser_download_url":"https://example.test/app.deb","digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},{"name":"v0.7.0-linux.aarch64.rpm","browser_download_url":"https://example.test/arm.rpm","digest":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}]}'
assert_eq $'https://example.test/app.deb\tv0.7.0-linux_amd64.deb\tsha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' \
  "$(printf '%s' "$asset_json" | select_asset deb x86_64)" 'selects matching DEB architecture and digest'
assert_eq $'https://example.test/app.rpm\tv0.7.0-linux.x86_64.rpm\tsha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
  "$(printf '%s' "$asset_json" | select_asset rpm x86_64)" 'selects matching RPM architecture and digest'
missing_digest_json='{"assets":[{"name":"x_amd64.deb","browser_download_url":"https://example.test/x.deb"}]}'
if printf '%s' "$missing_digest_json" | select_asset deb x86_64 >/dev/null 2>&1; then
  printf 'FAIL: accepted asset without SHA-256\n' >&2
  exit 1
fi
printf 'PASS: rejects asset without SHA-256\n'

help_output="$(bash "$INSTALLER" --help)"
[[ "$help_output" == *'--download-only'* ]] || { printf 'FAIL: help omits --download-only\n' >&2; exit 1; }
printf 'PASS: help lists --download-only\n'
[[ "$help_output" == *'--proxy URL'* ]] || { printf 'FAIL: help omits --proxy URL\n' >&2; exit 1; }
printf 'PASS: help lists --proxy URL\n'
[[ "$help_output" == *'--mirror URL'* ]] || { printf 'FAIL: help omits --mirror URL\n' >&2; exit 1; }
printf 'PASS: help lists --mirror URL\n'

if invalid_proxy_output="$(bash "$INSTALLER" --pkg rpm --proxy 'file:///tmp' --download-only 2>&1)"; then
  printf 'FAIL: invalid proxy URL should fail\n' >&2
  exit 1
fi
[[ "$invalid_proxy_output" == *'Proxy URL must use'* ]] || {
  printf 'FAIL: invalid proxy URL was rejected for the wrong reason\n%s\n' "$invalid_proxy_output" >&2
  exit 1
}
printf 'PASS: rejects unsupported proxy schemes\n'

if invalid_mirror_output="$(GITHUB_MIRROR='http://gh.inkchills.cn' bash "$INSTALLER" --pkg rpm --download-only 2>&1)"; then
  printf 'FAIL: insecure mirror URL should fail\n' >&2
  exit 1
fi
[[ "$invalid_mirror_output" == *'GitHub mirror URL must use HTTPS'* ]] || {
  printf 'FAIL: insecure mirror URL was rejected for the wrong reason\n%s\n' "$invalid_mirror_output" >&2
  exit 1
}
printf 'PASS: rejects insecure GitHub mirror URL\n'

if token_mirror_output="$(GH_TOKEN=test-token GITHUB_MIRROR='https://gh.inkchills.cn' bash "$INSTALLER" --pkg rpm --download-only 2>&1)"; then
  printf 'FAIL: GitHub token through mirror should fail\n' >&2
  exit 1
fi
[[ "$token_mirror_output" == *'Do not send GitHub tokens through a mirror'* ]] || {
  printf 'FAIL: token/mirror combination was rejected for the wrong reason\n%s\n' "$token_mirror_output" >&2
  exit 1
}
printf 'PASS: prevents sending GitHub tokens through mirror\n'
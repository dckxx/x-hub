# upload-release.ps1 — 上传 dist-release 产物到 Cloudflare R2（应用升级清单 + win-x64 包 + 历史包名副本），
# 并清理 `releases/win-x64/` 下最近 2 版之外的旧 zip。
# 用法:
#   .\scripts\upload-release.ps1 -AccountId <R2_ACCOUNT_ID> -AccessKeyId <R2_ACCESS_KEY_ID> -SecretAccessKey <R2_SECRET_ACCESS_KEY>
# 或从环境变量读（GitHub Actions 同款变量名）:
#   $env:R2_ACCOUNT_ID / $env:R2_ACCESS_KEY_ID / $env:R2_SECRET_ACCESS_KEY
# 依赖: rclone (winget install --id Rclone.Rclone)

param(
  [string]$AccountId      = $env:R2_ACCOUNT_ID,
  [string]$AccessKeyId    = $env:R2_ACCESS_KEY_ID,
  [string]$SecretAccessKey = $env:R2_SECRET_ACCESS_KEY,
  [string]$Bucket         = "x-hub-dist",
  [string]$BaseUrl        = "https://r2.dckxx.com",
  [string]$DistDir        = (Join-Path $PSScriptRoot "..\dist-release"),
  [int]$KeepVersions      = 2
)

$ErrorActionPreference = "Stop"

if (-not $AccountId -or -not $AccessKeyId -or -not $SecretAccessKey) {
  Write-Error "缺少 R2 凭据。请用 -AccountId/-AccessKeyId/-SecretAccessKey 传入，或先设置 R2_ACCOUNT_ID/R2_ACCESS_KEY_ID/R2_SECRET_ACCESS_KEY 环境变量。"
}
if (-not (Test-Path (Join-Path $DistDir "update.json"))) {
  Write-Error "本地产物缺失 update.json：请先运行 publish-release.ps1（$DistDir 不存在或未生成）。"
}

# --- 1. 检查 rclone ---
if (-not (Get-Command rclone -ErrorAction SilentlyContinue)) {
  $candidates = @()
  $pkg = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter "rclone.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($pkg) { $candidates += $pkg.FullName }
  $aliasPath = Join-Path $env:LOCALAPPDATA "Microsoft\WindowsApps\rclone.exe"
  if (Test-Path $aliasPath) { $candidates += $aliasPath }
  if ($candidates.Count -gt 0) {
    $env:PATH = "$(Split-Path $candidates[0]);$env:PATH"
  } else {
    Write-Error "未找到 rclone。请先安装: winget install --id Rclone.Rclone"
  }
}

# --- 2. 用环境变量配置临时 rclone remote ---
$env:RCLONE_CONFIG_R2_TYPE            = "s3"
$env:RCLONE_CONFIG_R2_PROVIDER        = "Cloudflare"
$env:RCLONE_CONFIG_R2_ACCESS_KEY_ID   = $AccessKeyId
$env:RCLONE_CONFIG_R2_SECRET_ACCESS_KEY = $SecretAccessKey
$env:RCLONE_CONFIG_R2_ENDPOINT        = "https://$AccountId.r2.cloudflarestorage.com"

$remote = "r2:$Bucket"
$target = "$remote/releases"
Write-Host "[1/5] 校验 R2 凭据（lsd）..."
rclone lsd $remote
if ($LASTEXITCODE -ne 0) { Write-Error "rclone lsd 失败，请检查凭据/账户 ID/endpoint。" }
Write-Host "      凭据 OK" -ForegroundColor Green

Write-Host "[2/5] 上传 update.json(.sig)，Cache-Control: no-cache ..."
rclone copy $DistDir $target --include "update.json*" --header-upload "Cache-Control: no-cache" -P
if ($LASTEXITCODE -ne 0) { Write-Error "update.json 上传失败。" }
Write-Host "      update.json OK" -ForegroundColor Green

Write-Host "[3/5] 上传 win-x64/** 与 packages/**，Cache-Control: immutable ..."
rclone copy $DistDir $target --include "win-x64/**" --include "packages/**" --header-upload "Cache-Control: public, max-age=31536000, immutable" -P
if ($LASTEXITCODE -ne 0) { Write-Error "包体上传失败。" }
Write-Host "      packages OK" -ForegroundColor Green

# --- 4. 保留策略：win-x64 下只留最近 N 版的 zip（按文件名内嵌版本号排序）---
Write-Host "[4/5] 清理 win-x64 下旧版本（保留最近 $KeepVersions 版）..."
$listed = rclone lsf $target/win-x64 --files-only 2>&1
if ($LASTEXITCODE -ne 0) { Write-Error "win-x64 列目录失败。" }
$zips = @($listed | Where-Object { $_ -match '\.zip$' } | ForEach-Object { $_.Trim() } |
  Sort-Object { if ($_ -match '-(\d+\.\d+\.\d+)-') { [version]$Matches[1].ToString() } else { [version]"0.0.0" } } -Descending)
$toRemove = @($zips | Select-Object -Skip ($KeepVersions * 2))
foreach ($f in $toRemove) {
  rclone delete "$target/win-x64/$f"
  if ($LASTEXITCODE -eq 0) { Write-Host "  已清理: $f" }
}

# --- 5. 验证 HTTPS 可访问性 ---
Write-Host "[5/5] 验证 HTTPS 可访问性 ..."
foreach ($p in "update.json", "update.json.sig") {
  $url = "$BaseUrl/releases/$p"
  $r = Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 20
  "{0} -> HTTP {1} ({2} bytes)" -f $url, $r.StatusCode, $r.RawContentLength
  if ($r.StatusCode -ne 200) { Write-Error "$url 未返回 200" }
}
Write-Host "全部完成 ✔ 升级清单现已可访问: $BaseUrl/releases/update.json" -ForegroundColor Green
# upload-market.ps1 — 上传 dist-market 产物到 Cloudflare R2 桶
# 用法:
#   .\scripts\upload-market.ps1 -AccountId <R2_ACCOUNT_ID> -AccessKeyId <R2_ACCESS_KEY_ID> -SecretAccessKey <R2_SECRET_ACCESS_KEY>
# 或从环境变量读（GitHub Actions 同款变量名）:
#   $env:R2_ACCOUNT_ID / $env:R2_ACCESS_KEY_ID / $env:R2_SECRET_ACCESS_KEY
# 依赖: rclone (winget install --id Rclone.Rclone)

param(
  [string]$AccountId      = $env:R2_ACCOUNT_ID,
  [string]$AccessKeyId    = $env:R2_ACCESS_KEY_ID,
  [string]$SecretAccessKey = $env:R2_SECRET_ACCESS_KEY,
  [string]$Bucket         = "x-hub-dist",
  [string]$BaseUrl        = "https://r2.dckxx.com",
  [string]$DistDir        = (Join-Path $PSScriptRoot "..\dist-market"),
  [string]$Prefix         = "extensions"
)

$ErrorActionPreference = "Stop"

if (-not $AccountId -or -not $AccessKeyId -or -not $SecretAccessKey) {
  Write-Error "缺少 R2 凭据。请用 -AccountId/-AccessKeyId/-SecretAccessKey 传入，或先设置 R2_ACCOUNT_ID/R2_ACCESS_KEY_ID/R2_SECRET_ACCESS_KEY 环境变量。"
}

# --- 1. 检查 rclone（PATH 未生效时自动定位 winget 安装路径）---
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

# --- 2. 用环境变量配置临时 rclone remote（不落地任何 config 文件）---
$env:RCLONE_CONFIG_R2_TYPE            = "s3"
$env:RCLONE_CONFIG_R2_PROVIDER        = "Cloudflare"
$env:RCLONE_CONFIG_R2_ACCESS_KEY_ID   = $AccessKeyId
$env:RCLONE_CONFIG_R2_SECRET_ACCESS_KEY = $SecretAccessKey
$env:RCLONE_CONFIG_R2_ENDPOINT        = "https://$AccountId.r2.cloudflarestorage.com"

$remote = "r2:$Bucket"
$target = if ($Prefix) { "$remote/$Prefix" } else { $remote }
Write-Host "[1/4] 校验 R2 凭据（lsd）..."
rclone lsd $remote
if ($LASTEXITCODE -ne 0) { Write-Error "rclone lsd 失败，请检查凭据/账户 ID/endpoint。" }
Write-Host "      凭据 OK" -ForegroundColor Green

Write-Host "[2/4] 上传 registry.json(.sig)，Cache-Control: no-cache ..."
rclone copy $DistDir $target --include "registry.json*" --header-upload "Cache-Control: no-cache" -P
if ($LASTEXITCODE -ne 0) { Write-Error "registry 上传失败。" }
Write-Host "      registry OK" -ForegroundColor Green

Write-Host "[3/4] 上传 packages/** 与 icons/**，Cache-Control: immutable ..."
rclone copy $DistDir $target --include "packages/**" --include "icons/**" --header-upload "Cache-Control: public, max-age=31536000, immutable" -P
if ($LASTEXITCODE -ne 0) { Write-Error "packages/icons 上传失败。" }
Write-Host "      packages/icons OK" -ForegroundColor Green

Write-Host "[4/4] 验证 HTTPS 可访问性 ..."
$urlBase = if ($Prefix) { "$BaseUrl/$Prefix" } else { $BaseUrl }
foreach ($p in "registry.json", "registry.json.sig") {
  $url = "$urlBase/$p"
  $r = Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 20
  "{0} -> HTTP {1} ({2} bytes)" -f $url, $r.StatusCode, $r.RawContentLength
  if ($r.StatusCode -ne 200) { Write-Error "$url 未返回 200" }
}

# 强校验：下载 zip 算 sha256 与本地一致
$local = Get-ChildItem "$DistDir\packages" -Recurse -Filter *.zip | Select-Object -First 1
if ($local) {
  $hash = (Get-FileHash $local.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  $dl = Join-Path $env:TEMP "market-check-$($local.Name)"
  Invoke-WebRequest -Uri "$urlBase/packages/$($local.FullName.Substring((Resolve-Path $DistDir).Path.Length + 1).Replace('\','/'))" -OutFile $dl -UseBasicParsing -TimeoutSec 30
  $dlHash = (Get-FileHash $dl -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($hash -ne $dlHash) { Write-Error "sha256 不一致！本地 $hash vs 远端 $dlHash" }
  Remove-Item $dl -Force
  "zip sha256 校验一致: $hash" 
}
Write-Host "全部完成 ✔ 市场现已可访问: $urlBase/registry.json" -ForegroundColor Green
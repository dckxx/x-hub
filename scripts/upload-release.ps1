# upload-release.ps1 — 上传 dist-release 产物到分发端点，并清理 `releases/win-x64/` 下最近 N 版之外的旧 zip。
# 通道：-Target cos（默认，腾讯云 COS，现行唯一有效通道）/-Target sftp（备选，自建 Nginx）。
# R2 通道已于 2026-09 摘除；详见 docs/self-hosted-distribution.md §6。
# 用法:
#   .\scripts\upload-release.ps1                                 # cos → 腾讯云 COS（读 COS_* 环境变量）
# 环境变量:
#   COS_SECRET_ID / COS_SECRET_KEY / COS_BUCKET(含 APPID 后缀) / COS_REGION(如 ap-guangzhou)
#   XHUB_DEPLOY_HOST / XHUB_DEPLOY_PORT(默认22) / XHUB_DEPLOY_USER(默认deploy) / XHUB_DEPLOY_KEY
# 依赖: rclone (winget install --id Rclone.Rclone)

param(
  [ValidateSet('cos', 'sftp')][string]$Target = 'cos',

  # —— cos（腾讯云 COS，长期主通道）——
  [string]$CosSecretId  = $env:COS_SECRET_ID,
  [string]$CosSecretKey = $env:COS_SECRET_KEY,
  [string]$CosBucket    = $env:COS_BUCKET,
  [string]$CosRegion    = $env:COS_REGION,

  # —— sftp（自建 Nginx，备选）——
  [string]$SftpHost    = $env:XHUB_DEPLOY_HOST,
  [string]$SftpPort    = $env:XHUB_DEPLOY_PORT,
  [string]$SftpUser    = $env:XHUB_DEPLOY_USER,
  [string]$SftpKeyPath = $env:XHUB_DEPLOY_KEY,
  [int]$HttpPort       = 8080,
  [string]$RemoteRoot  = "/srv/x-hub-dist",


  # —— 通用 ——
  [string]$DistDir   = (Join-Path $PSScriptRoot "..\dist-release"),
  [int]$KeepVersions = 2
)

$ErrorActionPreference = "Stop"

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

# --- 2. 按通道配置临时 rclone remote（环境变量，不落地 config）+ 目标 URL ---
$noCacheHeader   = "Cache-Control: no-cache"
$immutableHeader = "Cache-Control: public, max-age=31536000, immutable"

if ($Target -eq 'cos') {
  if (-not $CosSecretId -or -not $CosSecretKey -or -not $CosBucket -or -not $CosRegion) {
    Write-Error "缺少 COS 参数。请设置 COS_SECRET_ID / COS_SECRET_KEY / COS_BUCKET（含 APPID 后缀，如 x-hub-dist-125xxxxxxx）/ COS_REGION（如 ap-guangzhou）环境变量。"
  }

  $env:RCLONE_CONFIG_COS_TYPE              = "s3"
  $env:RCLONE_CONFIG_COS_PROVIDER          = "TencentCOS"
  $env:RCLONE_CONFIG_COS_ACCESS_KEY_ID     = $CosSecretId
  $env:RCLONE_CONFIG_COS_SECRET_ACCESS_KEY = $CosSecretKey
  $env:RCLONE_CONFIG_COS_ENDPOINT          = "cos.$CosRegion.myqcloud.com"

  $remote  = "cos:$CosBucket"
  $urlBase = "https://$CosBucket.cos.$CosRegion.myqcloud.com"
  Write-Host "通道: cos → $remote/releases（缓存头随上传设置）"
} else {
  if (-not $SftpHost -or -not $SftpKeyPath) {
    Write-Error "缺少部署参数。请用 -SftpHost/-SftpKeyPath 传入，或设置 XHUB_DEPLOY_HOST / XHUB_DEPLOY_KEY 环境变量（XHUB_DEPLOY_USER 默认 deploy、XHUB_DEPLOY_PORT 默认 22）。"
  }
  if (-not ($SftpUser)) { $SftpUser = "deploy" }
  if (-not ($SftpPort)) { $SftpPort = "22" }
  if (-not (Test-Path -LiteralPath $SftpKeyPath)) { Write-Error "未找到私钥文件: $SftpKeyPath" }

  $env:RCLONE_CONFIG_XHUBSFTP_TYPE           = "sftp"
  $env:RCLONE_CONFIG_XHUBSFTP_HOST           = $SftpHost
  $env:RCLONE_CONFIG_XHUBSFTP_PORT           = $SftpPort
  $env:RCLONE_CONFIG_XHUBSFTP_USER           = $SftpUser
  $env:RCLONE_CONFIG_XHUBSFTP_KEY_FILE       = $SftpKeyPath
  # 首次连接跳过 host key 校验；要加固可预置 known_hosts 并改用 known_hosts_file 选项
  $env:RCLONE_CONFIG_XHUBSFTP_HOST_KEY_VERIFY = "false"

  $remote  = "xhubsftp:$RemoteRoot"
  $urlBase = "http://$SftpHost`:$HttpPort"
  Write-Host "通道: sftp → $remote/releases（缓存头由服务器端 Nginx 管理）"
}

$dest = "$remote/releases"
$useHeader = ($Target -ne 'sftp')   # 对象存储通道缓存头随上传设置；sftp 通道由 Nginx 管理

# --- 3. 上传（清单可回源，包体不可变）---
Write-Host "[1/4] 校验连接与目录（lsd）..."
rclone lsd $remote
if ($LASTEXITCODE -ne 0) { Write-Error "rclone lsd 失败，请检查通道凭据与目标目录。" }
Write-Host "      连接 OK" -ForegroundColor Green

Write-Host "[2/4] 上传 update.json(.sig) 与版本副本 ..."
if ($useHeader) {
  rclone copy $DistDir $dest --include "update.json*" --include "packages/update-*.json" --header-upload $noCacheHeader -P
} else {
  rclone copy $DistDir $dest --include "update.json*" --include "packages/update-*.json" -P
}
if ($LASTEXITCODE -ne 0) { Write-Error "update.json 上传失败。" }
Write-Host "      update.json OK" -ForegroundColor Green

Write-Host "[3/4] 上传 win-x64/** 包体 ..."
# 只传发布 zip：win-x64 下若残留解压出来的便携版目录（含 data/app.db、剪贴板图片、日志），
# 用 win-x64/** 会把整棵目录递归传到公网桶（v0.9.10 曾因此误传过本机数据）。
if ($useHeader) {
  rclone copy $DistDir $dest --include "win-x64/*.zip" --header-upload $immutableHeader -P
} else {
  rclone copy $DistDir $dest --include "win-x64/*.zip" -P
}
if ($LASTEXITCODE -ne 0) { Write-Error "包体上传失败。" }
Write-Host "      packages OK" -ForegroundColor Green

# --- 4. 保留策略：win-x64 下只留最近 N 版的 zip（按文件名内嵌版本号排序）---
Write-Host "[4/4] 清理 win-x64 下旧版本（保留最近 $KeepVersions 版）..."
$listed = rclone lsf $dest/win-x64 --files-only 2>&1
if ($LASTEXITCODE -ne 0) { Write-Error "win-x64 列目录失败。" }
$zips = @($listed | Where-Object { $_ -match '\.zip$' } | ForEach-Object { $_.Trim() } |
  Sort-Object { if ($_ -match '-(\d+\.\d+\.\d+)-') { [version]$Matches[1].ToString() } else { [version]"0.0.0" } } -Descending)
$toRemove = @($zips | Select-Object -Skip ($KeepVersions * 2))
foreach ($f in $toRemove) {
  rclone delete "$dest/win-x64/$f"
  if ($LASTEXITCODE -eq 0) { Write-Host "  已清理: $f" }
}

# --- 5. 验证 HTTP 可访问性 ---
Write-Host "验证 HTTP 可访问性 ..."
foreach ($p in "update.json", "update.json.sig") {
  $url = "$urlBase/releases/$p"
  $r = Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 20
  "{0} -> HTTP {1} ({2} bytes)" -f $url, $r.StatusCode, $r.RawContentLength
  if ($r.StatusCode -ne 200) { Write-Error "$url 未返回 200" }
}
Write-Host "全部完成 ✔ 升级清单现已可访问: $urlBase/releases/update.json" -ForegroundColor Green

# upload-market.ps1 — 上传 dist-market 产物到分发端点
# 三通道：-Target cos（默认，腾讯云 COS）/-Target r2（过渡期兜底，Cloudflare R2）/-Target sftp（备选，自建 Nginx）。
# 过渡期每次发布建议 cos + r2 各跑一遍，保持两边清单/包一致；详见 docs/self-hosted-distribution.md §6。
# 用法:
#   .\scripts\upload-market.ps1                                  # cos → 腾讯云 COS（读 COS_* 环境变量）
#   .\scripts\upload-market.ps1 -Target r2                       # → R2（读 R2_* 环境变量）
#   .\scripts\upload-market.ps1 -Target sftp                     # → 自建服务器（读 XHUB_DEPLOY_*）
# 环境变量:
#   COS_SECRET_ID / COS_SECRET_KEY / COS_BUCKET(含 APPID 后缀) / COS_REGION(如 ap-guangzhou)
#   R2_ACCOUNT_ID / R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY
#   XHUB_DEPLOY_HOST / XHUB_DEPLOY_PORT(默认22) / XHUB_DEPLOY_USER(默认deploy) / XHUB_DEPLOY_KEY
# 依赖: rclone (winget install --id Rclone.Rclone)

param(
  [ValidateSet('cos', 'sftp', 'r2')][string]$Target = 'cos',

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

  # —— r2（过渡期兜底）——
  [string]$AccountId       = $env:R2_ACCOUNT_ID,
  [string]$AccessKeyId     = $env:R2_ACCESS_KEY_ID,
  [string]$SecretAccessKey = $env:R2_SECRET_ACCESS_KEY,
  [string]$Bucket          = "x-hub-dist",
  [string]$R2BaseUrl       = "https://r2.dckxx.com",

  # —— 通用 ——
  [string]$DistDir = (Join-Path $PSScriptRoot "..\dist-market"),
  [string]$Prefix  = "extensions"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path (Join-Path $DistDir "registry.json"))) {
  Write-Error "本地产物缺失 registry.json：请先运行 publish-extension.ps1（$DistDir 不存在或未生成）。"
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
  $urlBase = "https://$CosBucket.cos.$CosRegion.myqcloud.com" + $(if ($Prefix) { "/$Prefix" } else { "" })
  Write-Host "通道: cos → $remote/$Prefix（缓存头随上传设置）"
} elseif ($Target -eq 'sftp') {
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
  $urlBase = "http://$SftpHost`:$HttpPort" + $(if ($Prefix) { "/$Prefix" } else { "" })
  Write-Host "通道: sftp → $remote/$Prefix（缓存头由服务器端 Nginx 管理）"
} else {
  if (-not $AccountId -or -not $AccessKeyId -or -not $SecretAccessKey) {
    Write-Error "缺少 R2 凭据。请用 -AccountId/-AccessKeyId/-SecretAccessKey 传入，或设置 R2_ACCOUNT_ID/R2_ACCESS_KEY_ID/R2_SECRET_ACCESS_KEY 环境变量。"
  }

  $env:RCLONE_CONFIG_R2_TYPE              = "s3"
  $env:RCLONE_CONFIG_R2_PROVIDER          = "Cloudflare"
  $env:RCLONE_CONFIG_R2_ACCESS_KEY_ID     = $AccessKeyId
  $env:RCLONE_CONFIG_R2_SECRET_ACCESS_KEY = $SecretAccessKey
  $env:RCLONE_CONFIG_R2_ENDPOINT          = "https://$AccountId.r2.cloudflarestorage.com"

  $remote  = "r2:$Bucket"
  $urlBase = if ($Prefix) { "$R2BaseUrl/$Prefix" } else { $R2BaseUrl }
  Write-Host "通道: r2 → $remote/$Prefix（缓存头随上传设置）"
}

$dest = if ($Prefix) { "$remote/$Prefix" } else { $remote }
$useHeader = ($Target -ne 'sftp')   # 对象存储通道缓存头随上传设置；sftp 通道由 Nginx 管理

# --- 3. 上传（清单可回源，包/图标不可变）---
Write-Host "[1/3] 校验连接与目录（lsd）..."
rclone lsd $remote
if ($LASTEXITCODE -ne 0) { Write-Error "rclone lsd 失败，请检查通道凭据与目标目录。" }
Write-Host "      连接 OK" -ForegroundColor Green

Write-Host "[2/3] 上传 registry.json(.sig) ..."
if ($useHeader) {
  rclone copy $DistDir $dest --include "registry.json*" --header-upload $noCacheHeader -P
} else {
  rclone copy $DistDir $dest --include "registry.json*" -P
}
if ($LASTEXITCODE -ne 0) { Write-Error "registry 上传失败。" }
Write-Host "      registry OK" -ForegroundColor Green

Write-Host "[3/3] 上传 packages/** 与 icons/** ..."
if ($useHeader) {
  rclone copy $DistDir $dest --include "packages/**" --include "icons/**" --header-upload $immutableHeader -P
} else {
  rclone copy $DistDir $dest --include "packages/**" --include "icons/**" -P
}
if ($LASTEXITCODE -ne 0) { Write-Error "packages/icons 上传失败。" }
Write-Host "      packages/icons OK" -ForegroundColor Green

# --- 4. 验证 HTTP 可访问性 + sha256 抽查 ---
Write-Host "验证 HTTP 可访问性 ..."
foreach ($p in "registry.json", "registry.json.sig") {
  $url = "$urlBase/$p"
  $r = Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 20
  "{0} -> HTTP {1} ({2} bytes)" -f $url, $r.StatusCode, $r.RawContentLength
  if ($r.StatusCode -ne 200) { Write-Error "$url 未返回 200" }
}

$local = Get-ChildItem "$DistDir\packages" -Recurse -Filter *.xhpack | Select-Object -First 1
if ($local) {
  $hash = (Get-FileHash $local.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  $dl = Join-Path $env:TEMP "market-check-$($local.Name)"
  Invoke-WebRequest -Uri "$urlBase/$($local.FullName.Substring((Resolve-Path $DistDir).Path.Length + 1).Replace('\','/'))" -OutFile $dl -UseBasicParsing -TimeoutSec 30
  $dlHash = (Get-FileHash $dl -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($hash -ne $dlHash) { Write-Error "sha256 不一致！本地 $hash vs 远端 $dlHash" }
  Remove-Item $dl -Force
  "xhpack sha256 校验一致: $hash"
}
Write-Host "全部完成 ✔ 市场现已可访问: $urlBase/registry.json" -ForegroundColor Green

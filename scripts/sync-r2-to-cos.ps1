# sync-r2-to-cos.ps1 — 过渡期阶段 0（一次性）：把 R2 存量（extensions + releases）拉平到腾讯云 COS
# 详见 docs/self-hosted-distribution.md §6 阶段 0。旧版本包必须留在 COS 上，各版本客户端都依赖旧路径。
# 用法:
#   .\scripts\sync-r2-to-cos.ps1
# 环境变量:
#   R2_ACCOUNT_ID / R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY（源，从 GitHub Secrets 抄到本地）
#   COS_SECRET_ID / COS_SECRET_KEY / COS_BUCKET(含 APPID 后缀) / COS_REGION（目标）
# 依赖: rclone

param(
  # —— 源：R2 ——
  [string]$AccountId       = $env:R2_ACCOUNT_ID,
  [string]$AccessKeyId     = $env:R2_ACCESS_KEY_ID,
  [string]$SecretAccessKey = $env:R2_SECRET_ACCESS_KEY,
  [string]$Bucket          = "x-hub-dist",
  # —— 目标：COS ——
  [string]$CosSecretId  = $env:COS_SECRET_ID,
  [string]$CosSecretKey = $env:COS_SECRET_KEY,
  [string]$CosBucket    = $env:COS_BUCKET,
  [string]$CosRegion    = $env:COS_REGION
)

$ErrorActionPreference = "Stop"

if (-not $AccountId -or -not $AccessKeyId -or -not $SecretAccessKey) {
  Write-Error "缺少 R2 凭据（R2_ACCOUNT_ID / R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY）。"
}
if (-not $CosSecretId -or -not $CosSecretKey -or -not $CosBucket -or -not $CosRegion) {
  Write-Error "缺少 COS 参数（COS_SECRET_ID / COS_SECRET_KEY / COS_BUCKET / COS_REGION）。"
}

if (-not (Get-Command rclone -ErrorAction SilentlyContinue)) {
  Write-Error "未找到 rclone。请先安装: winget install --id Rclone.Rclone"
}

# --- 源：R2 ---
$env:RCLONE_CONFIG_R2_TYPE              = "s3"
$env:RCLONE_CONFIG_R2_PROVIDER          = "Cloudflare"
$env:RCLONE_CONFIG_R2_ACCESS_KEY_ID     = $AccessKeyId
$env:RCLONE_CONFIG_R2_SECRET_ACCESS_KEY = $SecretAccessKey
$env:RCLONE_CONFIG_R2_ENDPOINT          = "https://$AccountId.r2.cloudflarestorage.com"

# --- 目标：腾讯云 COS ---
$env:RCLONE_CONFIG_COS_TYPE              = "s3"
$env:RCLONE_CONFIG_COS_PROVIDER          = "TencentCOS"
$env:RCLONE_CONFIG_COS_ACCESS_KEY_ID     = $CosSecretId
$env:RCLONE_CONFIG_COS_SECRET_ACCESS_KEY = $CosSecretKey
$env:RCLONE_CONFIG_COS_ENDPOINT          = "cos.$CosRegion.myqcloud.com"

Write-Host "[1/3] 校验两端连接..."
rclone lsd "r2:$Bucket"
if ($LASTEXITCODE -ne 0) { Write-Error "R2 连接失败，请检查凭据。" }
rclone lsd "cos:$CosBucket"
if ($LASTEXITCODE -ne 0) { Write-Error "COS 连接失败，请检查桶名（含 APPID 后缀）/地域/密钥。" }

Write-Host "[2/3] 同步 extensions/ ..."
rclone sync "r2:$Bucket/extensions" "cos:$CosBucket/extensions" -P
if ($LASTEXITCODE -ne 0) { Write-Error "extensions 同步失败。" }

Write-Host "[3/3] 同步 releases/ ..."
rclone sync "r2:$Bucket/releases" "cos:$CosBucket/releases" -P
if ($LASTEXITCODE -ne 0) { Write-Error "releases 同步失败。" }

Write-Host ""
Write-Host "拉平完成 ✔ 建议抽查（清单/206/缓存头，见 docs/self-hosted-distribution.md §3）：" -ForegroundColor Green
Write-Host "  curl -sI https://$CosBucket.cos.$CosRegion.myqcloud.com/extensions/registry.json"
Write-Host "  curl -sI -H 'Range: bytes=0-99' https://$CosBucket.cos.$CosRegion.myqcloud.com/releases/update.json"

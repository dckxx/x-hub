#requires -Version 7
<#
.SYNOPSIS
  应用发版：打两个 zip（标准版 + 便携版）→ 生成 update.json + Ed25519 签名 →
  经 scripts/upload-release.ps1（rclone sftp）上传到自建分发服务器，并保留 `releases/win-x64/` 下最近 2 版。

.DESCRIPTION
  配合客户端自研 updater.rs（方案 A）：
    - update.json 是客户端唯一安全根（Ed25519 验签后才可信），其内 sha256 背书 zip 下载物；
    - 便携版（exe 旁有 portable 标志）优先取 portableUrl / portableSha256，标准版取 url / sha256。

.PARAMETER ExePath
  新版本 exe（Release 构建产物，如 src-tauri\target\release\x-hub.exe）。
.PARAMETER Version
  新版本号（如 0.4.0，须与 package.json/tauri.conf.json 一致）。
.PARAMETER SignKey
  Ed25519 私钥 PEM 路径（默认取环境变量 XHUB_SIGNING_KEY；与市场清单同一把密钥即可）。
.PARAMETER Notes
  版本说明摘要，写进 update.json.notes（默认为空，客户端展示"暂无更新说明"）。
.PARAMETER MinimumUpgradable
  可升级的最低版本下限（跳级保护；默认 0.1.0）。
.PARAMETER BaseUrl
  应用发布根 URL，默认取环境变量 XHUB_DIST_BASE_URL（如 http://IP:8080）拼 /releases；两者都缺则报错。
.PARAMETER OutDir
  本地产物目录，默认 <仓库>/dist-release。

.EXAMPLE
  ./scripts/publish-release.ps1 -ExePath E:\workspace\x-hub\src-tauri\target\release\x-hub.exe -Version 0.4.0 -SignKey E:\workspace\.x-hub-signing\market.key -Notes "v0.4.0: 新增应用自动升级"
#>
param(
  [Parameter(Mandatory = $true)][string]$ExePath,
  [Parameter(Mandatory = $true)][string]$Version,
  [string]$SignKey = $env:XHUB_SIGNING_KEY,
  [string]$Notes = '',
  [string]$MinimumUpgradable = '0.1.0',
  [string]$BaseUrl = $(if ($env:XHUB_DIST_BASE_URL) { "$($env:XHUB_DIST_BASE_URL.TrimEnd('/'))/releases" } else { '' }),
  [string]$OutDir = (Join-Path $PSScriptRoot '..\dist-release')
)

$ErrorActionPreference = 'Stop'

# ---------- 校验入参 ----------
if (-not (Test-Path -LiteralPath $ExePath)) { throw "未找到 exe: $ExePath" }
if ((Get-Item -LiteralPath $ExePath).Extension -ne '.exe') { throw "ExePath 需为 .exe 文件" }
if (-not ($Version -match '^\d+\.\d+\.\d+$')) { throw "Version 需为语义化版本（x.y.z），实际: $Version" }
if (-not $SignKey -and -not (Test-Path -LiteralPath $SignKey)) { throw '缺少签名私钥：请用 -SignKey 指定或设置环境变量 XHUB_SIGNING_KEY' }
if (-not $BaseUrl) { throw '未指定发布根 URL：请用 -BaseUrl 传入或设置环境变量 XHUB_DIST_BASE_URL' }
if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw '需要 Node.js（用于 Ed25519 签名）' }

# ---------- 目录 ----------
$winDir = Join-Path $OutDir 'win-x64'
$pkgDir = Join-Path $OutDir 'packages'
New-Item -ItemType Directory -Force -Path $winDir, $pkgDir | Out-Null

$exeName = [System.IO.Path]::GetFileName($ExePath)

function New-Zip {
  param([string]$SrcPath, [string]$ZipPath, [switch]$AddPortableMarker)
  # Compress-Archive 会包含父目录结构，这里先复制到临时目录再打包，保证 zip 根即文件
  $tmp = Join-Path $env:TEMP "xhub-rel-$([guid]::NewGuid().ToString('N'))"
  New-Item -ItemType Directory -Force -Path $tmp | Out-Null
  Copy-Item -LiteralPath $SrcPath -Destination $tmp -Force
  if ($AddPortableMarker) {
    New-Item -ItemType File -Path (Join-Path $tmp 'portable') -Force | Out-Null
  }
  if (Test-Path $ZipPath) { Remove-Item -LiteralPath $ZipPath -Force }
  Compress-Archive -Path (Join-Path $tmp '*') -DestinationPath $ZipPath -CompressionLevel Optimal
  Remove-Item -Recurse -Force $tmp
}

# 标准版 zip（可改名，保持 x-hub.exe 名称）
$stdZip = Join-Path $winDir "x-hub-$Version-win-x64.zip"
New-Zip -SrcPath $ExePath -ZipPath $stdZip
# 便携版 zip（含 portable 标志，随 exe 同目录放置）
$portableZip = Join-Path $winDir "x-hub-$Version-win-x64-portable.zip"
New-Zip -SrcPath $ExePath -ZipPath $portableZip -AddPortableMarker

$stdSha = (Get-FileHash -LiteralPath $stdZip -Algorithm SHA256).Hash.ToLowerInvariant()
$stdSize = (Get-Item -LiteralPath $stdZip).Length
$portableSha = (Get-FileHash -LiteralPath $portableZip -Algorithm SHA256).Hash.ToLowerInvariant()
$portableSize = (Get-Item -LiteralPath $portableZip).Length

# ---------- update.json ----------
$update = [ordered]@{
  schemaVersion      = 1
  version            = $Version
  publishedAt        = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  minimumUpgradable  = $MinimumUpgradable
  notes              = $Notes
  platforms          = [ordered]@{
    'windows-x86_64' = [ordered]@{
      url              = "$BaseUrl/win-x64/x-hub-$Version-win-x64.zip"
      portableUrl      = "$BaseUrl/win-x64/x-hub-$Version-win-x64-portable.zip"
      sha256           = $stdSha
      portableSha256   = $portableSha
      size             = $stdSize
      portableSize     = $portableSize
    }
  }
}
$updateJson = Join-Path $OutDir 'update.json'
$update | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $updateJson -Encoding utf8

# 同时保留一处带版本号的副本（便于回溯/回滚）
$verCopy = Join-Path $pkgDir "update-$Version.json"
Copy-Item -LiteralPath $updateJson -Destination $verCopy -Force

# ---------- Ed25519 分离签名 ----------
$sigPath = "$updateJson.sig"
node (Join-Path $PSScriptRoot 'pub-sign.mjs') $SignKey $updateJson $sigPath
if ($LASTEXITCODE -ne 0) { throw 'Ed25519 签名失败' }

# ---------- 输出 ----------
Write-Host ''
Write-Host '==== 发布产物（上传到分发服务器 releases/，与 dist-release 内容对应） ====' -ForegroundColor Cyan
Write-Host "standard : $stdZip"
Write-Host "  sha256 : $stdSha  ($stdSize bytes)"
Write-Host "portable : $portableZip"
Write-Host "  sha256 : $portableSha  ($portableSize bytes)"
Write-Host "manifest : $updateJson"
Write-Host "sig      : $sigPath"
Write-Host ''
Write-Host '上传：运行 scripts\upload-release.ps1（读 XHUB_DEPLOY_* 环境变量，sftp 上传并校验）。' -ForegroundColor Yellow
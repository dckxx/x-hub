#requires -Version 7
<#
.SYNOPSIS
  打包扩展并生成本地发布产物（R2 市场清单 v2）。
  生成到 <OutDir>（默认 <仓库>/dist/market）：
    packages/<id>/<version>/<id>-<version>.zip   — 扩展包（不可变路径）
    icons/<id>.<ext>                              — 图标（manifest.icon 存在时）
    registry.json                                 — 合并后的完整清单（upsert 该扩展）
    registry.json.sig                             — Ed25519 分离签名（base64 文本）
  上传用 rclone（脚本末尾打印命令），CI 里由 release-extension.yml 自动完成。

.PARAMETER ExtDir
  扩展源码目录（含 manifest.json）。
.PARAMETER SignKey
  Ed25519 私钥 PEM 路径（默认取环境变量 XHUB_SIGNING_KEY）。
.PARAMETER Endpoint
  市场 CDN 根 URL，默认 https://r2.dckxx.com/extensions。
.PARAMETER OutDir
  产物根目录，默认 <仓库>/dist/market。
.EXAMPLE
  ./scripts/publish-extension.ps1 -ExtDir E:\workspace\x-hub-extensions\extensions\hello-web -SignKey E:\workspace\.x-hub-signing\market.key
#>
param(
  [Parameter(Mandatory = $true)][string]$ExtDir,
  [string]$SignKey = $env:XHUB_SIGNING_KEY,
  [string]$Endpoint = 'https://r2.dckxx.com/extensions',
  [string]$OutDir = (Join-Path $PSScriptRoot '..\dist-market')
)

$ErrorActionPreference = 'Stop'

# ---------- 校验入参 ----------
$manifestPath = Join-Path $ExtDir 'manifest.json'
if (-not (Test-Path $manifestPath)) { throw "未找到 manifest.json: $manifestPath" }
if (-not $SignKey) { throw '缺少签名私钥：请用 -SignKey 指定或设置环境变量 XHUB_SIGNING_KEY' }
if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw '需要 Node.js（用于 Ed25519 签名）' }

$manifest = Get-Content $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
$id = $manifest.id
$version = $manifest.version
if (-not $id -or -not $version) { throw 'manifest.json 缺少 id 或 version' }
if (-not ($version -match '^\d+\.\d+\.\d+$')) { throw "version 需为语义化版本（x.y.z），实际: $version" }

# ---------- 1. 打包 zip（manifest.json 必须位于 zip 根） ----------
$pkgDir = Join-Path $OutDir "packages\$id\$version"
New-Item -ItemType Directory -Force -Path $pkgDir | Out-Null
$zip = Join-Path $pkgDir "$id-$version.zip"
$tmpZip = Join-Path $env:TEMP "$($id.Replace('.', '-'))-$version-$([guid]::NewGuid().ToString('N')).zip"
if (Test-Path $tmpZip) { Remove-Item $tmpZip -Force }
Compress-Archive -Path (Join-Path $ExtDir '*') -DestinationPath $tmpZip -CompressionLevel Optimal
Move-Item -LiteralPath $tmpZip -Destination $zip -Force
$sha256 = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
$size = (Get-Item -LiteralPath $zip).Length

# ---------- 2. 图标（可选）：复制到 icons/<id>.<ext>，清单 icon 指向 CDN ----------
$iconUrl = ''
if ($manifest.icon) {
  $iconSrc = Join-Path $ExtDir ($manifest.icon -replace '^\.?/', '')
  if (Test-Path -LiteralPath $iconSrc) {
    $ext = [System.IO.Path]::GetExtension($manifest.icon)
    $iconDestDir = Join-Path $OutDir 'icons'
    New-Item -ItemType Directory -Force -Path $iconDestDir | Out-Null
    Copy-Item -LiteralPath $iconSrc -Destination (Join-Path $iconDestDir "$id$ext") -Force
    $iconUrl = "$Endpoint/icons/$id$ext"
  } else {
    Write-Warning "manifest.icon 指向的文件不存在，跳过图标: $iconSrc"
  }
}

# ---------- 3. 市场条目：manifest 基础字段 + 可选 market.json 补充字段 ----------
function NonNull([string]$v) { if ($null -eq $v) { '' } else { $v } }
$entry = [ordered]@{
  id           = $id
  name         = NonNull $manifest.name
  version      = $version
  description  = NonNull $manifest.description
  runtime      = NonNull $manifest.runtime
  author       = NonNull $manifest.author
  downloadUrl  = "$Endpoint/packages/$id/$version/$id-$version.zip"
  sha256       = $sha256
  size         = $size
  icon         = $iconUrl
}
# 附加/覆盖字段（minAppVersion / changelog / homepage / required / description…）
$marketMeta = Join-Path $ExtDir 'market.json'
if (Test-Path -LiteralPath $marketMeta) {
  $meta = Get-Content -LiteralPath $marketMeta -Raw -Encoding utf8 | ConvertFrom-Json
  foreach ($p in $meta.PSObject.Properties) {
    if ($null -ne $p.Value) { $entry[$p.Name] = $p.Value }
  }
}

# ---------- 4. 合并 registry.json（upsert 该 id，保留其它条目） ----------
$registryPath = Join-Path $OutDir 'registry.json'
if (Test-Path -LiteralPath $registryPath) {
  $registry = Get-Content -LiteralPath $registryPath -Raw -Encoding utf8 | ConvertFrom-Json
  if ($registry.schemaVersion -gt 2) { throw "registry.json schemaVersion=$($registry.schemaVersion) 高于当前支持 v2" }
} else {
  $registry = [pscustomobject]@{ schemaVersion = 2; updatedAt = ''; extensions = @($null) }
}
if ($null -eq $registry.extensions) { $registry.extensions = @($null) }
$others = @($registry.extensions | Where-Object { $_ -and $_.id -ne $id })
$registry.extensions = $others + [pscustomobject]$entry
$registry.updatedAt = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')

# 幂等去重：同一 id 出现多次只保留最后一条
$seen = [System.Collections.Generic.HashSet[string]]::new()
$unique = @()
foreach ($e in @($registry.extensions)) {
  if ($e -and $seen.Add([string]$e.id)) { $unique += $e }
}
$registry.extensions = $unique

$registry | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $registryPath -Encoding utf8

# ---------- 5. Ed25519 分离签名 ----------
$sigPath = "$registryPath.sig"
node (Join-Path $PSScriptRoot 'pub-sign.mjs') $SignKey $registryPath $sigPath
if ($LASTEXITCODE -ne 0) { throw 'Ed25519 签名失败' }

# ---------- 6. 输出 ----------
Write-Host ''
Write-Host '==== 发布产物（上传到 R2 桶根，与 dist-market 内容对应） ====' -ForegroundColor Cyan
Write-Host "zip      : $zip"
Write-Host "sha256   : $sha256"
Write-Host "size     : $size bytes"
if ($iconUrl) { Write-Host "icon     : $iconUrl" }
Write-Host "registry : $registryPath"
Write-Host "sig      : $sigPath"
Write-Host ''
Write-Host '上传命令参考（rclone 已配置 r2 remote 时）：' -ForegroundColor Yellow
Write-Host "  rclone copy $OutDir r2:x-hub-dist/extensions --include 'registry.json*' --header-upload 'Cache-Control: no-cache' -P"
Write-Host "  rclone copy $OutDir r2:x-hub-dist/extensions --include 'packages/**' --include 'icons/**' --header-upload 'Cache-Control: public, max-age=31536000, immutable' -P"
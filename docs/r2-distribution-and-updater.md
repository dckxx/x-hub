# x-hub 基于 Cloudflare R2 的扩展中心与自动升级方案

> 状态：**P0+P1+P2 已实施**（2026-08-26）；**P3 应用升级已随 v0.3.0 实施**（`src-tauri/src/updater.rs` + `signing.rs`：update.json Ed25519 验签 + minimumUpgradable 跳级保护 + sha256 下载校验 + 重启自替换/回滚）。
> 实施记录见文末附录。

## 1. 目标与现状

### 1.1 目标

1. **扩展中心**：把市场清单从本地文件升级为 R2 托管的远端清单，支持浏览、安装、**一键更新**已装扩展，全程校验完整性（sha256）与防篡改（Ed25519 签名）。
2. **自动升级**：应用启动后静默检查新版本，手动确认 → 流式下载 → 校验 → 重启自替换，标准版与便携版都支持。

### 1.2 现状（改造基线）

| 关注点 | 现状 | 依据 |
|---|---|---|
| 市场清单 | 本地文件 `data_root()/market/registry.json`，格式 `{extensions:[{id,name,version,description,runtime,author,downloadUrl}]}` | `src-tauri/src/market.rs` |
| 市场安装 | `install_from_market(downloadUrl)`：reqwest 下载 → 解包（zip-slip 防护）→ 定位 manifest → 复制；**已安装则拒绝**（无更新语义） | `market.rs` |
| 扩展数据目录 | `data_root()/extensions/<id>/`，含 `.permissions.json` / `.config.json` / `.storage.json` / `.deploy-config.json` 等用户数据点文件 | `extension.rs`、`xhub_api.rs` |
| 扩展热更新 | `extensions_stamp` 对 manifest 路径+mtime 做 FNV 哈希，前端 5s 轮询刷新列表 | `extension.rs` |
| 应用发布 | tag `v*` 触发 GitHub Actions，`--no-bundle` 产物为纯 exe zip（标准版 + 便携版），传 GitHub Releases(draft) | `.github/workflows/release.yml` |
| 版本 | 0.3.0；`version` 单一来源 README → package.json / tauri.conf.json / Cargo.toml | 发版清单 |
| 网络库 | reqwest 0.12（已开 `stream` 特性，可流式下载 + 进度）、tokio、zip 2（deflate）已就位 | `Cargo.toml` |
| 更新检测 | 仅 `check_whats_new`：本地 RELEASE_NOTES 段落后弹 What's New，**零网络** | `about.rs`、`commands.rs` |

### 1.3 设计原则

- **复用现有链路**：市场安装继续走 `install_from_market` 的解包/校验路径；本地清单缓存保留，离线可读。
- **R2 只放不可变对象**：所有包路径含版本号（`.../1.2.0/...xhpack`），天然支持长缓存、回滚、增量。
- **清单签名为安全根**：清单本身 Ed25519 签名；所有下载物（zip 包、exe）的 sha256 由签名清单背书，客户端不信任任何未签名字节。
- **升级不丢用户数据**：扩展升级必须保留 `.permissions.json` / `.config.json` / `.storage.json` / `.deploy-config.json` 等点文件；应用升级只替换 exe，数据目录不动。

## 2. 总体架构

```
发布侧（GitHub Actions / 本地脚本）
  release.yml（打 tag 触发）
    ├─ tauri:build --no-bundle → x-hub-<ver>-win-x64.zip / portable.zip
    ├─ 生成 releases/update.json + Ed25519 签名 → .sig
    └─ rclone 上传 R2 桶 x-hub-dist
  release-extension.yml（dispatch 触发）
    ├─ 打包扩展目录 → <id>-<ver>.xhpack + sha256
    ├─ 更新 extensions/registry.json + 签名
    └─ rclone 上传 R2 桶

存储侧（Cloudflare R2 公开桶 + 自定义域名 dist.x-hub.dev）
  extensions/registry.json(.sig)   ← 扩展市场清单
  extensions/packages/<id>/<v>/…   ← 扩展包（不可变）
  extensions/icons/<id>.svg        ← 市场卡片图标
  releases/update.json(.sig)       ← 应用更新清单
  releases/win-x64/*.zip           ← 应用包（不可变）

客户端（x-hub 桌面应用）
  market.rs：refresh_market_registry（fetch+验签+缓存）→ get_market_registry（读缓存）
            install_from_market(+sha256 校验+进度) / update_extension(备份-替换-回滚)
  updater.rs：check_for_update / download_update / apply_pending_update（重启自替换）
  安全：内嵌 Ed25519 公钥；sha256 逐包校验
```

## 3. R2 基础设施准备（一次性）

1. 注册 Cloudflare → 控制台 **R2 → 创建桶** `x-hub-dist`（公开读）。
2. **自定义域名**：桶设置 → Settings → Public Access → **连接自定义域名**（如 `dist.x-hub.dev`）。域名须托管在 Cloudflare DNS，选「域由 Cloudflare 托管」自动建 CNAME 记录。HTTPS 证书免费自动签发。
   - ⚠️ 不建议用默认的 `*.r2.dev` 子域：有每秒请求速率限制且不适合正式分发。
3. **API Token**：My Profile → API Tokens → Create Token，权限 Object Read & Write（Edit 权限），仅限该桶 —— 存入 GitHub Secrets（见 §8）。
4. **CORS**（可选项）：WebView2 / 浏览器直连 fetch 不受同源策略限制，但为稳妥可在桶 CORS 规则加 `GET` `*`。
5. **缓存策略**：上传时按对象设 `Cache-Control`：
   - `registry.json` / `update.json`：`no-cache`（客户端有签名不怕内容变化，只求每次回源拿最新）
   - 包 / 图标：`public, max-age=31536000, immutable`（路径不可变）
   - 自定义域名自动走 Cloudflare 边缘缓存，无需额外配置。

## 4. 对象存储目录布局

```
x-hub-dist/
├── extensions/
│   ├── registry.json          # 市场总清单（一个 JSON 覆盖全部扩展，小文件、易缓存）
│   ├── registry.json.sig      # Ed25519 分离签名（base64，64 字节签名）
│   ├── icons/
│   │   └── com.x-hub.ctool.svg
│   └── packages/
│       └── com.x-hub.ctool/
│           └── 1.2.0/com.x-hub.ctool-1.2.0.xhpack   # 不可变：路径含版本（zip 格式）
└── releases/
    ├── update.json            # 应用更新清单
    ├── update.json.sig
    ├── notes/                 # （可选）每版 release notes 独立文件
    └── win-x64/
        ├── x-hub-0.3.0-win-x64.zip
        └── x-hub-0.3.0-win-x64-portable.zip
```

- **不变性约定**：`packages/` 与 `releases/` 下所有对象一经上传不改写，新版本只新增路径。清理旧版 = 删除旧路径（保留最近 2 个版本即可）。
- **下载量/统计**：一期不需要；如需计数，后续加一个 Cloudflare Worker 做重定向计数即可，纯 R2 静态无法统计。

## 5. 扩展中心方案

### 5.1 清单格式 v2（registry.json）

在现有格式上扩展，保持向后兼容（`id/name/version/description/runtime/author/downloadUrl` 字段不变）：

```json
{
  "schemaVersion": 2,
  "updatedAt": "2026-08-26T12:00:00Z",
  "extensions": [
    {
      "id": "com.x-hub.ctool",
      "name": "C 工具集",
      "version": "1.2.0",
      "description": "……",
      "runtime": "web",
      "author": "x-hub team",
      "icon": "https://dist.x-hub.dev/extensions/icons/com.x-hub.ctool.svg",
      "downloadUrl": "https://dist.x-hub.dev/extensions/packages/com.x-hub.ctool/1.2.0/com.x-hub.ctool-1.2.0.xhpack",
      "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
      "size": 1048576,
      "minAppVersion": "0.3.0",
      "changelog": "v1.2.0：新增 …；修复 …",
      "homepage": "https://github.com/...",
      "required": false
    }
  ]
}
```

字段说明：
- `sha256`/`size`：zip 包的完整性/体积信息，客户端下载后校验；**包本身不单独签名**（清单签名已覆盖其哈希）。
- `minAppVersion`：宿主最低版本，防止新扩展装到旧宿主上缺能力。
- `icon`：R2 上图标的公开 URL，前端 `<img>` 直接加载，**无需落盘**。
- `runtime` 沿用 web/service；`disabled`/`dependsOn` 等平台判定已在本地 manifest 求值，清单不重复。

### 5.2 签名约定（与 §7 共用一套）

- 私钥（Ed25519）只存 GitHub Secrets；公钥 raw 32 字节 base64 内嵌进应用二进制。
- **分离签名文件**：`registry.json`（原始字节）与 `registry.json.sig`（对原始字节的 64 字节签名，base64 编码文本）。客户端取原始字节验证，避免 JSON 重序列化不一致问题。

### 5.3 客户端改造（Rust）

**新增配置**（`config.rs` `AppConfig`）：

| 字段 | 默认值 | 说明 |
|---|---|---|
| `market_endpoint` | `https://dist.x-hub.dev/extensions/registry.json` | 市场清单 URL |
| `update_endpoint` | `https://dist.x-hub.dev/releases/update.json` | 更新清单 URL |
| `auto_update_enabled` | `true` | 设置页可关 |
| `update_interval_hours` | `4` | 定时检测间隔 |

**改造 `market.rs`**：

1. `refresh_market_registry(app)`（新命令，async）：
   - fetch `market_endpoint` 原始字节 → fetch `.sig` → **验签**（失败：若本地缓存可用则告警并使用缓存，否则报错「市场清单验证失败」）
   - 校验 `schemaVersion` → 原子写 `data_root()/market/registry.json`（沿用 `paths.rs` tmp+rename 模式）
   - 返回拉取结果 + 更新时间，前端据此刷新
2. `get_market_registry(app)`：保持同步读本地缓存（离线/验签失败时仍可浏览上次清单），新增 `errors`/`lastUpdated` 字段透出状态。
3. `install_from_market(app, download_url, sha256, size)`（改造）：
   - reqwest `bytes_stream()` 流式下载 → 前端 emit `market-download-progress`（received/total）事件
   - 边下边算 sha256 → 与清单不符则删除临时文件并报「下载内容校验失败（可能被篡改或损坏）」
   - 后续解包/定位 manifest/复制逻辑**完全复用现有代码**
4. `update_extension(app, id, download_url) -> UpdateResult`（新命令，扩展更新）：
   - 下载 + sha256 校验（同上）
   - 解包 → 读新 manifest：**id 一致**、`version` 高于已装版本（推荐引入 `semver` crate 比较）、`minAppVersion` ≤ 宿主版本
   - **备份**：`extensions/<id>` → `extensions/.backup/<id>-<timestamp>`
   - **保留用户数据**：从旧目录取出 `.permissions.json` / `.config.json` / `.storage.json` / `.shared-storage.json` / `.deploy-config.json`（与扩展 id 相关者）
   - **原子替换**：新内容先复制到 `extensions/<id>.new`，`rename` 旧→backup、`new`→id 就位，再把点文件放回新目录
   - 成功 → 删 backup；任一步失败 → 回滚（`rename` backup 还原），并停用受损目录
   - service 扩展：先 `service::stop_service` 再替换（重开时懒启动）
   - 完成后 `extensions_stamp` 自动变化，前端 5s 轮询即感知
5. `uninstall_extension` 顺带清理 `.backup/<id>-*`。

**依赖新增**（`Cargo.toml`）：
```toml
sha2 = "0.10"                # sha256 计算
ed25519-dalek = "2"          # Ed25519 验证
semver = "1"                 # 版本比较（扩展/应用升级）
```

### 5.4 扩展发布流程

**方式 A：GitHub Actions `release-extension.yml`（推荐，几无人工）**

```yaml
on:
  workflow_dispatch:
    inputs:
      ext_dir:                # 扩展源码目录（x-hub-extensions/extensions/<id>）
      publish_type:           # stable / beta
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - checkout
      - 打包：cd $ext_dir → Compress-Archive → <id>-<version>.xhpack（manifest.json 在包根；zip 格式，后缀统一 .xhpack）
      - 计算 sha256、读取 manifest 的 id/version/name/description/runtime/author
      - 下载现有 registry.json → upsert 该扩展条目 → 写回（保留其它条目）
      - 用 secrets.UPDATE_SIGNING_KEY 对 registry.json 派生 Ed25519 签名 → .sig
      - rclone 上传 packages/<id>/<v>/<id>-<v>.xhpack、icons/（如新）、registry.json、registry.json.sig
```

**方式 B：本地脚本** `scripts/publish-extension.ps1`（无 CI 场景）：
```
参数：-ExtDir <目录> -Endpoint <R2 endpoint> -SignKey <私钥文件>
流程：打包 → sha256 → 合并清单 → 签名 → rclone 上传
```

> 扩展源码仓库（`x-hub-extensions/`）未在本次工作区根目录，脚本按「传入扩展目录」设计，与仓库位置解耦。

### 5.5 前端改造（ExtensionCenter.vue）

- 市场 tab：`loadMarket()` 改为「先 `refresh_market_registry` 再 `get_market_registry`」，显示 `lastUpdated` 与远端/缓存状态徽标；空态文案从「配置本地 registry.json」改为「仓库暂无扩展」。
- 卡片：渲染 `icon` URL（`convertFileSrc` 不适用 https，直接 `<img :src>`）；显示版本 / 大小 / `minAppVersion` 不满足时禁用安装。
- **更新入口**：installed 列表项对比市场版本 → 新版则红点「更新」按钮 → 调 `update_extension`，进度条走 `market-download-progress` 事件。
- 校验失败/toast：复用 `showToast`（index.vue provide）。

## 6. 应用自动升级方案

### 6.1 更新清单 `releases/update.json`

```json
{
  "schemaVersion": 1,
  "version": "0.4.0",
  "publishedAt": "2026-08-30T10:00:00Z",
  "minimumUpgradable": "0.1.0",
  "notes": "v0.4.0：新增 …；修复 …（自动取自 RELEASE_NOTES.md 对应段落）",
  "platforms": {
    "windows-x86_64": {
      "url": "https://dist.x-hub.dev/releases/win-x64/x-hub-0.4.0-win-x64.zip",
      "portableUrl": "https://dist.x-hub.dev/releases/win-x64/x-hub-0.4.0-win-x64-portable.zip",
      "sha256": "…",
      "portableSha256": "…",
      "size": 12345678
    }
  }
}
```

### 6.2 客户端流程（新模块 `updater.rs`）

**① 检查 `check_for_update(app)`（async，启动 5s 后 + 每 4h 一次）**
- fetch `update_endpoint` + `.sig` → 验签 → 失败静默（记日志），不打扰用户
- 半按 `semver` 比较：`清单 version > 宿主 version` 且 `minimumUpgradable ≤ 宿主 version`（跳级保护）
- 平台匹配：取 `platforms.windows-x86_64`；**便携版优先选 `portableUrl`**（`paths::is_portable()` 判定）
- 命中 → emit `update-available`（version/notes/size）→ 前端设置「关于」区显示新版本 + toast

**② 下载 `download_update(app) -> UpdateStatus`（用户点击确认后）**
- 流式下载到 `data_root()/updates/<version>/x-hub.zip`，emit `update-download-progress`
- 边下边算 sha256 → 校验（不符则删文件报错）
- 写更新标记 `data_root()/updates/.pending.json`：`{ version, zipPath, portable: bool, sha256 }`
- 返回「就绪」，前端引导「重启以完成更新」（弹窗确认，或设置页按钮）

**③ 应用 `apply_pending_update(app)`（每次启动早期调用，幂等）**
- 无 `.pending.json` → 直接跳过
- 便携版：解压新 zip 得到 exe → 目录内执行：
  ```
  x-hub.exe         →  x-hub.exe.old     # Windows 允许 rename 正在运行的 exe！
  x-hub.exe.new     →  x-hub.exe
  删除 x-hub.exe.old
  ```
- 标准版：同法替换 exe 所在位置（数据在 %APPDATA%\x-hub 不受影响）
- 任一步失败 → 把 `.old` rename 回原位（应用仍能启动，记日志，下次启动重试）
- 成功 → 删 `.pending.json` + 旧版本压缩包 → `log::info`

> 为什么可行：Windows 禁止 delete/overwrite 正在运行的 exe，但**允许 rename**。应用启动早期（自己已加载进内存）做 `exe → exe.old`、`新 exe → exe` 两步 rename 是自替换的经典做法，无需额外 updater 进程。若未来引入 NSIS 安装器，可平滑迁移到官方 `tauri-plugin-updater`（见 §12）。

**④ 配置与 UI**
- 设置「通用」区：自动更新开关（`auto_update_enabled`）、检查频率（隐藏项，默认 4h）
- 设置「关于」区：当前版本 + 新版本提示 + 「检查更新」按钮（强制触发 `check_for_update`）+ 下载进度条 + 「重启更新」按钮

### 6.3 与现有 What's New 的关系

`check_whats_new`（本地 changelog）保留不动；升级完成后首次启动照常弹 What's New，`last_seen_version` 逻辑不变。`update.json.notes` 只是下载前给用户看的摘要，最终详情仍走本地 RELEASE_NOTES。

> ⚠️ v0.3.0 实施调整：What's New 机制（`check_whats_new` / `whats_new_enabled` / `last_seen_version`）已随更新弹窗一并移除，新版本说明直接由全局 `UpdateCheckDialog` 展示（摘要来自 `update.json.notes`，详情仍走 `about.rs` 内置 RELEASE_NOTES），见 `src-tauri/src/updater.rs`。

## 7. 安全设计

| 风险 | 对策 |
|---|---|
| 中间人 / 源被篡改 | 全链路 HTTPS（R2 自定义域名自动签发证书）；下载物 sha256 逐个校验 |
| 清单被篡改 | 清单 Ed25519 分离签名，公钥内嵌二进制；验签失败回退本地缓存并告警 |
| 私钥泄露 | 私钥仅存 GitHub Secrets，CI 内签名后即销毁；公钥轮换需随版本发布（在 release notes 注明 breaking） |
| 降级攻击 | `minimumUpgradable` 下限 + 客户端不重复下载已应用的版本；扩展更新要求 `新版本 > 已装版本` |
| zip-slip / 恶意包 | 已有 `enclosed_name()` 防护 + manifest 校验 + 权限授权（permission_granted 逐项开关）保持 |

**密钥生成与签名命令（发布侧）**
```bash
# 一次性生成（私钥入 GitHub Secrets: UPDATE_SIGNING_KEY；公钥嵌入 src-tauri/src/keys.rs）
openssl genpkey -algorithm ED25519 -out update.key
openssl pkey -in update.key -pubout -out update.pub
# 导出 raw 32 字节公钥（base64）→ 嵌入
openssl pkey -in update.key -pubout -outform DER | tail -c 32 | base64 -w0
# 每次发布签名
openssl pkeyutl -sign -inkey update.key -rawin -in registry.json -out registry.json.sig
base64 -w0 registry.json.sig > registry.json.sig.b64   # 上传 base64 文本
# 客户端验证：ed25519-dalek PublicKey::from_bytes(raw_32) + verify(bytes, sig)
```

## 8. CI/CD 流水线改造

### 8.1 `release.yml`（应用发版，打 tag 触发）追加

```yaml
- name: Install rclone
  run: |
    winget install --id Rclone.Rclone -e --accept-source-agreements   # 或 choco/直接下 zip
- name: Configure R2
  env: { R2_ACCOUNT_ID, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY, R2_BUCKET }
  run: rclone config create r2 s3 provider Cloudflare access_key_id ... endpoint https://${{ secrets.R2_ACCOUNT_ID }}.r2.cloudflarestorage.com
- name: Upload artifacts & write update.json
  shell: pwsh
  run: scripts/publish-release.ps1   # 参数：version、两个 zip 路径、signing key
```

`scripts/publish-release.ps1` 职责：
1. rclone 上传两个 zip → `releases/win-x64/`
2. 生成 `update.json`（version / sha256 / size / notes=`RELEASE_NOTES.md` 对应段落）
3. Ed25519 签名 → `update.json.sig`（base64）
4. 上传 `update.json` + `.sig`（`Cache-Control: no-cache`）
5. 清理 `releases/win-x64/` 下除最近 2 版外的旧包（保留策略）

### 8.2 新增 `release-extension.yml`

手动 dispatch，输入扩展目录 → 打包 / 合并 registry.json / 签名 / 上传（详见 §5.4 方式 A）。

### 8.3 新增 GitHub Secrets

| Secret | 用途 |
|---|---|
| `R2_ACCOUNT_ID` | R2 账户 ID（endpoint 域名用） |
| `R2_ACCESS_KEY_ID` / `R2_SECRET_ACCESS_KEY` | S3 兼容 API 凭证（限该桶权限） |
| `R2_BUCKET` | 桶名 `x-hub-dist` |
| `UPDATE_SIGNING_KEY` | Ed25519 私钥（base64，仅签名用） |
| `CDN_BASE_URL` | `https://dist.x-hub.dev`（写进 update.json / registry.json 的 URL） |

## 9. 客户端新增内容一览

**新命令**（`lib.rs` invoke_handler 追加）：
- `market::refresh_market_registry`
- `market::update_extension`
- `updater::check_for_update`
- `updater::download_update`
- `updater::get_update_status`

**改造命令**：`market::install_from_market`（+sha256/size/进度）、`market::get_market_registry`（+状态字段）、`extension::uninstall_extension`（+清理 backup）

**新事件**（前端监听）：`market-download-progress`、`update-available`、`update-download-progress`、`update-ready`

**新文件**：`src-tauri/src/updater.rs`、`src-tauri/src/keys.rs`（公钥常量）、`scripts/publish-release.ps1`、`scripts/publish-extension.ps1`、`.github/workflows/release-extension.yml`

**新配置**：`market_endpoint` / `update_endpoint` / `auto_update_enabled` / `update_interval_hours`

**新依赖**：`sha2`、`ed25519-dalek`、`semver`

## 10. 前端改造点

| 文件 | 改动 |
|---|---|
| `ExtensionCenter.vue` | 市场 tab 远端化 + 状态徽标；卡片 icon/大小/minAppVersion；已装扩展「更新」按钮 + 进度条 |
| `SettingsView.vue`（关于区） | 「检查更新」「下载」「重启更新」按钮 + 进度条；版本对比展示 |
| `SettingsView.vue`（通用区） | 自动更新开关 |
| `src/api/tauri.ts` | 新命令封装 + `MarketExtension` v2 类型 + 事件监听封装 |
| `src/index/index.vue` | 启动时触发静默 `check_for_update`（可放在 `useStore` 初始化） |

## 11. 分阶段实施计划

| 阶段 | 内容 | 估工 |
|---|---|---|
| **P0 基础设施** | 开桶 + 自定义域名 + API Token；生成 Ed25519 密钥；`keys.rs` 公钥 + 验签工具函数 + 单测 | 0.5 天 |
| **P1 扩展中心** | registry.json v2 + `refresh_market_registry` + sha256 校验 + 下载进度；`publish-extension.ps1` + `release-extension.yml`；ExtensionCenter UI | 1.5 天 |
| **P2 扩展更新** | `update_extension`（备份/保留点文件/原子替换/回滚）+ 前端更新按钮 | 1 天 |
| **P3 应用自动升级** | `updater.rs` 全套 + `publish-release.ps1` + `release.yml` 改造 + 关于区 UI | 2 天 |
| **P4 可选增强** | NSIS 安装器 + `tauri-plugin-updater` 迁移；断点续传；beta 渠道（`update.beta.json` + 配置指向） | 2~3 天 |

> 建议 P1 发布后即把现有内置扩展（如 `com.x-hub.token-stats`）走一遍脚本上传，作为真实用例验收。

## 12. 成本与备选

- **费用**：R2 免费层 10GB 存储 / 100 万次 Class A（写）/ 1000 万次 Class B（读）/ 月，**无出口流量费**。个人分发场景（每版本几百 MB、日活跃几百人）完全落在免费层内；超限后按量付费（存储 $0.015/GB·月、Class A $4.5/百万、Class B $0.36/百万），成本可忽略。
- **备选 1：GitHub Releases 直连**（现状）——无需任何配置，但无签名校验、下载慢（国内）、无统一目录。R2 + 自定义域名可配 Cloudflare 加速，国内体验更好。
- **备选 2：r2.dev 子域**——免域名但有限速，仅适合内测期快速验证。
- **升级路线：自替换 → tauri-plugin-updater**：当引入 NSIS/MSI 安装器分发时，官方 `tauri-plugin-updater` v2 支持自定义 endpoints（可直指 R2）与 minisign 签名，届时把 `updater.rs` 的下载/校验层替换为插件、保留 UI 即可。本期自替换方案已把「校验 + 进度 + pending 标记」沉淀为可复用层。

## 13. 需要你提供的资源

1. Cloudflare 账号 + 想绑定的自定义域名（本方案默认 `dist.x-hub.dev`，可换）。
2. 确认扩展源码仓库位置（`x-hub-extensions/` 当前不在本工作区根目录），以便挂 `release-extension.yml`。
3. 确认后期是否要做安装器分发（决定 P4 是否排期）。

## 附：关键文件改动清单（对应代码实现阶段）

```
src-tauri/Cargo.toml                  + sha2 / ed25519-dalek / semver
src-tauri/src/lib.rs                  + 5 个新命令注册（见 §9）
src-tauri/src/config.rs               + 4 个配置字段
src-tauri/src/market.rs               改造 get/install + 新增 refresh/update_extension
src-tauri/src/updater.rs              （新）check/download/apply
src-tauri/src/keys.rs                 （新）Ed25519 公钥常量 + verify
src/api/tauri.ts                      命令/事件/类型
src/components/ExtensionCenter.vue    市场与更新 UI
src/components/SettingsView.vue       关于区/通用区
.github/workflows/release.yml         追加上传与 update.json
.github/workflows/release-extension.yml（新）
scripts/publish-release.ps1           （新）
scripts/publish-extension.ps1         （新）
docs/r2-distribution-and-updater.md   （本文档）
```

## 附录 A：P0+P1 实施记录（2026-08-26）

**已落地（代码侧全部完成并验证）**

| 项 | 实际落地 |
|---|---|
| 桶 / 域名 | 桶 `x-hub-dist`（用户已建），自定义域名 **`r2.dckxx.com`** |
| 密钥对 | Ed25519 已生成：私钥 `E:\workspace\.x-hub-signing\market.key`（**工作区外，妥善保管；未来放 GitHub Secrets `UPDATE_SIGNING_KEY`**）；公钥入库 `src-tauri/keys/market_public.key`（raw 32B base64） |
| 验签模块 | `src-tauri/src/signing.rs`（`pub mod`）：`verify_detached(content, sig_b64)` + 5 个单测（已知向量 / 篡改 / 非法 base64 / 长度非法 / 空） |
| 依赖 | Cargo.toml 新增 `sha2 0.10`、`ed25519-dalek 2`、`base64 0.22`（semver 留给 P2） |
| 清单 v2 | `market.rs::MarketExtension` 扩展：`sha256/size/icon/minAppVersion/changelog/homepage/required`；顶层 `schemaVersion/updatedAt`；兼容旧 v1 格式 |
| 远端刷新 | 新命令 `refresh_market_registry`：fetch `registry.json` 原始字节 + `.sig` → 验签（**失败一律不信任，回退本地缓存并透出 error**）→ schema 校验 → 原子落缓存 `data_root()/market/registry.json` |
| 本地读取 | `get_market_registry` 改为同步读缓存，返回 `MarketStatus{extensions,last_updated,source,error}` |
| 安装 | `install_from_market` 改为接收整个 `MarketExtension`：流式下载（reqwest `bytes_stream`）→ 边下边算 sha256（与清单比对，不符中止）→ 大小核对 → 节流广播 `market-download-progress`（≥256KB 一次）→ 复用原解包/复制链路 |
| 配置 | `config.rs`：`market_endpoint`（默认 `https://r2.dckxx.com/extensions/registry.json`，可改） |
| 前端 | `ExtensionCenter.vue`：市场 tab 改远端（refresh+get 并行宿主版本）；源异常黄色警示条 + 重试；卡片显示 icon / changelog / 宿主门槛（`minAppVersion` 不满足禁用并提示）；下载进度条（`transform: scaleX`，符合性能约定）；`tauri.ts` 类型 v2 + 新命令封装 |
| 发布脚本 | `scripts/pub-sign.mjs`（Node Ed25519 分离签名）+ `scripts/publish-extension.ps1`（打包 zip → sha256 → 支持 `market.json` 补充字段 → upsert 合并 registry.json → 签名 → 打印 rclone 上传命令）；产物目录 **`dist-market/`**（独立于 vite 的 `dist/`，避免 `npm run build` 清空）；重复发布幂等（实测 count 保持 1） |
| CI | `.github/workflows/release-extension.yml`（手动 dispatch：checkout 扩展仓库 → 打包 → 签名 → rclone 上传 R2，缓存头区分清单/包） |

**已验证**（本机）
- `cargo check` 通过；`npm run build`（vue-tsc + vite）通过
- 发布链路端到端：`hello-web` 打包 → registry v2 生成 → 签名可被同一公钥验证（Node `crypto.verify` = true，与 Rust `signing.rs` 同一密钥对）
- 产物自检：`cargo run --example market_selftest`（绕过 test harness）验证「已知向量验签 / 真实发布产物验签 / v1 兼容解析 / zip sha256 与清单一致」
- 幂等：同一扩展重复发布，registry extensions 不重复

**环境注意事项**
- 本机 `cargo test` 的 test harness 二进制存在 0xc0000139 启动失败（**改动前的旧 test 二进制同样失败**，属既有环境问题，与本任务无关；主程序 `app.exe` 正常）。已用 `examples/market_selftest.rs` 独立二进制兜底验证关键逻辑；CI（GitHub Actions）环境可正常跑 `cargo test`。

**待用户在 Cloudflare 侧完成**
1. 桶 `x-hub-dist` → Settings → Public Access → **连接自定义域名 `r2.dckxx.com`**（域名需托管在 Cloudflare DNS，自动 CNAME + 免费证书）；等待生效后 `https://r2.dckxx.com/` 可访问。
2. 创建 API Token（Object Read & Write，**仅限该桶**）→ 预留给 GitHub Secrets：`R2_ACCOUNT_ID` / `R2_ACCESS_KEY_ID` / `R2_SECRET_ACCESS_KEY` / `R2_BUCKET` / `CDN_BASE_URL=https://r2.dckxx.com` / `UPDATE_SIGNING_KEY`（= `E:\workspace\.x-hub-signing\market.key` 内容）。
3. 本地手动上传（rclone 未配也可先用任意 S3 客户端）：把 `dist-market/` 下 `registry.json`、`registry.json.sig`、`packages/**`、`icons/**` 传到桶根（缓存头：清单 `no-cache`，包/图标 `public, max-age=31536000, immutable`）。
4. 验证：浏览器打开 `https://r2.dckxx.com/extensions/registry.json` 与 `.sig` → 启动 x-hub 进「扩展中心 → 市场」，应看到 `hello-web` 卡片并可安装（安装走 sha256 校验 + 进度条）。5. 上传命令务必带 `extensions/` 前缀：`rclone copy dist-market r2:x-hub-dist/extensions ...`（否则对象落在桶根，与 registry 里的 URL 前缀不符导致 404）。

## 附录 B：P2 扩展更新实施记录（2026-08-26）

**已落地（代码侧全部完成并验证）**

| 项 | 实际落地 |
|---|---|
| 依赖 | `Cargo.toml` 新增 `semver = "1"` |
| 更新命令 | `market.rs::update_extension`（`lib.rs` 已注册）：下载 → sha256 校验 → 读新 manifest → **前置校验**（`id` 一致、`version` 高于已装、`minAppVersion` ≤ 宿主）→ service 停进程（`service::stop_service`）→ 组装新内容到 `.tmp-update/<id>` → `replace_extension_dir`（备份旧 → 原子就位新 → 从备份恢复用户点文件）→ 清理；任一步失败回滚到旧版；完成后 `extensions_stamp` 自然变化 |
| 目录替换 | `replace_extension_dir`（`pub`）：
1. `extensions/<id>` → `extensions/.backup/<id>-<ts>`（备份）
2. `.tmp-update/<id>` → `extensions/<id>`（rename 原子）
3. 从备份恢复 `.permissions.json` / `.config.json` / `.storage.json` / `.deploy-config.json`
4. 成功删备份；失败删新内容、rename 还原旧目录 |
| 版本比较 | `version_cmp`（`pub`）：优先 `semver` 语义，非 semver（如 "v1"、"1.0"）回退逐节数字比较 |
| 隐藏目录隔离 | `extension.rs` 新增 `is_hidden_dir`：`scan_extensions` 与 `extensions_stamp` 均跳过 `.` 开头目录，避免 `.backup/`、`.tmp-update/` 被当成扩展或干扰热更新戳 |
| 前端 | `ExtensionCenter.vue`：已装行 + 市场卡片均支持「更新」（版本对比 `versionLessThan`）；市场卡片按钮三分态（安装/更新/已安装）；已装行「更新 vX.Y.Z」按钮；复用 `market-download-progress` 事件做更新进度条（`transform: scaleX`）；已装 tab 启动即拉取市场（`onMounted` 并行 `loadMarket`）以便判断可更新 |
| API | `tauri.ts` 新增 `updateFromMarket(extension) => invoke('update_extension')` |

**已验证**（本机 `cargo run --example market_selftest`）
- 版本比较 semver/回退 6 例 `[PASS]`
- 目录替换：旧备份 / 新就位 / 点文件保留 `[PASS]`
- 既有验签 / 发布产物验签 / zip sha256 / v1 兼容 全部 `[PASS]`
- `cargo check` 零警告；`npm run build`（vue-tsc + vite）通过

**验收方式（端到端）**
1. 把演示扩展发一版更高的版本（如 `hello-web` v0.1.0 → v0.2.0）并上传 R2（发布脚本 + rclone，注意 `extensions/` 前缀）。
2. 客户端扩展中心「已安装」列表的 `hello-web` 出现「更新 v0.2.0」按钮；市场卡片按钮由「已安装」变为「更新」。
3. 点击更新 → 进度条 → 完成后版本变为 v0.2.0，且 `.config.json` / `.storage.json` 等用户数据保留。

## 附录 C：P3 应用自动升级实施记录（2026-08-27，方案 A：自研 updater.rs）

**已落地（代码侧全部完成，验证方式见下）**

| 项 | 实际落地 |
|---|---|
| 更新清单 | 新文件 `src-tauri/src/updater.rs`：`UpdateManifest`（schemaVersion=1 / version / minimumUpgradable / notes / platforms.windows-x86_64{ url, portableUrl, sha256, portableSha256, size, portableSize }） |
| 检查更新 | `check_for_update`：拉取 `update_endpoint`（默认 `https://r2.dckxx.com/releases/update.json`，可配置）+ `.sig` → Ed25519 验签（复用 `signing::verify_detached`，失败一律不信任且**静默**不打扰用户）→ semver 比较（复用 `market::version_cmp`）+ `minimumUpgradable` 跳级保护 → 平台匹配（便携版 `paths::is_portable()` 优先 portableUrl）→ 命中广播 `update-available` |
| 下载更新 | `download_update(version)`：重新拉取清单并验签（防竞态下载旧条目）→ 流式下载到 `updates/<version>/x-hub.zip.tmp`，边下边算 sha256（与清单比对，不符删文件中止）→ rename 原子就位 → 写 `updates/.pending.json` 标记 → 广播 `update-ready`；进度经 `update-download-progress` 节流（≥256KB 一次） |
| 自替换 | `apply_pending_update`：每次启动在 setup 早期（db 初始化前）调用，幂等无副作用；无标记即返回；有标记则 `extract_zip_read` 解包 → `locate_new_exe`（根/一层子目录取体积最大的 .exe）→ `exe → exe.old`、`新 exe → exe` 两步 rename（Windows 允许 rename 运行中的 exe）→ 删 .old + 新包 + 标记；任一步失败回滚（.old 还原）并保留标记下次启动重试 |
| 状态查询 | `get_update_status`：无网络请求，读本地标记返回是否 ready（供前端启动展示「重启更新」） |
| 定时检查 | setup 内 spawned task：启动 5s 后 + 每 `update_interval_hours`（默认 4h）循环，受 `auto_update_enabled` 控制，失败静默 |
| 配置 | `config.rs` 新增 `update_endpoint` / `auto_update_enabled`（默认 true）/ `update_interval_hours`（默认 4） |
| 命令注册 | `lib.rs` invoke_handler 追加 `updater::{check_for_update, download_update, get_update_status, skip_update_version}` |
| 发布脚本 | `scripts/publish-release.ps1`：打标准版 + 便携版两个 zip（便携版含 `portable` 标志）→ 算 sha256/size → 生成 `update.json`（含 version 副本 `packages/update-<ver>.json`）→ Ed25519 签名（`pub-sign.mjs`）→ 打印 rclone 命令 |
| 上传脚本 | `scripts/upload-release.ps1`：rclone 上传 `releases/`（update.json` no-cache，包体 immutable）+ 保留最近 2 版（按文件名内嵌版本号排序清理 win-x64）+ HTTPS 可访问性校验 |
| 前端 | `tauri.ts`：`UpdateInfo` 类型 + `checkForUpdate/downloadUpdate/getUpdateStatus/skipUpdateVersion` + `AppConfig` 三新字段 + `skipped_update_version`；`UpdateCheckDialog.vue`（全局弹窗，仅主窗口）：监听 `update-available` / `update-download-progress` / `update-ready`，`available` 态含版本/说明/大小/便携标志 + 「跳过此版本 / 取消 / 立即更新」，`downloading` 态进度条，`ready` 态「稍后 / 立即重启」，启动时若已有待应用标记直接弹「立即重启」；`AboutSection.vue` 当前版本旁「检查更新」按钮（手动触发 + 忽略跳过记录；命中由弹窗接管）与「自动检查更新」开关；`SettingsView.vue` 常规区「自动检查更新」开关（`store.setAutoUpdateEnabled`） |

**已验证**
- `cargo check` 零警告；`npm run build`（vue-tsc + vite）通过；新增单测（清单解析/未来 schema 拒绝/缺 version 拒绝/`is_newer` 跳级保护，含 4 例）编译通过（本机 `cargo test` harness 0xc0000139 旧环境问题，见附录 A）
- 发布位端到端可闭环：`publish-release.ps1` 产出双 zip + update.json + sig，`pub-sign.mjs` 与 `signing.rs` 同一密钥对（`E:\workspace\.x-hub-signing\market.key`）

**验收方式（端到端）**
1. 把 x-hub 发一版更高的版本（如 0.3.0 → 0.4.0 或 0.3.1）：`npm run build` + `npm run tauri:build` → `publish-release.ps1 -ExePath src-tauri\target\release\x-hub.exe -Version <新版本> -SignKey E:\workspace\.x-hub-signing\market.key -Notes "…"` → `upload-release.ps1`（需 R2 凭据）。
2. 旧版本启动 → 设置 → 关于 → 「检查更新」应命中全局更新弹窗（版本/说明/大小 + 「跳过此版本 / 取消 / 立即更新」）；或自动静默命中 → 「立即更新」→ 弹窗内进度条 → 就绪后「立即重启」→ 重启后 `get_app_info` 显示新版本号。
3. 便携版验证：exe 同目录放 `portable` 标志 → 更新走 portableUrl 且自替换后数据仍在 exe\data。

**风险提示**
- 自替换仅在**便携/绿色版**（免安装）确定无害；若未来迁移 NSIS 安装版，exe 所在目录可能受 UAC 保护（Program Files），需迁移 `tauri-plugin-updater`（文档 §12 已留路径）。
- 验签失败/通信失败默认静默：用户侧零打扰，但「检查更新」按钮误触时也会只显示错误提示（正常）。

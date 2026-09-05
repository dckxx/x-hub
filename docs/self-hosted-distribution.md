# x-hub 分发端点迁移方案：腾讯云 COS 替代 Cloudflare R2

> 状态：**发布脚本 / CI / 拉平脚本已全部改造完毕**；COS 侧待按 §2 开桶。
> 备选：自建 Nginx 静态托管（`scripts/server/` + §8），当前不启用。
> 客户端（`market.rs` / `updater.rs`）零改动：只认 `market_endpoint` / `update_endpoint` 两个 URL + 内嵌 Ed25519 公钥。
> 前置方案文档：`docs/r2-distribution-and-updater.md`（R2 时代的目录布局与签名约定，本文完全沿用）。

## 1. 为什么 COS 优于自建 Nginx（当前处境下）

| 关注点 | 自建 Nginx（服务器） | 腾讯云 COS（采用） |
|---|---|---|
| 域名/HTTPS | 域名备案中，只能 `http://IP:8080` 明文 | **默认域名 `*.cos.<region>.myqcloud.com` 是腾讯已备案域名，自带 HTTPS，立即可用** |
| 运维 | Nginx + deploy 账号 + 防火墙/安全组 | 零运维 |
| 上传通道 | rclone sftp（CI 得走 rsync） | rclone s3（provider TencentCOS），与 R2 时代同构 |
| Range 断点续传 | 静态文件原生 206 | COS 支持 206 |
| 国内速度 | 取决于服务器带宽 | 腾讯云机房，国内快 |
| 费用 | 服务器带宽（已含在服务器里） | 无免费下行流量（约 0.5 元/GB 国内）+ 存储 ~0.1 元/GB·月；个人分发规模月成本通常在个位数元 |

安全性不依赖传输层：清单 Ed25519 分离签名 + 包体 sha256（由签名清单背书），没有私钥的中间人最多造成不可用，无法篡改。

## 2. COS 一次性准备

1. **开桶**：控制台 → 对象存储 COS → 创建桶。地域选离用户近的（如 `ap-guangzhou` / `ap-shanghai`）；桶名形如 `x-hub-dist-1251402600`（**含 APPID 后缀**，rclone/环境变量都要用完整名）。
2. **权限**：桶 → 权限管理 → **公有读私有写**（分发物本来就是公开的）。
3. **CAM 子账号密钥**（不要用主账号密钥）：
   - 访问管理 CAM → 用户 → 用户列表 → 新建用户 → 自定义创建 → 选「可访问资源并接收消息」；
   - 访问方式**只勾「编程访问」**；创建成功后**立即下载 CSV**（`SecretKey` 仅此时显示一次）；
   - 权限两档任选：
     - 速通：预置策略 `QcloudCOSDataFullControl`（官方预设，零配置；作用账号下全部 COS 桶）；
     - 最小化（推荐，锁死单桶）：新建自定义策略 → **按策略语法创建**。action 用服务级通配 `cos:*`（官方子账号授权示例的标准写法，避免逐个枚举 action 踩命名坑——注意 `cos:ListBucket` 是 AWS S3 命名，腾讯云不存在；列对象是 `cos:GetBucket`），安全边界靠 resource 锁桶：
       ```json
       {
         "version": "2.0",
         "statement": [
           {
             "effect": "allow",
             "action": [
               "cos:*"
             ],
             "resource": [
               "qcs::cos:ap-guangzhou:uid/1251402600:x-hub-dist-1251402600",
               "qcs::cos:ap-guangzhou:uid/1251402600:x-hub-dist-1251402600/*"
             ]
           }
         ]
       }
       ```
       （两条 resource 缺一不可：不带 `/*` 的授权桶级操作【列出对象，rclone lsd 依赖】，带 `/*` 的授权对象读写；策略生成器的分字段表单表达不了前者，请走策略语法模式。）
4. 目录布局与原 R2 桶一致（`extensions/…`、`releases/…`），由上传脚本自动维护，无需手动建。

## 3. 部署后验证（curl 三连）

```bash
BASE=https://x-hub-dist-1251402600.cos.ap-guangzhou.myqcloud.com

# ① 清单可访问且不缓存
curl -sI $BASE/extensions/registry.json     | grep -iE 'HTTP|cache-control'   # 200 + no-cache
# ② Range 断点续传（关键！updater 依赖 206）
curl -sI -H 'Range: bytes=0-99' $BASE/releases/update.json | grep -iE 'HTTP|content-range'  # 206 + content-range
# ③ 包体长缓存
curl -sI $BASE/extensions/registry.json.sig | grep -iE 'HTTP|cache-control'   # 200 + immutable
```

## 4. 本地上传通道（Windows 开发机）

环境变量（用户级持久化，`[Environment]::SetEnvironmentVariable(名,值,'User')`）：

| 变量 | 示例 | 用途 |
|---|---|---|
| `COS_SECRET_ID` / `COS_SECRET_KEY` | CAM 子账号密钥 | COS 通道（主） |
| `COS_BUCKET` / `COS_REGION` | `x-hub-dist-1251402600` / `ap-guangzhou` | COS 通道（主） |
| `XHUB_DIST_BASE_URL` | `https://x-hub-dist-1251402600.cos.ap-guangzhou.myqcloud.com` | 发布脚本拼清单内 URL（publish-*.ps1） |
| `XHUB_SIGNING_KEY` | `E:\workspace\.x-hub-signing\market.key` | Ed25519 签名私钥（不变） |
| `R2_ACCOUNT_ID` / `R2_ACCESS_KEY_ID` / `R2_SECRET_ACCESS_KEY` | 原 R2 凭据 | 过渡期 `-Target r2` 双传用（R2 退役后可清） |

用法（上传脚本三通道：`-Target cos` 默认主通道，`-Target r2` 过渡期兜底，`-Target sftp` 备选 Nginx）：

```powershell
# 应用发版
./scripts/publish-release.ps1 -ExePath src-tauri\target\release\x-hub.exe -Version 0.5.1 `
  -SignKey E:\workspace\.x-hub-signing\market.key -Notes "…"
./scripts/upload-release.ps1 -Target cos        # → 腾讯云 COS
./scripts/upload-release.ps1 -Target r2         # → R2（过渡期双传）

# 扩展发布
./scripts/publish-extension.ps1 -ExtDir E:\workspace\x-hub-extensions\extensions\hello-web `
  -SignKey E:\workspace\.x-hub-signing\market.key
./scripts/upload-market.ps1 -Target cos
./scripts/upload-market.ps1 -Target r2
```

脚本均以 rclone remote（环境变量临时配置，不落地）上传，末尾自动做 HTTP 200 + sha256 抽查校验；`upload-release.ps1` 保留 win-x64 只留最近 2 版的清理策略。对象存储通道的缓存头随上传设置（`--header-upload`，即写对象元数据 Cache-Control）。

## 5. CI 通道（GitHub Actions，扩展发布）

`release-extension.yml` 已从 rsync 改回 **rclone + TencentCOS**。GitHub Secrets：

| Secret | 值 | 变化 |
|---|---|---|
| `COS_SECRET_ID` / `COS_SECRET_KEY` | CAM 子账号密钥 | 新增 |
| `COS_BUCKET` / `COS_REGION` | 完整桶名 / 地域 | 新增 |
| `CDN_BASE_URL` | `https://x-hub-dist-1251402600.cos.ap-guangzhou.myqcloud.com` | 原有，改值 |
| `UPDATE_SIGNING_KEY` | Ed25519 私钥（签名用） | **不变** |
| `SSH_PRIVATE_KEY` / `DEPLOY_*` | — | **作废可删**（Nginx 备选才需要） |
| `R2_ACCOUNT_ID` / `R2_ACCESS_KEY_ID` / `R2_SECRET_ACCESS_KEY` / `R2_BUCKET` | — | R2 退役后可删（CI 已不用） |

## 6. 过渡方案：三阶段切换，老用户升级不断链

**原理**：老客户端能否升级，取决于它二进制里烘焙的默认 endpoint（当前 = R2）是否可达；而「清单从哪拉来」与「清单里包 URL 指向哪」互相独立——从 R2 拉到的 update.json 完全可以把包 URL 指向 COS。所以：

- R2 存活期间，老用户永远能升级，且升级目标可以是 COS 上的包；
- 「默认 endpoint 切到 COS」的那个版本是**分水岭版本**：升级过它的客户端从此走新链路；
- R2 的退役时间由「活跃用户版本 ≥ 分水岭版本」决定，与 COS 上线时间解耦，**不存在「一换就升不了级」的窗口**。

### 阶段 0：双活（现在 → 分水岭版发布）

1. COS 就位并按 §3 验证通过（重点 **206**）。
2. **存量拉平**（旧版本包必须留在 COS，各版本客户端都依赖旧路径）：
   ```powershell
   .\scripts\sync-r2-to-cos.ps1    # R2_* + COS_* 环境变量，一条命令同步 extensions + releases
   ```
3. 本机先验证：设置 → 扩展 → 市场源改为 `https://x-hub-dist-1251402600.cos.ap-guangzhou.myqcloud.com/extensions/registry.json`（UI 可改），刷新市场能拉清单、能安装扩展。
4. **每次发版双传**（§4 的 `-Target cos` + `-Target r2` 都跑），保证两边 update.json / registry.json 一致；清单内的包 URL 优先指 COS（提前分流下载），R2 仅作清单可达性兜底。

### 阶段 1：分水岭版本

- 改 `src-tauri/src/config.rs` 的 `DEFAULT_MARKET_ENDPOINT` / `DEFAULT_UPDATE_ENDPOINT` → COS 域名，发版；
- 该版本照常双传：老客户端从 R2 拿到这份 update.json → 包从 COS 下载 → 升级完成 → 从此走新链路；
- 分水岭版发布后仍**保持双传**，进入观察期。

### 阶段 2：观察期（建议 ≥ 4~8 周，覆盖 2~3 个发版周期）

- 继续双传；盯 Cloudflare R2 仪表盘的 Class B（读）操作数衰减；
- 读量降到接近零 = 活跃客户端基本都过了分水岭 → 进入阶段 3。

### 阶段 3：R2 优雅退役（302 兜底，客户端已验证跟随重定向）

- `market.rs` / `updater.rs` 的 reqwest 客户端均未关闭重定向（默认跟随最多 10 次），因此可在 Cloudflare 给 `r2.dckxx.com` 配 **Redirect Rule**：动态重定向 `concat("https://x-hub-dist-1251402600.cos.ap-guangzhou.myqcloud.com", http.request.uri.path)`，状态码 302——清单/`.sig`/包/图标按路径通配全部覆盖；
- DNS 侧：R2 桶的自定义域绑定可解除，但保留 `r2.dckxx.com` 的 DNS 记录并开启橙云代理（占位记录即可），Redirect Rule 才能接管该主机名的请求；
- 配好后旧客户端拉清单/包都会被 302 到 COS，**完全无感**；R2 桶内对象随后可清空（只留重定向规则），零流量成本；
- 最省事的替代：直接停止双传、R2 桶保留不删——旧客户端检查更新静默失败只是收不到更新提醒，不影响使用（市场验签失败回退本地缓存）。二选一，302 更体面。

## 7. 备案完成后的可选优化（非必需）

COS 默认域名长期可用，不必动。备案下来后如想用自己的域名：COS → 默认 CDN 加速域名 / 自定义源站域名绑定 + 上传证书 → `XHUB_DIST_BASE_URL` / `CDN_BASE_URL` 换新域名 → 发一版切默认 endpoint。步骤与三阶段切换同构。

## 8. 备选：自建 Nginx 静态托管（当前不启用，留作 PLAN B）

适用场景：想彻底省流量费、或 COS 不可用时。准备材料都在：

- `scripts/server/init-server.sh` + `scripts/server/x-hub-dist.conf`：服务器一键初始化（装 nginx / 建目录与 deploy 账号 / 写站点配置 / 放行防火墙），幂等可重复执行；缓存策略（清单 no-cache、包体 immutable）在 Nginx 配置里统一管理。
- 上传走 `-Target sftp`（环境变量 `XHUB_DEPLOY_HOST` / `XHUB_DEPLOY_PORT` / `XHUB_DEPLOY_USER` / `XHUB_DEPLOY_KEY`）；本机已生成部署密钥 `~\.ssh\xhub_deploy_ed25519`（本地用）与 `~\.ssh\xhub_deploy_ci_ed25519`（CI 用）。
- 备案期间只能 `http://<IP>:8080`（80/443 对未备案域名有拦截，非标端口直连 IP 不受限）；备案后切 `listen 443 ssl` + certbot 证书。

## 9. 与 R2 方案的差异备忘

| 关注点 | R2（现状） | COS（目标） |
|---|---|---|
| 域名 | 自定义域 `r2.dckxx.com`（Cloudflare 签发证书） | 默认域 `<bucket>.cos.<region>.myqcloud.com`（腾讯签发证书） |
| 上传 | rclone s3 provider Cloudflare | rclone s3 provider TencentCOS |
| 缓存头 | 上传时 `--header-upload` | 同左（写对象元数据） |
| 断点续传 | 边缘支持 206 | 支持 206 |
| 出口流量 | 免费 | ~0.5 元/GB（国内），个人规模月成本个位数元 |
| 签名/验签 | 不变 | 不变 |
| 清单格式 | 不变 | 不变 |
| 退役兜底 | — | R2 侧 Redirect Rule 302 → COS（reqwest 默认跟随重定向，已验证代码未关闭） |

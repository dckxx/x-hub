# x-hub · macOS 移植说明

本目录 `x-hub-mac` 是 [dckxx/x-hub](https://github.com/dckxx/x-hub)（Tauri 2 + Vue 3 + TypeScript + Rust 的本地个人效率工作台）的 **macOS 移植版**。

原项目主要面向 Windows（`%APPDATA%\x-hub` 数据目录、WebView2、注册表 Run 键自启动、exe/.lnk 图标提取、UAC 提权、Explorer 文件管理等），但其代码在写的时候就已大量内置了跨平台回退（大量 `#[cfg(not(target_os = "windows"))]` 分支 + `CommandOrControl` 快捷键默认值），所以移植工作量集中在「补齐 macOS 真正缺失的能力」上。

## 一、本次为 macOS 做了什么改动

| 文件 | 改动 | 说明 |
| --- | --- | --- |
| `src-tauri/src/autostart.rs` | ~~macOS LaunchAgent 自启动~~ **已按用户要求移除**（2026-10-31） | 用户明确要求去掉：设置页开关、store/api 调用链整体下线；启动时静默清理历史残留（配置开关置回 false + 删除可能存在的 LaunchAgent），防止「无 UI 可关」的僵尸自启动。后端 get/set 命令保留作后续恢复入口 |
| `src-tauri/src/app_icon.rs` | **新增 macOS .app 图标提取** | 读 `.app` 包 `Contents/Info.plist` 的 `CFBundleIconFile`，用系统 `plutil` 取值、`sips` 把 `.icns` 转 PNG（无新增 Rust 依赖、失败优雅回退）；Windows 的 `exe/.lnk` COM 提取仅 Windows 编译 |
| `src-tauri/src/process.rs` | 新增 macOS `.app` 包启动 | 路径为 `.app` 目录时改走系统 `open`（否则 `Command::new` 无法直接启动目录），参数经 `open --args` 传入 |
| `src-tauri/Cargo.toml` | 门控 Windows 专用 crate | `webview2-com`、`windows-core` 移入 `[target.'cfg(target_os="windows")'.dependencies]`，macOS 不再编译它们；并为 `tauri` 增加 `macos-private-api` feature（启用 macOS 的透明/无阴影窗口能力） |
| `src-tauri/src/main.rs` | 门控 `windows_subsystem` | 该属性仅 Windows 生效，改为 `cfg_attr(all(not(debug_assertions), target_os="windows"), …)` |
| `src-tauri/tauri.conf.json` | macOS 打包配置 | `identifier` 改为反域名 `com.xhub.app`；`bundle.active=true`、`targets=["app","dmg"]`；主窗去掉 Windows 专用 `additionalBrowserArgs`；`app.macOSPrivateApi: true`（配合 Cargo feature，非窗口字段） |
| `src-tauri/src/clipboard_mac.rs` | **新增 macOS 剪贴板模块** | 原 `clipboard.rs` 依赖 Windows 剪贴板 API（WM_CLIPBOARDUPDATE、鼠标钩子、SendInput 注入），在 macOS 链接不过。改为平台分流：Windows 用 `clipboard.rs`，macOS 用 `clipboard_mac.rs`（`pbpaste`/`pbcopy` 实现文本读写 + 后台轮询历史 + 剪贴板浮层窗口；图片/文件/富文本本版暂不支持） |
| `src-tauri/src/ext_protocol.rs` + `proxy.rs` + `lib.rs` | **macOS 扩展内容改走本地 HTTP** | WKWebView 实测限制：主窗是自定义协议（`tauri://localhost`）时，指向**另一个**自定义协议（`xhub-ext://`）的 iframe 子帧请求不会派发给 scheme handler，扩展页永远空白（8 秒后前端看门狗判白屏）；Windows 无此问题（WebView2 把自定义协议映射成 `http://…localhost`）。修复：macOS 上扩展内容由既有本地服务器承载（复用 proxy.rs 的 127.0.0.1 动态端口，扩展内容占根命名空间 `/<extId>/<rel>`，`/svc/` 仍为后端代理前缀），入口 URL 变为 `http://e-<hash>.localhost:<port>/…`——`*.localhost` 在 macOS 解析到 127.0.0.1，每扩展仍独立 origin；Host 身份比对天然挡住 DNS 重绑定；CSP frame-src/img-src 加 `http://*.localhost:*` 端口通配 |
| `src-tauri/src/runtime.rs` | **Node 探测增强** | GUI 启动的进程不继承登录 shell 的 PATH（launchd 默认只有 `/usr/bin:/bin:/usr/sbin:/sbin`），service 扩展后端找不到 node。修复：PATH 找不到时按序探测 `/opt/homebrew/bin`、`/usr/local/bin`、volta、nvm（取最高版本）、本机 AI 助手捆绑运行时（`~/.local/share/TeleAgent/runtimes/node/bin/node`）等常见位置，并返回**绝对路径**给后端 spawn |
| `src-tauri/src/lib.rs` + `proxy.rs` | **扩展内容请求日志与诊断探针** | 内容路由每次请求记 `[扩展内容]` 日志（有无请求是白屏排查的分水岭证据）；`XHUB_EXT_DIAG=<扩展id>` 启动时向主窗自动注入探针 iframe，事件（onload/桥消息/文档状态）双通道回传（document.title 轮询 + Image()→403 日志），headless 即可定位扩展加载问题 |
| `src/composables/useExtensionFrame.ts` | **WebKit 双跳导航竞态修复（间歇白屏根因）** | `load()` 先设 `src='about:blank'` 销毁旧页，`await read_extension_entry`（本地 IO 常在几毫秒内返回）后再设真实入口——两次导航落在同一周期时 **WKWebView 会丢弃第二次**，iframe 永停 about:blank → 8 秒看门狗判白屏（实测 0ms 间隔必丢、≥10ms 稳定；WebView2/Chromium 无此行为，故 Windows 从未暴露；IPC 耗时波动 → 时好时坏）。修复：真实入口延迟 30ms 到下一宏任务再设，带 generation/disposed 守卫 |
| `src/components/SudaFormDialog.vue` + `src/utils/platform.ts` | **速达「添加应用」macOS 适配** | 原「程序」选择器写死 `filters: ['exe','lnk']`——macOS 文件选择框里其余文件全部置灰，根本选不了。修复：mac 上不做扩展名过滤（`.app` 包在 NSOpenPanel 里整体可选，命令行工具/脚本无扩展名也不挡），新增 `IS_MAC` 平台工具；程序路径占位文案 mac 化 |
| `src-tauri/src/commands.rs` | **`parse_dropped_path` 支持 `.app`** | 原只认 `.exe/.lnk`，选/拖 `.app` 直接报「仅支持 .exe」。新增 `.app` 分支：名称取包目录名、target 即 `.app` 目录（启动走 `open`）、图标经 `app_icon::extract_icon_png` 读 `CFBundleIconFile` 转 PNG 落缓存（实测 Safari → 256×256 PNG）；错误文案按平台区分；拖拽导入同链路受益；单测 `parse_dropped_path_supports_mac_app_bundle` 钉住行为 |
| `src-tauri/src/commands.rs` | **速达「扫描已安装应用」macOS 暂缓**（用户决定） | 原 PowerShell 扫开始菜单/MSIX 专属，macOS 上不可用。按用户决定不移植，手动添加为主路径；后续若需要可扫 `/Applications` 的 `.app` 包 |
| `src-tauri/src/secret.rs` + `Cargo.toml` | **备注落盘加密补齐（原非 Windows 直接报错）** | 原「备注加密仅支持 Windows」——macOS 上给速达资源设备注会直接报错，旧库里的 `dp1:` 密文也读不出。补齐为 AES-256-GCM（新增 `aes-gcm` 依赖，仅非 Windows 编译）：密钥 32 字节随机，存**数据目录之外**的 `<config_dir>/x-hub-security/remark.v1.key`（0600），与 DPAPI「凭据不随数据目录迁移」语义对齐（同机同用户可解，数据目录搬走后按无备注处理）；选密钥文件而非钥匙串，避免本地 ad-hoc 重签后失去钥匙串 ACL；两平台共用 `dp1:` 前缀，跨平台残值互解不开 → None，不会误当明文 |
| `src-tauri/src/process.rs` | **`exe_file_name` 双分隔符解析** | 原用 `Path::file_name` 只认平台分隔符，macOS 上把整串 `C:\…\chrome.exe` 当文件名，跨平台同步的速达资源进程名匹配全废；改为按 `/` 与 `\` 双分隔符取末段（尾分隔符行为与 `Path` 对齐） |
| `src-tauri/src/process.rs` | **`launch_program` 存在性校验** | `sh -c` 对不存在路径与 Windows `cmd /C` 一样「假成功」（spawn 成功、命令在 shell 内才失败），点错路径界面毫无反应。对单一路径形态（无空白、含分隔符）目标先校验直接报错；带空白的 shell 片段与裸命令名（如 `code`，PATH 查找）保持 cmd /C 等价能力不变 |
| `src-tauri/src/webview_mem.rs` | 守卫测试平台门控 | `additionalBrowserArgs` 是 WebView2 专属字段，主窗配置在 macOS 上有意移除；对应 P3 守卫测试改为仅 Windows 生效，防止平台差异被误「修」回来 |
| `.cargo/config.toml` | crates.io 国内镜像（项目级） | 环境网络拉取 crates.io 索引超时，配置 rsproxy.cn sparse 镜像加速依赖解析；仅本项目生效、不动全局环境，网络通畅时可删除 |
| 设置页快捷键 UI 全面 mac 化（`useShortcutRecorder.ts` + `GeneralPanel/TitleBar/GlobalSearch/NoteEditor/ClipboardOverlay.vue`） | **快捷键展示/录入/占位平台化** | 代码审查结论：快捷键的注册/改绑/启用链路（tauri-plugin-global-shortcut 2.3.2 + global-hotkey 0.8.0，Carbon RegisterEventHotKey）与前后端分发代码均跨平台健全，启动日志注册成功；但展示层全是 Windows 写法：mac 上显示原始 token「CommandOrControl+…」、占位/兑底硬编码「Ctrl+…」、自启文案写死「登录 Windows 后…驻留托盘」。修复：mac 展示别名 CommandOrControl→Cmd、Alt→Option（存储值仍为规范串，手工输入别名在保存前经 formatShortcutTokens 归一）；占位/兑底按平台给出；标题栏/搜索弹窗/速记提示同步平台化 |
| `settings/FeaturesPanel.vue` | **mac 上两项无效设置的诚实标注** | 「粘贴方式」（Windows 的 SendInput 注入专属，clipboard_mac 不消费）与「隐藏窗口时降低内存占用」（webview_mem.rs 全部 cfg(windows)）在 mac 上是死开关：不隐藏而是置灰 + 说明文案（mac 剪贴板行为：点记录项后按 Cmd+V 粘贴；WKWebView 由系统自动管理内存） |
| `src-tauri/src/floating_ball.rs` | **悬浮球 macOS 移植（原 Windows-only，窗口在 mac 从不创建）** | 核心链路全面跨平台化：窗口创建（透明/置顶/无阴影/不抢焦点/跨 Space 常驻）、初始位（work_area = NSScreen.visibleFrame 同口径）、主窗显隐联动、环形菜单开合几何（expand/reapply）、设置即时生效（save_settings→apply_enabled）。**贴边半隐/滑出全套移植（2026-11-01）**：edge_tick/dock_snap/救球/落位补齐/settle_drag 常量与状态机整体 un-cfg；mac 输入源 = HIGetMousePosition（Carbon HIToolbox FFI，逻辑点）+ CGEventSourceButtonState(stateID,button)（真实签名无需 source 引用）；光标逻辑点→物理 px 按所在显示器 scale 换算（显示器 2s 缓存，本机实测与 NSEvent y-翻转×scale 期望值零偏差）；松手判定双保险（左键已松 + Moved 停跳≥250ms）；8 个几何不变式测试 + 4 个 mac FFI/换算测试首次在 mac 启用 |
| `src-tauri/src/clipboard_mac.rs` | **剪贴板图片/文件/富文本补齐（原仅文本）** | JXA（osascript -l JavaScript）桥接 NSPasteboard，无新增原生依赖：probe_meta 读 changeCount 统一门控（1.2s 轮询，~43ms/跳，计数未变整跳跳过）；图片 public.png/tiff 直落 PNG 快照（tiff 转码失败存 .tiff 兜底，写回剪贴板时再转）；文件 readObjectsForClasses 读 NSURL 列表（写入走 writeObjects NSURL 直写，自动补 NSFilenamesPboardType 传统类型，Finder 同款）；富文本 public.html 随文本入库/写回双类型；回声抑制/去重/暂停与 Windows 同口径。**JXA 桥接坑（实测）**：零参方法（clearContents）属性访问即调用、NSInteger 属性（changeCount）经 JSON.stringify 变字符串需 Number() 转回、CGEventSourceGetCursorPosition 函数根本不存在。另修 paste_to_previous_window：图片/文件粘贴传空内容时不可再写剪贴板（会把刚写入的数据覆盖成空文本），只收浮层还焦点 |
| `src-tauri/src/notify.rs` | 补非 Windows `hide_window` | 该函数原仅 Windows 定义但被无条件调用，macOS 补 `win.hide()` 实现 |
| `settings/GeneralPanel.vue` | 悬浮球「贴边自动隐藏」mac 解禁 | 贴边监视移植完成后开关恢复可用（2026-11-01，原先行内曾置灰） |
| `src-tauri/src/clipboard.rs` | 门控 `is_main_window` 的 `hwnd()` | `hwnd()` 是 Windows 专属方法，非 Windows 恒返回 false |
| `src-tauri/src/commands.rs` | 门控打包应用图标解析分支 | `resolve_packaged_icon_source`（shell:AppsFolder）仅 Windows 编译，非 Windows 直接用 target 作位图来源 |
| `src-tauri/src/app_icon.rs` | `resolve_lnk_target` 覆盖所有非 Windows | macOS/Linux 均返回“仅支持 Windows”，避免 commands.rs 无条件调用处编译失败 |
| `scripts/build-mac.sh` | **新增** | macOS 一键构建脚本（依赖检查 + npm install + tauri build / dev / test） |
| **上游 v0.9.6 同步**（2026-11-01） | 43 文件 patch + 7 冲突手工合并 | 上游 v0.9.5→v0.9.6（截图标注/GIF 录屏/待办一键完成逾期/日历圆点勾选/整夹导出合并/滚动条修复/构建瘦身）。36 文件干净应用；7 冲突中 Cargo.lock 与 package-lock 取上游、Cargo.toml/tauri.conf.json/tauri.ts/TitleBar/GeneralPanel 手工合并（保留 tiff/自启移除/贴边解禁/快捷键平台化等本地改动）。**recorder.rs 上游自带平台分支**：采集 mac 走 xcap、光标叠加/窗口自动识别为 Windows 分支（mac 无）；我们补 clipboard_mac.rs 的 `set_clipboard_image_bytes`（上游只给了 clipboard.rs 的 Windows 实现，截图进剪贴板的关键缺口，实现为临时文件中转 JXA 写 public.png） |
| `src-tauri/src/clipboard_mac.rs` | 补 `set_clipboard_image_bytes`（上游 v0.9.6 新增接口） | 字节入口统一转 PNG 经临时文件中转给 JXA 写 public.png；原 `set_clipboard_image` 重构为读文件后走字节入口，两处口径统一；截图（不落盘）经此进剪贴板 |

**前端无需大改**：`src/stores/workbench.ts`、`src/composables/useShortcutRecorder.ts` 已通过 `navigator.userAgent/platform` 检测，把默认全局快捷键自动切成 `CommandOrControl+…`；`shortcut.rs` 后端也已有 `#[cfg(target_os = "macos")]` 的 Cmd 默认值。

## 二、macOS 上已天然可用的部分（无需改动）

- **路径**：`paths.rs` 用 `dirs::config_dir()` → macOS 落在 `~/Library/Application Support/x-hub`，数据根解析/便携版逻辑已跨平台
- **全局快捷键**：`tauri-plugin-global-shortcut`（跨平台）
- **系统托盘**：`tauri-plugin-tray-icon`（跨平台）
- **本地通知/倒计时**：`notify.rs`、`countdown_window.rs` 等均有非 Windows 回退
- **系统资源监视**：`sysinfo`（跨平台）
- **速达、全局搜索**：均有 `cfg(not(target_os = "windows"))` 兜底；**悬浮球已完整移植**（见上表）
- **云端/账号/AI 对话/更新**：纯 Rust + reqwest + keyring，与平台无关（keyring 已启用 `apple-native`）
- **剪贴板历史**：macOS 用独立的 `clipboard_mac.rs`（见上表），支持文本历史；图片/文件/富文本暂不支持（后续可引入 NSPasteboard）

## 三、在 macOS 上构建运行

### 前置依赖（一次性）

```bash
# 1) Node.js 18+（官网或 brew install node）
node -v

# 2) Xcode Command Line Tools（链接器/WKWebView 需要）
xcode-select --install

# 3) Rust 1.77.2+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

> 说明：工具链已就绪并全量验证（Rust 1.99 经 rustup 装在 `~/.cargo`，Xcode CLT 已装；cargo/npx 需先 `. "$HOME/.cargo/env"`）。Windows 专属测试均已按平台门控或跨平台化，`cargo test` 全绿（466 通过，见下方验证记录）。

### 构建

```bash
# 开发调试窗口
./scripts/build-mac.sh --dev

# 打包 .app + .dmg
./scripts/build-mac.sh

# 仅跑 Rust 单元测试
./scripts/build-mac.sh --test
```

产物：`src-tauri/target/release/bundle/macos/x-hub.app`、`…/bundle/dmg/x-hub_*.dmg`

### 最终验证记录（2026-10-31）

- `cargo test` 全绿：**466 通过 / 0 失败**（含备注加密跨平台化后的 4 个加密回归、`.app` 解析单测；`webview_mem` 守卫按平台正确关闭）
- 备注加密实测：密钥生成于 `~/Library/Application Support/x-hub-security/remark.v1.key`（32 字节、0600、数据目录之外）
- 探针回归（`XHUB_EXT_DIAG=com.laohong.music`）在最终包上连续两轮通过：`about:blank → 真实入口（30ms 错峰）` 导航完成、`iframe-onload` 触发、入口 `200`、扩展桥消息（`theme.get`/`runtime.info`）到达、8 秒存活无白屏
- 待真人 GUI 验证：反复开关扩展不白屏；速达「添加应用」选 `.app` 得名称+图标并可启动（2026-10-31 19:01 用户实测已过：拖入 WeChat/天翼云 → 图标正常 → 可启动）；文档/网页添加与启动
- 快捷键链路代码审查通过（注册/改绑/冲突预检/回滚/事件分发/前端监听均健全）；**启动实测：5 个快捷键全部注册成功、零冲突**（主窗 Cmd+Shift+Space / 剪贴板 Cmd+Option+V / 搜索 Cmd+K / AI 对话 Cmd+Shift+K / 速记 Cmd+Shift+N）
- **快捷键真机验证已过（2026-10-31 19:41 用户实测）**：在任意前台应用按 Cmd+Shift+Space → 日志出现 `[快捷键] 事件触发: shift+super+Space` 且 `toggle_window: 窗口被盖住，提升到前台`——按键 → Carbon 事件 → 分发 → 动作全链路可用，无需辅助功能权限
- 悬浮球：`cargo test` 478 全绿（含 12 个贴边几何/FFI/换算测试）+ 前端构建通过；运行时验证点 = 换装新包启动后日志应出现「悬浮球窗口已创建」（失败则记「悬浮球窗口创建失败: …」）；贴边交互验证（拖到边缘吸附半隐/悬停滑出/移开隐回/甩出屏外救球）待用户装新包实测
- 剪贴板闭环实测（2026-11-01）：同进程 JXA 写入→立即读回码点正确；图片写→探→读回 sha 一致；多文件含中文/空格路径往返一致；文件读取走 readObjectsForClasses 主路径
- 上游 v0.9.6 同步验证（2026-11-01）：Rust 481 全绿（含上游新增 3 测试）、前端 141 全过、构建通过；截图/GIF 录屏首次使用时系统会弹「屏幕录制」权限（xcap 采集，拒绝后采集报错），GIF 帧率性能与选区层收窗延迟待实测（mac 无 GDI BitBlt 快路径，逐帧 xcap）

### ⚠️ 密钥文件须知

速达「备注」的加密密钥在 `~/Library/Application Support/x-hub-security/remark.v1.key`：

1. **不要把数据目录备份恢复到其它机器/用户后期待备注仍可读**——密钥不随数据目录迁移（与 Windows DPAPI 同一语义）
2. 该文件丢失/删除后，旧密文按「无备注」处理（不报错、不毒化数据），新备注会以新密钥正常加密
3. 重装系统/换机前想保留备注：把这个文件随数据一起备份并在新机放回同一位置

### 常见 macOS 注意事项

1. **首启网关**：未签名 / ad-hoc 签名的本地构建首次打开会提示「无法验证开发者」——在系统设置 → 隐私与安全性 →「仍要打开」，或 `xattr -dr com.apple.quarantine /path/to/x-hub.app`。
2. **全局快捷键/剪贴板**：首次会请求「辅助功能」/「输入监控」权限，需到 系统设置 → 隐私与安全性 里勾选 x-hub。
3. **自启动**：已按用户要求移除（2026-10-31）；启动时会自动清理历史残留的 `~/Library/LaunchAgents/com.xhub.app.plist`。
4. **签名/公证**：若要分发给他人的 `.dmg`，需配置 Apple Developer ID 签名 + notarization（本项目 `bundle.macOS` 未配签名，交付的本地构建适合自用/内网）。
5. **「天翼云电脑」剪贴板同步干扰（环境因素，非本应用缺陷）**：云桌面客户端开着时会双向同步剪贴板，把本机内容经云端 Windows（GBK 代码页）回写——中文文本会被改写成 GBK 字节、多文件列表被展平成只留第一个。实测铁证：同进程写入后立即读回码点完全正确，300ms 后跨进程读被改成 GBK。排查剪贴板问题时先关云客户端对照，勿误判为应用 bug。
6. **截图/GIF 录屏需要「屏幕录制」权限**（上游 v0.9.6 功能）：首次按 Cmd+Shift+A/G 时系统弹「"x-hub"想要录制屏幕」，需在 系统设置 → 隐私与安全性 → 屏幕录制 里勾选 x-hub；拒绝后采集报错（选区层无画面）。已知 mac 缺项：GIF 内不叠加鼠标光标、选区层无「光标下窗口自动识别高亮」（上游均为 Windows 分支，需时再移植）

## 四、Windows 独有功能全面清点（2026-11-01 自查）

### A. 本轮已修复（曾经是 Windows 专属，现已 mac 化）

| 功能 | 修复 |
| --- | --- |
| **更新器平台键（高危）** | 原 `platform_entry` 写死 `windows-x86_64`——mac 上会提示安装 **Windows 更新包**（点了会装 exe）。改为按目标平台取键（macOS 查 `darwin-aarch64`），官方清单无对应条目即按「无更新」处理。⚠️ 修复前的包里**别在 设置→关于 手动点「检查更新」** |
| 速达右键「以管理员身份运行」 | UAC 提权无 mac 对应语义，菜单项在 mac 上隐藏（原来显示、点了报错） |
| 速达右键「打开文件所在位置」 | mac 改用 `open -R`（Finder 中定位并选中，与 explorer /select 同语义；原实现只打开父目录不选中） |
| 通知窗 mac 锚定 | 原用整屏尺寸（通知会被程序坞压住），改用 `work_area`（NSScreen.visibleFrame，自动避让菜单栏/程序坞） |
| 设置文案「…交由 Windows 按系统设置打开」 | 速达网页打开方式描述平台化 |
| **剪贴板图片/文件/富文本记录**（2026-11-01） | JXA 桥接 NSPasteboard 补齐（clipboard_mac.rs），写入→探针→读回闭环实测：图片字节哈希一致、多文件含中文/空格路径往返一致、富文本 text+html 双类型正确 |
| **悬浮球贴边半隐/滑出**（2026-11-01） | edge_tick 全套移植（见第一节 floating_ball.rs 行）；8 个几何不变式测试 + 4 个 mac FFI/换算测试首次在 mac 启用，478 全绿；贴边交互（吸附半隐/悬停滑出/移开隐回/甩出屏外救球）待用户换装实测 |
| **速达「用指定浏览器打开」**（2026-11-01） | browsers.rs 扫 /Applications、/System/Applications、~/Applications 的 *.app，plutil 读 Info.plist，CFBundleURLTypes 含 http scheme 即浏览器（只认 http——微信/QQ 等仅注册 https 回跳的不算），10 并行 worker 控制冷扫延迟；process.rs open_with_browser mac 分支走 `open -a`（自带复用已运行实例+置前）。本机实测检出 Safari（唯一已装浏览器），前端 Suda.vue 零改动 |

### B. 已完整可用（无需任何移植）

全局快捷键（真机验证）、悬浮球、托盘/菜单栏、剪贴板、通知窗、倒计时浮窗、便签（贴/独立窗）、AI 对话（抽屉/独立窗）、速达浏览器窗、导出渲染窗、备份/恢复（zip）、数据迁移、主题/外观、系统监控（sysinfo）、扩展系统（加载/桥/市场/service/反馈）、速记、待办、PDF 导出（WebView2 静默写盘不可用→自动回退打印对话框，系统「另存为 PDF」）、备注加密（AES-256-GCM）。

### C. 不需要移植（平台语义不存在，UI 已置灰/隐藏或按设计保留）

- `webview_mem` WebView2 内存级别（mac 系统自动管理）→ 开关置灰
- 「粘贴方式」按键注入（mac 无自动粘贴，点记录项→收浮层→手动 Cmd+V）→ 置灰
- `win_taskbar` 任务栏 API、UWP/AUMID 启动、AppxManifest/.lnk/.exe 图标（mac 只有 .app）、service 防火墙 COM（mac 无防火墙弹窗）、secret DPAPI 分支（mac 有对等 AES-GCM）
- ~~NSWorkspace 激活已有实例~~：mac 的 `open` 本身就会切回已运行实例，**无需移植**（关闭原 Roadmap 条目）

### D. 剩余待移植 backlog（按建议优先级）

1. **使用说明**：内联渲染的 docs/user-guide.md 是 Windows 手册（教 UAC/资源管理器等）——需内容级 mac 适配
2. **工程化**：release.yml 增 mac 构建与 .dmg 产物；分发需 Developer ID 签名 + 公证；更新链路（COS + Ed25519）的 mac 安装/替换路径未验证——在官方清单提供 darwin 条目前，更新检查在 mac 上按「无更新」处理
3. **速达裁剪/本地化**：用户已定调「macOS 有访达，速达价值不大」——保留待改（2026-10-31 用户决定）；「扫描已安装应用」同样暂缓

> AI生成

## 五、上游同步流程（SOP，2026-11-01 首次执行沉淀）

本地副本不是 git 仓库（解压源码 + mac 移植改动），与上游（GitHub dckxx/x-hub）同步的固定流程：

1. **拉取上游两版源码**：`.temp/upstream` 浅 fetch 上一 tag 与新 tag（如 v0.9.5 / v0.9.6）
2. **生成 patch 并预检**：`git diff <旧tag> <新tag> > patch`，到项目根 `git apply --check` 分出「干净可应用」与「冲突」两组
3. **应用**：干净文件直接 `git apply --exclude=<冲突文件>...`；冲突文件逐个处理——lock 文件（Cargo.lock / package-lock.json）直接取上游，实质文件（Cargo.toml / tauri.conf.json / tauri.ts / TitleBar / GeneralPanel 等）手工合并，**保留本地 mac 移植改动**（tiff、自启移除、贴边、快捷键平台化、JXA 剪贴板桥等）
4. **平台缺口排查**：新功能若引用 Windows-only API，先 `cargo check` 探明真实缺口再补 mac 实现（recorder 例：上游自带 xcap 平台分支，只缺 clipboard 的 `set_clipboard_image_bytes`）
5. **验证**：`cargo test` 全量 → `npm run test:utils` → `npm run build` → `npx tauri build` → 换装（quit → 旧包入废纸篓 → ditto → open）→ 启动日志核对（快捷键注册/悬浮球/剪贴板线程）
6. **版本号跟随上游**（用户规则，2026-11-01 定）：上游升版后本机同步更新，不自行起号；`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` 三处版本号一致，lock 文件随上游取

> AI生成

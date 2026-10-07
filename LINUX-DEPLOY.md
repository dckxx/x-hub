# x-hub Linux 部署文档

面向 **v0.7.6** 的 Linux 发行版部署说明：装什么依赖、怎么装包、装完在哪看数据与日志、
以及 Linux 版和 Windows 版**确实不一样**的那几处（先看第 6 节，能省掉一大半「这是不是 bug」的来回）。

需要「WebView 为什么不能自动下载」「glibc 基座」「从源码编译」这类原理性问题时，配合读
[LINUX.md](LINUX.md)；本文只讲**部署**。

---

## 1. 支持范围

| 发行版 | 版本 | 包格式 | 状态 |
|--------|------|--------|------|
| Ubuntu | **24.04 LTS** | `.deb` | ✅ 实测正常运行 |
| Ubuntu | **22.04 LTS** | `.deb`（源码构建） | 🧪 自动化测试通过（2026-10：cargo test 291 项 / 源码构建 deb / 安装启动 / 窗口渲染均正常），**未经人工验证** |
| Fedora | **44** | `.rpm` | ✅ 实测正常运行 |
| Ubuntu / Fedora | 高于上述版本 | 同上 | ⚠️ 预期可用，未逐一验证 |
| Debian 系（Debian / Mint / Pop!_OS…） | 对应 Ubuntu 22.04+ 年份 | `.deb` | ⚠️ 机制相同，需自行验证 |
| RHEL 系（RHEL / Rocky / Alma / openSUSE…） | 对应 Fedora 37+ 年份 | `.rpm` | ⚠️ 机制相同，需自行验证 |
| 更老的发行版（CentOS 7、Ubuntu 20.04 等） | — | — | ❌ 仓库里没有 WebKitGTK 4.1，装不了 |

两条硬性下限：

1. **必须有 WebKitGTK 4.1**（`libwebkit2gtk-4.1.so.0`）。它大约从 Debian 12 / Ubuntu 22.04 /
   Fedora 37 / 同年份滚动发行版开始提供；更老的系统没有这个 `.so`，也没有替代安装包。
2. **预编译包只提供 x86_64**。其他架构（aarch64 等）需要自行从源码编译。

> **glibc 提醒**：目前 `.deb` / `.rpm` 预发布由 GitHub Actions 的 `ubuntu-latest`（当前 = Ubuntu 24.04）
> 构建，产物依赖较新的 glibc。**Ubuntu 22.04 及更旧系统大概率报 `GLIBC_2.3x not found`**，
> 那类机器需要在 22.04 基座上自行构建（见第 8 节；2026-10 已在 Ubuntu 22.04.5 实机完成
> 自动化验证：cargo test 全过、源码构建 deb、安装启动正常，但该构建未经人工验证）。
> 自查办法：
>
> ```bash
> strings /usr/bin/x-hub | grep -oE 'GLIBC_2\.[0-9]+' | sort -uV | tail -3
> ```

---

## 2. 运行时依赖

应用启动时动态链接器**进 `main` 之前**就要加载 WebKitGTK，缺库直接起不来（应用内没有任何
机会提示，也不可能自下），所以先用包管理器装一次即可。

`.deb` / `.rpm` 都已经把下面这些写进依赖，用 `apt` / `dnf` **装官方包**时会自动带上，正常不用手装：

```bash
# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2

# Fedora / RHEL
sudo dnf install webkit2gtk4.1 gtk3 libayatana-appindicator3-gtk3 librsvg2

# Arch / Manjaro
sudo pacman -S webkit2gtk-4.1 libappindicator-gtk3 librsvg

# openSUSE
sudo zypper install libwebkit2gtk-4_1-0 libayatana-appindicator3-1 librsvg-2-2
```

装的是 **4.1**。仓库里的 `webkit2gtk-4.0` 对不上，装了也不认。

`libayatana-appindicator3` 用于**系统托盘图标**；某些 RPM 系发行版只有 `libappindicator-gtk3`，
两者任装其一即可。

---

## 3. 一键安装（推荐）

仓库自带脚本 `scripts/install-linux.sh`：读 `/etc/os-release` 判断 deb / rpm → 装运行时依赖
→ 从 GitHub Release 挑最新的 Linux 包 → 下载 → 交给包管理器安装。

```bash
# 在仓库里
chmod +x scripts/install-linux.sh
scripts/install-linux.sh

# 或者单独把脚本拿走，在目标机上跑（只需 curl + python3）
bash install-linux.sh
```

脚本依赖：`curl`、`python3`（解析 GitHub API），并需要 `sudo`（除非以 root 运行）。

### 常用选项

| 选项 | 说明 |
|------|------|
| `--repo owner/name` | 指定仓库（默认 `dckxx/x-hub`） |
| `--tag TAG` | 指定 Release tag（默认取最新的 `v*-linux.*`） |
| `--pkg deb\|rpm` | 强制包格式（默认按 `/etc/os-release` 判断） |
| `--skip-deps` | 不装系统运行时依赖 |
| `--dir PATH` | 下载目录（默认 `$TMPDIR/x-hub-pkg`） |
| `--help` | 帮助 |

环境变量形式等价：`REPO`、`TAG`、`PKG`、`DOWNLOAD_DIR`，另有 `GH_TOKEN` / `GITHUB_TOKEN`
（Release 还是 draft 时必需，也能提高 API 限额）。

### 指定版本安装

Linux 包走的是**预发布**（prerelease），tag 形如 `vX.Y.Z-linux.<短SHA>`，例如：

```bash
scripts/install-linux.sh --tag v0.7.2-linux.<短SHA>
```

用 `--tag` 可以把目标机钉在某个确定构建上（推荐在生产机器这么做，避免下次重装被换成别的提交）。

若默认仓库暂时没有 `-linux.` 预发布，脚本会明确报错并提示；此时用 `--repo` 指向有该
预发布附件的镜像仓库，或直接用 `--tag` 指定已存在的 tag。

---

## 4. 手动安装

不想用脚本时，从 Release 页面下载对应附件（同一 tag 下 `.deb` 与 `.rpm` 各一个）：

| 发行版 | 附件 | 安装命令 |
|--------|------|----------|
| Debian / Ubuntu | `*_amd64.deb` | `sudo apt install ./v0.7.2-linux.<短SHA>_amd64.deb` |
| Fedora / RHEL | `*.x86_64.rpm` | `sudo dnf install ./v0.7.2-linux.<短SHA>.x86_64.rpm` |

要点：

- **优先用 `apt install ./xxx.deb` / `dnf install ./xxx.rpm`（带路径），不要裸 `dpkg -i`**（`dpkg` 不会自动解决依赖，缺 WebKit 时装完也起不来）。
- 用 `rpm -Uvh` 时同理，依赖得自己先行装好。

卸载：

```bash
sudo apt remove x-hub   # 或 sudo apt purge x-hub
sudo dnf remove x-hub
```

> 卸载**不会**删数据目录（见第 5 节），需要一并清理时手动删。

---

## 5. 安装后：启动、数据与日志

### 启动

```bash
x-hub            # 二进制在 /usr/bin/x-hub
```

也可以从应用菜单搜索「x-hub」启动。窗口无边框，标题栏左上角拖动、右上角是最小化/最大化/关闭，
**关闭按钮是「隐藏到托盘」而不是退出**，真正退出走托盘菜单（或悬浮球右键菜单）里的「退出」。

### 数据目录

默认 `~/.config/x-hub/`（即 `$XDG_CONFIG_HOME/x-hub`）：

| 路径 | 内容 |
|------|------|
| `~/.config/x-hub/app.db` | SQLite 主库（速达/速记/待办/便签/提示词/倒计时/对话/剪贴板） |
| `~/.config/x-hub/app.json` | 应用配置（主题/布局/快捷键/各功能开关等） |
| `~/.config/x-hub/logs/x-hub.log` | 运行日志（排查问题第一站） |
| `~/.config/x-hub/icons/`、`wallpapers/`、`clipboard/`、`notes/images/` | 图标 / 壁纸 / 剪贴板图片 / 笔记图片 |

设置里「更改数据存储路径」可把数据根迁到别处，**迁移后重启生效**；备份/恢复在设置「数据与关于」
里打包成单个 zip。

**便携版**：可执行文件同目录放一个空文件 `portable`，数据即固定跟随 `<exe 目录>/data`。
预编译包会把 exe 装到 `/usr/bin`，所以标准安装**不用**便携模式；要随身携带时请自行解包
二进制+`portable` 标志到同一个目录再运行。

### 开机自启动

设置里开启后写 `~/.config/autostart/x-hub.desktop`（XDG Autostart 标准），启动时带
`--autostart-hidden` 静默驻留托盘。关闭开关即删除该文件。

---

## 6. ⚠️ Linux 版的差异与限制（先读这节）

### 6.1 悬浮球：可用，但**贴边自动隐藏已在 Linux 上禁用**

悬浮球本体、环形菜单、拖拽、双击显示主窗都正常，但**不会被吸附到屏幕边缘半隐**：

- 设置里**不显示**「悬浮球贴边自动隐藏」这一开关，配置里该值在 Linux 上被强制为关
  （读取/保存/启动加载三处归一），因此也别手工去改 `app.json`，重启会被改回来。
- 这是**刻意禁用**，不是装坏了。原因是实机测到该窗在 Linux 下 `outer_size` 与 `inner_size`
  不一致（约 37px），视觉球心 ≠ 窗口中心，贴边落点会偏；再叠加 Mutter/KWin 把半截出屏的窗口
  钳回工作区，会出现「隐回去又被推开」的位置抖动。宁可不吸附，也不给一个会自己乱动的球。
- 因此 Linux 上的球只能**自由摆放**在屏内（仍可拖到任意位置，位置会记忆）。

> 已知记录与排查日志口径见 [LINUX.md](LINUX.md) 的「已知问题」。

### 6.2 应用自动更新：Linux 请用安装脚本升级

更新清单里目前只有 Windows 平台的包（`platforms.windows-x86_64`），Linux 侧**没有对应的
Linux 更新包**。因此在 Linux 上：

- 「关于 → 检查更新」可能提示有新版（那是 Windows 包），但**无法应用**——重启时就位阶段
  找不到 `.exe` 会直接跳过本次更新，不会损坏现有安装（更新包会滞留在 `<数据根>/updates/`，
  必要时手动删）。
- **升级方式 = 重新执行 `scripts/install-linux.sh`**（或手动装新包）。想避免被无意义的提示打扰，
  可在设置里关掉自动检查更新。

### 6.3 速达「扫描已安装应用 / 扫描桌面」

两个扫描功能 Linux 均可用，但数据源与 Windows 不同：

| 功能 | Windows 数据源 | Linux 数据源 |
|------|---------------|-------------|
| 扫描已安装应用 | 开始菜单 `.lnk` | `/usr/share/applications`、`~/.local/share/applications` 等目录的 `.desktop` |
| 扫描桌面（v0.7.4+） | 桌面 `.lnk` / `.url` / `.exe` | `~/桌面`（XDG 桌面目录）的 `.desktop` 与可执行文件 |

浏览器书签导入目前只识别 Windows 布局的浏览器配置路径，Linux 上会得到空结果。

### 6.4 剪贴板历史：可用，自动粘贴回填有条件

剪贴板历史浮层（默认快捷键 Ctrl + 反引号）记录文本/图片/文件正常。但「选中后自动粘贴回上一个窗口」
通过注入按键实现：

| 显示服务器 | 需要 | 缺失时的表现 |
|-----------|------|-------------|
| X11 | `xdotool` | 内容仍会写进剪贴板，只是不会自动按 Ctrl+V，需手动粘贴 |
| Wayland | `ydotool`（需相应权限） | 同上 |

```bash
sudo apt install xdotool        # X11
# Wayland 下 ydotool 需要额外配置（uinput 权限），且部分合成器仍会限制全局粘贴注入
```

另外 Wayland 下**全局快捷键**、后台剪贴板监听会受合成器限制，行为不保证与 X11 一致；
桌面环境跑 Wayland 时若快捷键失灵，先切到 X11 会话复测。

### 6.5 通知弹窗与应用内网页面板：Linux 暂不可用

- **右下角通知弹窗**：自绘通知窗依赖「创建时隐藏 → 首次 show 映射」的 X11 路径，实测部分
  环境（mutter + 虚拟显卡）下窗口永远无法映射（WM_STATE 停在 Withdrawn）。已在 Linux 上
  **禁用**该弹窗（后端日志仍记录每条提醒）。待办/倒计时提醒本身照常触发：主窗可见时以
  应用内 toast 呈现，主窗不可见时暂只有日志。待 wry/tauri 修复后恢复（参见
  [tauri#15656](https://github.com/tauri-apps/tauri/issues/15656) 一族子 webview 问题）。
- **速达「应用内打开网页」面板**：面板是主窗内的隐藏子 webview，Linux 的 GTK 布局会给它
  预留一半窗口高度（即使隐藏），已通过创建后将其移出布局的方式保证主界面满高，代价是
  Linux 上该面板暂不显示——打开网页请暂改用系统浏览器。

### 6.6 其它小差异

| 功能 | Linux 行为 |
|------|-----------|
| 「用指定浏览器打开」 | 按 `PATH` 扫描常见浏览器（Chrome / Chromium / Firefox / Edge / Brave / Vivaldi / Opera）；一个都没装时列表为空 |
| 「以管理员身份运行」 | 走 `pkexec`，需要 polkit（桌面版一般已具备） |
| 托盘图标 | 依赖 `libayatana-appindicator3`；GNOME 需启用 AppIndicator 扩展才显示（这是 GNOME 生态的既有行为） |
| 拖入导入 | 支持 `.desktop` 与可执行文件（不是 Windows 的 exe/lnk 那套） |

---

## 7. 排障

**先看日志**：`~/.config/x-hub/logs/x-hub.log`。

| 症状 | 处理 |
|------|------|
| `error while loading shared libraries: libwebkit2gtk-4.1.so.0` | 缺运行时库 → 按第 2 节装；确认装的是 `4.1` |
| `version 'GLIBC_2.3x' not found` | 发行版比构建基座老 → 换新系统，或在 22.04 基座上自行构建（第 8 节） |
| 装完提示找不到 `x-hub` | 重开终端；确认 `/usr/bin/x-hub` 存在；从应用菜单启动亦可 |
| 关掉窗口后「应用不见了」 | 关闭 = 隐藏到托盘，请用托盘图标菜单（或悬浮球右键）退出/唤回 |
| 托盘没有图标 | 装 `libayatana-appindicator3`；GNOME 需 AppIndicator 扩展 |
| 悬浮球贴不住屏幕边缘 | **预期行为**，见 6.1 |
| 提示有更新但点了没用 | **预期行为**，见 6.2 |
| 全局快捷键无反应 | 与已有快捷键冲突，或处于 Wayland 会话；改绑快捷键 / 切 X11 复测 |
| 界面起来但内容空白 | 看日志尾部；先确认依赖是否齐、是否有 WebKitGTK 版本冲突 |

### 数据备份 / 迁移

- 应用内：设置 →「数据与关于」→ 备份/恢复（打包为单个 zip）。
- 粗暴做法：退出应用后直接拷整个 `~/.config/x-hub/` 到目标机同路径（不要只拷 `app.db`，
  图片、图标、配置都在旁边目录里）。

---

## 8. 从源码构建（老系统 / 非 x86_64 / 自测发行版）

需要 Node.js 20 + pnpm 9 + Rust stable，以及各发行版的 **开发包**（Debian/Ubuntu：
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`，Fedora 对应
`webkit2gtk4.1-devel` 等，完整清单以 [Tauri 2 前置条件](https://v2.tauri.app/start/prerequisites/) 为准）。

Ubuntu 24.04 测试机上一把梭（拉代码 → 装依赖 → 打 deb → 安装 → 启动）：

```bash
chmod +x scripts/ubuntu-build.sh
scripts/ubuntu-build.sh --install --run
```

只打包：

```bash
pnpm install --frozen-lockfile
pnpm run tauri:build:deb      # 或 tauri:build:rpm / tauri:build:linux（deb+rpm）
# 产物：src-tauri/target/release/bundle/{deb,rpm}/
```

> **要给老系统用的包，必须在最老的受支持发行版上编**（当前基线 Ubuntu 22.04 / glibc 2.35）。
> 宿主是 24.04 时用 22.04 容器编，步骤与两个坑（容器内 apt 源要用 http、Node 的 PATH）
> 见 [LINUX.md](LINUX.md)。

---

## 9. 小结

```bash
# 目标机（Ubuntu 24.04 / Fedora 44 等）上，三条命令即可
chmod +x install-linux.sh
./install-linux.sh --tag v0.7.2-linux.<短SHA>
x-hub
```

装完先确认三件事：托盘图标是否出现（能唤回窗口）、设置里悬浮球开关是否如预期
（**Linux 上不会看到「贴边自动隐藏」那一项**）、日志 `~/.config/x-hub/logs/x-hub.log` 尾部无报错。

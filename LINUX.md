# Linux 安装系统 WebView（WebKitGTK 4.1）

## Ubuntu 测试机一键脚本 / CI 自动包

在 **Ubuntu 24.04** 测试机上可执行仓库内脚本拉代码、装依赖、打 deb、安装并启动：

```bash
chmod +x scripts/ubuntu-build.sh
scripts/ubuntu-build.sh --install --run
```

默认从 `https://github.com/inkchills/x-hub.git` 的 `feat/linux-support` 克隆到 `~/x-hub-build`；更多选项见 `scripts/ubuntu-build.sh --help`。

推送到 **`feat/linux-support`** 会触发 GitHub Actions 工作流 [`.github/workflows/linux-build.yml`](.github/workflows/linux-build.yml)：按 `v{package.json 版本}-linux.{短 SHA}` 打 annotated tag（若不存在），构建 `.deb` 并上传为 **draft + prerelease** 的 GitHub Release（不依赖 `release.yml` 的 tag 推送）。

---

x-hub 在 Linux 上用的是发行版自带的 **WebKitGTK 4.1**（`libwebkit2gtk-4.1.so.0`），不是 Windows 的 WebView2，也没有独立安装包可以点一下就下完。

动态链接器在进程进入 `main` 之前就要加载这个 `.so`。缺库时二进制直接起不来，应用内「自动下载 WebView」走不到任何代码，所以不会做进程内自下。用发行版的包管理器装一次即可。

`.deb` 已经声明了运行时依赖。用 `apt` / `aptitude` 装官方包时会自动带上 WebKit，一般不用再手装。

## 先确认缺没缺

```bash
# 有输出 = 运行时库已经在
ldconfig -p | grep libwebkit2gtk-4.1.so.0
```

常见失败形态（装完再开一次）：

```
error while loading shared libraries: libwebkit2gtk-4.1.so.0: cannot open shared object file
```

开发态编译失败则是缺 **开发头文件**（`-dev` / `-devel`），和运行时缺库不是同一件事。

## 运行 x-hub（只要运行时库）

| 发行版 | 命令 |
|--------|------|
| Debian / Ubuntu | `sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2` |
| Fedora | `sudo dnf install webkit2gtk4.1 libappindicator-gtk3 librsvg2` |
| Arch / Manjaro | `sudo pacman -S webkit2gtk-4.1 libappindicator-gtk3 librsvg` |
| openSUSE | `sudo zypper install libwebkit2gtk-4_1-0 libayatana-appindicator3-1 librsvg-2-2` |

装的是 **4.1**。仓库里的 `webkit2gtk-4.0` 对不上，装了也不认。

Debian / Ubuntu 更省事的路径是直接装我们打好的包（依赖会一起拉下来）：

```bash
sudo apt install ./src-tauri/target/release/bundle/deb/x-hub_*.deb
```

直接跑 `target/release/x-hub` 或拷贝裸二进制时，不会走 deb 依赖，需要自己执行上表对应发行版那一行。

## 从源码编译（还要开发包）

完整清单以 [Tauri 2 前置条件](https://v2.tauri.app/start/prerequisites/) 为准。Debian / Ubuntu 最少要有：

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

Fedora 对应 `webkit2gtk4.1-devel`，Arch 开发头和运行时打在同一个 `webkit2gtk-4.1` 里。

## glibc 基座：打包要在老系统上编

Linux 二进制是**前向**兼容的（在新系统上跑老系统编的包可以，反过来不行）。在 Ubuntu 24.04
（glibc 2.39）上编出来的包，装到 Ubuntu 22.04（glibc 2.35）会直接起不来：

```
/usr/bin/x-hub: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found
```

所以**发布用包必须在最老的受支持发行版上编**（当前基线 = Ubuntu 22.04 / glibc 2.35）。
自查办法：

```bash
dpkg-deb -x x-hub_*.deb /tmp/chk
strings /tmp/chk/usr/bin/x-hub | grep -oE 'GLIBC_2\.[0-9]+' | sort -uV | tail -3
# 末行不应高于 2.35（在 22.04 容器里编出来是 2.34）
```

宿主是 24.04 时，用容器编（`docker` 必需）：

```bash
docker run --rm -u "$(id -u):$(id -g)" \
  -e HOME=/tmp -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/target \
  -e PATH=/opt/cargo/bin:/opt/node/bin:/usr/bin:/bin -e CI=true \
  -v "$PWD":/src -v "$PWD/.cargo-vol":/cargo -v "$PWD/.target-vol":/target \
  -w /src x-hub-builder:22.04 \
  bash -lc 'pnpm install --frozen-lockfile && pnpm run tauri:build:deb'
```

`x-hub-builder:22.04` 是本机自建镜像（`ubuntu:22.04` + WebKitGTK 4.1 开发包 + Rust + Node 20 + pnpm）。
两个坑：容器内 apt 源要用 **http**（base 镜像不带 ca-certificates，https 会报证书不可信）；
装完 Node 要**同一步内** `export PATH` 或全程用绝对路径，否则 `npm` 找不到（exit 127）。

## 已知问题

- **悬浮球贴边（半隐 / 悬停滑出）在 Linux 上未解决，暂时搁置。** Mutter/KWin 不允许窗口
  半截出屏，会把坐标钳回工作区；更棘手的是这颗窗在 Linux 上的 `outer_size` 与 `inner_size`
  不一致（实测 `outer` 高 174 或 137、`inner` 恒为 100，差值与位置偏差都是同一个 **37px**），
  说明视觉球心并不等于 `outer_size` 的中心——现有几何模型（以窗口中心为球心）在这条路上
  不成立。已落地的防护：目标先钳进工作区、同一目标只搬一次、超时（1.2s）仍未到位就
  **采纳窗口现位并写回记忆**，因此不会再出现「每 100ms 反复搬窗」的抖动循环，但贴边后
  球的落点会与预期有偏移。**不改贴边设置即可正常使用**（托盘菜单「悬浮球」里关掉
  「贴边自动隐藏」，球就停在自由位置不动）。相关诊断日志已降到 `debug!` 级（默认 Info
  级别不输出）；需要排查时把日志级别调到 Debug 再看 `[悬浮球]` 前缀的行。

## 太老的发行版

WebKitGTK **4.1** 大约从 Debian 12 / Ubuntu 22.04 / Fedora 37 / 同年份的滚动发行版才有。更老的系统（CentOS 7、Ubuntu 20.04 等）仓库里没有这个 so，也没有可替换的官方 WebView2 安装包。请换受支持的发行版，或在新系统上打 `.deb` 再装过去。

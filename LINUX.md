# Linux 安装系统 WebView（WebKitGTK 4.1）

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

## 太老的发行版

WebKitGTK **4.1** 大约从 Debian 12 / Ubuntu 22.04 / Fedora 37 / 同年份的滚动发行版才有。更老的系统（CentOS 7、Ubuntu 20.04 等）仓库里没有这个 so，也没有可替换的官方 WebView2 安装包。请换受支持的发行版，或在新系统上打 `.deb` 再装过去。

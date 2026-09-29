# service 后端写法（仅 runtime: "service"）

> **何时读我**：扩展需要后端 / 调 AI / 调平台 API / 用原生能力时。

## 最小后端

零依赖 Node，监听宿主注入的 `PORT`：

```js
// service/index.js
const http = require('http')
const port = parseInt(process.env.PORT || '0', 10)
const server = http.createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json; charset=utf-8')
  if (req.url === '/healthz') { res.end(JSON.stringify({ ok: true })); return }
  res.end(JSON.stringify({ message: 'hi', extId: process.env.XHUB_EXT_ID }))
})
server.listen(port, '127.0.0.1', () => console.log('listening', server.address().port))
```

## 铁律

1. **只监听 `127.0.0.1`**，端口读 `process.env.PORT`（宿主动态分配，`manifest.backend.port` 写 `0`）。
   - `manifest.backend.host` 缺省就是回环，**保持缺省**。
   - 真要对外开服务（`0.0.0.0` / 局域网地址）**必须**声明 `network` 权限，否则宿主直接拒绝启动后端；这类扩展用户会被要求授权，非必要别做。宿主还会把实际监听主机经 `XHUB_LISTEN_HOST` 环境变量传给后端。
2. **前端调后端走 `window.xhub.service.request('/api/x')`**，**不要**直接 fetch 端口——宿主持 `/svc/<extId>/*` 反向代理统一解决 CORS。
3. **后端只带代码、不依赖本地 `node_modules`**（运行时由宿主管理）。确需自带完整运行时的重型应用是例外，需额外说明。
4. 提供 `health` 路径（如 `/healthz`）便于宿主探活。

> 后端自己发外部 HTTP 请求（抓取、调第三方 API）**不需要任何权限**——那是 Node 进程自己的事，权限系统只管扩展 ↔ 宿主的桥调用与对外监听。

## 排错：后端没起来先看哪里

宿主把后端的 stdout / stderr **落盘**到 `<数据根>\logs\service\<扩展 id>.log`（单份上限 1MB，每次启动写一条 `----- service 启动 <时间> -----` 分隔标记，便于分清哪几行属于哪一次启动）。前端 `runtime.info().serviceReady === false` 只说明「端口没探通」——可能没起来、起来就崩、也可能端口被占，**先打开这个文件**；宿主日志 `logs\x-hub.log` 里同时有 `service 后端未就绪: … exit=<退出码>` 加日志尾部 20 行。不要对用户写「正在下载 Node」这类猜测文案。

## 数据与日志写在哪

**不要用 `__dirname` 反推宿主数据根**——开发目录直挂时扩展的上一级是版本目录 / 任意源码目录，不是 `extensions`，反推必然落空。落盘位置只有两个选择：

| 写什么 | 写哪 |
|---|---|
| 后端自己的数据 / 缓存 / 日志 | 扩展目录内的 **`.data/`**（点开头：宿主的打包与热重载都会跳过） |
| 需要跨形态共享的键值 | 桥 API `config.*` / `storage.*`（宿主负责落盘，别自己写文件） |

⚠️ 直接写扩展目录（不套 `.data/`）在直挂模式下等于**写进你的源码目录**；若该目录就是发布源码目录，用户数据会被一起打包上传（实机事故：共享待办数据进了待审包）。旧数据迁移留下的备份文件同样要放进 `.data/`。

⚠️ 把路径交给外部程序（`spawn`、命令行参数）前先剥 Windows 的 `\\?\` 前缀——`__dirname` 在直挂目录下可能是 verbatim 形式（`\\?\C:\…`），对方读不了：`\\?\C:\x` → `C:\x`，`\\?\UNC\srv\share` → `\\srv\share`。

## 前端调用

```js
const res = await window.xhub.service.request('/api/x', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ q: 'hello' }),
})
const data = await res.json()   // 也可 res.status / res.headers / res.text()
```

`runtime.info()` 返回的 `serviceReady` / `proxyPrefix` 可用于探测后端就绪状态；后端未起来时 `service.request` 会失败，界面要给得出人话的提示而不是白屏。

## 授权（service:execute）

service 后端要用户**信任当前版本**（权限「运行本地后端」开关）才会跑，分两个时机：

- **打开扩展时未授权**：宿主直接拦截入口加载——扩展页面**根本不会运行**，宿主自己的错误态带「去授权」按钮。你不需要（也无法）在页面里处理这种情况。
- **运行中被撤销授权**：页面还活着，但 `service.request` 会 reject `PERMISSION_DENIED: 本地后端未获信任或网络权限已关闭…`。此时界面应展示「本地后端未授权」提示 + **「去授权」按钮**，点击调 `xhub.openPermissions()`（无需权限）让宿主直接打开本扩展的设置弹窗：

```js
try {
  await window.xhub.service.request('/api/summary')
} catch (e) {
  if (/PERMISSION_DENIED|未获信任/.test(e.message) && typeof window.xhub.openPermissions === 'function') {
    showAuthHint() // 渲染「去授权」按钮，点击 → window.xhub.openPermissions()
  }
}
```

`typeof … === 'function'` 守卫是给旧宿主的兼容：没有这个桥 API 时按钮不渲染，只留错误文案。

## 部署注意

部署 service 扩展前，若 x-hub 正在运行并锁定了该扩展的后端文件，`deploy` 会报 **EPERM**——先退出 x-hub 再部署。

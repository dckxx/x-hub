# 校验、真机调试与上架

> **何时读我**：扩展代码写完，要跑起来看效果 / 报交付清单 / 用户提到发布上架时。

## 先明确：你没有脚手架，也能走完全程

脚手架仓库（`npm run new/validate/preview/deploy/pack`）是**内部开发环境才有**的东西，外部开发者手里没有。本 skill 自带 `templates/`，加上宿主的本机源码目录直挂，全流程都能走通：

| 环节 | 没有脚手架（默认情况） | 有脚手架（内部开发环境） |
|---|---|---|
| 生成骨架 | 复制本 skill 的 `templates/` 改字段 | `npm run new` |
| 校验 | 下面的自检清单 + 平台关卡清单 + **真机跑一遍**（宿主扫描器的报错就是权威反馈） | `npm run validate` |
| 本地预览 | 浏览器直接打开入口 HTML（无桥，走 fallback 分支；主题也走兜底色） | `npm run preview`（mock 桥 + 主题注入） |
| 装进宿主 | **「我的扩展」直挂**（更优：改代码 1.5 秒自动重载） | `npm run deploy` |
| 打包分发 | 扩展中心「发布」（自动打包）或「导入扩展包」 | `npm run pack` |

**想让浏览器直开的预览跟宿主里一致**，就得按 `theming.md` 的「双声明 fallback」写颜色，并按 `convert-html.md` 的 boot 模式写存储读取（桥优先、`typeof window.xhub === 'undefined'` 时回退 localStorage）——这两条本来就是为了「无宿主环境也能看」而定的。

## 首选：宿主「我的扩展」直挂（不需要任何工具，改完就能看）

1. 扩展中心 →「**我的扩展**」标签页 → 添加本机扩展源码目录（须含 `manifest.json`）；
2. **添加即加载，不需要任何开关**：打开扩展即可跑真机——目录即真源（**不复制进已装列表**），**改代码约 1.5 秒自动重载**，可用真实数据与 service 后端；移除目录即撤销，右侧「发布」用于上架（需开发者认证）。

日常调试**一律走这条**，比反复 deploy 快得多。扩展跑起来后，`await window.xhub.runtime.info()` 返回的能力表就是判断「这个 API 到底能不能用」的最终权威。

> **service 扩展的后端没起来时**：先看 `<数据根>\logs\service\<扩展 id>.log`（宿主落盘的后端 stdout/stderr，含退出码与报错原文），再看 `logs\x-hub.log` 里的 `service 后端未就绪: … exit=…`。`serviceReady=false` 不等于「在下载 Node」，别猜——详见 `service.md` 的「排错」与「数据与日志写在哪」两节。

**改 service 后端代码后必须重启后端进程**：约 1.5 秒的自动重载只覆盖页面（iframe 重新导航），Node 后端进程不会跟着换（宿主按扩展 id 复用在跑的进程），光刷新页面永远拿不到新逻辑。入口：常驻运行的实例上**右键侧栏固定区的扩展图标 →「重启后端」**（停掉 Node 进程再起，端口会重新分配，扩展页面不必刷新）；没开常驻时重启一次 x-hub，全部后端随之重拉。前端 HTML/JS 改动照旧走自动重载，不用管。

## 提交前自检清单

- [ ] `manifest.json` 是合法 JSON、必填字段齐全、`entry` 指向的 HTML 文件都存在；
- [ ] 入口 HTML 引用的本地 JS / CSS / 图片路径都存在（**没有 CDN 外链**）；
- [ ] 所有颜色走 `var(--xhub-*, fallback)`，深浅色都看一遍；
- [ ] 桥调用有 `try/catch`，被拒（`PERMISSION_DENIED`）时界面要说人话而不是白屏；
- [ ] **页面底用 `var(--xhub-page-bg, transparent)`**（无壁纸=宿主页面背景，有壁纸=transparent），内容表面用 `var(--xhub-surface)`；`module` 卡片也透明；
- [ ] 声明了 `module` 的话：入口在**最小格子**下也能排版——卡片 iframe 视口 = 内容区（开了宿主表头再减约 30px），最矮只有 ~86px（见 `pitfalls.md` 第 22 条）；
- [ ] 声明了 `module` 的话：**没和宿主表头重复画标题**——扩展卡默认无表头、标题自己画；只有写了 `moduleOptions.defaultHideTitle: false` 才有宿主表头（此时卡名 = `manifest.name`，别再自己画，见第 23 条）；
- [ ] 壁纸透底态（`data-xhub-wallpaper-clear="1"`）去掉白描边、文字加黑柔光晕。

## 平台关卡会查什么（发布前逐条自查）

客户端「发布」时会先跑一遍**本地预检**，它与服务端关卡同口径。**error 阻止发布，warn 只提示**：

| 检查项 | 级别 | 说明 |
|---|---|---|
| `manifest.json` 可解析 | error | JSON 语法 / 字段类型错 |
| `id` 合法 | error | 见 `manifest.md` 的格式硬规则（小写反向域名） |
| `id` 用了 `com.x-hub.*` | warn | 平台保留命名空间；非官方账号提交会被**服务端**拒绝 |
| `version` 是 `x.y.z` | error | 三段纯数字，`1.0.0-beta` 之类不合法 |
| 入口文件齐全 | error | `entry` 里每个路径都要有真实文件（报错会列出 `形态 → 路径`） |
| 没声明任何入口 | warn | 至少要有一个形态的入口 HTML |
| service 后端入口存在 | error | `backend.entry` 指向的文件要真实存在 |
| 对外监听却没申请 `network` | error | `backend.host` 非回环时 |
| 用到了宿主没实现的桥 API | warn | 按 `runtime.info().capabilities` 口径 |
| **代码用到、但 manifest 没声明的权限** | **error** | 见下 |

**最后一条最容易中招**：预检会**静态扫描**扩展目录里的 `.html/.js/.mjs/.cjs`（跳过 `.` 开头的文件和 `node_modules`），把出现的每一处 `xhub.…` 调用链拿出来，反推它需要什么权限，再和 `manifest.permissions` 对账。规则是：

- `data.*` 中方法名以 `create/update/delete/set/toggle/reorder/import/schedule` 开头 → `data:write`，其余 → `data:read`
- `ui.*` → `notify`｜`net.*` → `network`｜`system.*` → `system`｜`clipboard.*` → `clipboard`｜`fs.*` → `fs`｜`sharedStorage.*` → `shared-storage`｜`titlebar.*` → `titlebar`｜`events.emit` → `events`
- **`xhub.openExternal(...)`（顶层方法）→ `open-url`**；其余 `runtime.*` 无需权限
- `runtime.*`（`openExternal` 除外）/ `storage.*` / `config.*` / `theme.*` / `service.*` / `expose` → 无需权限

⚠️ 扫描是**纯文本匹配**，所以**注释、字符串、示例代码里写的 `xhub.data.notes.create(...)` 也会被算作「用到了」**——要么补声明，要么别在注释里写这种调用示例。

反过来，**声明了却在代码里找不到调用**的权限会 warn（`network` 除外，它无法静态检测）：用不到就删掉，审核时更好过。

## 部署与打包

- 部署后重启 x-hub（或刷新扩展中心）即可见。
- `module` 形态要到设置「工作台 → 自定义布局」把模块拖入网格。
- 打包产物是 `.xhpack`（本质 zip，`--zip` 可输出 `.zip`；`manifest.json` 必须在包根或一层子目录），也可在扩展中心「导入扩展包」本地安装。

## 上架市场

**不用手改清单**：客户端「扩展中心」里对「开发中」的扩展点「**发布**」→ 客户端本地预检（上面那张表）→ 打包上传 → 平台**机器关卡**（包结构 / manifest / 入口 / 权限申报 / 静态扫描 / 版本递增）→ **人工审核** → 签名上架，已装用户自动收到更新；被拒时客户端「我的提交」里能看到逐项理由。

- **版本号不用手改 manifest**：发布弹窗的「发布版本」默认预填「当前补丁号 +1」，点发布时客户端会先把该版本写回扩展的 `manifest.json` 再打包（必须大于当前版本；留空则按 manifest 当前版本发布，适合关卡挂了重提同一版）。改完代码 → 点发布 → 直接发布，全程不必去扩展目录手改文件。
- 发布时可**上传截图**（最多 5 张、单张 ≤2MB，PNG/JPG/WebP），会展示在扩展详情页——建议传 1~3 张（主界面 + 典型用法）。
- 发布需要**发布者身份**：设置 →「账号」登录 → 兑换邀请码 → 申请成为开发者 → 审核通过后，「开发中」的扩展才会出现「发布」按钮。
- **用户反馈走宿主反馈板**：上架后，用户可在市场详情 / 扩展设置弹窗 / 扩展出错态提交反馈（bug / idea / question，可附联系方式与诊断信息）；开发者在扩展中心进「**反馈收件箱**」按扩展查看未读、行内回复、标记已解决。发布不是终点，记得回头看，别让用户反馈石沉大海。

## 可选：脚手架仓库（仅内部开发环境有）

外部开发者可跳过本节。下面是它提供的便利，命令名供参考：

生成骨架（本 skill 已带 `templates/`，非必需）：

```bash
npm run new                        # 交互式（会问 id）
npm run new -- --name 天气卡片 --surfaces module,view --id com.yourname.weather \
  --desc "在工作台显示实时天气" --runtime web --dir ./weather
# ⚠️ --id 必填、且必须是你自己的反向域名：com.x-hub.* 是平台保留命名空间（只给官方自营），
#    脚手架会直接拒绝（平台方自用需显式加 --official）
# module 多形态：--variants compact:紧凑:2x2:2x2,month:整月:4x4:5x5
#   格式 id:名称[:minWxminH:idealWxidealH]，尺寸缺省 2×2 / 4×3，会在 manifest 里生成 moduleVariants
```

校验、预览、部署、打包：

```bash
npm run preview -- <扩展目录>     # 本地预览：mock 桥 + 主题注入，无需宿主
npm run validate -- <扩展目录>    # 校验 manifest、entry 与本地资源引用
npm run deploy -- <扩展目录>      # 安装到 %APPDATA%\x-hub\extensions\<id>\
npm run pack -- <扩展目录>        # 打包 dist/<id>-<version>.xhpack
```

> ⚠️ npm script 名是 `deploy` 而**不是** `install`——`install` 是 npm 内置生命周期钩子，会被 `npm install` 误触发。
>
> ⚠️ 部署 service 扩展前，若 x-hub 正在运行并锁定了该扩展的后端文件，`deploy` 会报 **EPERM**——先退出 x-hub 再部署。

**预览说明**：会列出全部 entry URL；注入 mock 桥（`storage` / `config` / `theme` / `events` / `runtime.info`）与整套 `--xhub-*` 变量；`?xhub-theme=light|dark` 切主题、`?xhub-variant=<id>` 切 module 形态；mock 数据存浏览器 localStorage（与宿主不互通），`data.*` 返回空数组、`service.request` 会 reject——**这些最终仍需宿主内验证**。

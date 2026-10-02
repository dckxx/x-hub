# 把已有 HTML 页面改造成扩展

> **何时读我**：用户给了一个现成的网页 / 单文件工具站 / `.html`，要「改造成 x-hub 扩展」。

这是最常见的改造路径，按序做四件事。

## 1. 去 CDN（必做）

宿主 webview 可能受 CSP / 离线限制，入口**不依赖任何外链资源**：

- **Tailwind CDN** → 统计页面实际用到的工具类，写一份本地 CSS 替代（通常 30 行以内，含 `sm:` / `md:` / `lg:` 媒体查询）。
  别忘补 Tailwind preflight 的复位：`body{margin:0}`、`h1..p{margin:0}`、`button` 复位、`*{box-sizing:border-box}`、`input,textarea{font-family:inherit}`。
- **Font Awesome 等图标库** → 内联 SVG（`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor">`，CSS 里设 `svg{width:1em;height:1em}` 随字号缩放）。
  模板里图标多时，可保留 `fa-*` 类名，用 MutationObserver 把 `<i class="fas fa-xxx">` 自动替换为同尺寸 SVG（`el.innerHTML = svg`，加 `data-*` 标记防重入），避免逐处改模板。
- **Google Fonts** → 删除，字体栈落到系统字体（写好 `"Songti SC", SimSun, serif` 这类兜底链即可）。

## 2. 主题映射

把页面自有 design token 按 `theming.md` 的「双声明 fallback」映射到 `--xhub-*`。

**删除页面自带的深浅色切换按钮与对应 JS**——主题归宿主管。

## 3. 存储迁移

`localStorage` 同步读 → `xhub.storage` 异步读。用 **boot 模式**：

```js
// 先 await 读回进度/数据，再 buildNav / route 首绘——不要在顶层同步读
const saved = await window.xhub.storage.get('progress')
render(saved)
```

保存时**桥优先**，`typeof window.xhub === 'undefined'`（无桥独立预览）时回退 localStorage：

```js
async function save(v) {
  if (window.xhub) await window.xhub.storage.set('progress', v)
  else localStorage.setItem('progress', JSON.stringify(v))
}
```

## 4. 多形态拆分（可选）

module 摘要卡与 view 主页若共用数据，抽成 `assets/data.js`（`window.XXX = {...}` 挂全局，纯计算也放这里），两个入口 `<script src="../assets/data.js"></script>` 共享，避免复制两份数据。

## 5. 外链（多数情况不用改）

页面里的 `<a href="https://…">` 链接**保持原样即可**：宿主桥会拦截外链点击、按用户的「链接打开方式」设置打开（默认应用内置浏览器窗口）。两件事不要做：别改成 `target="_blank"` / `window.open`（宿主里静默失效）；别在外链 `<a>` 上绑自己的 click 逻辑（会被宿主接管吞掉），需要自定义点击行为就用按钮 + `xhub.openExternal(url)`（manifest 记得声明 `open-url` 权限）。

## 改完先验

起本地预览（见 `debug-deploy.md`），浏览器过一遍勾选 / 保存 / 搜索等交互，再进宿主真机跑。

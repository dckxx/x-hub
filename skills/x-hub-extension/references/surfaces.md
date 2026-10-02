# 形态（surface）与工作台模块多形态

> **何时读我**：决定扩展以什么形态呈现、或写 `module` 入口时。

## 四种形态怎么选

| 形态 | 场景 | 容器 |
|---|---|---|
| `module` | 工作台常驻摘要卡 | 宿主工作台网格的一个格子，**宿主已给它套玻璃卡**，扩展页面保持透明 |
| `view` | 完整工具页 | 主工作区（侧栏导航进入） |
| `window` | 并排参考 / 需要独立空间 | 独立浮窗 |
| `drawer` | 轻量速查 / 边看边用 | 右滑面板；宿主抽屉容器在壁纸态已是真实取景模糊 + 玻璃底 |

- **`window` / `drawer` 与 `view` 共用入口**：`entry.window` 直接指向 `./view/index.html` 即可，不必复制一份页面。
- `module` 卡片格不大，按紧凑卡设计（大数字 + 一两行说明 + 可选一个按钮）。
- 一个扩展可以声明多个形态，共用 `manifest.id` 与同一份 `storage`。

## 工作台模块多形态（variant）

声明了 `manifest.moduleVariants` 的扩展，**module 入口在同一份 HTML 里按「当前形态」渲染不同内容**。取形态有三个通道：

### 1. URL query（首帧）

```js
const variant = new URLSearchParams(location.search).get('xhub-variant')
```

入口加载时即可用（宿主动态拼接在 iframe src 上）。

### 2. CSS / DOM 属性（推荐，纯 CSS 分支）

宿主把当前形态写到 `<html>` 的 `data-xhub-variant="<id>"` 属性和 `--xhub-variant` CSS 变量，切形态实时更新：

```css
:root[data-xhub-variant="compact"] .month-grid { display: none; }
:root[data-xhub-variant="month"] .today { display: block; }
```

⚠️ 分支只能靠**属性选择器**。CSS 里没有 `var(--xhub-variant) === 'compact' ? … : …` 这种写法——那是在 JS 里才成立的条件表达式。

### 3. 事件（运行时切换，不重载）

用户在工作台编辑器点 ⇄ 切形态时，宿主广播 `xhub:variant-changed`，扩展订阅后自行重绘：

```js
window.xhub.events.on('xhub:variant-changed', (variant) => render(variant))
```

### 入口写法约定

module 入口默认按**多形态自适应**写：尺寸跟随当前格子（cq 单位或 `container-type`），形态决定内容密度/结构。

⚠️ **视口 = 卡片内容区（减去宿主表头，如果有的话），而且可能非常矮**：宿主保证 iframe 高度严格等于内容区高度（0.6.3 上曾有宿主 bug 让它恒为浏览器默认的 150px、矮卡底部被裁，已修）。**扩展卡默认没有宿主表头**；只有作者写了 `moduleOptions.defaultHideTitle: false` 时，卡顶才会多一行约 30px 的表头（用户在编辑器里也能按卡片开关）。工作台卡片高度由网格决定（行高下限 36px），4×2 的小格子在小窗口下内容区可能只有 ~86px，**有表头还要再减**。入口**不要**假设「至少 150px 高」，也别写死 `min-height`；用 `height: 100%` + `box-sizing: border-box` + `cqh`/`clamp()` 排版，溢出交给 `overflow: hidden/auto` 兜底。

### module 卡片的留白（边距）

**宿主不给 iframe 加内边距**——卡片内容区就是 iframe 视口，四周留白完全由扩展自己写。约定：

- `body { padding: 12px; box-sizing: border-box }`（模板的写法）。12px 与内置模块卡一致，放一起才齐；`box-sizing: border-box` 必写，否则 `padding + height: 100%` 会撑出滚动条。
- 有表头时表头自带 12px 上留白，iframe 从表头**下方**开始——所以**不要再额外加顶部留白**去「避开表头」。
- 格子矮的时候优先用 `clamp()` / `cqh` 缩字号与间距，而不是把留白删到 0（删到 0 在正常格子里会贴边）。

### 整页形态（view / window / drawer）的留白（边距）

**宿主同样不给整页形态加任何边距**（view 容器、抽屉、独立窗口都是 0）——页面留白完全由扩展自己控制。标准：

- **默认：左上 0、右下 20**，即 `body { padding: 0 20px 20px 0; box-sizing: border-box }`（`entry.view.html` 模板的写法）。左上 0 让标题行贴着标题栏下沿、与侧栏首个图标同一高度带；右下 20 防内容顶到窗沿。
- 生成前按 SKILL.md Step 3 问过用户，选了「完全无边距」就写 `padding: 0`（内容四面顶满，适合满版布局的页面）。
- `box-sizing: border-box` 必写，否则 `padding + height: 100%` 会撑出滚动条。

预览环境用 `?xhub-variant=<id>` 调试，`__xhubPreview.setVariant(id)` 模拟切换。

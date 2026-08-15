# Reka UI 使用规范（x-hub）

> 版本：reka-ui `^2.10.3`。本文档记录本项目使用 Reka UI 的正确姿势与踩坑记录，新增/修改 Reka UI 组件前必读。速查版见 `DESIGN.md §8` 与 `AGENTS.md` 关键约定 23-25。

## 1. 背景与定位

Reka UI（原 radix-vue）是**无头（headless）组件库**：只提供交互逻辑、键盘导航、无障碍与可访问性结构，**不含任何样式**。本项目所有 UI 自绘（无 NaiveUI），样式基于 `src/style.css` 设计令牌，因此组件外观完全由项目侧控制。

当前只使用其「复杂输入」能力，因为这些组件从零自绘成本极高：

| 组件家族 | 用途 | 位置 |
|---|---|---|
| `DatePicker`（含 `DatePickerField/Content/Calendar/Grid/Cell`…） | 定时模式日期选择（日历弹层） | `CountdownCard.vue` |
| `TimeField`（`TimeFieldRoot/Input`） | 定时/每天模式的时:分 segment 输入 | `CountdownCard.vue` |
| `NumberField`（`Root/Input/Increment/Decrement`） | 时长（分钟）、间隔（每 N 分钟）步进输入 | `CountdownCard.vue` |

> 评估：DatePicker / TimeField 自绘成本极高（国际化日期、日历算法、segment 输入、键盘导航、无障碍），值得用 Reka UI；若只用 NumberField 这类简单组件则收益可忽略，可考虑原生控件。

## 2. 铁律（违反必出 bug，已实测踩坑两次）

### 2.1 Portal 渲染的弹层：容器样式必须用 `:global()`，禁止 scoped

**适用组件**：`DatePickerContent` 等经 `PopoverPortal`（内部是 Vue `Teleport`）渲染到 `<body>` 的弹层。

**现象（v0.1.13 实测）**：日历弹层出现在弹窗遮罩**后面**，点不到日期。

**根因**：
1. `DatePickerContent` 默认包在 `PopoverPortal` 里，渲染到 `<body>`（`to ?? "body"`）；
2. Reka UI 渲染链 `DatePickerContent → PopoverPortal → Teleport → PopoverContent → Presence → ContentImpl` 导致父组件的 Vue scoped `data-v-xxx` 属性**不传播到容器元素**（日历内部插槽元素有 data-v，唯独 reka-ui 渲染的容器没有）；
3. `.cc-calendar-content[data-v-xxx]` 规则全部不匹配 → `z-index: 110`、背景、边框、阴影全失效 → z-index 退化为 `auto`，被 `modal-mask`（`position: fixed; z-index: 100`）盖住。

**正确写法**：

```css
/* CountdownCard.vue —— 容器必须 :global()，否则 scoped 规则不匹配 */
:global(.cc-calendar-content) {
  background: var(--frost-surface);
  z-index: 110; /* 必须 > modal-mask 的 100 */
  /* … 边框/阴影/padding/min-width/backdrop-filter */
}
```

**排查方法**：
- DevTools inspect 弹层容器，若 `z-index: auto` / 背景透明 / 阴影 `none`，即为 scoped 规则未匹配；
- 用 `[data-v-xxxx].my-class` 选择器能否匹配元素，能匹配说明 data-v 在；匹配不到说明 data-v 丢失（本坑）。

### 2.2 segment 输入组件外层：禁止用 `<label>` 包裹，用 `<div>`

**适用组件**：`TimeField`、`DatePickerField`（segment 是可编辑 `contenteditable` div）。

**现象（v0.1.13 实测）**：点击「分」焦点自动跳到「时」；点击「日」跳到「年」，无法单独设置。

**根因**：
1. `contenteditable` div **不是 labelable element**（labelable 仅限 input/select/textarea/button 等）；
2. `TimeFieldRoot`/`DatePickerField` 内部渲染了一个隐藏 input（`tabindex="-1"`），它是该字段内**唯一**的 labelable 元素；
3. 点击 segment 时，`<label>` 的激活行为会聚焦关联的 labelable 控件 → 隐藏 input 获得焦点；
4. Reka UI 的隐藏 input `onFocus` 会强制 `segmentElements[0].focus()` → **聚焦第一个 segment（时/年）**。

**正确写法**：

```vue
<!-- 错误：label 包裹 segment 组件，点「分」会跳「时」 -->
<label class="cc-field">
  <span class="cc-field-label">时间</span>
  <TimeFieldRoot …>…</TimeFieldRoot>
</label>

<!-- 正确：外层用 div，保留 span 标签文本 -->
<div class="cc-field">
  <span class="cc-field-label">时间</span>
  <TimeFieldRoot …>…</TimeFieldRoot>
</div>
```

**例外**：`NumberField` 的原生 `<input>` 是 labelable，`<label>` 包裹正常，无需改动。

### 2.3 v-model 绑定 DateValue/TimeValue：必须用 `shallowRef`

`@internationalized/date` 的 `Time`/`DateValue` 是含 `#private` 字段的名义 class，放进 `ref` 会被 `UnwrapRef` 深度解包成结构类型，导致与 Reka UI 组件 prop 类型不匹配（TS 报错）。

```ts
import { shallowRef } from 'vue'
import { Time } from '@internationalized/date'
import type { TimeValue } from 'reka-ui'

// 正确：shallowRef 只追踪 .value 整体替换
const scheduleTime = shallowRef<TimeValue | null>(null)
const dailyTime = shallowRef<TimeValue>(new Time(15, 0))
```

## 3. 层级与外观约定

| 层 | z-index | 说明 |
|---|---|---|
| `modal-mask`（弹窗遮罩） | `100` | 全局 `style.css` |
| 日历弹层 `.cc-calendar-content` | `110` | 必须高于遮罩，否则被盖住（见铁律 2.1） |

- 日历弹层是**瞬态表面**，允许 `backdrop-filter`（与 DESIGN.md §7 性能策略一致）。
- 弹层外观沿用玻璃令牌：`--frost-surface` 背景 + `--border-soft` 边框 + `--frost-edge/--shadow-dock` 阴影 + `--radius-lg`。

## 4. 调试指南

### 4.1 segment 输入 / 焦点问题，必须用真实键盘事件验证

`browser.type`（openchamber 等自动化工具）默认是**直接设置 DOM 文本**，不触发 `keydown`，因此不会走 Reka UI 的输入处理（`handleSegmentKeydown`），表现为「segment 显示变了但 v-model 不同步」——这是**假象**，不代表真实用户输入。

正确验证方式：**Playwright MCP**，先点击 segment 再用 `browser_press_key` 逐个按键：

```js
await page.getByRole('spinbutton', { name: 'minute,' }).click()
await page.keyboard.press('4')
await page.keyboard.press('5')
```

然后检查快照：segment 是否 `[active]`（焦点位置）、隐藏 input 的 value 是否同步（如 `15:45:00`）。

### 4.2 scopeId（scoped CSS）丢失排查

- 现象：弹层样式完全没生效（z-index/背景/阴影全无），但 DOM 里 class 存在。
- 判断：DevTools 里 `document.querySelector('[data-v-xxxx].your-class')` 是否命中；或 inspect computed style。
- 结论：Reka UI 的 Portal/渲染链会丢 scoped data-v，此类元素样式一律 `:global()`（铁律 2.1）。

### 4.3 Reka UI 内部关键机制（调试时有用）

- `PopoverContent` → `Presence` → `PopoverContentModal/NonModal` → `ContentImpl`：attrs（class/data-v）沿此链透传，Portal 场景会在某处断裂。
- `useVModel(props, 'modelValue', emits, { passive: false })`：非 passive 时 `.value = x` 只 `emit('update:modelValue', x)` 不回写 props，读回仍是父组件传入值——`v-model` 同步靠父组件监听更新后重新传 props。
- 焦点跳转链路：隐藏 input 的 `onFocus` → `segmentElements[0].focus()`（这是 2.2 坑的直接源头）。

## 5. 引入新组件的评估清单

- 该组件自绘成本是否显著？（日历/segment 输入/复杂弹层 = 值得；简单 input+按钮 = 不值得）
- 是否会 **Portal 到 body**？是 → 容器样式必须 `:global()`。
- 是否有 **segment/contenteditable 输入**？是 → 外层用 `<div>` 包裹，v-model 用 `shallowRef`。
- 是否有隐藏 input（`feature="focusable"`）？有 → 注意 label 激活行为（铁律 2.2）。

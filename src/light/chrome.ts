// 速达「应用内浏览器」chrome 顶栏的轻量入口（内存优化 P1）。
// 完整 SPA 的「活堆」（Vue 应用实例 + 全组件树 + store 拷贝）是隐藏窗口 renderer
// 的内存大头（~100MB，MemoryUsageTargetLevel=Low 只能吐缓存吐不掉活堆）；
// chrome 页只需要顶栏一条，换轻量入口后隐藏态降到 ~20-30MB。
// 只挂 BrowserChrome + 主题自举；splash / 滚动条 hover 联动 / UpdateCheckDialog
// 等主窗启动环境都不需要。
//
// 主题由 BrowserChrome 内部的 useShellThemeConfig 自举（2026-10-31 起）：
// 原先这里补一次 getThemeConfig + 组件内 useTheme()，而 useTheme 会 import
// useStore —— 于是这个「轻量」入口实际 preload 了全量 store（workbench-*.js）。
import { createApp } from 'vue'
import '../style.css'
import BrowserChrome from '../components/BrowserChrome.vue'

createApp(BrowserChrome).mount('#app')

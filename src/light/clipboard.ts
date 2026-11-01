// 剪贴板浮层（clipboard）的轻量入口（内存优化 P2）。
// 完整 SPA 的活堆是隐藏窗口 renderer 的内存大头；浮层只渲染列表 + 搜索 + 预览。
// 组件内部已解耦全量 store（主题走 useShellThemeConfig，存速记/提示词直调命令），
// 因此本入口不会 preload workbench 那份 store 模块。
import { createApp } from 'vue'
import '../style.css'
import ClipboardOverlay from '../components/ClipboardOverlay.vue'

createApp(ClipboardOverlay).mount('#app')

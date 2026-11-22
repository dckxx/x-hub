// 全局搜索浮窗（search）的轻量入口。
// 搜索要「不弹主窗」就必须有自己的窗口（搜索 UI 原先挂主窗遮罩层，显示它就得先亮主窗）；
// 这里只挂 SearchWindow → GlobalSearch 这一小块，不加载完整 SPA，也不 import workbench
// store——那正是轻量入口要减掉的活堆（口径同 clipboard.html / chat.html）。
import { createApp } from 'vue'
import '../style.css'
import SearchWindow from '../components/SearchWindow.vue'

createApp(SearchWindow).mount('#app')

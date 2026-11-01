// AI 对话独立窗口（chat）的轻量入口（内存优化 P2）。
// 该窗口无条件启动预创建 + 隐藏常驻（约定 41），且只在用户开启「以独立窗口打开
// AI 对话」后才可见——不开启时它是纯闲置 renderer，活堆必须尽量小。
// ChatWindow 已解耦全量 store（主题走 useShellThemeConfig；ChatPanel 只用
// api/tauri + AppSelect，自带数据拉取），因此本入口不 preload workbench store。
import { createApp } from 'vue'
import '../style.css'
import ChatWindow from '../components/ChatWindow.vue'

createApp(ChatWindow).mount('#app')

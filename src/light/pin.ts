// 贴图窗口（pin-shot-{0..2}）的轻量入口（内存优化 P2）。
// 完整 SPA 的活堆是隐藏窗口 renderer 的内存大头，贴图窗只需要一张图 + 一个关闭钮；
// 三个贴图窗启动即隐藏常驻、可能整场会话都不显示，换成轻量入口后单个 renderer
// 从 ~60-80MB 降到 ~20MB。
// PinShotWindow 自包含（零 store / 零主题依赖：只显示一张截图，无设计令牌着色）。
import { createApp } from 'vue'
import '../style.css'
import PinShotWindow from '../components/PinShotWindow.vue'

createApp(PinShotWindow).mount('#app')

// 截图/录屏两个窗口（recorder-overlay 选区层 + recorder-control 控制条）的轻量入口
// （内存优化 P2）。两窗启动即隐藏常驻、运行期只显隐，活堆越轻越好；组件自包含
// （零 store / 零主题依赖：选区层是全屏遮罩 + 标注工具条，控制条是计时 + 停止）。
//
// 为什么两个组件共用一份入口：它们由同一模块（recorder.rs）预创建、成对出现，
// 合并入口只为少一个 html 文件；实际各窗口只挂自己那一个组件。
import { createApp } from 'vue'
import '../style.css'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import RecorderOverlay from '../components/RecorderOverlay.vue'
import RecorderControl from '../components/RecorderControl.vue'
import { isTauri } from '../api/tauri'

// 按 webview label 分流（与 App.vue 同口径：多 webview 窗口下 window label 会不同，
// 这里两个窗口都是单 webview，label 恒等，但仍取 webview label 保持一致）
const label = isTauri() ? getCurrentWebview().label : ''
const Root = label === 'recorder-control' ? RecorderControl : RecorderOverlay

createApp(Root).mount('#app')

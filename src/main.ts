import { createApp } from 'vue'
import App from './App.vue'
import './style.css'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri } from './api/tauri'
import { reportClientError } from './utils/error-report'

// 主窗口：内容可绘制后再显示，避免 WebView2 冷启动期间的空白等待窗口（白/底色）。
// tauri.conf.json 里主窗口 visible:false，等 HTML 解析、欢迎页已就位后这里再 show。
// 浮窗便签（sticky-* 标签）由 Rust 自行控制显示，不受影响。
if (isTauri() && getCurrentWindow().label === 'main') {
  const win = getCurrentWindow()
  void win.show()
  void win.setFocus()
}

const app = createApp(App)

app.config.errorHandler = (err, _instance, info) => {
  void reportClientError(`前端运行错误: ${info}`, err)
}

window.addEventListener('error', (event) => {
  void reportClientError('前端窗口错误', event.error ?? event.message)
})

window.addEventListener('unhandledrejection', (event) => {
  void reportClientError('前端未处理的 Promise 拒绝', event.reason)
})

app.mount('#app')

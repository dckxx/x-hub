import { createApp } from 'vue'
import App from './App.vue'
import './style.css'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri, tauriApi } from './api/tauri'
import { reportClientError } from './utils/error-report'

// 禁用页面缩放：WebView2 里 Ctrl+滚轮 / 触控板捏合 / Ctrl+± 会放大整个页面，
// 导致工作台内容区变小、出滚动条、布局错位。工作台只靠窗口缩放适配，不靠页面缩放。
function disablePageZoom() {
  // Chromium 把捏合（pinch）也转成 ctrlKey 的 wheel 事件，一并拦截
  window.addEventListener(
    'wheel',
    (e) => {
      if (e.ctrlKey) e.preventDefault()
    },
    { passive: false },
  )
  // 浏览器缩放快捷键 Ctrl/Cmd + -/=/+ /0
  window.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && ['-', '=', '+', '0'].includes(e.key)) {
      e.preventDefault()
    }
  })
  // Safari/部分 WebView 手势捏合
  document.addEventListener('gesturestart', (e) => e.preventDefault())
  document.addEventListener('gesturechange', (e) => e.preventDefault())
}

disablePageZoom()

// 禁用网页默认右键菜单：WebView 里右键会弹浏览器上下文菜单（重新加载/检查元素等）。
// 需要自定义右键菜单的组件（速达资源、剪贴板条目）已在元素级 @contextmenu 里自行 preventDefault + 弹自定义菜单，
// 这里全局兜底拦截其余区域的默认菜单。
function disableContextMenu() {
  window.addEventListener('contextmenu', (e) => e.preventDefault())
}

disableContextMenu()

// 主窗口：内容可绘制后再显示，避免 WebView2 冷启动期间的空白等待窗口（白/底色）。
// tauri.conf.json 里主窗口 visible:false，等 HTML 解析、欢迎页已就位后这里再 show。
// 浮窗便签（sticky-* 标签）由 Rust 自行控制显示，不受影响。
// 开机自启动（--autostart-hidden）时不显示主窗口，直接驻留托盘。
if (isTauri() && getCurrentWindow().label === 'main') {
  void (async () => {
    const win = getCurrentWindow()
    const hiddenLaunched = await tauriApi.getStartupHidden().catch(() => false)
    if (hiddenLaunched) {
      void win.hide()
    } else {
      void win.show()
      void win.setFocus()
    }
  })()
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

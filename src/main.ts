import { createApp } from 'vue'
import App from './App.vue'
import './style.css'
import { reportClientError } from './utils/error-report'

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

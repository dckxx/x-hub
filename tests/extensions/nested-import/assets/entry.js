// 回归用：入口 chunk。第二级 import 由**模块自身 URL** 解析——修复前这一步会 404。
import { level1 } from './level1.js'

const out = document.getElementById('out')
out.textContent = ''

function line(cls, text) {
  const span = document.createElement('span')
  span.className = cls
  span.textContent = text + '\n'
  out.appendChild(span)
}

line('ok', 'entry.js 已加载（' + import.meta.url + '）')

try {
  const value = await level1()
  line('ok', 'level1 → level2 已加载，回传值：' + value)
  line('ok', '全部通过：entry → level1 → level2')
} catch (e) {
  line('bad', '模块链断裂：' + (e && e.message ? e.message : e))
}

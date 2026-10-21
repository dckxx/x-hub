// service/index.js —— 零依赖 Node 后端，监听宿主注入的 PORT
// 落地时把 TODO 换成真实逻辑；前端一律用 window.xhub.service.request('/api/...') 访问，
// 不要直接 fetch 端口（宿主的 /svc/<extId>/* 反向代理统一解决 CORS）。
const http = require('http')

const port = parseInt(process.env.PORT || '0', 10)
const HOST = '127.0.0.1' // 铁律：只监听本机回环

const server = http.createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json; charset=utf-8')

  // 健康检查路径需与 manifest.backend.health 一致
  if (req.url === '/healthz') {
    res.end(JSON.stringify({ ok: true }))
    return
  }

  if (req.url === '/api/hello') {
    res.end(JSON.stringify({ message: 'hi', extId: process.env.XHUB_EXT_ID }))
    return
  }

  // AI 工具端点（manifest.agent.tools 声明的工具在这里登记；2xx 响应体 = 工具结果，
  // 失败返回非 2xx 并把原因写进 error 字段；响应体上限 256KB，大清单自己分页）
  if (req.url === '/xhub/agent-tool' && req.method === 'POST') {
    let body = ''
    req.on('data', (c) => (body += c))
    req.on('end', () => {
      try {
        const { tool, args } = JSON.parse(body || '{}')
        const handlers = {
          get_info: () => ({ name: '我的扩展', version: '0.1.0' }),
          // TODO: 落地时换成真实的数据查询/操作
        }
        const handler = handlers[tool]
        if (!handler) {
          res.statusCode = 404
          res.end(JSON.stringify({ error: `未声明的工具 ${tool}` }))
          return
        }
        res.end(JSON.stringify(handler(args ?? {})))
      } catch (e) {
        res.statusCode = 400
        res.end(JSON.stringify({ error: String((e && e.message) || e) }))
      }
    })
    return
  }

  // TODO: 在这里挂真实路由（读取 body 时注意分段拼接，不要假设一次到齐）
  res.statusCode = 404
  res.end(JSON.stringify({ error: 'NOT_FOUND', path: req.url }))
})

server.listen(port, HOST, () => {
  // 端口由宿主分配（manifest.backend.port = 0），实际端口从 server.address() 取
  console.log(`[service] listening on http://${HOST}:${server.address().port}`)
})

// 宿主停止扩展时发 SIGTERM，优雅收尾
process.on('SIGTERM', () => server.close(() => process.exit(0)))

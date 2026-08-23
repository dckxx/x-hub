// 零依赖 Node 后端（内置 http 模块），监听宿主注入的 PORT 环境变量。
// 宿主启动本进程时注入 PORT（动态分配）与 XHUB_EXT_ID，并在此端口上做 TCP 探活。
const http = require('http')

const port = parseInt(process.env.PORT || '0', 10)

const server = http.createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json; charset=utf-8')
  if (req.url === '/healthz') {
    res.end(JSON.stringify({ ok: true }))
    return
  }
  if (req.url === '/api/hello') {
    res.end(
      JSON.stringify({
        message: '来自 service 后端的问候',
        port,
        extId: process.env.XHUB_EXT_ID,
      }),
    )
    return
  }
  res.statusCode = 404
  res.end(JSON.stringify({ error: 'not found', url: req.url }))
})

server.listen(port, '127.0.0.1', () => {
  // 后端日志当前被宿主丢弃（stdout: null），这里仅示意；正式日志后续接入宿主日志
  console.log('hello-service listening on', server.address().port)
})

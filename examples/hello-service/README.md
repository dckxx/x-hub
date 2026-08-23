# Hello Service（service 扩展示例）

演示 service 扩展：宿主托管一个零依赖 Node 后端，前端经 `window.xhub.service.request` 调用。

## 目录结构

```
hello-service/
├── manifest.json      # runtime: service + backend 声明
├── service/
│   └── index.js       # 后端入口（内置 http，监听宿主注入的 PORT）
└── view/
    └── index.html     # 前端入口（调 runtime.info + service.request）
```

## 手动安装与验证

1. 复制本目录到 `%APPDATA%\x-hub\extensions\com.x-hub.hello-service\`
2. 运行 `npm run tauri:dev`，侧栏「扩展中心」点开「Hello Service」。
3. 宿主会：检测系统 Node → 动态分配端口 → 启动 `service/index.js` → TCP 探活。
4. 页面展示 `runtime.info()`（`serviceReady: true`、`runtime: "service"`）与
   `service.request('/api/hello')` 的返回（含后端端口、扩展 id）。

> 需本机已装 Node（`backend.engine.minVersion: 18`）。卸载扩展时宿主会终止该 Node 进程。

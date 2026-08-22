# Hello 扩展（示例）

x-hub 扩展系统的最小示例：演示 `view` 形态 + 桥 API（`window.xhub`）。

## 目录结构

```
hello-extension/
├── manifest.json    # 扩展声明（runtime: web，entry 指向 view/index.html）
├── icon.svg         # 图标
└── view/
    └── index.html   # 前端入口（HTML，可内嵌 JS/CSS）
```

## 手动安装（开发期验证）

1. 把本目录复制到扩展根目录：
   `%APPDATA%\x-hub\extensions\com.x-hub.hello\`
   （即 `manifest.json` 位于 `%APPDATA%\x-hub\extensions\com.x-hub.hello\manifest.json`）
2. 运行 `npm run tauri:dev`，打开侧栏「扩展中心」。
3. 点击「Hello 扩展」行，主区即加载 `view/index.html`。
4. 页面会依次调用 `window.xhub.runtime.info()`、`data.notes.list()`、
   `data.todos.list()`、`data.usage.summary()`、`storage.get/set` 并展示结果。

> `data.*` 需要 manifest `permissions` 里声明 `data:read`（本示例已声明）。
> `storage.*`、`runtime.*` 无需权限。

## 扩展作者要点

- 入口（`entry.view`）统一为 **HTML 文件**，可引用同目录 JS/CSS（相对路径自动解析）。
- 宿主在加载时自动注入 `window.xhub`，扩展脚本直接调用，无需 import / 安装任何包。
- 所有 `window.xhub.*` 方法返回 Promise；失败时 reject 一个带 `code` 的错误对象。
- 详见 `docs/extension-spec.md` 与 `docs/extension-api.md`。

# Hello 扩展（示例）

x-hub 扩展系统的最小示例：演示 `view` 形态 + 桥 API（`window.xhub`）。

## 目录结构

```
hello-extension/
├── manifest.json    # 扩展声明（runtime: web，surfaces 含 module/view）
├── icon.svg         # 图标
├── module/
│   └── index.html   # module 形态入口（工作台摘要卡）
└── view/
    └── index.html   # view 形态入口（完整工具页）
```

## 手动安装（开发期验证）

1. 把本目录复制到扩展根目录：
   `%APPDATA%\x-hub\extensions\com.x-hub.hello\`
   （即 `manifest.json` 位于 `%APPDATA%\x-hub\extensions\com.x-hub.hello\manifest.json`）
2. 运行 `npm run tauri:dev`。
3. **view 形态**：侧栏「扩展中心」→ 点击「Hello 扩展」行 → 主区加载 `view/index.html`，
   依次调用 `window.xhub.runtime.info()`、`data.notes.list()`、`data.todos.list()`、
   `data.usage.summary()`、`storage.get/set` 并展示结果。
4. **module 形态**：设置「工作台」→「自定义布局」，左侧模块库会出现「Hello 扩展」，
   拖入画布后工作台网格即渲染 `module/index.html`（笔记条数 + 今日 AI 调用次数）。

> `data.*` 需要 manifest `permissions` 里声明 `data:read`（本示例已声明）。
> `storage.*`、`runtime.*` 无需权限。

## 扩展作者要点

- 入口（`entry.view`）统一为 **HTML 文件**，可引用同目录 JS/CSS（相对路径自动解析）。
- 宿主在加载时自动注入 `window.xhub`，扩展脚本直接调用，无需 import / 安装任何包。
- 所有 `window.xhub.*` 方法返回 Promise；失败时 reject 一个带 `code` 的错误对象。
- 详见 `docs/extension-spec.md` 与 `docs/extension-api.md`。

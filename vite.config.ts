import { defineConfig, type Plugin } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import AutoImport from 'unplugin-auto-import/vite'

const host = process.env.TAURI_DEV_HOST

// 剔除 Univer engine-render 内嵌的连字断词词典（pattern-loaders.gen.ts 的 77 个
// 语言动态 import，af/be/de-1901/hu/nn… 共 ~4.4MB）。这是 docs 文本排版引擎的
// 连字数据，表格笔记的单元格排版根本不做连字，运行时永远不会加载；但 Tauri 是
// 全 dist 目录嵌入 exe，一个不落。替换为 null 后 _loadPattern 走 `if (!loader)
// return` 优雅降级（消费端有判空，不 throw），且无引用者 → rolldown 不再生成
// 对应 chunk。正则按「小写语言码(-变体)*-8位hash.js」匹配；engine-render 内的
// 动态 import 已核实全部是断词词典（无其它形态），升级 Univer 后仍适用。
function stripUniverHyphenationPatterns(): Plugin {
  return {
    name: 'strip-univer-hyphenation-patterns',
    transform(code, id) {
      if (!id.includes('@univerjs/engine-render')) return null
      if (!code.includes('import("./')) return null
      const next = code.replace(
        /import\("\.\/[a-z]{2,3}(?:-[a-z0-9]+)*-[A-Za-z0-9_-]{6,12}\.js"\)/g,
        'null',
      )
      return next === code ? null : { code: next, map: null }
    },
  }
}

// KaTeX 字体三格式只留 woff2：WebView2 是 Chromium，woff2 全支持，
// ttf/woff 是纯冗余（~0.78MB）。generateBundle 阶段直接改产物：从 @font-face 的
// src 列表删掉 woff/ttf 引用 + 从 bundle 删掉这两份字体文件。
// （transform 阶段做不了：rolldown-vite 的 CSS 管线在 transform 时已把 url() 改写
// 成 asset 占位符，enforce: 'post' 也拿不到原始 fonts/*.woff 路径；产物层正则按
// format("woff"/"truetype") 匹配，特异性足够，不会误伤业务 CSS。）
function stripKaTeXLegacyFonts(): Plugin {
  return {
    name: 'strip-katex-legacy-fonts',
    apply: 'build',
    generateBundle(_options, bundle) {
      for (const file of Object.values(bundle)) {
        if (file.type !== 'asset' || !String(file.fileName).endsWith('.css')) continue
        const css = String(file.source)
        const next = css
          .replace(/,\s*url\([^)]*?\.woff\)\s*format\("woff"\)/g, '')
          .replace(/,\s*url\([^)]*?\.ttf\)\s*format\("truetype"\)/g, '')
        if (next !== css) file.source = next
      }
      for (const name of Object.keys(bundle)) {
        if (/\.(woff|ttf)$/.test(name)) delete bundle[name]
      }
    },
  }
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    vue(),
    tailwindcss(),
    AutoImport({
      imports: ['vue'],
    }),
    stripUniverHyphenationPatterns(),
    stripKaTeXLegacyFonts(),
  ],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  clearScreen: false,
  build: {
    // 分包：速记编辑器依赖（Milkdown/ProseMirror/KaTeX）单独成 chunk——仅打开速记时按需加载，
    // 且这些库版本稳定，独立 chunk 可长期命中缓存
    rolldownOptions: {
      // 多页入口（内存优化 P1/P2）：常驻壳窗口一律走轻量入口，只挂自己的组件、
      // 不加载完整 SPA 的活堆——隐藏窗口 renderer 从 ~100MB 降到 ~20-30MB。
      //
      // P1/P2（v0.7.0）：chrome.html / ball.html / notice.html
      // P2 续（2026-10-31）：pin.html / recorder.html / clipboard.html / chat.html
      //   —— 这四个原先挂 index.html，哪怕只渲染一个 147 行的贴图组件，也要解析执行
      //   整个工作台模块图（main-*.js 498KB + 166KB CSS + 全套 store/组件表）。
      //
      // index.html 现在是「主窗 + 便签/倒计时浮窗 + 扩展窗 + 提示词/待办浮窗」的入口
      // （那批是低频瞬态或按需创建，不在启动常驻之列）。
      // export.html：导出渲染窗占位页（纯静态零脚本，见 export_render.rs）
      input: {
        main: 'index.html',
        chrome: 'chrome.html',
        ball: 'ball.html',
        notice: 'notice.html',
        export: 'export.html',
        pin: 'pin.html',
        recorder: 'recorder.html',
        clipboard: 'clipboard.html',
        chat: 'chat.html',
      },
      output: {
        codeSplitting: {
          groups: [
            // Vue 运行时单独成 chunk（2026-10-31）：**必须在 editor-vendor 之前**。
            // 此前 Vue 被打进 editor-vendor 组（该组 test 只匹配 milkdown/prosemirror，
            // 但 Vue 最终被合并进同一 chunk），后果是所有入口的 `createApp` 都从
            // editor-vendor 里 import ⇒ 连悬浮球/通知窗/贴图窗这些「轻量」壳窗口
            // 也要执行整个 1.25MB 的 Milkdown/ProseMirror 模块图。切开后轻量入口
            // 只 preload 这份纯 Vue（~百 KB），编辑器依赖仍按需加载。
            {
              name: 'vue-vendor',
              test: /[\\/]node_modules[\\/](vue|@vue)[\\/]/,
            },
            {
              name: 'editor-vendor-katex',
              test: /[\\/]node_modules[\\/]katex[\\/]/,
            },
            // DOMPurify 单独成 chunk（2026-10-31）：对话面板渲染 Markdown 只要它一份，
            // 若被合并进 editor-vendor，chat.html 就得为「净化一段 Markdown」加载
            // 整个 Milkdown/ProseMirror（1.14MB）。
            {
              name: 'dompurify',
              test: /[\\/]node_modules[\\/]dompurify[\\/]/,
            },
            {
              name: 'editor-vendor',
              test: /[\\/]node_modules[\\/](@milkdown|@prosemirror|lowlight|highlight\.js)[\\/]/,
            },
          ],
        },
      },
    },
    // 编辑器 vendor chunk 体积在预期内（懒加载 + 强缓存；桌面端从本地磁盘加载），放宽默认 500kB 告警阈值
    chunkSizeWarningLimit: 1280,
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || '0.0.0.0',
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Tell Vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
    allowedHosts: ['.monkeycode-ai.online'],
  },
})

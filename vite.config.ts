import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import AutoImport from 'unplugin-auto-import/vite'

const host = process.env.TAURI_DEV_HOST

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    vue(),
    tailwindcss(),
    AutoImport({
      imports: ['vue'],
    }),
  ],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  clearScreen: false,
  build: {
    // 分包：速记编辑器依赖（Milkdown/ProseMirror/KaTeX）单独成 chunk——仅打开速记时按需加载，
    // 且这些库版本稳定，独立 chunk 可长期命中缓存
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: 'editor-vendor-katex',
              test: /[\\/]node_modules[\\/]katex[\\/]/,
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

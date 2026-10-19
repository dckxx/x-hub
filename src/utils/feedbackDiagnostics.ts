import { tauriApi } from '../api/tauri'

/**
 * 扩展反馈的「诊断信息」：应用版本 / 扩展 id@版本@运行时 / 系统与缩放 / 时间。
 *
 * 只含版本与环境，不含笔记、账号、路径等任何个人数据 —— 弹窗里明示「仅开发者可见」，
 * 公开列表接口也不返回这一列。拼装是纯函数（`buildFeedbackDiagnostics`），
 * 版本读取带进程内缓存：一次反馈弹窗里只读一次 `get_app_info`。
 */

let cachedAppVersion: string | null = null

/** 应用版本（运行时读 package_info；浏览器预览 / 旧后端取不到时为「未知」） */
export async function currentAppVersion(): Promise<string> {
  if (cachedAppVersion !== null) return cachedAppVersion
  try {
    cachedAppVersion = (await tauriApi.getAppInfo()).version || '未知'
  } catch {
    cachedAppVersion = '未知'
  }
  return cachedAppVersion
}

/** 系统标识：优先 Windows NT 版本（WebView2 的 UA 始终带 NT 10.0，Win11 也报 10.0，如实展示） */
function osLabel(): string {
  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : ''
  const nt = ua.match(/Windows NT ([\d.]+)/)
  if (nt) {
    const arch = /Win64|x64|WOW64/i.test(ua) ? 'x64' : /ARM64/i.test(ua) ? 'arm64' : ''
    return `Windows NT ${nt[1]}${arch ? ` · ${arch}` : ''}`
  }
  if (/Mac OS X/.test(ua)) return 'macOS'
  if (/Linux/.test(ua)) return 'Linux'
  return (typeof navigator !== 'undefined' && navigator.platform) || 'unknown'
}

function two(n: number): string {
  return String(n).padStart(2, '0')
}

/** 本地时间 `YYYY-MM-DD HH:mm` */
export function formatDiagTime(d = new Date()): string {
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}`
}

export interface FeedbackDiagnosticsInput {
  appVersion: string
  extId: string
  extVersion?: string | null
  runtime?: string | null
  now?: Date
}

/** 拼装诊断文本（纯函数：除时钟与 `window.devicePixelRatio` 外不读任何外部状态） */
export function buildFeedbackDiagnostics(input: FeedbackDiagnosticsInput): string {
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio : 1
  const scale = Math.round((dpr || 1) * 100)
  const ext = [input.extId, input.extVersion || '', input.runtime ? `· ${input.runtime}` : '']
    .filter(Boolean)
    .join(' ')
  return [
    `app   ${input.appVersion || '未知'}`,
    `ext   ${ext}`,
    `os    ${osLabel()} · 缩放 ${scale}%`,
    `time  ${formatDiagTime(input.now)}`,
  ].join('\n')
}

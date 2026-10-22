/**
 * 工作台「系统资源」卡片的指标目录与解析/格式化纯函数。
 * 真卡（SysMonitorCard）、布局编辑器缩印（DashModulePreview）与显示设置弹窗三方共用，
 * 指标顺序、取值与格式化口径唯一实现在这里，禁止另写一份（预览口径铁律）。
 * 被 node --test 直接加载 TS 源码，保持零运行时依赖（类型 import 会被剥离）。
 */

export type SysmonMetricKind = 'bar' | 'value' | 'pair'

export interface SysmonMetricDef {
  id: string
  label: string
  /** bar = 百分比进度条，value = 名称 + 数值行，pair = 上行/下行一对数值（无名称） */
  kind: SysmonMetricKind
}

/** 指标目录：数组顺序 = 显示设置弹窗里未勾选区的展示顺序。
 *  net_rate 在卡片上渲染为一行 ↑上行 ↓下行（label「上传/下载」只出现在显示设置弹窗） */
export const SYSMON_METRIC_DEFS: SysmonMetricDef[] = [
  { id: 'net_rate', label: '上传/下载', kind: 'pair' },
  { id: 'cpu', label: 'CPU', kind: 'bar' },
  { id: 'mem', label: '内存', kind: 'bar' },
  { id: 'net_total', label: '总网速', kind: 'value' },
  { id: 'cpu_threads', label: 'CPU线程', kind: 'value' },
  { id: 'traffic', label: '总流量', kind: 'value' },
]

/** 未自定义（配置为空）时的默认显示与顺序 */
export const DEFAULT_SYSMON_METRICS = ['net_rate', 'cpu', 'mem']

/** 旧版独立「上传」「下载」两项 → 合并后的 net_rate（老配置平滑迁移，去重后只落一项） */
const LEGACY_METRIC_IDS: Record<string, string> = {
  net_upload: 'net_rate',
  net_download: 'net_rate',
}

const DEF_BY_ID = new Map(SYSMON_METRIC_DEFS.map((d) => [d.id, d]))

/** 配置解析唯一入口：旧 id 迁移 + 过滤非法 id + 去重 + 空回退内置默认 */
export function resolveSysmonMetrics(
  configured: readonly string[] | undefined | null,
): SysmonMetricDef[] {
  const out: SysmonMetricDef[] = []
  if (configured) {
    for (const rawId of configured) {
      const def = DEF_BY_ID.get(LEGACY_METRIC_IDS[rawId] ?? rawId)
      const id = def?.id
      if (def && id && !out.some((m) => m.id === id)) out.push(def)
    }
  }
  if (out.length === 0) {
    for (const id of DEFAULT_SYSMON_METRICS) out.push(DEF_BY_ID.get(id)!)
  }
  return out
}

/** 字节数格式化：B → KB → MB → GB → TB，<1024 取整、≥100 一位小数收敛为整数 */
export function fmtBytes(bytes: number): string {
  const p = bytesParts(bytes)
  return `${p.text} ${p.unit}`
}

/** 字节数拆数值与单位（卡片排版：数字大、单位小） */
function bytesParts(bytes: number): { text: string; unit: string } {
  if (!Number.isFinite(bytes) || bytes <= 0) return { text: '0', unit: 'B' }
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let v = bytes
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  const text = i === 0 ? String(Math.round(v)) : v >= 100 ? v.toFixed(0) : v.toFixed(1)
  return { text, unit: units[i] }
}

/** 速率格式化：字节/秒自适应单位 */
export function fmtSpeed(bytesPerSec: number): string {
  return `${fmtBytes(bytesPerSec)}/s`
}

export interface SysmonMetricDisplay {
  /** 数值部分（不含单位） */
  text: string
  /** 单位（%、KB/s、线程…），缺失时为空串 */
  unit: string
  /** 仅 bar 型有值：百分比 0-100（已钳制） */
  pct?: number
}

/** 按指标 id 从采样数据取展示值（真卡与缩印共用的取值口径；
 *  net_upload/net_download 供 pair 型「上传/下载」行分别取上下行） */
export function sysmonMetricDisplay(id: string, info: SysmonSample | null): SysmonMetricDisplay {
  const missing = { text: '—', unit: '' }
  if (!info) return missing
  switch (id) {
    case 'cpu': {
      const pct = clampPct(info.cpuUsage)
      return { text: String(Math.round(pct)), unit: '%', pct }
    }
    case 'mem': {
      const pct = clampPct(info.memPercent)
      return { text: String(Math.round(pct)), unit: '%', pct }
    }
    case 'net_upload': {
      const p = bytesParts(info.netUpload)
      return { text: p.text, unit: `${p.unit}/s` }
    }
    case 'net_download': {
      const p = bytesParts(info.netDownload)
      return { text: p.text, unit: `${p.unit}/s` }
    }
    case 'net_total': {
      const p = bytesParts(info.netTotalSpeed)
      return { text: p.text, unit: `${p.unit}/s` }
    }
    case 'cpu_threads':
      return { text: String(info.cpuThreads), unit: '线程' }
    case 'traffic': {
      const p = bytesParts(info.netTotalTraffic)
      return { text: p.text, unit: p.unit }
    }
    default:
      return missing
  }
}

function clampPct(v: number): number {
  if (!Number.isFinite(v)) return 0
  return Math.min(100, Math.max(0, v))
}

/** 采样数据的最小结构（SystemInfo 的子集，避免测试加载整条 API 链） */
export interface SysmonSample {
  cpuUsage: number
  memPercent: number
  netUpload: number
  netDownload: number
  netTotalSpeed: number
  netTotalTraffic: number
  cpuThreads: number
}

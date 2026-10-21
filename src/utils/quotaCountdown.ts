/**
 * 发布配额重置倒计时文案（扩展中心「我的扩展」与发布弹窗共用）。
 *
 * 服务端的「每日提交次数」按服务端时区的自然日刷新，配额视图里带 `daily_reset_at`
 * （epoch ms）。客户端只负责把「还有多久」讲清楚——不猜「是不是明天」，跨时区也不会说错日期。
 */

/** 剩余时长：3 小时 25 分 / 25 分 12 秒 / 12 秒 / 即将重置 */
export function formatResetCountdown(resetAt: number, now: number): string {
  const ms = resetAt - now
  if (!Number.isFinite(ms) || ms <= 0) return '即将重置'
  const totalSec = Math.ceil(ms / 1000)
  const h = Math.floor(totalSec / 3600)
  const m = Math.floor((totalSec % 3600) / 60)
  const s = totalSec % 60
  if (h > 0) return m > 0 ? `${h} 小时 ${m} 分` : `${h} 小时`
  if (m > 0) return s > 0 ? `${m} 分 ${s} 秒` : `${m} 分`
  return `${s} 秒`
}

/** 重置时刻（客户端本地时区的 HH:MM）：与服务端时区不同也能对上用户自己的表 */
export function formatResetClock(resetAt: number): string {
  const d = new Date(resetAt)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}`
}

/**
 * 扩展反馈的展示口径（类型 / 状态 / 相对时间）。
 *
 * 市场详情（公开面）与开发者收件箱共用同一份：两处对同一条反馈的类型与时间
 * 必须显示一致，否则用户与开发者会以为是两件事。
 */

export function feedbackKindLabel(kind: string): string {
  return kind === 'bug' ? '缺陷' : kind === 'idea' ? '建议' : '提问'
}

export function feedbackStatusLabel(status: string): string {
  return status === 'resolved' ? '已解决' : status === 'replied' ? '已回复' : '待处理'
}

/** 相对时间：刚刚 / N 分钟前 / N 小时前 / 月-日（一年以上带年份） */
export function feedbackTimeText(ts: number, now = Date.now()): string {
  const diff = now - ts
  if (diff < 60_000) return '刚刚'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`
  const d = new Date(ts)
  const md = `${d.getMonth() + 1}-${String(d.getDate()).padStart(2, '0')}`
  return d.getFullYear() === new Date(now).getFullYear() ? md : `${d.getFullYear()}-${md}`
}

/**
 * 把 `CODE: message` 形式的后端错误换成人话（网络类特殊处理，业务文案直接透出）。
 * 提交弹窗与收件箱共用一份，避免同一错误在两处出现两种说法。
 */
export function feedbackErrorText(raw: string): string {
  if (raw.startsWith('NETWORK_ERROR')) return '网络不可用，请检查网络后重试'
  if (raw.startsWith('INTERNAL')) return '服务返回异常，请稍后再试'
  // 旧服务端没有这批路由（404 走统一的 not_found）——指路升级，而不是回一句「内容不存在」
  if (raw.startsWith('NOT_FOUND')) return '平台服务暂不支持该操作，请升级平台服务后重试'
  const cleaned = raw.replace(/^[A-Z][A-Z_]*:\s*/, '').trim()
  return cleaned || '操作失败，请稍后再试'
}

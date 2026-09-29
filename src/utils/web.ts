/** 归一化用户输入的网址：http/https 原样保留；其它协议前缀一律剥掉，按没写协议处理、默认补 http:// */
export function normalizeWebUrl(input: string): string {
  const trimmed = input.trim()
  if (/^https?:\/\//i.test(trimmed)) return trimmed
  const host = trimmed.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '')
  return `http://${host}`
}

/** 从完整网址推导站点 favicon 地址 */
export function deriveFaviconUrl(target: string): string | null {
  try {
    const url = new URL(target)
    return `${url.origin}/favicon.ico`
  } catch {
    return null
  }
}

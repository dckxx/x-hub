/** 远程资源协议（smb/ftp 系）：作为「网页」资源保存时原样保留、不走 http 补全 */
const REMOTE_SCHEME_RE = /^(ftp|ftps|sftp|smb):\/\//i

/** 是否为可内嵌/浏览器打开的普通网页（http/https）——smb/ftp 等远程协议只能交给系统打开 */
export function isHttpWebTarget(target: string): boolean {
  return /^https?:\/\//i.test(target.trim())
}

/**
 * 归一化用户输入的网址：http/https 与 smb/ftp 系远程协议原样保留；
 * 其它协议前缀一律剥掉，按没写协议处理、默认补 http://
 */
export function normalizeWebUrl(input: string): string {
  const trimmed = input.trim()
  if (/^https?:\/\//i.test(trimmed) || REMOTE_SCHEME_RE.test(trimmed)) return trimmed
  const host = trimmed.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '')
  return `http://${host}`
}

/** 从完整网址推导站点 favicon 地址（远程协议无站点图标语义，返回 null） */
export function deriveFaviconUrl(target: string): string | null {
  if (!isHttpWebTarget(target)) return null
  try {
    const url = new URL(target)
    return `${url.origin}/favicon.ico`
  } catch {
    return null
  }
}

export type WebScheme = 'http' | 'https'

export function splitWebTarget(input: string): { scheme: WebScheme; value: string } {
  const trimmed = input.trim()
  const match = trimmed.match(/^(https?):\/\/(.+)$/i)
  if (match) {
    return {
      scheme: match[1].toLowerCase() === 'http' ? 'http' : 'https',
      value: match[2],
    }
  }
  return { scheme: 'https', value: trimmed }
}

export function joinWebTarget(scheme: WebScheme, value: string): string {
  const trimmed = value.trim().replace(/^(https?):\/\//i, '')
  return `${scheme}://${trimmed}`
}

export function deriveFaviconUrl(target: string): string | null {
  try {
    const url = new URL(target)
    return `https://www.google.com/s2/favicons?domain=${encodeURIComponent(url.hostname)}&sz=64`
  } catch {
    return null
  }
}

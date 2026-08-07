export function parseTimestamp(value: string): number {
  const direct = Date.parse(value)
  if (!Number.isNaN(direct)) return direct

  if (value.includes(' ')) {
    const hasZone = /([zZ]|[+-]\d\d:?\d\d)$/.test(value)
    const normalized = value.replace(' ', 'T') + (hasZone ? '' : 'Z')
    const parsed = Date.parse(normalized)
    if (!Number.isNaN(parsed)) return parsed
  }

  return new Date(value).getTime()
}

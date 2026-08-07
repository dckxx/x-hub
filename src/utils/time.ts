export function parseTimestamp(value: string): number {
  const sqliteUtc = value.match(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?$/)
  if (sqliteUtc) {
    const normalized = value.replace(' ', 'T') + 'Z'
    const parsed = Date.parse(normalized)
    if (!Number.isNaN(parsed)) return parsed
  }

  const direct = Date.parse(value)
  if (!Number.isNaN(direct)) return direct

  if (value.includes(' ')) {
    const normalized = value.replace(' ', 'T')
    const parsed = Date.parse(normalized)
    if (!Number.isNaN(parsed)) return parsed
  }

  return new Date(value).getTime()
}

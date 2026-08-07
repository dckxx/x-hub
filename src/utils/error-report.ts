import { isTauri, tauriApi } from '../api/tauri'

export async function reportClientError(message: string, detail?: unknown) {
  if (!isTauri()) {
    console.error(message, detail)
    return
  }

  try {
    await tauriApi.logClientError({
      message,
      detail: detail == null ? null : stringifyDetail(detail),
    })
  } catch {
    console.error(message, detail)
  }
}

function stringifyDetail(detail: unknown): string {
  if (detail instanceof Error) {
    return `${detail.name}: ${detail.message}\n${detail.stack ?? ''}`.trim()
  }
  if (typeof detail === 'string') return detail
  try {
    return JSON.stringify(detail)
  } catch {
    return String(detail)
  }
}

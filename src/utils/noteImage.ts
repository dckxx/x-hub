import { isTauri, tauriApi } from '../api/tauri'

/**
 * 把编辑器拿到的图片 File 落盘为笔记图片，返回可内嵌 Markdown 的 URL。
 * Tauri 环境走 import_note_image（数据根 notes/images，xhub-note 协议渲染）；
 * 浏览器预览环境无后端，回退 data URL 直接展示。
 */
export async function saveNoteImageFile(file: File): Promise<string> {
  // 与 Rust 端 NOTE_IMAGE_MAX_BYTES 对齐：提前拒绝，避免 13MB+ 的 base64 白过一次 IPC
  if (file.size > 10 * 1024 * 1024) {
    throw new Error('图片超过 10MB，请压缩后再试')
  }
  const mime = file.type.startsWith('image/') ? file.type : 'image/png'
  const ext = (mime.slice('image/'.length) || 'png').toLowerCase()

  if (!isTauri()) {
    return await readAsDataURL(file)
  }
  const dataUrl = await readAsDataURL(file)
  const b64 = dataUrl.slice(dataUrl.indexOf(',') + 1)
  return await tauriApi.importNoteImage(b64, ext)
}

function readAsDataURL(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = () => reject(new Error('读取图片失败'))
    reader.readAsDataURL(file)
  })
}

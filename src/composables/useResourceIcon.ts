import { ref } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Archive, File, FileText, Film, Folder, Image, Music } from 'lucide-vue-next'
import { isTauri, type Resource } from '../api/tauri'

export const CATEGORY_ICONS = {
  文件夹: Folder,
  文档: FileText,
  图片: Image,
  视频: Film,
  音频: Music,
  压缩包: Archive,
  其他: File,
} as const

export const CATEGORY_ACCENTS = {
  文件夹: { soft: 'var(--c-yellow-soft)', strong: 'var(--c-yellow)', ink: 'var(--c-yellow-ink)' },
  文档: { soft: 'var(--c-purple-soft)', strong: 'var(--c-purple)', ink: 'var(--c-purple-ink)' },
  图片: { soft: 'var(--c-pink-soft)', strong: 'var(--c-pink)', ink: 'var(--c-pink-ink)' },
  视频: { soft: 'var(--c-blue-soft)', strong: 'var(--c-blue)', ink: 'var(--c-blue-ink)' },
  音频: { soft: 'var(--c-green-soft)', strong: 'var(--c-green)', ink: 'var(--c-green-ink)' },
  压缩包: { soft: 'var(--c-orange-soft)', strong: 'var(--c-orange)', ink: 'var(--c-orange-ink)' },
  其他: { soft: 'var(--c-gray-soft)', strong: 'var(--c-gray)', ink: 'var(--c-gray-ink)' },
} as const

const ACCENTS = [
  { strong: 'var(--c-yellow)', soft: 'var(--c-yellow-soft)', text: 'var(--c-yellow-ink)' },
  { strong: 'var(--c-red)', soft: 'var(--c-red-soft)', text: 'var(--c-red-ink)' },
  { strong: 'var(--c-blue)', soft: 'var(--c-blue-soft)', text: 'var(--c-blue-ink)' },
  { strong: 'var(--c-green)', soft: 'var(--c-green-soft)', text: 'var(--c-green-ink)' },
  { strong: 'var(--c-pink)', soft: 'var(--c-pink-soft)', text: 'var(--c-pink-ink)' },
  { strong: 'var(--c-orange)', soft: 'var(--c-orange-soft)', text: 'var(--c-orange-ink)' },
  { strong: 'var(--c-purple)', soft: 'var(--c-purple-soft)', text: 'var(--c-purple-ink)' },
  { strong: 'var(--c-gray)', soft: 'var(--c-gray-soft)', text: 'var(--c-gray-ink)' },
]

export function accentOf(name: string) {
  let h = 0
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0
  return ACCENTS[h % ACCENTS.length]
}

export function fileAccentOf(category: string) {
  return CATEGORY_ACCENTS[category as keyof typeof CATEGORY_ACCENTS] ?? CATEGORY_ACCENTS.其他
}

const IMAGE_ICON_RE = /\.(png|jpg|jpeg|ico|gif|webp)$/i

export function isImageIcon(icon: string | null): boolean {
  return !!icon && (/^https?:\/\//i.test(icon) || IMAGE_ICON_RE.test(icon))
}

export function iconSrc(icon: string): string {
  if (/^https?:\/\//i.test(icon)) return icon
  return isTauri() ? convertFileSrc(icon) : ''
}

export function useResourceIcon() {
  const failedIcons = ref(new Set<number>())

  function onIconError(r: Resource) {
    failedIcons.value.add(r.id)
  }

  function showImageIcon(r: Resource): boolean {
    return isImageIcon(r.icon) && !failedIcons.value.has(r.id)
  }

  function showWebFallbackIcon(r: Resource): boolean {
    return r.kind === 'web' && !showImageIcon(r)
  }

  function iconText(r: Resource): string {
    return r.name.charAt(0).toUpperCase()
  }

  function fileIconOf(r: Resource) {
    return CATEGORY_ICONS[(r.category ?? '其他') as keyof typeof CATEGORY_ICONS] ?? File
  }

  function accentFor(r: Resource) {
    if (r.kind === 'file') return fileAccentOf(r.category ?? '其他')
    return accentOf(r.name)
  }

  return {
    failedIcons,
    onIconError,
    showImageIcon,
    showWebFallbackIcon,
    iconText,
    fileIconOf,
    accentFor,
  }
}

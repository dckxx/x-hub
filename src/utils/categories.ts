// 文件分类定义（Suda / SudaFormDialog 共享）

export const CATEGORIES = [
  '文件夹',
  '文档',
  '图片',
  '视频',
  '音频',
  '压缩包',
  '其他',
] as const

export type FileCategory = (typeof CATEGORIES)[number]

const CATEGORY_PATTERNS: [RegExp, FileCategory][] = [
  [/\.(docx?|pdf|txt|md|xlsx?|pptx?|rtf|wps)$/i, '文档'],
  [/\.(png|jpe?g|gif|webp|svg|ico|bmp|tiff?)$/i, '图片'],
  [/\.(mp4|mkv|avi|mov|wmv|flv|webm|rmvb)$/i, '视频'],
  [/\.(mp3|wav|flac|aac|ogg|m4a|wma)$/i, '音频'],
  [/\.(zip|rar|7z|tar|gz|bz2|xz|iso)$/i, '压缩包'],
]

/** 按扩展名/目录自动识别分类 */
export function categorize(path: string, isDir: boolean): FileCategory {
  if (isDir) return '文件夹'
  for (const [re, c] of CATEGORY_PATTERNS) {
    if (re.test(path)) return c
  }
  return '其他'
}

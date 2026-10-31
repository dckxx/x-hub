/**
 * 发布弹窗「上一版截图」的引用口径（ExtensionPublishDialog.vue 使用）。
 *
 * 市场清单里的截图 URL 形如 `https://host/screenshots/<extId>/<sha16>.<ext>`；引用时
 * Rust `fetch_remote_screenshots` 按**内容哈希基名**落盘到临时目录（同图天然去重），
 * 所以「这张图是不是上一版那张」只能比基名——比整条路径必然不等（远端 URL vs 本地临时目录）。
 *
 * 纯函数零依赖：tests/shotRefs.test.mjs 直接加载（Node ESM 需带 .ts 扩展名）。
 */

/** 取路径或 URL 的最后一段文件名（去 query/fragment、统一小写）；取不到返回空串 */
export function shotBasename(pathOrUrl: string): string {
  const clean = pathOrUrl.split('#')[0].split('?')[0]
  const seg = clean.split(/[\\/]/).filter(Boolean).pop() ?? ''
  return seg.toLowerCase()
}

/** 已选图片里有多少张来自上一版截图（自动引用与手动引用同口径） */
export function referencedShotCount(prevUrls: readonly string[], picked: readonly string[]): number {
  const prev = new Set(prevUrls.map(shotBasename).filter(Boolean))
  if (!prev.size) return 0
  let n = 0
  for (const p of picked) {
    const b = shotBasename(p)
    if (b && prev.has(b)) n += 1
  }
  return n
}

/** 上一版截图是否已全部带到表单里（全都带齐就不必再给「引用上一版」入口） */
export function allShotsReferenced(prevUrls: readonly string[], picked: readonly string[]): boolean {
  const prev = new Set(prevUrls.map(shotBasename).filter(Boolean))
  if (!prev.size) return false
  const have = new Set(picked.map(shotBasename).filter(Boolean))
  for (const name of prev) {
    if (!have.has(name)) return false
  }
  return true
}

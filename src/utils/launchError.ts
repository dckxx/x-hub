/**
 * 速达启动错误判定。
 *
 * 后端 `process::launch_program` 对「路径型目标但文件已不存在」返回带
 * `TARGET_NOT_FOUND:` 前缀的错误（应用卸载/移动后点图标不再静默无反应）。
 * 前端据此提示「找不到对应应用，可以删除」，而不是抛出原始路径错误。
 */
const TARGET_NOT_FOUND = 'TARGET_NOT_FOUND'

/** 是否为「目标程序不存在」类错误（应用已卸载/移动） */
export function isTargetMissingError(e: unknown): boolean {
  const msg = e instanceof Error ? e.message : String(e)
  return msg.includes(TARGET_NOT_FOUND)
}

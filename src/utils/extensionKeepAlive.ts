/**
 * 扩展常驻后台（view 形态）的口径纯函数。
 * index.vue（停车场挂载/剪枝/自启动）、ExtensionSettingsDialog（开关 + 上限校验）、
 * ExtensionsPanel（设置 → 扩展「常驻扩展」小节）共用，口径唯一实现在这里，禁止各抄一份。
 * 被 node --test 直接加载 TS 源码，保持零运行时依赖（类型会被剥离）。
 */

/** 常驻实例上限：每个常驻 view 实例是独立渲染进程（约几十 MB），超上限提示先取消一个 */
export const EXT_KEEP_ALIVE_CAP = 5

export interface KeepAliveConfig {
  extension_keep_alive?: readonly string[]
  extension_autostart?: readonly string[]
}

/**
 * 生效的常驻 id 集合 = keep_alive ∪ autostart（「随应用启动」是常驻的加强项，
 * 语义上隐含常驻，运行时不区分来源）。保序去重：keep_alive 在前、autostart 补后。
 */
export function effectiveKeepAliveIds(cfg: KeepAliveConfig): string[] {
  const out: string[] = []
  for (const id of [...(cfg.extension_keep_alive ?? []), ...(cfg.extension_autostart ?? [])]) {
    if (id && !out.includes(id)) out.push(id)
  }
  return out
}

/**
 * 还能不能再常驻一个：excludeId 不计入——对已常驻的扩展调整自身开关时，
 * 上限只数其它扩展，否则先关自己会被误判成「满员」。
 */
export function hasKeepAliveSlot(cfg: KeepAliveConfig, excludeId?: string): boolean {
  return effectiveKeepAliveIds(cfg).filter((id) => id !== excludeId).length < EXT_KEEP_ALIVE_CAP
}

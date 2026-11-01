/** 是否运行在 macOS（桌面端）：文件选择器过滤、路径示例等平台分支用。
 *  与 stores/workbench.ts 的 IS_MAC_PREVIEW 同口径（userAgent + platform 双判）。
 *  浏览器预览态下 Mac 判定同样成立——速达的添加表单只在桌面端出现文件选择按钮，
 *  误判无实际影响。 */
export const IS_MAC =
  typeof navigator !== 'undefined' &&
  (/Mac|iPhone|iPad/.test(navigator.userAgent) || /Mac|iPhone|iPad/.test(navigator.platform))
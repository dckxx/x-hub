// 设置分区元数据（**单一数据源**）：左栏 10 大类（GROUPS）+ 各大类下的分区（SECTIONS）
// + 分区→大类归属（SECTION_GROUP）。SettingsView 的两级导航由这三份派生；
// GlobalSearch 的「设置」检索结果也用它显示归属路径（口径与设置页内搜索一致）。
//
// ⚠️ 新增/改名分区要同步三处：① 本文件 ② settings/*.vue 面板的 <section id="sv-sec-*">
// ③ settingsIndex.generated.ts（构建期从面板模板重新生成，npm run gen:settings-index）。
import { Bot, CircleHelp, Database, Info, LayoutGrid, Palette, Puzzle, Settings, Sparkles, User } from 'lucide-vue-next'

/** 各大类下的分区（左栏子项由此 + SECTION_GROUP 自动派生） */
export const SECTIONS = [
  { id: 'general', label: '常规' },
  { id: 'ball', label: '悬浮球' },
  { id: 'assistant', label: '助手' },
  { id: 'ai', label: '面板' },
  { id: 'credit', label: '额度' },
  { id: 'providers', label: '供应商' },
  { id: 'suda', label: '速达' },
  { id: 'notes', label: '速记' },
  { id: 'appearance', label: '外观' },
  { id: 'workbench', label: '工作台' },
  { id: 'shortcut', label: '快捷键' },
  { id: 'clipboard', label: '剪贴板' },
  // GIF 录制（2026-10-31）：截图/GIF 录屏功能设置分区（默认帧率/保存目录）
  { id: 'gif', label: 'GIF 录制' },
  { id: 'online', label: '联网' },
  { id: 'mem', label: '性能' },
  { id: 'account', label: '账号' },
  { id: 'extensions', label: '扩展' },
  { id: 'skills', label: 'Skills' },
  { id: 'data', label: '数据' },
  // 云同步（2026-11-02）：手机扫码配对 + E2EE，挂「数据」大类（docs/sync-protocol.md §8.4）
  { id: 'sync', label: '云同步' },
  // 使用说明（2026-10-24）：正文是仓库 docs/user-guide.md，面板内联渲染（见 GuidePanel.vue）
  { id: 'guide', label: '使用说明' },
  // 关于（2026-11-15 自「数据与关于」拆出独立成大类）：版本 / 开源声明 / 检查更新（见 AboutPanel.vue）
  { id: 'about', label: '关于' },
] as const

export type SectionId = (typeof SECTIONS)[number]['id']

/** 左栏 10 大类 */
export const GROUPS = [
  { id: 'general', label: '常规', icon: Settings },
  { id: 'appearance', label: '外观', icon: Palette },
  { id: 'workbench', label: '工作台', icon: LayoutGrid },
  // AI 助手独立成大类（2026-10-16 自「功能」拉出），排在工作台下面
  { id: 'ai', label: 'AI 助手', icon: Bot },
  { id: 'features', label: '功能', icon: Sparkles },
  { id: 'extensions', label: '扩展', icon: Puzzle },
  { id: 'account', label: '账号', icon: User },
  { id: 'data', label: '数据', icon: Database },
  // 使用说明独立成大类（2026-10-24）：面板内联 docs/user-guide.md，解决「功能多、路径难记」
  { id: 'guide', label: '使用说明', icon: CircleHelp },
  // 「关于」独立成大类（2026-11-15 自「数据与关于」拆出），排在左栏最下
  { id: 'about', label: '关于', icon: Info },
] as const

export type GroupId = (typeof GROUPS)[number]['id']

/** 分区 → 大类（Record 的键类型是 SectionId，漏一个分区编译期就报错） */
export const SECTION_GROUP: Record<SectionId, GroupId> = {
  general: 'general',
  ball: 'general',
  shortcut: 'general',
  appearance: 'appearance',
  workbench: 'workbench',
  assistant: 'ai',
  ai: 'ai',
  credit: 'ai',
  providers: 'ai',
  suda: 'features',
  notes: 'features',
  clipboard: 'features',
  online: 'features',
  mem: 'features',
  gif: 'features',
  extensions: 'extensions',
  skills: 'extensions',
  account: 'account',
  data: 'data',
  sync: 'data',
  guide: 'guide',
  about: 'about',
}

export const SECTION_LABEL: Record<string, string> = Object.fromEntries(
  SECTIONS.map((s) => [s.id, s.label]),
)

export const GROUP_LABEL: Record<string, string> = Object.fromEntries(
  GROUPS.map((g) => [g.id, g.label]),
)

import type { Component } from 'vue'
import {
  AppWindow,
  ClipboardList,
  FileText,
  FolderOpen,
  LayoutDashboard,
  MessageSquare,
  PenLine,
  Puzzle,
  Search,
  Settings,
} from 'lucide-vue-next'

/** 环形菜单按钮上限（与 Rust floating_ball::MAX_BUTTONS 一致） */
export const FLOATING_BALL_MAX_BUTTONS = 8

/**
 * 悬浮球环形菜单按钮目录：设置页配置与 FloatingBallWindow 渲染共用。
 * id 约定与 Rust floating_ball::trigger 对齐：view:* 切视图（先唤起主窗）、
 * act:* 快捷动作（search/note 经主窗事件，clipboard/main 由 Rust 直呼）。
 */
export const FLOATING_BALL_BUTTONS: Record<string, { label: string; icon: Component }> = {
  'view:dashboard': { label: '工作台', icon: LayoutDashboard },
  'view:notes': { label: '速记', icon: FileText },
  'view:suda': { label: '速达', icon: FolderOpen },
  'view:chat': { label: 'AI 对话', icon: MessageSquare },
  'view:extensions': { label: '扩展中心', icon: Puzzle },
  'view:settings': { label: '设置', icon: Settings },
  'act:search': { label: '全局搜索', icon: Search },
  'act:clipboard': { label: '剪贴板', icon: ClipboardList },
  'act:note': { label: '新建速记', icon: PenLine },
  'act:main': { label: '主窗口', icon: AppWindow },
}

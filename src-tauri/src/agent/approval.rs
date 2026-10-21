//! 权限三档 + 会话级授权（计划 §5，#t44）。
//!
//! **口径**：这是「用户对 AI 的授权」，与扩展系统 manifest 权限是两套独立体系，
//! 不可混用、不可共用存储（§5 已定）。

use std::collections::{HashMap, HashSet};

/// 工具权限级别（工具注册表 #t45 落 30 工具时标注）
#[allow(dead_code)] // Write/Dangerous 由 #t45 的工具标注使用
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// 读：直接执行
    Read,
    /// 写：受权限模式管
    Write,
    /// 危险：删除类与 launch_resource
    Dangerous,
}

impl Level {
    /// 事件协议里的级别标识（前端工具卡片着色/文案用）
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Read => "read",
            Level::Write => "write",
            Level::Dangerous => "dangerous",
        }
    }
}

/// 审批答复（Ask 档内联卡片三选一，计划 §5）：
/// - `Once`：本次允许；`Always`：本次允许且**本会话内**这类操作不再问（只存内存，§5 口径）
/// - `Denied`：拒绝——作为工具结果回给模型，由它决定换路或向用户解释
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApproveAnswer {
    Denied,
    Once,
    Always,
}

impl ApproveAnswer {
    /// 前端命令参数 → 答复（未知值一律按拒绝，宁严勿宽）
    pub fn from_param(s: &str) -> Self {
        match s {
            "once" => ApproveAnswer::Once,
            "always" => ApproveAnswer::Always,
            _ => ApproveAnswer::Denied,
        }
    }
}

impl PermissionMode {
    /// 前端/配置字符串 → 权限档（未知值回落由调用方处理，这里返回 None）
    pub fn from_param(s: &str) -> Option<PermissionMode> {
        match s {
            "readonly" => Some(PermissionMode::ReadOnly),
            "ask" => Some(PermissionMode::Ask),
            "full" => Some(PermissionMode::Full),
            _ => None,
        }
    }
}

/// 默认权限档：Ask（读直跑、写每次确认）。安全起步——#t47 接 `AppConfig.agent_permission_mode`
/// 后由用户设置决定，配置缺失/非法时回落到这里。
pub const DEFAULT_PERMISSION_MODE: PermissionMode = PermissionMode::Ask;

/// 权限三档（落 `AppConfig.agent_permission_mode`；serde default 兼容老配置，接线在 #t47）
#[allow(dead_code)] // ReadOnly/Full 由 #t47 配置接线后使用
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// 仅可查看：写/危险工具根本不进 tools 列表（循环里兜底拒绝，防模型硬调）
    ReadOnly,
    /// 每次都问：读直跑；写/危险每次内联确认，可「本会话内允许这类操作」
    Ask,
    /// 完全权限：全部直接执行，含删除与启动外部程序（2026-10-13 用户拍板，风险自担）
    Full,
}

/// 单次工具调用的处置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Execute,
    NeedApproval,
    /// ReadOnly 档对写/危险的兜底拒绝（正常流程它们不该出现在 tools 列表里）
    Denied,
}

/// 会话级授权（§5）：「每次都问」档下用户点「本会话内允许这类操作」后记录。
/// **只存内存**——语义就是「本会话」，进程重启即失效，避免临时允许悄悄变成永久允许。
#[derive(Clone, Default)]
pub struct SessionGrants {
    by_session: HashMap<i64, HashSet<String>>,
}

impl SessionGrants {
    pub fn grant(&mut self, session_id: i64, tool: &str) {
        self.by_session.entry(session_id).or_default().insert(tool.to_string());
    }

    pub fn is_granted(&self, session_id: i64, tool: &str) -> bool {
        self.by_session
            .get(&session_id)
            .is_some_and(|s| s.contains(tool))
    }

    /// 回合并发化后（0.9.0 修复）：每个回合在各自的快照上运行，结束后把新授权并回全局。
    /// 取并集而不是整体替换，避免并发回合互相覆盖「本会话允许」的授权。
    pub fn merge(&mut self, other: &SessionGrants) {
        for (sid, tools) in &other.by_session {
            let slot = self.by_session.entry(*sid).or_default();
            slot.extend(tools.iter().cloned());
        }
    }

    /// 会话删除/切换时清理（防 map 随长会话数无限增长）
    pub fn clear_session(&mut self, session_id: i64) {
        self.by_session.remove(&session_id);
    }
}

/// 权限判定（循环里对每个工具调用先过这里，计划 §5 三档口径）：
/// - ReadOnly：写/危险一律 Denied（兜底防御——它们本不该进 tools 列表）
/// - Ask：Read 直跑；Write/Dangerous → 会话级授权命中直跑，否则 NeedApproval
/// - Full：全部直跑；「不可逆操作仍提示」子开关（默认关）开启时 Dangerous 仍需确认，
///   但「本会话内允许」同样生效（卡片上点了不再反复弹）
pub fn decide(
    mode: PermissionMode,
    level: Level,
    tool: &str,
    session_id: i64,
    grants: &SessionGrants,
    prompt_on_dangerous_in_full: bool,
) -> Decision {
    match mode {
        PermissionMode::ReadOnly => match level {
            Level::Read => Decision::Execute,
            Level::Write | Level::Dangerous => Decision::Denied,
        },
        PermissionMode::Ask => match level {
            Level::Read => Decision::Execute,
            Level::Write | Level::Dangerous => {
                if grants.is_granted(session_id, tool) {
                    Decision::Execute
                } else {
                    Decision::NeedApproval
                }
            }
        },
        PermissionMode::Full => match level {
            Level::Read | Level::Write => Decision::Execute,
            Level::Dangerous => {
                if !prompt_on_dangerous_in_full || grants.is_granted(session_id, tool) {
                    Decision::Execute
                } else {
                    Decision::NeedApproval
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_denies_writes_even_if_granted() {
        let mut g = SessionGrants::default();
        g.grant(1, "delete_todo");
        assert_eq!(decide(PermissionMode::ReadOnly, Level::Read, "list_todos", 1, &g, false), Decision::Execute);
        assert_eq!(decide(PermissionMode::ReadOnly, Level::Write, "create_note", 1, &g, false), Decision::Denied);
        // 会话级授权救不了只读档（工具压根不该在列表里，这里是防御）
        assert_eq!(decide(PermissionMode::ReadOnly, Level::Dangerous, "delete_todo", 1, &g, false), Decision::Denied);
    }

    #[test]
    fn ask_mode_uses_session_grants() {
        let mut g = SessionGrants::default();
        assert_eq!(decide(PermissionMode::Ask, Level::Read, "search_notes", 1, &g, false), Decision::Execute);
        assert_eq!(decide(PermissionMode::Ask, Level::Write, "create_todo", 1, &g, false), Decision::NeedApproval);
        g.grant(1, "create_todo");
        assert_eq!(decide(PermissionMode::Ask, Level::Write, "create_todo", 1, &g, false), Decision::Execute);
        // 授权按会话隔离：会话 2 没授过
        assert_eq!(decide(PermissionMode::Ask, Level::Write, "create_todo", 2, &g, false), Decision::NeedApproval);
        // 按工具隔离：授了 create_todo 不等于授了 delete_todo
        assert_eq!(decide(PermissionMode::Ask, Level::Dangerous, "delete_todo", 1, &g, false), Decision::NeedApproval);
    }

    #[test]
    fn full_mode_executes_unless_irreversible_switch_on() {
        let g = SessionGrants::default();
        assert_eq!(decide(PermissionMode::Full, Level::Write, "create_note", 1, &g, false), Decision::Execute);
        // 删除类默认直接执行（2026-10-13 拍板：完全权限 = 风险自担）
        assert_eq!(decide(PermissionMode::Full, Level::Dangerous, "launch_resource", 1, &g, false), Decision::Execute);
        // 子开关「不可逆操作仍提示」（默认关）打开时危险级恢复确认
        assert_eq!(decide(PermissionMode::Full, Level::Dangerous, "launch_resource", 1, &g, true), Decision::NeedApproval);
    }

    /// 0.9.0 审查回归锁：Full+「危险仍提示」下「本会话内允许」也要生效——
    /// turn.rs 的 Always 答复会写 grants，这里不查的话按钮承诺是假的（每个危险操作仍反复弹卡）
    #[test]
    fn full_mode_prompt_on_honors_session_grants() {
        let mut g = SessionGrants::default();
        g.grant(1, "launch_resource");
        assert_eq!(decide(PermissionMode::Full, Level::Dangerous, "launch_resource", 1, &g, true), Decision::Execute);
        // 未授权的其它危险工具仍要确认；其它会话的授权不算数
        assert_eq!(decide(PermissionMode::Full, Level::Dangerous, "delete_todo", 1, &g, true), Decision::NeedApproval);
        assert_eq!(decide(PermissionMode::Full, Level::Dangerous, "launch_resource", 2, &g, true), Decision::NeedApproval);
    }

    #[test]
    fn grants_clear_per_session() {
        let mut g = SessionGrants::default();
        g.grant(1, "create_todo");
        g.grant(2, "create_todo");
        g.clear_session(1);
        assert!(!g.is_granted(1, "create_todo"));
        assert!(g.is_granted(2, "create_todo"));
    }
}

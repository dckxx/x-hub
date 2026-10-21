//! 工具注册表（计划 §5，#t44 增量 2）：trait + 过滤 + schema 组装。
//!
//! 30 个内置工具在 #t45 落地——本模块只定契约：
//! - `AgentTool`：同步 `execute`（本机 DB/文件操作都是同步的，没必要引入 async trait），
//!   结果统一 `ToolOutcome`（summary 给 UI 卡片，detail 原文回给模型）。
//! - `ToolRegistry::schemas_for(mode)`：按权限档过滤后组装 OpenAI tools 数组。
//!   ReadOnly 只下发读工具（写/危险**根本不进列表**，模型无从调用——§5 第一道防线；
//!   循环里 `decide()` 的 Denied 是第二道兜底）。

use rusqlite::Connection;
use serde_json::Value;

use super::approval::{Level, PermissionMode};

/// 一次工具执行的结果：`ok` 决定 tool 消息状态（ok/error）；`summary` 一句话给 UI；
/// `detail` 完整结果给模型（读类工具的正文明细都放这里）。
#[allow(dead_code)] // ok() 由 #t45 的工具实现使用
pub struct ToolOutcome {
    pub ok: bool,
    pub summary: String,
    pub detail: String,
}

impl ToolOutcome {
    #[allow(dead_code)] // #t45 的工具实现使用
    pub fn ok(summary: impl Into<String>, detail: impl Into<String>) -> Self {
        ToolOutcome { ok: true, summary: summary.into(), detail: detail.into() }
    }

    pub fn err(summary: impl Into<String>, detail: impl Into<String>) -> Self {
        ToolOutcome { ok: false, summary: summary.into(), detail: detail.into() }
    }
}

/// 工具执行上下文：执行时才借用连接（注册表是全局单例，连接随调用传入）。
/// #t45 若需要更多环境（数据目录、AppConfig 只读快照），在此扩字段。
pub struct ToolCtx<'a> {
    #[allow(dead_code)] // #t45 的工具实现读取；本增量只有测试假工具
    pub conn: &'a Connection,
}

/// 内置工具的统一契约（#t45 逐个实现并注册）
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn level(&self) -> Level;
    /// 参数 JSON Schema（object），随请求下发给模型
    fn parameters(&self) -> Value;
    /// 审批档位与会话级授权键（turn 循环在 decide 前调用）。
    /// 默认 = 静态档位 + 工具名；extension_tool_call 覆写为「目标扩展工具的声明档 +
    /// 带 ext/tool 的键」，让 Ask 档「本会话内允许」按目标细粒度生效
    fn approval_for(&self, _args: &Value) -> (Level, String) {
        (self.level(), self.name().to_string())
    }
    /// 执行：参数已由循环做 JSON 解析；实现内部自行做字段校验，
    /// 失败返回 `ToolOutcome::err`（作为工具结果回给模型自行纠错，不终止会话）。
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome;
}

/// 全局工具注册表（lib 启动时注册一次；查询多、注册少，无需读写锁）
pub struct ToolRegistry {
    tools: Vec<Box<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn empty() -> Self {
        ToolRegistry { tools: Vec::new() }
    }

    #[allow(dead_code)] // #t45 注册内置工具时使用
    pub fn register(&mut self, tool: Box<dyn AgentTool>) {
        // 同名注册是编程错误：直接 panic 暴露（内置表在启动时注册，不该发生）
        assert!(
            self.tools.iter().all(|t| t.name() != tool.name()),
            "重复注册工具: {}",
            tool.name()
        );
        self.tools.push(tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn AgentTool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// 按权限档可用的工具（ReadOnly 档只暴露读工具）
    pub fn available(&self, mode: PermissionMode) -> Vec<&dyn AgentTool> {
        self.tools
            .iter()
            .map(|t| t.as_ref())
            .filter(|t| match mode {
                PermissionMode::ReadOnly => t.level() == Level::Read,
                PermissionMode::Ask | PermissionMode::Full => true,
            })
            .collect()
    }

    /// 组装 OpenAI tools 数组（空列表时调用方应不下发 tools 字段，省 token）
    pub fn schemas_for(&self, mode: PermissionMode) -> Vec<Value> {
        self.available(mode)
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.parameters(),
                    }
                })
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::empty()
    }
}

static REGISTRY: std::sync::OnceLock<ToolRegistry> = std::sync::OnceLock::new();

/// 全局注册表（只读）：启动时注册全部 30 个内置工具（计划 §5 全量表，#t45）。
/// 顺序在 `builtin::register_all` 里固定——tools 数组随每次请求下发，勿乱序。
pub fn global() -> &'static ToolRegistry {
    REGISTRY.get_or_init(|| {
        let mut reg = ToolRegistry::empty();
        super::builtin::register_all(&mut reg);
        reg
    })
}

/// 回填模型的正文硬截断（§5 结果长度控制：正文类最多 2000 字；列表条数由各工具自限 20 条）
pub fn cap_for_model(s: &str) -> String {
    const MAX: usize = 2000;
    let count = s.chars().count();
    if count <= MAX {
        s.to_string()
    } else {
        let head: String = s.chars().take(MAX).collect();
        format!("{head}…（已截断，全文见界面，共 {count} 字）")
    }
}

/// 工具 → 数据域映射：执行成功的写类工具记录所属域，回合结束由调用方广播
/// 对应刷新事件（0.9.0 修复：agent 改数据后界面无感知）。只列**会改数据**的工具；
/// 纯读工具与 set_clipboard/launch_resource（无持久数据变化）返回 None。
pub fn domain_of(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "create_todo" | "update_todo" | "delete_todo" => Some("todos"),
        "create_note" | "update_note" | "append_to_note" | "delete_note" => Some("notes"),
        "add_snippet" | "update_snippet" | "delete_snippet" => Some("snippets"),
        "write_sticky" => Some("stickies"),
        "create_countdown" | "toggle_countdown" | "delete_countdown" => Some("countdowns"),
        // 0.9.0 修复：速达资源/小类此前不广播刷新——AI 建的资源界面看不到（重启才出现）
        "create_resource" | "delete_resource" | "create_subcategory" => Some("resources"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct FakeRead;
    struct FakeWrite;

    impl AgentTool for FakeRead {
        fn name(&self) -> &'static str {
            "fake_read"
        }
        fn description(&self) -> &'static str {
            "读工具"
        }
        fn level(&self) -> Level {
            Level::Read
        }
        fn parameters(&self) -> Value {
            json!({"type":"object","properties":{}})
        }
        fn execute(&self, _args: &Value, _ctx: ToolCtx) -> ToolOutcome {
            ToolOutcome::ok("ok", "done")
        }
    }

    impl AgentTool for FakeWrite {
        fn name(&self) -> &'static str {
            "fake_write"
        }
        fn description(&self) -> &'static str {
            "写工具"
        }
        fn level(&self) -> Level {
            Level::Write
        }
        fn parameters(&self) -> Value {
            json!({"type":"object","properties":{}})
        }
        fn execute(&self, _args: &Value, _ctx: ToolCtx) -> ToolOutcome {
            ToolOutcome::ok("ok", "done")
        }
    }

    #[test]
    fn cap_for_model_truncates_long_text() {
        let long = "字".repeat(3000);
        let capped = super::cap_for_model(&long);
        assert!(capped.chars().count() < 2100);
        assert!(capped.contains("已截断"));
        assert_eq!(super::cap_for_model("短文本"), "短文本");
    }
    #[test]
    fn readonly_only_exposes_read_tools() {
        let mut reg = ToolRegistry::empty();
        reg.register(Box::new(FakeRead));
        reg.register(Box::new(FakeWrite));
        assert_eq!(reg.available(PermissionMode::ReadOnly).len(), 1);
        assert_eq!(reg.available(PermissionMode::Ask).len(), 2);

        let schemas = reg.schemas_for(PermissionMode::ReadOnly);
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["type"], "function");
        assert_eq!(schemas[0]["function"]["name"], "fake_read");
        assert!(schemas[0]["function"]["parameters"].is_object());
        // Ask/Full 档两把都在列表里
        assert_eq!(reg.schemas_for(PermissionMode::Full).len(), 2);
    }

    #[test]
    fn get_by_name_and_duplicate_register_panics() {
        let mut reg = ToolRegistry::empty();
        reg.register(Box::new(FakeRead));
        assert!(reg.get("fake_read").is_some());
        assert!(reg.get("nope").is_none());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            reg.register(Box::new(FakeRead));
        }));
        assert!(result.is_err(), "重复注册应 panic");
    }

    #[test]
    fn outcome_helpers() {
        let o = ToolOutcome::ok("查到 3 条", "明细");
        assert!(o.ok && o.summary == "查到 3 条" && o.detail == "明细");
        let e = ToolOutcome::err("失败", "原因");
        assert!(!e.ok);
    }
}

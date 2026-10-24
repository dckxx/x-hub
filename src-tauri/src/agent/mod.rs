//! Agent 底座（计划 §4/§5，#t44）：
//! - `context`：按 turn 截断 + 历史工具消息成对剪枝
//! - `approval`：权限三档判定 + 会话级授权（只存内存）
//! - `prompt`：system prompt 四段组装
//! - `tools`：工具注册表契约（ToolRegistry / ToolCtx / schema 组装）
//! - `builtin`：34 个内置工具实现（#t45 全量 30 + create_subcategory + get_snippet + 扩展桥 2：读 15 / 写 14 / 危险 5）
//! - `ext_tools`：扩展 AI 工具聚合与转发（manifest.agent.tools → service 后端）
//! - `turn`：循环编排（模型调用 ↔ 工具执行，10 轮上限、审批、中断）
//! - `state`：运行时状态（审批挂起表 / 会话授权 / 中断旗标，tauri managed）
pub mod approval;
pub mod builtin;
pub mod context;
pub mod ext_tools;
pub mod prompt;
pub mod state;
pub mod tools;
pub mod turn;

/// 模型返回空正文时的统一报错文案（agent 循环用）。
///
/// 空回复不是正常回复：上游把错误塞进 SSE 错误帧、模型被网关截断、供应方临时不可用，
/// 都会以「HTTP 200 + 0 字符」的样子回来。放行的话前端会落一条 0 字符的 assistant
/// 消息——界面上是一个没有文字也没有报错的空气泡（0.9.2 反馈「发消息不回复」）。
/// 用户至少得知道自己该重试或换模型。
pub const EMPTY_REPLY_MESSAGE: &str =
    "模型没有返回任何内容（上游可能暂无可用供应方或连接被中断）。请稍后重试，或在模型下拉里换一个模型。";

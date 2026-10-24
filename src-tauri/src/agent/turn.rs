//! Agent 循环编排（计划 §4，#t44 增量 2）。
//!
//! 一次用户输入的处理流程：
//! ```text
//! user 消息（调用方已落库，含 turn_id）
//!   → 【模型调用 + 工具执行】循环（最多 max_rounds 轮；末轮强制不下发工具，逼模型收尾）
//!       → 组装 messages：system + 历史（调用方已按 turn 截断）+ 本轮工具结果
//!       → 流式：content 片段 → Chunk 事件；tool_calls 按 index 累积（chat.rs 已做）
//!       → 无 tool_calls → 最终回复，循环结束
//!       → 有 tool_calls → 逐个过 decide()：
//!             Execute      → ToolCall 事件 → 执行 → ToolResult 事件
//!             NeedApproval → ApprovalRequest 事件 → 等前端答复（once/always/denied）
//!             Denied       → 只读档兜底拒绝（写/危险本不该在 tools 列表里）
//!       → 结果作为 role:"tool" 消息追加 → 下一轮
//!   → 产出 TurnArtifacts：待落库消息序列 + 累计 usage + 工具计数（落库与 TurnDone 事件由调用方做）
//! ```
//! 关键口径（§4）：
//! - 工具执行失败 = 工具结果回给模型（自行纠错/换路），**不终止会话**
//! - 模型调用失败（网络/格式）= 原样报错给调用方，已执行的工具不回滚
//! - 中断 = 停止循环 + 已执行写操作不回滚（UI 已实时呈现）
//! - 轮次耗尽仍想要工具 = 合成一条收尾提示作为最终回复

use futures_util::future::BoxFuture;
use serde_json::Value;

use super::approval::{decide, ApproveAnswer, Decision, Level, PermissionMode, SessionGrants};
use super::tools::{ToolCtx, ToolOutcome, ToolRegistry};
use crate::chat::{ChatStreamEvent, ChatUsage, ToolCallRequest};
use crate::models::ChatMessage;

/// 生产命令传 [`MAX_ROUNDS`]；测试传小值验证收尾逻辑
pub const MAX_ROUNDS: usize = 10;

/// 流式增量回调（循环里转成 Chunk 事件；返回 Err 即中止流）。
/// 标注 Send：回调会跨进 invoke 的 'static future（生产实现里随流式循环逐段转发）。
pub type ChunkCb<'a> = &'a mut (dyn FnMut(String) -> Result<(), String> + Send + 'a);

/// 一次模型调用的输出：正文 + usage + 累积出的 tool_calls（chat::stream_chat_ex 的加工形态）
pub struct RoundOutput {
    pub content: String,
    pub usage: ChatUsage,
    pub tool_calls: Vec<ToolCallRequest>,
}

/// invoke 返回的 future 借用增量回调（Send 由 ChunkCb 的 Send 标注保证）
pub type InvokeFuture<'a> = BoxFuture<'a, Result<RoundOutput, String>>;
pub type ApproveFuture = BoxFuture<'static, ApproveAnswer>;

/// 模型调用抽象：入参为本轮完整 messages（已含 system）、权限档过滤后的 tools
///（None=不下发 tools 字段）、增量回调。生产实现包 `chat::stream_chat_ex`，测试用脚本假实现。
pub trait ModelInvoke: Send {
    fn invoke<'a>(
        &mut self,
        msgs: Vec<ChatMessage>,
        tools: Option<Vec<Value>>,
        cb: ChunkCb<'a>,
    ) -> InvokeFuture<'a>;
}

/// 审批流抽象：Ask 档需要确认时调用，返回用户的答复。
/// 生产实现：发 ApprovalRequest 事件后挂 oneshot 等前端命令回答；测试直接脚本化。
pub trait Approver: Send {
    fn approve(&mut self, ask: ApprovalAsk) -> ApproveFuture;
}

/// 一次待确认的工具调用（审批卡片/oneshot 键用）。
/// 序列化给前端 `list_agent_approvals`：面板重开/切回会话时还原 awaiting 卡片，
/// 字段命名与 ApprovalRequest 事件（camelCase）保持一致。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalAsk {
    pub session_id: i64,
    pub turn_id: String,
    pub call_id: String,
    pub name: String,
    pub args: String,
    pub level: &'static str,
}

/// 循环产出的待落库消息（按发生顺序）：调用方逐条 `add_message_ex(turn_id)` 落库
#[derive(Debug, Clone)]
pub struct PendingMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_status: Option<String>,
}

/// 一轮对话（turn）的完整产物
#[derive(Debug, Default)]
pub struct TurnArtifacts {
    /// 全部模型调用 usage 之和
    pub usage: ChatUsage,
    /// 实际执行成功的工具调用数（被拒/失败不计）
    pub tool_count: usize,
    /// 完成的模型调用轮数
    pub rounds: usize,
    /// 用户中断（已执行的操作不回滚，UI 已实时呈现）
    pub interrupted: bool,
    /// 本回合工具实际改动过的数据域（todos/notes/…，去重）。
    /// 调用方据此广播对应刷新事件（0.9.0 修复：agent 写库后界面无感知）。
    pub domains: std::collections::BTreeSet<String>,
    /// 待落库消息：中间 assistant(tool_calls)、tool 结果、最终 assistant
    pub pending: Vec<PendingMessage>,
    /// 最终 assistant 正文（pending 里最后一条 assistant 的 content）
    pub final_reply: Option<String>,
}

/// 跑一轮对话。`history` 为调用方组装好的上下文（**已含本轮 user 消息**，已按 turn 截断）；
/// `system_prompt` 由 `prompt::build_system_prompt` 组装。
///
/// 真错误以 `Err((原因, 已产出产物))` 返回：出错前工具可能已经改过数据（比如待办已建），
/// 调用方必须照常按 `domains` 广播刷新事件——只把错误抛上去会把「已生效的写入」瞒在库里，
/// 界面上又是「说建好了却看不到」。
#[allow(clippy::too_many_arguments)]
pub async fn run_agent_turn<K>(
    session_id: i64,
    turn_id: &str,
    mode: PermissionMode,
    grants: &mut SessionGrants,
    prompt_on_dangerous_in_full: bool,
    registry: &ToolRegistry,
    system_prompt: &str,
    history: Vec<ChatMessage>,
    max_rounds: usize,
    invoke: &mut dyn ModelInvoke,
    approve: &mut dyn Approver,
    emit: impl FnMut(ChatStreamEvent) -> Result<(), String> + Send,
    is_cancelled: impl Fn() -> bool,
    // 生产传 db::open_tool_connection，测试传内存库工厂（工具执行不碰真实数据）
    open_tool_conn: K,
) -> Result<TurnArtifacts, (String, TurnArtifacts)>
where
    K: Fn() -> std::result::Result<rusqlite::Connection, String>,
{
    let mut art = TurnArtifacts::default();
    match run_agent_turn_inner(
        session_id, turn_id, mode, grants, prompt_on_dangerous_in_full, registry,
        system_prompt, history, max_rounds, invoke, approve, emit, is_cancelled,
        open_tool_conn, &mut art,
    )
    .await
    {
        Ok(()) => Ok(art),
        Err(e) => Err((e, art)),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_agent_turn_inner<K>(
    session_id: i64,
    turn_id: &str,
    mode: PermissionMode,
    grants: &mut SessionGrants,
    prompt_on_dangerous_in_full: bool,
    registry: &ToolRegistry,
    system_prompt: &str,
    history: Vec<ChatMessage>,
    max_rounds: usize,
    invoke: &mut dyn ModelInvoke,
    approve: &mut dyn Approver,
    mut emit: impl FnMut(ChatStreamEvent) -> Result<(), String> + Send,
    is_cancelled: impl Fn() -> bool,
    open_tool_conn: K,
    art: &mut TurnArtifacts,
) -> Result<(), String>
where
    K: Fn() -> std::result::Result<rusqlite::Connection, String>,
{
    let mut convo: Vec<ChatMessage> = Vec::with_capacity(history.len() + 8);
    convo.push(wire_msg("system", system_prompt.to_string(), turn_id, None, None, None, None));
    convo.extend(history);

    for round in 1..=max_rounds {
        if is_cancelled() {
            art.interrupted = true;
            break;
        }
        art.rounds = round;

        // 末轮不下发工具：强制模型收尾（§4 循环上限口径），工具列表空时同样省略字段
        let tools_json = registry.schemas_for(mode);
        let pass_tools = if round == max_rounds || tools_json.is_empty() {
            None
        } else {
            Some(tools_json)
        };

        let mut buf = String::new();
        let mut on_chunk =
            |delta: String| emit(ChatStreamEvent::Chunk { content: delta });
        // 流式中途点「停止」：invoke 以 Err("已中断") 收场，这里不能向上抛——
        // 已执行的工具过程（art.pending）照常落库，与轮间检查点的中断口径一致；
        // 真错误才冒泡（commands.rs 的 Err 分支不落库，前端只收到 Error 事件）
        let output = match invoke.invoke(convo.clone(), pass_tools, &mut on_chunk).await {
            Ok(o) => o,
            Err(e) => {
                if is_cancelled() {
                    art.interrupted = true;
                    break;
                }
                return Err(e);
            }
        };
        art.usage.accumulate(output.usage);
        buf.push_str(&output.content);

        if output.tool_calls.is_empty() {
            let content = buf.trim().to_string();
            // 空回复不能当正常收尾：上游把错误塞进 SSE 错误帧、或网关偶发返回空正文时，
            // 放行这里就会落一条 0 字符的 assistant 消息——界面是个没有文字的空气泡，
            // 用户看到的就是「发消息它不回复」（0.9.2 反馈）。宁可报错让前端渲染错误气泡，
            // 也不要把一次失败的调用伪装成成功的空回复。
            if content.is_empty() {
                // 本回合还没执行过任何工具：整轮啥也没发生，按真错误上报（前端渲染错误气泡），
                // 不落库——落一条空的 assistant 只会污染历史。
                if art.pending.is_empty() {
                    return Err(super::EMPTY_REPLY_MESSAGE.into());
                }
                // 已经跑过工具轮：工具调用与结果必须成对落库（否则下次请求上游 400），
                // 这里不能向上抛。合成一条收尾说明当最终回复，让用户知道工具做了什么、
                // 但这次没能给出总结。
                let text = super::EMPTY_REPLY_MESSAGE.to_string();
                log::warn!("[agent:{turn_id}] 第 {round} 轮空回复，但本回合已有工具过程，合成收尾说明");
                art.pending.push(PendingMessage {
                    role: "assistant".into(),
                    content: text.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                    tool_status: None,
                });
                art.final_reply = Some(text);
                return Ok(());
            }
            log::info!("[agent:{turn_id}] 第 {round} 轮：模型未发起工具调用，直接给出回复");
            art.pending.push(PendingMessage {
                role: "assistant".into(),
                content: content.clone(),
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                tool_status: None,
            });
            art.final_reply = Some(content);
            return Ok(());
        }

        // 中间 assistant 消息：落库与回传都按 OpenAI 协议形态存 tool_calls
        log::info!("[agent:{turn_id}] 第 {round} 轮：{} 个工具调用", output.tool_calls.len());
        let tc_json = tool_calls_json(&output.tool_calls)?;
        let asst = PendingMessage {
            role: "assistant".into(),
            content: buf.trim().to_string(),
            tool_calls: Some(tc_json.clone()),
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
        };
        art.pending.push(asst.clone());
        convo.push(wire_msg(
            "assistant",
            asst.content.clone(),
            turn_id,
            Some(tc_json),
            None,
            None,
            None,
        ));

        for (ci, call) in output.tool_calls.iter().enumerate() {
            if is_cancelled() {
                art.interrupted = true;
                // 中断时未执行的调用必须补一条取消结果再落库：assistant.tool_calls 与
                // role:"tool" 消息是上游强校验的成对关系，缺一边下轮请求会被 400 拒绝
                //（已执行的写操作不回滚，这里只补协议占位，不产生 ToolCall 事件；
                //  发送失败也忽略——马上要 break，落库成对性比事件送达更重要）
                for rest in &output.tool_calls[ci..] {
                    let _ = push_tool_result(
                        turn_id,
                        rest,
                        &ToolOutcome::err("回合已中断，该调用未执行", String::new()),
                        "cancelled",
                        &mut emit,
                        art,
                        &mut convo,
                    );
                }
                break;
            }
            // 档位与授权键可按参数覆写（extension_tool_call：档位取目标扩展工具声明档，
            // 授权键带 ext/tool——「本会话内允许」按目标细粒度生效，不会一通百通）
            let probe: Value = if call.arguments.trim().is_empty() {
                Value::Object(serde_json::Map::new())
            } else {
                serde_json::from_str(&call.arguments).unwrap_or(Value::Null)
            };
            let (level, perm_key) = registry
                .get(&call.name)
                .map(|t| t.approval_for(&probe))
                .unwrap_or((Level::Read, call.name.clone()));
            let decision = decide(
                mode,
                level,
                &perm_key,
                session_id,
                grants,
                prompt_on_dangerous_in_full,
            );
            // 全链路留痕：模型没调工具 / 调了被拒 / 等审批 / 已授权直跑，翻日志即知
            // （「模型谎报已记录」那次靠翻库才定位，log 里有案底就不用再翻）。
            // 打 perm_key 而非工具名：extension_tool_call 的日志能看出打的是哪个扩展的哪个工具
            log::info!(
                "[agent:{turn_id}] 工具 {}（{}）→ {:?}",
                perm_key,
                level.as_str(),
                decision
            );
            match decision {
                Decision::Execute => {
                    execute_call(
                        session_id, turn_id, &call, level, registry, &mut emit,
                        art, &mut convo, &open_tool_conn,
                    )?;
                }
                Decision::NeedApproval => {
                    // 事件由循环统一发（Approver 只负责等待答复）；前端渲染内联确认卡片。
                    // 发送失败（前端窗口已销毁）直接终止回合——没必要再等一个永远无人应答的审批
                    emit(ChatStreamEvent::ApprovalRequest {
                        turn_id: turn_id.to_string(),
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        args: call.arguments.clone(),
                        level: level.as_str().to_string(),
                    })?;
                    let answer = approve.approve(ApprovalAsk {
                        session_id,
                        turn_id: turn_id.to_string(),
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        args: call.arguments.clone(),
                        level: level.as_str(),
                    })
                    .await;
                    match answer {
                        ApproveAnswer::Denied => {
                            // 拒绝也作为工具结果回给模型：它可以道歉、换路或向用户解释
                            push_tool_result(
                                turn_id,
                                &call,
                                &ToolOutcome::err("用户拒绝执行该工具", String::new()),
                                "denied",
                                &mut emit,
                                art,
                                &mut convo,
                            )?;
                        }
                        ApproveAnswer::Once => {
                            execute_call(
                                session_id, turn_id, &call, level, registry,
                                &mut emit, art, &mut convo, &open_tool_conn,
                            )?;
                        }
                        ApproveAnswer::Always => {
                            // 「本会话内允许这类操作」：只存内存，重启即失效（§5）。
                            // 键经 approval_for 生成（extension_tool_call 带 ext/tool）
                            grants.grant(session_id, &perm_key);
                            execute_call(
                                session_id, turn_id, &call, level, registry,
                                &mut emit, art, &mut convo, &open_tool_conn,
                            )?;
                        }
                    }
                }
                Decision::Denied => {
                    // 只读档兜底：写/危险本不该出现在 tools 列表（第一道防线已滤），
                    // 模型硬调时明确回绝，不让它误以为执行成功
                    push_tool_result(
                        turn_id,
                        &call,
                        &ToolOutcome::err(
                            "当前权限模式为「仅可查看」，已拒绝执行该操作",
                            String::new(),
                        ),
                        "denied",
                        &mut emit,
                        art,
                        &mut convo,
                    )?;
                }
            }
        }
        if art.interrupted {
            break;
        }
    }

    // 轮次耗尽（非中断）仍没等到纯文本回复：合成收尾提示作为最终回复
    if !art.interrupted && art.final_reply.is_none() {
        let text = "这个任务比较复杂，达到了单次对话的工具调用上限。已有进展不会丢失，\
                    你可以把剩余的部分拆成新的要求，我们继续。"
            .to_string();
        art.pending.push(PendingMessage {
            role: "assistant".into(),
            content: text.clone(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
        });
        art.final_reply = Some(text);
    }
    Ok(())
}

/// 已获准（或会话已授权）的调用：发 ToolCall → 执行 → 发 ToolResult → 追加 tool 消息。
/// 事件发送失败（前端窗口已销毁）向上传播终止回合——纯工具轮的 Chunk 增量可能一条
/// 都没有，不传播的话循环会对着死通道空转完 MAX_ROUNDS 轮模型调用
#[allow(clippy::too_many_arguments)]
fn execute_call(
    // 预留给 #t45 的工具上下文（如会话级作用域）；当前执行路径未用
    _session_id: i64,
    turn_id: &str,
    call: &ToolCallRequest,
    level: Level,
    registry: &ToolRegistry,
    emit: &mut dyn FnMut(ChatStreamEvent) -> Result<(), String>,
    art: &mut TurnArtifacts,
    convo: &mut Vec<ChatMessage>,
    open_conn: &dyn Fn() -> std::result::Result<rusqlite::Connection, String>,
) -> Result<(), String> {
    emit(ChatStreamEvent::ToolCall {
        turn_id: turn_id.to_string(),
        call_id: call.id.clone(),
        name: call.name.clone(),
        args: call.arguments.clone(),
        level: level.as_str().to_string(),
    })?;
    let outcome = run_tool(registry, call, open_conn);
    log::info!(
        "[agent:{turn_id}] 工具 {} {}",
        call.name,
        if outcome.ok { "执行成功" } else { "执行失败" }
    );
    if outcome.ok {
        art.tool_count += 1;
        // 记录实际改动的数据域，回合结束由调用方广播刷新事件
        if let Some(domain) = super::tools::domain_of(&call.name) {
            art.domains.insert(domain.to_string());
        }
    }
    push_tool_result(turn_id, call, &outcome, if outcome.ok { "ok" } else { "error" }, emit, art, convo)
}

/// 解析参数并执行；任何一层失败都变成 err 形态的工具结果（模型自行纠错，§4）
fn run_tool(
    registry: &ToolRegistry,
    call: &ToolCallRequest,
    open_conn: &dyn Fn() -> std::result::Result<rusqlite::Connection, String>,
) -> ToolOutcome {
    let tool = match registry.get(&call.name) {
        Some(t) => t,
        None => {
            return ToolOutcome::err(
                format!("未知工具：{}", call.name),
                format!("工具 {} 不存在，请从可用工具列表中选择", call.name),
            )
        }
    };
    // 空参数串（无参工具常见）按 {} 处理
    let args: Value = if call.arguments.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_str(&call.arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolOutcome::err(
                    format!("参数不是合法 JSON：{e}"),
                    format!("原始参数：{}", call.arguments),
                )
            }
        }
    };
    // 循环跨 await 不能持 DbState 锁：工具执行用调用方注入的短连接工厂
    match open_conn() {
        Ok(conn) => tool.execute(&args, ToolCtx { conn: &conn }),
        Err(e) => ToolOutcome::err(format!("打开本地数据库失败：{e}"), String::new()),
    }
}

/// 把工具结果发事件 + 追加进 convo / pending（tool 消息与调用成对，§4② 铁律）
#[allow(clippy::too_many_arguments)]
fn push_tool_result(
    turn_id: &str,
    call: &ToolCallRequest,
    outcome: &ToolOutcome,
    status: &str,
    emit: &mut dyn FnMut(ChatStreamEvent) -> Result<(), String>,
    art: &mut TurnArtifacts,
    convo: &mut Vec<ChatMessage>,
) -> Result<(), String> {
    // §5 结果长度控制：事件给 UI 全文（可展开），回填模型做截断
    let full = if outcome.detail.trim().is_empty() {
        outcome.summary.clone()
    } else {
        outcome.detail.clone()
    };
    let for_model = crate::agent::tools::cap_for_model(&full);
    let msg = PendingMessage {
        role: "tool".into(),
        content: for_model,
        tool_calls: None,
        tool_call_id: Some(call.id.clone()),
        tool_name: Some(call.name.clone()),
        tool_status: Some(status.to_string()),
    };
    // 先落 pending/convo 再发事件：assistant.tool_calls ↔ tool 结果的成对关系是落库
    // 铁律，事件送达失败不能让已执行的结果从历史里消失
    art.pending.push(msg.clone());
    convo.push(wire_msg(
        "tool",
        msg.content,
        turn_id,
        None,
        msg.tool_call_id,
        msg.tool_name,
        msg.tool_status,
    ));
    emit(ChatStreamEvent::ToolResult {
        call_id: call.id.clone(),
        ok: outcome.ok,
        summary: outcome.summary.clone(),
        detail: full,
    })
}

/// 循环内构建内存 ChatMessage（payload 只消费 role/content/tool 字段，
/// id/created_at 是落库后才有意义的占位）
#[allow(clippy::too_many_arguments)]
fn wire_msg(
    role: &str,
    content: String,
    turn_id: &str,
    tool_calls: Option<String>,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_status: Option<String>,
) -> ChatMessage {
    ChatMessage {
        id: 0,
        session_id: 0,
        turn_id: Some(turn_id.to_string()),
        role: role.to_string(),
        content,
        tool_calls,
        tool_call_id,
        tool_name,
        tool_status,
        created_at: String::new(),
    }
}

/// tool_calls 序列化为 OpenAI 协议形态的 JSON 数组文本（落库与回传同一形态）
fn tool_calls_json(calls: &[ToolCallRequest]) -> Result<String, String> {
    let arr: Vec<Value> = calls
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments }
            })
        })
        .collect();
    serde_json::to_string(&arr).map_err(|e| format!("tool_calls 序列化失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::AgentTool;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    const SID: i64 = 1;
    const TURN: &str = "turn_test";

    fn call(id: &str, name: &str, args: &str) -> ToolCallRequest {
        ToolCallRequest { id: id.into(), name: name.into(), arguments: args.into() }
    }

    fn reply(content: &str) -> RoundOutput {
        RoundOutput { content: content.into(), usage: ChatUsage::default(), tool_calls: vec![] }
    }

    fn round_with(content: &str, calls: Vec<ToolCallRequest>) -> RoundOutput {
        RoundOutput { content: content.into(), usage: ChatUsage::default(), tool_calls: calls }
    }

    /// 脚本化模型：按序返回预置输出，并记录每次收到的 messages / tools 供断言
    struct Scripted {
        outs: std::vec::IntoIter<RoundOutput>,
        pub seen: Arc<std::sync::Mutex<Vec<(usize, Option<usize>)>>>, // (消息数, tools 数)
    }

    impl Scripted {
        fn new(outs: Vec<RoundOutput>) -> (Self, Arc<std::sync::Mutex<Vec<(usize, Option<usize>)>>>) {
            let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
            (Self { outs: outs.into_iter(), seen: seen.clone() }, seen)
        }
    }

    impl ModelInvoke for Scripted {
        fn invoke<'a>(
            &mut self,
            msgs: Vec<ChatMessage>,
            tools: Option<Vec<Value>>,
            cb: ChunkCb<'a>,
        ) -> InvokeFuture<'a> {
            let out = self.outs.next().expect("脚本输出用尽");
            self.seen.lock().unwrap().push((msgs.len(), tools.as_ref().map(Vec::len)));
            Box::pin(async move {
                let _ = cb(out.content.clone());
                Ok(out)
            })
        }
    }

    /// 带执行计数的工具（不碰 DB，连接工厂给内存库仅为走通 open 流程）
    struct CountingTool {
        level: Level,
        execs: Arc<AtomicUsize>,
    }

    impl AgentTool for CountingTool {
        fn name(&self) -> &'static str {
            "count_tool"
        }
        fn description(&self) -> &'static str {
            "测试工具"
        }
        fn level(&self) -> Level {
            self.level
        }
        fn parameters(&self) -> Value {
            json!({"type":"object","properties":{}})
        }
        fn execute(&self, _args: &Value, _ctx: ToolCtx) -> ToolOutcome {
            self.execs.fetch_add(1, Ordering::SeqCst);
            ToolOutcome::ok("执行成功", "结果内容")
        }
    }

    fn mem_conn() -> std::result::Result<rusqlite::Connection, String> {
        crate::db::init_in_memory().map_err(|e| e.to_string())
    }

    fn reg_with(level: Level, execs: Arc<AtomicUsize>) -> ToolRegistry {
        let mut r = ToolRegistry::empty();
        r.register(Box::new(CountingTool { level, execs }));
        r
    }

    /// 收集事件供断言
    fn collector() -> (impl FnMut(ChatStreamEvent) -> Result<(), String>, Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let s = seen.clone();
        (move |e: ChatStreamEvent| {
            let tag = match e {
                ChatStreamEvent::Chunk { .. } => "chunk".to_string(),
                ChatStreamEvent::ToolCall { name, .. } => format!("call:{name}"),
                ChatStreamEvent::ToolResult { ok, summary, .. } => format!("result:{ok}:{summary}"),
                ChatStreamEvent::ApprovalRequest { name, .. } => format!("ask:{name}"),
                _ => "other".to_string(),
            };
            s.lock().unwrap().push(tag);
            Ok(())
        }, seen)
    }

    struct Nop;
    impl Approver for Nop {
        fn approve(&mut self, _ask: ApprovalAsk) -> ApproveFuture {
            Box::pin(async { ApproveAnswer::Denied })
        }
    }

    #[tokio::test]
    async fn direct_reply_finishes_in_one_round() {
        let (mut inv, seen) = Scripted::new(vec![reply("你好呀")]);
        let (emit, events) = collector();
        let mut grants = SessionGrants::default();
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false,
            &ToolRegistry::empty(), "sys", vec![wire_msg("user", "在吗".into(), TURN, None, None, None, None,)],
            MAX_ROUNDS, &mut inv, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .unwrap();
        assert_eq!(art.rounds, 1);
        assert_eq!(art.final_reply.as_deref(), Some("你好呀"));
        assert_eq!(art.pending.len(), 1);
        assert_eq!(art.pending[0].role, "assistant");
        assert!(art.usage.input == 0);
        // 消息数 = system + user
        assert_eq!(seen.lock().unwrap()[0], (2, None));
        assert_eq!(*events.lock().unwrap(), vec!["chunk".to_string()]);
    }

    #[tokio::test]
    async fn tool_round_then_final_reply() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Read, execs.clone());
        let (mut inv, _) = Scripted::new(vec![
            round_with("查一下", vec![call("c1", "count_tool", "{}")]),
            reply("查完了，结果是结果内容"),
        ]);
        let (emit, events) = collector();
        let mut grants = SessionGrants::default();
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys",
            vec![], MAX_ROUNDS, &mut inv, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .unwrap();
        assert_eq!(execs.load(Ordering::SeqCst), 1);
        assert_eq!(art.tool_count, 1);
        assert_eq!(art.rounds, 2);
        // pending: assistant(tool_calls) + tool + 最终 assistant
        assert_eq!(art.pending.len(), 3);
        assert_eq!(art.pending[0].role, "assistant");
        let tc: Value = serde_json::from_str(art.pending[0].tool_calls.as_deref().unwrap()).unwrap();
        assert_eq!(tc[0]["function"]["name"], "count_tool");
        assert_eq!(art.pending[1].role, "tool");
        assert_eq!(art.pending[1].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(art.pending[1].tool_status.as_deref(), Some("ok"));
        assert_eq!(art.pending[1].content, "结果内容");
        assert_eq!(art.final_reply.as_deref(), Some("查完了，结果是结果内容"));
        let ev = events.lock().unwrap();
        assert!(ev.iter().any(|t| t == "call:count_tool"));
        assert!(ev.iter().any(|t| t == "result:true:执行成功"));
    }

    #[tokio::test]
    async fn ask_mode_approval_once_and_always() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Write, execs.clone());
        struct OnceAlways(Vec<ApproveAnswer>);
        impl Approver for OnceAlways {
            fn approve(&mut self, _ask: ApprovalAsk) -> ApproveFuture {
                let a = self.0.remove(0);
                Box::pin(async move { a })
            }
        }
        let mut appr = OnceAlways(vec![ApproveAnswer::Once, ApproveAnswer::Always]);
        let (emit, events) = collector();
        let mut grants = SessionGrants::default();
        // 三轮脚本：调用 → 调用（此时已会话授权，直跑）→ 最终回复
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys", vec![],
            MAX_ROUNDS, &mut ScriptedWrap::calls(3), &mut appr, emit, || false, mem_conn,
        )
        .await
        .unwrap();
        // 两次调用都执行了：第一次 Once，第二次命中会话级授权直跑
        assert_eq!(execs.load(Ordering::SeqCst), 2);
        assert!(grants.is_granted(SID, "count_tool"));
        assert_eq!(art.tool_count, 2);
        let ev = events.lock().unwrap();
        // 两轮各问一次（Once 不授权；Always 才写会话授权），第三轮已出最终回复无需再问
        assert_eq!(ev.iter().filter(|t| t.starts_with("ask:")).count(), 2);
        // Always 已写入会话授权，后续同会话同工具不再询问
        assert!(grants.is_granted(SID, "count_tool"));
    }

    /// 连续 N 轮都发起 count_tool 调用的脚本模型
    struct ScriptedWrap {
        remaining: usize,
    }
    impl ScriptedWrap {
        fn calls(n: usize) -> Self {
            Self { remaining: n }
        }
    }
    impl ModelInvoke for ScriptedWrap {
        fn invoke<'a>(
            &mut self,
            _msgs: Vec<ChatMessage>,
            _tools: Option<Vec<Value>>,
            _cb: ChunkCb<'a>,
        ) -> InvokeFuture<'a> {
            if self.remaining > 1 {
                self.remaining -= 1;
                let out = round_with("", vec![call("cx", "count_tool", "{}")]);
                Box::pin(async move { Ok(out) })
            } else {
                Box::pin(async { Ok(reply("完成")) })
            }
        }
    }

    #[tokio::test]
    async fn ask_mode_denied_feeds_result_back() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Write, execs.clone());
        let (mut inv, seen) = Scripted::new(vec![
            round_with("", vec![call("c1", "count_tool", "不是json")]),
            reply("好的，那我不做了"),
        ]);
        let (emit, _) = collector();
        let mut grants = SessionGrants::default();
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys", vec![],
            MAX_ROUNDS, &mut inv, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .unwrap();
        // 被拒：没有执行；工具结果（拒绝说明）回给了模型
        assert_eq!(execs.load(Ordering::SeqCst), 0);
        assert_eq!(art.pending[1].tool_status.as_deref(), Some("denied"));
        // 第二轮模型看到的最后一条消息是拒绝说明：system + assistant(tool_calls) + tool
        let (n, _) = seen.lock().unwrap()[1];
        assert_eq!(n, 3);
        assert_eq!(art.final_reply.as_deref(), Some("好的，那我不做了"));
    }

    #[tokio::test]
    async fn readonly_fallback_denies_write_calls() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Write, execs.clone());
        // ReadOnly 档 tools 列表里根本没有写工具；这里模拟模型硬调的兜底路径
        let (mut inv, _) = Scripted::new(vec![
            round_with("", vec![call("c1", "count_tool", "{}")]),
            reply("明白，只读模式"),
        ]);
        let (emit, events) = collector();
        let mut grants = SessionGrants::default();
        let art = run_agent_turn(
            SID, TURN, PermissionMode::ReadOnly, &mut grants, false, &registry, "sys", vec![],
            MAX_ROUNDS, &mut inv, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .unwrap();
        assert_eq!(execs.load(Ordering::SeqCst), 0);
        assert_eq!(art.pending[1].tool_status.as_deref(), Some("denied"));
        assert_eq!(art.tool_count, 0);
        assert!(events.lock().unwrap().iter().any(|t| t.contains("仅可查看")));
    }

    #[tokio::test]
    async fn rounds_cap_forces_no_tools_then_summary() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Read, execs.clone());
        // 两轮上限：第一轮带工具；第二轮（末轮）不下发工具。模型仍想要工具 →
        // 兜底拒绝后循环耗尽 → 合成收尾提示
        struct AlwaysTools;
        impl ModelInvoke for AlwaysTools {
            fn invoke<'a>(
&mut self,
                _msgs: Vec<ChatMessage>,
                _tools: Option<Vec<Value>>,
                _cb: ChunkCb<'a>,
            ) -> InvokeFuture<'a> {
                Box::pin(async { Ok(round_with("", vec![call("c", "count_tool", "{}")])) })
            }
        }
        struct Recording {
            inner: AlwaysTools,
            seen: Arc<std::sync::Mutex<Vec<Option<usize>>>>,
        }
        impl ModelInvoke for Recording {
            fn invoke<'a>(
&mut self,
                msgs: Vec<ChatMessage>,
                tools: Option<Vec<Value>>,
                cb: ChunkCb<'a>,
            ) -> InvokeFuture<'a> {
                self.seen.lock().unwrap().push(tools.as_ref().map(Vec::len));
                self.inner.invoke(msgs, tools, cb)
            }
        }
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut inv_box: Box<dyn ModelInvoke> =
            Box::new(Recording { inner: AlwaysTools, seen: seen.clone() });
        let (emit, _) = collector();
        let mut grants = SessionGrants::default();
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys", vec![],
            2, &mut *inv_box, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .unwrap();
        let s = seen.lock().unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], Some(1)); // 第一轮下发了 1 个工具
        assert_eq!(s[1], None); // 末轮强制不下发
        assert_eq!(art.rounds, 2);
        assert!(art.final_reply.unwrap().contains("上限"));
    }

    #[tokio::test]
    async fn interrupt_stops_loop_and_keeps_executed() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Read, execs.clone());
        let flag = Arc::new(AtomicBool::new(false));
        let flag2 = flag.clone();
        // 第二次模型调用期间置位中断旗标：第一轮工具已执行（不回滚），随后循环停下
        struct Toggle(Arc<AtomicBool>, usize);
        impl ModelInvoke for Toggle {
            fn invoke<'a>(
&mut self,
                _msgs: Vec<ChatMessage>,
                _tools: Option<Vec<Value>>,
                _cb: ChunkCb<'a>,
            ) -> InvokeFuture<'a> {
                self.1 += 1;
                if self.1 >= 2 {
                    self.0.store(true, Ordering::SeqCst);
                }
                Box::pin(async { Ok(round_with("", vec![call("c", "count_tool", "{}")])) })
            }
        }
        let mut inv: Box<dyn ModelInvoke> = Box::new(Toggle(flag2, 0));
        let (emit, _) = collector();
        let mut grants = SessionGrants::default();
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys", vec![],
            MAX_ROUNDS, inv.as_mut(), &mut Nop, emit, || flag.load(Ordering::SeqCst), mem_conn,
        )
        .await
        .unwrap();
        // 第一轮的工具已执行（不回滚），随后循环停下；无最终回复（前端按中断呈现）
        assert!(art.interrupted);
        assert_eq!(execs.load(Ordering::SeqCst), 1);
        assert!(art.final_reply.is_none());
        // 0.9.0 修复回归：中断后未执行的调用必须补 cancelled 结果——
        // assistant(tool_calls) 与 tool 消息成对落库，否则下轮请求被上游 400 拒绝
        let asst_idx = art
            .pending
            .iter()
            .rposition(|m| m.role == "assistant" && m.tool_calls.is_some())
            .expect("中断发生在工具轮，应有带 tool_calls 的 assistant 消息");
        let calls: Vec<Value> =
            serde_json::from_str(art.pending[asst_idx].tool_calls.as_deref().unwrap()).unwrap();
        let results: Vec<&PendingMessage> = art.pending.as_slice()[asst_idx + 1..]
            .iter()
            .filter(|m| m.role == "tool")
            .collect();
        assert_eq!(results.len(), calls.len(), "每个 tool_call 都要有对应 tool 结果");
        for (c, r) in calls.iter().zip(&results) {
            assert_eq!(r.tool_call_id.as_deref(), Some(c["id"].as_str().unwrap()));
            assert_eq!(r.tool_status.as_deref(), Some("cancelled"));
        }
    }

    /// 0.9.0 审查优化回归：前端通道已死（emit 失败）应立即终止回合，
    /// 不再对死通道空转烧完 MAX_ROUNDS 轮模型调用
    #[tokio::test]
    async fn dead_channel_aborts_turn_instead_of_spinning() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Read, execs.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let c2 = calls.clone();
        struct CountingInvoke(Arc<AtomicUsize>);
        impl ModelInvoke for CountingInvoke {
            fn invoke<'a>(
                &mut self,
                _msgs: Vec<ChatMessage>,
                _tools: Option<Vec<Value>>,
                _cb: ChunkCb<'a>,
            ) -> InvokeFuture<'a> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(round_with("", vec![call("c", "count_tool", "{}")])) })
            }
        }
        let mut inv: Box<dyn ModelInvoke> = Box::new(CountingInvoke(c2));
        // 通道恒失败：模拟前端窗口已销毁
        let mut emit = |_e: ChatStreamEvent| -> Result<(), String> { Err("channel closed".into()) };
        let mut grants = SessionGrants::default();
        let res = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys", vec![],
            MAX_ROUNDS, inv.as_mut(), &mut Nop, &mut emit, || false, mem_conn,
        )
        .await;
        assert!(res.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "首个工具事件发送失败即终止，不得继续调用模型");
    }

    #[tokio::test]
    async fn invoke_error_propagates() {
        struct Boom;
        impl ModelInvoke for Boom {
            fn invoke<'a>(
                &mut self,
                _msgs: Vec<ChatMessage>,
                _tools: Option<Vec<Value>>,
                _cb: ChunkCb<'a>,
            ) -> InvokeFuture<'a> {
                Box::pin(async { Err("上游 500".into()) })
            }
        }
        let (emit, _) = collector();
        let mut grants = SessionGrants::default();
        let mut boom = Boom;
        let err = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &ToolRegistry::empty(), "sys",
            vec![], MAX_ROUNDS, &mut boom, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .unwrap_err();
        assert_eq!(err.0, "上游 500");
    }

    /// 错误路径不得丢掉 domains：工具执行成功之后上游才断掉时，Err 必须带回工具
    /// 改过的数据域——调用方要靠它在 Err 分支照常广播刷新事件（待办已建、界面得知道）
    #[tokio::test]
    async fn invoke_error_keeps_domains() {
        // 名字进 domain_of 映射（→ "todos"）；档位 Read 免走审批，直通执行
        struct TodoTool;
        impl AgentTool for TodoTool {
            fn name(&self) -> &'static str {
                "create_todo"
            }
            fn description(&self) -> &'static str {
                "测试写入工具"
            }
            fn level(&self) -> Level {
                Level::Read
            }
            fn parameters(&self) -> Value {
                json!({"type":"object","properties":{}})
            }
            fn execute(&self, _args: &Value, _ctx: ToolCtx) -> ToolOutcome {
                ToolOutcome::ok("已创建", "已创建待办")
            }
        }
        // 第一轮正常返回工具调用，第二轮模型调用失败：模拟「工具已执行、上游断连」
        struct ToolThenBoom(usize);
        impl ModelInvoke for ToolThenBoom {
            fn invoke<'a>(
                &mut self,
                _msgs: Vec<ChatMessage>,
                _tools: Option<Vec<Value>>,
                _cb: ChunkCb<'a>,
            ) -> InvokeFuture<'a> {
                self.0 += 1;
                let n = self.0;
                Box::pin(async move {
                    if n == 1 {
                        Ok(round_with("", vec![call("c1", "create_todo", "{}")]))
                    } else {
                        Err("上游 500".into())
                    }
                })
            }
        }
        let mut registry = ToolRegistry::empty();
        registry.register(Box::new(TodoTool));
        let (emit, _) = collector();
        let mut grants = SessionGrants::default();
        let mut inv = ToolThenBoom(0);
        let err = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys", vec![],
            MAX_ROUNDS, &mut inv, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .unwrap_err();
        assert_eq!(err.0, "上游 500");
        assert!(
            err.1.domains.contains("todos"),
            "Err 必须带回工具已改过的数据域，实际：{:?}",
            err.1.domains
        );
    }

    /// 0.9.0 审查回归：流式中途取消（invoke 以 Err("已中断") 收场）不能再向上抛——
    /// 已执行的工具过程必须留在 pending 落库，与轮间检查点的中断口径一致
    #[tokio::test]
    async fn cancel_during_stream_keeps_executed_artifacts() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Read, execs.clone());
        let cancelled = Arc::new(AtomicBool::new(false));
        let c2 = cancelled.clone();
        // 第一轮正常返回工具调用；第二轮模拟流式被取消掐断（Err + 旗标已置位）
        struct StreamCancelled(usize, Arc<AtomicBool>);
        impl ModelInvoke for StreamCancelled {
            fn invoke<'a>(
                &mut self,
                _msgs: Vec<ChatMessage>,
                _tools: Option<Vec<Value>>,
                _cb: ChunkCb<'a>,
            ) -> InvokeFuture<'a> {
                self.0 += 1;
                let n = self.0;
                let flag = self.1.clone();
                Box::pin(async move {
                    if n >= 2 {
                        flag.store(true, Ordering::SeqCst);
                        return Err("已中断".into());
                    }
                    Ok(round_with("", vec![call("c1", "count_tool", "{}")]))
                })
            }
        }
        let mut inv: Box<dyn ModelInvoke> = Box::new(StreamCancelled(0, c2));
        let (emit, _) = collector();
        let mut grants = SessionGrants::default();
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys", vec![],
            MAX_ROUNDS, inv.as_mut(), &mut Nop, emit,
            || cancelled.load(Ordering::SeqCst), mem_conn,
        )
        .await
        .expect("取消不应走 Err 路径丢掉已执行过程");
        assert!(art.interrupted);
        assert_eq!(execs.load(Ordering::SeqCst), 1, "第一轮的工具已执行");
        // 已执行的工具过程完整成对留在 pending：assistant(tool_calls) + tool 结果
        assert!(art.pending.iter().any(|m| m.role == "assistant" && m.tool_calls.is_some()));
        assert!(art.pending.iter().any(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("c1")));
        assert!(art.final_reply.is_none());
    }


    /// 0.9.2 回归：模型返回空正文（上游把错误塞进 SSE 错误帧时的表现）不能当正常收尾，
    /// 否则会落一条 0 字符的 assistant 消息——界面是一个没文字也没报错的空气泡。
    /// 整轮毫无产出时按真错误上报，让前端渲染错误气泡。
    #[tokio::test]
    async fn empty_reply_without_tools_is_an_error() {
        let (mut inv, _) = Scripted::new(vec![reply("")]);
        let (emit, _) = collector();
        let mut grants = SessionGrants::default();
        let res = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &ToolRegistry::empty(), "sys",
            vec![], MAX_ROUNDS, &mut inv, &mut Nop, emit, || false, mem_conn,
        )
        .await;
        let (e, art) = res.expect_err("空回复必须报错，不得当作成功");
        assert!(e.contains("没有返回任何内容"), "错误文案要可读：{e}");
        assert!(art.pending.is_empty(), "没产出就不得留下空 assistant 消息");
    }

    /// 空回复出现在**工具轮之后**时不能向上抛：assistant(tool_calls) 与 tool 结果
    /// 必须成对落库（否则下次请求被上游 400）。此时合成收尾说明，保住已执行过程。
    #[tokio::test]
    async fn empty_reply_after_tools_keeps_process_and_explains() {
        let execs = Arc::new(AtomicUsize::new(0));
        let registry = reg_with(Level::Read, execs.clone());
        let (mut inv, _) = Scripted::new(vec![
            round_with("", vec![call("c1", "count_tool", "{}")]),
            reply(""),
        ]);
        let (emit, _) = collector();
        let mut grants = SessionGrants::default();
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &registry, "sys", vec![],
            MAX_ROUNDS, &mut inv, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .expect("工具轮之后的空回复不能走 Err，否则工具过程落不了库");
        assert_eq!(execs.load(Ordering::SeqCst), 1);
        // assistant(tool_calls) + tool + 合成的收尾 assistant
        assert_eq!(art.pending.len(), 3);
        assert_eq!(art.pending[1].role, "tool");
        let last = art.pending.last().unwrap();
        assert_eq!(last.role, "assistant");
        assert!(last.content.contains("没有返回任何内容"), "收尾说明要交代原因：{}", last.content);
        assert!(art.final_reply.is_some());
    }

    #[tokio::test]
    async fn unknown_tool_and_bad_args_become_error_results() {
        // 注册表为空：模型调了一个不存在的工具
        struct Phantom;
        impl ModelInvoke for Phantom {
            fn invoke<'a>(
&mut self,
                _msgs: Vec<ChatMessage>,
                _tools: Option<Vec<Value>>,
                _cb: ChunkCb<'a>,
            ) -> InvokeFuture<'a> {
                Box::pin(async { Ok(round_with("", vec![call("c1", "ghost_tool", "{bad json")])) })
            }
        }
        let (emit, events) = collector();
        let mut grants = SessionGrants::default();
        let mut phantom = Phantom;
        let art = run_agent_turn(
            SID, TURN, PermissionMode::Ask, &mut grants, false, &ToolRegistry::empty(), "sys",
            vec![], MAX_ROUNDS, &mut phantom, &mut Nop, emit, || false, mem_conn,
        )
        .await
        .unwrap();
        // 未知工具优先于参数错误被报告
        assert!(!art.pending[1].tool_status.as_deref().unwrap().is_empty());
        assert_eq!(art.pending[1].tool_status.as_deref(), Some("error"));
        assert!(events.lock().unwrap().iter().any(|t| t.contains("未知工具")));
    }
}

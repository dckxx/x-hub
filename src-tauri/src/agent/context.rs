use crate::models::ChatMessage;
use crate::repo;
use rusqlite::{Connection, Result};

/// 上下文窗口保留的完整 turn 数（计划 §4①：8 起步，待实测调整）
pub const CONTEXT_MAX_TURNS: usize = 8;
/// 单次从库里读取的行数上限（0.9.0 审查优化：此前全量加载整个会话再分组，
/// 长会话（数千行）每次发消息都全表读出；8 个 turn 的窗口语义不需要更早的行）
pub const CONTEXT_ROW_LIMIT: i64 = 500;

/// 组装发送给模型的上下文：按 turn 截断 + 历史工具消息成对剪枝（计划 §4①②）。
///
/// - **按 turn 截断**：只保留最近 N 个**完整** turn——同一 turn 的消息（user + 中间工具
///   过程 + 最终回复）要么全留、要么全不留，绝不截半个 turn（按条数截断在 agent 场景下
///   会把工具调用与其结果截散，模型直接失去关键上下文）。
/// - **历史工具剪枝**：除最近一个 turn 外，更早 turn 里的「assistant(tool_calls) + 其
///   tool 结果」整组剪掉，只留 user + 最终 assistant 正文——中间过程的信息已体现在该
///   turn 的最终回复里，剪掉省 token。**整组剪**天然满足 §4② 铁律（tool_call_id 与调用
///   成对出现/成对消失），不会被上游判 400。
/// - `turn_id` 为 NULL 的消息（更老版本写入的）按「user 开新组、其余跟随前一组」兜底归组，
///   同样保证整组去留。
pub fn build_context(conn: &Connection, session_id: i64) -> Result<Vec<ChatMessage>> {
    let all = repo::chat::list_recent_messages(conn, session_id, CONTEXT_ROW_LIMIT)?;
    Ok(build_context_from(all))
}

/// 纯函数版（测试与未来缓存复用）：分组 → 截断 → 剪枝。
pub fn build_context_from(all: Vec<ChatMessage>) -> Vec<ChatMessage> {
    // 1) 按 turn 分组（保序）：有 turn_id 用 turn_id；NULL 时 user 开新组、其余跟随当前组
    let mut groups: Vec<Vec<ChatMessage>> = Vec::new();
    let mut current_key = String::new();
    for m in all {
        let key = match m.turn_id.as_deref() {
            Some(t) => t.to_string(),
            None => {
                if m.role == "user" || groups.is_empty() {
                    format!("__orphan_{}", m.id)
                } else {
                    current_key.clone()
                }
            }
        };
        match groups.last_mut() {
            Some(g) if current_key == key => g.push(m),
            _ => {
                current_key = key;
                groups.push(vec![m]);
            }
        }
    }

    // 行数上限截在回合中间时，头部那组不带 user 起始（从 assistant/tool 行开始）——
    // 残组整组丢弃：它的孤儿 tool 消息发出去会被上游 400（完整回合必以 user 开头）
    if groups.first().is_some_and(|g| g.first().is_some_and(|m| m.role != "user")) {
        groups.remove(0);
    }

    // 2) 截断：只留最近 N 个完整 turn
    let keep_from = groups.len().saturating_sub(CONTEXT_MAX_TURNS);

    // 3) 剪枝：除最后一组（最近的 turn）外，把中间工具过程整组剪掉
    let mut out: Vec<ChatMessage> = Vec::new();
    for (i, msgs) in groups.iter().enumerate().skip(keep_from) {
        if i == groups.len() - 1 {
            out.extend(msgs.iter().cloned());
        } else {
            // tool 消息与携带 tool_calls 的 assistant 一起消失——成对铁律
            out.extend(
                msgs.iter()
                    .filter(|m| m.role != "tool" && m.tool_calls.is_none())
                    .cloned(),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: i64, turn: Option<&str>, role: &str, content: &str, tool_calls: Option<&str>) -> ChatMessage {
        ChatMessage {
            id,
            session_id: 1,
            turn_id: turn.map(|s| s.to_string()),
            role: role.into(),
            content: content.into(),
            tool_calls: tool_calls.map(|s| s.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn keeps_whole_turns_only() {
        // 3 个 turn 未超窗口（8）：原样全留；截断行为由 window_never_splits_a_turn 覆盖
        let all = vec![
            msg(1, Some("t1"), "user", "问一", None),
            msg(2, Some("t1"), "assistant", "答一", None),
            msg(3, Some("t2"), "user", "问二", None),
            msg(4, Some("t2"), "assistant", "答二", None),
            msg(5, Some("t3"), "user", "问三", None),
            msg(6, Some("t3"), "assistant", "答三", None),
        ];
        let ctx = build_context_from(all);
        let ids: Vec<i64> = ctx.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn prunes_tool_pairs_in_older_turns_but_keeps_last_turn_intact() {
        // 历史 turn：user + assistant(tool_calls) + tool + 最终 assistant → 剪掉中间两个；
        // 最近 turn：工具过程全保留
        let all = vec![
            msg(1, Some("t1"), "user", "我的待办", None),
            msg(2, Some("t1"), "assistant", "", Some(r#"[{"id":"c1"}]"#)),
            msg(3, Some("t1"), "tool", "[...]", None),
            msg(4, Some("t1"), "assistant", "你有 3 条待办", None),
            msg(5, Some("t2"), "user", "再查一次", None),
            msg(6, Some("t2"), "assistant", "", Some(r#"[{"id":"c2"}]"#)),
            msg(7, Some("t2"), "tool", "[...]", None),
            msg(8, Some("t2"), "assistant", "还是 3 条", None),
        ];
        let ctx = build_context_from(all);
        let rendered: Vec<String> = ctx
            .iter()
            .map(|m| format!("{}:{}", m.role, m.content))
            .collect();
        // t1 被剪成 user + 最终 assistant；t2 原样保留
        assert_eq!(
            rendered,
            vec![
                "user:我的待办".to_string(),
                "assistant:你有 3 条待办".to_string(),
                "user:再查一次".to_string(),
                "assistant:".to_string(),
                "tool:[...]".to_string(),
                "assistant:还是 3 条".to_string(),
            ]
        );
    }

    #[test]
    fn orphan_null_turn_messages_group_correctly() {
        // 老数据（turn_id NULL）：user 开新组、assistant 跟随；截断/剪枝同样整组生效
        let all = vec![
            msg(1, None, "user", "老问题", None),
            msg(2, None, "assistant", "老回答", None),
            msg(3, Some("t_new"), "user", "新问题", None),
            msg(4, Some("t_new"), "assistant", "新回答", None),
        ];
        let ctx = build_context_from(all);
        assert_eq!(ctx.len(), 4);
        // NULL 组整体保留（未超窗口），且不被并入 t_new
        assert_eq!(ctx[0].content, "老问题");
        assert_eq!(ctx[3].turn_id.as_deref(), Some("t_new"));
    }

    #[test]
    fn window_never_splits_a_turn() {
        // turn 数超过窗口：留 FULL_TURN 个完整 turn，条数可以很多
        let mut all = Vec::new();
        for t in 0..(CONTEXT_MAX_TURNS + 3) {
            let turn = format!("t{t}");
            all.push(msg((t * 4) as i64 + 1, Some(&turn), "user", "问", None));
            all.push(msg((t * 4) as i64 + 2, Some(&turn), "assistant", "", Some("[{}]")));
            all.push(msg((t * 4) as i64 + 3, Some(&turn), "tool", "结果", None));
            all.push(msg((t * 4) as i64 + 4, Some(&turn), "assistant", "答", None));
        }
        let ctx = build_context_from(all);
        // 窗口内 8 个 turn × 每组剪后 2 条（user + 最终答）+ 最后一组 4 条全留
        assert_eq!(ctx.len(), (CONTEXT_MAX_TURNS - 1) * 2 + 4);
        assert_eq!(ctx.first().unwrap().turn_id.as_deref(), Some("t3"));
    }

    /// 0.9.0 审查回归（LIMIT 截断防护）：行数上限截在回合中间时，头部残组（无 user
    /// 起始、从 assistant/tool 行开始）整组丢弃——孤儿 tool 消息发出去会被上游 400
    #[test]
    fn leading_partial_turn_group_is_dropped() {
        let all = vec![
            // 残组：t0 的 user 行被 LIMIT 截掉了，剩 tool 结果行开头
            msg(2, Some("t0"), "assistant", "", Some(r#"[{"id":"c1"}]"#)),
            msg(3, Some("t0"), "tool", "[...]", None),
            msg(4, Some("t0"), "assistant", "答", None),
            // 完整组
            msg(5, Some("t1"), "user", "问", None),
            msg(6, Some("t1"), "assistant", "答", None),
        ];
        let ctx = build_context_from(all);
        let ids: Vec<i64> = ctx.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![5, 6], "残缺的 t0 组应整组丢弃");
        // 头部就是完整组（user 开头）时不受影响
        let all2 = vec![msg(1, Some("t0"), "user", "问", None), msg(2, Some("t1"), "user", "问", None)];
        assert_eq!(build_context_from(all2).len(), 2);
    }
}

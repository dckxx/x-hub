//! Agent 运行时状态（#t44）：审批挂起表、会话级授权、中断旗标。
//!
//! - **审批**：Ask 档发起确认后，oneshot 发送端挂在这里；前端 `respond_agent_approval`
//!   按 call_id 回答。前端关窗/超时未答 → 接收端 Err → 循环按「拒绝」处理（宁严勿宽）。
//! - **会话授权**：见 `approval::SessionGrants`（只存内存，进程重启即失效）。
//! - **中断**：`cancel_agent_turn` 置旗标；循环在轮与轮之间、工具调用之前检查；
//!   流式回调里也检查（直接掐断上游流）。已执行的写操作不回滚（§4）。

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tokio::sync::oneshot;

use super::approval::{ApproveAnswer, SessionGrants};
use super::turn::ApprovalAsk;

/// 进行中回合的登记：中断旗标 + 归属会话（会话删除时按会话批量中断）
struct TurnReg {
    session_id: i64,
    flag: Arc<AtomicBool>,
}

pub struct AgentState {
    /// 「每次都问」档的会话级授权（跨 turn 存续，会话删除时清理）。
    /// **同步锁**：只在快照/合并的一瞬间持有，绝不跨 await——Ask 档等用户确认
    /// 可能无限期，持锁跨 await 会让其它回合全部卡死在取锁上
    /// （0.9.0 测试包实测「一直卡在思考中」的根因）。
    pub grants: std::sync::Mutex<SessionGrants>,
    /// 等待前端答复的审批（(turn_id, call_id) → (审批内容, 发送端)）；答复后即摘除。
    /// 带完整审批内容：回合收尾只清**本回合**的挂起审批——回合并发化后（不同会话
    /// 同时跑回合），清别的回合的确认会把对方正在等的答复误判成「拒绝」；
    /// `pending_approvals` 按会话查询还原 awaiting 卡片（0.9.0）。
    /// 键带 turn_id：不少兼容后端每请求都产 `call_0` 这类跨请求复用的 id，
    /// 并发回合撞车时裸 call_id 会互相顶掉（0.9.0 审查修复）。
    approvals: Mutex<HashMap<(String, String), (ApprovalAsk, oneshot::Sender<ApproveAnswer>)>>,
    /// 进行中 turn 的登记（turn_id → 旗标 + 归属会话）
    cancels: Mutex<HashMap<String, TurnReg>>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentState {
    pub fn new() -> Self {
        AgentState {
            grants: std::sync::Mutex::new(SessionGrants::default()),
            approvals: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
        }
    }

    /// 取会话授权快照（回合开始时调用；回合在自己的副本上判定与记录授权）
    pub fn grants_snapshot(&self) -> SessionGrants {
        self.grants.lock().expect("grants 锁").clone()
    }

    /// 把回合期间新产生的「本会话允许」并回全局（回合结束时调用；取并集防互相覆盖）
    pub fn grants_merge(&self, snapshot: &SessionGrants) {
        self.grants.lock().expect("grants 锁").merge(snapshot);
    }

    /// turn 开始：登记中断旗标与归属会话
    pub fn register_turn(&self, session_id: i64, turn_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.cancels
            .lock()
            .expect("cancels 锁")
            .insert(turn_id.to_string(), TurnReg { session_id, flag: flag.clone() });
        flag
    }

    /// turn 结束：清旗标与本回合的残留审批（正常收尾/中断/出错都要走）
    pub fn finish_turn(&self, turn_id: &str) {
        log::info!("[agent] 回合结束: {turn_id}");
        let mut cancels = self.cancels.lock().expect("cancels 锁");
        cancels.remove(turn_id);
        drop(cancels);
        // 本回合残留审批的发送端被丢弃 → 接收端 Err → 循环若还在等，按拒绝处理；
        // 其它并发回合的挂起审批不受影响（0.9.0 修复：原先 clear() 会误伤）
        self.approvals
            .lock()
            .expect("approvals 锁")
            .retain(|_, (a, _)| a.turn_id != turn_id);
    }

    /// 用户请求中断：置旗标（流式回调与循环检查点都会看到）
    pub fn cancel_turn(&self, turn_id: &str) {
        if let Some(reg) = self.cancels.lock().expect("cancels 锁").get(turn_id) {
            reg.flag.store(true, Ordering::Relaxed);
        }
    }

    /// 会话删除时中断其全部进行中回合：不取消的话回合会继续烧模型调用，
    /// 收尾写入也会因会话已删（外键级联清了消息）而报错
    pub fn cancel_session_turns(&self, session_id: i64) {
        let regs = self.cancels.lock().expect("cancels 锁");
        let hits: Vec<&TurnReg> = regs.values().filter(|r| r.session_id == session_id).collect();
        if !hits.is_empty() {
            log::info!("[agent] 会话 {session_id} 删除，中断其 {} 个进行中回合", hits.len());
        }
        for reg in hits {
            reg.flag.store(true, Ordering::Relaxed);
        }
    }

    /// 发起一次审批：登记审批内容与发送端，等待前端经 `respond_agent_approval` 答复
    pub fn open_approval(&self, ask: ApprovalAsk, tx: oneshot::Sender<ApproveAnswer>) {
        // 同 (turn_id, call_id) 重复审批不该发生；旧发送端被顶掉 → 旧接收端 Err → 按「拒绝」
        log::info!("[agent] 审批挂起: {} {}", ask.turn_id, ask.call_id);
        let key = (ask.turn_id.clone(), ask.call_id.clone());
        self.approvals
            .lock()
            .expect("approvals 锁")
            .insert(key, (ask, tx));
    }

    /// 某会话当前仍在等待确认的审批快照（面板重开/切回会话时还原 awaiting 卡片）。
    /// 只读：不摘除发送端，正在等待答复的循环不受影响；答复仍走 `answer_approval`。
    pub fn pending_approvals(&self, session_id: i64) -> Vec<ApprovalAsk> {
        self.approvals
            .lock()
            .expect("approvals 锁")
            .values()
            .filter(|(a, _)| a.session_id == session_id)
            .map(|(a, _)| a.clone())
            .collect()
    }

    /// 前端回答审批：true=已送达等待中的循环。
    /// 精确命中 (turn_id, call_id)；turn_id 缺失（空串）时仅当该 call_id **唯一**挂起
    /// 才代答——多条同 id 并发挂起时不猜对象，让调用方收到 false（前端显示「已失效」）
    pub fn answer_approval(&self, turn_id: &str, call_id: &str, answer: ApproveAnswer) -> bool {
        let mut map = self.approvals.lock().expect("approvals 锁");
        let key = if !turn_id.is_empty() {
            Some((turn_id.to_string(), call_id.to_string()))
        } else {
            let matches: Vec<(String, String)> =
                map.keys().filter(|(_, c)| c == call_id).cloned().collect();
            if matches.len() == 1 { matches.into_iter().next() } else { None }
        };
        let hit = match key.and_then(|k| map.remove(&k)) {
            Some((_ask, tx)) => tx.send(answer).is_ok(),
            None => false,
        };
        drop(map);
        // 0.9.0 排查日志：命中/未命中（确认不存在或已失效）都留痕
        log::info!("[agent] 审批答复 {turn_id}/{call_id}: {}", if hit { "已送达" } else { "未命中" });
        hit
    }

    /// 会话删除/清空时清掉其会话级授权（防已授权工具在残留语义里继续直跑）
    pub fn clear_session(&self, session_id: i64) {
        self.grants.lock().expect("grants 锁").clear_session(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(session_id: i64, turn_id: &str, call_id: &str) -> ApprovalAsk {
        ApprovalAsk {
            session_id,
            turn_id: turn_id.into(),
            call_id: call_id.into(),
            name: "count_tool".into(),
            args: "{}".into(),
            level: "write",
        }
    }

    #[tokio::test]
    async fn approval_roundtrip_and_missing() {
        let st = AgentState::new();
        let (tx, rx) = oneshot::channel();
        st.open_approval(ask(1, "t1", "call_1"), tx);
        assert!(st.answer_approval("t1", "call_1", ApproveAnswer::Always));
        assert_eq!(rx.await.unwrap(), ApproveAnswer::Always);
        // 未知 call_id / 重复回答
        assert!(!st.answer_approval("t1", "call_1", ApproveAnswer::Once));
    }

    /// 0.9.0 审查回归锁：兼容后端会跨请求复用 `call_0` 这类 id——并发回合同 id
    /// 挂起时互不干扰，答复按 (turn_id, call_id) 精确路由
    #[tokio::test]
    async fn same_call_id_in_concurrent_turns_routes_independently() {
        let st = AgentState::new();
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();
        st.open_approval(ask(1, "t1", "call_0"), tx1);
        st.open_approval(ask(2, "t2", "call_0"), tx2);
        // 答 t2 的那张卡：只送达 t2，t1 不被顶掉
        assert!(st.answer_approval("t2", "call_0", ApproveAnswer::Once));
        assert_eq!(rx2.await.unwrap(), ApproveAnswer::Once);
        assert!(st.answer_approval("t1", "call_0", ApproveAnswer::Always));
        assert_eq!(rx1.await.unwrap(), ApproveAnswer::Always);
        // turn_id 为空且同 call_id 唯一挂起时可代答；多条同 id 时不猜
        let (tx3, rx3) = oneshot::channel();
        st.open_approval(ask(1, "t3", "call_9"), tx3);
        assert!(st.answer_approval("", "call_9", ApproveAnswer::Once));
        assert_eq!(rx3.await.unwrap(), ApproveAnswer::Once);
        let (tx4, _rx4) = oneshot::channel();
        let (tx5, _rx5) = oneshot::channel();
        st.open_approval(ask(1, "t4", "call_7"), tx4);
        st.open_approval(ask(2, "t5", "call_7"), tx5);
        assert!(!st.answer_approval("", "call_7", ApproveAnswer::Once));
    }

    /// 0.9.0：挂起审批按会话查询还原 awaiting 卡片——快照不摘除，答复仍可达
    #[tokio::test]
    async fn pending_approvals_filter_by_session() {
        let st = AgentState::new();
        let (tx1, _rx1) = oneshot::channel();
        let (tx2, _rx2) = oneshot::channel();
        st.open_approval(ask(1, "t1", "call_1"), tx1);
        st.open_approval(ask(2, "t2", "call_2"), tx2);
        let s1 = st.pending_approvals(1);
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].call_id, "call_1");
        assert_eq!(s1[0].session_id, 1);
        assert_eq!(s1[0].turn_id, "t1");
        assert!(st.pending_approvals(3).is_empty());
        // 查询不消费挂起：答复仍能送达等待中的循环
        assert!(st.answer_approval("t1", "call_1", ApproveAnswer::Once));
    }

    #[tokio::test]
    async fn cancel_flag_and_finish_cleanup() {
        let st = AgentState::new();
        let flag = st.register_turn(1, "t1");
        assert!(!flag.load(Ordering::Relaxed));
        st.cancel_turn("t1");
        assert!(flag.load(Ordering::Relaxed));
        // 未知 turn 不炸
        st.cancel_turn("nope");
        st.finish_turn("t1");
        // finish 后旗标登记已清（重新 register 拿的是新旗标）
        let flag2 = st.register_turn(1, "t1");
        assert!(!flag2.load(Ordering::Relaxed));
    }

    /// 0.9.0 审查优化回归：会话删除时中断其全部进行中回合——只影响归属该会话的旗标
    #[tokio::test]
    async fn cancel_session_turns_only_flags_own_session() {
        let st = AgentState::new();
        let f1 = st.register_turn(1, "t1");
        let f2 = st.register_turn(1, "t2");
        let f3 = st.register_turn(2, "t3");
        st.cancel_session_turns(1);
        assert!(f1.load(Ordering::Relaxed), "会话 1 的回合应被中断");
        assert!(f2.load(Ordering::Relaxed));
        assert!(!f3.load(Ordering::Relaxed), "其它会话的回合不受影响");
        // 登记还在（由各回合自己的 finish_turn 清理）
        st.cancel_turn("t1");
    }

    #[tokio::test]
    async fn finish_turn_drops_pending_approvals() {
        let st = AgentState::new();
        let (tx, rx) = oneshot::channel();
        st.open_approval(ask(1, "t1", "call_x"), tx);
        st.finish_turn("t1");
        // 发送端被丢弃 → 接收端 Err（循环侧按拒绝处理）
        assert!(rx.await.is_err());
    }

    /// 0.9.0 修复回归：回合收尾只清本回合的挂起审批——并发回合（如另一会话）
    /// 正在等的确认不能被误杀成「拒绝」
    #[tokio::test]
    async fn finish_turn_spares_other_turns_approvals() {
        let st = AgentState::new();
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();
        st.open_approval(ask(1, "t1", "call_1"), tx1);
        st.open_approval(ask(2, "t2", "call_2"), tx2);
        st.finish_turn("t1");
        assert!(rx1.await.is_err(), "本回合的挂起审批应被丢弃");
        st.answer_approval("t2", "call_2", ApproveAnswer::Once);
        assert_eq!(rx2.await.unwrap(), ApproveAnswer::Once, "其它回合的挂起审批不应受影响");
    }

    #[test]
    fn grants_clear_per_session() {
        let st = AgentState::new();
        st.grants
            .lock()
            .expect("grants 锁")
            .grant(7, "create_todo");
        st.clear_session(7);
        assert!(!st.grants_snapshot().is_granted(7, "create_todo"));
    }

    /// 0.9.0 修复回归：快照-合并口径——回合内新授权并回全局，取并集不互覆
    #[test]
    fn grants_snapshot_merge_union() {
        let st = AgentState::new();
        st.grants.lock().expect("g").grant(1, "create_note");
        let mut snap = st.grants_snapshot();
        assert!(snap.is_granted(1, "create_note"));
        // 回合运行期间另一回合并回了新授权
        st.grants.lock().expect("g").grant(1, "create_todo");
        // 本回合新授权记录在快照上
        snap.grant(1, "write_sticky");
        st.grants_merge(&snap);
        let g = st.grants.lock().expect("g");
        assert!(g.is_granted(1, "create_note"));
        assert!(g.is_granted(1, "create_todo"));
        assert!(g.is_granted(1, "write_sticky"));
    }
}

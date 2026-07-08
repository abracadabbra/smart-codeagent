//! `AppState` —— Phase 3.2 多 session 并行核心。
//!
//! design.md §5 的实现。职责：
//! - **busy 守门**：`chat_active_replies` 按会话分桶，跨会话不互斥
//! - **cancel 判定**：`chat_active_generations` 按会话分桶，cancel 清空该会话所有 generation
//! - **per-session state**：`session_states` 记录每个会话的 `AgentState`
//! - **pending 路由信息**：`pending_approvals` / `pending_ask_users` 跟踪哪个 conv 有 pending
//!   （实际 oneshot::Sender 仍在 `TauriHost`，这里只存 id 集合用于 badge 显示）
//!
//! 照搬 Kivio `state.rs` 的 `chat_active_replies` / `chat_active_generations` 分桶模式，
//! 但砍掉了 Kivio 的"同会话多模型一问多答 fan-out"（Phase 3.2 每会话同时只有一个 run）。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::agent::AgentState;

/// 多 session 并行核心状态。
///
/// 全局单例，通过 `app.manage(Arc::new(AppState::new()))` 注册。
/// 所有字段用 `Mutex<HashMap<conv_id, ...>>` 分桶，跨会话不互斥。
pub struct AppState {
    /// 每会话活跃 run 集合（busy 守门）。
    ///
    /// conv_id → set of run_id。
    /// Phase 3.2 每会话同时只有一个 run_id（`try_reserve_chat_send` 守门）。
    /// 跨会话不互斥（会话 A busy 不影响会话 B）。
    pub chat_active_replies: Mutex<HashMap<String, HashSet<String>>>,

    /// 每会话活跃 generation 集合（cancel 判定）。
    ///
    /// conv_id → set of generation。
    /// run 在每个检查点通过 `is_generation_active(conv_id, gen)` 查询自身 generation 是否仍在集合内。
    /// `cancel_chat_generation(conv_id)` 清空该会话整个集合，使所有 run 在下个检查点判失效。
    pub chat_active_generations: Mutex<HashMap<String, HashSet<u64>>>,

    /// 每会话的 AgentState（Idle/Running/...）。
    pub session_states: Mutex<HashMap<String, AgentState>>,

    /// 每会话的 pending approval id 集合（per-session + badge 路由，design.md D8）。
    ///
    /// conv_id → set of approval_id。
    /// 实际 oneshot::Sender 仍在 `TauriHost.approvals`，这里只存 id 用于：
    /// 1. 前端 badge 显示（"这个 session 有 pending approval"）
    /// 2. `take_pending_approval` 时从集合移除
    pub pending_approvals: Mutex<HashMap<String, HashSet<String>>>,

    /// 每会话的 pending ask_user id 集合（同 pending_approvals）。
    pub pending_ask_users: Mutex<HashMap<String, HashSet<String>>>,

    /// generation 单调递增计数器（全局，简化实现；Kivio 是 per-conv）。
    generation_counter: Mutex<u64>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            chat_active_replies: Mutex::new(HashMap::new()),
            chat_active_generations: Mutex::new(HashMap::new()),
            session_states: Mutex::new(HashMap::new()),
            pending_approvals: Mutex::new(HashMap::new()),
            pending_ask_users: Mutex::new(HashMap::new()),
            generation_counter: Mutex::new(0),
        }
    }

    // -----------------------------------------------------------------------
    // busy 守门（ChatSendReservation 用）
    // -----------------------------------------------------------------------

    /// 尝试预留某会话的发送哨兵。
    ///
    /// 原子地「busy 检查 + 占一个哨兵槽位」，关闭 busy 判定与真实 per-run 槽位注册之间的
    /// TOCTOU 窗口（防同会话并发发送同时通过 busy 检查）。
    ///
    /// 返回 `false` 表示该会话已有 run 在跑（busy）。
    /// 照搬 Kivio `state.rs:433-444` 的 `try_reserve_chat_send`。
    pub fn try_reserve_chat_send(&self, conv_id: &str, run_id: &str) -> bool {
        let mut active = self
            .chat_active_replies
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let runs = active.entry(conv_id.to_string()).or_default();
        if !runs.is_empty() {
            return false;
        }
        runs.insert(run_id.to_string());
        true
    }

    /// 释放某会话的 run 槽位（run 结束或命令退出时调）。
    ///
    /// 照搬 Kivio `state.rs:457-468` 的 `end_chat_reply`。
    pub fn end_chat_reply(&self, conv_id: &str, run_id: &str) {
        let mut active = self
            .chat_active_replies
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(runs) = active.get_mut(conv_id) {
            runs.remove(run_id);
            if runs.is_empty() {
                active.remove(conv_id);
            }
        }
    }

    /// 检查某会话是否 busy（有活跃 run）。
    pub fn is_session_busy(&self, conv_id: &str) -> bool {
        self.chat_active_replies
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(conv_id)
            .map(|runs| !runs.is_empty())
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // generation 管理（cancel 用）
    // -----------------------------------------------------------------------

    /// 为某会话分配新的 generation 编号并注册到活跃集合。
    ///
    /// 返回 generation 编号。run 在每个检查点用 `is_generation_active(conv_id, gen)` 查询。
    pub fn new_run_generation(&self, conv_id: &str) -> u64 {
        // 全局递增（简化实现，Kivio 是 per-conv）
        let gen_val = {
            let mut counter = self.generation_counter.lock().unwrap();
            *counter += 1;
            *counter
        };
        let mut gens = self
            .chat_active_generations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        gens.entry(conv_id.to_string()).or_default().insert(gen_val);
        gen_val
    }

    /// 检查某会话的某 generation 是否仍活跃（cancel 检查点）。
    ///
    /// run 在每个 round / 工具调用前调这个。返回 `false` 表示已被 cancel，应停止。
    pub fn is_generation_active(&self, conv_id: &str, generation: u64) -> bool {
        self.chat_active_generations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(conv_id)
            .map(|gens| gens.contains(&generation))
            .unwrap_or(false)
    }

    /// 取消指定会话的所有当前 run：清空其活跃 generation 集合。
    ///
    /// 使任何持旧 generation 的 run 在下一个检查点判失效。
    /// 不删除 `chat_active_replies` 槽位（busy 由命令自然 drop 释放）。
    /// 照搬 Kivio `state.rs:343-354` 的 `cancel_chat_generation`。
    pub fn cancel_chat_generation(&self, conv_id: &str) {
        if let Some(active) = self
            .chat_active_generations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(conv_id)
        {
            active.clear();
        }
    }

    /// run 自然结束时移除单条 generation（区别于 cancel 的清空全部）。
    pub fn end_generation(&self, conv_id: &str, generation: u64) {
        let mut should_remove = false;
        {
            let mut gens = self
                .chat_active_generations
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(active) = gens.get_mut(conv_id) {
                active.remove(&generation);
                should_remove = active.is_empty();
            }
            if should_remove {
                gens.remove(conv_id);
            }
        }
    }

    // -----------------------------------------------------------------------
    // per-session AgentState
    // -----------------------------------------------------------------------

    pub fn set_session_state(&self, conv_id: &str, state: AgentState) {
        self.session_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(conv_id.to_string(), state);
    }

    pub fn get_session_state(&self, conv_id: &str) -> AgentState {
        self.session_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(conv_id)
            .copied()
            .unwrap_or(AgentState::Idle)
    }

    /// 列出所有非 Idle 状态的会话（前端启动时同步用，诊断"卡住"的会话）。
    ///
    /// 返回 (conv_id, state) 列表。前端可据此同步 agentStore，或对僵尸状态强制重置。
    pub fn list_non_idle_sessions(&self) -> Vec<(String, AgentState)> {
        self.session_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, s)| **s != AgentState::Idle)
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// 强制重置某会话为 Idle：清空活跃 generation + replies + 状态。
    ///
    /// 用于解除"僵尸"状态（前端卡在 Running 但后端 run 已不存在）。
    pub fn force_reset_session(&self, conv_id: &str) {
        if let Some(gens) = self
            .chat_active_generations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(conv_id)
        {
            gens.clear();
        }
        if let Some(runs) = self
            .chat_active_replies
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(conv_id)
        {
            runs.clear();
        }
        self.session_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(conv_id.to_string(), AgentState::Idle);
    }

    // -----------------------------------------------------------------------
    // pending approvals / ask_users（per-session + badge 路由，D8）
    // -----------------------------------------------------------------------

    /// 注册某会话有 pending approval（TauriHost::request_tool_approval 时调）。
    pub fn insert_pending_approval(&self, conv_id: &str, approval_id: &str) {
        self.pending_approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(conv_id.to_string())
            .or_default()
            .insert(approval_id.to_string());
    }

    /// 取走某会话的 pending approval（approve_tool 命令调）。
    ///
    /// 返回 `true` 表示存在并已移除；`false` 表示不存在（可能超时/重复）。
    pub fn take_pending_approval(&self, conv_id: &str, approval_id: &str) -> bool {
        let mut should_remove = false;
        let existed;
        {
            let mut pending = self
                .pending_approvals
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(set) = pending.get_mut(conv_id) {
                existed = set.remove(approval_id);
                should_remove = set.is_empty();
            } else {
                existed = false;
            }
            if should_remove {
                pending.remove(conv_id);
            }
        }
        existed
    }

    /// 检查某会话是否有 pending approval（前端 badge 用）。
    pub fn has_pending_approval(&self, conv_id: &str) -> bool {
        self.pending_approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(conv_id)
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// 注册某会话有 pending ask_user。
    pub fn insert_pending_ask_user(&self, conv_id: &str, ask_user_id: &str) {
        self.pending_ask_users
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(conv_id.to_string())
            .or_default()
            .insert(ask_user_id.to_string());
    }

    /// 取走某会话的 pending ask_user。
    pub fn take_pending_ask_user(&self, conv_id: &str, ask_user_id: &str) -> bool {
        let mut should_remove = false;
        let existed;
        {
            let mut pending = self
                .pending_ask_users
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(set) = pending.get_mut(conv_id) {
                existed = set.remove(ask_user_id);
                should_remove = set.is_empty();
            } else {
                existed = false;
            }
            if should_remove {
                pending.remove(conv_id);
            }
        }
        existed
    }

    /// 检查某会话是否有 pending ask_user。
    pub fn has_pending_ask_user(&self, conv_id: &str) -> bool {
        self.pending_ask_users
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(conv_id)
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// 删除会话时彻底清除该会话所有运行态（cancel + pending）。
    ///
    /// 照搬 Kivio `forget_chat_conversation_runtime`。
    pub fn forget_session(&self, conv_id: &str) {
        self.chat_active_replies
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(conv_id);
        self.chat_active_generations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(conv_id);
        self.session_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(conv_id);
        self.pending_approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(conv_id);
        self.pending_ask_users
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(conv_id);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// 命令入口的哨兵预留守卫：原子地「busy 检查 + 占一个哨兵槽位」。
///
/// 照搬 Kivio `commands.rs:99-128` 的 `ChatSendReservation`。
/// 关闭 busy 判定与真实 per-run 槽位注册之间的 TOCTOU 窗口。
/// 哨兵槽位只占 `chat_active_replies`、不参与 generation/取消，
/// 命令任意退出路径 drop 时释放。
///
/// Phase 3.2：改为 own `Arc<AppState>`（不再 borrow），以便 move 进 `tokio::spawn`。
/// 用法：
/// ```ignore
/// let reservation = match ChatSendReservation::try_acquire(app_state.clone(), &conv_id) {
///     Some(r) => r,
///     None => return Ok(json!({"success": false, "error": "busy"})),
/// };
/// // ... spawn run_agent_loop (move reservation into task) ...
/// // reservation drop 时自动释放
/// ```
pub struct ChatSendReservation {
    state: Arc<AppState>,
    conversation_id: String,
    run_id: String,
}

impl ChatSendReservation {
    /// 尝试预留某会话的发送哨兵。返回 `None` 表示该会话已有 run 在跑（busy）。
    pub fn try_acquire(state: Arc<AppState>, conv_id: &str) -> Option<Self> {
        let run_id = format!("reservation-{}", uuid::Uuid::new_v4());
        if !state.try_reserve_chat_send(conv_id, &run_id) {
            return None;
        }
        Some(Self {
            state,
            conversation_id: conv_id.to_string(),
            run_id,
        })
    }

    /// 获取 reservation 的 run_id（测试用）。
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
}

impl Drop for ChatSendReservation {
    fn drop(&mut self) {
        self.state
            .end_chat_reply(&self.conversation_id, &self.run_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_reserve_chat_send_succeeds_on_idle_session() {
        let state = AppState::new();
        assert!(state.try_reserve_chat_send("conv_a", "run_1"));
        assert!(state.is_session_busy("conv_a"));
    }

    #[test]
    fn try_reserve_chat_send_fails_on_busy_session() {
        let state = AppState::new();
        assert!(state.try_reserve_chat_send("conv_a", "run_1"));
        // 同会话第二个 reserve 失败
        assert!(!state.try_reserve_chat_send("conv_a", "run_2"));
    }

    #[test]
    fn try_reserve_chat_send_independent_across_sessions() {
        let state = AppState::new();
        assert!(state.try_reserve_chat_send("conv_a", "run_1"));
        // 会话 A busy 不影响 B
        assert!(state.try_reserve_chat_send("conv_b", "run_2"));
    }

    #[test]
    fn end_chat_reply_releases_slot() {
        let state = AppState::new();
        state.try_reserve_chat_send("conv_a", "run_1");
        assert!(state.is_session_busy("conv_a"));

        state.end_chat_reply("conv_a", "run_1");
        assert!(!state.is_session_busy("conv_a"));

        // 释放后可以再次 reserve
        assert!(state.try_reserve_chat_send("conv_a", "run_2"));
    }

    #[test]
    fn cancel_chat_generation_clears_all_generations_for_session() {
        let state = AppState::new();
        let gen1 = state.new_run_generation("conv_a");
        let gen2 = state.new_run_generation("conv_a"); // 注意：Phase 3.2 每会话同时只一个 run，但 generation 是历史记录

        assert!(state.is_generation_active("conv_a", gen1));
        assert!(state.is_generation_active("conv_a", gen2));

        state.cancel_chat_generation("conv_a");

        assert!(!state.is_generation_active("conv_a", gen1));
        assert!(!state.is_generation_active("conv_a", gen2));
    }

    #[test]
    fn cancel_chat_generation_is_per_conversation() {
        let state = AppState::new();
        let gen_a = state.new_run_generation("conv_a");
        let gen_b = state.new_run_generation("conv_b");

        state.cancel_chat_generation("conv_a");

        assert!(!state.is_generation_active("conv_a", gen_a));
        // cancel A 不影响 B
        assert!(state.is_generation_active("conv_b", gen_b));
    }

    #[test]
    fn new_run_generation_increments() {
        let state = AppState::new();
        let gen1 = state.new_run_generation("conv_a");
        let gen2 = state.new_run_generation("conv_a");
        let gen3 = state.new_run_generation("conv_b");
        assert!(gen2 > gen1);
        assert!(gen3 > gen2);
    }

    #[test]
    fn is_generation_active_returns_false_after_cancel() {
        let state = AppState::new();
        let gen_val = state.new_run_generation("conv_a");
        assert!(state.is_generation_active("conv_a", gen_val));

        state.cancel_chat_generation("conv_a");
        assert!(!state.is_generation_active("conv_a", gen_val));
    }

    #[test]
    fn end_generation_removes_single_generation() {
        let state = AppState::new();
        let gen1 = state.new_run_generation("conv_a");
        let gen2 = state.new_run_generation("conv_a");

        state.end_generation("conv_a", gen1);
        // gen1 已结束，gen2 仍活跃
        assert!(!state.is_generation_active("conv_a", gen1));
        assert!(state.is_generation_active("conv_a", gen2));
    }

    #[test]
    fn set_get_session_state_round_trip() {
        let state = AppState::new();
        assert_eq!(state.get_session_state("conv_a"), AgentState::Idle);

        state.set_session_state("conv_a", AgentState::ToolLoop);
        assert_eq!(state.get_session_state("conv_a"), AgentState::ToolLoop);

        state.set_session_state("conv_a", AgentState::Idle);
        assert_eq!(state.get_session_state("conv_a"), AgentState::Idle);
    }

    #[test]
    fn pending_approval_insert_and_take() {
        let state = AppState::new();
        assert!(!state.has_pending_approval("conv_a"));

        state.insert_pending_approval("conv_a", "ap_1");
        assert!(state.has_pending_approval("conv_a"));

        state.insert_pending_approval("conv_a", "ap_2");
        assert!(state.has_pending_approval("conv_a"));

        // take 移除单个
        assert!(state.take_pending_approval("conv_a", "ap_1"));
        assert!(state.has_pending_approval("conv_a")); // ap_2 还在

        assert!(state.take_pending_approval("conv_a", "ap_2"));
        assert!(!state.has_pending_approval("conv_a")); // 全部移除

        // 重复 take 返回 false
        assert!(!state.take_pending_approval("conv_a", "ap_1"));
    }

    #[test]
    fn pending_ask_user_insert_and_take() {
        let state = AppState::new();
        assert!(!state.has_pending_ask_user("conv_a"));

        state.insert_pending_ask_user("conv_a", "ask_1");
        assert!(state.has_pending_ask_user("conv_a"));

        assert!(state.take_pending_ask_user("conv_a", "ask_1"));
        assert!(!state.has_pending_ask_user("conv_a"));
    }

    #[test]
    fn forget_session_clears_all_runtime_state() {
        let state = AppState::new();
        state.try_reserve_chat_send("conv_a", "run_1");
        state.new_run_generation("conv_a");
        state.set_session_state("conv_a", AgentState::ToolLoop);
        state.insert_pending_approval("conv_a", "ap_1");
        state.insert_pending_ask_user("conv_a", "ask_1");

        state.forget_session("conv_a");

        assert!(!state.is_session_busy("conv_a"));
        assert!(!state.is_generation_active("conv_a", 1));
        assert_eq!(state.get_session_state("conv_a"), AgentState::Idle);
        assert!(!state.has_pending_approval("conv_a"));
        assert!(!state.has_pending_ask_user("conv_a"));
    }

    #[test]
    fn chat_send_reservation_releases_on_drop() {
        let state = Arc::new(AppState::new());
        {
            let r = ChatSendReservation::try_acquire(state.clone(), "conv_a");
            assert!(r.is_some());
            assert!(state.is_session_busy("conv_a"));
            // drop here
        }
        assert!(!state.is_session_busy("conv_a"));
    }

    #[test]
    fn chat_send_reservation_returns_none_when_busy() {
        let state = Arc::new(AppState::new());
        let r1 = ChatSendReservation::try_acquire(state.clone(), "conv_a");
        assert!(r1.is_some());

        // 第二次应该失败
        let r2 = ChatSendReservation::try_acquire(state.clone(), "conv_a");
        assert!(r2.is_none());

        drop(r1);
        // 释放后可以再次 acquire
        let r3 = ChatSendReservation::try_acquire(state.clone(), "conv_a");
        assert!(r3.is_some());
    }
}
